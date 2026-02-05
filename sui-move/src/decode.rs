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
/// use sui_move::{decode_keyed, parse_address, parse_identifier, HasKey, MoveStruct, MoveType};
/// use sui_sdk_types::{StructTag, TypeTag};
///
/// #[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
/// struct ID {
///     bytes: sui_sdk_types::Address,
/// }
///
/// impl MoveType for ID {
///     fn type_tag_static() -> TypeTag {
///         TypeTag::Struct(Box::new(<Self as MoveStruct>::struct_tag_static()))
///     }
/// }
///
/// impl MoveStruct for ID {
///     fn struct_tag_static() -> StructTag {
///         StructTag::new(
///             parse_address("0x2").unwrap(),
///             parse_identifier("object").unwrap(),
///             parse_identifier("ID").unwrap(),
///             vec![],
///         )
///     }
/// }
///
/// #[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
/// struct UID {
///     id: ID,
/// }
///
/// impl MoveType for UID {
///     fn type_tag_static() -> TypeTag {
///         TypeTag::Struct(Box::new(<Self as MoveStruct>::struct_tag_static()))
///     }
/// }
///
/// impl MoveStruct for UID {
///     fn struct_tag_static() -> StructTag {
///         StructTag::new(
///             parse_address("0x2").unwrap(),
///             parse_identifier("object").unwrap(),
///             parse_identifier("UID").unwrap(),
///             vec![],
///         )
///     }
/// }
///
/// #[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
/// struct DemoCoin {
///     id: UID,
///     value: u64,
/// }
///
/// impl HasKey for DemoCoin {}
///
/// impl MoveType for DemoCoin {
///     fn type_tag_static() -> TypeTag {
///         TypeTag::Struct(Box::new(<Self as MoveStruct>::struct_tag_static()))
///     }
/// }
///
/// impl MoveStruct for DemoCoin {
///     fn struct_tag_static() -> StructTag {
///         StructTag::new(
///             parse_address("0x1").unwrap(),
///             parse_identifier("demo").unwrap(),
///             parse_identifier("DemoCoin").unwrap(),
///             vec![],
///         )
///     }
/// }
///
/// let coin = DemoCoin {
///     id: UID {
///         id: ID {
///             bytes: sui_sdk_types::Address::new([0u8; 32]),
///         },
///     },
///     value: 10,
/// };
///
/// let bytes = coin.to_bcs().unwrap();
/// let inst =
///     decode_keyed::<DemoCoin>(<DemoCoin as MoveType>::type_tag_static(), &bytes).unwrap();
/// assert_eq!(inst.value.value, 10);
/// ```
pub fn decode_keyed<T: MoveStruct + HasKey + DeserializeOwned>(
    type_tag: sui_sdk_types::TypeTag,
    bytes: &[u8],
) -> Result<MoveInstance<T>, DecodeError> {
    MoveInstance::from_raw_type(type_tag, bytes)
}
