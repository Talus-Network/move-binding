#![doc = include_str!("../README.md")]
#![deny(missing_docs)]

mod effects;
mod handles;
mod tx;

pub use crate::handles::{AnyObject, Object, ReceivingObject, SharedObject};
pub use crate::tx::{
    BcsValue, CommandOutputs, InspectOptions, InspectReceipt, Receipt, SimulateOptions,
    SimulationReceipt, TxOptions,
};

use std::time::Duration;

use sui_crypto::SuiSigner;
use sui_sdk_types::{Address, Mutability, ProgrammableTransaction, TransactionEffects};

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

/// Long-lived runtime owning RPC + signer + handle registry.
///
/// This is the entry point for the “two namespaces” mental model:
/// - [`Runtime::read`] for Rust-time reads and handle construction
/// - [`Runtime::move_time`] for Move-time transaction commit
///
/// # Runtime-owned handles
///
/// Handles returned by [`Read::object`] and [`Read::receiving_object`] are **interned** in the
/// runtime by `object_id`. Clones of the same handle share the same internal cell, so they all see
/// updates.
///
/// After [`MoveTime::commit`], the runtime decodes `TransactionEffects` from RPC and updates any
/// live handle cells whose object id appears in the effects.
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
    registry: handles::Registry,
    wait_timeout: Duration,
    default_gas_budget: u64,
}

impl<S: SuiSigner> Runtime<S> {
    /// Create a runtime from an RPC client and a signer.
    pub fn new(client: sui_rpc::Client, signer: S) -> Self {
        Self {
            client,
            signer,
            registry: handles::Registry::default(),
            wait_timeout: Duration::from_secs(30),
            default_gas_budget: 2_000_000,
        }
    }

    /// Override the default checkpoint wait timeout used by [`MoveTime::commit`].
    pub fn with_wait_timeout(mut self, timeout: Duration) -> Self {
        self.wait_timeout = timeout;
        self
    }

    /// Override the default gas budget used when none is provided.
    pub fn with_default_gas_budget(mut self, budget: u64) -> Self {
        self.default_gas_budget = budget;
        self
    }

    /// Enter Rust-time (read-only namespace).
    ///
    /// The returned [`Read`] view contains RPC helpers for fetching typed handles.
    pub fn read(&mut self) -> Read<'_, S> {
        Read { rt: self }
    }

    /// Enter Move-time (transaction namespace) for the given sender.
    ///
    /// The returned [`MoveTime`] view contains the execution actions:
    /// - [`MoveTime::simulate`] (checks enabled, no mutation)
    /// - [`MoveTime::inspect`] (checks disabled + command outputs)
    /// - [`MoveTime::commit`] (sign/submit/wait + update handles)
    pub fn move_time(&mut self, sender: Address) -> MoveTime<'_, S> {
        MoveTime { rt: self, sender }
    }

    fn apply_effects(&self, effects: &TransactionEffects) {
        self.registry.apply_effects(effects);
    }
}

