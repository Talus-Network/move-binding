use std::str::FromStr;

use serde::{Deserialize, Serialize};
use sui_move::{
    decode_keyed, parse_address, parse_identifier, Copyable, HasKey, HasStore, MoveInstance,
    MoveStruct, MoveType, Storable,
};
use sui_sdk_types::{Address, StructTag, TypeTag};

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

#[test]
fn primitive_type_tags_are_correct() {
    assert!(matches!(<u8 as MoveType>::type_tag_static(), TypeTag::U8));
    assert!(matches!(<u16 as MoveType>::type_tag_static(), TypeTag::U16));
    assert!(matches!(<u32 as MoveType>::type_tag_static(), TypeTag::U32));
    assert!(matches!(<u64 as MoveType>::type_tag_static(), TypeTag::U64));
    assert!(matches!(
        <u128 as MoveType>::type_tag_static(),
        TypeTag::U128
    ));
    assert!(matches!(
        <bool as MoveType>::type_tag_static(),
        TypeTag::Bool
    ));
    assert_eq!(<Address as MoveType>::type_tag_static(), TypeTag::Address);
}

#[test]
fn vector_type_tags_are_recursive() {
    match <Vec<Vec<u64>> as MoveType>::type_tag_static() {
        TypeTag::Vector(outer) => match *outer {
            TypeTag::Vector(inner) => assert!(matches!(*inner, TypeTag::U64)),
            other => panic!("expected inner vector, got {other:?}"),
        },
        other => panic!("expected outer vector, got {other:?}"),
    }
}

#[test]
fn custom_struct_type_tag_matches_definition() {
    match Counter::type_tag_static() {
        TypeTag::Struct(tag) => {
            assert_eq!(*tag.address(), Address::from_str("0x123").unwrap());
            assert_eq!(tag.module().to_string(), "counter");
            assert_eq!(tag.name().to_string(), "Counter");
            assert!(tag.type_params().is_empty());
        }
        other => panic!("expected struct tag, got {other:?}"),
    }
}

#[test]
fn tag_verification_and_bcs_roundtrip_work_for_custom_types() {
    let value = Counter { value: 7 };
    let bytes = value.to_bcs().unwrap();

    let inst = MoveInstance::<Counter>::from_raw_type(Counter::type_tag_static(), &bytes).unwrap();
    assert_eq!(inst.value, value);

    let err = MoveInstance::<Counter>::from_raw_type(TypeTag::U64, &bytes).unwrap_err();
    assert!(matches!(err, sui_move::DecodeError::TypeTagMismatch { .. }));
}

#[test]
fn keyed_decode_uses_the_same_type_tag_boundary() {
    let value = Counter { value: 9 };
    let bytes = value.to_bcs().unwrap();

    let decoded = decode_keyed::<Counter>(Counter::type_tag_static(), &bytes).unwrap();
    assert_eq!(decoded.value, value);
}

fn require_store<T: Storable>(_: &T) {}
fn require_copy<T: Copyable>(_: &T) {}

#[test]
fn primitive_ability_markers_are_available() {
    let value = 5u64;
    require_store(&value);
    require_copy(&value);

    let bytes = vec![1u8, 2, 3];
    require_store(&bytes);
    require_copy(&bytes);
}
