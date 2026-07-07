use std::str::FromStr;

use sui_move_call::{
    CallArg, CallSpec, CallTarget, MoveObject, ReceivingMoveObject, SharedMoveObject,
};
use sui_move_ptb::{ptb, BuildError, PtbBuilder};
use sui_sdk_types::{
    Address, Argument, Command, Digest, FundsWithdrawal, Mutability, ObjectReference, Owner,
    TypeTag, WithdrawFrom,
};

#[sui_move::move_struct(address = "0x2", module = "object", abilities = "copy, store")]
struct ID {
    bytes: Address,
}

#[sui_move::move_struct(address = "0x2", module = "object", abilities = "store")]
struct UID {
    id: ID,
}

#[sui_move::move_struct(address = "0x1", module = "demo", abilities = "key")]
struct Thing {
    id: UID,
}

fn mk_obj(id: &str, version: u64) -> ObjectReference {
    ObjectReference::new(Address::from_str(id).unwrap(), version, Digest::default())
}

fn addr(byte: u8) -> Address {
    Address::new([byte; 32])
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
fn generic_object_helpers_build_canonical_inputs() {
    let object = mk_obj("0x2", 3);
    let shared_id = Address::from_str("0x3").unwrap();

    let mut tx = PtbBuilder::new();
    let owned = tx.owned_object(&object).unwrap();
    let shared = tx.shared_object_by_id(shared_id, 7, true).unwrap();
    let clock = tx.clock().unwrap();

    assert_eq!(owned, Argument::Input(0));
    assert_eq!(shared, Argument::Input(1));
    assert_eq!(clock, Argument::Input(2));

    assert!(matches!(tx.inputs()[0], CallArg::ImmutableOrOwned(_)));

    let CallArg::Shared(shared) = &tx.inputs()[1] else {
        panic!("expected shared input")
    };
    assert_eq!(shared.object_id(), shared_id);
    assert_eq!(shared.version(), 7);
    assert_eq!(shared.mutability(), Mutability::Mutable);

    let CallArg::Shared(clock) = &tx.inputs()[2] else {
        panic!("expected shared clock input")
    };
    assert_eq!(clock.object_id(), sui_move_ptb::CLOCK_OBJECT_ID);
    assert_eq!(clock.version(), 1);
    assert_eq!(clock.mutability(), Mutability::Immutable);
}

#[test]
fn object_from_owner_uses_owner_shape() {
    let mut tx = PtbBuilder::new();
    let shared_object = ObjectReference::new(addr(0x02), 9, Digest::default());
    let owned_object = ObjectReference::new(addr(0x03), 9, Digest::default());
    let mutable_owned_object = ObjectReference::new(addr(0x04), 9, Digest::default());

    let shared = tx
        .object_from_owner(&shared_object, Owner::Shared(5), true)
        .unwrap();
    let owned = tx
        .object_from_owner(&owned_object, Owner::Address(addr(0xAA)), false)
        .unwrap();

    assert_eq!(shared, Argument::Input(0));
    assert_eq!(owned, Argument::Input(1));
    assert!(matches!(tx.inputs()[0], CallArg::Shared(_)));
    assert!(matches!(tx.inputs()[1], CallArg::ImmutableOrOwned(_)));

    let err = tx
        .object_from_owner(&mutable_owned_object, Owner::Address(addr(0xAA)), true)
        .unwrap_err();
    assert!(matches!(err, BuildError::UnsupportedOwner { .. }));
}

#[test]
fn builds_raw_bcs_inputs_funds_withdrawals_and_typed_vectors() {
    let mut tx = PtbBuilder::new();
    let raw = tx.pure_bcs(vec![1, 2, 3]).unwrap();
    let amount = tx.arg(&10u64).unwrap();
    let coin = tx.funds_withdrawal_coin(TypeTag::U64, 11).unwrap();
    let vector = tx.move_vector::<u64>(vec![amount]).unwrap();

    assert_eq!(raw, Argument::Input(0));
    assert_eq!(coin, Argument::Input(2));
    assert_eq!(vector, Argument::Result(0));

    let pt = tx.finish();
    assert!(matches!(pt.inputs[0], CallArg::Pure(_)));
    assert!(matches!(pt.inputs[2], CallArg::FundsWithdrawal(_)));
    assert!(matches!(pt.commands[0], Command::MakeMoveVector(_)));
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
fn build_move_call_from_target_and_existing_arguments() {
    let package = Address::from_str("0x1").unwrap();
    let mut target = CallTarget::new(package, "m", "composed").unwrap();
    target.push_type_arg::<u64>();

    let mut builder = PtbBuilder::new();
    let value = builder.arg(&7u64).unwrap();
    let result = builder
        .call_target(target, vec![Argument::Gas, value])
        .unwrap();

    assert_eq!(result, Argument::Result(0));
    let pt = builder.finish();
    assert_eq!(pt.commands.len(), 1);

    let Command::MoveCall(call) = &pt.commands[0] else {
        panic!("expected move call");
    };
    assert_eq!(call.package, package);
    assert_eq!(call.module.as_str(), "m");
    assert_eq!(call.function.as_str(), "composed");
    assert_eq!(call.arguments, vec![Argument::Gas, Argument::Input(0)]);
}

#[test]
fn build_call_target_from_existing_arguments() {
    let package = Address::from_str("0x1").unwrap();
    let mut target = CallTarget::new(package, "m", "composed").unwrap();
    target.push_type_arg::<u64>();

    let mut builder = PtbBuilder::new();
    let value = builder.arg(&7u64).unwrap();
    let result = builder
        .call_target(target, vec![Argument::Gas, value])
        .unwrap();

    assert_eq!(result, Argument::Result(0));
    let pt = builder.finish();
    assert_eq!(pt.commands.len(), 1);

    let Command::MoveCall(call) = &pt.commands[0] else {
        panic!("expected move call");
    };
    assert_eq!(call.package, package);
    assert_eq!(call.module.as_str(), "m");
    assert_eq!(call.function.as_str(), "composed");
    assert_eq!(call.arguments, vec![Argument::Gas, Argument::Input(0)]);
}

#[test]
fn publish_returns_upgrade_cap_result() {
    let mut builder = PtbBuilder::new();
    let result = builder
        .publish(vec![vec![1, 2, 3]], vec![Address::from_str("0x1").unwrap()])
        .unwrap();

    assert_eq!(result, Argument::Result(0));
    let pt = builder.finish();
    assert_eq!(pt.commands.len(), 1);
    assert!(matches!(pt.commands[0], Command::Publish(_)));
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

#[test]
fn nested_result_helper_requires_command_result() {
    let tx = PtbBuilder::new();
    let result = Argument::Result(7);
    assert_eq!(
        tx.nested_result(result, 2).unwrap(),
        Argument::NestedResult(7, 2)
    );
    assert_eq!(
        sui_move_ptb::nested_result(result, 2).unwrap(),
        Argument::NestedResult(7, 2)
    );

    let err = sui_move_ptb::nested_result(Argument::Input(0), 0).unwrap_err();
    assert!(matches!(err, BuildError::ExpectedCommandResult { .. }));
}
