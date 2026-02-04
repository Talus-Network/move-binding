use std::str::FromStr;
use sui_move::prelude::*;
use sui_move::{containers::MoveOption, decode_keyed, MoveInstance};

#[test]
fn primitive_type_tags_are_correct() {
    assert!(matches!(
        <u8 as MoveType>::type_tag_static(),
        sui_sdk_types::TypeTag::U8
    ));
    assert!(matches!(
        <u16 as MoveType>::type_tag_static(),
        sui_sdk_types::TypeTag::U16
    ));
    assert!(matches!(
        <u32 as MoveType>::type_tag_static(),
        sui_sdk_types::TypeTag::U32
    ));
    assert!(matches!(
        <u64 as MoveType>::type_tag_static(),
        sui_sdk_types::TypeTag::U64
    ));
    assert!(matches!(
        <u128 as MoveType>::type_tag_static(),
        sui_sdk_types::TypeTag::U128
    ));
    assert!(matches!(
        <bool as MoveType>::type_tag_static(),
        sui_sdk_types::TypeTag::Bool
    ));
    assert!(matches!(
        <sui_sdk_types::Address as MoveType>::type_tag_static(),
        sui_sdk_types::TypeTag::Address
    ));

    match <Vec<u64> as MoveType>::type_tag_static() {
        sui_sdk_types::TypeTag::Vector(inner) => {
            assert!(matches!(*inner, sui_sdk_types::TypeTag::U64));
        }
        other => panic!("expected vector type tag, got {other:?}"),
    }
}

#[test]
fn core_types_have_correct_tags() {
    match <sui_move::types::ID as MoveType>::type_tag_static() {
        sui_sdk_types::TypeTag::Struct(tag) => {
            assert_eq!(
                *tag.address(),
                sui_sdk_types::Address::from_str("0x2").unwrap()
            );
            assert_eq!(tag.module().to_string(), "object");
            assert_eq!(tag.name().to_string(), "ID");
        }
        other => panic!("expected struct tag, got {other:?}"),
    }

    match <sui_move::types::UID as MoveType>::type_tag_static() {
        sui_sdk_types::TypeTag::Struct(tag) => {
            assert_eq!(
                *tag.address(),
                sui_sdk_types::Address::from_str("0x2").unwrap()
            );
            assert_eq!(tag.module().to_string(), "object");
            assert_eq!(tag.name().to_string(), "UID");
        }
        other => panic!("expected struct tag, got {other:?}"),
    }
}

#[test]
fn tag_verification_and_bcs_roundtrip() {
    let value = MoveOption::<u64> { vec: vec![1, 2, 3] };
    let bytes = value.to_bcs().unwrap();

    let inst = MoveInstance::<MoveOption<u64>>::from_raw_type(
        <MoveOption<u64> as MoveType>::type_tag_static(),
        &bytes,
    )
    .unwrap();
    assert_eq!(inst.value.vec, vec![1, 2, 3]);

    let err = MoveInstance::<MoveOption<u64>>::from_raw_type(sui_sdk_types::TypeTag::U8, &bytes)
        .unwrap_err();
    assert!(matches!(err, sui_move::DecodeError::TypeTagMismatch { .. }));
}

fn require_store<T: Storable>(_: &T) {}
fn require_copy<T: Copyable>(_: &T) {}

#[test]
fn type_abilities_are_respected() {
    let value = 5u64;
    require_store(&value);
    require_copy(&value);

    let uid = uid_with_byte(9);
    require_store(&uid);
}

#[test]
fn move_option_and_containers_have_correct_tags() {
    let opt = MoveOption::<u64> { vec: vec![5] };
    let bytes = opt.to_bcs().unwrap();
    let decoded = MoveOption::<u64>::from_bcs(&bytes).unwrap();
    assert_eq!(decoded.vec, vec![5]);
    match MoveOption::<u64>::type_tag_static() {
        sui_sdk_types::TypeTag::Struct(tag) => {
            assert_eq!(tag.module().to_string(), "option");
            assert_eq!(tag.name().to_string(), "Option");
        }
        _ => panic!("expected struct tag"),
    }

    let table = sui_move::containers::Table::<u64, u64> {
        id: uid_with_byte(0),
        size: 0,
        phantom: std::marker::PhantomData,
    };
    let table_bytes = table.to_bcs().unwrap();
    let decoded_table = sui_move::containers::Table::<u64, u64>::from_bcs(&table_bytes).unwrap();
    assert_eq!(decoded_table.id.id.bytes, Address::new([0u8; 32]));
    assert_eq!(decoded_table.size, 0);

    let df = sui_move::containers::DynamicField::<u64, u64> {
        id: uid_with_byte(1),
        name: 9,
        value: 8,
    };
    let df_bytes = df.to_bcs().unwrap();
    let decoded_df = sui_move::containers::DynamicField::<u64, u64>::from_bcs(&df_bytes).unwrap();
    assert_eq!(decoded_df.value, 8);

    let dof = sui_move::containers::DynamicObjectField::<u64> {
        id: uid_with_byte(2),
        name: sui_move::containers::DynamicObjectFieldWrapper { name: 1u64 },
        value: sui_move::types::ID {
            bytes: Address::new([2u8; 32]),
        },
    };
    let dof_bytes = dof.to_bcs().unwrap();
    let decoded_dof =
        sui_move::containers::DynamicObjectField::<u64>::from_bcs(&dof_bytes).unwrap();
    assert_eq!(decoded_dof.name.name, 1);
}

