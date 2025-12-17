# sui-move-runtime

Runtime layer for typed Move interactions on Sui.

This crate sits at the top of this stack:

- [`sui-move`](../sui-move/README.md): Move-shaped types (`MoveType`, `MoveStruct`, abilities)
- [`sui-move-call`](../sui-move-call/README.md): typed call descriptions (`CallSpec`) and `ToCallArg`
- [`sui-move-ptb`](../sui-move-ptb/README.md): build a `ProgrammableTransaction` (PTB) from `CallSpec`
- `sui-move-runtime` (this crate): submit/simulate/inspect PTBs + keep runtime-owned handles up to date

This crate solves one problem:
**provide an ergonomic Read → Tx → Commit boundary for typed Sui interactions**:

- In **Read** you fetch objects and prepare call specs.
- In **Tx** you dry-run / dev-inspect / commit a PTB.
- On **commit**, all live runtime-owned handles are automatically refreshed from transaction effects.

## Quickstart (end-to-end)

```rust,no_run
use sui_move_runtime::prelude::*;
use sui_move::{coin::Coin, sui::SUI};
use sui_sdk_types::{Address, PersonalMessage, Transaction, UserSignature};

# #[derive(Clone)]
# struct DummySigner;
# impl sui_crypto::SuiSigner for DummySigner {
#     fn sign_transaction(&self, _tx: &Transaction) -> Result<UserSignature, sui_crypto::SignatureError> {
#         unimplemented!("provide a real signer (keypair, wallet, kms, ...)")
#     }
#     fn sign_personal_message(&self, _msg: &PersonalMessage<'_>) -> Result<UserSignature, sui_crypto::SignatureError> {
#         unimplemented!("provide a real signer (keypair, wallet, kms, ...)")
#     }
# }

fn touch_coin(coin: &impl ToCallArg, amount: u64) -> CallSpec {
    let package: Address = "0x1".parse().unwrap();
    let mut spec = CallSpec::new(package, "demo", "touch").unwrap();
    spec.push_arg(coin).unwrap();
    spec.push_arg(&amount).unwrap();
    spec
}

async fn demo() -> Result<(), Error> {
    let client = sui_rpc::Client::new(sui_rpc::Client::TESTNET_FULLNODE).unwrap();
    let signer = DummySigner;
    let sender: Address = "0x123".parse().unwrap();
    let coin_id: Address = "0x2".parse().unwrap();

    let mut rt = Runtime::new(client, signer);

    // Read: fetch a typed runtime-owned handle.
    let coin: Object<Coin<SUI>> = rt.read().object::<Coin<SUI>>(coin_id).await?;

    // Tx: preflight (checks enabled, no chain mutation).
    let ptb = sui_move_ptb::ptb! { touch_coin(&coin, 10); }?;
    let _sim = rt.tx(sender).simulate(ptb).await?;

    // Tx: inspect (checks disabled, returns per-command outputs).
    let ptb = sui_move_ptb::ptb! { touch_coin(&coin, 10); }?;
    let _dbg = rt.tx(sender).inspect(ptb).await?;

    // Tx: commit (mutates chain, updates all live handles from effects).
    let ptb = sui_move_ptb::ptb! { touch_coin(&coin, 10); }?;
    rt.tx(sender).commit(ptb).await?;

    // Back in Read: `coin`'s `ObjectReference` has been updated internally.
    let _latest_ref = coin.reference();
    Ok(())
}
# let _ = demo;
```

## Core API

- Runtime views:
  - `Runtime`: owns RPC client, signer, and the handle cursor
  - `Read`: read view (fetch typed handles)
  - `Tx`: transaction view (simulate/inspect/commit PTBs)
- Handles (all implement `ToCallArg`):
  - `Object<T>`: runtime-owned object handle; picks the correct input mode on conversion (shared defaults to immutable)
  - `SharedObject<T>`: explicit shared input (`Input::Shared`)
  - `ReceivingObject<T>`: explicit receiving input (`Input::Receiving`)
