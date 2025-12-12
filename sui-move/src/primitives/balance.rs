use serde::{Deserialize, Serialize};

use crate::{parse_address, parse_identifier, HasStore, MoveStruct, MoveType};

/// 0x2::balance::Balance
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Balance<T> {
    pub value: u64,
    #[serde(skip, default)]
    pub phantom: std::marker::PhantomData<T>,
}

impl<T: MoveType> MoveType for Balance<T> {
    fn type_tag_static() -> sui_sdk_types::TypeTag {
        sui_sdk_types::TypeTag::Struct(Box::new(Self::struct_tag_static()))
    }
}

impl<T: MoveType> MoveStruct for Balance<T> {
    fn struct_tag_static() -> sui_sdk_types::StructTag {
        sui_sdk_types::StructTag::new(
            parse_address("0x2").expect("address literal"),
            parse_identifier("balance").expect("module"),
            parse_identifier("Balance").expect("name"),
            vec![T::type_tag_static()],
        )
    }
}

impl<T> HasStore for Balance<T> {}
