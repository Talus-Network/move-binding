//! Common Move framework container types.
//!
//! These are “shape” types: they exist to preserve and compute correct Move type tags and to
//! support typed decoding. They do not implement any on-chain behavior.

use serde::{Deserialize, Serialize};

use crate::{parse_address, parse_identifier, HasCopy, HasDrop, HasStore, MoveStruct, MoveType};

/// Move `0x1::option::Option<T>`.
///
/// In Move, `Option<T>` is represented as a `vector<T>` with length `0` (none) or `1` (some).
///
/// # Example
/// ```
/// use sui_move::prelude::*;
/// use sui_move::containers::MoveOption;
///
/// match <MoveOption<u64> as MoveType>::type_tag_static() {
///     TypeTag::Struct(tag) => {
///         assert_eq!(tag.module().to_string(), "option");
///         assert_eq!(tag.name().to_string(), "Option");
///     }
///     other => panic!("expected struct type tag, got {other:?}"),
/// }
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MoveOption<T> {
    pub vec: Vec<T>,
}

impl<T: MoveType + HasCopy + HasDrop + HasStore> MoveType for MoveOption<T> {
    fn type_tag_static() -> sui_sdk_types::TypeTag {
        sui_sdk_types::TypeTag::Struct(Box::new(Self::struct_tag_static()))
    }
}

impl<T: MoveType + HasCopy + HasDrop + HasStore> MoveStruct for MoveOption<T> {
    fn struct_tag_static() -> sui_sdk_types::StructTag {
        sui_sdk_types::StructTag::new(
            parse_address("0x1").expect("address literal"),
            parse_identifier("option").expect("module"),
            parse_identifier("Option").expect("name"),
            vec![T::type_tag_static()],
        )
    }
}

impl<T: HasCopy + HasDrop + HasStore> HasCopy for MoveOption<T> {}
impl<T: HasDrop + HasStore> HasDrop for MoveOption<T> {}
impl<T: HasStore> HasStore for MoveOption<T> {}

/// Move `0x2::table::Table<K, V>`.
///
/// The Sui framework table stores data under a `UID`. In Rust this struct carries the ID and a
/// phantom to preserve type parameters.
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Table<K, V> {
    pub id: crate::types::UID,
    #[serde(skip, default)]
    pub phantom: std::marker::PhantomData<(K, V)>,
}

impl<K: MoveType, V: MoveType> MoveType for Table<K, V> {
    fn type_tag_static() -> sui_sdk_types::TypeTag {
        sui_sdk_types::TypeTag::Struct(Box::new(Self::struct_tag_static()))
    }
}

impl<K: MoveType, V: MoveType> MoveStruct for Table<K, V> {
    fn struct_tag_static() -> sui_sdk_types::StructTag {
        sui_sdk_types::StructTag::new(
            parse_address("0x2").expect("address literal"),
            parse_identifier("table").expect("module"),
            parse_identifier("Table").expect("name"),
            vec![K::type_tag_static(), V::type_tag_static()],
        )
    }
}

impl<K: HasStore, V: HasStore> HasStore for Table<K, V> {}

/// Move `0x2::dynamic_field::Field<K, V>`.
///
/// Dynamic fields are stored under an owning object and addressed by a “name” value.
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DynamicField<K, V> {
    pub id: crate::types::UID,
    pub name: K,
    pub value: V,
}

impl<K: MoveType, V: MoveType> MoveType for DynamicField<K, V> {
    fn type_tag_static() -> sui_sdk_types::TypeTag {
        sui_sdk_types::TypeTag::Struct(Box::new(Self::struct_tag_static()))
    }
}

impl<K: MoveType, V: MoveType> MoveStruct for DynamicField<K, V> {
    fn struct_tag_static() -> sui_sdk_types::StructTag {
        sui_sdk_types::StructTag::new(
            parse_address("0x2").expect("address literal"),
            parse_identifier("dynamic_field").expect("module"),
            parse_identifier("Field").expect("name"),
            vec![K::type_tag_static(), V::type_tag_static()],
        )
    }
}

impl<K: HasStore, V: HasStore> HasStore for DynamicField<K, V> {}

/// Move `0x2::dynamic_object_field::DynamicField<K, V>`.
///
/// This variant stores an object value (a `key` struct) in the dynamic field.
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DynamicObjectField<K, V> {
    pub id: crate::types::UID,
    pub name: K,
    pub value: V,
}

impl<K: MoveType, V: MoveType> MoveType for DynamicObjectField<K, V> {
    fn type_tag_static() -> sui_sdk_types::TypeTag {
        sui_sdk_types::TypeTag::Struct(Box::new(Self::struct_tag_static()))
    }
}

impl<K: MoveType, V: MoveType> MoveStruct for DynamicObjectField<K, V> {
    fn struct_tag_static() -> sui_sdk_types::StructTag {
        sui_sdk_types::StructTag::new(
            parse_address("0x2").expect("address literal"),
            parse_identifier("dynamic_object_field").expect("module"),
            parse_identifier("DynamicField").expect("name"),
            vec![K::type_tag_static(), V::type_tag_static()],
        )
    }
}

impl<K: HasStore, V: HasStore> HasStore for DynamicObjectField<K, V> {}
