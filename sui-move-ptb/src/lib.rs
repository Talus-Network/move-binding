#![doc = include_str!("../README.md")]
#![deny(missing_docs)]

use sui_move_call::{CallArg, CallArgError, CallArgument, CallReturn, CallSpec, ToCallArg};
use sui_sdk_types::{
    Address, Argument, Command, MakeMoveVector, MergeCoins, MoveCall, Mutability,
    ProgrammableTransaction, Publish, SharedInput, SplitCoins, TransferObjects, TypeTag,
};

/// Errors that can occur while building a `ProgrammableTransaction`.
#[derive(thiserror::Error, Debug)]
pub enum BuildError {
    /// A value could not be converted into a Sui input (typically BCS encoding failed).
    #[error(transparent)]
    CallArg(#[from] CallArgError),

    /// Too many inputs were added (PTB uses `u16` indices).
    #[error("too many PTB inputs (u16 overflow)")]
    TooManyInputs,

    /// Too many commands were added (PTB uses `u16` indices for results).
    #[error("too many PTB commands (u16 overflow)")]
    TooManyCommands,

    /// The same object id appears more than once across object inputs (including receiving).
    ///
    /// This matches Sui's `DuplicateObjectRefInput` class of errors.
    #[error("duplicate object id {object_id} in PTB inputs")]
    DuplicateObjectRefInput {
        /// Duplicated object id.
        object_id: Address,
    },
}

/// Mutable builder for `ProgrammableTransaction`.
///
/// `ProgrammableTransaction` commands refer to inputs by index (`Argument::Input(u16)`), so
/// building a PTB is fundamentally about maintaining an input table and wiring command arguments
/// to those indices.
///
/// `PtbBuilder` is the small “allocation” layer that:
/// - collects inputs (Sui `Input`s, via `sui_move_call::CallArg`) into a PTB input table,
/// - emits PTB commands,
/// - returns the final `ProgrammableTransaction`.
///
/// It is intentionally minimal: it does not fetch objects, it does not submit transactions, and it
/// does not try to add extra type systems on top of Sui's canonical wire types.
#[derive(Default, Debug, Clone)]
pub struct PtbBuilder {
    inputs: Vec<CallArg>,
    commands: Vec<Command>,
}

impl PtbBuilder {
    /// Create an empty PTB builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Borrow the accumulated PTB inputs.
    ///
    /// These are the canonical Sui `Input`s (re-exported as `sui_move_call::CallArg`).
    pub fn inputs(&self) -> &[CallArg] {
        &self.inputs
    }

    /// Borrow the accumulated PTB commands.
    pub fn commands(&self) -> &[Command] {
        &self.commands
    }

    /// The gas argument (`Argument::Gas`).
    ///
    /// This is useful when building native commands that can refer to the gas coin.
    pub fn gas(&self) -> Argument {
        Argument::Gas
    }

    fn push_input(&mut self, input: CallArg) -> Result<u16, BuildError> {
        if matches!(input, CallArg::FundsWithdrawal(_)) {
            let idx = u16::try_from(self.inputs.len()).map_err(|_| BuildError::TooManyInputs)?;
            self.inputs.push(input);
            return Ok(idx);
        }

        if let Some(idx) = self.inputs.iter().position(|existing| existing == &input) {
            return u16::try_from(idx).map_err(|_| BuildError::TooManyInputs);
        }

        match &input {
            CallArg::Shared(shared) => {
                let object_id = shared.object_id();
                let version = shared.version();
                let mutability = shared.mutability();

                for (idx, existing) in self.inputs.iter_mut().enumerate() {
                    match existing {
                        CallArg::Shared(existing_shared)
                            if existing_shared.object_id() == object_id =>
                        {
                            if existing_shared.version() != version {
                                return Err(BuildError::DuplicateObjectRefInput { object_id });
                            }

                            let upgraded =
                                upgrade_shared_mutability(existing_shared.mutability(), mutability);
                            if upgraded != existing_shared.mutability() {
                                *existing =
                                    CallArg::Shared(SharedInput::new(object_id, version, upgraded));
                            }

                            return u16::try_from(idx).map_err(|_| BuildError::TooManyInputs);
                        }
                        other if call_arg_object_id(other) == Some(object_id) => {
                            return Err(BuildError::DuplicateObjectRefInput { object_id });
                        }
                        _ => {}
                    }
                }
            }
            CallArg::ImmutableOrOwned(reference) | CallArg::Receiving(reference) => {
                let object_id = *reference.object_id();
                if self
                    .inputs
                    .iter()
                    .any(|existing| call_arg_object_id(existing) == Some(object_id))
                {
                    return Err(BuildError::DuplicateObjectRefInput { object_id });
                }
            }
            _ => {}
        }

        let idx = u16::try_from(self.inputs.len()).map_err(|_| BuildError::TooManyInputs)?;
        self.inputs.push(input);
        Ok(idx)
    }

