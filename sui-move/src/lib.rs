//! Move-shaped typed layer for Rust, built on top of `sui-sdk-types`.
//!
//! This crate exposes a small set of traits that mirror Move's type system
//! (type tags, struct tags, abilities) plus helpers for safely decoding data
//! that carries a Move type tag.

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

pub mod prelude {
    //! Convenient import of the core traits and common Sui types.
    pub use crate::{
        containers::DynamicField, containers::DynamicObjectField, containers::MoveOption,
        containers::Table, types::ID, types::UID, Copyable, Droppable, HasCopy, HasDrop, HasKey,
        HasStore, MoveInstance, MoveStruct, MoveType, Storable,
    };
    pub use sui_sdk_types::{Address, Identifier, StructTag, TypeTag};
}

mod builtins;
pub mod containers;
pub mod decode;
pub mod primitives;
pub mod types;
pub use decode::{decode_copyable, decode_keyed, decode_storable};
pub use primitives::*;

/// A Move type. Implementors know how to produce their `TypeTag` and
/// serialize/deserialize themselves with BCS/serde.
pub trait MoveType: Serialize + for<'de> Deserialize<'de> + fmt::Debug + PartialEq + Eq {
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
pub trait HasCopy: Clone {}
pub trait HasDrop {}

/// Combinators that encode Move ability surfaces into Rust type bounds.
pub trait Storable: MoveType + HasStore {}
impl<T: MoveType + HasStore> Storable for T {}

pub trait Copyable: MoveType + HasCopy + HasDrop + Clone {}
impl<T: MoveType + HasCopy + HasDrop + Clone> Copyable for T {}

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

/// Convenience helper to get a static `TypeTag` for any `MoveType`.
pub fn type_tag_of<T: MoveType>() -> sui_sdk_types::TypeTag {
    T::type_tag_static()
}
