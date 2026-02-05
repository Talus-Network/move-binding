use std::str::FromStr;

use sui_move::prelude::*;

#[test]
fn primitive_type_tags_are_correct() {
    assert!(matches!(<u8 as MoveType>::type_tag_static(), TypeTag::U8));
    assert!(matches!(<u16 as MoveType>::type_tag_static(), TypeTag::U16));
    assert!(matches!(<u32 as MoveType>::type_tag_static(), TypeTag::U32));
    assert!(matches!(<u64 as MoveType>::type_tag_static(), TypeTag::U64));
    assert!(matches!(<u128 as MoveType>::type_tag_static(), TypeTag::U128));
    assert!(matches!(<U256 as MoveType>::type_tag_static(), TypeTag::U256));
    assert!(matches!(<bool as MoveType>::type_tag_static(), TypeTag::Bool));
    assert!(matches!(<Address as MoveType>::type_tag_static(), TypeTag::Address));

    match <Vec<u64> as MoveType>::type_tag_static() {
        TypeTag::Vector(inner) => assert!(matches!(*inner, TypeTag::U64)),
        other => panic!("expected vector type tag, got {other:?}"),
    }
}

#[test]
fn parse_helpers_work() {
    assert_eq!(
        parse_address("0x2").unwrap(),
        Address::from_str("0x2").unwrap()
    );
    assert_eq!(parse_identifier("coin").unwrap().to_string(), "coin");
}

#[test]
fn move_instance_verifies_type_tag_and_roundtrips_bcs() {
    let value = vec![1u8, 2, 3];
    let bytes = value.to_bcs().unwrap();

    let inst = MoveInstance::<Vec<u8>>::from_raw_type(<Vec<u8> as MoveType>::type_tag_static(), &bytes)
        .unwrap();
    assert_eq!(inst.value, value);

    let err = MoveInstance::<Vec<u8>>::from_raw_type(TypeTag::U64, &bytes).unwrap_err();
    assert!(matches!(err, DecodeError::TypeTagMismatch { .. }));
}

#[test]
fn u256_bcs_is_32_bytes() {
    let value = U256([9u8; 32]);
    let bytes = value.to_bcs().unwrap();
    assert_eq!(bytes.len(), 32);
    let decoded = <U256 as MoveType>::from_bcs(&bytes).unwrap();
    assert_eq!(decoded, value);
}
