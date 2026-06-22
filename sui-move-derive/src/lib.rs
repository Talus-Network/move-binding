#![doc = include_str!("../README.md")]

use proc_macro::TokenStream;

mod abilities;
mod args;
mod expand;
mod util;

/// Marker attribute for Move module namespaces.
///
/// Today this is a no-op and exists mainly to make code more readable and leave room for future
/// tooling.
///
/// # Example
/// ```rust,no_run
/// use sui_move_derive::{move_module, move_struct};
///
/// #[move_module(address = "0x1", name = "vault")]
/// mod vault {
///     #[sui_move_derive::move_struct(address = "0x1", module = "vault", abilities = "copy, store")]
///     pub struct Counter {
///         pub value: u64,
///     }
/// }
///
/// fn main() {}
/// ```
#[proc_macro_attribute]
pub fn move_module(_args: TokenStream, input: TokenStream) -> TokenStream {
    input
}

/// Define a Move-shaped struct and derive `sui-move` trait implementations.
///
/// The attribute arguments define the Move address/module/name plus ability surface, and allow
/// marking type parameters as phantom.
///
/// The macro generates:
/// - `impl sui_move::MoveType` and `impl sui_move::MoveStruct`
/// - Ability marker impls (`HasKey`, `HasStore`, `HasCopy`, `HasDrop`)
/// - `serde` derives (via `sui_move::__private`, so downstream crates don't need direct `serde`)
/// - Optional injected `PhantomData` fields (see `phantoms = "..."`)
///
/// # Supported input
/// - Named-field structs only (`struct X { ... }`)
///
/// # Arguments
/// - `address = "0x..."` (required): Move address
/// - `module = "..."` (required): Move module name
/// - `name = "..."` (optional): override Move struct name (defaults to Rust name)
/// - `abilities = "key, store, copy, drop"` (optional): comma-separated Move abilities
/// - `phantoms = "T, U"` (optional): comma-separated phantom type params
/// - `type_abilities = "T: store, copy; U: drop"` (optional): ability expectations for type params
/// - `uid_type = "path::to::UID"` (optional): override what counts as `UID` for `key` enforcement
///
/// # Example
/// ```rust,no_run
/// use std::marker::PhantomData;
/// use sui_move::prelude::Address;
/// use sui_move::types::{ID, UID};
/// use sui_move_derive::move_struct;
///
/// #[move_struct(
///     address = "0x1",
///     module = "vault",
///     abilities = "key, store",
///     phantoms = "T",
///     type_abilities = "T: store"
/// )]
/// pub struct Vault<T> {
///     pub id: UID,
///     pub balance: Vec<T>,
/// }
///
/// let _value = Vault::<u64> {
///     id: UID {
///         id: ID {
///             bytes: Address::new([0u8; 32]),
///         },
///     },
///     balance: vec![1, 2, 3],
///     phantom_t: PhantomData,
/// };
/// ```
#[proc_macro_attribute]
pub fn move_struct(attr: TokenStream, item: TokenStream) -> TokenStream {
    let args = syn::parse_macro_input!(attr as args::MoveStructArgs);
    let input = syn::parse_macro_input!(item as syn::DeriveInput);

    match expand::expand_move_struct(args, input) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

#[cfg(test)]
mod tests {
    use super::args::MoveStructArgs;
    use std::collections::BTreeMap;

    #[test]
    fn parse_args() {
        let args: MoveStructArgs = syn::parse_quote!(
            address = "0x1",
            module = "vault",
            abilities = "key, store",
            phantoms = "T",
            type_abilities = "T: store, copy"
        );
        assert_eq!(args.address.as_deref(), Some("0x1"));
        assert_eq!(args.module.as_deref(), Some("vault"));
        assert_eq!(args.name, None);
        assert_eq!(args.abilities, vec!["key".to_string(), "store".to_string()]);
        assert_eq!(args.phantoms, vec!["T".to_string()]);

        let mut expected = BTreeMap::new();
        expected.insert(
            "T".to_string(),
            vec!["store".to_string(), "copy".to_string()],
        );
        assert_eq!(args.type_abilities, expected);
    }
}
