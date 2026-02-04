use serde::{Deserialize, Serialize};

use crate::{parse_address, parse_identifier, MoveStruct, MoveType};

/// Phantom placeholder for `0x2::tx_context::TxContext` so it can appear in type tags.
///
/// This type is not meant to be instantiated; it exists to build tags for entry function
/// signatures that reference `TxContext`.
///
/// # Example
/// ```
/// use sui_move::{tx_context::TxContext, MoveType};
///
/// let _tag = <TxContext as MoveType>::type_tag_static();
/// ```
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TxContext;

impl MoveType for TxContext {
    fn type_tag_static() -> sui_sdk_types::TypeTag {
        sui_sdk_types::TypeTag::Struct(Box::new(Self::struct_tag_static()))
    }
}

impl MoveStruct for TxContext {
    fn struct_tag_static() -> sui_sdk_types::StructTag {
        sui_sdk_types::StructTag::new(
            parse_address("0x2").expect("address literal"),
            parse_identifier("tx_context").expect("module"),
            parse_identifier("TxContext").expect("name"),
            vec![],
        )
    }
}
