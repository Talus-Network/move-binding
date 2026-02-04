//! Common Move framework container types.
//!
//! These are “shape” types: they exist to preserve and compute correct Move type tags and to
//! support typed decoding. They do not implement any on-chain behavior.

use serde::{Deserialize, Serialize};

use crate::{
    parse_address, parse_identifier, HasCopy, HasDrop, HasKey, HasStore, MoveStruct, MoveType,
};

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
/// size, plus a phantom to preserve type parameters.
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(bound = "")]
pub struct Table<K: MoveType + HasCopy + HasDrop + HasStore, V: MoveType + HasStore> {
    pub id: crate::types::UID,
    pub size: u64,
    #[serde(skip, default)]
    pub phantom: std::marker::PhantomData<(K, V)>,
}

impl<K: MoveType + HasCopy + HasDrop + HasStore, V: MoveType + HasStore> MoveType for Table<K, V> {
    fn type_tag_static() -> sui_sdk_types::TypeTag {
        sui_sdk_types::TypeTag::Struct(Box::new(Self::struct_tag_static()))
    }
}

impl<K: MoveType + HasCopy + HasDrop + HasStore, V: MoveType + HasStore> MoveStruct
    for Table<K, V>
{
    fn struct_tag_static() -> sui_sdk_types::StructTag {
        sui_sdk_types::StructTag::new(
            parse_address("0x2").expect("address literal"),
            parse_identifier("table").expect("module"),
            parse_identifier("Table").expect("name"),
            vec![K::type_tag_static(), V::type_tag_static()],
        )
    }
}

impl<K: MoveType + HasCopy + HasDrop + HasStore, V: MoveType + HasStore> HasKey for Table<K, V> {}
impl<K: MoveType + HasCopy + HasDrop + HasStore, V: MoveType + HasStore> HasStore for Table<K, V> {}

/// Move `0x2::table_vec::TableVec<V>`.
///
/// The Sui framework table vec stores data as a `Table<u64, T>`.
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(bound = "")]
pub struct TableVec<T: MoveType + HasStore> {
    pub contents: Table<u64, T>,
}

impl<T: MoveType + HasStore> MoveType for TableVec<T> {
    fn type_tag_static() -> sui_sdk_types::TypeTag {
        sui_sdk_types::TypeTag::Struct(Box::new(Self::struct_tag_static()))
    }
}

impl<T: MoveType + HasStore> MoveStruct for TableVec<T> {
    fn struct_tag_static() -> sui_sdk_types::StructTag {
        sui_sdk_types::StructTag::new(
            parse_address("0x2").expect("address literal"),
            parse_identifier("table_vec").expect("module"),
            parse_identifier("TableVec").expect("name"),
            vec![T::type_tag_static()],
        )
    }
}

impl<T: MoveType + HasStore> HasStore for TableVec<T> {}

/// Move `0x2::dynamic_field::Field<K, V>`.
///
/// Dynamic fields are stored under an owning object and addressed by a “name” value.
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(bound = "")]
pub struct DynamicField<Name: MoveType + HasCopy + HasDrop + HasStore, Value: MoveType + HasStore> {
    pub id: crate::types::UID,
    pub name: Name,
    pub value: Value,
}

impl<Name: MoveType + HasCopy + HasDrop + HasStore, Value: MoveType + HasStore> MoveType
    for DynamicField<Name, Value>
{
    fn type_tag_static() -> sui_sdk_types::TypeTag {
        sui_sdk_types::TypeTag::Struct(Box::new(Self::struct_tag_static()))
    }
}

impl<Name: MoveType + HasCopy + HasDrop + HasStore, Value: MoveType + HasStore> MoveStruct
    for DynamicField<Name, Value>
{
    fn struct_tag_static() -> sui_sdk_types::StructTag {
        sui_sdk_types::StructTag::new(
            parse_address("0x2").expect("address literal"),
            parse_identifier("dynamic_field").expect("module"),
            parse_identifier("Field").expect("name"),
            vec![Name::type_tag_static(), Value::type_tag_static()],
        )
    }
}

impl<Name: MoveType + HasCopy + HasDrop + HasStore, Value: MoveType + HasStore> HasKey
    for DynamicField<Name, Value>
{
}

/// Move `0x2::dynamic_object_field::Wrapper<Name>`.
///
/// Dynamic object fields are stored as a `dynamic_field::Field<Wrapper<Name>, ID>`, where the
/// field's `value` stores the child's `object::ID`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(bound = "")]
pub struct DynamicObjectFieldWrapper<Name: MoveType + HasCopy + HasDrop + HasStore> {
    pub name: Name,
}

impl<Name: MoveType + HasCopy + HasDrop + HasStore> MoveType for DynamicObjectFieldWrapper<Name> {
    fn type_tag_static() -> sui_sdk_types::TypeTag {
        sui_sdk_types::TypeTag::Struct(Box::new(Self::struct_tag_static()))
    }
}

impl<Name: MoveType + HasCopy + HasDrop + HasStore> MoveStruct for DynamicObjectFieldWrapper<Name> {
    fn struct_tag_static() -> sui_sdk_types::StructTag {
        sui_sdk_types::StructTag::new(
            parse_address("0x2").expect("address literal"),
            parse_identifier("dynamic_object_field").expect("module"),
            parse_identifier("Wrapper").expect("name"),
            vec![Name::type_tag_static()],
        )
    }
}

impl<Name: MoveType + HasCopy + HasDrop + HasStore> HasCopy for DynamicObjectFieldWrapper<Name> {}
impl<Name: MoveType + HasCopy + HasDrop + HasStore> HasDrop for DynamicObjectFieldWrapper<Name> {}
impl<Name: MoveType + HasCopy + HasDrop + HasStore> HasStore for DynamicObjectFieldWrapper<Name> {}

/// Move `0x2::dynamic_field::Field<dynamic_object_field::Wrapper<Name>, object::ID>`.
pub type DynamicObjectField<Name> = DynamicField<DynamicObjectFieldWrapper<Name>, crate::types::ID>;
