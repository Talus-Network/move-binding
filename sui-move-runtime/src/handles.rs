use std::collections::HashMap;
use std::fmt;
use std::marker::PhantomData;
use std::sync::{Arc, Mutex, RwLock, Weak};

use sui_move_call::{CallArg, ToCallArg};
use sui_sdk_types::{Address, Mutability, ObjectReference, SharedInput};

use crate::effects;

#[derive(Debug)]
struct ObjectCell {
    object_id: Address,
    reference: RwLock<ObjectReference>,
}

impl ObjectCell {
    fn new(reference: ObjectReference) -> Self {
        Self {
            object_id: *reference.object_id(),
            reference: RwLock::new(reference),
        }
    }

    fn reference(&self) -> ObjectReference {
        self.reference.read().expect("poisoned object lock").clone()
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

/// Runtime-owned handle for an immutable-or-owned object input.
///
/// This is the ergonomic “seamless handle” variant: it carries the Rust type `T` while storing
/// the mutable `ObjectReference` behind interior mutability. The runtime updates it after commit.
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
/// move_time!(rt, sender, { touch(&coin); }).await?;
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
    /// This value is updated after [`crate::MoveTime::commit`] (if the object was changed by the
    /// committed transaction). It is not automatically refreshed when other transactions mutate
    /// the object on-chain.
    pub fn reference(&self) -> ObjectReference {
        self.cell.reference()
    }
}

impl<T: sui_move::MoveStruct + sui_move::HasKey> ToCallArg for Object<T> {
    fn to_call_arg(&self) -> Result<CallArg, sui_move_call::CallArgError> {
        Ok(CallArg::ImmutableOrOwned(self.reference()))
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
    /// This value is updated after [`crate::MoveTime::commit`] (if the object was changed by the
    /// committed transaction).
    pub fn reference(&self) -> ObjectReference {
        self.cell.reference()
    }
}

impl<T: sui_move::MoveStruct + sui_move::HasKey> ToCallArg for ReceivingObject<T> {
    fn to_call_arg(&self) -> Result<CallArg, sui_move_call::CallArgError> {
        Ok(CallArg::Receiving(self.reference()))
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

#[derive(Default, Debug)]
pub(crate) struct Registry {
    objects: Mutex<HashMap<Address, Weak<ObjectCell>>>,
}

impl Registry {
    pub(crate) fn intern_object<T: sui_move::MoveStruct + sui_move::HasKey>(
        &self,
        reference: ObjectReference,
    ) -> Object<T> {
        let cell = self.intern_cell(reference);
        Object {
            cell,
            phantom: PhantomData,
        }
    }

    pub(crate) fn intern_receiving_object<T: sui_move::MoveStruct + sui_move::HasKey>(
        &self,
        reference: ObjectReference,
    ) -> ReceivingObject<T> {
        let cell = self.intern_cell(reference);
        ReceivingObject {
            cell,
            phantom: PhantomData,
        }
    }

    fn intern_cell(&self, reference: ObjectReference) -> Arc<ObjectCell> {
        let object_id = *reference.object_id();

        let mut map = self.objects.lock().expect("poisoned registry lock");
        if let Some(existing) = map.get(&object_id).and_then(Weak::upgrade) {
            existing.set_reference(reference);
            return existing;
        }

        let cell = Arc::new(ObjectCell::new(reference));
        map.insert(object_id, Arc::downgrade(&cell));
        cell
    }

    pub(crate) fn apply_effects(&self, effects_in: &sui_sdk_types::TransactionEffects) {
        for update in effects::updated_references(effects_in) {
            if let Some(cell) = self.upgrade(update.object_id) {
                cell.set_reference(update.reference);
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
        ChangedObject, Digest, ExecutionStatus, GasCostSummary, IdOperation, ObjectIn, ObjectOut,
        Owner, TransactionEffects, TransactionEffectsV2,
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
        let obj_a: Object<Demo> = registry.intern_object(a);
        let obj_b: Object<Demo> = registry.intern_object(b);

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
        let obj: Object<Demo> = registry.intern_object(old);
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
}
