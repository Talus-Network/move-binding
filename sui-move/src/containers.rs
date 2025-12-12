use serde::{Deserialize, Serialize};

use crate::{parse_address, parse_identifier, HasCopy, HasDrop, HasStore, MoveStruct, MoveType};

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
