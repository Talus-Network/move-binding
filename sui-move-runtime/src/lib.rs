#![doc = include_str!("../README.md")]
#![deny(missing_docs)]

mod effects;
mod handles;
mod tx;

pub use crate::effects::TombstoneReason;
pub use crate::handles::{CursorSnapshot, Object, ReceivingObject, SharedObject};
pub use crate::tx::{
    BcsValue, CheckpointWaitOutcome, CommandOutputs, EnsureSuccessError, Finality, InspectOptions,
    InspectReceipt, ObservedFinality, Receipt, SimulateOptions, SimulationReceipt, TxOptions,
};

use std::time::Duration;

/// Re-export of `sui_crypto::SuiSigner` used by [`Runtime`] and [`Tx`].
///
/// This lets generated bindings name the signer bound as `sui_move_runtime::SuiSigner` without
/// requiring consumers to depend on `sui-crypto` directly.
pub use sui_crypto::SuiSigner;
use sui_sdk_types::{Address, Mutability, ProgrammableTransaction, TransactionEffects, TypeTag};

/// Errors produced by `sui-move-runtime`.
///
/// This is a small umbrella enum that preserves the main failure boundary:
/// - building a PTB (`Build`)
/// - submitting/waiting (`Tx`)
/// - fetching objects for handles (`Rpc`)
/// - simulation/dev-inspection (`Simulate`)
#[derive(thiserror::Error, Debug)]
pub enum Error {
    /// Building a PTB failed.
    #[error(transparent)]
    Build(#[from] sui_move_ptb::BuildError),

    /// Signing or submitting failed.
    #[error(transparent)]
    Tx(#[from] tx::TxError),

    /// Fetching data from RPC failed.
    #[error(transparent)]
    Rpc(#[from] tx::RpcError),

    /// Simulating or dev-inspecting failed.
    #[error(transparent)]
    Simulate(#[from] tx::SimulateError),

    /// The transaction executed with failure.
    #[error(transparent)]
    Execution(#[from] EnsureSuccessError),

    /// Decoding Move contents failed.
    #[error("decode object {object_id}: {source}")]
    Decode {
        /// Object id that was being decoded.
        object_id: Address,
        /// Underlying verification/BCS error.
        #[source]
        source: sui_move::DecodeError,
    },

    /// The requested object kind does not match on-chain ownership.
    #[error("object {object_id} is {actual}, expected {expected}")]
    ObjectKind {
        /// Object id that was fetched.
        object_id: Address,
        /// Expected kind (for the API used).
        expected: &'static str,
        /// Actual kind from RPC.
        actual: &'static str,
    },
}

/// Long-lived runtime owning RPC + signer + handle cursor.
///
/// This is the entry point for the Read → Tx → Commit mental model:
/// - [`Runtime::read`] for fetching/constructing typed handles
/// - [`Runtime::tx`] for simulating/inspecting/committing PTBs
///
/// # Runtime-owned handles
///
/// Handles returned by [`Read::object`] and [`Read::receiving_object`] are **interned** in the
/// runtime’s cursor (your local frontier) by `object_id`. Clones of the same handle share the same
/// internal cell, so they all see updates.
///
/// After [`Tx::commit`], the runtime decodes `TransactionEffects` from RPC, derives an
/// effects-based patch, and applies it to the cursor, updating any live handle cells whose object
/// id appears in the effects.
///
/// This is the core ergonomic win: you can store typed handles in normal Rust structs without
/// threading `&mut` everywhere just to keep `ObjectReference`s current.
///
/// # Example
/// ```rust,no_run
/// use sui_move_runtime::prelude::*;
/// use sui_sdk_types::{PersonalMessage, Transaction, UserSignature};
///
/// # #[derive(Clone)]
/// # struct DummySigner;
/// # impl sui_crypto::SuiSigner for DummySigner {
/// #     fn sign_transaction(&self, _tx: &Transaction) -> Result<UserSignature, sui_crypto::SignatureError> {
/// #         unimplemented!("provide a real signer")
/// #     }
/// #     fn sign_personal_message(&self, _msg: &PersonalMessage<'_>) -> Result<UserSignature, sui_crypto::SignatureError> {
/// #         unimplemented!("provide a real signer")
/// #     }
/// # }
///
/// let client = sui_rpc::Client::new(sui_rpc::Client::TESTNET_FULLNODE).unwrap();
/// let signer = DummySigner;
/// let mut rt = Runtime::new(client, signer);
/// # let _ = &mut rt;
/// ```
pub struct Runtime<S> {
    client: sui_rpc::Client,
    signer: S,
    cursor: handles::Cursor,
    wait_timeout: Duration,
    default_gas_budget: u64,
}

impl<S: SuiSigner> Runtime<S> {
    /// Create a runtime from an RPC client and a signer.
    pub fn new(client: sui_rpc::Client, signer: S) -> Self {
        Self {
            client,
            signer,
            cursor: handles::Cursor::default(),
            wait_timeout: Duration::from_secs(30),
            default_gas_budget: 2_000_000,
        }
    }

    /// Replace the runtime cursor with a previously captured snapshot.
    ///
    /// Prefer calling this immediately after [`Runtime::new`] and before creating any runtime-owned
    /// handles.
    pub fn with_cursor_snapshot(mut self, snapshot: CursorSnapshot) -> Self {
        self.cursor = handles::Cursor::from_snapshot(snapshot);
        self
    }

    /// Snapshot the runtime cursor (your local frontier).
    pub fn cursor_snapshot(&self) -> CursorSnapshot {
        self.cursor.snapshot()
    }

    /// Fetch transaction effects by digest and advance the runtime cursor.
    ///
    /// This is the recovery escape hatch for cases where you have a transaction digest but do not
    /// have effects locally (for example, a receipt that was persisted without `effects`, or
    /// a receipt returned by an RPC node that did not include `effects.bcs`).
    ///
    /// If the transaction effects are returned by RPC, the runtime derives an effects patch and
    /// applies it to its cursor, updating any matching runtime-owned handles.
    pub async fn sync_transaction(
        &mut self,
        digest: sui_sdk_types::Digest,
    ) -> Result<Option<TransactionEffects>, Error> {
        let effects = tx::fetch_transaction_effects(&mut self.client, digest).await?;
        if let Some(effects) = &effects {
            self.apply_patch(effects);
        }
        Ok(effects)
    }

    /// Override the default checkpoint wait timeout used by [`Tx::commit`].
    pub fn with_wait_timeout(mut self, timeout: Duration) -> Self {
        self.wait_timeout = timeout;
        self
    }

    /// Override the default gas budget used when none is provided.
    pub fn with_default_gas_budget(mut self, budget: u64) -> Self {
        self.default_gas_budget = budget;
        self
    }

    /// Create a read view for fetching typed handles.
    pub fn read(&mut self) -> Read<'_, S> {
        Read { rt: self }
    }

    /// Create a transaction view for the given sender.
    ///
    /// The returned [`Tx`] view contains the execution actions:
    /// - [`Tx::simulate`] (checks enabled, no mutation)
    /// - [`Tx::inspect`] (checks disabled + command outputs)
    /// - [`Tx::commit`] (sign/submit/wait + update handles)
    pub fn tx(&mut self, sender: Address) -> Tx<'_, S> {
        Tx {
            rt: self,
            sender,
            ptb: sui_move_ptb::PtbBuilder::new(),
        }
    }

    fn apply_patch(&self, effects: &TransactionEffects) {
        self.cursor.apply_patch(effects);
    }
}

/// Read view: read/fetch helpers and handle construction.
///
/// This view is intentionally read-only with respect to the chain: it fetches data from RPC and
/// constructs typed handles, but it does not submit transactions.
pub struct Read<'a, S> {
    rt: &'a mut Runtime<S>,
}

impl<'a, S: SuiSigner> Read<'a, S> {
    /// Mutable access to the underlying RPC client (escape hatch).
    pub fn client_mut(&mut self) -> &mut sui_rpc::Client {
        &mut self.rt.client
    }

    /// Refresh the reference and owner information for an object id in the runtime cursor.
    ///
    /// This is the explicit escape hatch for external drift: if other transactions mutate an
    /// object you care about, your local cursor does not update until you either commit through
    /// this runtime or refresh explicitly.
    ///
    /// If the object does not exist, the cursor is tombstoned (if it was being tracked).
    pub async fn refresh_id(&mut self, object_id: Address) -> Result<(), Error> {
        match tx::fetch_object_reference_and_owner(&mut self.rt.client, object_id).await {
            Ok(fetched) => {
                self.rt
                    .cursor
                    .intern_untyped(fetched.reference, fetched.owner);
                Ok(())
            }
            Err(err @ tx::RpcError::Missing(_)) => {
                self.rt
                    .cursor
                    .tombstone(object_id, TombstoneReason::NotExist);
                Err(Error::Rpc(err))
            }
            Err(err) => Err(Error::Rpc(err)),
        }
    }

    /// Refresh multiple object ids in sequence.
    ///
    /// This calls [`Read::refresh_id`] for each provided id.
    pub async fn refresh_ids(
        &mut self,
        object_ids: impl IntoIterator<Item = Address>,
    ) -> Result<(), Error> {
        for object_id in object_ids {
            self.refresh_id(object_id).await?;
        }
        Ok(())
    }

    /// Refresh the reference and owner information for a runtime-owned handle.
    ///
    /// This is the explicit escape hatch for external drift: if another transaction changes an
    /// owned object (or rotates its `ObjectReference`), your local cursor does not update until you
    /// either commit through this runtime or refresh explicitly.
    pub async fn refresh<T: sui_move::MoveStruct + sui_move::HasKey>(
        &mut self,
        obj: &Object<T>,
    ) -> Result<(), Error> {
        self.refresh_id(obj.object_id()).await
    }

    /// Construct a runtime-owned object handle by fetching its latest `ObjectReference` and owner kind.
    ///
    /// The returned [`Object<T>`] is the default handle type used throughout this crate:
    /// - owned/immutable objects convert to `Input::ImmutableOrOwned(ObjectReference)`
    /// - shared-like objects convert to `Input::Shared(SharedInput)` (immutable by default)
    ///
    /// If you need an explicit input mode, derive a view from the handle at the moment it matters:
    /// - `obj.shared_immutable()?` / `obj.shared_mutable()?`
    /// - `obj.receiving()?`
    pub async fn object<T: sui_move::MoveStruct + sui_move::HasKey>(
        &mut self,
        object_id: Address,
    ) -> Result<Object<T>, Error> {
        let fetched =
            match tx::fetch_object_reference_and_owner(&mut self.rt.client, object_id).await {
                Ok(fetched) => fetched,
                Err(err @ tx::RpcError::Missing(_)) => {
                    self.rt
                        .cursor
                        .tombstone(object_id, TombstoneReason::NotExist);
                    return Err(Error::Rpc(err));
                }
                Err(err) => return Err(Error::Rpc(err)),
            };

        let kind = tx::classify_owner(&fetched.owner);
        match kind {
            tx::OwnerKind::Immutable | tx::OwnerKind::AddressOwned => {}
            kind if kind.is_shared_like() => {}
            other => {
                return Err(Error::ObjectKind {
                    object_id,
                    expected: "immutable-or-owned or shared",
                    actual: other.label(),
                });
            }
        }

        Ok(self
            .rt
            .cursor
            .intern_object::<T>(fetched.reference, fetched.owner))
    }

    /// Fetch an object and decode its Move contents into `T`.
    ///
    /// This performs a tag check (`T::type_tag_static()` must match the on-chain `TypeTag`) and
    /// returns both:
    /// - a runtime-owned handle (`Object<T>`) and
    /// - the decoded value (`T`).
    pub async fn get<T: sui_move::MoveStruct + sui_move::HasKey>(
        &mut self,
        object_id: Address,
    ) -> Result<(Object<T>, T), Error> {
        let fetched =
            match tx::fetch_object_reference_and_owner(&mut self.rt.client, object_id).await {
                Ok(fetched) => fetched,
                Err(err @ tx::RpcError::Missing(_)) => {
                    self.rt
                        .cursor
                        .tombstone(object_id, TombstoneReason::NotExist);
                    return Err(Error::Rpc(err));
                }
                Err(err) => return Err(Error::Rpc(err)),
            };

        let obj = self
            .rt
            .cursor
            .intern_object::<T>(fetched.reference, fetched.owner);

        let got = TypeTag::Struct(Box::new(fetched.struct_tag));
        let decoded = sui_move::MoveInstance::<T>::from_raw_type(got, &fetched.contents)
            .map_err(|source| Error::Decode { object_id, source })?
            .value;

        Ok((obj, decoded))
    }

    /// Fetch an object and decode its contents as `T` without verifying the on-chain type tag.
    pub async fn get_unchecked<T: sui_move::MoveStruct + sui_move::HasKey>(
        &mut self,
        object_id: Address,
    ) -> Result<(Object<T>, T), Error> {
        let fetched =
            match tx::fetch_object_reference_and_owner(&mut self.rt.client, object_id).await {
                Ok(fetched) => fetched,
                Err(err @ tx::RpcError::Missing(_)) => {
                    self.rt
                        .cursor
                        .tombstone(object_id, TombstoneReason::NotExist);
                    return Err(Error::Rpc(err));
                }
                Err(err) => return Err(Error::Rpc(err)),
            };

        let obj = self
            .rt
            .cursor
            .intern_object::<T>(fetched.reference, fetched.owner);
        let decoded = T::from_bcs(&fetched.contents).map_err(|err| Error::Decode {
            object_id,
            source: err.into(),
        })?;

        Ok((obj, decoded))
    }

    /// Fetch and decode the latest on-chain contents for a runtime-owned object handle.
    ///
    /// This performs a tag check and refreshes the runtime-owned handle's reference/owner
    /// information in the cursor.
    pub async fn decode<T: sui_move::MoveStruct + sui_move::HasKey>(
        &mut self,
        obj: &Object<T>,
    ) -> Result<T, Error> {
        let object_id = obj.object_id();
        let fetched =
            match tx::fetch_object_reference_and_owner(&mut self.rt.client, object_id).await {
                Ok(fetched) => fetched,
                Err(err @ tx::RpcError::Missing(_)) => {
                    self.rt
                        .cursor
                        .tombstone(object_id, TombstoneReason::NotExist);
                    return Err(Error::Rpc(err));
                }
                Err(err) => return Err(Error::Rpc(err)),
            };

        self.rt
            .cursor
            .intern_object::<T>(fetched.reference, fetched.owner);

        let got = TypeTag::Struct(Box::new(fetched.struct_tag));
        let decoded = sui_move::MoveInstance::<T>::from_raw_type(got, &fetched.contents)
            .map_err(|source| Error::Decode { object_id, source })?
            .value;

        Ok(decoded)
    }

    /// Fetch and decode the latest on-chain contents for a runtime-owned object handle without tag
    /// verification.
    ///
    /// This refreshes the runtime-owned handle's reference/owner information in the cursor.
    pub async fn decode_unchecked<T: sui_move::MoveStruct + sui_move::HasKey>(
        &mut self,
        obj: &Object<T>,
    ) -> Result<T, Error> {
        let object_id = obj.object_id();
        let fetched =
            match tx::fetch_object_reference_and_owner(&mut self.rt.client, object_id).await {
                Ok(fetched) => fetched,
                Err(err @ tx::RpcError::Missing(_)) => {
                    self.rt
                        .cursor
                        .tombstone(object_id, TombstoneReason::NotExist);
                    return Err(Error::Rpc(err));
                }
                Err(err) => return Err(Error::Rpc(err)),
            };

        self.rt
            .cursor
            .intern_object::<T>(fetched.reference, fetched.owner);

        T::from_bcs(&fetched.contents).map_err(|err| Error::Decode {
            object_id,
            source: err.into(),
        })
    }

    /// Construct a receiving object handle by fetching the latest `ObjectReference`.
    ///
    /// This corresponds to Sui's `Input::Receiving`.
    ///
    /// Receiving is a transaction input mode (the Move framework type
    /// `sui::transfer::Receiving<T>`), not an on-chain owner kind. This helper only fetches the
    /// latest reference and does not validate that the object is valid to receive in the current
    /// transaction.
    pub async fn receiving_object<T: sui_move::MoveStruct + sui_move::HasKey>(
        &mut self,
        object_id: Address,
    ) -> Result<ReceivingObject<T>, Error> {
        let fetched =
            match tx::fetch_object_reference_and_owner(&mut self.rt.client, object_id).await {
                Ok(fetched) => fetched,
                Err(err @ tx::RpcError::Missing(_)) => {
                    self.rt
                        .cursor
                        .tombstone(object_id, TombstoneReason::NotExist);
                    return Err(Error::Rpc(err));
                }
                Err(err) => return Err(Error::Rpc(err)),
            };
        Ok(self
            .rt
            .cursor
            .intern_receiving_object::<T>(fetched.reference, fetched.owner))
    }

    /// Construct a shared object handle by fetching its initial shared version.
    ///
    /// This corresponds to Sui's `Input::Shared`.
    pub async fn shared_object<T: sui_move::MoveStruct + sui_move::HasKey>(
        &mut self,
        object_id: Address,
        mutability: Mutability,
    ) -> Result<SharedObject<T>, Error> {
        let fetched =
            match tx::fetch_object_reference_and_owner(&mut self.rt.client, object_id).await {
                Ok(fetched) => fetched,
                Err(err @ tx::RpcError::Missing(_)) => {
                    self.rt
                        .cursor
                        .tombstone(object_id, TombstoneReason::NotExist);
                    return Err(Error::Rpc(err));
                }
                Err(err) => return Err(Error::Rpc(err)),
            };

        let kind = tx::classify_owner(&fetched.owner);
        let Some(initial_shared_version) = kind.shared_start_version() else {
            return Err(Error::ObjectKind {
                object_id,
                expected: "shared",
                actual: kind.label(),
            });
        };

        Ok(SharedObject::new(
            object_id,
            initial_shared_version,
            mutability,
        ))
    }

    /// Convenience: immutable shared input.
    pub async fn shared_immutable<T: sui_move::MoveStruct + sui_move::HasKey>(
        &mut self,
        object_id: Address,
    ) -> Result<SharedObject<T>, Error> {
        self.shared_object(object_id, Mutability::Immutable).await
    }

    /// Convenience: mutable shared input.
    pub async fn shared_mutable<T: sui_move::MoveStruct + sui_move::HasKey>(
        &mut self,
        object_id: Address,
    ) -> Result<SharedObject<T>, Error> {
        self.shared_object(object_id, Mutability::Mutable).await
    }
}

/// Transaction builder: build, simulate/inspect, and commit a PTB.
pub struct Tx<'a, S> {
    rt: &'a mut Runtime<S>,
    sender: Address,
    ptb: sui_move_ptb::PtbBuilder,
}

impl<'a, S: SuiSigner> Tx<'a, S> {
    /// Borrow the underlying PTB builder.
    ///
    /// This is useful for accessing less common PTB commands without this crate adding extra
    /// wrappers.
    pub fn ptb(&self) -> &sui_move_ptb::PtbBuilder {
        &self.ptb
    }

