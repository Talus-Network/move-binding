use serde::{Deserialize, Serialize};

use crate::{
    containers::MoveOption, parse_address, parse_identifier, HasKey, HasStore, MoveStruct, MoveType,
};

/// Move `0x2::linked_table::LinkedTable<K, V>`.
///
/// A key object representing an ordered, table-like container in the Sui framework.
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(bound = "")]
pub struct LinkedTable<
    K: MoveType + crate::HasCopy + crate::HasDrop + crate::HasStore,
    V: MoveType + crate::HasStore,
> {
    pub id: crate::types::UID,
    pub size: u64,
    pub head: MoveOption<K>,
    pub tail: MoveOption<K>,
    #[serde(skip, default)]
    pub phantom_v: std::marker::PhantomData<V>,
}

impl<
        K: MoveType + crate::HasCopy + crate::HasDrop + crate::HasStore,
        V: MoveType + crate::HasStore,
    > MoveType for LinkedTable<K, V>
{
    fn type_tag_static() -> sui_sdk_types::TypeTag {
        sui_sdk_types::TypeTag::Struct(Box::new(Self::struct_tag_static()))
    }
}

impl<
        K: MoveType + crate::HasCopy + crate::HasDrop + crate::HasStore,
        V: MoveType + crate::HasStore,
    > MoveStruct for LinkedTable<K, V>
{
    fn struct_tag_static() -> sui_sdk_types::StructTag {
        sui_sdk_types::StructTag::new(
            parse_address("0x2").expect("address literal"),
            parse_identifier("linked_table").expect("module"),
            parse_identifier("LinkedTable").expect("name"),
            vec![K::type_tag_static(), V::type_tag_static()],
        )
    }
}

impl<
        K: MoveType + crate::HasCopy + crate::HasDrop + crate::HasStore,
        V: MoveType + crate::HasStore,
    > HasKey for LinkedTable<K, V>
{
}
impl<
        K: MoveType + crate::HasCopy + crate::HasDrop + crate::HasStore,
        V: MoveType + crate::HasStore,
    > HasStore for LinkedTable<K, V>
{
}

/// Move `0x2::linked_table::Node<K, V>`.
///
/// A store-only node value stored under a `LinkedTable`.
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(bound = "")]
pub struct Node<
    K: MoveType + crate::HasCopy + crate::HasDrop + crate::HasStore,
    V: MoveType + crate::HasStore,
> {
    pub prev: MoveOption<K>,
    pub next: MoveOption<K>,
    pub value: V,
}

impl<
        K: MoveType + crate::HasCopy + crate::HasDrop + crate::HasStore,
        V: MoveType + crate::HasStore,
    > MoveType for Node<K, V>
{
    fn type_tag_static() -> sui_sdk_types::TypeTag {
        sui_sdk_types::TypeTag::Struct(Box::new(Self::struct_tag_static()))
    }
}

impl<
        K: MoveType + crate::HasCopy + crate::HasDrop + crate::HasStore,
        V: MoveType + crate::HasStore,
    > MoveStruct for Node<K, V>
{
    fn struct_tag_static() -> sui_sdk_types::StructTag {
        sui_sdk_types::StructTag::new(
            parse_address("0x2").expect("address literal"),
            parse_identifier("linked_table").expect("module"),
            parse_identifier("Node").expect("name"),
            vec![K::type_tag_static(), V::type_tag_static()],
        )
    }
}

impl<
        K: MoveType + crate::HasCopy + crate::HasDrop + crate::HasStore,
        V: MoveType + crate::HasStore,
    > HasStore for Node<K, V>
{
}
