use std::collections::HashMap;
use std::fmt;
use std::marker::PhantomData;
use std::sync::{Arc, Mutex, RwLock, Weak};

use sui_move_call::{CallArg, ToCallArg};
use sui_sdk_types::{Address, Mutability, ObjectReference, Owner, SharedInput};

use crate::effects;
use crate::tx;

#[derive(Debug)]
struct ObjectCell {
    object_id: Address,
    reference: RwLock<ObjectReference>,
    owner: RwLock<Owner>,
    tombstone: RwLock<Option<effects::TombstoneReason>>,
}

impl ObjectCell {
    fn new(reference: ObjectReference, owner: Owner) -> Self {
        Self {
            object_id: *reference.object_id(),
            reference: RwLock::new(reference),
            owner: RwLock::new(owner),
            tombstone: RwLock::new(None),
        }
    }

    fn reference(&self) -> ObjectReference {
        self.reference.read().expect("poisoned object lock").clone()
    }

    fn set_owner(&self, owner: Owner) {
        let mut lock = self.owner.write().expect("poisoned object lock");
        *lock = owner;
    }

    fn tombstone_reason(&self) -> Option<effects::TombstoneReason> {
        *self.tombstone.read().expect("poisoned object lock")
    }

    fn set_tombstone_reason(&self, reason: effects::TombstoneReason) {
        let mut lock = self.tombstone.write().expect("poisoned object lock");
        *lock = Some(reason);
    }

    fn clear_tombstone(&self) {
        let mut lock = self.tombstone.write().expect("poisoned object lock");
        *lock = None;
    }

    fn set_reference(&self, reference: ObjectReference) {
        debug_assert_eq!(
            *reference.object_id(),
            self.object_id,
            "attempted to update handle for wrong object id"
        );

        let mut lock = self.reference.write().expect("poisoned object lock");
        if reference.version() >= lock.version() {
            *lock = reference;
        }
    }
}

/// Runtime-owned handle for an on-chain object used as a transaction input.
///
/// This is the ergonomic “seamless handle” variant: it carries the Rust type `T` while storing
/// the mutable `ObjectReference` behind interior mutability. The runtime updates it after commit.
///
/// Unlike `sui-move-call::MoveObject<T>`, this handle also tracks:
/// - the latest known on-chain owner kind (owned/immutable/shared/...)
/// - tombstone status (deleted/wrapped/not-exist)
///
/// When converted into a `CallArg`, the handle chooses the correct Sui input shape based on its
/// current owner kind:
/// - immutable/address-owned → `Input::ImmutableOrOwned(ObjectReference)`
/// - shared-like (`Owner::Shared` or `Owner::ConsensusAddress`) → `Input::Shared(SharedInput)` (immutable by default)
///
/// If the object becomes a child object, is tombstoned, or the owner kind is unknown, conversion
/// fails early with a [`sui_move_call::CallArgError`].
///
/// `Object<T>` implements [`ToCallArg`], so you can pass it to `sui-move-call` interface functions
/// that accept `&impl ToCallArg` and push it into a `CallSpec`.
///
/// # Example
/// ```rust,no_run
/// use sui_move_runtime::prelude::*;
/// use sui_move::{coin::Coin, sui::SUI};
///
/// fn touch(coin: &impl ToCallArg) -> CallSpec {
///     let package: sui_sdk_types::Address = "0x1".parse().unwrap();
///     let mut spec = CallSpec::new(package, "demo", "touch").unwrap();
///     spec.push_arg(coin).unwrap();
///     spec
/// }
///
/// # async fn demo(mut rt: Runtime<impl sui_crypto::SuiSigner>, sender: sui_sdk_types::Address) -> Result<(), Error> {
/// let coin: Object<Coin<SUI>> = rt.read().object("0x2".parse().unwrap()).await?;
/// let ptb = sui_move_ptb::ptb! { touch(&coin); }?;
/// rt.tx(sender).commit(ptb).await?;
/// # Ok(())
/// # }
/// ```
pub struct Object<T> {
    cell: Arc<ObjectCell>,
    phantom: PhantomData<T>,
}