    /// Mutably borrow the underlying PTB builder (escape hatch).
    pub fn ptb_mut(&mut self) -> &mut sui_move_ptb::PtbBuilder {
        &mut self.ptb
    }

    /// Add a raw input to the transaction and return its `Argument::Input`.
    pub fn input(
        &mut self,
        input: sui_move_call::CallArg,
    ) -> Result<sui_sdk_types::Argument, Error> {
        Ok(self.ptb.input(input)?)
    }

    /// Convert a typed value into an input and return its `Argument::Input`.
    pub fn arg<A: sui_move_call::ToCallArg>(
        &mut self,
        value: &A,
    ) -> Result<sui_sdk_types::Argument, Error> {
        Ok(self.ptb.arg(value)?)
    }

    /// Add a Move call command from a typed [`sui_move_call::CallSpec`].
    pub fn call<R: sui_move_call::CallReturn>(
        &mut self,
        spec: sui_move_call::CallSpec<R>,
    ) -> Result<R, Error> {
        Ok(self.ptb.call(spec)?)
    }

    /// The gas argument (`Argument::Gas`).
    ///
    /// This is useful when building native PTB commands that can refer to the gas coin.
    pub fn gas(&self) -> sui_sdk_types::Argument {
        self.ptb.gas()
    }

    /// Add a `TransferObjects` command.
    pub fn transfer_objects(
        &mut self,
        objects: Vec<sui_sdk_types::Argument>,
        address: sui_sdk_types::Argument,
    ) -> Result<(), Error> {
        Ok(self.ptb.transfer_objects(objects, address)?)
    }

