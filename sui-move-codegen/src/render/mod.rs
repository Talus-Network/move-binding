//! Render [`crate::ir::NormalizedPackage`] into Rust source code.
//!
//! Rendering is deterministic and does not require network access. The idea is:
//! - fetch and normalize metadata once (network) into [`crate::ir::NormalizedPackage`],
//! - commit that IR as JSON (optional),
//! - render Rust source from the IR in CI/builds (offline).
//!
//! The generated code is designed to plug into the rest of the workspace:
//! - generated types implement `sui-move` traits (`MoveType` / `MoveStruct`) and ability markers
//! - generated call targets identify Move functions for PTB builders
//! - generated call-spec builders can return `sui-move-call::CallSpec`
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
    /// Emit typed `CallSpec` builders in addition to generated `*_target` functions.
    ///
    /// Set this to `false` for consumers that compose PTBs from generated targets and explicit
    /// `sui_sdk_types::Argument`s.
    pub emit_call_specs: bool,
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
    /// If `true`, re-export generated datatypes from the package root.
    pub emit_reexports: bool,
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
            emit_call_specs: true,
            emit_tx_ext: false,
            flatten: false,
            use_aliases: true,
            emit_reexports: true,
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

/// Rendered package split into package-root code and one generated module block per Move module.
///
/// This is useful for build scripts that need to choose their own `include!` layout without
/// parsing rendered Rust source text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenderedPackageParts {
    /// Package-root code: package constants, package scoping helpers, optional `TxExt`, and
    /// optional root re-exports.
    pub root: String,
    /// `pub mod ... { ... }` blocks keyed by Move module name.
    pub modules: BTreeMap<String, String>,
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
/// assert!(code.contains("pub const CALL_PACKAGE"));
/// ```
pub fn render_package(pkg: &NormalizedPackage, opts: &RenderOptions) -> String {
    let tokens = util::render_package_tokens(pkg, opts);
    util::prettify(tokens)
}

/// Render a normalized package into package-root source and per-module `pub mod ...` blocks.
///
/// Unlike [`render_package_split`], this does not write files or assume `mod.rs` plus sibling
/// module files. Callers decide how to include or store the returned strings.
pub fn render_package_parts(pkg: &NormalizedPackage, opts: &RenderOptions) -> RenderedPackageParts {
    let mut parts_opts = opts.clone();
    parts_opts.flatten = false;

    let root = util::prettify(util::render_package_root_tokens(pkg, &parts_opts, false));
    let modules = pkg
        .modules
        .values()
        .map(|module| {
            let tokens = util::render_module(module, pkg, &parts_opts);
            (module.name.clone(), util::prettify(tokens))
        })
        .collect();

    RenderedPackageParts { root, modules }
}

