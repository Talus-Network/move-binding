use serde::{Deserialize, Serialize};

use crate::{parse_address, parse_identifier, HasCopy, HasDrop, HasStore, MoveStruct, MoveType};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ID {
    pub bytes: Vec<u8>,
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
