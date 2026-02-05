#![doc = include_str!("../README.md")]
#![deny(missing_docs)]

use std::marker::PhantomData;
use std::str::FromStr;

use sui_move::{HasKey, MoveStruct, MoveType};
use sui_sdk_types::{
    Address, Argument, Identifier, Mutability, ObjectReference, SharedInput, TypeTag,
};

/// Canonical Sui transaction input kind.
///
/// This is a re-export of [`sui_sdk_types::Input`]. `sui-move-call` uses it directly so this
/// crate can represent every on-chain input kind without re-modeling Sui's wire types.
///
/// In this crate, call arguments are typically produced via [`ToCallArg`].
pub use sui_sdk_types::Input as CallArg;

/// A single argument to a Move call.
///
/// This is either:
/// - a raw transaction input ([`CallArg`]) that must be allocated into the PTB input table, or
/// - an already-resolved programmable transaction argument ([`Argument`]), typically the result
///   of a prior PTB command.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum CallArgument {
    /// A raw transaction input.
    Input(CallArg),
    /// An already-resolved PTB argument.
    Argument(Argument),
}

/// Typed object handle for key-bearing Move structs.
///
/// This is a small wrapper around [`ObjectReference`] that carries the Rust type `T`.
///
/// `MoveObject<T>` is intentionally small: it does not fetch object contents, and it does not
/// model shared or receiving objects. Higher layers can build on top.
///
/// Semantically, this corresponds to Sui's [`CallArg::ImmutableOrOwned`] input kind: use
/// [`SharedMoveObject`] for shared objects and [`ReceivingMoveObject`] for receiving objects.
///
/// # Example
/// ```
/// use std::str::FromStr;
/// use sui_move_call::MoveObject;
/// use sui_sdk_types::{Address, Digest, ObjectReference};
///
/// #[sui_move::move_struct(address = "0x2", module = "object", abilities = "copy, drop, store")]
/// struct ID {
///     bytes: Address,
/// }
///
/// #[sui_move::move_struct(address = "0x2", module = "object", abilities = "store")]
/// struct UID {
///     id: ID,
/// }
///
/// #[sui_move::move_struct(address = "0x1", module = "demo", abilities = "key")]
/// struct Demo {
///     id: UID,
/// }
///
/// let id = Address::from_str("0x1").unwrap();
/// let obj_ref = ObjectReference::new(id, 1, Digest::default());
/// let _obj = MoveObject::<Demo>::new(obj_ref);
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MoveObject<T> {
    reference: ObjectReference,
    phantom: PhantomData<T>,
}

impl<T: MoveStruct + HasKey> MoveObject<T> {
    /// Create a typed handle from an object reference.
    pub fn new(reference: ObjectReference) -> Self {
        Self {
            reference,
            phantom: PhantomData,
        }
    }

    /// Borrow the underlying object reference.
    pub fn reference(&self) -> &ObjectReference {
        &self.reference
    }

    /// Consume this handle and return the underlying object reference.
    pub fn into_reference(self) -> ObjectReference {
        self.reference
    }

    /// Replace the stored reference (useful after transaction effects).
    pub fn update_reference(&mut self, reference: ObjectReference) {
        self.reference = reference;
    }
}

/// Typed handle for a shared object argument.
///
/// Shared objects are passed by `(object_id, initial_shared_version, mutability)` rather than an
/// [`ObjectReference`]. This wrapper models that shape while carrying the expected Move type `T`.
///
/// # Example
/// ```
/// use std::str::FromStr;
/// use sui_move_call::SharedMoveObject;
/// use sui_sdk_types::Address;
///
/// #[sui_move::move_struct(address = "0x2", module = "object", abilities = "copy, drop, store")]
/// struct ID {
///     bytes: Address,
/// }
///
/// #[sui_move::move_struct(address = "0x2", module = "object", abilities = "store")]
/// struct UID {
///     id: ID,
/// }
///
/// #[sui_move::move_struct(address = "0x1", module = "demo", abilities = "key")]
/// struct SharedThing {
///     id: UID,
/// }
///
/// let object_id = Address::from_str("0x1").unwrap();
/// let initial_shared_version = 1u64;
/// let _shared = SharedMoveObject::<SharedThing>::mutable(object_id, initial_shared_version);
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SharedMoveObject<T> {
    input: SharedInput,
    phantom: PhantomData<T>,
}

