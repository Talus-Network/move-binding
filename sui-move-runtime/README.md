# sui-move-runtime

Runtime layer for typed Move interactions on Sui.

This crate sits at the top of this stack:

- [`sui-move`](../sui-move/README.md): Move-shaped types (`MoveType`, `MoveStruct`, abilities)
- [`sui-move-call`](../sui-move-call/README.md): typed call descriptions (`CallSpec`) and `ToCallArg`
- [`sui-move-ptb`](../sui-move-ptb/README.md): build a `ProgrammableTransaction` (PTB) from `CallSpec`
- `sui-move-runtime` (this crate): submit/simulate/inspect PTBs + keep runtime-owned handles up to date

This crate solves one problem:
**provide an ergonomic Read → Tx → Commit boundary for typed Sui interactions** while staying
truthful to Sui’s “versioned objects + effects” model (`MODEL.md`):

- In **Read** you fetch objects and prepare call specs.
- In **Tx** you build a PTB and can simulate / dev-inspect / commit it.
- On **commit**, you always get a `Receipt` (digest + effects/status when available + finality
  info), and the runtime advances its local cursor by applying an effects-derived patch whenever
  effects are present.

## Quickstart (end-to-end)

```rust,no_run
use sui_move_runtime::prelude::*;
use sui_sdk_types::{Address, PersonalMessage, Transaction, UserSignature};

# #[sui_move::move_struct(address = "0x2", module = "object", abilities = "copy, drop, store")]
# struct ID { bytes: Address }
# #[sui_move::move_struct(address = "0x2", module = "object", abilities = "store")]
# struct UID { id: ID }
# #[sui_move::move_struct(address = "0x1", module = "demo", abilities = "key")]
# struct DemoCoin { id: UID }
#
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
    let coin: Object<DemoCoin> = rt.read().object::<DemoCoin>(coin_id).await?;

    // Tx: one-shot build + commit with the `tx!` macro.
    let receipt = sui_move_runtime::tx!(&mut rt, sender => {
        touch_coin(&coin, 10);
    })
    .await?;

    // On-chain execution failures are recorded in the receipt (they are not transport errors).
    receipt.ensure_success()?;

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
- Ergonomic helper:
  - `tx!`: macro that builds a `Tx`, runs an action, and returns a future (`.await`)
- Handles (all implement `ToCallArg`):
  - `Object<T>`: runtime-owned object handle; picks the correct input mode on conversion (shared defaults to immutable)
  - `SharedObject<T>`: explicit shared input (`Input::Shared`)
  - `ReceivingObject<T>`: explicit receiving input (`Input::Receiving`)
- Transaction actions:
  - `commit`: signs/submits and then updates handles (waits for checkpoint inclusion by default)
  - `simulate`: checks enabled, no mutation, no handle updates
  - `inspect`: checks disabled, returns command outputs, no handle updates

## Preflight and debugging (simulate / inspect)

If you want a one-shot action, use the `tx!` macro variants:

```rust,no_run
use sui_move_runtime::prelude::*;
use sui_sdk_types::Address;

# #[sui_move::move_struct(address = "0x2", module = "object", abilities = "copy, drop, store")]
# struct ID { bytes: Address }
# #[sui_move::move_struct(address = "0x2", module = "object", abilities = "store")]
# struct UID { id: ID }
# #[sui_move::move_struct(address = "0x1", module = "demo", abilities = "key")]
# struct DemoCoin { id: UID }
# async fn demo(mut rt: Runtime<impl sui_crypto::SuiSigner>, sender: Address, coin: Object<DemoCoin>) -> Result<(), Error> {
let _sim = sui_move_runtime::tx!(simulate, &mut rt, sender => {
    CallSpec::new("0x1".parse().unwrap(), "m", "f").unwrap();
})
.await?;

let _dbg = sui_move_runtime::tx!(inspect, &mut rt, sender => {
    CallSpec::new("0x1".parse().unwrap(), "m", "f").unwrap();
})
.await?;
# let _ = coin;
# Ok(())
# }
```

If you need to simulate/inspect the **exact same PTB** before committing it, build a `Tx` once:

```rust,no_run
use sui_move_runtime::prelude::*;

# async fn demo(mut rt: Runtime<impl sui_crypto::SuiSigner>, sender: sui_sdk_types::Address) -> Result<(), Error> {
let mut tx = rt.tx(sender);
tx.call(CallSpec::new("0x1".parse().unwrap(), "m", "f").unwrap())?;

