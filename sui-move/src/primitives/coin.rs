use serde::{Deserialize, Serialize};

use crate::{
    balance::Balance, parse_address, parse_identifier, types::UID, HasKey, HasStore, MoveStruct,
    MoveType,
};

/// 0x2::coin::Coin
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Coin<T> {
    pub id: UID,
    pub balance: Balance<T>,
}

impl<T: MoveType> MoveType for Coin<T> {
    fn type_tag_static() -> sui_sdk_types::TypeTag {
        sui_sdk_types::TypeTag::Struct(Box::new(Self::struct_tag_static()))
    }
}

impl<T: MoveType> MoveStruct for Coin<T> {
    fn struct_tag_static() -> sui_sdk_types::StructTag {
        sui_sdk_types::StructTag::new(
            parse_address("0x2").expect("address literal"),
            parse_identifier("coin").expect("module"),
            parse_identifier("Coin").expect("name"),
            vec![T::type_tag_static()],
        )
    }
}

impl<T> HasKey for Coin<T> {}
impl<T> HasStore for Coin<T> {}
