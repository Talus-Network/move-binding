use std::str::FromStr;
use sui_move::prelude::*;
use sui_move::MoveInstance;

#[sui_move_derive::move_module(address = "0x1", name = "vault")]
mod vault {
    use super::*;

    #[sui_move_derive::move_struct(
        address = "0x1",
        module = "vault",
        abilities = "key, store",
        phantoms = "T"
    )]
    pub struct Vault<T> {
        pub id: u64,
        pub balance: Vec<T>,
    }
}

#[sui_move_derive::move_module(address = "0x1", name = "wrapper")]
mod wrapper {
    use super::*;

    #[sui_move_derive::move_struct(address = "0x1", module = "wrapper", abilities = "key")]
    pub struct VaultWrapper {
        pub inner: crate::vault::Vault<u64>,
    }
}

#[test]
fn type_tag_matches_move_definition() {
    let tag = vault::Vault::<u64>::type_tag_static();
    match tag {
        sui_sdk_types::TypeTag::Struct(inner) => {
            let expected_addr = sui_sdk_types::Address::from_str("0x1").unwrap();
            assert_eq!(*inner.address(), expected_addr);
            assert_eq!(inner.module().to_string(), "vault");
            assert_eq!(inner.name().to_string(), "Vault");
            assert_eq!(inner.type_params().len(), 1);
            assert!(matches!(
                inner.type_params()[0],
                sui_sdk_types::TypeTag::U64
            ));
        }
        _ => panic!("expected struct type tag"),
    }
}

#[test]
fn nested_structs_are_supported() {
    let wrapper = wrapper::VaultWrapper {
        inner: vault::Vault::<u64> {
            id: 42,
            balance: vec![9, 8],
            phantom_t: std::marker::PhantomData,
        },
    };
    let bytes = wrapper.to_bcs().unwrap();
    let decoded = wrapper::VaultWrapper::from_bcs(&bytes).unwrap();
    assert_eq!(decoded.inner.id, 42);
    assert_eq!(decoded.inner.balance, vec![9, 8]);
    match wrapper::VaultWrapper::type_tag_static() {
        sui_sdk_types::TypeTag::Struct(tag) => {
            assert_eq!(tag.module().to_string(), "wrapper");
            assert_eq!(tag.name().to_string(), "VaultWrapper");
        }
        _ => panic!("expected struct tag"),
    }
}

#[test]
fn tag_verification_and_bcs_roundtrip() {
    let value = vault::Vault::<u64> {
        id: 7,
        balance: vec![1, 2, 3],
        phantom_t: std::marker::PhantomData,
    };
    let bytes = value.to_bcs().unwrap();

    // Correct tag decodes
    let inst = MoveInstance::<vault::Vault<u64>>::from_raw_type(
        vault::Vault::<u64>::type_tag_static(),
        &bytes,
    )
    .unwrap();
    assert_eq!(inst.value.id, 7);
    assert_eq!(inst.value.balance, vec![1, 2, 3]);

    // Wrong tag is rejected
    let err = MoveInstance::<vault::Vault<u64>>::from_raw_type(sui_sdk_types::TypeTag::U8, &bytes)
        .unwrap_err();
    matches!(err, sui_move::DecodeError::TypeTagMismatch { .. });
}
