use serde::{Deserialize, Serialize};

use crate::{parse_address, parse_identifier, MoveStruct, MoveType};

/// Move `0x2::object_bag::ObjectBag`.
///
/// A heterogeneous container which stores objects (`key` values).
///
/// # Example
/// ```
/// use sui_move::{object_bag::ObjectBag, MoveType};
///
/// let _tag = <ObjectBag as MoveType>::type_tag_static();
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObjectBag {
    pub id: crate::types::UID,
}

impl MoveType for ObjectBag {
    fn type_tag_static() -> sui_sdk_types::TypeTag {
        sui_sdk_types::TypeTag::Struct(Box::new(Self::struct_tag_static()))
    }
}

impl MoveStruct for ObjectBag {
    fn struct_tag_static() -> sui_sdk_types::StructTag {
        sui_sdk_types::StructTag::new(
            parse_address("0x2").expect("address literal"),
            parse_identifier("object_bag").expect("module"),
            parse_identifier("ObjectBag").expect("name"),
            vec![],
        )
    }
}

impl crate::HasStore for ObjectBag {}
impl crate::HasKey for ObjectBag {}