- Transaction actions:
  - `commit`: signs/submits/waits and then updates handles
  - `simulate`: checks enabled, no mutation, no handle updates
  - `inspect`: checks disabled, returns command outputs, no handle updates

## Choosing the right handle kind

Sui has multiple *input kinds* for objects, and they are intentionally represented as distinct
types (because they have different wire shapes):

- Immutable/owned: `Input::ImmutableOrOwned(ObjectReference)`
- Shared: `Input::Shared(SharedInput)` (uses `initial_shared_version` + mutability)
- Receiving: `Input::Receiving(ObjectReference)` (transaction input mode for `sui::transfer::Receiving<T>`)

Note: receiving is not an on-chain owner kind. It is an ephemeral per-transaction “receiving
ticket” consumed by `sui::transfer::receive`/`public_receive`.

Note: Sui also has `Owner::ConsensusAddressOwner` objects. They use the shared-like input shape
(`start_version` plays the same role as `initial_shared_version`).

This crate mirrors those shapes:

- Use `Read::object::<T>(id)` to get an `Object<T>` for owned/immutable and shared-like objects.
  - Shared-like objects default to immutable shared when used as an argument.
  - Derive explicit views when needed: `obj.shared_immutable()?`, `obj.shared_mutable()?`, `obj.receiving()?`.

### Explicit views from `Object<T>`

`Object<T>` chooses `Input::ImmutableOrOwned` vs `Input::Shared` based on the runtime’s latest
known owner kind. When an object is shared-like, it defaults to **immutable shared**.

If you need a specific input mode, derive an explicit view at the moment it matters:

```rust,no_run
use sui_move_runtime::prelude::*;

#[sui_move::move_struct(address = "0x1", module = "demo", abilities = "key")]
struct Demo {
    id: sui_move::types::UID,
}

fn views(obj: Object<Demo>) -> Result<(), sui_move_call::CallArgError> {
    let _shared_mut: SharedObject<Demo> = obj.shared_mutable()?;
    let _receiving: ReceivingObject<Demo> = obj.receiving()?;
    Ok(())
}
```

## Transaction actions in detail

All transaction actions take a sender address, and all operate on a PTB:

- `commit`: builds a full `Transaction`, signs it, submits it, and waits for checkpoint inclusion.
  It requests `effects.bcs` so the runtime can decode `TransactionEffects` and refresh handles.
  If checkpoint waiting times out (or the checkpoint stream errors), `commit` still returns a
  `Receipt` with `digest` + any decoded effects, and marks the finality as observed `Executed`.
- `simulate`: calls `simulate_transaction` with checks enabled. No signature is required and the
  chain is not mutated. Handles are not updated.
- `inspect`: calls `simulate_transaction` with checks disabled and asks RPC for
  `command_outputs`. This is meant for debugging and observability, not for guaranteeing that a
  real on-chain commit will succeed.

## Building PTBs directly (more complex flows)

If you need native PTB commands (coin ops, transfers, result wiring, etc), build the PTB explicitly
using `sui-move-ptb` and pass it to `commit` / `simulate` / `inspect`.

```rust
use sui_move_runtime::prelude::*;
use sui_sdk_types::Address;

let package: Address = "0x1".parse().unwrap();
let spec = CallSpec::new(package, "m", "f").unwrap();

let ptb = ptb(|tx| {
    tx.call(spec)?;
    Ok(())
})
.unwrap();

assert_eq!(ptb.commands.len(), 1);
```

## Gas and sponsorship (commit only)

`commit_with(ptb, TxOptions)` lets you control gas details:

- `TxOptions::sponsor`: gas owner (defaults to sender)
- `TxOptions::gas`: explicit gas object reference (otherwise the runtime selects one coin owned by the gas owner)
- `TxOptions::gas_price`: defaults to the reference gas price from RPC
- `TxOptions::gas_budget`: defaults to `Runtime::default_gas_budget`
- `TxOptions::expiration`: optional TTL

`simulate`/`inspect` do not sign or submit, and do not currently model explicit gas payment
configuration (they rely on the simulation RPC).

