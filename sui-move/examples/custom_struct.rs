use serde::{Deserialize, Serialize};
use sui_move::{parse_address, parse_identifier, HasStore, MoveStruct, MoveType};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct MyCounter {
    value: u64,
}

impl MoveType for MyCounter {
    fn type_tag_static() -> sui_sdk_types::TypeTag {
        sui_sdk_types::TypeTag::Struct(Box::new(Self::struct_tag_static()))
    }
}

impl MoveStruct for MyCounter {
    fn struct_tag_static() -> sui_sdk_types::StructTag {
        sui_sdk_types::StructTag::new(
            parse_address("0x123").expect("address literal"),
            parse_identifier("counter").expect("module"),
            parse_identifier("MyCounter").expect("name"),
            vec![],
        )
    }
}

impl HasStore for MyCounter {}

fn main() {
    let tag = <MyCounter as MoveType>::type_tag_static();
    match tag {
        sui_sdk_types::TypeTag::Struct(struct_tag) => {
            assert_eq!(struct_tag.module().to_string(), "counter");
            assert_eq!(struct_tag.name().to_string(), "MyCounter");
        }
        other => panic!("expected struct type tag, got {other:?}"),
    }
}
