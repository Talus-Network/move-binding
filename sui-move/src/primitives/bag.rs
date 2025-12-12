use serde::{Deserialize, Serialize};

use crate::{parse_address, parse_identifier, types::UID, HasKey, HasStore, MoveStruct, MoveType};

/// 0x2::bag::Bag
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bag {
    pub id: UID,
    pub size: u64,
}

impl MoveType for Bag {
    fn type_tag_static() -> sui_sdk_types::TypeTag {
        sui_sdk_types::TypeTag::Struct(Box::new(Self::struct_tag_static()))
    }
}

impl MoveStruct for Bag {
    fn struct_tag_static() -> sui_sdk_types::StructTag {
        sui_sdk_types::StructTag::new(
            parse_address("0x2").expect("address literal"),
            parse_identifier("bag").expect("module"),
            parse_identifier("Bag").expect("name"),
            vec![],
        )
    }
}

impl HasKey for Bag {}
impl HasStore for Bag {}
