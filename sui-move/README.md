# sui-move

Move-shaped type layer for Rust, built on top of `sui-sdk-types`.

This crate solves one problem: **represent Move types precisely in Rust** (including their
`TypeTag`/`StructTag` and ability surface), so you can build strongly-typed Sui clients and
helpers that can **verify type tags and decode BCS safely**.

## Where it fits

`sui-move` is the bottom layer of this repository’s stack (`MODEL.md`). Higher layers use it to:

- name types precisely when building Move calls (`TypeTag`/`StructTag`),
- express Move ability constraints as normal Rust bounds,
- verify on-chain type tags and decode BCS in a controlled way.

## Quickstart

The core traits are `MoveType` and `MoveStruct`.

```rust
use sui_move::prelude::*;

assert_eq!(<u64 as MoveType>::type_tag_static(), TypeTag::U64);
assert_eq!(
    <Vec<u8> as MoveType>::type_tag_static(),
    TypeTag::Vector(Box::new(TypeTag::U8)),
);
```

## Core concepts

### `MoveType` and `MoveStruct`

`MoveType` means a Rust type knows how to describe itself as a Move `TypeTag`.
`MoveStruct` is the struct-specific version that produces a `StructTag`.

For a Move struct type, you typically implement both:

```rust
use serde::{Deserialize, Serialize};
use sui_move::{parse_address, parse_identifier, HasStore, MoveStruct, MoveType};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MyCounter {
    pub value: u64,
}

impl MoveType for MyCounter {
    fn type_tag_static() -> sui_sdk_types::TypeTag {
        sui_sdk_types::TypeTag::Struct(Box::new(Self::struct_tag_static()))
    }
}

impl MoveStruct for MyCounter {
    fn struct_tag_static() -> sui_sdk_types::StructTag {
        sui_sdk_types::StructTag::new(
            parse_address("0x123").expect("address literal"),
            parse_identifier("counter").expect("module"),
            parse_identifier("MyCounter").expect("name"),
            vec![],
        )
    }
}

impl HasStore for MyCounter {}
```

### Macros (feature `derive`)

If you prefer, enable the `derive` feature to use attribute macros for defining Move-shaped
structs.

```rust
#[cfg(feature = "derive")]
mod example {
    use sui_move::move_struct;

    #[move_struct(address = "0x1", module = "vault", abilities = "key, store")]
    pub struct Vault {
        pub id: sui_move::types::UID,
        pub value: u64,
    }
}
```

### Move abilities as Rust bounds

Move abilities are modeled as marker traits (`HasKey`, `HasStore`, `HasCopy`, `HasDrop`), plus
small “ability surface” aliases:

- `Storable = MoveType + HasStore`
- `Copyable = MoveType + HasCopy + HasDrop + Clone`
- `Droppable = MoveType + HasDrop`

This lets you express the same constraints that Move does:

```rust
use sui_move::Storable;

fn requires_store<T: Storable>(_: &T) {}

let x = 42u64;
requires_store(&x);
```

### Tag-checked decoding (`MoveInstance`)

When you have `(TypeTag, bytes)` from the chain, you can decode and verify the tag matches the
Rust type you expect:

```rust
use sui_move::{MoveInstance, MoveType};

let value = vec![1u8, 2, 3];
let bytes = value.to_bcs().unwrap();

let inst = MoveInstance::<Vec<u8>>::from_raw_type(
    <Vec<u8> as MoveType>::type_tag_static(),
    &bytes,
)
.unwrap();

assert_eq!(inst.value, vec![1u8, 2, 3]);
```

## What’s included

### Built-in Rust types

The crate implements `MoveType` (and ability markers) for:

- `u8`, `u16`, `u32`, `u64`, `u128`, `bool`
- `sui_sdk_types::Address`
- `Vec<T>` where `T: MoveType`

### Sui framework types (`sui_move::primitives`)

`sui_move::primitives` contains minimal Rust mirrors of common Sui Move framework structs
so they can be referenced in `TypeTag`s and decoded in a typed way (e.g. `coin::Coin<T>`,
`balance::Balance<T>`, `vec_map::VecMap<K, V>`, `vec_set::VecSet<T>`, etc).

```rust
use sui_move::{coin::Coin, sui::SUI, MoveType};

let _tag = <Coin<SUI> as MoveType>::type_tag_static();
```

### Framework containers (`sui_move::containers`)

`sui_move::containers` includes widely-used container structs such as `MoveOption<T>`,
`Table<K, V>`, and dynamic field shapes, represented in a way that preserves their tags.

## Module guide

- `sui_move::prelude`: convenient imports for common traits/types
- `sui_move::types`: core Sui object types (`ID`, `UID`)
- `sui_move::containers`: Move framework container shapes
- `sui_move::primitives`: Sui framework “primitive” structs
- `sui_move::decode`: ability-aware decode helpers