    fn push_command(&mut self, command: Command) -> Result<u16, BuildError> {
        let idx = u16::try_from(self.commands.len()).map_err(|_| BuildError::TooManyCommands)?;
        self.commands.push(command);
        Ok(idx)
    }

    /// Add an input to this PTB and return an `Argument::Input` pointing at it.
    ///
    /// `PtbBuilder` reuses identical inputs to keep PTBs compact and make it natural to reuse the
    /// same typed handle across multiple calls.
    ///
    /// Exception: `Input::FundsWithdrawal` is never deduplicated (duplicates can be meaningful).
    ///
    /// Additional rules for object inputs:
    ///
    /// - Shared inputs are unified by `(object_id, initial_shared_version)` and upgraded to the
    ///   most permissive mutability mode when the same shared object is added multiple times.
    /// - Duplicate object ids across input objects and receiving objects are rejected early with
    ///   [`BuildError::DuplicateObjectRefInput`].
    ///
    /// # Example
    /// ```
    /// use sui_move_call::CallArg;
    /// use sui_move_ptb::PtbBuilder;
    /// use sui_sdk_types::Argument;
    ///
    /// let mut tx = PtbBuilder::new();
    /// let a0 = tx.input(CallArg::Pure(vec![1, 2, 3])).unwrap();
    /// let a1 = tx.input(CallArg::Pure(vec![1, 2, 3])).unwrap();
    ///
    /// assert_eq!(a0, Argument::Input(0));
    /// assert_eq!(a1, Argument::Input(0));
    /// assert_eq!(tx.inputs().len(), 1);
    /// ```
    pub fn input(&mut self, input: CallArg) -> Result<Argument, BuildError> {
        Ok(Argument::Input(self.push_input(input)?))
    }

    /// Convert a typed value into an input and return the corresponding `Argument::Input`.
    ///
    /// This uses [`ToCallArg`] from `sui-move-call`, so:
    /// - `T: MoveType` becomes `Input::Pure(bcs(T))`
    /// - typed object handles become the corresponding object input kinds
    pub fn arg<A: ToCallArg>(&mut self, value: &A) -> Result<Argument, BuildError> {
        self.input(value.to_call_arg()?)
    }

    /// Add a Move call command from a typed `CallSpec`.
    ///
    /// This consumes the spec, allocates all of its inputs into the PTB input table (reusing
    /// identical ones when possible), emits `Command::MoveCall`, and returns `Argument::Result`
    /// pointing at the command result.
    ///
    /// If the called function returns multiple values, you can access sub-results using
    /// `Argument::nested`.
    pub fn call<R: CallReturn>(&mut self, spec: CallSpec<R>) -> Result<R, BuildError> {
        let arguments = spec
            .arguments
            .into_iter()
            .map(|arg| match arg {
                CallArgument::Input(input) => self.input(input),
                CallArgument::Argument(arg) => Ok(arg),
            })
            .collect::<Result<Vec<_>, _>>()?;

        let cmd_idx = self.push_command(Command::MoveCall(MoveCall {
            package: spec.package,
            module: spec.module,
            function: spec.function,
            type_arguments: spec.type_arguments,
            arguments,
        }))?;

        Ok(R::from_move_call_result(Argument::Result(cmd_idx)))
    }

    /// Add a `TransferObjects` command.
    ///
    /// This is a thin wrapper around `sui_sdk_types::Command::TransferObjects`.
    pub fn transfer_objects(
        &mut self,
        objects: Vec<Argument>,
        address: Argument,
    ) -> Result<(), BuildError> {
        self.push_command(Command::TransferObjects(TransferObjects {
            objects,
            address,
        }))?;
        Ok(())
    }

    /// Add a `SplitCoins` command.
    ///
    /// This is a thin wrapper around `sui_sdk_types::Command::SplitCoins`.
    pub fn split_coins(
        &mut self,
        coin: Argument,
        amounts: Vec<Argument>,
    ) -> Result<Argument, BuildError> {
        let cmd_idx = self.push_command(Command::SplitCoins(SplitCoins { coin, amounts }))?;
        Ok(Argument::Result(cmd_idx))
    }

    /// Add a `MergeCoins` command.
    ///
    /// This is a thin wrapper around `sui_sdk_types::Command::MergeCoins`.
    pub fn merge_coins(
        &mut self,
        destination: Argument,
        sources: Vec<Argument>,
    ) -> Result<(), BuildError> {
        self.push_command(Command::MergeCoins(MergeCoins {
            coin: destination,
            coins_to_merge: sources,
        }))?;
        Ok(())
    }

