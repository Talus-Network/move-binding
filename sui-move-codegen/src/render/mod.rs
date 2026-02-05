//! Render [`crate::ir::NormalizedPackage`] into Rust source code.
//!
//! Rendering is deterministic and does not require network access. The idea is:
//! - fetch and normalize metadata once (network) into [`crate::ir::NormalizedPackage`],
//! - commit that IR as JSON (optional),
//! - render Rust source from the IR in CI/builds (offline).
//!
//! The generated code is designed to plug into the rest of the workspace:
//! - generated types implement `sui-move` traits (`MoveType` / `MoveStruct`) and ability markers
//! - generated functions return `sui-move-call::CallSpec`
//! - optionally, a `TxExt` trait is emitted to add calls directly to `sui-move-runtime::Tx`

use std::fs;
use std::path::Path;

use crate::ir::NormalizedPackage;

mod calls;
mod callable;
mod externals;
mod idents;
mod tx_ext;
mod types;
mod util;

pub use externals::ExternalResolver;

/// Options controlling what gets emitted.
///
/// These options only affect the *shape* of the rendered Rust source; they do not change the IR.
#[derive(Clone, Debug)]
pub struct RenderOptions {
    /// Emit datatype definitions (structs/enums).
    pub emit_types: bool,
    /// Emit call builder functions.
    pub emit_calls: bool,
    /// Emit runtime helpers for `sui-move-runtime` (`TxExt`).
    ///
    /// When enabled, generated code includes a `TxExt` trait implemented for
    /// `sui_move_runtime::Tx<'_, S>`. Each Move function becomes a convenience method like
    /// `module__function(...)` that appends a `MoveCall` command by calling `Tx::call(...)`.
    ///
    /// This is optional so consumers can use the generated bindings without depending on the
    /// runtime layer.
    pub emit_tx_ext: bool,
    /// Emit everything into a single flat module (no per-Move-module `mod` blocks).
    pub flatten: bool,
    /// If `true`, include small aliases to reduce verbosity in generated code.
    ///
    /// Concretely, this adds:
    /// - `use sui_move as sm;`
    /// - `use sui_move_call as sm_call;`
    pub use_aliases: bool,
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self {
            emit_types: true,
            emit_calls: true,
            emit_tx_ext: false,
            flatten: false,
            use_aliases: true,
        }
    }
}

/// Render a normalized package into a single Rust source string.
///
/// The output is valid Rust source that you can write to a `.rs` file (or `include!` as a module).
///
/// # Example
/// ```
/// use std::collections::BTreeMap;
/// use sui_move_codegen::ir::*;
/// use sui_move_codegen::render::{render_package, RenderOptions};
///
/// let pkg = NormalizedPackage {
///     storage_id: "0x1".into(),
///     original_id: None,
///     version: 0,
///     modules: BTreeMap::from([(
///         "m".into(),
///         NormalizedModule {
///             name: "m".into(),
///             datatypes: vec![],
///             functions: vec![],
///         },
///     )]),
/// };
///
/// let code = render_package(&pkg, &RenderOptions::default());
/// assert!(code.contains("pub const PACKAGE"));
/// ```
pub fn render_package(pkg: &NormalizedPackage, opts: &RenderOptions) -> String {
    let tokens = util::render_package_tokens(pkg, opts, None);
    util::prettify(tokens)
}

/// Render a normalized package into a single Rust source string, resolving external packages.
///
/// If an external type reference cannot be resolved via `resolver`, the generated code will
/// include a `compile_error!` (same behavior as [`render_package`]).
pub fn render_package_with_resolver(
    pkg: &NormalizedPackage,
    opts: &RenderOptions,
    resolver: &ExternalResolver,
) -> String {
    let tokens = util::render_package_tokens(pkg, opts, Some(resolver));
    util::prettify(tokens)
}

/// Render a normalized package into multiple files (`mod.rs` + one file per Move module).
///
/// This is convenient if you want the generated code to mirror the Move module structure on disk.
/// The output directory will contain:
/// - `mod.rs` (with `PACKAGE`, `pub mod ...;`, and `pub use ...;` re-exports)
/// - one `*.rs` file per Move module
pub fn render_package_split(
    pkg: &NormalizedPackage,
    opts: &RenderOptions,
    out_dir: impl AsRef<Path>,
) -> std::io::Result<()> {
    let out_dir = out_dir.as_ref();
    fs::create_dir_all(out_dir)?;

    // Split output is always “per-module”; `flatten` only applies to single-file rendering.
    let mut split_opts = opts.clone();
    split_opts.flatten = false;

    for module in pkg.modules.values() {
        let tokens = util::render_module_file(module, pkg, &split_opts, None);
        let code = util::prettify(tokens);
        let filename = format!("{}.rs", module.name);
        fs::write(out_dir.join(filename), code)?;
    }

    let mod_tokens = util::render_split_mod_rs_tokens(pkg, &split_opts, None);
    let mod_code = util::prettify(mod_tokens);
    fs::write(out_dir.join("mod.rs"), mod_code)?;
    Ok(())
}

