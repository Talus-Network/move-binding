//! Core traits and helpers for typed Move bindings.

pub use sui_sdk_types as sui;

/// Placeholder trait for Move type identity.
pub trait MoveType {
    fn type_tag() -> sui::TypeTag;
}

/// Placeholder trait for Move structs with layout information.
pub trait MoveStruct: MoveType {}
