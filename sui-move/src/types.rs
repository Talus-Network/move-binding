//! Core Sui object types from the Move framework.
//!
//! These types are widely used by other Sui framework structs (e.g. `coin::Coin<T>`).

use serde::{Deserialize, Serialize};

use crate::{parse_address, parse_identifier, HasCopy, HasDrop, HasStore, MoveStruct, MoveType};

/// Move `0x2::object::ID`.
///
/// In the Sui framework this wraps a Move `address` for compact BCS encoding.
///
/// # Example
/// ```
/// use sui_move::prelude::*;
///
/// let tag = sui_move::types::ID::struct_tag_static();
/// assert_eq!(tag.module().to_string(), "object");
/// assert_eq!(tag.name().to_string(), "ID");
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ID {
    pub bytes: sui_sdk_types::Address,
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

/// Move `0x2::object::UID`.
///
/// This is the “unique ID” embedded in `key` objects.
///
/// # Example
/// ```
/// use sui_move::prelude::*;
///
/// let tag = sui_move::types::UID::struct_tag_static();
/// assert_eq!(tag.module().to_string(), "object");
/// assert_eq!(tag.name().to_string(), "UID");
/// ```
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