impl<T> Clone for Object<T> {
    fn clone(&self) -> Self {
        Self {
            cell: Arc::clone(&self.cell),
            phantom: PhantomData,
        }
    }
}

impl<T> fmt::Debug for Object<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Object")
            .field("object_id", &self.object_id())
            .field("reference", &self.reference())
            .finish()
    }
}

impl<T> Object<T> {
    /// Object id.
    pub fn object_id(&self) -> Address {
        self.cell.object_id
    }

    /// Snapshot of the current `ObjectReference`.
    ///
    /// This value is updated after [`crate::Tx::commit`] (if the object was changed by the
    /// committed transaction). It is not automatically refreshed when other transactions mutate
    /// the object on-chain.
    pub fn reference(&self) -> ObjectReference {
        self.cell.reference()
    }

    /// Return an immutable shared view of this object, if it is shared-like on-chain.
    pub fn shared_immutable(&self) -> Result<SharedObject<T>, sui_move_call::CallArgError> {
        let initial_shared_version = self.shared_start_version()?;
        Ok(SharedObject::immutable(
            self.object_id(),
            initial_shared_version,
        ))
    }

    /// Return a mutable shared view of this object, if it is shared-like on-chain.
    pub fn shared_mutable(&self) -> Result<SharedObject<T>, sui_move_call::CallArgError> {
        let initial_shared_version = self.shared_start_version()?;
        Ok(SharedObject::mutable(
            self.object_id(),
            initial_shared_version,
        ))
    }

    /// Return a receiving view of this object, if it is address-owned.
    ///
    /// Receiving is a transaction input mode used for `sui::transfer::Receiving<T>`: it allows
    /// receiving an object that was transferred to an address that is also an object ID (transfer-to-object).
    ///
    /// This helper validates only the coarse owner kind:
    /// - allowed: address-owned objects
    /// - rejected: shared-like, immutable, child objects, tombstoned objects
    ///
    /// On-chain, Sui also checks that the object can be received through the specific parent
    /// object you prove mutable access to (this runtime does not try to pre-validate that).
    pub fn receiving(&self) -> Result<ReceivingObject<T>, sui_move_call::CallArgError> {
        self.ensure_not_tombstoned()?;

        let owner = self.cell.owner.read().expect("poisoned object lock");
        match tx::classify_owner(&owner) {
            tx::OwnerKind::AddressOwned => Ok(ReceivingObject {
                cell: Arc::clone(&self.cell),
                phantom: PhantomData,
            }),
            other => Err(sui_move_call::CallArgError::ObjectKind {
                object_id: self.object_id(),
                expected: "address-owned",
                actual: other.label(),
            }),
        }
    }

    fn ensure_not_tombstoned(&self) -> Result<(), sui_move_call::CallArgError> {
        if let Some(reason) = self.cell.tombstone_reason() {
            return Err(sui_move_call::CallArgError::Tombstoned {
                object_id: self.object_id(),
                reason: reason.label(),
            });
        }
        Ok(())
    }

    fn shared_start_version(&self) -> Result<u64, sui_move_call::CallArgError> {
        self.ensure_not_tombstoned()?;

        let owner = self.cell.owner.read().expect("poisoned object lock");
        let kind = tx::classify_owner(&owner);
        if !kind.is_shared_like() {
            return Err(sui_move_call::CallArgError::ObjectKind {
                object_id: self.object_id(),
                expected: "shared",
                actual: kind.label(),
            });
        };

        let Some(initial_shared_version) = kind.shared_start_version() else {
            return Err(sui_move_call::CallArgError::ObjectKind {
                object_id: self.object_id(),
                expected: "shared",
                actual: kind.label(),
            });
        };

        Ok(initial_shared_version)
    }
}

