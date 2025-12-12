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
/// use std::marker::PhantomData;
/// use sui_move::{balance::Balance, coin::Coin, decode_keyed, sui::SUI, types::ID, types::UID, MoveType};
///
/// let coin = Coin::<SUI> {
///     id: UID {
///         id: ID {
///             bytes: vec![0u8; 32],
///         },
///     },
///     balance: Balance::<SUI> {
///         value: 10,
///         phantom: PhantomData,
///     },
/// };
///
/// let bytes = coin.to_bcs().unwrap();
/// let inst = decode_keyed::<Coin<SUI>>(<Coin<SUI> as MoveType>::type_tag_static(), &bytes).unwrap();
/// assert_eq!(inst.value.balance.value, 10);
/// ```
pub fn decode_keyed<T: MoveStruct + HasKey + DeserializeOwned>(
    type_tag: sui_sdk_types::TypeTag,
    bytes: &[u8],
) -> Result<MoveInstance<T>, DecodeError> {
    MoveInstance::from_raw_type(type_tag, bytes)
}
