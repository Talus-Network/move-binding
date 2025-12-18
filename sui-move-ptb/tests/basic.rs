use std::str::FromStr;

use sui_move_call::{CallArg, CallSpec, MoveObject, ReceivingMoveObject, SharedMoveObject};
use sui_move_ptb::{ptb, BuildError, PtbBuilder};
use sui_sdk_types::{
    Address, Argument, Command, Digest, FundsWithdrawal, Mutability, ObjectReference, TypeTag,
    WithdrawFrom,
};

#[sui_move::move_struct(address = "0x1", module = "demo", abilities = "key")]
struct Thing {
    id: sui_move::types::UID,
}

fn mk_obj(id: &str, version: u64) -> ObjectReference {
    ObjectReference::new(Address::from_str(id).unwrap(), version, Digest::default())
}

#[test]
fn build_move_call_from_callspec() {
    let package = Address::from_str("0x1").unwrap();
    let obj = MoveObject::<Thing>::new(mk_obj("0x2", 1));

    let mut spec = CallSpec::new(package, "demo", "run").unwrap();
    spec.push_type_arg::<u64>();
    spec.push_arg(&obj).unwrap();
    spec.push_arg(&7u64).unwrap();

    let mut tx = PtbBuilder::new();
    let ret = tx.call(spec).unwrap();
    let pt = tx.finish();

    assert_eq!(ret, Argument::Result(0));
    assert_eq!(pt.inputs.len(), 2);
    assert_eq!(pt.commands.len(), 1);

    let Command::MoveCall(call) = &pt.commands[0] else {
        panic!("expected move call");
    };
    assert_eq!(call.arguments, vec![Argument::Input(0), Argument::Input(1)]);
}

#[test]
fn supports_shared_and_receiving_inputs() {
    let package = Address::from_str("0x1").unwrap();

    let shared = SharedMoveObject::<Thing>::immutable(Address::from_str("0x2").unwrap(), 1);
    let receiving = ReceivingMoveObject::<Thing>::new(mk_obj("0x3", 1));

    let mut spec = CallSpec::new(package, "demo", "run").unwrap();
    spec.push_arg(&shared).unwrap();
    spec.push_arg(&receiving).unwrap();

    let pt = ptb(|tx| {
        tx.call(spec)?;
        Ok(())
    })
    .unwrap();

    assert!(matches!(pt.inputs[0], CallArg::Shared(_)));
    assert!(matches!(pt.inputs[1], CallArg::Receiving(_)));
}

#[test]
fn dedups_identical_inputs_except_funds_withdrawal() {
    let package = Address::from_str("0x1").unwrap();
    let obj = MoveObject::<Thing>::new(mk_obj("0x2", 1));

    let mut a = CallSpec::new(package, "demo", "run").unwrap();
    a.push_arg(&obj).unwrap();
    a.push_arg(&1u64).unwrap();

    let mut b = CallSpec::new(package, "demo", "run").unwrap();
    b.push_arg(&obj).unwrap();
    b.push_arg(&2u64).unwrap();

    let fw = CallArg::FundsWithdrawal(FundsWithdrawal::new(10, TypeTag::U64, WithdrawFrom::Sender));
    let mut c = CallSpec::new(package, "demo", "run").unwrap();
    c.push_input(fw.clone());

    let mut d = CallSpec::new(package, "demo", "run").unwrap();
    d.push_input(fw.clone());

    let pt = ptb(|tx| {
        tx.call(a)?;
        tx.call(b)?;
        tx.call(c)?;
        tx.call(d)?;
        Ok(())
    })
    .unwrap();

    // Inputs: obj, 1, 2, fw, fw  (object reused; withdrawal not deduped)
    assert_eq!(pt.inputs.len(), 5);

    let fw_count = pt
        .inputs
        .iter()
        .filter(|i| matches!(i, CallArg::FundsWithdrawal(_)))
        .count();
    assert_eq!(fw_count, 2);
}

#[test]
fn upgrades_shared_mutability_and_reuses_input_index() {
    let object_id = Address::from_str("0x2").unwrap();
    let shared_imm = SharedMoveObject::<Thing>::immutable(object_id, 7);
    let shared_mut = SharedMoveObject::<Thing>::mutable(object_id, 7);

    let mut tx = PtbBuilder::new();
    let a0 = tx.arg(&shared_imm).unwrap();
    let a1 = tx.arg(&shared_mut).unwrap();

    assert_eq!(a0, Argument::Input(0));
    assert_eq!(a1, Argument::Input(0));
    assert_eq!(tx.inputs().len(), 1);

    let CallArg::Shared(shared) = &tx.inputs()[0] else {
        panic!("expected shared input")
    };
    assert_eq!(shared.object_id(), object_id);
    assert_eq!(shared.version(), 7);
    assert_eq!(shared.mutability(), Mutability::Mutable);
}

#[test]
fn rejects_duplicate_object_ids_between_receiving_and_input_objects() {
    let object_ref = mk_obj("0x2", 1);
    let owned = MoveObject::<Thing>::new(object_ref.clone());
    let receiving = ReceivingMoveObject::<Thing>::new(object_ref);

    let mut tx = PtbBuilder::new();
    tx.arg(&owned).unwrap();

    let err = tx.arg(&receiving).unwrap_err();
    assert!(matches!(err, BuildError::DuplicateObjectRefInput { .. }));
}
