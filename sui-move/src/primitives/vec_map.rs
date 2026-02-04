use serde::{Deserialize, Serialize};

use crate::{parse_address, parse_identifier, MoveStruct, MoveType};

/// Move `0x2::vec_map::Entry<K, V>`.
///
/// The key type must be `copy` in the framework.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(bound = "")]
pub struct Entry<K: crate::MoveType + crate::HasCopy, V: crate::MoveType> {
    pub key: K,
    pub value: V,
}

impl<K: MoveType + crate::HasCopy, V: MoveType> MoveType for Entry<K, V> {
    fn type_tag_static() -> sui_sdk_types::TypeTag {
        sui_sdk_types::TypeTag::Struct(Box::new(Self::struct_tag_static()))
    }
}

impl<K: MoveType + crate::HasCopy, V: MoveType> MoveStruct for Entry<K, V> {
    fn struct_tag_static() -> sui_sdk_types::StructTag {
        sui_sdk_types::StructTag::new(
            parse_address("0x2").expect("address literal"),
            parse_identifier("vec_map").expect("module"),
            parse_identifier("Entry").expect("name"),
            vec![K::type_tag_static(), V::type_tag_static()],
        )
    }
}

impl<K: MoveType + crate::HasCopy + Clone, V: MoveType + Clone> crate::HasCopy for Entry<K, V> {}
impl<K: MoveType + crate::HasCopy, V: MoveType> crate::HasDrop for Entry<K, V> {}
impl<K: MoveType + crate::HasCopy, V: MoveType> crate::HasStore for Entry<K, V> {}

/// Move `0x2::vec_map::VecMap<K, V>`.
///
/// A small ordered map implementation backed by a vector of entries. The key type must be
/// `copy` in the framework.
///
/// # Example
/// ```
/// use sui_move::{prelude::*, vec_map::VecMap};
///
/// let _tag = <VecMap<u64, bool> as MoveType>::type_tag_static();
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(bound = "")]
pub struct VecMap<K: crate::MoveType + crate::HasCopy, V: crate::MoveType> {
    pub contents: Vec<Entry<K, V>>,
}

impl<K: MoveType + crate::HasCopy, V: MoveType> MoveType for VecMap<K, V> {
    fn type_tag_static() -> sui_sdk_types::TypeTag {
        sui_sdk_types::TypeTag::Struct(Box::new(Self::struct_tag_static()))
    }
}

impl<K: MoveType + crate::HasCopy, V: MoveType> MoveStruct for VecMap<K, V> {
    fn struct_tag_static() -> sui_sdk_types::StructTag {
        sui_sdk_types::StructTag::new(
            parse_address("0x2").expect("address literal"),
            parse_identifier("vec_map").expect("module"),
            parse_identifier("VecMap").expect("name"),
            vec![K::type_tag_static(), V::type_tag_static()],
        )
    }
}

impl<K: MoveType + crate::HasCopy + Clone, V: MoveType + Clone> crate::HasCopy for VecMap<K, V> {}
impl<K: MoveType + crate::HasCopy, V: MoveType> crate::HasDrop for VecMap<K, V> {}
impl<K: MoveType + crate::HasCopy, V: MoveType> crate::HasStore for VecMap<K, V> {}
