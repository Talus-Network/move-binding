use sui_move::prelude::*;

fn main() {
    assert_eq!(sui_move::type_tag_of::<u64>(), TypeTag::U64);

    let _u256 = U256([0u8; 32]);
    assert!(matches!(<U256 as MoveType>::type_tag_static(), TypeTag::U256));

    #[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
    struct Demo;

    impl MoveType for Demo {
        fn type_tag_static() -> TypeTag {
            TypeTag::Struct(Box::new(<Self as MoveStruct>::struct_tag_static()))
        }
    }

    impl MoveStruct for Demo {
        fn struct_tag_static() -> StructTag {
            StructTag::new(
                parse_address("0x1").expect("address"),
                parse_identifier("demo").expect("module"),
                parse_identifier("Demo").expect("name"),
                vec![],
            )
        }
    }

    match <Demo as MoveType>::type_tag_static() {
        TypeTag::Struct(tag) => {
            assert_eq!(tag.module().to_string(), "demo");
            assert_eq!(tag.name().to_string(), "Demo");
        }
        other => panic!("expected struct type tag, got {other:?}"),
    }
}
