use crate::ir::{Function, TypeRef, Visibility};

/// Whether a Move function can be called via a Sui `MoveCall` command.
///
/// Based on `sui-sdk-types` docs: callable functions are `entry` or `public` and must not have
/// reference return types.
pub(crate) fn is_callable(f: &Function) -> bool {
    if !(f.is_entry || matches!(f.visibility, Visibility::Public)) {
        return false;
    }
    !f.return_types.iter().any(type_contains_ref)
}

fn type_contains_ref(ty: &TypeRef) -> bool {
    match ty {
        TypeRef::Ref { .. } => true,
        TypeRef::Vector(inner) => type_contains_ref(inner),
        TypeRef::Datatype {
            type_arguments, ..
        } => type_arguments.iter().any(type_contains_ref),
        TypeRef::Address
        | TypeRef::Bool
        | TypeRef::U8
        | TypeRef::U16
        | TypeRef::U32
        | TypeRef::U64
        | TypeRef::U128
        | TypeRef::U256
        | TypeRef::TypeParameter(_) => false,
    }
}

