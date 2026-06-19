use serde::{Deserialize, Serialize};
use sui_move::{
    decode_keyed, parse_address, parse_identifier, HasKey, HasStore, MoveStruct, MoveType,
};
use sui_sdk_types::{StructTag, TypeTag};

/// Example key-bearing type used to demonstrate tag-checked decoding.
///
/// In real package bindings, framework and user types should be generated from package metadata.
/// This example defines a small local type so the `sui-move` kernel remains self-contained.
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

impl HasKey for Counter {}
impl HasStore for Counter {}

fn main() {
    let counter = Counter { value: 10 };
    let bytes = counter.to_bcs().unwrap();

    let inst = decode_keyed::<Counter>(Counter::type_tag_static(), &bytes).unwrap();
    assert_eq!(inst.value.value, 10);

    let err = decode_keyed::<Counter>(TypeTag::U8, &bytes).unwrap_err();
    assert!(matches!(err, sui_move::DecodeError::TypeTagMismatch { .. }));
}
