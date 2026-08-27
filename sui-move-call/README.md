# talus-sui-move-call

Typed building blocks for describing Move calls on Sui.

The crates.io package is `talus-sui-move-call`; Rust code imports it as `talus_sui_move_call`.

This crate builds on top of [`talus-sui-move`](https://docs.rs/talus-sui-move) and solves one problem:
**describe a Move call in a typed way** (object handles + type arguments + arguments) without
building or executing transactions.

## Where it fits

`talus-sui-move-call` is the “Call” layer in the repository’s
[Read → Tx → Commit model](https://github.com/Talus-Network/move-binding/blob/main/MODEL.md):

- **Read** (runtime) fetches objects and classifies ownership from chain state.
- **Call** (this crate) describes *what* to call and how to encode arguments.
- **PTB** builds a `ProgrammableTransaction` from a `CallSpec`.
- **Commit** (runtime) submits and applies effects to advance the cursor.

This crate sits directly above `talus-sui-move`: it uses `MoveType` and `MoveStruct` to build call
descriptions (`CallSpec`) whose types are checked by Rust. Transaction building and execution are
intentionally out of scope.

## Core types

- `CallSpec`: `(package, module, function)` + type arguments + call arguments
- `CallArg`: canonical call argument representation (a public alias for `sui_sdk_types::Input`)
- `ToCallArg`: convert values into `CallArg` without consuming them
- `ToCallArgMut`: convert values into `CallArg` for Move `&mut` parameters (shared inputs become
  mutable)
- `ObjectArg<T>`: typed object argument trait used by generated interfaces (accepts any object
  handle that can be encoded as both `&` and `&mut` in Move)
- `MoveObject<T>`: typed handle for `Input::ImmutableOrOwned(ObjectReference)`
- `SharedMoveObject<T>`: typed handle for `Input::Shared(SharedInput)`
- `ReceivingMoveObject<T>`: typed handle for `Input::Receiving(ObjectReference)`

Note: `ToCallArg` can fail even when BCS encoding is not involved (for example, higher layers can
refuse to convert tombstoned handles or invalid owner kinds into object inputs).

## Receiving is an input mode (not ownership)

Sui's “receiving” is a distinct **transaction input mode**. It corresponds to the Move framework
type `sui::transfer::Receiving<T>`: an ephemeral “receiving ticket” consumed by
`sui::transfer::receive`/`public_receive`.

It is not an owner kind recorded on chain, and this crate does not attempt to prove that a given
reference is valid to receive. It only models the correct wire shape
(`Input::Receiving(ObjectReference)`).

## Argument mapping

This crate keeps its public API small and maps typed values into Sui transaction input kinds:

- `T: MoveType` → `CallArg::Pure(bcs(T))`
- `MoveObject<T>` → `CallArg::ImmutableOrOwned(..)`
- `SharedMoveObject<T>` → `CallArg::Shared(..)`
- `ReceivingMoveObject<T>` → `CallArg::Receiving(..)`

For Move `&mut` parameters, use `CallSpec::push_arg_mut` (or implement `ToCallArgMut` on your own
handle type). This matters for shared objects: Sui's shared input encodes mutability in the
transaction input itself.

These are intentionally separate wrapper types because their transaction input shapes differ:
shared objects are described by `(id, initial_shared_version, mutability)`, while
immutable/owned and receiving inputs use full `ObjectReference`s.

If you need an input kind that doesn't have a typed wrapper here (for example
`CallArg::FundsWithdrawal(..)`), use `CallSpec::push_input`.

## Example: a typed interface function

The typical pattern is to write small interface functions that produce a `CallSpec`:

```rust
use std::str::FromStr;
use talus_sui_move::prelude::*;
use talus_sui_move_call::{CallSpec, MoveObject};
use sui_sdk_types::{Address, Digest, ObjectReference, TypeTag};

#[talus_sui_move::move_struct(address = "0x2", module = "object", abilities = "store")]
pub struct UID {
    pub id: u64,
}

#[talus_sui_move::move_struct(address = "0x1", module = "vault", abilities = "key")]
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
use talus_sui_move_call::{CallArg, CallSpec, ReceivingMoveObject, SharedMoveObject};
use sui_sdk_types::{Address, Digest, ObjectReference};

#[talus_sui_move::move_struct(address = "0x2", module = "object", abilities = "store")]
struct UID {
    id: u64,
}

#[talus_sui_move::move_struct(address = "0x1", module = "demo", abilities = "key")]
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

## Out of scope

- No transaction building: this crate does not produce `ProgrammableTransaction`.
- No execution/runtime: this crate does not talk to a network client or submit transactions.
- No object fetching: object contents are not loaded or decoded here.