    /// Add a `SplitCoins` command.
    pub fn split_coins(
        &mut self,
        coin: sui_sdk_types::Argument,
        amounts: Vec<sui_sdk_types::Argument>,
    ) -> Result<sui_sdk_types::Argument, Error> {
        Ok(self.ptb.split_coins(coin, amounts)?)
    }

    /// Add a `MergeCoins` command.
    pub fn merge_coins(
        &mut self,
        destination: sui_sdk_types::Argument,
        sources: Vec<sui_sdk_types::Argument>,
    ) -> Result<(), Error> {
        Ok(self.ptb.merge_coins(destination, sources)?)
    }

    /// Add a `MakeMoveVector` command.
    pub fn make_move_vector(
        &mut self,
        type_: Option<TypeTag>,
        elements: Vec<sui_sdk_types::Argument>,
    ) -> Result<sui_sdk_types::Argument, Error> {
        Ok(self.ptb.make_move_vector(type_, elements)?)
    }

    /// Add a `Publish` command.
    pub fn publish(
        &mut self,
        modules: Vec<Vec<u8>>,
        dependencies: Vec<Address>,
    ) -> Result<(), Error> {
        Ok(self.ptb.publish(modules, dependencies)?)
    }

    /// Finish and return the built PTB without submitting it.
    pub fn finish_ptb(self) -> ProgrammableTransaction {
        self.ptb.finish()
    }