impl<T: sui_move::MoveStruct + sui_move::HasKey> ToCallArg for Object<T> {
    fn to_call_arg(&self) -> Result<CallArg, sui_move_call::CallArgError> {
        self.ensure_not_tombstoned()?;

        let owner = self.cell.owner.read().expect("poisoned object lock");
        match tx::classify_owner(&owner) {
            tx::OwnerKind::Immutable | tx::OwnerKind::AddressOwned => {
                Ok(CallArg::ImmutableOrOwned(self.reference()))
            }
            kind if kind.is_shared_like() => {
                let Some(initial_shared_version) = kind.shared_start_version() else {
                    return Err(sui_move_call::CallArgError::ObjectKind {
                        object_id: self.object_id(),
                        expected: "shared",
                        actual: kind.label(),
                    });
                };

                Ok(CallArg::Shared(SharedInput::new(
                    self.object_id(),
                    initial_shared_version,
                    Mutability::Immutable,
                )))
            }
            other => Err(sui_move_call::CallArgError::ObjectKind {
                object_id: self.object_id(),
                expected: "immutable-or-owned or shared",
                actual: other.label(),
            }),
        }
    }
}

/// Runtime-owned handle for a receiving object input.
///
/// This is the runtime-owned counterpart of Sui's `Input::Receiving`.
pub struct ReceivingObject<T> {
    cell: Arc<ObjectCell>,
    phantom: PhantomData<T>,
}

impl<T> Clone for ReceivingObject<T> {
    fn clone(&self) -> Self {
        Self {
            cell: Arc::clone(&self.cell),
            phantom: PhantomData,
        }
    }
}

impl<T> fmt::Debug for ReceivingObject<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ReceivingObject")
            .field("object_id", &self.object_id())
            .field("reference", &self.reference())
            .finish()
    }
}

impl<T> ReceivingObject<T> {
    /// Object id.
    pub fn object_id(&self) -> Address {
        self.cell.object_id
    }

    /// Snapshot of the current `ObjectReference`.
    ///
    /// This value is updated after [`crate::Tx::commit`] (if the object was changed by the
    /// committed transaction).
    pub fn reference(&self) -> ObjectReference {
        self.cell.reference()
    }
}

impl<T: sui_move::MoveStruct + sui_move::HasKey> ToCallArg for ReceivingObject<T> {
    fn to_call_arg(&self) -> Result<CallArg, sui_move_call::CallArgError> {
        let obj = Object::<T> {
            cell: Arc::clone(&self.cell),
            phantom: PhantomData,
        };

        obj.ensure_not_tombstoned()?;

        let owner = obj.cell.owner.read().expect("poisoned object lock");
        match tx::classify_owner(&owner) {
            tx::OwnerKind::AddressOwned => Ok(CallArg::Receiving(self.reference())),
            other => Err(sui_move_call::CallArgError::ObjectKind {
                object_id: self.object_id(),
                expected: "address-owned",
                actual: other.label(),
            }),
        }
    }
}

/// Typed handle for a shared object input.
///
/// Shared inputs are stable across mutations (they use `initial_shared_version`), so they do not
/// require runtime updates. This type exists for ergonomics and symmetry with `Object<T>`.
///
/// # Example
/// ```
/// use sui_move::{coin::Coin, sui::SUI};
/// use sui_move_call::{CallArg, CallSpec};
/// use sui_move_runtime::SharedObject;
/// use sui_sdk_types::Address;
///
/// let package: Address = "0x1".parse().unwrap();
/// let shared = SharedObject::<Coin<SUI>>::mutable("0x2".parse().unwrap(), 1);
///
/// let mut spec = CallSpec::new(package, "m", "f").unwrap();
/// spec.push_arg(&shared).unwrap();
///
/// assert!(matches!(spec.arguments[0], CallArg::Shared(_)));
/// ```
pub struct SharedObject<T> {
    input: SharedInput,
    phantom: PhantomData<T>,
}

