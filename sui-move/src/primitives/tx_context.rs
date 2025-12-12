use serde::{Deserialize, Serialize};

use crate::{parse_address, parse_identifier, MoveStruct, MoveType};

/// Phantom placeholder for `0x2::tx_context::TxContext` so it can appear in type tags.
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
