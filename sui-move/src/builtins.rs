//! Implementations of [`MoveType`](crate::MoveType) and ability markers for Rust built-ins.
//!
//! This module provides mappings for:
//! - integer and boolean primitives (`u8`, `u16`, `u32`, `u64`, `u128`, `bool`)
//! - `sui_sdk_types::Address`
//! - `Vec<T>` as Move `vector<T>`

use crate::{HasCopy, HasDrop, HasStore, MoveType};

macro_rules! impl_primitive {
    ($ty:ty, $variant:ident) => {
        impl MoveType for $ty {
            fn type_tag_static() -> sui_sdk_types::TypeTag {
                sui_sdk_types::TypeTag::$variant
            }
        }
    };
}

impl_primitive!(u8, U8);
impl_primitive!(u16, U16);
impl_primitive!(u32, U32);
impl_primitive!(u64, U64);
impl_primitive!(u128, U128);
impl_primitive!(bool, Bool);

impl MoveType for sui_sdk_types::Address {
    fn type_tag_static() -> sui_sdk_types::TypeTag {
        sui_sdk_types::TypeTag::Address
    }
}

impl<T: MoveType> MoveType for Vec<T> {
    fn type_tag_static() -> sui_sdk_types::TypeTag {
        sui_sdk_types::TypeTag::Vector(Box::new(T::type_tag_static()))
    }
}

macro_rules! impl_ability_markers_primitive {
    ($ty:ty) => {
        impl HasCopy for $ty {}
        impl HasDrop for $ty {}
        impl HasStore for $ty {}
    };
}

impl_ability_markers_primitive!(u8);
impl_ability_markers_primitive!(u16);
impl_ability_markers_primitive!(u32);
impl_ability_markers_primitive!(u64);
impl_ability_markers_primitive!(u128);
impl_ability_markers_primitive!(bool);
impl_ability_markers_primitive!(sui_sdk_types::Address);

impl<T: HasCopy> HasCopy for Vec<T> {}
impl<T: HasDrop> HasDrop for Vec<T> {}
impl<T: HasStore> HasStore for Vec<T> {}