impl<T> Clone for SharedObject<T> {
    fn clone(&self) -> Self {
        Self {
            input: self.input.clone(),
            phantom: PhantomData,
        }
    }
}

impl<T> fmt::Debug for SharedObject<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SharedObject")
            .field("object_id", &self.object_id())
            .field("initial_shared_version", &self.initial_shared_version())
            .field("mutability", &self.mutability())
            .finish()
    }
}

impl<T> PartialEq for SharedObject<T> {
    fn eq(&self, other: &Self) -> bool {
        self.input == other.input
    }
}

impl<T> Eq for SharedObject<T> {}

impl<T> SharedObject<T> {
    /// Create a shared object handle with an explicit mutability mode.
    pub fn new(object_id: Address, initial_shared_version: u64, mutability: Mutability) -> Self {
        Self {
            input: SharedInput::new(object_id, initial_shared_version, mutability),
            phantom: PhantomData,
        }
    }

    /// Create an immutable shared object handle.
    pub fn immutable(object_id: Address, initial_shared_version: u64) -> Self {
        Self::new(object_id, initial_shared_version, Mutability::Immutable)
    }

    /// Create a mutable shared object handle.
    pub fn mutable(object_id: Address, initial_shared_version: u64) -> Self {
        Self::new(object_id, initial_shared_version, Mutability::Mutable)
    }

    /// Shared object ID.
    pub fn object_id(&self) -> Address {
        self.input.object_id()
    }

    /// Initial shared version of the object.
    pub fn initial_shared_version(&self) -> u64 {
        self.input.version()
    }

    /// Requested mutability mode for this shared object argument.
    pub fn mutability(&self) -> Mutability {
        self.input.mutability()
    }
}

impl<T: sui_move::MoveStruct + sui_move::HasKey> ToCallArg for SharedObject<T> {
    fn to_call_arg(&self) -> Result<CallArg, sui_move_call::CallArgError> {
        Ok(CallArg::Shared(self.input.clone()))
    }
}

/// Convenience wrapper for “some object” whose on-chain ownership might be owned/immutable or shared.
///
/// This wrapper exists for ergonomics when you don't know whether an on-chain object is
/// immutable/owned or shared:
/// - fetch a handle (`Read::object_any`)
/// - pass it to interface functions as `&impl ToCallArg`
///
/// If the object is shared on-chain, this wrapper **defaults to immutable shared** when converted
/// to a `CallArg`. If you need mutable shared access, call [`AnyObject::as_shared_mutable`].
///
/// # Example
/// ```rust,no_run
/// use sui_move_runtime::prelude::*;
/// use sui_move::{coin::Coin, sui::SUI};
///
/// fn touch(obj: &impl ToCallArg) -> CallSpec {
///     let package: sui_sdk_types::Address = "0x1".parse().unwrap();
///     let mut spec = CallSpec::new(package, "demo", "touch").unwrap();
///     spec.push_arg(obj).unwrap();
///     spec
/// }
///
/// # async fn demo(mut rt: Runtime<impl sui_crypto::SuiSigner>, sender: sui_sdk_types::Address) -> Result<(), Error> {
/// let any: AnyObject<Coin<SUI>> = rt.read().object_any("0x2".parse().unwrap()).await?;
///
/// // Works for both owned/immutable and shared objects (shared defaults to immutable).
/// let ptb = sui_move_ptb::ptb! { touch(&any); }?;
/// rt.tx(sender).commit(ptb).await?;
///
/// // If it is shared and you need &mut on the Move side:
/// let shared_mut = any.as_shared_mutable()?;
/// let ptb = sui_move_ptb::ptb! { touch(&shared_mut); }?;
/// rt.tx(sender).commit(ptb).await?;
/// # Ok(())
/// # }
/// ```
pub struct AnyObject<T> {
    inner: AnyObjectInner<T>,
}

enum AnyObjectInner<T> {
    ImmutableOrOwned(Object<T>),
    Shared(SharedObject<T>),
}