impl<T: MoveStruct + HasKey> SharedMoveObject<T> {
    /// Create an immutable shared object handle.
    pub fn immutable(object_id: Address, initial_shared_version: u64) -> Self {
        Self::new(object_id, initial_shared_version, Mutability::Immutable)
    }

    /// Create a mutable shared object handle.
    pub fn mutable(object_id: Address, initial_shared_version: u64) -> Self {
        Self::new(object_id, initial_shared_version, Mutability::Mutable)
    }

    /// Create a non-exclusive-write shared object handle.
    pub fn non_exclusive_write(object_id: Address, initial_shared_version: u64) -> Self {
        Self::new(
            object_id,
            initial_shared_version,
            Mutability::NonExclusiveWrite,
        )
    }

    /// Create a shared object handle with an explicit mutability mode.
    pub fn new(object_id: Address, initial_shared_version: u64, mutability: Mutability) -> Self {
        Self {
            input: SharedInput::new(object_id, initial_shared_version, mutability),
            phantom: PhantomData,
        }
    }

    /// Borrow the underlying shared-input description.
    pub fn shared_input(&self) -> &SharedInput {
        &self.input
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

    /// Convenience: whether the requested mutability is writable.
    pub fn is_mutable(&self) -> bool {
        self.mutability().is_mutable()
    }
}

/// Typed handle for a "receiving" object argument.
///
/// This corresponds to Sui's `Input::Receiving(ObjectReference)`.
///
/// Receiving is a **transaction input mode**, not an on-chain owner kind. It is used for the Move
/// framework concept `sui::transfer::Receiving<T>`: an ephemeral per-transaction “receiving
/// ticket” that can be consumed by `sui::transfer::receive`/`public_receive`.
///
/// This wrapper does not validate whether the referenced object can be received in the current
/// transaction; invalid uses are rejected by Sui.
///
/// # Example
/// ```
/// use std::str::FromStr;
/// use sui_move_call::ReceivingMoveObject;
/// use sui_sdk_types::{Address, Digest, ObjectReference};
///
/// #[sui_move::move_struct(address = "0x2", module = "object", abilities = "copy, drop, store")]
/// struct ID {
///     bytes: Address,
/// }
///
/// #[sui_move::move_struct(address = "0x2", module = "object", abilities = "store")]
/// struct UID {
///     id: ID,
/// }
///
/// #[sui_move::move_struct(address = "0x1", module = "demo", abilities = "key")]
/// struct ReceivingThing {
///     id: UID,
/// }
///
/// let id = Address::from_str("0x1").unwrap();
/// let obj_ref = ObjectReference::new(id, 1, Digest::default());
/// let _recv = ReceivingMoveObject::<ReceivingThing>::new(obj_ref);
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReceivingMoveObject<T> {
    reference: ObjectReference,
    phantom: PhantomData<T>,
}

impl<T: MoveStruct + HasKey> ReceivingMoveObject<T> {
    /// Create a typed receiving handle from an object reference.
    pub fn new(reference: ObjectReference) -> Self {
        Self {
            reference,
            phantom: PhantomData,
        }
    }

    /// Borrow the underlying object reference.
    pub fn reference(&self) -> &ObjectReference {
        &self.reference
    }

    /// Consume this handle and return the underlying object reference.
    pub fn into_reference(self) -> ObjectReference {
        self.reference
    }