/// Rust-time view: read/fetch helpers and handle construction.
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

    /// Construct an owned-or-immutable object handle by fetching the latest `ObjectReference`.
    ///
    /// This corresponds to Sui's `Input::ImmutableOrOwned`.
    ///
    /// If the object is shared on-chain, this returns [`Error::ObjectKind`]; use
    /// [`Read::shared_object`]/[`Read::shared_mutable`]/[`Read::shared_immutable`] instead.
    pub async fn object<T: sui_move::MoveStruct + sui_move::HasKey>(
        &mut self,
        object_id: Address,
    ) -> Result<Object<T>, Error> {
        let (reference, owner) =
            tx::fetch_object_reference_and_owner(&mut self.rt.client, object_id).await?;

        let kind = tx::classify_owner(&owner);
        if kind.is_shared_like() {
            return Err(Error::ObjectKind {
                object_id,
                expected: "immutable-or-owned",
                actual: kind.label(),
            });
        }

        Ok(self.rt.registry.intern_object::<T>(reference))
    }

    /// Construct an object handle regardless of whether it is owned/immutable or shared on-chain.
    ///
    /// - If the object is owned/immutable, this returns [`AnyObject`] wrapping an [`Object<T>`].
    /// - If the object is shared, this returns [`AnyObject`] wrapping an **immutable** [`SharedObject<T>`].
    ///
    /// If you need a mutable shared input, call [`AnyObject::as_shared_mutable`] on the returned
    /// value (or use [`Read::shared_mutable`] directly).
    pub async fn object_any<T: sui_move::MoveStruct + sui_move::HasKey>(
        &mut self,
        object_id: Address,
    ) -> Result<AnyObject<T>, Error> {
        let (reference, owner) =
            tx::fetch_object_reference_and_owner(&mut self.rt.client, object_id).await?;

        match tx::classify_owner(&owner) {
            tx::OwnerKind::ImmutableOrOwned => Ok(AnyObject::from_object(
                self.rt.registry.intern_object::<T>(reference),
            )),
            tx::OwnerKind::SharedLike => {
                let Some(initial_shared_version) = tx::shared_version_from_owner(&owner) else {
                    return Err(Error::ObjectKind {
                        object_id,
                        expected: "shared",
                        actual: "unknown",
                    });
                };

                Ok(AnyObject::from_shared(SharedObject::immutable(
                    object_id,
                    initial_shared_version,
                )))
            }
            tx::OwnerKind::Unknown => Err(Error::ObjectKind {
                object_id,
                expected: "immutable-or-owned or shared",
                actual: "unknown",
            }),
        }
    }

    /// Construct a receiving object handle by fetching the latest `ObjectReference`.
    ///
    /// This corresponds to Sui's `Input::Receiving`.
    pub async fn receiving_object<T: sui_move::MoveStruct + sui_move::HasKey>(
        &mut self,
        object_id: Address,
    ) -> Result<ReceivingObject<T>, Error> {
        let (reference, _owner) =
            tx::fetch_object_reference_and_owner(&mut self.rt.client, object_id).await?;
        Ok(self.rt.registry.intern_receiving_object::<T>(reference))
    }

    /// Construct a shared object handle by fetching its initial shared version.
    ///
    /// This corresponds to Sui's `Input::Shared`.
    pub async fn shared_object<T: sui_move::MoveStruct + sui_move::HasKey>(
        &mut self,
        object_id: Address,
        mutability: Mutability,
    ) -> Result<SharedObject<T>, Error> {
        let (_reference, owner) =
            tx::fetch_object_reference_and_owner(&mut self.rt.client, object_id).await?;

        let Some(initial_shared_version) = tx::shared_version_from_owner(&owner) else {
            return Err(Error::ObjectKind {
                object_id,
                expected: "shared",
                actual: tx::classify_owner(&owner).label(),
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

/// Move-time view: build/submit/wait + automatic handle updates.
///
/// The core mental model is that you “open Move-time” for a sender and then perform one action.
/// This is why these methods take `self` by value: the view represents a scoped boundary.
pub struct MoveTime<'a, S> {
    rt: &'a mut Runtime<S>,
    sender: Address,
}

impl<'a, S: SuiSigner> MoveTime<'a, S> {
    /// Commit a pre-built PTB and wait for checkpoint inclusion.
    ///
    /// On success, the runtime applies the returned transaction effects to its registry, updating
    /// all live [`Object`] and [`ReceivingObject`] handles that match changed objects.
    pub async fn commit(self, ptb: ProgrammableTransaction) -> Result<Receipt, Error> {
        self.commit_with(ptb, TxOptions::default()).await
    }

    /// Dry-run a PTB (checks enabled) without mutating chain state.
    ///
    /// This does not update runtime-owned handles.
    pub async fn simulate(self, ptb: ProgrammableTransaction) -> Result<SimulationReceipt, Error> {
        self.simulate_with(ptb, SimulateOptions::default()).await
    }

    /// Dry-run a PTB with explicit simulation options.
    ///
    /// This does not update runtime-owned handles.
    pub async fn simulate_with(
        self,
        ptb: ProgrammableTransaction,
        opts: SimulateOptions,
    ) -> Result<SimulationReceipt, Error> {
        Ok(tx::simulate_ptb(&mut self.rt.client, self.sender, ptb, opts).await?)
    }

    /// Dev-inspect a PTB (checks disabled) to retrieve command outputs for debugging.
    ///
    /// This does not update runtime-owned handles.
    pub async fn inspect(self, ptb: ProgrammableTransaction) -> Result<InspectReceipt, Error> {
        self.inspect_with(ptb, InspectOptions::default()).await
    }

    /// Dev-inspect a PTB with explicit inspect options.
    ///
    /// This does not update runtime-owned handles.
    pub async fn inspect_with(
        self,
        ptb: ProgrammableTransaction,
        opts: InspectOptions,
    ) -> Result<InspectReceipt, Error> {
        Ok(tx::inspect_ptb(&mut self.rt.client, self.sender, ptb, opts).await?)
    }

    /// Commit a pre-built PTB using explicit transaction options.
    pub async fn commit_with(
        self,
        ptb: ProgrammableTransaction,
        opts: TxOptions,
    ) -> Result<Receipt, Error> {
        let receipt = tx::submit_and_wait(
            &mut self.rt.client,
            &self.rt.signer,
            self.sender,
            ptb,
            opts,
            self.rt.default_gas_budget,
            self.rt.wait_timeout,
        )
        .await?;

        if let Some(effects) = &receipt.effects {
            self.rt.apply_effects(effects);
        }

        Ok(receipt)
    }
}

/// Macro wrapper: open Move-time, build a PTB from `CallSpec` expressions, commit, and return.
///
/// The block should contain expressions that evaluate to `sui_move_call::CallSpec` (typically
/// module interface functions).
///
/// This macro expands to an async block, so it must be awaited.
///
/// For more complex transactions (native commands, branching, explicit result wiring), build a
/// `ProgrammableTransaction` directly using `sui_move_ptb::ptb!(tx => { ... })` and call
/// [`MoveTime::commit`] / [`MoveTime::simulate`] / [`MoveTime::inspect`] instead.
///
/// # Example
/// ```rust,no_run
/// use sui_move_runtime::prelude::*;
/// # async fn demo(mut rt: Runtime<impl sui_crypto::SuiSigner>, sender: sui_sdk_types::Address) -> Result<(), Error> {
/// let package: sui_sdk_types::Address = "0x1".parse().unwrap();
/// move_time!(rt, sender, {
///     CallSpec::new(package, "m", "f").unwrap();
/// }).await?;
/// # Ok(())
/// # }
/// ```
#[macro_export]
macro_rules! move_time {
    ($rt:expr, $sender:expr, { $($spec:expr);+ $(;)? }) => {{
        async {
            let ptb = sui_move_ptb::ptb! { $($spec);+ }?;
            $rt.move_time($sender).commit(ptb).await
        }
    }};
}

/// Macro wrapper: open Move-time, build a PTB from `CallSpec` expressions, dry-run it, and return.
///
/// This runs with checks enabled and does not mutate chain state.
///
/// This macro expands to an async block, so it must be awaited.
///
/// # Example
/// ```rust,no_run
/// use sui_move_runtime::prelude::*;
/// # async fn demo(mut rt: Runtime<impl sui_crypto::SuiSigner>, sender: sui_sdk_types::Address) -> Result<(), Error> {
/// let package: sui_sdk_types::Address = "0x1".parse().unwrap();
/// let receipt = simulate_time!(rt, sender, {
///     CallSpec::new(package, "m", "f").unwrap();
/// })
/// .await?;
/// # let _ = receipt;
/// # Ok(())
/// # }
/// ```
#[macro_export]
macro_rules! simulate_time {
    ($rt:expr, $sender:expr, { $($spec:expr);+ $(;)? }) => {{
        async {
            let ptb = sui_move_ptb::ptb! { $($spec);+ }?;
            $rt.move_time($sender).simulate(ptb).await
        }
    }};
}

/// Macro wrapper: open Move-time, build a PTB from `CallSpec` expressions, dev-inspect it, and return.
///
/// This runs with checks disabled and does not mutate chain state. It returns per-command outputs
/// (return values + mutated-by-ref values).
///
/// This macro expands to an async block, so it must be awaited.
///
/// # Example
/// ```rust,no_run
/// use sui_move_runtime::prelude::*;
/// # async fn demo(mut rt: Runtime<impl sui_crypto::SuiSigner>, sender: sui_sdk_types::Address) -> Result<(), Error> {
/// let package: sui_sdk_types::Address = "0x1".parse().unwrap();
/// let receipt = inspect_time!(rt, sender, {
///     CallSpec::new(package, "m", "f").unwrap();
/// })
/// .await?;
/// # let _ = receipt;
/// # Ok(())
/// # }
/// ```
#[macro_export]
macro_rules! inspect_time {
    ($rt:expr, $sender:expr, { $($spec:expr);+ $(;)? }) => {{
        async {
            let ptb = sui_move_ptb::ptb! { $($spec);+ }?;
            $rt.move_time($sender).inspect(ptb).await
        }
    }};
}

/// Alias for [`inspect_time!`] with a more “debugging” oriented name.
#[macro_export]
macro_rules! debug_time {
    ($($tt:tt)*) => {
        $crate::inspect_time!($($tt)*)
    };
}

/// Convenience re-exports for downstream code.
///
/// This prelude is meant for application code and examples. It includes:
/// - `sui-move-runtime` macros and core types
/// - `sui-move-call` prelude (call building)
/// - `sui-move-ptb` prelude (PTB building)
pub mod prelude {
    pub use crate::{
        debug_time, inspect_time, move_time, simulate_time, AnyObject, BcsValue, CommandOutputs,
        Error, InspectOptions, InspectReceipt, MoveTime, Object, Read, Receipt, ReceivingObject,
        Runtime, SharedObject, SimulateOptions, SimulationReceipt, TxOptions,
    };
    pub use sui_move_call::prelude::*;
    pub use sui_move_ptb::prelude::*;
    pub use sui_sdk_types::Mutability;
}