    /// Commit the built PTB and wait for checkpoint inclusion.
    ///
    /// If checkpoint waiting times out (or the checkpoint stream errors), this still returns a
    /// [`Receipt`] with `digest` and any decoded effects, and marks the observed finality as
    /// `Executed`.
    ///
    /// On success, the runtime applies an effects-derived patch to its cursor, updating
    /// all live [`Object`] and [`ReceivingObject`] handles that match changed objects.
    pub async fn commit(self) -> Result<Receipt, Error> {
        self.commit_with(TxOptions::default()).await
    }

    /// Commit the built PTB using explicit transaction options.
    pub async fn commit_with(self, opts: TxOptions) -> Result<Receipt, Error> {
        let Tx { rt, sender, ptb } = self;
        Self::commit_ptb_with_inner(rt, sender, ptb.finish(), opts).await
    }

    /// Commit a pre-built PTB and wait for checkpoint inclusion.
    ///
    /// This is the escape hatch for advanced PTB building (coin ops, wiring, etc).
    pub async fn commit_ptb(self, ptb: ProgrammableTransaction) -> Result<Receipt, Error> {
        self.commit_ptb_with(ptb, TxOptions::default()).await
    }

    /// Commit a pre-built PTB using explicit transaction options.
    pub async fn commit_ptb_with(
        self,
        ptb: ProgrammableTransaction,
        opts: TxOptions,
    ) -> Result<Receipt, Error> {
        let Tx { rt, sender, .. } = self;
        Self::commit_ptb_with_inner(rt, sender, ptb, opts).await
    }