    /// Replace the stored reference.
    pub fn update_reference(&mut self, reference: ObjectReference) {
        self.reference = reference;
    }
}

/// Errors that can occur when converting values into `CallArg`.
#[derive(thiserror::Error, Debug)]
pub enum CallArgError {
    /// BCS encoding failed.
    #[error(transparent)]
    Bcs(#[from] bcs::Error),

    /// The referenced object is tombstoned (e.g. deleted or wrapped).
    ///
    /// Higher layers may track object liveness across commits and refuse to use stale handles as
    /// inputs.
    #[error("object {object_id} is tombstoned ({reason})")]
    Tombstoned {
        /// Object id that was attempted to be used as an argument.
        object_id: Address,
        /// Human-readable reason (deleted, wrapped, not-exist, ...).
        reason: &'static str,
    },

    /// The referenced object kind does not match what the conversion requires.
    ///
    /// This is primarily used by higher layers that classify on-chain ownership and choose the
    /// correct Sui input mode (owned/immutable vs shared).
    #[error("object {object_id} has kind {actual}, expected {expected}")]
    ObjectKind {
        /// Object id that was attempted to be used as an argument.
        object_id: Address,
        /// Expected kind label (input mode / ownership kind).
        expected: &'static str,
        /// Actual kind label (input mode / ownership kind).
        actual: &'static str,
    },

    /// A mutable shared input was required, but the provided shared handle is immutable.
    #[error("shared object {object_id} has mutability {actual:?}, expected writable")]
    SharedMutability {
        /// Shared object id.
        object_id: Address,
        /// Actual mutability of the provided shared handle.
        actual: Mutability,
    },
}

/// Convert a value into a `CallArg`.
///
/// This is used to build `CallSpec` values ergonomically while keeping ownership simple: the
/// conversion only needs an `&self`.
///
/// `ToCallArg` is intentionally generic:
/// - for `T: MoveType`, it returns a `CallArg::Pure` by BCS-encoding the value
/// - for object handle types, it returns the appropriate object input
///
/// Higher layers may implement `ToCallArg` for runtime-owned handles. Those implementations can
/// fail even without BCS encoding errors (for example, if the handle is stale/tombstoned or if its
/// on-chain kind makes it invalid for the requested input mode).
///
/// # Example
/// ```
/// use sui_move_call::{CallArg, ToCallArg};
///
/// let arg = 7u64.to_call_arg().unwrap();
/// let CallArg::Pure(bytes) = arg else {
///     panic!("expected pure arg");
/// };
/// assert_eq!(bcs::from_bytes::<u64>(&bytes).unwrap(), 7);
/// ```
pub trait ToCallArg {
    /// Convert this value into a `CallArg`.
    fn to_call_arg(&self) -> Result<CallArg, CallArgError>;
}

/// Convert a value into a `CallArg`, forcing a "mutable object input" when applicable.
///
/// This is primarily used by code generation and higher layers that mirror Move `&mut` parameters:
/// shared objects need their transaction input marked mutable, while owned objects use the same
/// `ObjectReference` shape regardless.
pub trait ToCallArgMut: ToCallArg {
    /// Convert this value into a `CallArg` suitable for a Move `&mut` parameter.
    fn to_call_arg_mutable(&self) -> Result<CallArg, CallArgError>;
}

impl<T: MoveType> ToCallArg for T {
    fn to_call_arg(&self) -> Result<CallArg, CallArgError> {
        Ok(CallArg::Pure(self.to_bcs()?))
    }
}

impl<T: MoveStruct + HasKey> ToCallArg for MoveObject<T> {
    fn to_call_arg(&self) -> Result<CallArg, CallArgError> {
        Ok(CallArg::ImmutableOrOwned(self.reference().clone()))
    }
}

impl<T: MoveStruct + HasKey> ToCallArg for SharedMoveObject<T> {
    fn to_call_arg(&self) -> Result<CallArg, CallArgError> {
        Ok(CallArg::Shared(self.input.clone()))
    }
}

impl<T: MoveStruct + HasKey> ToCallArg for ReceivingMoveObject<T> {
    fn to_call_arg(&self) -> Result<CallArg, CallArgError> {
        Ok(CallArg::Receiving(self.reference().clone()))
    }
}

impl<T: MoveStruct + HasKey> ToCallArgMut for MoveObject<T> {
    fn to_call_arg_mutable(&self) -> Result<CallArg, CallArgError> {
        self.to_call_arg()
    }
}

impl<T: MoveStruct + HasKey> ToCallArgMut for SharedMoveObject<T> {
    fn to_call_arg_mutable(&self) -> Result<CallArg, CallArgError> {
        let actual = self.mutability();
        if !actual.is_mutable() {
            return Err(CallArgError::SharedMutability {
                object_id: self.object_id(),
                actual,
            });
        }
        Ok(CallArg::Shared(self.input.clone()))
    }
}

impl<T: MoveStruct + HasKey> ToCallArgMut for ReceivingMoveObject<T> {
    fn to_call_arg_mutable(&self) -> Result<CallArg, CallArgError> {
        self.to_call_arg()
    }
}

/// Typed object arguments.
///
/// Generated interface code uses this trait to accept both owned/immutable and shared object
/// handles while still carrying the expected Move type `T`.
pub trait ObjectArg<T: MoveStruct + HasKey>: ToCallArgMut {}

impl<T: MoveStruct + HasKey> ObjectArg<T> for MoveObject<T> {}
impl<T: MoveStruct + HasKey> ObjectArg<T> for SharedMoveObject<T> {}

/// Typed wrapper around a programmable-transaction [`Argument`].
///
/// When building a PTB, Move call results are represented as [`Argument`] indices, not as actual
/// Rust values. `PtbValue<T>` keeps the type `T` at compile time while carrying the underlying
/// [`Argument`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PtbValue<T> {
    argument: Argument,
    phantom: PhantomData<T>,
}

impl<T> PtbValue<T> {
    /// Wrap an existing PTB argument.
    pub fn new(argument: Argument) -> Self {
        Self {
            argument,
            phantom: PhantomData,
        }
    }

