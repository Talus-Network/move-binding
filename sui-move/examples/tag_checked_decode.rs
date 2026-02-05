use sui_move::{decode_keyed, parse_address, parse_identifier, HasKey, MoveStruct, MoveType};
use sui_sdk_types::{Address, StructTag, TypeTag};

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct UID {
    bytes: Address,
}

impl MoveType for UID {
    fn type_tag_static() -> TypeTag {
        TypeTag::Struct(Box::new(<Self as MoveStruct>::struct_tag_static()))
    }
}

impl MoveStruct for UID {
    fn struct_tag_static() -> StructTag {
        StructTag::new(
            parse_address("0x2").expect("address"),
            parse_identifier("object").expect("module"),
            parse_identifier("UID").expect("name"),
            vec![],
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct DemoCoin {
    id: UID,
    balance: u64,
}

impl MoveType for DemoCoin {
    fn type_tag_static() -> TypeTag {
        TypeTag::Struct(Box::new(<Self as MoveStruct>::struct_tag_static()))
    }
}

impl MoveStruct for DemoCoin {
    fn struct_tag_static() -> StructTag {
        StructTag::new(
            parse_address("0x1").expect("address"),
            parse_identifier("demo").expect("module"),
            parse_identifier("Coin").expect("name"),
            vec![],
        )
    }
}

impl HasKey for DemoCoin {}

fn main() {
    let coin = DemoCoin {
        id: UID {
            bytes: Address::new([7u8; 32]),
        },
        balance: 10,
    };

    let bytes = coin.to_bcs().unwrap();

    let inst = decode_keyed::<DemoCoin>(<DemoCoin as MoveType>::type_tag_static(), &bytes).unwrap();
    assert_eq!(inst.value.balance, 10);

    let err = decode_keyed::<DemoCoin>(TypeTag::U8, &bytes).unwrap_err();
    assert!(matches!(err, sui_move::DecodeError::TypeTagMismatch { .. }));
}