## Writing interface functions (important)

To keep your Move interface layer usable in every context (unit tests, pure PTB building, runtime),
write interface functions against `ToCallArg` instead of concrete handle types:

```rust
use sui_move_runtime::prelude::*;
use sui_sdk_types::Address;

fn transfer_any_object(obj: &impl ToCallArg, recipient: Address) -> CallSpec {
    let package: Address = "0x1".parse().unwrap();
    let mut spec = CallSpec::new(package, "demo", "transfer").unwrap();
    spec.push_arg(obj).unwrap();
    spec.push_arg(&recipient).unwrap();
    spec
}
```

This works with:

- `sui_move_call::MoveObject<T>` (a plain wrapper around `ObjectReference`), and
- `sui_move_runtime::Object<T>` (a runtime-owned handle that auto-updates on commit).

## How runtime-owned handles work

On Sui, mutating an object changes its `ObjectReference` (version/digest). Updating these refs
manually is annoying and tends to leak plumbing into user code.

This crate makes handles *runtime-owned*:

- `Read::object`/`Read::receiving_object` fetch the current `ObjectReference` and **intern** it in
  a cursor (your local frontier) keyed by `object_id`.
- The returned `Object<T>` / `ReceivingObject<T>` is a small `Clone` handle backed by `Arc<RwLock<...>>`.
- `Tx::commit` requests `effects.bcs` from RPC, decodes `TransactionEffects`, extracts updated
  object information, derives an effects-based patch, and applies it to the cursor, updating any
  live handle cells that match those object ids.

Consequences:

- Clones of the same handle stay in sync (they share the same cell).
- Only commits performed through the same `Runtime` advance the cursor.
- `simulate`/`inspect` never update handles (they do not mutate the chain).
- If other transactions mutate an object you track, use `Read::refresh(&obj)` to refetch the latest
  reference/owner and overwrite the cursor’s slot.

### Storing handles in Rust structs

The point of runtime-owned handles is that you can store them in normal Rust state without
threading `&mut ObjectReference` everywhere.

```rust,no_run
use sui_move_runtime::prelude::*;
use sui_move::{coin::Coin, sui::SUI};

#[derive(Clone)]
struct Wallet {
    coin: Object<Coin<SUI>>,
}

# async fn demo(mut rt: Runtime<impl sui_crypto::SuiSigner>, sender: sui_sdk_types::Address) -> Result<(), Error> {
let wallet = Wallet {
    coin: rt.read().object("0x2".parse().unwrap()).await?,
};

let ptb = sui_move_ptb::ptb! {
    // any call that mutates `wallet.coin` on-chain
    CallSpec::new("0x1".parse().unwrap(), "m", "f").unwrap();
}?; 
rt.tx(sender).commit(ptb).await?;

// The `ObjectReference` is refreshed internally after commit.
let _latest = wallet.coin.reference();
# Ok(())
# }
```

## Outputs from `inspect` / decoding BCS

Dev-inspect returns per-command outputs as raw BCS blobs (`BcsValue`). This crate does not impose a
new typing layer on top of those blobs, but you can decode them using `sui_sdk_types::bcs`:

```rust
use sui_move_runtime::BcsValue;
use sui_sdk_types::bcs::{FromBcs, ToBcs};

let bytes = 10u64.to_bcs().unwrap();
let value = BcsValue {
    name: Some("u64".to_owned()),
    bytes,
};

let decoded = u64::from_bcs(&value.bytes).unwrap();
assert_eq!(decoded, 10);
```

## Configuration and escape hatches

- `Runtime::with_default_gas_budget` and `TxOptions::gas_budget` configure gas budget for commits.
- `TxOptions::sponsor` lets you submit sponsored transactions (gas owner differs from sender).
- `Read::client_mut` gives direct access to the underlying `sui_rpc::Client` when needed.

## Non-goals

- No code generation: interface functions are still handwritten or derived elsewhere.
- No “full ORM”: fetched contents are not modeled as live decoded Rust structs (this crate focuses on handles + execution).
