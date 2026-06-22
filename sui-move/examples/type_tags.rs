use serde::{Deserialize, Serialize};
use sui_move::prelude::*;
use sui_move::{parse_address, parse_identifier};

/// Example package-defined type used to demonstrate the kernel type-tag API.
///
/// Framework declarations such as `0x2::coin::Coin` are intentionally not exported from
/// `sui-move`; they should be generated from package metadata like user-defined types.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Counter {
    value: u64,
}

impl MoveType for Counter {
    fn type_tag_static() -> TypeTag {
        TypeTag::Struct(Box::new(Self::struct_tag_static()))
    }
}

impl MoveStruct for Counter {
    fn struct_tag_static() -> StructTag {
        StructTag::new(
            parse_address("0x123").expect("address literal"),
            parse_identifier("counter").expect("module"),
            parse_identifier("Counter").expect("name"),
            vec![],
        )
    }
}

impl HasStore for Counter {}

fn main() {
    assert_eq!(sui_move::type_tag_of::<u64>(), TypeTag::U64);

    match Counter::type_tag_static() {
        TypeTag::Struct(tag) => {
            assert_eq!(tag.module().to_string(), "counter");
            assert_eq!(tag.name().to_string(), "Counter");
        }
        other => panic!("expected struct type tag, got {other:?}"),
    }
}
