//! Mapping for irreducible built-in Move types.
//!
//! Sui framework types such as `0x2::object::UID`, `0x2::coin::Coin`, and
//! `0x1::option::Option` are deliberately not mapped here. They are package-defined datatypes and
//! must be generated from package metadata like any other Move package.

use proc_macro2::TokenStream;

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
    let _ = (type_name, use_aliases);
    None
}
