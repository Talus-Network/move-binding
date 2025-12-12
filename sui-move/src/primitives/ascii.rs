use serde::{Deserialize, Serialize};

use crate::{parse_address, parse_identifier, MoveStruct, MoveType};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct String(pub Vec<u8>);

impl MoveType for String {
    fn type_tag_static() -> sui_sdk_types::TypeTag {
        sui_sdk_types::TypeTag::Struct(Box::new(Self::struct_tag_static()))
    }
}

impl MoveStruct for String {
    fn struct_tag_static() -> sui_sdk_types::StructTag {
        sui_sdk_types::StructTag::new(
            parse_address("0x1").expect("address literal"),
            parse_identifier("ascii").expect("module"),
            parse_identifier("String").expect("name"),
            vec![],
        )
    }
}

impl crate::HasCopy for String {}
impl crate::HasDrop for String {}
impl crate::HasStore for String {}
