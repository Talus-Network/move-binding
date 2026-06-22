use sui_move::move_struct;

/// Local declaration for the framework-shaped `0x2::object::ID` type.
///
/// `sui-move` intentionally does not export framework mirrors from its core. Generated package
/// bindings or local declarations provide these types when a package needs them.
#[move_struct(address = "0x2", module = "object", abilities = "copy, store")]
pub struct ID {
    /// Raw object address bytes.
    pub bytes: sui_move::prelude::Address,
}

/// Local declaration for the framework-shaped `0x2::object::UID` type.
///
/// The derive macro only needs a field whose type represents a Move `UID`; it does not require
/// the `UID` type to be exported by `sui-move`.
#[move_struct(address = "0x2", module = "object", abilities = "store")]
pub struct UID {
    /// Inner object id.
    pub id: ID,
}

/// Example key object using a locally declared `UID`.
#[move_struct(address = "0x1", module = "vault", abilities = "key, store")]
pub struct Vault {
    /// Object identity field required for `key` structs.
    pub id: UID,
    /// Stored counter value.
    pub value: u64,
}

fn main() {
    let _tag = <Vault as sui_move::MoveType>::type_tag_static();
}
