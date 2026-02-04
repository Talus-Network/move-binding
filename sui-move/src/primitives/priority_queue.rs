use serde::{Deserialize, Serialize};

use crate::{parse_address, parse_identifier, MoveStruct, MoveType};

/// Move `0x2::priority_queue::Entry<T>`.
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(bound = "")]
pub struct Entry<T: crate::MoveType + crate::HasDrop> {
    pub priority: u64,
    pub value: T,
}

impl<T: MoveType + crate::HasDrop> MoveType for Entry<T> {
    fn type_tag_static() -> sui_sdk_types::TypeTag {
        sui_sdk_types::TypeTag::Struct(Box::new(Self::struct_tag_static()))
    }
}

impl<T: MoveType + crate::HasDrop> MoveStruct for Entry<T> {
    fn struct_tag_static() -> sui_sdk_types::StructTag {
        sui_sdk_types::StructTag::new(
            parse_address("0x2").expect("address literal"),
            parse_identifier("priority_queue").expect("module"),
            parse_identifier("Entry").expect("name"),
            vec![T::type_tag_static()],
        )
    }
}

impl<T: MoveType + crate::HasDrop> crate::HasDrop for Entry<T> {}
impl<T: MoveType + crate::HasDrop> crate::HasStore for Entry<T> {}

/// Move `0x2::priority_queue::PriorityQueue<T>`.
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(bound = "")]
pub struct PriorityQueue<T: crate::MoveType + crate::HasDrop> {
    pub entries: Vec<Entry<T>>,
}

impl<T: MoveType + crate::HasDrop> MoveType for PriorityQueue<T> {
    fn type_tag_static() -> sui_sdk_types::TypeTag {
        sui_sdk_types::TypeTag::Struct(Box::new(Self::struct_tag_static()))
    }
}

impl<T: MoveType + crate::HasDrop> MoveStruct for PriorityQueue<T> {
    fn struct_tag_static() -> sui_sdk_types::StructTag {
        sui_sdk_types::StructTag::new(
            parse_address("0x2").expect("address literal"),
            parse_identifier("priority_queue").expect("module"),
            parse_identifier("PriorityQueue").expect("name"),
            vec![T::type_tag_static()],
        )
    }
}

impl<T: MoveType + crate::HasDrop> crate::HasDrop for PriorityQueue<T> {}
impl<T: MoveType + crate::HasDrop> crate::HasStore for PriorityQueue<T> {}