    /// Borrow the underlying PTB argument.
    pub fn as_argument(&self) -> &Argument {
        &self.argument
    }

    /// Clone the underlying PTB argument.
    pub fn argument(&self) -> Argument {
        self.argument.clone()
    }

    /// Consume this wrapper and return the underlying PTB argument.
    pub fn into_argument(self) -> Argument {
        self.argument
    }

    /// Access a nested result value.
    ///
    /// This is useful when a Move call returns multiple values. In PTBs, additional return values
    /// are addressed via [`Argument::NestedResult`].
    pub fn nested<U>(&self, idx: u16) -> Option<PtbValue<U>> {
        self.argument.nested(idx).map(PtbValue::new)
    }
}

impl<T> From<PtbValue<T>> for Argument {
    fn from(value: PtbValue<T>) -> Self {
        value.argument
    }
}

impl<T> From<&PtbValue<T>> for Argument {
    fn from(value: &PtbValue<T>) -> Self {
        value.argument.clone()
    }
}

/// Convert a Rust value into a Move call argument (non-object).
pub trait IntoMoveArg<T: MoveType> {
    /// Convert into a [`CallArgument`].
    fn into_move_arg(self) -> Result<CallArgument, CallArgError>;
}

/// Convert a Rust value into a Move call argument (object).
pub trait IntoObjectArg<T: MoveStruct + HasKey> {
    /// Convert into a [`CallArgument`].
    fn into_object_arg(self) -> Result<CallArgument, CallArgError>;
}

/// Convert a Rust value into a Move call argument for a Move `&mut` object parameter.
pub trait IntoObjectArgMut<T: MoveStruct + HasKey> {
    /// Convert into a [`CallArgument`].
    fn into_object_arg_mut(self) -> Result<CallArgument, CallArgError>;
}

impl<T: MoveType> IntoMoveArg<T> for T {
    fn into_move_arg(self) -> Result<CallArgument, CallArgError> {
        Ok(CallArgument::Input(self.to_call_arg()?))
    }
}

impl<'a, T: MoveType> IntoMoveArg<T> for &'a T {
    fn into_move_arg(self) -> Result<CallArgument, CallArgError> {
        Ok(CallArgument::Input(self.to_call_arg()?))
    }
}

impl<'a, T: MoveStruct + HasKey, A: ObjectArg<T>> IntoObjectArg<T> for &'a A {
    fn into_object_arg(self) -> Result<CallArgument, CallArgError> {
        Ok(CallArgument::Input(self.to_call_arg()?))
    }
}

impl<'a, T: MoveStruct + HasKey, A: ObjectArg<T>> IntoObjectArgMut<T> for &'a mut A {
    fn into_object_arg_mut(self) -> Result<CallArgument, CallArgError> {
        Ok(CallArgument::Input(self.to_call_arg_mutable()?))
    }
}

impl<T> IntoMoveArg<T> for PtbValue<T>
where
    T: MoveType,
{
    fn into_move_arg(self) -> Result<CallArgument, CallArgError> {
        Ok(CallArgument::Argument(self.argument))
    }
}

impl<'a, T> IntoMoveArg<T> for &'a PtbValue<T>
where
    T: MoveType,
{
    fn into_move_arg(self) -> Result<CallArgument, CallArgError> {
        Ok(CallArgument::Argument(self.argument.clone()))
    }
}

impl<T> IntoObjectArg<T> for PtbValue<T>
where
    T: MoveStruct + HasKey,
{
    fn into_object_arg(self) -> Result<CallArgument, CallArgError> {
        Ok(CallArgument::Argument(self.argument))
    }
}

impl<'a, T> IntoObjectArg<T> for &'a PtbValue<T>
where
    T: MoveStruct + HasKey,
{
    fn into_object_arg(self) -> Result<CallArgument, CallArgError> {
        Ok(CallArgument::Argument(self.argument.clone()))
    }
}

impl<T> IntoObjectArgMut<T> for PtbValue<T>
where
    T: MoveStruct + HasKey,
{
    fn into_object_arg_mut(self) -> Result<CallArgument, CallArgError> {
        Ok(CallArgument::Argument(self.argument))
    }
}

impl<'a, T> IntoObjectArgMut<T> for &'a PtbValue<T>
where
    T: MoveStruct + HasKey,
{
    fn into_object_arg_mut(self) -> Result<CallArgument, CallArgError> {
        Ok(CallArgument::Argument(self.argument.clone()))
    }
}

/// Build typed results from a Move call command result.
pub trait CallReturn: Sized {
    /// Build the return value from the base PTB result argument.
    fn from_move_call_result(result: Argument) -> Self;
}

impl CallReturn for () {
    fn from_move_call_result(_result: Argument) -> Self {}
}

impl CallReturn for Argument {
    fn from_move_call_result(result: Argument) -> Self {
        result
    }
}

impl<T> CallReturn for PtbValue<T> {
    fn from_move_call_result(result: Argument) -> Self {
        PtbValue::new(result)
    }
}

fn nested_result(result: &Argument, idx: u16) -> Argument {
    result
        .nested(idx)
        .unwrap_or_else(|| panic!("expected Argument::Result for multi-return move call"))
}

impl<A, B> CallReturn for (PtbValue<A>, PtbValue<B>) {
    fn from_move_call_result(result: Argument) -> Self {
        (
            PtbValue::new(nested_result(&result, 0)),
            PtbValue::new(nested_result(&result, 1)),
        )
    }
}

impl<A, B, C> CallReturn for (PtbValue<A>, PtbValue<B>, PtbValue<C>) {
    fn from_move_call_result(result: Argument) -> Self {
        (
            PtbValue::new(nested_result(&result, 0)),
            PtbValue::new(nested_result(&result, 1)),
            PtbValue::new(nested_result(&result, 2)),
        )
    }
}

impl<A, B, C, D> CallReturn for (PtbValue<A>, PtbValue<B>, PtbValue<C>, PtbValue<D>) {
    fn from_move_call_result(result: Argument) -> Self {
        (
            PtbValue::new(nested_result(&result, 0)),
            PtbValue::new(nested_result(&result, 1)),
            PtbValue::new(nested_result(&result, 2)),
            PtbValue::new(nested_result(&result, 3)),
        )
    }
}

/// Errors that can occur when constructing a `CallSpec`.
#[derive(thiserror::Error, Debug)]
pub enum CallSpecError {
    /// The provided module string is not a valid Move identifier.
    #[error("invalid Move identifier for module: `{0}`")]
    Module(String),
    /// The provided function string is not a valid Move identifier.
    #[error("invalid Move identifier for function: `{0}`")]
    Function(String),
}

/// A description of a Move function call.
///
/// A `CallSpec` carries the call target plus already-encoded arguments. It is designed to be
/// consumed by a transaction-building layer.
///
/// # Example
/// ```
/// use std::str::FromStr;
/// use sui_move_call::{CallSpec, MoveObject};
/// use sui_sdk_types::{Address, Digest, ObjectReference, TypeTag};
///
/// #[sui_move::move_struct(address = "0x2", module = "object", abilities = "copy, drop, store")]
/// struct ID {
///     bytes: Address,
/// }
///
/// #[sui_move::move_struct(address = "0x2", module = "object", abilities = "store")]
/// struct UID {
///     id: ID,
/// }
///
/// #[sui_move::move_struct(address = "0x1", module = "vault", abilities = "key")]
/// struct Vault {
///     id: UID,
/// }
///
/// let package = Address::from_str("0x1").unwrap();
/// let obj_ref = ObjectReference::new(package, 1, Digest::default());
/// let vault = MoveObject::<Vault>::new(obj_ref);
///
/// let mut spec = CallSpec::new(package, "vault", "withdraw").unwrap();
/// spec.push_type_arg::<u64>();
/// spec.push_arg(&vault).unwrap();
/// spec.push_arg(&1u64).unwrap();
///
/// assert_eq!(spec.type_arguments, vec![TypeTag::U64]);
/// assert_eq!(spec.arguments.len(), 2);
/// ```
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CallSpec<R = ()> {
    /// Package ID that contains the Move module.
    pub package: Address,
    /// Move module identifier.
    pub module: Identifier,
    /// Move function identifier.
    pub function: Identifier,
    /// Type arguments for the call (Move `TypeTag`s).
    pub type_arguments: Vec<TypeTag>,
    /// Call arguments (inputs and/or already-resolved PTB arguments).
    pub arguments: Vec<CallArgument>,
    #[serde(skip)]
    phantom: PhantomData<R>,
}

impl CallSpec<()> {
    /// Create an empty call spec for a `(package, module, function)` triple.
    pub fn new(
        package: Address,
        module: impl AsRef<str>,
        function: impl AsRef<str>,
    ) -> Result<Self, CallSpecError> {
        let module_str = module.as_ref();
        let function_str = function.as_ref();

        let module = Identifier::from_str(module_str)
            .map_err(|_| CallSpecError::Module(module_str.to_string()))?;
        let function = Identifier::from_str(function_str)
            .map_err(|_| CallSpecError::Function(function_str.to_string()))?;

        Ok(Self {
            package,
            module,
            function,
            type_arguments: Vec::new(),
            arguments: Vec::new(),
            phantom: PhantomData,
        })
    }
}

impl<R> CallSpec<R> {
    /// Create an empty typed call spec for a `(package, module, function)` triple.
    pub fn new_typed(
        package: Address,
        module: impl AsRef<str>,
        function: impl AsRef<str>,
    ) -> Result<Self, CallSpecError> {
        let module_str = module.as_ref();
        let function_str = function.as_ref();

        let module = Identifier::from_str(module_str)
            .map_err(|_| CallSpecError::Module(module_str.to_string()))?;
        let function = Identifier::from_str(function_str)
            .map_err(|_| CallSpecError::Function(function_str.to_string()))?;

        Ok(Self {
            package,
            module,
            function,
            type_arguments: Vec::new(),
            arguments: Vec::new(),
            phantom: PhantomData,
        })
    }

