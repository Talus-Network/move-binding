//! Ability-aware decoding helpers.
//!
//! These helpers mirror the conceptual “this value is storable/copyable/keyed” boundaries that
//! exist in Move, while keeping the runtime behavior small and explicit.

use serde::de::DeserializeOwned;

use crate::{Copyable, DecodeError, HasKey, MoveInstance, MoveStruct, Storable};

/// Decode a `store` value from BCS bytes.
pub fn decode_storable<T: Storable + DeserializeOwned>(bytes: &[u8]) -> Result<T, DecodeError> {
    Ok(T::from_bcs(bytes)?)
}

/// Decode a `copy + drop` value from BCS bytes.
pub fn decode_copyable<T: Copyable + DeserializeOwned>(bytes: &[u8]) -> Result<T, DecodeError> {
    Ok(T::from_bcs(bytes)?)
}

/// Decode a `key` struct from raw `(TypeTag, BCS bytes)`, verifying the tag matches the type.
///
/// # Example
/// ```
/// use serde::{Deserialize, Serialize};
/// use sui_move::{decode_keyed, parse_address, parse_identifier, HasKey, HasStore, MoveStruct, MoveType};
/// use sui_sdk_types::{StructTag, TypeTag};
///
/// #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
/// struct Counter {
///     value: u64,
/// }
///
/// impl MoveType for Counter {
///     fn type_tag_static() -> TypeTag {
///         TypeTag::Struct(Box::new(Self::struct_tag_static()))
///     }
/// }
///
/// impl MoveStruct for Counter {
///     fn struct_tag_static() -> StructTag {
///         StructTag::new(
///             parse_address("0x1").unwrap(),
///             parse_identifier("counter").unwrap(),
///             parse_identifier("Counter").unwrap(),
///             vec![],
///         )
///     }
/// }
///
/// impl HasKey for Counter {}
/// impl HasStore for Counter {}
///
/// let value = Counter { value: 10 };
/// let bytes = value.to_bcs().unwrap();
/// let inst = decode_keyed::<Counter>(Counter::type_tag_static(), &bytes).unwrap();
/// assert_eq!(inst.value.value, 10);
/// ```
pub fn decode_keyed<T: MoveStruct + HasKey + DeserializeOwned>(
    type_tag: sui_sdk_types::TypeTag,
    bytes: &[u8],
) -> Result<MoveInstance<T>, DecodeError> {
    MoveInstance::from_raw_type(type_tag, bytes)
}
