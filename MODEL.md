# Sui Model (Client-Side) and move-binding Mental Model

This repository builds an abstraction on top of `sui-rust-sdk` that is intentionally aligned with
Sui’s observable client-side semantics.

## Sui in one page: versioned objects + effects

On Sui, most state is carried by **objects** identified by an object id.

An object has a **version** (and a digest). Many transactions must name the exact object version
they intend to use.

When a transaction executes, Sui returns **effects** describing how object versions changed:

- some objects are written (new version/digest, possibly new owner),
- some objects are deleted or wrapped,
- new objects can be created or unwrapped.

You can think of this as “the frontier advances”: each successful execution produces a new set of
latest live versions for the objects it touched.

## Two separate concepts: on-chain owner vs transaction input mode

Sui has both:

### 1) On-chain owner kinds (what the object *is* right now)

From `sui_sdk_types::Owner`:

- `Immutable`
- `Address(owner)` (address-owned, mutable)
- `Object(owner_object_id)` (child object; object-owned)
- `Shared(initial_shared_version)`
- `ConsensusAddress { start_version, owner }` (address-owned but sequenced via consensus)

### 2) Transaction input modes (how you pass an object *this time*)

From `sui_sdk_types::Input` (BCS encodings are part of the protocol):

- `ImmutableOrOwned(ObjectReference)` — full `(id, version, digest)`
- `Shared { object_id, initial_shared_version, mutable }`
- `Receiving(ObjectReference)` — a distinct per-transaction input mode

Important: **“Receiving” is not an owner kind.** It corresponds to the Move framework concept
`sui::transfer::Receiving<T>`: an ephemeral per-transaction “ticket” used by `sui::transfer::receive`.

## The move-binding mental model: Read → Tx → Commit

This repo’s top-level abstraction is a single, explicit workflow:

1) **Read**: fetch the current state you need (object references, owner kinds, optionally contents).
2) **Tx**: build a transaction proposal (typically a PTB) using typed handles.
3) **Commit**: execute the proposal and obtain a **receipt** with digest + effects + finality info.

### Cursor: local view of the frontier

`move-binding` maintains a **Cursor**: a small local data structure that remembers the latest
object references and owner kinds that *you* observed and/or produced via your own commits.

After commit, the cursor advances by applying an **effects patch** derived from the transaction’s
effects.

Key properties:

- Cursor advancement is **effects-driven** (no hidden background syncing).
- Cursor represents **local knowledge**: external transactions can invalidate it.
- Cursor can **tombstone** objects it sees as deleted/wrapped to fail early on stale handles.

## Finality (requested vs observed)

Execution and checkpoint inclusion are distinct:

- a transaction can be **executed** (effects exist),
- later it can be **checkpointed** (indexes are updated; “read-your-writes” on that node).

RPC calls may time out waiting for checkpoint inclusion even though execution succeeded. A correct
client abstraction must preserve recovery information (digest/effects when available) and represent
finality explicitly rather than as a boolean.