    /// Append a type argument derived from a `MoveType`.
    pub fn push_type_arg<T: MoveType>(&mut self) {
        self.type_arguments.push(T::type_tag_static());
    }

    /// Append an argument by converting it into a `CallArg`.
    ///
    /// This can fail for BCS encoding errors (pure values) and, for higher-layer object handles,
    /// for handle state errors (tombstoned/stale handles, kind mismatches, etc.).
    pub fn push_arg<A: ToCallArg>(&mut self, arg: &A) -> Result<(), CallArgError> {
        self.arguments.push(CallArgument::Input(arg.to_call_arg()?));
        Ok(())
    }

    /// Append an argument as if it were used for a Move `&mut` parameter.
    ///
    /// This differs from [`CallSpec::push_arg`] only for shared object handles: shared inputs must
    /// be marked mutable in the transaction input.
    pub fn push_arg_mut<A: ToCallArgMut>(&mut self, arg: &A) -> Result<(), CallArgError> {
        self.arguments
            .push(CallArgument::Input(arg.to_call_arg_mutable()?));
        Ok(())
    }

    /// Append a raw Sui transaction input.
    ///
    /// This is an escape hatch for `sui_sdk_types::Input` variants that don't have typed wrappers
    /// in this crate.
    pub fn push_input(&mut self, input: CallArg) {
        self.arguments.push(CallArgument::Input(input));
    }

