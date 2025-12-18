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

use std::fs;
use std::path::Path;

use crate::ir::NormalizedPackage;

mod builtins;
mod calls;
mod idents;
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

    let mut mod_rs = String::new();
    mod_rs.push_str("/// Package address (the on-chain package object id).\n");
    mod_rs.push_str(&format!(
        "pub const PACKAGE: sui_move::prelude::Address = sui_move::prelude::Address::from_static(\"{}\");\n",
        pkg.storage_id
    ));

    for module in pkg.modules.values() {
        let tokens = util::render_module_file(module, pkg, opts);
        let code = util::prettify(tokens);
        let filename = format!("{}.rs", module.name);
        fs::write(out_dir.join(filename), code)?;

        let mod_ident = idents::ident(&module.name);
        mod_rs.push_str(&format!("pub mod {mod_ident};\n"));
        for dt in &module.datatypes {
            let ty_ident = idents::ident(&dt.name);
            mod_rs.push_str(&format!("pub use {mod_ident}::{ty_ident};\n"));
        }
    }

    fs::write(out_dir.join("mod.rs"), util::insert_item_spacing(&mod_rs))?;
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
}
