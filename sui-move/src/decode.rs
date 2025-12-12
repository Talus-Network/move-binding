use serde::de::DeserializeOwned;

use crate::{Copyable, DecodeError, HasKey, MoveInstance, MoveStruct, Storable};

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
