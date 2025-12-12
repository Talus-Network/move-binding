use serde::{Deserialize, Serialize};

use crate::{parse_address, parse_identifier, HasDrop, MoveStruct, MoveType};

/// 0x2::sui::SUI (the Sui coin type)
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SUI;

impl MoveType for SUI {
    fn type_tag_static() -> sui_sdk_types::TypeTag {
        sui_sdk_types::TypeTag::Struct(Box::new(Self::struct_tag_static()))
    }
}

impl MoveStruct for SUI {
    fn struct_tag_static() -> sui_sdk_types::StructTag {
        sui_sdk_types::StructTag::new(
            parse_address("0x2").expect("address literal"),
            parse_identifier("sui").expect("module"),
            parse_identifier("SUI").expect("name"),
            vec![],
        )
    }
}

impl HasDrop for SUI {}
