//! Move-shaped typed layer for Rust, built on top of `sui-sdk-types`.
//!
//! This crate exposes a small set of traits that mirror Move's type system
//! (type tags, struct tags, abilities) plus helpers for safely decoding data
//! that carries a Move type tag. Attribute macros are provided by the companion
//! `sui-move-derive` crate and re-exported here.

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

pub use sui_move_derive::{move_module, move_struct};

pub mod prelude {
    //! Convenient import of the core traits, macros, and common Sui types.
    pub use crate::{
        containers::DynamicField, containers::DynamicObjectField, containers::MoveOption,
        containers::Table, move_module, move_struct, types::ID, types::UID, Copyable, Droppable,
        HasCopy, HasDrop, HasKey, HasStore, MoveInstance, MoveStruct, MoveType, Storable,
    };
    pub use sui_sdk_types::{Address, Identifier, StructTag, TypeTag};
}

/// A Move type. Implementors know how to produce their `TypeTag` and
/// serialize/deserialize themselves with BCS/serde.
pub trait MoveType:
    Clone + Serialize + for<'de> Deserialize<'de> + fmt::Debug + PartialEq + Eq
{
    /// Construct the static type tag for this type (including type arguments).
    fn type_tag_static() -> sui_sdk_types::TypeTag;

    /// Convenience to get the type tag for this value.
    fn type_tag(&self) -> sui_sdk_types::TypeTag {
        Self::type_tag_static()
    }

    fn to_bcs(&self) -> Result<Vec<u8>, bcs::Error> {
        bcs::to_bytes(self)
    }

    fn from_bcs(bytes: &[u8]) -> Result<Self, bcs::Error>
    where
        Self: Sized,
    {
        bcs::from_bytes(bytes)
    }

    fn to_json(&self) -> serde_json::Value {
        serde_json::to_value(self).expect("serialization should not fail")
    }
}

/// A Move struct (including any type parameters).
pub trait MoveStruct: MoveType {
    /// Construct the static struct tag (including type arguments).
    fn struct_tag_static() -> sui_sdk_types::StructTag;

    fn struct_tag(&self) -> sui_sdk_types::StructTag {
        Self::struct_tag_static()
    }
}

/// Ability markers matching Move abilities.
pub trait HasKey {}
pub trait HasStore {}
pub trait HasCopy {}
pub trait HasDrop {}

/// Combinators that encode Move ability surfaces into Rust type bounds.
pub trait Storable: MoveType + HasStore {}
impl<T: MoveType + HasStore> Storable for T {}

pub trait Copyable: MoveType + HasCopy + HasDrop {}
impl<T: MoveType + HasCopy + HasDrop> Copyable for T {}

pub trait Droppable: MoveType + HasDrop {}
impl<T: MoveType + HasDrop> Droppable for T {}

/// Errors that can occur when verifying or decoding Move data.
#[derive(thiserror::Error, Debug)]
pub enum DecodeError {
    #[error("type tag mismatch. expected {expected:?}, got {got:?}")]
    TypeTagMismatch {
        expected: sui_sdk_types::TypeTag,
        got: sui_sdk_types::TypeTag,
    },
    #[error(transparent)]
    Bcs(#[from] bcs::Error),
    #[error("failed to parse identifier: {0}")]
    Identifier(String),
    #[error("failed to parse address: {0}")]
    Address(String),
}

/// A typed value accompanied by its Move type tag.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MoveInstance<T: MoveType> {
    pub type_tag: sui_sdk_types::TypeTag,
    pub value: T,
}

impl<T: MoveType + DeserializeOwned> MoveInstance<T> {
    /// Decode from raw type tag + BCS bytes, verifying that the tag matches the type.
    pub fn from_raw_type(
        type_tag: sui_sdk_types::TypeTag,
        bytes: &[u8],
    ) -> Result<Self, DecodeError> {
        let expected = T::type_tag_static();
        if type_tag != expected {
            return Err(DecodeError::TypeTagMismatch {
                expected,
                got: type_tag,
            });
        }
        let value = T::from_bcs(bytes)?;
        Ok(Self { type_tag, value })
    }
}

