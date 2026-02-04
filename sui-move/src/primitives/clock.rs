use serde::{Deserialize, Serialize};

use crate::{parse_address, parse_identifier, types::UID, HasKey, HasStore, MoveStruct, MoveType};

/// Move `0x2::clock::Clock`.
///
/// A `key` object that stores the current on-chain timestamp (milliseconds since epoch).
///
/// # Example
/// ```
/// use sui_move::{clock::Clock, MoveType};
///
/// let _tag = <Clock as MoveType>::type_tag_static();
/// ```
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Clock {
    pub id: UID,
    pub timestamp_ms: u64,
}

impl MoveType for Clock {
    fn type_tag_static() -> sui_sdk_types::TypeTag {
        sui_sdk_types::TypeTag::Struct(Box::new(Self::struct_tag_static()))
    }
}

impl MoveStruct for Clock {
    fn struct_tag_static() -> sui_sdk_types::StructTag {
        sui_sdk_types::StructTag::new(
            parse_address("0x2").expect("address literal"),
            parse_identifier("clock").expect("module"),
            parse_identifier("Clock").expect("name"),
            vec![],
        )
    }
}

impl HasKey for Clock {}
impl HasStore for Clock {}