let _sim = tx.simulate().await?;
let _dbg = tx.inspect().await?;
let receipt = tx.commit().await?;
receipt.ensure_success()?;
# Ok(())
# }
```

## Choosing the right handle kind

Sui has two related but distinct concepts:

- **On-chain owner kinds** (`sui_sdk_types::Owner`): what an object *is* right now.
- **Transaction input modes** (`sui_sdk_types::Input`): how you pass an object *this time*.

This crate uses on-chain ownership (from `Owner`) to choose a safe default transaction input mode
when converting an [`Object<T>`] into an argument.

Transaction input modes for objects have different wire shapes and are intentionally represented as
distinct types:

- Immutable/owned: `Input::ImmutableOrOwned(ObjectReference)`
- Shared: `Input::Shared(SharedInput)` (uses `initial_shared_version` + mutability)
- Receiving: `Input::Receiving(ObjectReference)` (transaction input mode for `sui::transfer::Receiving<T>`)

Note: receiving is not an on-chain owner kind. It is an ephemeral per-transaction “receiving
ticket” consumed by `sui::transfer::receive`/`public_receive`.

Note: Sui also has `Owner::ConsensusAddress { start_version, owner }` objects. They use the
shared-like input shape (`start_version` plays the same role as `initial_shared_version`).

Note: child objects (`Owner::Object(_)`) cannot be used as direct transaction inputs. This crate
rejects them early when you try to construct handles that would later become invalid inputs.

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

#[sui_move::move_struct(address = "0x2", module = "object", abilities = "copy, drop, store")]
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

fn views(obj: Object<Demo>) -> Result<(), sui_move_call::CallArgError> {
    let _shared_mut: SharedObject<Demo> = obj.shared_mutable()?;
    let _receiving: ReceivingObject<Demo> = obj.receiving()?;
    Ok(())
}
```

## Transaction actions in detail

All transaction actions take a sender address and are methods on a transaction builder (`Tx`):

- `call` / `arg` / `input`: build the PTB in-place.
- `simulate`: calls `simulate_transaction` with checks enabled. No signature is required and the
  chain is not mutated. Handles are not updated.
- `inspect`: calls `simulate_transaction` with checks disabled and asks RPC for
  `command_outputs`. This is meant for debugging and observability, not for guaranteeing that a
  real on-chain commit will succeed.
- `commit`: builds a full `Transaction`, signs it, and submits it.
  - By default it also waits for checkpoint inclusion (`TxOptions::finality = Checkpointed`).
  - When execution-only finality is requested (`TxOptions::finality = Executed`), the runtime
    returns as soon as the transaction is executed (effects produced).
  - In both cases it requests `effects.bcs` so the runtime can decode `TransactionEffects` and
    refresh handles.
  - If checkpoint waiting times out (or the checkpoint stream errors), `commit` still returns a
    `Receipt` with `digest` + any decoded effects, and marks the observed finality as `Executed`.

## Receipts, finality, and recovery

`Tx::commit*` returns a [`Receipt`]. The receipt preserves recovery information:

- `digest` is always present once submission succeeds.
- `effects`/`status` are present when returned by RPC (this crate requests `effects.bcs`).
- `requested_finality` is what the runtime asked for (defaults to checkpointed for commits).
- `observed_finality` + `checkpoint_wait` describe what the runtime actually observed.

If checkpoint waiting times out, your transaction may still have executed. When effects are present
in the receipt, the runtime already advanced its cursor before returning the receipt.

If a receipt does not contain effects (for example, because it was persisted without them), you
can recover by digest and advance the cursor explicitly:

```rust,no_run
use sui_move_runtime::prelude::*;

# async fn demo(mut rt: Runtime<impl sui_crypto::SuiSigner>, receipt: Receipt) -> Result<(), Error> {
if receipt.effects.is_none() {
    let _effects = rt.sync_transaction(receipt.digest).await?;
}
Ok(())
# }
```

```rust,no_run
use sui_move_runtime::prelude::*;
use std::time::Duration;
use sui_sdk_types::Address;

# #[derive(Clone)]
# struct DummySigner;
# impl sui_crypto::SuiSigner for DummySigner {
#     fn sign_transaction(&self, _tx: &sui_sdk_types::Transaction) -> Result<sui_sdk_types::UserSignature, sui_crypto::SignatureError> {
#         unimplemented!("provide a real signer")
#     }
#     fn sign_personal_message(&self, _msg: &sui_sdk_types::PersonalMessage<'_>) -> Result<sui_sdk_types::UserSignature, sui_crypto::SignatureError> {
#         unimplemented!("provide a real signer")
#     }
# }
#
# async fn demo() -> Result<(), Error> {
	let client = sui_rpc::Client::new(sui_rpc::Client::TESTNET_FULLNODE).unwrap();
	let signer = DummySigner;
	let sender: Address = "0x123".parse().unwrap();

	let mut rt = Runtime::new(client, signer)
	    .with_wait_timeout(Duration::from_millis(1)); // deliberately tiny to illustrate timeouts

	let mut tx = rt.tx(sender);
	let package: Address = "0x1".parse().unwrap();
	tx.call(CallSpec::new(package, "m", "f").unwrap())?;

let receipt = tx.commit().await?;

match receipt.checkpoint_wait {
    CheckpointWaitOutcome::Ok => {}
    CheckpointWaitOutcome::Timeout | CheckpointWaitOutcome::StreamError { .. } => {
        // `receipt.digest` is known; `receipt.effects` may be present.
        // If effects are present, the cursor has already been advanced.
    }
    CheckpointWaitOutcome::NotRequested => {
        // This can happen when the commit only requests execution finality.
    }
}

receipt.ensure_success()?;
# Ok(())
# }
# let _ = demo;
```