impl<T> Clone for AnyObject<T> {
    fn clone(&self) -> Self {
        Self {
            inner: match &self.inner {
                AnyObjectInner::ImmutableOrOwned(o) => AnyObjectInner::ImmutableOrOwned(o.clone()),
                AnyObjectInner::Shared(s) => AnyObjectInner::Shared(s.clone()),
            },
        }
    }
}

impl<T> fmt::Debug for AnyObject<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.inner {
            AnyObjectInner::ImmutableOrOwned(o) => f
                .debug_tuple("AnyObject::ImmutableOrOwned")
                .field(o)
                .finish(),
            AnyObjectInner::Shared(s) => f.debug_tuple("AnyObject::Shared").field(s).finish(),
        }
    }
}

impl<T> AnyObject<T> {
    pub(crate) fn from_object(object: Object<T>) -> Self {
        Self {
            inner: AnyObjectInner::ImmutableOrOwned(object),
        }
    }

    pub(crate) fn from_shared(shared: SharedObject<T>) -> Self {
        Self {
            inner: AnyObjectInner::Shared(shared),
        }
    }

    /// Object id.
    pub fn object_id(&self) -> Address {
        match &self.inner {
            AnyObjectInner::ImmutableOrOwned(o) => o.object_id(),
            AnyObjectInner::Shared(s) => s.object_id(),
        }
    }

    /// Returns `true` if this object is shared on-chain.
    pub fn is_shared(&self) -> bool {
        matches!(self.inner, AnyObjectInner::Shared(_))
    }

    /// Return the owned/immutable handle, if the object is not shared.
    pub fn as_object(&self) -> Result<Object<T>, crate::Error> {
        match &self.inner {
            AnyObjectInner::ImmutableOrOwned(o) => Ok(o.clone()),
            AnyObjectInner::Shared(s) => Err(crate::Error::ObjectKind {
                object_id: s.object_id(),
                expected: "immutable-or-owned",
                actual: "shared",
            }),
        }
    }

    /// Return an immutable shared handle, if the object is shared.
    pub fn as_shared_immutable(&self) -> Result<SharedObject<T>, crate::Error> {
        match &self.inner {
            AnyObjectInner::Shared(s) => Ok(s.clone()),
            AnyObjectInner::ImmutableOrOwned(o) => Err(crate::Error::ObjectKind {
                object_id: o.object_id(),
                expected: "shared",
                actual: "immutable-or-owned",
            }),
        }
    }

    /// Return a mutable shared handle, if the object is shared.
    pub fn as_shared_mutable(&self) -> Result<SharedObject<T>, crate::Error> {
        match &self.inner {
            AnyObjectInner::Shared(s) => Ok(SharedObject::mutable(
                s.object_id(),
                s.initial_shared_version(),
            )),
            AnyObjectInner::ImmutableOrOwned(o) => Err(crate::Error::ObjectKind {
                object_id: o.object_id(),
                expected: "shared",
                actual: "immutable-or-owned",
            }),
        }
    }
}

impl<T: sui_move::MoveStruct + sui_move::HasKey> ToCallArg for AnyObject<T> {
    fn to_call_arg(&self) -> Result<CallArg, sui_move_call::CallArgError> {
        match &self.inner {
            AnyObjectInner::ImmutableOrOwned(o) => o.to_call_arg(),
            AnyObjectInner::Shared(s) => s.to_call_arg(),
        }
    }
}

#[derive(Default, Debug)]
pub(crate) struct Registry {
    objects: Mutex<HashMap<Address, Weak<ObjectCell>>>,
}

impl Registry {
    pub(crate) fn intern_object<T: sui_move::MoveStruct + sui_move::HasKey>(
        &self,
        reference: ObjectReference,
        owner: Owner,
    ) -> Object<T> {
        let cell = self.intern_cell(reference, owner);
        Object {
            cell,
            phantom: PhantomData,
        }
    }

