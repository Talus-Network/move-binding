# sui-move-codegen

Generate typed Rust bindings for a Move package on Sui.

This crate solves one problem: **turn on-chain Move package metadata into Rust source code** that
fits the layered `sui-move*` stack.

- `sui-move`: Move-shaped types (`MoveType`, `MoveStruct`, abilities)
- `sui-move-call`: typed call specs (`CallSpec`) + PTB values (`PtbValue<T>`) + input traits
- `sui-move-ptb`: build a Sui programmable transaction (PTB) from `CallSpec`
- `sui-move-runtime`: cursor-driven runtime for the Read → Tx → Commit mental model + auto-updating
  object handles
- `sui-move-codegen` (this crate): generate the bindings (types + call builders)

## Problem

Interacting with Move from Rust is repetitive and easy to get subtly wrong:

- you must spell `(package, module, function)` identifiers correctly
- you must build correct `TypeTag`s for generic type arguments
- you must encode pure values with BCS and pass objects with the right input kind
- you want the *Move* signature reflected in the *Rust* function signature (especially for object
  inputs and `&mut`)

This crate removes the repetition by generating a small amount of Rust code from the chain’s own
metadata.

## How it works (deterministic pipeline)

The pipeline is intentionally split in two:

1. **Source (network)**: fetch package metadata once and normalize it into a serde-friendly IR
   (`NormalizedPackage`).
2. **Render (offline)**: render Rust source from that IR.

Because the IR is JSON-friendly, you can commit it and re-render deterministically in CI without
needing network access.

## What gets generated

Given a `NormalizedPackage` (either fetched from gRPC or loaded from JSON), this crate can render:

- A `pub const PACKAGE: Address` (the on-chain package id)
- One Rust module per Move module (or a flat layout via `RenderOptions::flatten`)
- Move datatypes as Rust types (structs use `#[sui_move::move_struct]` via `sui-move`’s `derive`
  feature)
- Move functions as Rust functions that return `sui_move_call::CallSpec<...>`
- (optional) A `TxExt` trait implemented for `sui_move_runtime::Tx` (enable with
  `RenderOptions::emit_tx_ext`)

Those generated call builders are designed to be used directly in higher layers:
- `sui-move-ptb` can consume `CallSpec` to build a `ProgrammableTransaction`
- `sui-move-runtime` can consume `CallSpec` via its tx builder (or `sui_move_runtime::tx!`)

## Example: render from an in-memory IR

```rust
use std::collections::BTreeMap;
use sui_move_codegen::ir::*;
use sui_move_codegen::render::{render_package, RenderOptions};

let pkg = NormalizedPackage {
    storage_id: "0x1".into(),
    original_id: None,
    version: 0,
    modules: BTreeMap::from([(
        "m".into(),
        NormalizedModule {
            name: "m".into(),
            datatypes: vec![Datatype {
                type_name: TypeName {
                    address: "0x1".into(),
                    module: "m".into(),
                    name: "S".into(),
                },
                module: "m".into(),
                name: "S".into(),
                abilities: vec![Ability::Store],
                type_parameters: vec![],
                kind: DatatypeKind::Struct {
                    fields: vec![Field {
                        name: "value".into(),
                        position: 0,
                        ty: TypeRef::U64,
                    }],
                },
            }],
            functions: vec![Function {
                name: "f".into(),
                visibility: Visibility::Public,
                is_entry: true,
                type_parameters: vec![],
                parameters: vec![FunctionParam {
                    name: "arg0".into(),
                    ty: TypeRef::U64,
                }],
                return_types: vec![],
            }],
        },
    )]),
};

let code = render_package(&pkg, &RenderOptions::default());
assert!(code.contains("pub const PACKAGE"));
assert!(code.contains("pub struct S"));
assert!(code.contains("pub fn f"));
```

## Example: object args + `TxContext` omission

Move functions often take objects by `&` / `&mut`, and also take a `&mut TxContext`. In the
generated Rust API:

- any parameter whose type has the Move `key` ability becomes `&impl ObjectArg<T>` (or `&mut ...`
  if the Move signature is `&mut`)
- for `&mut` object parameters, the generated builder uses `CallSpec::push_object_arg_mut` so shared
  objects are marked mutable in the transaction input when needed
- any `TxContext` parameter is omitted (higher layers supply it when building the transaction)

