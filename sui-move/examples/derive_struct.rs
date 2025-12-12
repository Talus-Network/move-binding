use sui_move::move_struct;

#[move_struct(address = "0x1", module = "vault", abilities = "key, store")]
pub struct Vault {
    pub id: sui_move::types::UID,
    pub value: u64,
}

fn main() {
    let _tag = <Vault as sui_move::MoveType>::type_tag_static();
}
