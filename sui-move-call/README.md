# sui-move-call

Typed building blocks for describing Move calls on Sui.

This crate builds on top of [`sui-move`](../sui-move/README.md) and solves one problem:
**describe a Move call in a typed way** (object handles + type arguments + arguments) without
building or executing transactions.

## Where it fits

`sui-move-call` is the “Call” layer in the repository’s Read → Tx → Commit mental model (`MODEL.md`):

- **Read** (runtime) fetches objects and classifies on-chain ownership.
- **Call** (this crate) describes *what* to call and how to encode arguments.
- **PTB** builds a `ProgrammableTransaction` from a `CallSpec`.
- **Commit** (runtime) submits and applies effects to advance the cursor.

This crate sits directly above `sui-move`: it uses `MoveType`/`MoveStruct` to build type-checked
call descriptions (`CallSpec`). Transaction-building and execution are intentionally out of scope.

## Core types

- `CallSpec`: `(package, module, function)` + type arguments + call arguments
- `CallArg`: canonical call-argument representation (re-export of `sui_sdk_types::Input`)
- `ToCallArg`: convert values into `CallArg` without consuming them
- `ToCallArgMut`: convert values into `CallArg` for Move `&mut` parameters (shared inputs become
  mutable)
- `ObjectArg<T>`: typed object-argument trait used by generated interfaces (accepts any object
  handle that can be encoded as both `&` and `&mut` in Move)
- `MoveObject<T>`: typed handle for `Input::ImmutableOrOwned(ObjectReference)`
- `SharedMoveObject<T>`: typed handle for `Input::Shared(SharedInput)`
- `ReceivingMoveObject<T>`: typed handle for `Input::Receiving(ObjectReference)`

Note: `ToCallArg` can fail even when BCS encoding is not involved (for example, higher layers can
refuse to convert tombstoned handles or invalid owner kinds into object inputs).

## Receiving is an input mode (not ownership)

Sui's “receiving” is a distinct **transaction input mode**. It corresponds to the Move framework
type `sui::transfer::Receiving<T>`: an ephemeral per-transaction “receiving ticket” consumed by
`sui::transfer::receive`/`public_receive`.

It is not an on-chain owner kind, and this crate does not attempt to prove that a given reference
is valid to receive. It only models the correct wire shape (`Input::Receiving(ObjectReference)`).

## Argument mapping

This crate keeps the user-facing API small, and maps typed values into Sui's on-chain input kinds:

- `T: MoveType` → `CallArg::Pure(bcs(T))`
- `MoveObject<T>` → `CallArg::ImmutableOrOwned(..)`
- `SharedMoveObject<T>` → `CallArg::Shared(..)`
- `ReceivingMoveObject<T>` → `CallArg::Receiving(..)`

For Move `&mut` parameters, use `CallSpec::push_arg_mut` (or implement `ToCallArgMut` on your own
handle type). This matters for shared objects: Sui's shared input encodes mutability in the
transaction input itself.

These are intentionally separate wrapper types because the on-chain input shapes differ:
shared objects are described by `(id, initial_shared_version, mutability)`, while
immutable/owned and receiving inputs use full `ObjectReference`s.

If you need an input kind that doesn't have a typed wrapper here (for example
`CallArg::FundsWithdrawal(..)`), use `CallSpec::push_input`.

## Example: a typed interface function

The typical pattern is to write small interface functions that produce a `CallSpec`:

```rust
use std::str::FromStr;
use sui_move::prelude::*;
use sui_move_call::{CallSpec, MoveObject};
use sui_sdk_types::{Address, Digest, ObjectReference, TypeTag};

#[sui_move::move_struct(address = "0x2", module = "object", abilities = "store")]
pub struct UID {
    pub id: u64,
}

#[sui_move::move_struct(address = "0x1", module = "vault", abilities = "key")]
pub struct Vault {
    pub id: UID,
}

pub fn withdraw(vault: &MoveObject<Vault>, amount: u64) -> CallSpec {
    let mut spec = CallSpec::new(
        Address::from_str("0x1").expect("address"),
        "vault",
        "withdraw",
    )
    .expect("valid identifiers");

    spec.push_type_arg::<u64>();
    spec.push_arg(vault).expect("arg");
    spec.push_arg(&amount).expect("arg");
    spec
}

fn main() {
    let package = Address::from_str("0x1").unwrap();
    let obj_ref = ObjectReference::new(package, 1, Digest::default());
    let vault = MoveObject::<Vault>::new(obj_ref);

    let spec = withdraw(&vault, 10);
    assert_eq!(spec.module.to_string(), "vault");
    assert_eq!(spec.function.to_string(), "withdraw");
    assert_eq!(spec.type_arguments, vec![TypeTag::U64]);
    assert_eq!(spec.arguments.len(), 2);
}
```

## Example: shared and receiving arguments

```rust
use std::str::FromStr;
use sui_move_call::{CallArg, CallSpec, ReceivingMoveObject, SharedMoveObject};
use sui_sdk_types::{Address, Digest, ObjectReference};

#[sui_move::move_struct(address = "0x2", module = "object", abilities = "store")]
struct UID {
    id: u64,
}

#[sui_move::move_struct(address = "0x1", module = "demo", abilities = "key")]
struct Thing {
    id: UID,
}

let package = Address::from_str("0x1").unwrap();

let shared = SharedMoveObject::<Thing>::immutable(Address::from_str("0x2").unwrap(), 1);
let recv_ref = ObjectReference::new(Address::from_str("0x3").unwrap(), 1, Digest::default());
let receiving = ReceivingMoveObject::<Thing>::new(recv_ref);

let mut spec = CallSpec::new(package, "demo", "run").unwrap();
spec.push_arg(&shared).unwrap();
spec.push_arg(&receiving).unwrap();

assert!(matches!(spec.arguments[0], CallArg::Shared(_)));
assert!(matches!(spec.arguments[1], CallArg::Receiving(_)));
```

## Non-goals

- No transaction building: this crate does not produce `ProgrammableTransaction`.
- No execution/runtime: this crate does not talk to RPC or submit transactions.
- No object fetching: object contents are not loaded or decoded here.