## Building PTBs directly (more complex flows)

If you need native PTB commands (coin ops, transfers, result wiring, etc), build the PTB explicitly
using `sui-move-ptb` and pass it to `commit_ptb` / `simulate_ptb` / `inspect_ptb`.

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

`Tx::commit_with(TxOptions)` and `Tx::commit_ptb_with(ptb, TxOptions)` let you control gas details:

- `TxOptions::sponsor`: gas owner (defaults to sender)
- `TxOptions::gas`: explicit gas object reference (otherwise the runtime selects one coin owned by the gas owner)
- `TxOptions::gas_price`: defaults to the reference gas price from RPC
- `TxOptions::gas_budget`: defaults to `Runtime::default_gas_budget`
- `TxOptions::expiration`: optional TTL
- `TxOptions::finality`: `Checkpointed` (default) or `Executed`

`simulate`/`inspect` do not sign or submit, and do not currently model explicit gas payment
configuration (they rely on the simulation RPC).

## Typed reads (tag-checked decoding)

In addition to constructing handles, `Read` can fetch and decode Move object contents:

- `Read::get::<T>(id) -> (Object<T>, T)`: tag-check + decode and return both a handle and value.
- `Read::decode(&Object<T>) -> T`: refresh the handle and decode the latest contents.
- `*_unchecked` variants skip type-tag verification (explicit escape hatch).

These helpers use `sui-move`’s tag-checked decoding (`MoveInstance<T>`), so “type tag says X but
BCS layout expects Y” becomes an explicit error instead of a silent footgun.

```rust,no_run
use sui_move_runtime::prelude::*;
use sui_sdk_types::Address;

# async fn demo(mut rt: Runtime<impl sui_crypto::SuiSigner>) -> Result<(), Error> {
let coin_id: Address = "0x2".parse().unwrap();

# #[sui_move::move_struct(address = "0x2", module = "object", abilities = "copy, drop, store")]
# struct ID { bytes: Address }
# #[sui_move::move_struct(address = "0x2", module = "object", abilities = "store")]
# struct UID { id: ID }
# #[sui_move::move_struct(address = "0x1", module = "demo", abilities = "key")]
# struct DemoCoin { id: UID }

let (coin, value): (Object<DemoCoin>, DemoCoin) = rt.read().get(coin_id).await?;
let _latest: DemoCoin = rt.read().decode(&coin).await?;
let _unchecked: DemoCoin = rt.read().decode_unchecked(&coin).await?;
# let _ = value;
# Ok(())
# }
```

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
- If other transactions mutate an object you track, use `Read::refresh(&obj)` (or
  `Read::refresh_id` / `Read::refresh_ids`) to refetch the latest reference/owner and overwrite the
  cursor’s slot.

### Storing handles in Rust structs

The point of runtime-owned handles is that you can store them in normal Rust state without
threading `&mut ObjectReference` everywhere.

```rust,no_run
use sui_move_runtime::prelude::*;
use sui_sdk_types::Address;

# #[sui_move::move_struct(address = "0x2", module = "object", abilities = "copy, drop, store")]
# struct ID { bytes: Address }
# #[sui_move::move_struct(address = "0x2", module = "object", abilities = "store")]
# struct UID { id: ID }
# #[sui_move::move_struct(address = "0x1", module = "demo", abilities = "key")]
# struct DemoCoin { id: UID }

#[derive(Clone)]
struct Wallet {
    coin: Object<DemoCoin>,
}

# async fn demo(mut rt: Runtime<impl sui_crypto::SuiSigner>, sender: sui_sdk_types::Address) -> Result<(), Error> {
let wallet = Wallet {
    coin: rt.read().object("0x2".parse().unwrap()).await?,
};

let ptb = sui_move_ptb::ptb! {
    // any call that mutates `wallet.coin` on-chain
    CallSpec::new("0x1".parse().unwrap(), "m", "f").unwrap();
}?; 
rt.tx(sender).commit_ptb(ptb).await?;

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
- `Runtime::with_wait_timeout` controls how long `commit*` waits for checkpoint inclusion when
  checkpointed finality is requested.
- `TxOptions::sponsor` lets you submit sponsored transactions (gas owner differs from sender).
- `TxOptions::finality` chooses whether commits request `Checkpointed` or `Executed` finality.
- `Runtime::with_cursor_snapshot` / `Runtime::cursor_snapshot` provide snapshot/restore for the cursor.
- `Runtime::sync_transaction` fetches effects by digest and advances the cursor (recovery escape hatch).
- `Read::refresh_id` / `Read::refresh_ids` refresh cursor state explicitly (external drift escape hatch).
- `Read::client_mut` gives direct access to the underlying `sui_rpc::Client` when needed.

## Non-goals

- No code generation: interface functions are still handwritten or derived elsewhere.
- No “live ORM”: decoding is explicit snapshot reads (`Read::get` / `Read::decode`), not a background-syncing cache.
