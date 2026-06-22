use std::str::FromStr;

use sui_move_call::{
    CallArg, CallArgError, CallSpec, MoveObject, ReceivingMoveObject, SharedMoveObject, ToCallArg,
    ToCallArgMut,
};
use sui_sdk_types::{Address, Digest, FundsWithdrawal, ObjectReference, TypeTag, WithdrawFrom};

#[sui_move::move_struct(address = "0x2", module = "object", abilities = "copy, store")]
struct ID {
    bytes: Address,
}

#[sui_move::move_struct(address = "0x2", module = "object", abilities = "store")]
struct UID {
    id: ID,
}

#[sui_move::move_struct(address = "0x1", module = "demo", abilities = "key")]
struct Demo {
    id: UID,
}

#[test]
fn move_object_wraps_reference() {
    let id = Address::from_str("0x1").unwrap();
    let obj_ref = ObjectReference::new(id, 1, Digest::default());

    let mut obj = MoveObject::<Demo>::new(obj_ref.clone());
    assert_eq!(obj.reference(), &obj_ref);

    let replacement = ObjectReference::new(id, 2, Digest::default());
    obj.update_reference(replacement.clone());
    assert_eq!(obj.reference(), &replacement);
}

#[test]
fn into_call_arg_encodes_pure_values() {
    let arg = 7u64.to_call_arg().unwrap();
    let CallArg::Pure(bytes) = arg else {
        panic!("expected pure arg");
    };
    assert_eq!(bcs::from_bytes::<u64>(&bytes).unwrap(), 7);
}

#[test]
fn into_call_arg_converts_objects() {
    let id = Address::from_str("0x1").unwrap();
    let obj_ref = ObjectReference::new(id, 1, Digest::default());
    let obj = MoveObject::<Demo>::new(obj_ref.clone());

    assert_eq!(
        obj.to_call_arg().unwrap(),
        CallArg::ImmutableOrOwned(obj_ref.clone())
    );
}

#[test]
fn shared_move_object_converts_to_shared_input() {
    let object_id = Address::from_str("0x2").unwrap();
    let shared = SharedMoveObject::<Demo>::mutable(object_id, 7);

    assert_eq!(shared.object_id(), object_id);
    assert_eq!(shared.initial_shared_version(), 7);
    assert!(shared.is_mutable());

    assert!(matches!(shared.to_call_arg().unwrap(), CallArg::Shared(_)));
}

#[test]
fn shared_move_object_mut_call_arg_requires_writable() {
    let object_id = Address::from_str("0x2").unwrap();
    let shared = SharedMoveObject::<Demo>::immutable(object_id, 7);

    let err = shared.to_call_arg_mutable().unwrap_err();
    assert!(matches!(
        err,
        CallArgError::SharedMutability {
            object_id: _,
            actual: _
        }
    ));
}

#[test]
fn shared_move_object_mut_call_arg_encodes_shared_input() {
    let object_id = Address::from_str("0x2").unwrap();
    let shared = SharedMoveObject::<Demo>::mutable(object_id, 7);

    assert!(matches!(
        shared.to_call_arg_mutable().unwrap(),
        CallArg::Shared(_)
    ));
}

#[test]
fn receiving_move_object_converts_to_receiving_input() {
    let id = Address::from_str("0x3").unwrap();
    let obj_ref = ObjectReference::new(id, 1, Digest::default());

    let mut recv = ReceivingMoveObject::<Demo>::new(obj_ref.clone());
    assert_eq!(recv.reference(), &obj_ref);

    let replacement = ObjectReference::new(id, 2, Digest::default());
    recv.update_reference(replacement.clone());
    assert_eq!(recv.reference(), &replacement);

    assert_eq!(
        recv.to_call_arg().unwrap(),
        CallArg::Receiving(replacement.clone())
    );
}

#[test]
fn call_spec_validates_identifiers_and_accumulates_args() {
    assert!(CallSpec::new(Address::from_str("0x1").unwrap(), "1bad", "f").is_err());
    assert!(CallSpec::new(Address::from_str("0x1").unwrap(), "m", "1bad").is_err());

    let mut spec = CallSpec::new(Address::from_str("0x1").unwrap(), "m", "f").unwrap();
    spec.push_type_arg::<u64>();
    spec.push_arg(&7u64).unwrap();

    assert_eq!(spec.type_arguments, vec![TypeTag::U64]);
    assert_eq!(spec.arguments.len(), 1);
}

#[test]
fn call_spec_push_input_accepts_non_typed_variants() {
    let withdrawal =
        CallArg::FundsWithdrawal(FundsWithdrawal::new(10, TypeTag::U64, WithdrawFrom::Sender));

    let mut spec = CallSpec::new(Address::from_str("0x1").unwrap(), "m", "f").unwrap();
    spec.push_input(withdrawal.clone());

    assert_eq!(spec.arguments, vec![withdrawal]);
}
