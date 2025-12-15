use std::str::FromStr;

use sui_move_call::{CallArg, CallSpec, MoveObject, ReceivingMoveObject, SharedMoveObject};
use sui_move_ptb::{ptb, PtbBuilder};
use sui_sdk_types::{
    Address, Argument, Command, Digest, FundsWithdrawal, ObjectReference, TypeTag, WithdrawFrom,
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
