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

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use crate::ir::{Ability, NormalizedPackage, TypeName};

mod builtins;
mod calls;
mod idents;
mod tx_ext;
mod types;
mod util;

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
    /// Rust paths for Move datatypes defined outside the package currently being rendered.
    ///
    /// This is the explicit cross-package symbol table. The renderer still treats unknown external
    /// datatypes as compile errors, but entries in this map are rendered as generated Rust paths
    /// instead of being folded into the `sui-move` core.
    pub external_types: BTreeMap<TypeName, ExternalType>,
}

/// A generated Rust binding for a Move datatype outside the current package.
///
/// `rust_path` is the path to the generated Rust type without generic arguments, for example
/// `sui_framework::object::UID` or `nexus_primitives::data::Data`. `is_key` carries the Move
/// `key` ability so call generation can decide whether a parameter should be an object argument.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExternalType {
    /// Rust path to the generated type, without generic arguments.
    pub rust_path: String,
    /// Whether the external Move datatype has the `key` ability.
    pub is_key: bool,
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self {
            emit_types: true,
            emit_calls: true,
            emit_tx_ext: false,
            flatten: false,
            use_aliases: true,
            external_types: BTreeMap::new(),
        }
    }
}

impl RenderOptions {
    /// Return options with one externally generated Move datatype registered.
    ///
    /// This is useful when a dependency package is rendered in a custom layout and the caller wants
    /// to provide the exact Rust path for one datatype.
    #[must_use]
    pub fn with_external_type(
        mut self,
        type_name: TypeName,
        rust_path: impl Into<String>,
        is_key: bool,
    ) -> Self {
        self.add_external_type(type_name, rust_path, is_key);
        self
    }

    /// Register one externally generated Move datatype.
    ///
    /// The path should name the generated Rust type without generic arguments. Generic arguments
    /// are rendered from the Move type reference at the use site.
    pub fn add_external_type(
        &mut self,
        type_name: TypeName,
        rust_path: impl Into<String>,
        is_key: bool,
    ) {
        self.external_types.insert(
            type_name,
            ExternalType {
                rust_path: rust_path.into(),
                is_key,
            },
        );
    }

    /// Return options with every datatype from an external package registered.
    ///
    /// The `rust_root` should point at the generated package module or crate. Datatypes are mapped
    /// using the default generated layout: `<rust_root>::<move_module>::<move_type>`.
    #[must_use]
    pub fn with_external_package(
        mut self,
        pkg: &NormalizedPackage,
        rust_root: impl Into<String>,
    ) -> Self {
        self.add_external_package(pkg, rust_root);
        self
    }

    /// Register every datatype from an external package using the default generated layout.
    ///
    /// Entries are added for the datatype address reported by metadata plus the package storage id
    /// and original id, when present. That keeps generated paths stable across the address forms Sui
    /// metadata can expose for upgraded packages.
    pub fn add_external_package(&mut self, pkg: &NormalizedPackage, rust_root: impl Into<String>) {
        let rust_root = rust_root.into();

        for module in pkg.modules.values() {
            let module_ident = idents::ident(&module.name).to_string();
            for dt in &module.datatypes {
                let type_ident = idents::ident(&dt.name).to_string();
                let rust_path = format!("{rust_root}::{module_ident}::{type_ident}");
                let is_key = dt.abilities.contains(&Ability::Key);

                for type_name in external_type_names(pkg, dt) {
                    self.add_external_type(type_name, rust_path.clone(), is_key);
                }
            }
        }
    }
}

fn external_type_names(pkg: &NormalizedPackage, dt: &crate::ir::Datatype) -> Vec<TypeName> {
    let mut names = vec![dt.type_name.clone()];
    names.push(TypeName {
        address: pkg.storage_id.clone(),
        module: dt.module.clone(),
        name: dt.name.clone(),
    });
    if let Some(original_id) = &pkg.original_id {
        names.push(TypeName {
            address: original_id.clone(),
            module: dt.module.clone(),
            name: dt.name.clone(),
        });
    }
    names.sort();
    names.dedup();
    names
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
    let tokens = util::render_package_tokens(pkg, opts);
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
        let tokens = util::render_module_file(module, pkg, &split_opts);
        let code = util::prettify(tokens);
        let filename = format!("{}.rs", module.name);
        fs::write(out_dir.join(filename), code)?;
    }

    let mod_tokens = util::render_split_mod_rs_tokens(pkg, &split_opts);
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
    fn external_framework_types_are_not_mapped_to_sui_move_core() {
        let code = render_package(&demo_pkg(), &RenderOptions::default());
        assert!(!code.contains("sm::types::UID"));
        assert!(code.contains("unknown external type `0x2::object::UID`"));
    }

    #[test]
    fn registered_external_package_types_render_as_generated_paths() {
        let external = NormalizedPackage {
            storage_id: "0x9".into(),
            original_id: None,
            version: 0,
            modules: BTreeMap::from([(
                "dep".into(),
                NormalizedModule {
                    name: "dep".into(),
                    datatypes: vec![Datatype {
                        type_name: TypeName::parse("0x9::dep::External").unwrap(),
                        module: "dep".into(),
                        name: "External".into(),
                        abilities: vec![Ability::Key, Ability::Store],
                        type_parameters: vec![],
                        kind: DatatypeKind::Struct { fields: vec![] },
                    }],
                    functions: vec![],
                },
            )]),
        };

        let pkg = NormalizedPackage {
            storage_id: "0x1".into(),
            original_id: None,
            version: 0,
            modules: BTreeMap::from([(
                "m".into(),
                NormalizedModule {
                    name: "m".into(),
                    datatypes: vec![Datatype {
                        type_name: TypeName::parse("0x1::m::UsesExternal").unwrap(),
                        module: "m".into(),
                        name: "UsesExternal".into(),
                        abilities: vec![Ability::Store],
                        type_parameters: vec![],
                        kind: DatatypeKind::Struct {
                            fields: vec![Field {
                                name: "external".into(),
                                position: 0,
                                ty: TypeRef::Datatype {
                                    type_name: TypeName::parse("0x9::dep::External").unwrap(),
                                    type_arguments: vec![],
                                },
                            }],
                        },
                    }],
                    functions: vec![Function {
                        name: "touch".into(),
                        visibility: Visibility::Public,
                        is_entry: true,
                        type_parameters: vec![],
                        parameters: vec![FunctionParam {
                            name: "arg0".into(),
                            ty: TypeRef::Ref {
                                mutable: true,
                                inner: Box::new(TypeRef::Datatype {
                                    type_name: TypeName::parse("0x9::dep::External").unwrap(),
                                    type_arguments: vec![],
                                }),
                            },
                        }],
                        return_types: vec![],
                    }],
                },
            )]),
        };

        let opts = RenderOptions::default().with_external_package(&external, "dep_bindings");
        let code = render_package(&pkg, &opts);

        assert!(!code.contains("unknown external type"));
        assert!(code.contains("pub external: dep_bindings::dep::External"));
        assert!(code.contains("&mut impl sm_call::ObjectArg<dep_bindings::dep::External>"));
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
}