    async fn commit_ptb_with_inner(
        rt: &mut Runtime<S>,
        sender: Address,
        ptb: ProgrammableTransaction,
        opts: TxOptions,
    ) -> Result<Receipt, Error> {
        let receipt = tx::submit_and_wait(
            &mut rt.client,
            &rt.signer,
            sender,
            ptb,
            opts,
            rt.default_gas_budget,
            rt.wait_timeout,
        )
        .await?;

        if let Some(effects) = &receipt.effects {
            rt.apply_patch(effects);
        }

        Ok(receipt)
    }

    /// Dry-run the built PTB (checks enabled) without mutating chain state.
    ///
    /// This does not update runtime-owned handles.
    pub async fn simulate(&mut self) -> Result<SimulationReceipt, Error> {
        self.simulate_with(SimulateOptions::default()).await
    }

    /// Dry-run the built PTB with explicit simulation options.
    ///
    /// This does not update runtime-owned handles.
    pub async fn simulate_with(
        &mut self,
        opts: SimulateOptions,
    ) -> Result<SimulationReceipt, Error> {
        let ptb = self.ptb.clone().finish();
        self.simulate_ptb_with(ptb, opts).await
    }

    /// Dry-run a pre-built PTB (checks enabled) without mutating chain state.
    ///
    /// This does not update runtime-owned handles.
    pub async fn simulate_ptb(
        &mut self,
        ptb: ProgrammableTransaction,
    ) -> Result<SimulationReceipt, Error> {
        self.simulate_ptb_with(ptb, SimulateOptions::default())
            .await
    }

