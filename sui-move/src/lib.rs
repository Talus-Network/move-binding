#![doc = include_str!("../README.md")]

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

#[cfg(feature = "derive")]
pub use sui_move_derive::{move_module, move_struct};

pub mod prelude {
    //! Convenient imports for working with this crate.
    //!
    //! Intended for end-user code and examples.
    #[cfg(feature = "derive")]
    pub use crate::{move_module, move_struct};
    pub use crate::{
        Copyable, Droppable, HasCopy, HasDrop, HasKey, HasStore, MoveInstance, MoveStruct,
        MoveType, Storable, U256,
    };
    pub use sui_sdk_types::{Address, Identifier, StructTag, TypeTag};
}

#[doc(hidden)]
pub mod __private {
    pub use serde;
    pub use sui_sdk_types;
}

mod builtins;
pub mod decode;
pub use decode::{decode_copyable, decode_keyed, decode_storable};

/// Move `u256` value backed by [`ethnum::U256`].
///
/// Serialization uses the 32 byte little endian representation required by Move rather than the
/// hexadecimal string representation provided by `ethnum`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct U256(pub ethnum::U256);

impl U256 {
    /// Construct a `u256` from its little endian byte representation.
    pub fn from_le_bytes(bytes: [u8; 32]) -> Self {
        Self(ethnum::U256::from_le_bytes(bytes))
    }

    /// Return the little endian byte representation.
    pub fn to_le_bytes(self) -> [u8; 32] {
        self.0.to_le_bytes()
    }
}

impl From<ethnum::U256> for U256 {
    fn from(value: ethnum::U256) -> Self {
        Self(value)
    }
}

impl From<U256> for ethnum::U256 {
    fn from(value: U256) -> Self {
        value.0
    }
}

impl Serialize for U256 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.to_le_bytes().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for U256 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        <[u8; 32]>::deserialize(deserializer).map(Self::from_le_bytes)
    }
}

/// A Rust type that corresponds to a Move type.
///
/// Implementors provide a static [`TypeTag`](sui_sdk_types::TypeTag) (including any type
/// arguments). This enables strongly-typed construction and verification of Move type tags and
/// safe BCS decoding.
///
/// # Example
/// ```
/// use sui_move::prelude::*;
///
/// assert_eq!(<u64 as MoveType>::type_tag_static(), TypeTag::U64);
/// ```
pub trait MoveType: Serialize + for<'de> Deserialize<'de> + fmt::Debug + PartialEq + Eq {
    /// Construct the static type tag for this type (including type arguments).
    fn type_tag_static() -> sui_sdk_types::TypeTag;

    /// Convenience to get the type tag for this value.
    fn type_tag(&self) -> sui_sdk_types::TypeTag {
        Self::type_tag_static()
    }

    /// Serialize this value with BCS.
    fn to_bcs(&self) -> Result<Vec<u8>, bcs::Error> {
        bcs::to_bytes(self)
    }

    /// Deserialize a value of this type from BCS bytes.
    fn from_bcs(bytes: &[u8]) -> Result<Self, bcs::Error>
    where
        Self: Sized,
    {
        bcs::from_bytes(bytes)
    }
}

/// A Move struct type (including any type parameters).
///
/// Move structs have both a [`TypeTag`](sui_sdk_types::TypeTag) and a
/// [`StructTag`](sui_sdk_types::StructTag).
///
pub trait MoveStruct: MoveType {
    /// Construct the static struct tag (including type arguments).
    fn struct_tag_static() -> sui_sdk_types::StructTag;

    fn struct_tag(&self) -> sui_sdk_types::StructTag {
        Self::struct_tag_static()
    }
}

/// Marker trait for the Move `key` ability.
pub trait HasKey {}

/// Marker trait for the Move `store` ability.
pub trait HasStore {}

/// Marker trait for the Move `copy` ability.
pub trait HasCopy: Clone {}

/// Marker trait for the Move `drop` ability.
pub trait HasDrop {}

/// A convenient bound for types that have the `store` ability.
pub trait Storable: MoveType + HasStore {}
impl<T: MoveType + HasStore> Storable for T {}

/// A convenient bound for types that have `copy` and `drop`.
pub trait Copyable: MoveType + HasCopy + HasDrop + Clone {}
impl<T: MoveType + HasCopy + HasDrop + Clone> Copyable for T {}

/// A convenient bound for types that have the `drop` ability.
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

/// A value paired with the Move type tag it was accompanied by.
///
/// This is useful when you receive `(type_tag, bcs_bytes)` from the chain and want to both verify
/// the tag and decode the value.
///
/// # Example
/// ```
/// use sui_move::{MoveInstance, MoveType};
///
/// let value = 7u64;
/// let bytes = value.to_bcs().unwrap();
///
/// let inst = MoveInstance::<u64>::from_raw_type(<u64 as MoveType>::type_tag_static(), &bytes)
///     .unwrap();
/// assert_eq!(inst.value, 7);
/// ```
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

/// Parse a Move identifier (e.g. module or struct name).
///
/// This is primarily a convenience for building `StructTag`s in manual [`MoveStruct`] impls.
///
/// # Example
/// ```
/// use sui_move::parse_identifier;
///
/// assert_eq!(parse_identifier("coin").unwrap().to_string(), "coin");
/// ```
pub fn parse_identifier(value: &str) -> Result<sui_sdk_types::Identifier, DecodeError> {
    sui_sdk_types::Identifier::from_str(value)
        .map_err(|_| DecodeError::Identifier(value.to_string()))
}

/// Parse a Sui address (e.g. `"0x2"`).
///
/// # Example
/// ```
/// use std::str::FromStr;
/// use sui_move::parse_address;
/// use sui_sdk_types::Address;
///
/// assert_eq!(
///     parse_address("0x2").unwrap(),
///     Address::from_str("0x2").unwrap()
/// );
/// ```
pub fn parse_address(value: &str) -> Result<sui_sdk_types::Address, DecodeError> {
    sui_sdk_types::Address::from_str(value).map_err(|_| DecodeError::Address(value.to_string()))
}

/// Convenience helper to get a static `TypeTag` for any `MoveType`.
///
/// # Example
/// ```
/// use sui_move::{type_tag_of, MoveType};
///
/// assert_eq!(type_tag_of::<u64>(), <u64 as MoveType>::type_tag_static());
/// ```
pub fn type_tag_of<T: MoveType>() -> sui_sdk_types::TypeTag {
    T::type_tag_static()
}
