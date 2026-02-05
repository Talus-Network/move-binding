use sui_move::move_struct;
use sui_move::prelude::Address;

#[move_struct(address = "0x2", module = "object", abilities = "copy, drop, store")]
pub struct ID {
    pub bytes: Address,
}

#[move_struct(address = "0x2", module = "object", abilities = "store")]
pub struct UID {
    pub id: ID,
}

#[move_struct(address = "0x1", module = "vault", abilities = "key, store")]
pub struct Vault {
    pub id: UID,
    pub value: u64,
}

fn main() {
    let _tag = <Vault as sui_move::MoveType>::type_tag_static();
}
