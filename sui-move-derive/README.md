# sui-move-derive

Procedural macros for [`sui-move`](../sui-move/README.md): define Move-shaped Rust types with minimal boilerplate.

This crate exists to solve one problem: **turn a Rust struct into a Move-shaped type** (correct
`TypeTag`/`StructTag` + ability markers) so it can be used with `sui-move`’s type-tag plumbing and
tag-checked decoding.

## Where it fits

In the repository’s layered stack (`MODEL.md`), `sui-move-derive` is a convenience layer for the
bottom type system (`sui-move`):

- you describe the Move identity (`address`, `module`, `abilities`) as attributes,
- the macro generates the corresponding `sui_move::MoveType` / `sui_move::MoveStruct` impls and
  ability marker impls,
- higher layers (call/PTB/runtime) consume those traits for typed interactions.

## What you get

Given a struct like:

```rust,no_run
use sui_move::prelude::*;
use sui_move_derive::move_struct;

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

- `impl sui_move::MoveType` and `impl sui_move::MoveStruct`
- Ability marker impls (`HasKey`, `HasStore`, `HasCopy`, `HasDrop`) based on `abilities = "..."`
- `serde` derives (without requiring your crate to depend on `serde` directly)
- Optional injected `PhantomData` fields for phantom type params
- Compile-time validation for common mistakes (e.g. `key` requires an `id: UID` field)

## Recommended usage

Most users should depend on `sui-move` and enable its `derive` feature (it re-exports these macros):

```toml
[dependencies]
sui-move = { path = "../sui-move", features = ["derive"] }
```

Then use:

```rust,ignore
use sui_move::move_struct;
```

You can also depend on `sui-move-derive` directly, but you must still depend on `sui-move` because
the generated impls reference it.

## `#[move_module]`

`#[move_module]` is currently a no-op marker attribute. It can be used to annotate Rust `mod`
blocks that correspond to Move modules.

## `#[move_struct(...)]` reference

Required arguments:

- `address = "0x..."`: Move address
- `module = "..."`: Move module name

Optional arguments:

- `name = "..."`: Override the Move struct name (defaults to the Rust struct name)
- `abilities = "key, store, copy, drop"`: Move abilities (comma-separated)
  - `copy` implies `drop`
  - `key` and `copy` are mutually exclusive
- `phantoms = "T, U"`: Mark type parameters as phantom and inject `PhantomData` fields
- `type_abilities = "T: store, copy; U: drop"`: Specify ability expectations for type parameters
- `uid_type = "path::to::UID"`: Override what type counts as `UID` when enforcing the `key` rule

## How bounds are enforced

The macro tries to make the “Move rules” visible as normal Rust type errors:

- Every type parameter gets a `T: sui_move::MoveType` bound.
- If the struct has a Move ability (e.g. `store`), the macro adds the corresponding bounds to
  each non-phantom field type (e.g. `field_ty: sui_move::HasStore`).
- For generic fields like `Vec<T>`, this naturally pushes requirements onto `T` (e.g.
  `Vec<T>: HasStore` implies `T: HasStore`).

You can satisfy those requirements either by:

- writing normal Rust bounds (`struct Vault<T: sui_move::HasStore> { ... }`), or
- using `type_abilities = "T: store"` to have the macro add the ability bounds for you.

## Examples

### `key` objects require `id: UID`

```rust,no_run
use std::marker::PhantomData;
use sui_move::prelude::Address;
use sui_move_derive::move_struct;

#[move_struct(address = "0x2", module = "object", abilities = "copy, drop, store")]
pub struct ID {
    pub bytes: Address,
}

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

let _tag = <Vault<u64> as sui_move::MoveType>::type_tag_static();
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
use sui_move_derive::move_struct;

#[move_struct(address = "0x1", module = "broken", abilities = "key, store")]
pub struct MissingId {
    pub value: u64,
}
```

Similarly, invalid ability combinations are rejected:

```rust,compile_fail
use sui_move_derive::move_struct;
use sui_move::prelude::Address;

// A struct cannot be both `key` and `copy`.
#[move_struct(address = "0x2", module = "object", abilities = "store")]
pub struct UID {
    pub id: Address,
}

#[move_struct(address = "0x1", module = "broken", abilities = "key, store, copy")]
pub struct KeyAndCopy {
    pub id: UID,
}
```