    pub(crate) fn intern_receiving_object<T: sui_move::MoveStruct + sui_move::HasKey>(
        &self,
        reference: ObjectReference,
        owner: Owner,
    ) -> ReceivingObject<T> {
        let cell = self.intern_cell(reference, owner);
        ReceivingObject {
            cell,
            phantom: PhantomData,
        }
    }

    fn intern_cell(&self, reference: ObjectReference, owner: Owner) -> Arc<ObjectCell> {
        let object_id = *reference.object_id();

        let mut map = self.objects.lock().expect("poisoned registry lock");
        if let Some(existing) = map.get(&object_id).and_then(Weak::upgrade) {
            existing.set_reference(reference);
            existing.set_owner(owner);
            existing.clear_tombstone();
            return existing;
        }

        let cell = Arc::new(ObjectCell::new(reference, owner));
        map.insert(object_id, Arc::downgrade(&cell));
        cell
    }

    pub(crate) fn apply_effects(&self, effects_in: &sui_sdk_types::TransactionEffects) {
        let patch = effects::EffectsPatch::from_effects(effects_in);
        let tombstones: HashMap<Address, effects::TombstoneReason> = patch
            .tombstones
            .into_iter()
            .map(|tombstone| (tombstone.object_id, tombstone.reason))
            .collect();

        for (object_id, reason) in &tombstones {
            if let Some(cell) = self.upgrade(*object_id) {
                cell.set_tombstone_reason(*reason);
            }
        }

        for upsert in patch.upserts {
            if tombstones.contains_key(&upsert.object_id) {
                continue;
            }
            if let Some(cell) = self.upgrade(upsert.object_id) {
                cell.set_reference(upsert.reference);
                if let Some(owner) = upsert.owner {
                    cell.set_owner(owner);
                }
                cell.clear_tombstone();
            }
        }
    }

