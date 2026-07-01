#![cfg(feature = "derive")]

use std::str::FromStr;

use sui_move::prelude::*;
use sui_move::{Copyable, MoveInstance, Storable};

mod object {
    #[sui_move::move_struct(address = "0x2", module = "object", abilities = "copy, store")]
    pub struct ID {
        pub bytes: sui_move::prelude::Address,
    }

    #[sui_move::move_struct(address = "0x2", module = "object", abilities = "store")]
    pub struct UID {
        pub id: ID,
    }
}

#[sui_move::move_module(address = "0x1", name = "vault")]
mod vault {
    #[sui_move::move_struct(
        address = "0x1",
        module = "vault",
        abilities = "key, store",
        phantoms = "T",
        uid_type = "crate::object::UID"
    )]
    pub struct Vault<T: sui_move::HasCopy + sui_move::HasStore> {
        pub id: crate::object::UID,
        pub balance: Vec<T>,
    }
}

#[sui_move::move_module(address = "0x1", name = "wrapper")]
mod wrapper {
    #[sui_move::move_struct(
        address = "0x1",
        module = "wrapper",
        abilities = "key, store",
        uid_type = "crate::object::UID"
    )]
    pub struct VaultWrapper {
        pub id: crate::object::UID,
        pub inner: crate::vault::Vault<u64>,
    }
}

#[sui_move::move_module(address = "0x1", name = "bounded")]
mod bounded {
    #[sui_move::move_struct(
        address = "0x1",
        module = "bounded",
        abilities = "copy, store, drop",
        type_abilities = "T: store, copy"
    )]
    pub struct Boxed<T> {
        pub value: T,
    }
}

mod dynamic_address {
    use std::str::FromStr;

    use sui_move::prelude::Address;

    pub fn package() -> Address {
        Address::from_str("0x9").unwrap()
    }

    #[sui_move::move_struct(
        address = "0x1",
        address_fn = "crate::dynamic_address::package",
        module = "dynamic_address",
        abilities = "copy, drop, store"
    )]
    pub struct Value {
        pub value: u64,
    }
}

#[test]
fn type_tag_matches_move_definition() {
    let tag = vault::Vault::<u64>::type_tag_static();
    match tag {
        TypeTag::Struct(inner) => {
            let expected_addr = Address::from_str("0x1").unwrap();
            assert_eq!(*inner.address(), expected_addr);
            assert_eq!(inner.module().to_string(), "vault");
            assert_eq!(inner.name().to_string(), "Vault");
            assert_eq!(inner.type_params().len(), 1);
            assert!(matches!(inner.type_params()[0], TypeTag::U64));
        }
        _ => panic!("expected struct type tag"),
    }
}

#[test]
fn address_fn_overrides_literal_address_for_type_tags() {
    let tag = dynamic_address::Value::type_tag_static();
    match tag {
        TypeTag::Struct(inner) => {
            assert_eq!(*inner.address(), Address::from_str("0x9").unwrap());
            assert_eq!(inner.module().to_string(), "dynamic_address");
            assert_eq!(inner.name().to_string(), "Value");
        }
        _ => panic!("expected struct type tag"),
    }
}

#[test]
fn local_uid_type_matches_framework_identity_without_core_exports() {
    let tag = object::UID::type_tag_static();
    match tag {
        TypeTag::Struct(inner) => {
            assert_eq!(*inner.address(), Address::from_str("0x2").unwrap());
            assert_eq!(inner.module().to_string(), "object");
            assert_eq!(inner.name().to_string(), "UID");
        }
        _ => panic!("expected struct type tag"),
    }
}

#[test]
fn nested_structs_are_supported() {
    let wrapper = wrapper::VaultWrapper {
        id: uid_with_byte(0),
        inner: vault::Vault::<u64> {
            id: uid_with_byte(42),
            balance: vec![9, 8],
            phantom_t: std::marker::PhantomData,
        },
    };

    let bytes = wrapper.to_bcs().unwrap();
    let decoded = wrapper::VaultWrapper::from_bcs(&bytes).unwrap();
    assert_eq!(decoded.inner.id.id.bytes.as_bytes()[0], 42);
    assert_eq!(decoded.inner.balance, vec![9, 8]);

    match wrapper::VaultWrapper::type_tag_static() {
        TypeTag::Struct(tag) => {
            assert_eq!(tag.module().to_string(), "wrapper");
            assert_eq!(tag.name().to_string(), "VaultWrapper");
        }
        _ => panic!("expected struct tag"),
    }
}

#[test]
fn tag_verification_and_bcs_roundtrip() {
    let value = vault::Vault::<u64> {
        id: uid_with_byte(7),
        balance: vec![1, 2, 3],
        phantom_t: std::marker::PhantomData,
    };
    let bytes = value.to_bcs().unwrap();

    let inst = MoveInstance::<vault::Vault<u64>>::from_raw_type(
        vault::Vault::<u64>::type_tag_static(),
        &bytes,
    )
    .unwrap();
    assert_eq!(inst.value.id.id.bytes.as_bytes()[0], 7);
    assert_eq!(inst.value.balance, vec![1, 2, 3]);

    let err = MoveInstance::<vault::Vault<u64>>::from_raw_type(TypeTag::U8, &bytes).unwrap_err();
    assert!(matches!(err, sui_move::DecodeError::TypeTagMismatch { .. }));
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
        id: uid_with_byte(9),
        balance: vec![bounded::Boxed::<u64> { value: 7 }],
        phantom_t: std::marker::PhantomData,
    };
    assert_eq!(nested.balance[0].value, 7);
}

fn uid_with_byte(byte: u8) -> object::UID {
    object::UID {
        id: object::ID {
            bytes: Address::new([byte; 32]),
        },
    }
}