    /// Dry-run a pre-built PTB with explicit simulation options.
    ///
    /// This does not update runtime-owned handles.
    pub async fn simulate_ptb_with(
        &mut self,
        ptb: ProgrammableTransaction,
        opts: SimulateOptions,
    ) -> Result<SimulationReceipt, Error> {
        Ok(tx::simulate_ptb(&mut self.rt.client, self.sender, ptb, opts).await?)
    }

    /// Dev-inspect the built PTB (checks disabled) to retrieve command outputs for debugging.
    ///
    /// This does not update runtime-owned handles.
    pub async fn inspect(&mut self) -> Result<InspectReceipt, Error> {
        self.inspect_with(InspectOptions::default()).await
    }

    /// Dev-inspect the built PTB with explicit inspect options.
    ///
    /// This does not update runtime-owned handles.
    pub async fn inspect_with(&mut self, opts: InspectOptions) -> Result<InspectReceipt, Error> {
        let ptb = self.ptb.clone().finish();
        self.inspect_ptb_with(ptb, opts).await
    }

    /// Dev-inspect a pre-built PTB (checks disabled) to retrieve command outputs for debugging.
    ///
    /// This does not update runtime-owned handles.
    pub async fn inspect_ptb(
        &mut self,
        ptb: ProgrammableTransaction,
    ) -> Result<InspectReceipt, Error> {
        self.inspect_ptb_with(ptb, InspectOptions::default()).await
    }

