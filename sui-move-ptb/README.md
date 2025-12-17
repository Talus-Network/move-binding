# sui-move-ptb

Programmable-transaction building blocks for typed Move calls on Sui.

This crate sits on top of [`sui-move-call`](../sui-move-call/README.md) and solves one problem:
**turn typed Move call descriptions into a `sui_sdk_types::ProgrammableTransaction`** while
hiding input/argument indexing.

## Where it fits

- `sui-move`: Move-shaped types (`MoveType`, `MoveStruct`, abilities)
- `sui-move-call`: typed call descriptions (`CallSpec`, typed input wrappers)
- `sui-move-ptb`: build PTBs from `CallSpec` (this crate)

## The problem this crate solves

On Sui, a programmable transaction (PTB) is structurally:

- `inputs: Vec<Input>`: a table of inputs (pure bytes, object refs, shared inputs, …)
- `commands: Vec<Command>`: a list of commands

Commands do **not** embed inputs directly. Instead, they refer to them by index using
`Argument::Input(u16)`. They can also refer to prior command results using `Argument::Result(u16)`
and `Argument::NestedResult(u16, u16)`.

`sui-move-call::CallSpec` already gives you a typed way to describe a Move call target and its
arguments, but it still contains the *inputs themselves* (`Vec<Input>`), not `Argument` indices.

This crate is the tiny “allocation” layer that:

1. collects inputs into a single PTB input table,
2. turns each `Input` into an `Argument::Input(index)`,
3. emits `Command::MoveCall` and other native PTB commands,
4. returns the finished `ProgrammableTransaction`.

## Design principles

- **Canonical wire types**: this crate uses `sui_sdk_types::{Input, Command, ProgrammableTransaction}`
  (via `sui_move_call::CallArg`) instead of re-modeling them.
- **Minimal surface**: one builder type (`PtbBuilder`) plus a small set of command helpers.
- **No runtime**: it only builds PTBs; signing/submission belongs in a higher layer.

## Input reuse (dedup)

Because inputs live in a shared table, it’s common to reference the same input multiple times
(especially the same object handle across multiple Move calls). `PtbBuilder` reuses identical
inputs to keep PTBs compact and make reuse natural.

Additional rules for object inputs:

- Shared inputs are unified by `(object_id, initial_shared_version)` and upgraded to the most
  permissive mutability mode when the same shared object is added multiple times.
- Duplicate object ids across input objects and receiving objects are rejected early (matching
  Sui’s `DuplicateObjectRefInput` class of errors).

Exception: `Input::FundsWithdrawal` is intentionally **not** deduplicated (duplicates can be
meaningful).

## Core API

- `PtbBuilder`: accumulates inputs and commands, and can `call(CallSpec)` to add `Command::MoveCall`
- `ptb`: convenience function that runs a closure with a fresh `PtbBuilder` and returns the
  finished `ProgrammableTransaction`
- `ptb!`: macro wrapper around `ptb(...)` for concise call-site syntax

## Example: build a PTB from `CallSpec`

```rust
use std::str::FromStr;
use sui_move_call::{CallSpec, MoveObject};
use sui_move_ptb::ptb;
use sui_sdk_types::{Address, Digest, ObjectReference};

#[sui_move::move_struct(address = "0x1", module = "vault", abilities = "key")]
struct Vault {
    id: sui_move::types::UID,
}

fn withdraw(package: Address, vault: &MoveObject<Vault>, amount: u64) -> CallSpec {
    let mut spec = CallSpec::new(package, "vault", "withdraw").unwrap();
    spec.push_type_arg::<u64>();
    spec.push_arg(vault).unwrap();
    spec.push_arg(&amount).unwrap();
    spec
}

let package = Address::from_str("0x1").unwrap();
let obj_ref = ObjectReference::new(package, 1, Digest::default());
let vault = MoveObject::<Vault>::new(obj_ref);

let pt = ptb(|tx| {
    tx.call(withdraw(package, &vault, 10))?;
    Ok(())
})
.unwrap();

assert_eq!(pt.inputs.len(), 2);
assert_eq!(pt.commands.len(), 1);
```

## Example: input reuse across calls

Using the same typed handle multiple times produces a single PTB input (reused by index):

```rust
use std::str::FromStr;
use sui_move_call::{CallSpec, MoveObject};
use sui_move_ptb::ptb;
use sui_sdk_types::{Address, Digest, ObjectReference};

#[sui_move::move_struct(address = "0x1", module = "vault", abilities = "key")]
struct Vault {
    id: sui_move::types::UID,
}

fn touch(package: Address, vault: &MoveObject<Vault>, amount: u64) -> CallSpec {
    let mut spec = CallSpec::new(package, "vault", "touch").unwrap();
    spec.push_arg(vault).unwrap();
    spec.push_arg(&amount).unwrap();
    spec
}

let package = Address::from_str("0x1").unwrap();
let obj_ref = ObjectReference::new(package, 1, Digest::default());
let vault = MoveObject::<Vault>::new(obj_ref);

let pt = ptb(|tx| {
    tx.call(touch(package, &vault, 1))?;
    tx.call(touch(package, &vault, 2))?;
    Ok(())
})
.unwrap();

// inputs: vault-object, 1u64, 2u64 (vault input is reused)
assert_eq!(pt.inputs.len(), 3);
assert_eq!(pt.commands.len(), 2);
```

## Example: use the `ptb!` macro

```rust
use std::str::FromStr;
use sui_move_call::CallSpec;
use sui_sdk_types::Address;

let package = Address::from_str("0x1").unwrap();
let spec = CallSpec::new(package, "m", "f").unwrap();

let pt = sui_move_ptb::ptb! {
    spec;
}
.unwrap();

assert_eq!(pt.commands.len(), 1);
```

## Non-goals

- No execution/runtime: this crate does not submit transactions.
- No object fetching/decoding: it only wires inputs and commands.
