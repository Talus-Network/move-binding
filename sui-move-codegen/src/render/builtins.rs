//! Mapping for common Sui framework types.

use proc_macro2::TokenStream;
use quote::quote;

use crate::ir::TypeName;

pub(crate) struct BuiltinDatatype {
    pub(crate) path: TokenStream,
    /// Whether this builtin has the Move `key` ability (i.e. is an object type).
    ///
    /// This flag is used by call generation to decide whether a parameter should be typed as an
    /// object argument (`&impl ObjectArg<T>`) rather than a pure value.
    pub(crate) is_key: bool,
}

pub(crate) fn map_builtin(type_name: &TypeName, use_aliases: bool) -> Option<BuiltinDatatype> {
    let address = type_name.address.as_str();
    let module = type_name.module.as_str();
    let name = type_name.name.as_str();

    let (path, is_key) = match (address, module, name) {
        ("0x2", "object", "UID") => (sm(use_aliases, quote! { types::UID }), false),
        ("0x2", "object", "ID") => (sm(use_aliases, quote! { types::ID }), false),
        ("0x2", "sui", "SUI") => (sm(use_aliases, quote! { sui::SUI }), false),
        ("0x2", "bag", "Bag") => (sm(use_aliases, quote! { bag::Bag }), true),
        ("0x2", "balance", "Balance") => (sm(use_aliases, quote! { balance::Balance }), false),
        ("0x2", "coin", "Coin") => (sm(use_aliases, quote! { coin::Coin }), true),
        ("0x2", "clock", "Clock") => (sm(use_aliases, quote! { clock::Clock }), true),
        ("0x2", "tx_context", "TxContext") => {
            (sm(use_aliases, quote! { tx_context::TxContext }), false)
        }
        ("0x1", "type_name", "TypeName") => {
            (sm(use_aliases, quote! { type_name::TypeName }), false)
        }
        ("0x1", "ascii", "String") => (sm(use_aliases, quote! { ascii::String }), false),
        ("0x2", "vec_map", "VecMap") => (sm(use_aliases, quote! { vec_map::VecMap }), false),
        ("0x2", "vec_set", "VecSet") => (sm(use_aliases, quote! { vec_set::VecSet }), false),
        ("0x2", "object_bag", "ObjectBag") => {
            (sm(use_aliases, quote! { object_bag::ObjectBag }), true)
        }
        ("0x2", "linked_table", "LinkedTable") => {
            (sm(use_aliases, quote! { linked_table::LinkedTable }), false)
        }
        ("0x2", "object_table", "ObjectTable") => {
            (sm(use_aliases, quote! { object_table::ObjectTable }), false)
        }
        ("0x1", "option", "Option") => (sm(use_aliases, quote! { containers::MoveOption }), false),
        ("0x2", "table", "Table") => (sm(use_aliases, quote! { containers::Table }), false),
        ("0x2", "table_vec", "TableVec") => {
            (sm(use_aliases, quote! { containers::TableVec }), false)
        }
        ("0x2", "dynamic_field", "Field") => {
            (sm(use_aliases, quote! { containers::DynamicField }), false)
        }
        ("0x2", "dynamic_object_field", "DynamicField") => (
            sm(use_aliases, quote! { containers::DynamicObjectField }),
            false,
        ),
        _ => return None,
    };

    Some(BuiltinDatatype { path, is_key })
}

fn sm(use_aliases: bool, path: TokenStream) -> TokenStream {
    if use_aliases {
        quote! { sm::#path }
    } else {
        quote! { sui_move::#path }
    }
}