    /// Dev-inspect a pre-built PTB with explicit inspect options.
    ///
    /// This does not update runtime-owned handles.
    pub async fn inspect_ptb_with(
        &mut self,
        ptb: ProgrammableTransaction,
        opts: InspectOptions,
    ) -> Result<InspectReceipt, Error> {
        Ok(tx::inspect_ptb(&mut self.rt.client, self.sender, ptb, opts).await?)
    }
}

/// Convenience re-exports for downstream code.
///
/// This prelude is meant for application code and examples. It includes:
/// - `sui-move-runtime` core types
/// - `sui-move-call` prelude (call building)
/// - `sui-move-ptb` prelude (PTB building)
pub mod prelude {
    pub use crate::{
        BcsValue, CheckpointWaitOutcome, CommandOutputs, CursorSnapshot, EnsureSuccessError, Error,
        Finality, InspectOptions, InspectReceipt, Object, ObservedFinality, Read, Receipt,
        ReceivingObject, Runtime, SharedObject, SimulateOptions, SimulationReceipt,
        TombstoneReason, Tx, TxOptions,
    };
    pub use sui_move_call::prelude::*;
    pub use sui_move_ptb::prelude::*;
    pub use sui_sdk_types::Mutability;
}

/// Build and run a transaction in the Read → Tx → Commit mental model.
///
/// This macro is a small ergonomic wrapper around [`Runtime::tx`]. It builds a transaction using a
/// temporary [`Tx`] builder and then runs exactly one action:
/// - commit (default),
/// - simulate (checks enabled), or
/// - inspect (checks disabled + command outputs).
///
/// The macro returns a **future**, so callers use `.await`:
///
/// ```rust,no_run
/// use sui_move_runtime::prelude::*;
///
/// # async fn demo(mut rt: Runtime<impl sui_crypto::SuiSigner>, sender: sui_sdk_types::Address) -> Result<(), Error> {
/// let receipt = sui_move_runtime::tx!(&mut rt, sender => {
///     CallSpec::new("0x1".parse().unwrap(), "m", "f").unwrap();
/// })
/// .await?;
/// receipt.ensure_success()?;
/// # Ok(())
/// # }
/// ```
///
/// # Forms
///
/// **Call list (default commit)**: each statement expression must evaluate to a `CallSpec`.
///
/// ```rust,no_run
/// # use sui_move_runtime::prelude::*;
/// # async fn demo(mut rt: Runtime<impl sui_crypto::SuiSigner>, sender: sui_sdk_types::Address) -> Result<(), Error> {
/// let _receipt = sui_move_runtime::tx!(&mut rt, sender => {
///     CallSpec::new("0x1".parse().unwrap(), "m", "f").unwrap();
///     CallSpec::new("0x1".parse().unwrap(), "m", "g").unwrap();
/// })
/// .await?;
/// # Ok(())
/// # }
/// ```
///
/// **Builder form**: opt into direct access to `Tx` (escape hatch for PTB wiring).
///
/// ```rust,no_run
/// # use sui_move_runtime::prelude::*;
/// # async fn demo(mut rt: Runtime<impl sui_crypto::SuiSigner>, sender: sui_sdk_types::Address) -> Result<(), Error> {
/// let _receipt = sui_move_runtime::tx!(&mut rt, sender, tx => {
///     let _out = tx.call(CallSpec::new("0x1".parse().unwrap(), "m", "f").unwrap())?;
/// })
/// .await?;
/// # Ok(())
/// # }
/// ```
///
/// **Action variants**:
///
/// ```rust,no_run
/// # use sui_move_runtime::prelude::*;
/// # async fn demo(mut rt: Runtime<impl sui_crypto::SuiSigner>, sender: sui_sdk_types::Address) -> Result<(), Error> {
/// let _sim = sui_move_runtime::tx!(simulate, &mut rt, sender => {
///     CallSpec::new("0x1".parse().unwrap(), "m", "f").unwrap();
/// })
/// .await?;
///
/// let _dbg = sui_move_runtime::tx!(inspect, &mut rt, sender => {
///     CallSpec::new("0x1".parse().unwrap(), "m", "f").unwrap();
/// })
/// .await?;
///
/// let _receipt = sui_move_runtime::tx!(
///     commit_with(TxOptions { finality: Finality::Executed, ..Default::default() }),
///     &mut rt,
///     sender => {
///         CallSpec::new("0x1".parse().unwrap(), "m", "f").unwrap();
///     }
/// )
/// .await?;
/// # Ok(())
/// # }
/// ```
#[macro_export]
macro_rules! tx {
    (commit, $rt:expr, $sender:expr => { $($spec:expr);+ $(;)? }) => {{
        async {
            let mut __tx = ($rt).tx($sender);
            $(
                __tx.call($spec)?;
            )+
            __tx.commit().await
        }
    }};
    (commit, $rt:expr, $sender:expr, $tx:ident => { $($body:tt)* }) => {{
        async {
            let mut $tx = ($rt).tx($sender);
            { $($body)* }
            $tx.commit().await
        }
    }};

    (commit_with($opts:expr), $rt:expr, $sender:expr => { $($spec:expr);+ $(;)? }) => {{
        async {
            let mut __tx = ($rt).tx($sender);
            $(
                __tx.call($spec)?;
            )+
            __tx.commit_with($opts).await
        }
    }};
    (commit_with($opts:expr), $rt:expr, $sender:expr, $tx:ident => { $($body:tt)* }) => {{
        async {
            let mut $tx = ($rt).tx($sender);
            { $($body)* }
            $tx.commit_with($opts).await
        }
    }};

    (simulate, $rt:expr, $sender:expr => { $($spec:expr);+ $(;)? }) => {{
        async {
            let mut __tx = ($rt).tx($sender);
            $(
                __tx.call($spec)?;
            )+
            __tx.simulate().await
        }
    }};
    (simulate, $rt:expr, $sender:expr, $tx:ident => { $($body:tt)* }) => {{
        async {
            let mut $tx = ($rt).tx($sender);
            { $($body)* }
            $tx.simulate().await
        }
    }};

    (simulate_with($opts:expr), $rt:expr, $sender:expr => { $($spec:expr);+ $(;)? }) => {{
        async {
            let mut __tx = ($rt).tx($sender);
            $(
                __tx.call($spec)?;
            )+
            __tx.simulate_with($opts).await
        }
    }};
    (simulate_with($opts:expr), $rt:expr, $sender:expr, $tx:ident => { $($body:tt)* }) => {{
        async {
            let mut $tx = ($rt).tx($sender);
            { $($body)* }
            $tx.simulate_with($opts).await
        }
    }};

    (inspect, $rt:expr, $sender:expr => { $($spec:expr);+ $(;)? }) => {{
        async {
            let mut __tx = ($rt).tx($sender);
            $(
                __tx.call($spec)?;
            )+
            __tx.inspect().await
        }
    }};
    (inspect, $rt:expr, $sender:expr, $tx:ident => { $($body:tt)* }) => {{
        async {
            let mut $tx = ($rt).tx($sender);
            { $($body)* }
            $tx.inspect().await
        }
    }};

    (inspect_with($opts:expr), $rt:expr, $sender:expr => { $($spec:expr);+ $(;)? }) => {{
        async {
            let mut __tx = ($rt).tx($sender);
            $(
                __tx.call($spec)?;
            )+
            __tx.inspect_with($opts).await
        }
    }};
    (inspect_with($opts:expr), $rt:expr, $sender:expr, $tx:ident => { $($body:tt)* }) => {{
        async {
            let mut $tx = ($rt).tx($sender);
            { $($body)* }
            $tx.inspect_with($opts).await
        }
    }};

    ($rt:expr, $sender:expr => { $($spec:expr);+ $(;)? }) => {{
        $crate::tx!(commit, $rt, $sender => { $($spec);+ })
    }};
    ($rt:expr, $sender:expr, $tx:ident => { $($body:tt)* }) => {{
        $crate::tx!(commit, $rt, $sender, $tx => { $($body)* })
    }};
}
