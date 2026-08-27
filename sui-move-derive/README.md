# talus-sui-move-derive

Procedural macros for [`talus-sui-move`](https://docs.rs/talus-sui-move): define Rust types that
represent Move types with minimal boilerplate. The package is `talus-sui-move-derive`; Rust code
imports it as `talus_sui_move_derive`.

This crate exists to solve one problem: **turn a Rust struct into a representation of a Move type**
with the correct `TypeTag`, `StructTag`, and ability markers. It can then use the type tag plumbing
and verified decoding in `talus-sui-move`.

## Where it fits

In the repository’s layered
[stack](https://github.com/Talus-Network/move-binding/blob/main/MODEL.md),
`talus-sui-move-derive` is a convenience layer for the bottom type system (`talus-sui-move`):

- you describe the Move identity (`address`, `module`, `abilities`) as attributes,
- the macro generates the corresponding `talus_sui_move::MoveType` / `talus_sui_move::MoveStruct` impls and
  ability marker impls,
- higher layers (call/PTB/runtime) consume those traits for typed interactions.

## What you get

Given a struct like:

```rust,no_run
use talus_sui_move::prelude::*;
use talus_sui_move_derive::move_struct;

#[move_struct(address = "0x1", module = "demo", abilities = "copy, store")]
pub struct Point {
    pub x: u64,
    pub y: u64,
}

let tag = <Point as MoveType>::type_tag_static();
match tag {
    TypeTag::Struct(struct_tag) => {
        assert_eq!(struct_tag.module().to_string(), "demo");
        assert_eq!(struct_tag.name().to_string(), "Point");
    }
    other => panic!("expected struct type tag, got {other:?}"),
}
```

The macro generates:

- `impl talus_sui_move::MoveType` and `impl talus_sui_move::MoveStruct`
- Ability marker impls (`HasKey`, `HasStore`, `HasCopy`, `HasDrop`) based on `abilities = "..."`
- `serde` derives (without requiring your crate to depend on `serde` directly)
- Optional injected `PhantomData` fields for phantom type params
- Validation during compilation for common mistakes (e.g. `key` requires an `id: UID` field)

## Recommended usage

Most users should depend on `talus-sui-move` and enable its `derive` feature, which exports these macros:

```toml
[dependencies]
talus-sui-move = { version = "=0.2.0-rc.1", features = ["derive"] }
```

Then use:

```rust,ignore
use talus_sui_move::move_struct;
```

You can also depend on `talus-sui-move-derive` directly, but you must still depend on
`talus-sui-move` because the generated impls reference it.

## `#[move_module]`

`#[move_module]` currently leaves the module unchanged. It can annotate Rust `mod` blocks that
correspond to Move modules.

## `#[move_struct(...)]` reference

Required arguments:

- `address = "0x..."`: Move address
- `module = "..."`: Move module name

Optional arguments:

- `address_fn = "path::to::fn"`: Function returning the Move address to use for `StructTag`s.
  `address` remains the default/documented address.
- `name = "..."`: Override the Move struct name (defaults to the Rust struct name)
- `abilities = "key, store, copy, drop"`: Move abilities (separated by commas)
  - `copy` implies `drop`
  - `key` and `copy` are mutually exclusive
- `phantoms = "T, U"`: Mark type parameters as phantom and inject `PhantomData` fields
- `type_abilities = "T: store, copy; U: drop"`: Specify ability expectations for type parameters
- `uid_type = "path::to::UID"`: Override what type counts as `UID` when enforcing the `key` rule

## How bounds are enforced

The macro tries to make the “Move rules” visible as normal Rust type errors:

- Every type parameter gets a `T: talus_sui_move::MoveType` bound.
- If the struct has a Move ability (e.g. `store`), the macro adds the corresponding bounds to
  each field type that is not phantom (e.g. `field_ty: talus_sui_move::HasStore`).
- For generic fields like `Vec<T>`, this naturally pushes requirements onto `T` (e.g.
  `Vec<T>: HasStore` implies `T: HasStore`).

You can satisfy those requirements either by:

- writing normal Rust bounds (`struct Vault<T: talus_sui_move::HasStore> { ... }`), or
- using `type_abilities = "T: store"` to have the macro add the ability bounds for you.

## Examples

### `key` objects require an `id: UID` type defined by a package

```rust,no_run
use std::marker::PhantomData;
use talus_sui_move::prelude::Address;
use talus_sui_move_derive::move_struct;

/// Local package declaration for `0x2::object::ID`.
///
/// Framework types are ordinary Move declarations from the type kernel perspective. In production
/// this shape should come from generated package bindings rather than from `talus-sui-move` itself.
#[move_struct(address = "0x2", module = "object", abilities = "copy, drop, store")]
pub struct ID {
    pub bytes: Address,
}

/// Local package declaration for `0x2::object::UID`.
///
/// A `key` object is recognized by an `id` field whose type is a `UID` shape defined by a package.
#[move_struct(address = "0x2", module = "object", abilities = "store")]
pub struct UID {
    pub id: ID,
}

#[move_struct(
    address = "0x1",
    module = "vault",
    abilities = "key, store",
    phantoms = "T",
    type_abilities = "T: store"
)]
pub struct Vault<T> {
    pub id: UID,
    pub balance: Vec<T>,
}

let _tag = <Vault<u64> as talus_sui_move::MoveType>::type_tag_static();
let _value = Vault::<u64> {
    id: UID {
        id: ID {
            bytes: Address::new([0u8; 32]),
        },
    },
    balance: vec![1, 2, 3],
    phantom_t: PhantomData,
};
```

If you try to declare a `key` struct without an `id` field, it fails at compile time:

```rust,compile_fail
use talus_sui_move_derive::move_struct;

#[move_struct(address = "0x1", module = "broken", abilities = "key, store")]
pub struct MissingId {
    pub value: u64,
}
```

Similarly, invalid ability combinations are rejected:

```rust,compile_fail
use talus_sui_move_derive::move_struct;

/// Minimal UID fixture defined by a package for the compilation failure example.
#[move_struct(address = "0x2", module = "object", abilities = "store")]
pub struct UID {
    pub id: u64,
}

// A struct cannot be both `key` and `copy`.
#[move_struct(address = "0x1", module = "broken", abilities = "key, store, copy")]
pub struct KeyAndCopy {
    pub id: UID,
}
```