    fn upgrade(&self, object_id: Address) -> Option<Arc<ObjectCell>> {
        let mut map = self.objects.lock().expect("poisoned registry lock");
        let weak = map.get(&object_id).cloned()?;

        match weak.upgrade() {
            Some(cell) => Some(cell),
            None => {
                map.remove(&object_id);
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sui_sdk_types::{
        ChangedObject, Digest, ExecutionStatus, GasCostSummary, IdOperation, Mutability, ObjectIn,
        ObjectOut, Owner, TransactionEffects, TransactionEffectsV2,
    };

    #[sui_move::move_struct(address = "0x1", module = "demo", abilities = "key")]
    struct Demo {
        id: sui_move::types::UID,
    }

    #[test]
    fn registry_interns_cells_by_object_id() {
        let id = Address::from_hex("0x2").unwrap();
        let a = ObjectReference::new(id, 1, Digest::default());
        let b = ObjectReference::new(id, 2, Digest::default());

        let registry = Registry::default();
        let obj_a: Object<Demo> = registry.intern_object(a, Owner::Immutable);
        let obj_b: Object<Demo> = registry.intern_object(b, Owner::Immutable);

        assert_eq!(obj_a.object_id(), id);
        assert_eq!(obj_b.object_id(), id);
        assert_eq!(obj_a.reference().version(), 2);
        assert_eq!(obj_b.reference().version(), 2);
    }

    #[test]
    fn apply_effects_updates_live_handles() {
        let id = Address::from_hex("0x2").unwrap();
        let old = ObjectReference::new(id, 1, Digest::default());

        let registry = Registry::default();
        let obj: Object<Demo> = registry.intern_object(old, Owner::Immutable);
        assert_eq!(obj.reference().version(), 1);

        let effects = TransactionEffects::V2(Box::new(TransactionEffectsV2 {
            status: ExecutionStatus::Success,
            epoch: 0,
            gas_used: GasCostSummary::new(0, 0, 0, 0),
            transaction_digest: Digest::default(),
            gas_object_index: None,
            events_digest: None,
            dependencies: vec![],
            lamport_version: 7,
            changed_objects: vec![ChangedObject {
                object_id: id,
                input_state: ObjectIn::Exist {
                    version: 1,
                    digest: Digest::default(),
                    owner: Owner::Immutable,
                },
                output_state: ObjectOut::ObjectWrite {
                    digest: Digest::default(),
                    owner: Owner::Immutable,
                },
                id_operation: IdOperation::None,
            }],
            unchanged_consensus_objects: vec![],
            auxiliary_data_digest: None,
        }));

        registry.apply_effects(&effects);
        assert_eq!(obj.reference().version(), 7);
    }

    #[test]
    fn object_defaults_shared_to_immutable() {
        let id = Address::from_hex("0x2").unwrap();
        let reference = ObjectReference::new(id, 1, Digest::default());

        let registry = Registry::default();
        let obj: Object<Demo> = registry.intern_object(reference, Owner::Shared(7));

        let arg = obj.to_call_arg().unwrap();
        let CallArg::Shared(shared) = arg else {
            panic!("expected shared call arg")
        };

        assert_eq!(shared.object_id(), id);
        assert_eq!(shared.version(), 7);
        assert_eq!(shared.mutability(), Mutability::Immutable);
    }

    #[test]
    fn object_to_call_arg_fails_for_tombstoned_objects() {
        let id = Address::from_hex("0x2").unwrap();
        let reference = ObjectReference::new(id, 1, Digest::default());

        let registry = Registry::default();
        let obj: Object<Demo> = registry.intern_object(reference, Owner::Immutable);

        let effects = TransactionEffects::V2(Box::new(TransactionEffectsV2 {
            status: ExecutionStatus::Success,
            epoch: 0,
            gas_used: GasCostSummary::new(0, 0, 0, 0),
            transaction_digest: Digest::default(),
            gas_object_index: None,
            events_digest: None,
            dependencies: vec![],
            lamport_version: 7,
            changed_objects: vec![ChangedObject {
                object_id: id,
                input_state: ObjectIn::Exist {
                    version: 1,
                    digest: Digest::default(),
                    owner: Owner::Immutable,
                },
                output_state: ObjectOut::NotExist,
                id_operation: IdOperation::None,
            }],
            unchanged_consensus_objects: vec![],
            auxiliary_data_digest: None,
        }));

        registry.apply_effects(&effects);
        let err = obj.to_call_arg().unwrap_err();

        match err {
            sui_move_call::CallArgError::Tombstoned { object_id, reason } => {
                assert_eq!(object_id, id);
                assert_eq!(reason, "not-exist");
            }
            other => panic!("expected tombstoned error, got {other:?}"),
        }
    }

    #[test]
    fn object_to_call_arg_fails_for_child_objects() {
        let id = Address::from_hex("0x2").unwrap();
        let parent = Address::from_hex("0x3").unwrap();
        let reference = ObjectReference::new(id, 1, Digest::default());

        let registry = Registry::default();
        let obj: Object<Demo> = registry.intern_object(reference, Owner::Object(parent));

        let err = obj.to_call_arg().unwrap_err();
        match err {
            sui_move_call::CallArgError::ObjectKind {
                object_id,
                expected,
                actual,
            } => {
                assert_eq!(object_id, id);
                assert_eq!(expected, "immutable-or-owned or shared");
                assert_eq!(actual, "child-object");
            }
            other => panic!("expected object kind error, got {other:?}"),
        }
    }

    #[test]
    fn object_shared_mutable_view_is_explicit() {
        let id = Address::from_hex("0x2").unwrap();
        let reference = ObjectReference::new(id, 1, Digest::default());

        let registry = Registry::default();
        let obj: Object<Demo> = registry.intern_object(reference, Owner::Shared(7));

        let shared_mut = obj.shared_mutable().unwrap();
        let CallArg::Shared(shared) = shared_mut.to_call_arg().unwrap() else {
            panic!("expected shared call arg")
        };
        assert_eq!(shared.object_id(), id);
        assert_eq!(shared.version(), 7);
        assert_eq!(shared.mutability(), Mutability::Mutable);
    }

    #[test]
    fn object_receiving_view_requires_address_owned() {
        let id = Address::from_hex("0x2").unwrap();
        let owner = Address::from_hex("0x3").unwrap();
        let reference = ObjectReference::new(id, 1, Digest::default());

        let registry = Registry::default();
        let obj: Object<Demo> = registry.intern_object(reference.clone(), Owner::Address(owner));

        let recv = obj.receiving().unwrap();
        let arg = recv.to_call_arg().unwrap();
        assert_eq!(arg, CallArg::Receiving(reference.clone()));

        let obj: Object<Demo> = registry.intern_object(reference.clone(), Owner::Shared(7));
        let err = obj.receiving().unwrap_err();
        match err {
            sui_move_call::CallArgError::ObjectKind {
                object_id,
                expected,
                actual,
            } => {
                assert_eq!(object_id, id);
                assert_eq!(expected, "address-owned");
                assert_eq!(actual, "shared");
            }
            other => panic!("expected object kind error, got {other:?}"),
        }
    }

    #[test]
    fn apply_effects_tombstones_deleted_objects() {
        let id = Address::from_hex("0x2").unwrap();
        let old = ObjectReference::new(id, 1, Digest::default());

        let registry = Registry::default();
        let obj: Object<Demo> = registry.intern_object(old, Owner::Immutable);
        assert_eq!(obj.cell.tombstone_reason(), None);

        let effects = TransactionEffects::V2(Box::new(TransactionEffectsV2 {
            status: ExecutionStatus::Success,
            epoch: 0,
            gas_used: GasCostSummary::new(0, 0, 0, 0),
            transaction_digest: Digest::default(),
            gas_object_index: None,
            events_digest: None,
            dependencies: vec![],
            lamport_version: 7,
            changed_objects: vec![ChangedObject {
                object_id: id,
                input_state: ObjectIn::Exist {
                    version: 1,
                    digest: Digest::default(),
                    owner: Owner::Immutable,
                },
                output_state: ObjectOut::NotExist,
                id_operation: IdOperation::None,
            }],
            unchanged_consensus_objects: vec![],
            auxiliary_data_digest: None,
        }));

        registry.apply_effects(&effects);
        assert_eq!(
            obj.cell.tombstone_reason(),
            Some(effects::TombstoneReason::NotExist)
        );
    }

    #[test]
    fn any_object_defaults_shared_to_immutable() {
        let id = Address::from_hex("0x2").unwrap();

        let any = AnyObject::<Demo>::from_shared(SharedObject::immutable(id, 1));
        let arg = any.to_call_arg().unwrap();

        let CallArg::Shared(shared) = arg else {
            panic!("expected shared call arg")
        };

        assert_eq!(shared.object_id(), id);
        assert_eq!(shared.version(), 1);
        assert_eq!(shared.mutability(), Mutability::Immutable);

        let shared_mut = any.as_shared_mutable().unwrap();
        let CallArg::Shared(shared_mut_arg) = shared_mut.to_call_arg().unwrap() else {
            panic!("expected shared call arg")
        };
        assert_eq!(shared_mut_arg.mutability(), Mutability::Mutable);
    }

    #[test]
    fn any_object_owned_delegates_to_immutable_or_owned() {
        let id = Address::from_hex("0x2").unwrap();
        let reference = ObjectReference::new(id, 1, Digest::default());

        let registry = Registry::default();
        let obj: Object<Demo> = registry.intern_object(reference.clone(), Owner::Immutable);
        let any = AnyObject::<Demo>::from_object(obj);

        assert!(!any.is_shared());

        let arg = any.to_call_arg().unwrap();
        let CallArg::ImmutableOrOwned(arg_ref) = arg else {
            panic!("expected immutable-or-owned call arg")
        };
        assert_eq!(arg_ref, reference);

        any.as_object().unwrap();
        assert!(any.as_shared_mutable().is_err());
    }
}
