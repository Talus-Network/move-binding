use sui_move::prelude::*;
use sui_move::{coin::Coin, sui::SUI};

fn main() {
    assert_eq!(sui_move::type_tag_of::<u64>(), TypeTag::U64);

    match <Coin<SUI> as MoveType>::type_tag_static() {
        TypeTag::Struct(tag) => {
            assert_eq!(tag.module().to_string(), "coin");
            assert_eq!(tag.name().to_string(), "Coin");
        }
        other => panic!("expected struct type tag, got {other:?}"),
    }
}
