use std::str::FromStr;
use sui_move::prelude::*;
use sui_move::{containers::MoveOption, Copyable, MoveInstance, Storable};

#[sui_move_derive::move_module(address = "0x1", name = "vault")]
mod vault {
    use super::*;

    #[sui_move_derive::move_struct(
        address = "0x1",
        module = "vault",
        abilities = "key, store",
        phantoms = "T",
        uid_type = "sui_move::types::UID"
    )]
    pub struct Vault<T: sui_move::HasCopy + sui_move::HasStore> {
        pub id: sui_move::types::UID,
        pub balance: Vec<T>,
    }
}

#[sui_move_derive::move_module(address = "0x1", name = "wrapper")]
mod wrapper {
    use super::*;

    #[sui_move_derive::move_struct(address = "0x1", module = "wrapper", abilities = "key, store")]
    pub struct VaultWrapper {
        pub id: sui_move::types::UID,
        pub inner: crate::vault::Vault<u64>,
    }
}

#[sui_move_derive::move_module(address = "0x1", name = "bounded")]
mod bounded {
    use super::*;

    #[sui_move_derive::move_struct(
        address = "0x1",
        module = "bounded",
        abilities = "copy, store, drop",
        type_abilities = "T: store, copy"
    )]
    pub struct Boxed<T> {
        pub value: T,
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
        id: sui_move::types::UID {
            id: sui_move::types::ID {
                bytes: vec![0u8; 32],
            },
        },
        inner: vault::Vault::<u64> {
            id: sui_move::types::UID {
                id: sui_move::types::ID {
                    bytes: vec![42u8; 32],
                },
            },
            balance: vec![9, 8],
            phantom_t: std::marker::PhantomData,
        },
    };
    let bytes = wrapper.to_bcs().unwrap();
    let decoded = wrapper::VaultWrapper::from_bcs(&bytes).unwrap();
    assert_eq!(decoded.inner.id.id.bytes[0], 42);
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
        id: sui_move::types::UID {
            id: sui_move::types::ID {
                bytes: vec![7u8; 32],
            },
        },
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
    assert_eq!(inst.value.id.id.bytes[0], 7);
    assert_eq!(inst.value.balance, vec![1, 2, 3]);

    // Wrong tag is rejected
    let err = MoveInstance::<vault::Vault<u64>>::from_raw_type(sui_sdk_types::TypeTag::U8, &bytes)
        .unwrap_err();
    matches!(err, sui_move::DecodeError::TypeTagMismatch { .. });
}

fn require_store<T: Storable>(_: &T) {}
fn require_copy<T: Copyable>(_: &T) {}

#[test]
fn type_abilities_are_respected() {
    let boxed = bounded::Boxed::<u64> { value: 5 };
    require_store(&boxed);
    require_store(&boxed.value);
    require_copy(&boxed.value);

    let nested = vault::Vault {
        id: sui_move::types::UID {
            id: sui_move::types::ID {
                bytes: vec![9u8; 32],
            },
        },
        balance: vec![bounded::Boxed::<u64> { value: 7 }],
        phantom_t: std::marker::PhantomData,
    };
    assert_eq!(nested.balance[0].value, 7);
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

    let uid = sui_move::types::UID {
        id: sui_move::types::ID {
            bytes: vec![0u8; 32],
        },
    };
    let table = sui_move::containers::Table::<u64, u64> {
        id: uid.clone(),
        phantom: std::marker::PhantomData,
    };
    let table_bytes = table.to_bcs().unwrap();
    let decoded_table = sui_move::containers::Table::<u64, u64>::from_bcs(&table_bytes).unwrap();
    assert_eq!(decoded_table.id, uid);

    let df = sui_move::containers::DynamicField::<u64, u64> {
        id: uid.clone(),
        name: 9,
        value: 8,
    };
    let df_bytes = df.to_bcs().unwrap();
    let decoded_df = sui_move::containers::DynamicField::<u64, u64>::from_bcs(&df_bytes).unwrap();
    assert_eq!(decoded_df.value, 8);

    let dof = sui_move::containers::DynamicObjectField::<u64, u64> {
        id: uid.clone(),
        name: 1,
        value: 2,
    };
    let dof_bytes = dof.to_bcs().unwrap();
    let decoded_dof =
        sui_move::containers::DynamicObjectField::<u64, u64>::from_bcs(&dof_bytes).unwrap();
    assert_eq!(decoded_dof.name, 1);
}
