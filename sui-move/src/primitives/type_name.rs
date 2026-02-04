use serde::{Deserialize, Serialize};

use crate::{parse_address, parse_identifier, MoveStruct, MoveType};

/// Move `0x1::type_name::TypeName`.
///
/// This is used in the Sui framework to carry a runtime type name.
///
/// # Example
/// ```
/// use sui_move::{type_name::TypeName, MoveStruct};
///
/// let tag = TypeName::struct_tag_static();
/// assert_eq!(tag.module().to_string(), "type_name");
/// assert_eq!(tag.name().to_string(), "TypeName");
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TypeName {
    pub name: String,
}

impl MoveType for TypeName {
    fn type_tag_static() -> sui_sdk_types::TypeTag {
        sui_sdk_types::TypeTag::Struct(Box::new(Self::struct_tag_static()))
    }
}

impl crate::MoveStruct for TypeName {
    fn struct_tag_static() -> sui_sdk_types::StructTag {
        sui_sdk_types::StructTag::new(
            parse_address("0x1").expect("address literal"),
            parse_identifier("type_name").expect("module"),
            parse_identifier("TypeName").expect("name"),
            vec![],
        )
    }
}

impl crate::HasCopy for TypeName {}
impl crate::HasDrop for TypeName {}
impl crate::HasStore for TypeName {}