/// Utility used by the derive macros to construct identifiers and addresses
/// from string literals at runtime.
pub fn parse_identifier(value: &str) -> Result<sui_sdk_types::Identifier, DecodeError> {
    sui_sdk_types::Identifier::from_str(value)
        .map_err(|_| DecodeError::Identifier(value.to_string()))
}

pub fn parse_address(value: &str) -> Result<sui_sdk_types::Address, DecodeError> {
    sui_sdk_types::Address::from_str(value).map_err(|_| DecodeError::Address(value.to_string()))
}

// Primitive MoveType implementations

macro_rules! impl_primitive {
    ($ty:ty, $variant:ident) => {
        impl MoveType for $ty {
            fn type_tag_static() -> sui_sdk_types::TypeTag {
                sui_sdk_types::TypeTag::$variant
            }
        }
    };
}

impl_primitive!(u8, U8);
impl_primitive!(u16, U16);
impl_primitive!(u32, U32);
impl_primitive!(u64, U64);
impl_primitive!(u128, U128);
impl_primitive!(bool, Bool);

impl MoveType for sui_sdk_types::Address {
    fn type_tag_static() -> sui_sdk_types::TypeTag {
        sui_sdk_types::TypeTag::Address
    }
}

impl<T: MoveType> MoveType for Vec<T> {
    fn type_tag_static() -> sui_sdk_types::TypeTag {
        sui_sdk_types::TypeTag::Vector(Box::new(T::type_tag_static()))
    }
}

macro_rules! impl_ability_markers_primitive {
    ($ty:ty) => {
        impl HasCopy for $ty {}
        impl HasDrop for $ty {}
        impl HasStore for $ty {}
    };
}

impl_ability_markers_primitive!(u8);
impl_ability_markers_primitive!(u16);
impl_ability_markers_primitive!(u32);
impl_ability_markers_primitive!(u64);
impl_ability_markers_primitive!(u128);
impl_ability_markers_primitive!(bool);
impl_ability_markers_primitive!(sui_sdk_types::Address);

impl<T: HasCopy> HasCopy for Vec<T> {}
impl<T: HasDrop> HasDrop for Vec<T> {}
impl<T: HasStore> HasStore for Vec<T> {}

pub mod types {
    use super::*;

    #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
    pub struct ID {
        pub bytes: Vec<u8>,
    }

    impl MoveType for ID {
        fn type_tag_static() -> sui_sdk_types::TypeTag {
            sui_sdk_types::TypeTag::Struct(Box::new(Self::struct_tag_static()))
        }
    }

    impl MoveStruct for ID {
        fn struct_tag_static() -> sui_sdk_types::StructTag {
            sui_sdk_types::StructTag::new(
                parse_address("0x2").expect("address literal"),
                parse_identifier("object").expect("module"),
                parse_identifier("ID").expect("name"),
                vec![],
            )
        }
    }

    impl HasCopy for ID {}
    impl HasDrop for ID {}
    impl HasStore for ID {}

    #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
    pub struct UID {
        pub id: ID,
    }

    impl MoveType for UID {
        fn type_tag_static() -> sui_sdk_types::TypeTag {
            sui_sdk_types::TypeTag::Struct(Box::new(Self::struct_tag_static()))
        }
    }

    impl MoveStruct for UID {
        fn struct_tag_static() -> sui_sdk_types::StructTag {
            sui_sdk_types::StructTag::new(
                parse_address("0x2").expect("address literal"),
                parse_identifier("object").expect("module"),
                parse_identifier("UID").expect("name"),
                vec![],
            )
        }
    }

    impl HasStore for UID {}
}

pub mod containers {
    use super::*;

    #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
    pub struct MoveOption<T> {
        pub vec: Vec<T>,
    }

    impl<T: MoveType + HasCopy + HasDrop + HasStore> MoveType for MoveOption<T> {
        fn type_tag_static() -> sui_sdk_types::TypeTag {
            sui_sdk_types::TypeTag::Struct(Box::new(Self::struct_tag_static()))
        }
    }

