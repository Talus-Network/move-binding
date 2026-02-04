use serde::{Deserialize, Serialize};

use crate::{parse_address, parse_identifier, HasDrop, MoveStruct, MoveType};

/// Move `0x2::sui::SUI` (the Sui coin type).
///
/// This is the type argument used for `coin::Coin<SUI>` and `balance::Balance<SUI>`.
///
/// # Example
/// ```
/// use sui_move::{sui::SUI, MoveStruct};
///
/// let tag = SUI::struct_tag_static();
/// assert_eq!(tag.module().to_string(), "sui");
/// assert_eq!(tag.name().to_string(), "SUI");
/// ```
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