#[test]
fn primitives_can_decode_keyed_values() {
    let coin = sui_move::coin::Coin::<sui_move::sui::SUI> {
        id: uid_with_byte(7),
        balance: sui_move::balance::Balance::<sui_move::sui::SUI> {
            value: 123,
            phantom: std::marker::PhantomData,
        },
    };
    let bytes = coin.to_bcs().unwrap();

    let decoded = decode_keyed::<sui_move::coin::Coin<sui_move::sui::SUI>>(
        <sui_move::coin::Coin<sui_move::sui::SUI> as MoveType>::type_tag_static(),
        &bytes,
    )
    .unwrap();
    assert_eq!(decoded.value.balance.value, 123);
}

#[test]
fn object_id_uses_address_bcs_layout() {
    let addr = Address::new([9u8; 32]);
    let id = sui_move::types::ID { bytes: addr };
    let bytes = id.to_bcs().unwrap();
    assert_eq!(bytes.len(), 32);
    assert_eq!(bytes.as_slice(), addr.as_bytes());
}

#[test]
fn table_and_dynamic_object_field_match_framework_layouts() {
    let table = sui_move::containers::Table::<u64, u64> {
        id: uid_with_byte(1),
        size: 7,
        phantom: std::marker::PhantomData,
    };
    let bytes = table.to_bcs().unwrap();
    assert_eq!(bytes.len(), 40);
    let decoded = sui_move::containers::Table::<u64, u64>::from_bcs(&bytes).unwrap();
    assert_eq!(decoded.size, 7);

    let wrapper_tag = sui_move::containers::DynamicObjectFieldWrapper::<u64>::struct_tag_static();
    assert_eq!(wrapper_tag.module().to_string(), "dynamic_object_field");
    assert_eq!(wrapper_tag.name().to_string(), "Wrapper");

    let dof_tag = sui_move::containers::DynamicObjectField::<u64>::struct_tag_static();
    assert_eq!(dof_tag.module().to_string(), "dynamic_field");
    assert_eq!(dof_tag.name().to_string(), "Field");
    assert_eq!(dof_tag.type_params().len(), 2);

    match &dof_tag.type_params()[0] {
        TypeTag::Struct(inner) => {
            assert_eq!(inner.module().to_string(), "dynamic_object_field");
            assert_eq!(inner.name().to_string(), "Wrapper");
        }
        other => panic!("expected wrapper struct type tag, got {other:?}"),
    }

    assert_eq!(
        dof_tag.type_params()[1],
        sui_move::types::ID::type_tag_static()
    );
}

#[test]
fn framework_containers_store_sizes() {
    let object_bag = sui_move::object_bag::ObjectBag {
        id: uid_with_byte(5),
        size: 0,
    };
    assert_eq!(object_bag.to_bcs().unwrap().len(), 40);

    let object_table =
        sui_move::object_table::ObjectTable::<u64, sui_move::coin::Coin<sui_move::sui::SUI>> {
            id: uid_with_byte(6),
            size: 0,
            phantom: std::marker::PhantomData,
        };
    assert_eq!(object_table.to_bcs().unwrap().len(), 40);

    let linked_table = sui_move::linked_table::LinkedTable::<u64, u64> {
        id: uid_with_byte(7),
        size: 0,
        head: MoveOption { vec: vec![] },
        tail: MoveOption { vec: vec![] },
        phantom_v: std::marker::PhantomData,
    };
    assert_eq!(linked_table.to_bcs().unwrap().len(), 42);
}

fn uid_with_byte(byte: u8) -> sui_move::types::UID {
    sui_move::types::UID {
        id: sui_move::types::ID {
            bytes: Address::new([byte; 32]),
        },
    }
}