    impl<T: MoveType + HasCopy + HasDrop + HasStore> MoveStruct for MoveOption<T> {
        fn struct_tag_static() -> sui_sdk_types::StructTag {
            sui_sdk_types::StructTag::new(
                parse_address("0x1").expect("address literal"),
                parse_identifier("option").expect("module"),
                parse_identifier("Option").expect("name"),
                vec![T::type_tag_static()],
            )
        }
    }

    impl<T: HasCopy + HasDrop + HasStore> HasCopy for MoveOption<T> {}
    impl<T: HasDrop + HasStore> HasDrop for MoveOption<T> {}
    impl<T: HasStore> HasStore for MoveOption<T> {}

    #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
    pub struct Table<K, V> {
        pub id: crate::types::UID,
        #[serde(skip, default)]
        pub phantom: std::marker::PhantomData<(K, V)>,
    }

    impl<K: MoveType, V: MoveType> MoveType for Table<K, V> {
        fn type_tag_static() -> sui_sdk_types::TypeTag {
            sui_sdk_types::TypeTag::Struct(Box::new(Self::struct_tag_static()))
        }
    }

    impl<K: MoveType, V: MoveType> MoveStruct for Table<K, V> {
        fn struct_tag_static() -> sui_sdk_types::StructTag {
            sui_sdk_types::StructTag::new(
                parse_address("0x2").expect("address literal"),
                parse_identifier("table").expect("module"),
                parse_identifier("Table").expect("name"),
                vec![K::type_tag_static(), V::type_tag_static()],
            )
        }
    }

    impl<K: HasStore, V: HasStore> HasStore for Table<K, V> {}

    #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
    pub struct DynamicField<K, V> {
        pub id: crate::types::UID,
        pub name: K,
        pub value: V,
    }

    impl<K: MoveType, V: MoveType> MoveType for DynamicField<K, V> {
        fn type_tag_static() -> sui_sdk_types::TypeTag {
            sui_sdk_types::TypeTag::Struct(Box::new(Self::struct_tag_static()))
        }
    }

    impl<K: MoveType, V: MoveType> MoveStruct for DynamicField<K, V> {
        fn struct_tag_static() -> sui_sdk_types::StructTag {
            sui_sdk_types::StructTag::new(
                parse_address("0x2").expect("address literal"),
                parse_identifier("dynamic_field").expect("module"),
                parse_identifier("Field").expect("name"),
                vec![K::type_tag_static(), V::type_tag_static()],
            )
        }
    }

    impl<K: HasStore, V: HasStore> HasStore for DynamicField<K, V> {}

    #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
    pub struct DynamicObjectField<K, V> {
        pub id: crate::types::UID,
        pub name: K,
        pub value: V,
    }

    impl<K: MoveType, V: MoveType> MoveType for DynamicObjectField<K, V> {
        fn type_tag_static() -> sui_sdk_types::TypeTag {
            sui_sdk_types::TypeTag::Struct(Box::new(Self::struct_tag_static()))
        }
    }

    impl<K: MoveType, V: MoveType> MoveStruct for DynamicObjectField<K, V> {
        fn struct_tag_static() -> sui_sdk_types::StructTag {
            sui_sdk_types::StructTag::new(
                parse_address("0x2").expect("address literal"),
                parse_identifier("dynamic_object_field").expect("module"),
                parse_identifier("DynamicField").expect("name"),
                vec![K::type_tag_static(), V::type_tag_static()],
            )
        }
    }

    impl<K: HasStore, V: HasStore> HasStore for DynamicObjectField<K, V> {}
}

/// Ability-aware decoding helpers.
pub fn decode_storable<T: Storable + DeserializeOwned>(bytes: &[u8]) -> Result<T, DecodeError> {
    Ok(T::from_bcs(bytes)?)
}

pub fn decode_copyable<T: Copyable + DeserializeOwned>(bytes: &[u8]) -> Result<T, DecodeError> {
    Ok(T::from_bcs(bytes)?)
}

pub fn decode_keyed<T: MoveStruct + HasKey + DeserializeOwned>(
    type_tag: sui_sdk_types::TypeTag,
    bytes: &[u8],
) -> Result<MoveInstance<T>, DecodeError> {
    MoveInstance::from_raw_type(type_tag, bytes)
}