/// Render a normalized package into multiple files, resolving external packages.
pub fn render_package_split_with_resolver(
    pkg: &NormalizedPackage,
    opts: &RenderOptions,
    resolver: &ExternalResolver,
    out_dir: impl AsRef<Path>,
) -> std::io::Result<()> {
    let out_dir = out_dir.as_ref();
    fs::create_dir_all(out_dir)?;

    let mut split_opts = opts.clone();
    split_opts.flatten = false;

    for module in pkg.modules.values() {
        let tokens = util::render_module_file(module, pkg, &split_opts, Some(resolver));
        let code = util::prettify(tokens);
        let filename = format!("{}.rs", module.name);
        fs::write(out_dir.join(filename), code)?;
    }

    let mod_tokens = util::render_split_mod_rs_tokens(pkg, &split_opts, Some(resolver));
    let mod_code = util::prettify(mod_tokens);
    fs::write(out_dir.join("mod.rs"), mod_code)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::ir::*;

    fn demo_pkg() -> NormalizedPackage {
        NormalizedPackage {
            storage_id: "0x1".into(),
            original_id: None,
            version: 0,
            modules: BTreeMap::from([(
                "m".into(),
                NormalizedModule {
                    name: "m".into(),
                    datatypes: vec![Datatype {
                        type_name: TypeName::parse("0x1::m::Obj").unwrap(),
                        module: "m".into(),
                        name: "Obj".into(),
                        abilities: vec![Ability::Key, Ability::Store],
                        type_parameters: vec![],
                        kind: DatatypeKind::Struct {
                            fields: vec![Field {
                                name: "id".into(),
                                position: 0,
                                ty: TypeRef::Datatype {
                                    type_name: TypeName::parse("0x2::object::UID").unwrap(),
                                    type_arguments: vec![],
                                },
                            }],
                        },
                    }],
                    functions: vec![Function {
                        name: "mutate".into(),
                        visibility: Visibility::Public,
                        is_entry: true,
                        type_parameters: vec![],
                        parameters: vec![
                            FunctionParam {
                                name: "arg0".into(),
                                ty: TypeRef::Ref {
                                    mutable: true,
                                    inner: Box::new(TypeRef::Datatype {
                                        type_name: TypeName::parse("0x1::m::Obj").unwrap(),
                                        type_arguments: vec![],
                                    }),
                                },
                            },
                            FunctionParam {
                                name: "arg1".into(),
                                ty: TypeRef::Ref {
                                    mutable: true,
                                    inner: Box::new(TypeRef::Datatype {
                                        type_name: TypeName::parse("0x2::tx_context::TxContext")
                                            .unwrap(),
                                        type_arguments: vec![],
                                    }),
                                },
                            },
                        ],
                        return_types: vec![],
                    }],
                },
            )]),
        }
    }

    #[test]
    fn renders_mutable_object_params_with_push_arg_mut() {
        let code = render_package(&demo_pkg(), &RenderOptions::default());
        assert!(code.contains("push_arg_mut(arg0)"));
    }

    #[test]
    fn renders_structs_with_sui_move_move_struct_attribute() {
        let opts = RenderOptions {
            use_aliases: false,
            ..RenderOptions::default()
        };
        let code = render_package(&demo_pkg(), &opts);
        assert!(code.contains("#[sui_move::move_struct"));
    }

    #[test]
    fn renders_tx_ext_trait_when_enabled() {
        let opts = RenderOptions {
            emit_tx_ext: true,
            ..RenderOptions::default()
        };
        let code = render_package(&demo_pkg(), &opts);
        assert!(code.contains("pub trait TxExt"));
        assert!(code.contains("fn m__mutate"));
        assert!(code.contains("impl<'a, S> TxExt for sui_move_runtime::Tx<'a, S>"));
        assert!(code.contains("self.call(m::mutate"));
    }

    #[test]
    fn split_output_includes_tx_ext_in_mod_rs() {
        let opts = RenderOptions {
            emit_tx_ext: true,
            ..RenderOptions::default()
        };

        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("sui-move-codegen-{unique}"));

        render_package_split(&demo_pkg(), &opts, &dir).unwrap();

        let mod_rs = std::fs::read_to_string(dir.join("mod.rs")).unwrap();
        assert!(mod_rs.contains("pub trait TxExt"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn renders_external_types_via_resolver_as_dep_crate_module_type() {
        let dep = NormalizedPackage {
            storage_id: "0xb".into(),
            original_id: None,
            version: 0,
            modules: BTreeMap::from([(
                "dep".into(),
                NormalizedModule {
                    name: "dep".into(),
                    datatypes: vec![Datatype {
                        type_name: TypeName::parse("0xb::dep::Obj").unwrap(),
                        module: "dep".into(),
                        name: "Obj".into(),
                        abilities: vec![Ability::Key, Ability::Store],
                        type_parameters: vec![],
                        kind: DatatypeKind::Struct { fields: vec![] },
                    }],
                    functions: vec![],
                },
            )]),
        };

        let root = NormalizedPackage {
            storage_id: "0xa".into(),
            original_id: None,
            version: 0,
            modules: BTreeMap::from([(
                "m".into(),
                NormalizedModule {
                    name: "m".into(),
                    datatypes: vec![Datatype {
                        type_name: TypeName::parse("0xa::m::UsesDep").unwrap(),
                        module: "m".into(),
                        name: "UsesDep".into(),
                        abilities: vec![Ability::Store],
                        type_parameters: vec![],
                        kind: DatatypeKind::Struct {
                            fields: vec![Field {
                                name: "obj".into(),
                                position: 0,
                                ty: TypeRef::Datatype {
                                    type_name: TypeName::parse("0xb::dep::Obj").unwrap(),
                                    type_arguments: vec![],
                                },
                            }],
                        },
                    }],
                    functions: vec![Function {
                        name: "take".into(),
                        visibility: Visibility::Public,
                        is_entry: true,
                        type_parameters: vec![],
                        parameters: vec![FunctionParam {
                            name: "arg0".into(),
                            ty: TypeRef::Ref {
                                mutable: true,
                                inner: Box::new(TypeRef::Datatype {
                                    type_name: TypeName::parse("0xb::dep::Obj").unwrap(),
                                    type_arguments: vec![],
                                }),
                            },
                        }],
                        return_types: vec![],
                    }],
                },
            )]),
        };

        let mut resolver = ExternalResolver::new();
        resolver.add_package(&dep, "dep-crate");

        let code = render_package_with_resolver(&root, &RenderOptions::default(), &resolver);

        assert!(code.contains("dep_crate::dep::Obj"));
        assert!(!code.contains("compile_error!"));
        assert!(code.contains("arg0: &mut impl sm_call::ObjectArg<dep_crate::dep::Obj>"));
    }

    #[test]
    fn external_key_ability_resolution_is_robust_to_upgrades() {
        let dep = NormalizedPackage {
            storage_id: "0xb01".into(),
            original_id: Some("0xb00".into()),
            version: 0,
            modules: BTreeMap::from([(
                "dep".into(),
                NormalizedModule {
                    name: "dep".into(),
                    datatypes: vec![Datatype {
                        // Simulate metadata that encodes the storage id.
                        type_name: TypeName::parse("0xb01::dep::Obj").unwrap(),
                        module: "dep".into(),
                        name: "Obj".into(),
                        abilities: vec![Ability::Key, Ability::Store],
                        type_parameters: vec![],
                        kind: DatatypeKind::Struct { fields: vec![] },
                    }],
                    functions: vec![],
                },
            )]),
        };

        let root = NormalizedPackage {
            storage_id: "0xa".into(),
            original_id: None,
            version: 0,
            modules: BTreeMap::from([(
                "m".into(),
                NormalizedModule {
                    name: "m".into(),
                    datatypes: vec![],
                    functions: vec![Function {
                        name: "take".into(),
                        visibility: Visibility::Public,
                        is_entry: true,
                        type_parameters: vec![],
                        // Simulate a type reference that uses the original id.
                        parameters: vec![FunctionParam {
                            name: "arg0".into(),
                            ty: TypeRef::Ref {
                                mutable: true,
                                inner: Box::new(TypeRef::Datatype {
                                    type_name: TypeName::parse("0xb00::dep::Obj").unwrap(),
                                    type_arguments: vec![],
                                }),
                            },
                        }],
                        return_types: vec![],
                    }],
                },
            )]),
        };

        let mut resolver = ExternalResolver::new();
        resolver.add_package(&dep, "dep-crate");

        let code = render_package_with_resolver(&root, &RenderOptions::default(), &resolver);

        // Key ability should still be detected, so the param becomes an object arg.
        assert!(code.contains("arg0: &mut impl sm_call::ObjectArg<dep_crate::dep::Obj>"));
    }
}
