use serde::{Deserialize, Serialize};

use crate::{parse_address, parse_identifier, types::UID, HasKey, HasStore, MoveStruct, MoveType};

/// 0x2::clock::Clock
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
