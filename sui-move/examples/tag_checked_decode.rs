use std::marker::PhantomData;

use sui_move::MoveType;
use sui_move::{balance::Balance, coin::Coin, decode_keyed, sui::SUI, types::ID, types::UID};
use sui_sdk_types::Address;

fn main() {
    let coin = Coin::<SUI> {
        id: UID {
            id: ID {
                bytes: Address::new([7u8; 32]),
            },
        },
        balance: Balance::<SUI> {
            value: 10,
            phantom: PhantomData,
        },
    };

    let bytes = coin.to_bcs().unwrap();

    let inst =
        decode_keyed::<Coin<SUI>>(<Coin<SUI> as sui_move::MoveType>::type_tag_static(), &bytes)
            .unwrap();
    assert_eq!(inst.value.balance.value, 10);

    let err = decode_keyed::<Coin<SUI>>(sui_sdk_types::TypeTag::U8, &bytes).unwrap_err();
    assert!(matches!(err, sui_move::DecodeError::TypeTagMismatch { .. }));
}
