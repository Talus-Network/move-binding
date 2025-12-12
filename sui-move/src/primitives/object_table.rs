use serde::{Deserialize, Serialize};

use crate::{parse_address, parse_identifier, MoveStruct, MoveType};

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObjectTable<
    K: MoveType + crate::HasCopy + crate::HasDrop + crate::HasStore,
    V: MoveType + crate::HasKey + crate::HasStore,
> {
    pub id: crate::types::UID,
    #[serde(skip, default)]
    pub phantom: std::marker::PhantomData<(K, V)>,
}

impl<
        K: MoveType + crate::HasCopy + crate::HasDrop + crate::HasStore,
        V: MoveType + crate::HasKey + crate::HasStore,
    > MoveType for ObjectTable<K, V>
{
    fn type_tag_static() -> sui_sdk_types::TypeTag {
        sui_sdk_types::TypeTag::Struct(Box::new(Self::struct_tag_static()))
    }
}

impl<
        K: MoveType + crate::HasCopy + crate::HasDrop + crate::HasStore,
        V: MoveType + crate::HasKey + crate::HasStore,
    > MoveStruct for ObjectTable<K, V>
{
    fn struct_tag_static() -> sui_sdk_types::StructTag {
        sui_sdk_types::StructTag::new(
            parse_address("0x2").expect("address literal"),
            parse_identifier("object_table").expect("module"),
            parse_identifier("ObjectTable").expect("name"),
            vec![K::type_tag_static(), V::type_tag_static()],
        )
    }
}

impl<
        K: MoveType + crate::HasCopy + crate::HasDrop + crate::HasStore,
        V: MoveType + crate::HasKey + crate::HasStore,
    > crate::HasStore for ObjectTable<K, V>
{
}
