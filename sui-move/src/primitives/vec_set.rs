use serde::{Deserialize, Serialize};

use crate::{parse_address, parse_identifier, MoveStruct, MoveType};

/// Move `0x2::vec_set::VecSet<T>`.
///
/// A small set implementation backed by a vector. In the Move framework, the type parameter must
/// be `copy`.
///
/// # Example
/// ```
/// use sui_move::{prelude::*, vec_set::VecSet};
///
/// let _tag = <VecSet<u64> as MoveType>::type_tag_static();
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(bound = "")]
pub struct VecSet<T: MoveType + crate::HasCopy>(pub Vec<T>);

impl<T: MoveType + crate::HasCopy> MoveType for VecSet<T> {
    fn type_tag_static() -> sui_sdk_types::TypeTag {
        sui_sdk_types::TypeTag::Struct(Box::new(Self::struct_tag_static()))
    }
}

impl<T: MoveType + crate::HasCopy> MoveStruct for VecSet<T> {
    fn struct_tag_static() -> sui_sdk_types::StructTag {
        sui_sdk_types::StructTag::new(
            parse_address("0x2").expect("address literal"),
            parse_identifier("vec_set").expect("module"),
            parse_identifier("VecSet").expect("name"),
            vec![T::type_tag_static()],
        )
    }
}

impl<T: MoveType + crate::HasCopy + Clone> crate::HasCopy for VecSet<T> {}
impl<T: MoveType + crate::HasCopy> crate::HasDrop for VecSet<T> {}
impl<T: MoveType + crate::HasCopy> crate::HasStore for VecSet<T> {}