    /// Append a raw programmable-transaction argument.
    pub fn push_argument(&mut self, arg: Argument) {
        self.arguments.push(CallArgument::Argument(arg));
    }

    /// Append a non-object argument (pure value or prior PTB result).
    pub fn push_value_arg<T: MoveType>(
        &mut self,
        arg: impl IntoMoveArg<T>,
    ) -> Result<(), CallArgError> {
        self.arguments.push(arg.into_move_arg()?);
        Ok(())
    }

    /// Append an object argument (object input handle or prior PTB result).
    pub fn push_object_arg<T: MoveStruct + HasKey>(
        &mut self,
        arg: impl IntoObjectArg<T>,
    ) -> Result<(), CallArgError> {
        self.arguments.push(arg.into_object_arg()?);
        Ok(())
    }

    /// Append an object argument for a Move `&mut` parameter.
    pub fn push_object_arg_mut<T: MoveStruct + HasKey>(
        &mut self,
        arg: impl IntoObjectArgMut<T>,
    ) -> Result<(), CallArgError> {
        self.arguments.push(arg.into_object_arg_mut()?);
        Ok(())
    }
}

/// Convenience re-exports for downstream code.
pub mod prelude {
    pub use crate::{
        CallArg, CallArgError, CallArgument, CallReturn, CallSpec, CallSpecError, IntoMoveArg,
        IntoObjectArg, IntoObjectArgMut, MoveObject, ObjectArg, PtbValue, ReceivingMoveObject,
        SharedMoveObject, ToCallArg, ToCallArgMut,
    };
    pub use sui_move::prelude::*;
    pub use sui_sdk_types::Mutability;
}