/// Render a normalized package into multiple files (`mod.rs` + one file per Move module).
///
/// This is convenient if you want the generated code to mirror the Move module structure on disk.
/// The output directory will contain:
/// - `mod.rs` (with package scope helpers, `pub mod ...;`, and `pub use ...;` re-exports)
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
                    datatypes: vec![
                        Datatype {
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
                        },
                        Datatype {
                            type_name: TypeName::parse("0x1::m::Pair").unwrap(),
                            module: "m".into(),
                            name: "Pair".into(),
                            abilities: vec![Ability::Store, Ability::Copy, Ability::Drop],
                            type_parameters: vec![],
                            kind: DatatypeKind::Struct {
                                fields: vec![
                                    Field {
                                        name: "left".into(),
                                        position: 0,
                                        ty: TypeRef::U8,
                                    },
                                    Field {
                                        name: "right".into(),
                                        position: 1,
                                        ty: TypeRef::Bool,
                                    },
                                ],
                            },
                        },
                    ],
                    functions: vec![
                        Function {
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
                                            type_name: TypeName::parse(
                                                "0x2::tx_context::TxContext",
                                            )
                                            .unwrap(),
                                            type_arguments: vec![],
                                        }),
                                    },
                                },
                            ],
                            return_types: vec![],
                        },
                        Function {
                            name: "private_entry".into(),
                            visibility: Visibility::Private,
                            is_entry: true,
                            type_parameters: vec![],
                            parameters: vec![],
                            return_types: vec![],
                        },
                    ],
                },
            )]),
        }
    }

    fn generic_value_arg_pkg() -> NormalizedPackage {
        NormalizedPackage {
            storage_id: "0x1".into(),
            original_id: None,
            version: 0,
            modules: BTreeMap::from([(
                "m".into(),
                NormalizedModule {
                    name: "m".into(),
                    datatypes: vec![Datatype {
                        type_name: TypeName::parse("0x1::m::Cloneable").unwrap(),
                        module: "m".into(),
                        name: "Cloneable".into(),
                        abilities: vec![Ability::Store, Ability::Copy, Ability::Drop],
                        type_parameters: vec![TypeParameter {
                            constraints: vec![Ability::Store, Ability::Copy],
                            is_phantom: false,
                        }],
                        kind: DatatypeKind::Struct {
                            fields: vec![Field {
                                name: "value".into(),
                                position: 0,
                                ty: TypeRef::TypeParameter(0),
                            }],
                        },
                    }],
                    functions: vec![Function {
                        name: "use_cloneable".into(),
                        visibility: Visibility::Public,
                        is_entry: true,
                        type_parameters: vec![TypeParameter {
                            constraints: vec![Ability::Store, Ability::Copy],
                            is_phantom: false,
                        }],
                        parameters: vec![FunctionParam {
                            name: "arg0".into(),
                            ty: TypeRef::Datatype {
                                type_name: TypeName::parse("0x1::m::Cloneable").unwrap(),
                                type_arguments: vec![TypeRef::TypeParameter(0)],
                            },
                        }],
                        return_types: vec![],
                    }],
                },
            )]),
        }
    }

    fn named_parameter_pkg(names: &[&str]) -> NormalizedPackage {
        NormalizedPackage {
            storage_id: "0x1".into(),
            original_id: None,
            version: 0,
            modules: BTreeMap::from([(
                "m".into(),
                NormalizedModule {
                    name: "m".into(),
                    datatypes: vec![],
                    functions: vec![Function {
                        name: "named".into(),
                        visibility: Visibility::Public,
                        is_entry: true,
                        type_parameters: vec![],
                        parameters: names
                            .iter()
                            .map(|name| FunctionParam {
                                name: (*name).into(),
                                ty: TypeRef::U64,
                            })
                            .collect(),
                        return_types: vec![],
                    }],
                },
            )]),
        }
    }

    fn option_pkg() -> NormalizedPackage {
        NormalizedPackage {
            storage_id: "0x1".into(),
            original_id: None,
            version: 0,
            modules: BTreeMap::from([(
                "option".into(),
                NormalizedModule {
                    name: "option".into(),
                    datatypes: vec![Datatype {
                        type_name: TypeName::parse("0x1::option::Option").unwrap(),
                        module: "option".into(),
                        name: "Option".into(),
                        abilities: vec![Ability::Store, Ability::Copy, Ability::Drop],
                        type_parameters: vec![TypeParameter {
                            constraints: vec![],
                            is_phantom: false,
                        }],
                        kind: DatatypeKind::Struct {
                            fields: vec![Field {
                                name: "vec".into(),
                                position: 0,
                                ty: TypeRef::Vector(Box::new(TypeRef::TypeParameter(0))),
                            }],
                        },
                    }],
                    functions: vec![],
                },
            )]),
        }
    }

    fn layout_helper_pkg() -> NormalizedPackage {
        NormalizedPackage {
            storage_id: "0x2".into(),
            original_id: None,
            version: 0,
            modules: BTreeMap::from([
                (
                    "table_vec".into(),
                    NormalizedModule {
                        name: "table_vec".into(),
                        datatypes: vec![Datatype {
                            type_name: TypeName::parse("0x2::table_vec::TableVec").unwrap(),
                            module: "table_vec".into(),
                            name: "TableVec".into(),
                            abilities: vec![Ability::Store],
                            type_parameters: vec![TypeParameter {
                                constraints: vec![Ability::Store],
                                is_phantom: true,
                            }],
                            kind: DatatypeKind::Struct {
                                fields: vec![Field {
                                    name: "contents".into(),
                                    position: 0,
                                    ty: TypeRef::Datatype {
                                        type_name: TypeName::parse("0x2::table::Table").unwrap(),
                                        type_arguments: vec![
                                            TypeRef::U64,
                                            TypeRef::TypeParameter(0),
                                        ],
                                    },
                                }],
                            },
                        }],
                        functions: vec![],
                    },
                ),
                (
                    "vec_map".into(),
                    NormalizedModule {
                        name: "vec_map".into(),
                        datatypes: vec![
                            Datatype {
                                type_name: TypeName::parse("0x2::vec_map::Entry").unwrap(),
                                module: "vec_map".into(),
                                name: "Entry".into(),
                                abilities: vec![Ability::Store, Ability::Copy, Ability::Drop],
                                type_parameters: vec![
                                    TypeParameter {
                                        constraints: vec![Ability::Copy],
                                        is_phantom: false,
                                    },
                                    TypeParameter {
                                        constraints: vec![],
                                        is_phantom: false,
                                    },
                                ],
                                kind: DatatypeKind::Struct {
                                    fields: vec![
                                        Field {
                                            name: "key".into(),
                                            position: 0,
                                            ty: TypeRef::TypeParameter(0),
                                        },
                                        Field {
                                            name: "value".into(),
                                            position: 1,
                                            ty: TypeRef::TypeParameter(1),
                                        },
                                    ],
                                },
                            },
                            Datatype {
                                type_name: TypeName::parse("0x2::vec_map::VecMap").unwrap(),
                                module: "vec_map".into(),
                                name: "VecMap".into(),
                                abilities: vec![Ability::Store, Ability::Copy, Ability::Drop],
                                type_parameters: vec![
                                    TypeParameter {
                                        constraints: vec![Ability::Copy],
                                        is_phantom: false,
                                    },
                                    TypeParameter {
                                        constraints: vec![],
                                        is_phantom: false,
                                    },
                                ],
                                kind: DatatypeKind::Struct {
                                    fields: vec![Field {
                                        name: "contents".into(),
                                        position: 0,
                                        ty: TypeRef::Vector(Box::new(TypeRef::Datatype {
                                            type_name: TypeName::parse("0x2::vec_map::Entry")
                                                .unwrap(),
                                            type_arguments: vec![
                                                TypeRef::TypeParameter(0),
                                                TypeRef::TypeParameter(1),
                                            ],
                                        })),
                                    }],
                                },
                            },
                        ],
                        functions: vec![],
                    },
                ),
            ]),
        }
    }

    fn rust_copy_shape_pkg() -> NormalizedPackage {
        NormalizedPackage {
            storage_id: "0x1".into(),
            original_id: None,
            version: 0,
            modules: BTreeMap::from([(
                "m".into(),
                NormalizedModule {
                    name: "m".into(),
                    datatypes: vec![
                        Datatype {
                            type_name: TypeName::parse("0x1::m::Scalar").unwrap(),
                            module: "m".into(),
                            name: "Scalar".into(),
                            abilities: vec![Ability::Store, Ability::Copy, Ability::Drop],
                            type_parameters: vec![],
                            kind: DatatypeKind::Struct {
                                fields: vec![Field {
                                    name: "value".into(),
                                    position: 0,
                                    ty: TypeRef::U64,
                                }],
                            },
                        },
                        Datatype {
                            type_name: TypeName::parse("0x1::m::Nested").unwrap(),
                            module: "m".into(),
                            name: "Nested".into(),
                            abilities: vec![Ability::Store, Ability::Copy, Ability::Drop],
                            type_parameters: vec![],
                            kind: DatatypeKind::Struct {
                                fields: vec![Field {
                                    name: "scalar".into(),
                                    position: 0,
                                    ty: TypeRef::Datatype {
                                        type_name: TypeName::parse("0x1::m::Scalar").unwrap(),
                                        type_arguments: vec![],
                                    },
                                }],
                            },
                        },
                        Datatype {
                            type_name: TypeName::parse("0x1::m::Bytes").unwrap(),
                            module: "m".into(),
                            name: "Bytes".into(),
                            abilities: vec![Ability::Store, Ability::Copy, Ability::Drop],
                            type_parameters: vec![],
                            kind: DatatypeKind::Struct {
                                fields: vec![Field {
                                    name: "value".into(),
                                    position: 0,
                                    ty: TypeRef::Vector(Box::new(TypeRef::U8)),
                                }],
                            },
                        },
                        Datatype {
                            type_name: TypeName::parse("0x1::m::Generic").unwrap(),
                            module: "m".into(),
                            name: "Generic".into(),
                            abilities: vec![Ability::Store, Ability::Copy, Ability::Drop],
                            type_parameters: vec![TypeParameter {
                                constraints: vec![Ability::Store, Ability::Copy],
                                is_phantom: false,
                            }],
                            kind: DatatypeKind::Struct {
                                fields: vec![Field {
                                    name: "value".into(),
                                    position: 0,
                                    ty: TypeRef::TypeParameter(0),
                                }],
                            },
                        },
                        Datatype {
                            type_name: TypeName::parse("0x1::m::ScalarChoice").unwrap(),
                            module: "m".into(),
                            name: "ScalarChoice".into(),
                            abilities: vec![Ability::Store, Ability::Copy, Ability::Drop],
                            type_parameters: vec![],
                            kind: DatatypeKind::Enum {
                                variants: vec![
                                    Variant {
                                        name: "None".into(),
                                        position: 0,
                                        fields: vec![],
                                    },
                                    Variant {
                                        name: "Some".into(),
                                        position: 1,
                                        fields: vec![Field {
                                            name: "value".into(),
                                            position: 0,
                                            ty: TypeRef::Datatype {
                                                type_name: TypeName::parse("0x1::m::Nested")
                                                    .unwrap(),
                                                type_arguments: vec![],
                                            },
                                        }],
                                    },
                                ],
                            },
                        },
                    ],
                    functions: vec![],
                },
            )]),
        }
    }

    fn item_prefix<'a>(code: &'a str, item: &str) -> &'a str {
        let end = code.find(item).expect("rendered item");
        let start = code[..end].rfind("///Move type:").unwrap_or(0);
        &code[start..end]
    }

    #[test]
    fn renders_mutable_object_params_with_push_arg_mut() {
        let code = render_package(&demo_pkg(), &RenderOptions::default());
        assert!(code.contains("push_arg_mut(arg0)"));
    }

    #[test]
    fn generated_bindings_use_scoped_package_for_calls_and_type_tags() {
        let code = render_package(&demo_pkg(), &RenderOptions::default());

        assert!(code.contains("thread_local!"));
        assert!(code.contains("pub fn call_package() -> sui_move::prelude::Address"));
        assert!(code.contains("pub fn type_package() -> sui_move::prelude::Address"));
        assert!(code.contains("pub fn with_packages<R>"));
        assert!(!code.contains("pub fn package()"));
        assert!(!code.contains("pub fn with_package<R>"));
        assert!(code.contains("CallTarget::new(call_package(), \"m\", \"mutate\")"));
        assert!(code.contains("address_fn = \"super::type_package\""));
    }

    #[test]
    fn generated_bindings_split_call_and_type_package_scopes() {
        let code = render_package(&demo_pkg(), &RenderOptions::default());

        assert!(code.contains("pub const CALL_PACKAGE"));
        assert!(code.contains("pub const TYPE_PACKAGE"));
        assert!(code.contains("pub fn call_package() -> sui_move::prelude::Address"));
        assert!(code.contains("pub fn type_package() -> sui_move::prelude::Address"));
        assert!(code.contains("pub fn with_packages<R>"));
        assert!(code.contains("CallTarget::new(call_package(), \"m\", \"mutate\")"));
        assert!(code.contains("address_fn = \"super::type_package\""));
    }

    #[test]
    fn generated_calls_include_entry_functions_even_when_not_public() {
        let code = render_package(&demo_pkg(), &RenderOptions::default());
        assert!(code.contains("pub fn private_entry"));
        assert!(code.contains("pub fn private_entry_target"));
    }

    #[test]
    fn generated_calls_return_result_instead_of_panicking() {
        let code = render_package(&demo_pkg(), &RenderOptions::default());
        assert!(code.contains("-> Result<sm_call::CallSpec"));
        assert!(!code.contains("expect(\"encode arg\")"));
    }

    #[test]
    fn generated_calls_use_move_parameter_names() {
        let package = named_parameter_pkg(&["amount", "type", "self", "_"]);
        let code = render_package(&package, &RenderOptions::default());

        assert!(code.contains("pub fn named("));
        assert!(code.contains("amount: u64,"));
        assert!(code.contains("r#type: u64,"));
        assert!(code.contains("self_: u64,"));
        assert!(code.contains("arg3: u64,"));
        assert!(code.contains("spec.push_arg(&amount)?;"));
        assert!(code.contains("spec.push_arg(&r#type)?;"));
        assert!(code.contains("spec.push_arg(&self_)?;"));
        assert!(code.contains("spec.push_arg(&arg3)?;"));
        assert!(code.contains("named(amount: u64, type: u64, self: u64, _: u64)"));
    }

    #[test]
    fn generated_parameter_names_remain_unique_after_rust_conversion() {
        let package = named_parameter_pkg(&["self", "self_", "arg1"]);
        let code = render_package(&package, &RenderOptions::default());

        assert!(code.contains("pub fn named("));
        assert!(code.contains("spec.push_arg(&self_)?;"));
        assert!(code.contains("spec.push_arg(&arg1)?;"));
        assert!(code.contains("spec.push_arg(&arg2)?;"));
    }

    #[test]
    fn generated_calls_can_emit_targets_without_call_specs() {
        let opts = RenderOptions {
            emit_call_specs: false,
            ..RenderOptions::default()
        };
        let code = render_package(&demo_pkg(), &opts);

        assert!(code.contains("pub fn mutate_target() -> Result<sm_call::CallTarget"));
        assert!(!code.contains("pub fn mutate("));
        assert!(!code.contains("Result<sm_call::CallSpec"));
        assert!(!code.contains("push_arg_mut(arg0)"));
    }

    #[test]
    fn generated_calls_include_bounds_required_by_argument_structs() {
        let code = render_package(&generic_value_arg_pkg(), &RenderOptions::default());

        assert!(code.contains("pub fn use_cloneable<T0>"));
        assert!(code.contains("T0: sm::MoveType + sm::HasCopy + sm::HasDrop + sm::HasStore"));
    }

    #[test]
    fn generated_structs_include_named_field_constructors() {
        let code = render_package(&demo_pkg(), &RenderOptions::default());

        assert!(code.contains("pub fn new(left: u8, right: bool) -> Self"));
        assert!(code.contains("left: left.into()"));
        assert!(code.contains("right: right.into()"));
    }

    #[test]
    fn generated_option_layout_helpers_do_not_require_move_type_bounds() {
        let code = render_package(&option_pkg(), &RenderOptions::default());

        assert!(code.contains("pub fn from_option(value: std::option::Option<T0>) -> Self"));
        assert!(code.contains("impl<T0> Default for Option<T0>"));
        assert!(code.contains("impl<T0> From<std::option::Option<T0>> for Option<T0>"));
        assert!(!code.contains("T0: sm::MoveType"));
    }

    #[test]
    fn generated_collection_layout_helpers_use_minimal_rust_bounds() {
        let code = render_package(&layout_helper_pkg(), &RenderOptions::default());

        assert!(code.contains("pub fn size_u64(&self) -> u64"));
        assert!(code.contains("pub fn into_hash_map(self) -> std::collections::HashMap<T0, T1>"));
        assert!(!code.contains("T0: sm::MoveType"));
        assert!(!code.contains("T1: sm::MoveType"));
        assert!(!code.contains("T0: sm::MoveType + sm::HasCopy"));
    }

    #[test]
    fn generated_rust_copy_tracks_rust_carrier_shape() {
        let code = render_package(&rust_copy_shape_pkg(), &RenderOptions::default());

        assert!(item_prefix(&code, "pub struct Scalar").contains("#[derive(::core::marker::Copy)]"));
        assert!(item_prefix(&code, "pub struct Nested").contains("#[derive(::core::marker::Copy)]"));
        assert!(!item_prefix(&code, "pub struct Bytes").contains("#[derive(::core::marker::Copy)]"));
        assert!(
            !item_prefix(&code, "pub struct Generic").contains("#[derive(::core::marker::Copy)]")
        );
        assert!(item_prefix(&code, "pub enum ScalarChoice").contains("::core::marker::Copy"));
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
    fn render_package_parts_returns_root_and_modules_without_string_parsing() {
        let opts = RenderOptions {
            emit_reexports: false,
            ..RenderOptions::default()
        };
        let parts = render_package_parts(&demo_pkg(), &opts);

        assert!(parts.root.contains("pub const CALL_PACKAGE"));
        assert!(parts.root.contains("pub fn with_packages<R>"));
        assert!(!parts.root.contains("pub mod m"));
        assert!(!parts.root.contains("pub use m::Obj"));

        let module = parts.modules.get("m").expect("module body");
        assert!(module.contains("pub mod m"));
        assert!(module.contains("use super::{call_package, type_package};"));
        assert!(module.contains("pub struct Obj"));
        assert!(module.contains("pub fn mutate"));
    }

    #[test]
    fn render_package_parts_can_emit_targets_without_call_specs() {
        let opts = RenderOptions {
            emit_call_specs: false,
            ..RenderOptions::default()
        };
        let parts = render_package_parts(&demo_pkg(), &opts);
        let module = parts.modules.get("m").expect("module body");

        assert!(module.contains("pub fn mutate_target() -> Result<sm_call::CallTarget"));
        assert!(!module.contains("pub fn mutate("));
        assert!(!module.contains("Result<sm_call::CallSpec"));
        assert!(!module.contains("push_arg_mut(arg0)"));
    }

    #[test]
    fn emit_reexports_false_suppresses_single_file_root_reexports() {
        let opts = RenderOptions {
            emit_reexports: false,
            ..RenderOptions::default()
        };
        let code = render_package(&demo_pkg(), &opts);

        assert!(!code.contains("pub use m::Obj"));
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
        assert!(code.contains("let spec = m::mutate"));
        assert!(code.contains("self.call(spec)"));
    }

    #[test]
    fn rendered_tx_ext_uses_move_parameter_names() {
        let package = named_parameter_pkg(&["amount", "type", "self"]);
        let opts = RenderOptions {
            emit_tx_ext: true,
            ..RenderOptions::default()
        };
        let code = render_package(&package, &opts);

        assert!(code.contains("fn m__named("));
        assert!(code.contains("amount: u64,"));
        assert!(code.contains("r#type: u64,"));
        assert!(code.contains("self_: u64,"));
        assert!(code.contains("let spec = m::named("));
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