```rust
use std::collections::BTreeMap;
use sui_move_codegen::ir::*;
use sui_move_codegen::render::{render_package, RenderOptions};

let pkg = NormalizedPackage {
    storage_id: "0x1".into(),
    original_id: None,
    version: 0,
    modules: BTreeMap::from([(
        "m".into(),
        NormalizedModule {
            name: "m".into(),
            datatypes: vec![Datatype {
                type_name: TypeName {
                    address: "0x1".into(),
                    module: "m".into(),
                    name: "Obj".into(),
                },
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
                                type_name: TypeName::parse("0x2::tx_context::TxContext").unwrap(),
                                type_arguments: vec![],
                            }),
                        },
                    },
                ],
                return_types: vec![],
            }],
        },
    )]),
};

let code = render_package(&pkg, &RenderOptions::default());
let start = code.find("pub fn mutate").unwrap();
let sig_end = start + code[start..].find('{').unwrap();
let sig = &code[start..sig_end];

assert!(sig.contains("arg0: impl sm_call::IntoObjectArgMut<Obj>"));
assert!(!sig.contains("TxContext"));
assert!(code.contains("push_object_arg_mut(arg0)"));
```

## Optional: runtime `Tx` extension trait

If you want a slightly more ergonomic “append a call” API on top of `sui-move-runtime`, you can
ask codegen to emit a `TxExt` trait implemented for `sui_move_runtime::Tx`.

The generated methods do **not** submit the transaction; they only call `Tx::call(...)`. This
keeps the Read → Tx → Commit boundary explicit.

```rust
use std::collections::BTreeMap;
use sui_move_codegen::ir::*;
use sui_move_codegen::render::{render_package, RenderOptions};

let pkg = NormalizedPackage {
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
                        name: "ctx".into(),
                        ty: TypeRef::Ref {
                            mutable: true,
                            inner: Box::new(TypeRef::Datatype {
                                type_name: TypeName::parse("0x2::tx_context::TxContext").unwrap(),
                                type_arguments: vec![],
                            }),
                        },
                    },
                ],
                return_types: vec![],
            }],
        },
    )]),
};

let opts = RenderOptions {
    emit_tx_ext: true,
    ..RenderOptions::default()
};
let code = render_package(&pkg, &opts);
assert!(code.contains("pub trait TxExt"));
assert!(code.contains("fn m__mutate"));
```

## Example: generic type params and ability bounds

Move generic constraints become Rust `where` bounds using `sui-move`’s marker traits. For example,
`T: store` becomes `T0: MoveType + HasStore`.

```rust
use std::collections::BTreeMap;
use sui_move_codegen::ir::*;
use sui_move_codegen::render::{render_package, RenderOptions};

let pkg = NormalizedPackage {
    storage_id: "0x1".into(),
    original_id: None,
    version: 0,
    modules: BTreeMap::from([(
        "m".into(),
        NormalizedModule {
            name: "m".into(),
            datatypes: vec![],
            functions: vec![Function {
                name: "id".into(),
                visibility: Visibility::Public,
                is_entry: true,
                type_parameters: vec![TypeParameter {
                    constraints: vec![Ability::Store],
                    is_phantom: false,
                }],
                parameters: vec![FunctionParam {
                    name: "arg0".into(),
                    ty: TypeRef::Vector(Box::new(TypeRef::TypeParameter(0))),
                }],
                return_types: vec![],
            }],
        },
    )]),
};

let code = render_package(&pkg, &RenderOptions::default());
let start = code.find("pub fn id").unwrap();
let sig_end = start + code[start..].find('{').unwrap();
let sig = &code[start..sig_end];

assert!(sig.contains("pub fn id<T0>(arg0: impl sm_call::IntoMoveArg<Vec<T0>>)"));
assert!(sig.contains("where"));
assert!(sig.contains("T0: sm::MoveType + sm::HasStore"));
assert!(code.contains("spec.push_type_arg::<T0>();"));
```

## Example: fetch over gRPC

```rust,no_run
use sui_move_codegen::fetch_package;
use sui_rpc::Client;
use sui_sdk_types::Address;

# async fn demo() -> Result<(), Box<dyn std::error::Error>> {
let mut client = Client::new(Client::MAINNET_FULLNODE)?;
let package_id: Address = "0x2".parse()?;

let pkg = fetch_package(&mut client, package_id).await?;
let json = pkg.to_json_string()?;
println!("{json}");
# Ok(())
# }
```

## Recommended workflow

To keep builds deterministic, fetch metadata once and commit it (JSON), then render from JSON:

1. Fetch and save `NormalizedPackage` JSON (out-of-band; not in `build.rs`)
2. Render Rust bindings from that JSON during builds or as a pre-generation step

This avoids putting network access in CI/build scripts.

## Using the generated code

The rendered Rust is meant to live in its own crate or module. At minimum, the generated code
expects these crates in the consumer’s `Cargo.toml`:

```toml
[dependencies]
sui-move = { path = "../sui-move" }
sui-move-derive = { path = "../sui-move-derive" }
sui-move-call = { path = "../sui-move-call" }
```

If you want to execute calls, add higher layers (`sui-move-ptb`, `sui-move-runtime`) in the same
consumer crate.

`RenderOptions::use_aliases` only affects verbosity in the emitted source (it adds `use sui_move as
sm; use sui_move_call as sm_call;`). It does not change which crates are required.