    /// Add a `MakeMoveVector` command.
    ///
    /// This is a thin wrapper around `sui_sdk_types::Command::MakeMoveVector`.
    pub fn make_move_vector(
        &mut self,
        type_: Option<TypeTag>,
        elements: Vec<Argument>,
    ) -> Result<Argument, BuildError> {
        let cmd_idx =
            self.push_command(Command::MakeMoveVector(MakeMoveVector { type_, elements }))?;
        Ok(Argument::Result(cmd_idx))
    }

    /// Add a `Publish` command.
    ///
    /// This is a thin wrapper around `sui_sdk_types::Command::Publish`.
    pub fn publish(
        &mut self,
        modules: Vec<Vec<u8>>,
        dependencies: Vec<Address>,
    ) -> Result<(), BuildError> {
        self.push_command(Command::Publish(Publish {
            modules,
            dependencies,
        }))?;
        Ok(())
    }

    /// Finish and return the underlying `ProgrammableTransaction`.
    ///
    /// This does not attempt to validate command semantics; it just returns the constructed PTB.
    pub fn finish(self) -> ProgrammableTransaction {
        ProgrammableTransaction {
            inputs: self.inputs,
            commands: self.commands,
        }
    }
}

/// Convenience helper: run `build` with a fresh `PtbBuilder` and return the finished PTB.
///
/// # Example
/// ```
/// use sui_move_call::CallSpec;
/// use sui_move_ptb::ptb;
/// use sui_sdk_types::Address;
///
/// let package = Address::from_hex("0x1").unwrap();
/// let spec = CallSpec::new(package, "m", "f").unwrap();
///
/// let pt = ptb(|tx| {
///     tx.call(spec)?;
///     Ok(())
/// })
/// .unwrap();
///
/// assert_eq!(pt.commands.len(), 1);
/// ```
pub fn ptb(
    build: impl FnOnce(&mut PtbBuilder) -> Result<(), BuildError>,
) -> Result<ProgrammableTransaction, BuildError> {
    let mut builder = PtbBuilder::new();
    build(&mut builder)?;
    Ok(builder.finish())
}

/// Build a `ProgrammableTransaction` using a scoped `PtbBuilder`.
///
/// This is a thin macro wrapper around [`ptb`]. It exists purely for call-site ergonomics.
///
/// # Examples
/// ```rust
/// use sui_move_call::CallSpec;
/// use sui_sdk_types::Address;
///
/// let package = Address::from_hex("0x1").unwrap();
/// let spec = CallSpec::new(package, "m", "f").unwrap();
///
/// let pt = sui_move_ptb::ptb!(tx => {
///     tx.call(spec)?;
///     Ok(())
/// })
/// .unwrap();
///
/// assert_eq!(pt.commands.len(), 1);
/// ```
///
/// ```rust
/// use sui_move_call::CallSpec;
/// use sui_sdk_types::Address;
///
/// let package = Address::from_hex("0x1").unwrap();
/// let spec = CallSpec::new(package, "m", "f").unwrap();
///
/// let pt = sui_move_ptb::ptb! { spec; }.unwrap();
/// assert_eq!(pt.commands.len(), 1);
/// ```
#[macro_export]
macro_rules! ptb {
    ($tx:ident => { $($body:tt)* }) => {{
        $crate::ptb(|$tx| { $($body)* })
    }};
    ($($spec:expr);+ $(;)?) => {{
        $crate::ptb(|tx| {
            $(
                tx.call($spec)?;
            )+
            Ok(())
        })
    }};
}

/// Convenience re-exports for downstream code.
pub mod prelude {
    pub use crate::{ptb, BuildError, PtbBuilder};
    pub use sui_move_call::prelude::*;
    pub use sui_sdk_types::{Argument, Command, ProgrammableTransaction};
}

fn call_arg_object_id(input: &CallArg) -> Option<Address> {
    match input {
        CallArg::ImmutableOrOwned(reference) | CallArg::Receiving(reference) => {
            Some(*reference.object_id())
        }
        CallArg::Shared(shared) => Some(shared.object_id()),
        _ => None,
    }
}

fn upgrade_shared_mutability(existing: Mutability, requested: Mutability) -> Mutability {
    use Mutability::{Immutable, Mutable, NonExclusiveWrite};

    match (existing, requested) {
        (Mutable, _) | (_, Mutable) => Mutable,
        (NonExclusiveWrite, _) | (_, NonExclusiveWrite) => NonExclusiveWrite,
        _ => Immutable,
    }
}
