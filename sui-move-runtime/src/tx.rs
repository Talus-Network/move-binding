use std::time::Duration;

use sui_crypto::SuiSigner;
use sui_rpc::client::ExecuteAndWaitError;
use sui_rpc::field::FieldMaskTree;
use sui_rpc::proto::sui::rpc::v2::{
    simulate_transaction_request::TransactionChecks, transaction_kind, ExecuteTransactionRequest,
    GetObjectRequest, ProgrammableTransaction as ProtoProgrammableTransaction,
    SimulateTransactionRequest, SimulateTransactionResponse, Transaction as ProtoTransaction,
    TransactionKind as ProtoTransactionKind,
};
use sui_rpc::proto::TryFromProtoError;
use sui_sdk_types::{
    Address, Digest, ExecutionStatus, GasPayment, ObjectReference, Owner, StructTag,
    Transaction as SdkTransaction, TransactionEffects, TransactionExpiration, TransactionKind,
    TypeTag, UserSignature,
};

#[derive(Clone, Debug)]
pub(crate) struct FetchedMoveObject {
    pub(crate) reference: ObjectReference,
    pub(crate) owner: Owner,
    pub(crate) struct_tag: StructTag,
    pub(crate) contents: Vec<u8>,
}

/// Additional transaction options for submitting a PTB.
#[derive(Clone, Debug, Default)]
pub struct TxOptions {
    /// Optional sponsor address (gas owner). Defaults to sender.
    pub sponsor: Option<Address>,
    /// Optional explicit gas object reference. Defaults to selecting one coin owned by the gas owner.
    pub gas: Option<ObjectReference>,
    /// Optional explicit gas budget. Defaults to `Runtime::default_gas_budget`.
    pub gas_budget: Option<u64>,
    /// Optional explicit gas price. Defaults to the reference gas price from RPC.
    pub gas_price: Option<u64>,
    /// Optional explicit expiration (TTL). Defaults to `None`.
    pub expiration: Option<TransactionExpiration>,
}

/// Transaction finality requested by the caller.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Finality {
    /// Only require that the transaction was executed (effects produced).
    Executed,
    /// Also wait for the transaction to be observed in a checkpoint on the connected RPC node.
    Checkpointed,
}

/// Finality actually observed by the runtime.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObservedFinality {
    /// Transaction was executed (digest known), but checkpoint inclusion was not confirmed.
    Executed,
    /// Transaction was observed in a checkpoint.
    Checkpointed,
}

/// Outcome of waiting for checkpoint inclusion.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CheckpointWaitOutcome {
    /// The runtime did not request checkpoint waiting.
    NotRequested,
    /// Checkpoint inclusion was confirmed.
    Ok,
    /// Checkpoint wait timed out.
    Timeout,
    /// Checkpoint stream returned an error.
    StreamError {
        /// Checkpoint stream error message.
        error: String,
    },
}

/// Receipt for a submitted transaction.
#[derive(Clone, Debug)]
pub struct Receipt {
    /// Transaction digest.
    pub digest: Digest,
    /// Transaction effects, if returned by RPC.
    pub effects: Option<TransactionEffects>,
    /// Execution status, if known.
    pub status: Option<ExecutionStatus>,
    /// The user signature used for submission.
    pub signature: UserSignature,
    /// Finality requested by the caller.
    pub requested_finality: Finality,
    /// Finality observed by the runtime.
    pub observed_finality: ObservedFinality,
    /// Outcome of waiting for checkpoint inclusion.
    pub checkpoint_wait: CheckpointWaitOutcome,
}

/// Error returned by [`Receipt::ensure_success`].
#[derive(thiserror::Error, Clone, Debug, PartialEq, Eq)]
pub enum EnsureSuccessError {
    /// The receipt does not contain an execution status.
    #[error("transaction execution status is unknown")]
    UnknownStatus,
    /// The transaction executed with failure.
    #[error("transaction execution failed: {0:?}")]
    Failure(Box<ExecutionStatus>),
}

impl Receipt {
    /// Return `Ok(())` if the transaction executed successfully.
    ///
    /// Failed transactions are still committed on-chain and may update gas references.
    pub fn ensure_success(&self) -> Result<(), EnsureSuccessError> {
        match &self.status {
            Some(ExecutionStatus::Success) => Ok(()),
            Some(status) => Err(EnsureSuccessError::Failure(Box::new(status.clone()))),
            None => Err(EnsureSuccessError::UnknownStatus),
        }
    }
}

/// Options for running a transaction in dry-run mode (checks enabled).
#[derive(Clone, Debug)]
pub struct SimulateOptions {
    /// Whether the server should perform gas selection automatically.
    ///
    /// When `true` (default), the request can omit gas payment information and the server will
    /// pick a gas coin and budget for simulation.
    ///
    /// Note: this option is ignored when checks are disabled (dev-inspect mode).
    ///
    /// If you set this to `false`, the simulation RPC may require an explicit gas payment on the
    /// provided transaction. The current `sui-move-runtime` simulation helper does not model
    /// explicit gas payment configuration, so `false` is generally only useful if your RPC accepts
    /// omitted gas payment without auto-selection.
    pub do_gas_selection: bool,
}

impl Default for SimulateOptions {
    fn default() -> Self {
        Self {
            do_gas_selection: true,
        }
    }
}

/// Options for running a transaction in dev-inspect mode (checks disabled).
///
/// This is currently an empty placeholder for future knobs (e.g. requesting JSON renderings of
/// outputs).
#[derive(Clone, Debug, Default)]
pub struct InspectOptions {}

/// Receipt for a simulated transaction (dry-run).
///
/// Returned by [`crate::Tx::simulate`]. This does not mutate chain state and does not update
/// runtime-owned handles.
#[derive(Clone, Debug)]
pub struct SimulationReceipt {
    /// Transaction digest, if returned by RPC.
    pub digest: Option<Digest>,
    /// Transaction effects, if returned by RPC.
    pub effects: Option<TransactionEffects>,
}

/// Receipt for a dev-inspected transaction (checks disabled) including command outputs.
///
/// Returned by [`crate::Tx::inspect`]. This does not mutate chain state and does not update
/// runtime-owned handles.
///
/// `outputs[i]` corresponds to the `i`-th PTB command.
#[derive(Clone, Debug)]
pub struct InspectReceipt {
    /// Transaction digest, if returned by RPC.
    pub digest: Option<Digest>,
    /// Transaction effects, if returned by RPC.
    pub effects: Option<TransactionEffects>,
    /// Command outputs (return values + mutated-by-ref values), if requested.
    pub outputs: Vec<CommandOutputs>,
}

/// Per-command outputs returned by dev-inspect.
///
/// This struct is intentionally minimal: it only contains raw BCS blobs.
#[derive(Clone, Debug, Default)]
pub struct CommandOutputs {
    /// Values returned from the command.
    pub return_values: Vec<BcsValue>,
    /// Values mutated by reference during the command.
    pub mutated_by_ref: Vec<BcsValue>,
}

/// A raw BCS value returned by simulation/dev-inspect.
///
/// You can decode this using `sui_sdk_types::bcs` (enabled via the `serde` feature on
/// `sui-sdk-types`):
///
/// ```
/// use sui_move_runtime::BcsValue;
/// use sui_sdk_types::bcs::{FromBcs, ToBcs};
///
/// let value = BcsValue {
///     name: Some("u64".to_owned()),
///     bytes: 10u64.to_bcs().unwrap(),
/// };
///
/// let decoded = u64::from_bcs(&value.bytes).unwrap();
/// assert_eq!(decoded, 10);
/// ```
#[derive(Clone, Debug, Default)]
pub struct BcsValue {
    /// Optional type name for this BCS blob, if provided by RPC.
    pub name: Option<String>,
    /// Raw BCS bytes.
    pub bytes: Vec<u8>,
}

/// Errors for submit/wait operations.
#[derive(thiserror::Error, Debug)]
pub enum TxError {
    /// Signing failed.
    #[error("sign transaction: {0}")]
    Sign(String),

    /// RPC execution failed.
    #[error("execute transaction: {0}")]
    Execute(String),

    /// RPC returned a response missing the executed transaction.
    #[error("missing executed transaction in response")]
    MissingExecuted,

    /// Proto conversion failed.
    #[error("proto conversion: {0}")]
    Proto(String),

    /// Gas selection failed (no gas coin or RPC error).
    #[error("select gas: {0}")]
    Gas(String),
}

/// Errors for simulate/dev-inspect operations.
#[derive(thiserror::Error, Debug)]
pub enum SimulateError {
    /// RPC error.
    #[error("simulate transaction: {0}")]
    Rpc(String),

    /// RPC returned a response missing the executed transaction.
    #[error("missing executed transaction in response")]
    MissingExecuted,

    /// Proto conversion failed.
    #[error("proto conversion: {0}")]
    Proto(String),
}

/// Errors for read/fetch helpers.
#[derive(thiserror::Error, Debug)]
pub enum RpcError {
    /// RPC error.
    #[error("rpc: {0}")]
    Rpc(String),
    /// Object not found.
    #[error("object {0} not found")]
    Missing(Address),
    /// Failed to convert protobuf object.
    #[error("proto conversion: {0}")]
    Proto(String),
    /// Object is a package, not a Move struct object.
    #[error("object {0} is a package, expected a Move struct object")]
    NotMoveStruct(Address),
}

pub(crate) async fn fetch_object_reference_and_owner(
    client: &mut sui_rpc::Client,
    id: Address,
) -> Result<FetchedMoveObject, RpcError> {
    let mut req = GetObjectRequest::new(&id);
    let mut mask = FieldMaskTree::default();
    for path in [
        "object_id",
        "version",
        "digest",
        "owner",
        "object_type",
        "has_public_transfer",
        "contents",
        "previous_transaction",
        "storage_rebate",
    ] {
        mask.add_field_path(path);
    }
    req.read_mask = Some(mask.to_field_mask());

    let resp = client
        .ledger_client()
        .get_object(req)
        .await
        .map_err(|e| RpcError::Rpc(e.to_string()))?
        .into_inner();

    let obj_proto = resp.object.ok_or(RpcError::Missing(id))?;

    let proto_ref = obj_proto.object_reference();
    let reference: ObjectReference = (&proto_ref)
        .try_into()
        .map_err(|e: TryFromProtoError| RpcError::Proto(e.to_string()))?;

    let sui_obj: sui_sdk_types::Object = (&obj_proto)
        .try_into()
        .map_err(|e: TryFromProtoError| RpcError::Proto(e.to_string()))?;

    let move_struct = sui_obj
        .as_struct()
        .ok_or(RpcError::NotMoveStruct(*reference.object_id()))?;

    Ok(FetchedMoveObject {
        reference,
        owner: *sui_obj.owner(),
        struct_tag: move_struct.object_type().clone(),
        contents: move_struct.contents().to_vec(),
    })
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum OwnerKind {
    Immutable,
    AddressOwned,
    ChildObject,
    Shared { initial_shared_version: u64 },
    ConsensusAddress { start_version: u64 },
    Unknown,
}

impl OwnerKind {
    pub(crate) fn label(self) -> &'static str {
        match self {
            OwnerKind::Immutable => "immutable",
            OwnerKind::AddressOwned => "address-owned",
            OwnerKind::ChildObject => "child-object",
            OwnerKind::Shared { .. } => "shared",
            OwnerKind::ConsensusAddress { .. } => "consensus-address-owned",
            OwnerKind::Unknown => "unknown",
        }
    }

    pub(crate) fn is_shared_like(self) -> bool {
        matches!(
            self,
            OwnerKind::Shared { .. } | OwnerKind::ConsensusAddress { .. }
        )
    }

    pub(crate) fn shared_start_version(self) -> Option<u64> {
        match self {
            OwnerKind::Shared {
                initial_shared_version,
            } => Some(initial_shared_version),
            OwnerKind::ConsensusAddress { start_version, .. } => Some(start_version),
            _ => None,
        }
    }
}

pub(crate) fn classify_owner(owner: &Owner) -> OwnerKind {
    match owner {
        Owner::Immutable => OwnerKind::Immutable,
        Owner::Address(_) => OwnerKind::AddressOwned,
        Owner::Object(_) => OwnerKind::ChildObject,
        Owner::Shared(initial_shared_version) => OwnerKind::Shared {
            initial_shared_version: *initial_shared_version,
        },
        Owner::ConsensusAddress { start_version, .. } => OwnerKind::ConsensusAddress {
            start_version: *start_version,
        },
        _ => OwnerKind::Unknown,
    }
}

pub(crate) async fn submit_and_wait<S: SuiSigner>(
    client: &mut sui_rpc::Client,
    signer: &S,
    sender: Address,
    ptb: sui_sdk_types::ProgrammableTransaction,
    opts: TxOptions,
    default_gas_budget: u64,
    timeout: Duration,
) -> Result<Receipt, TxError> {
    let sponsor = opts.sponsor.unwrap_or(sender);
    let gas_price = match opts.gas_price {
        Some(p) => p,
        None => client
            .get_reference_gas_price()
            .await
            .map_err(|e| TxError::Execute(e.to_string()))?,
    };

    let gas = match opts.gas {
        Some(g) => g,
        None => select_gas_coin(client, &sponsor).await?,
    };

    let gas_budget = opts.gas_budget.unwrap_or(default_gas_budget);

    let tx = SdkTransaction {
        kind: TransactionKind::ProgrammableTransaction(ptb),
        sender,
        gas_payment: GasPayment {
            objects: vec![gas],
            owner: sponsor,
            price: gas_price,
            budget: gas_budget,
        },
        expiration: opts.expiration.unwrap_or(TransactionExpiration::None),
    };

    let signature = signer
        .sign_transaction(&tx)
        .map_err(|e| TxError::Sign(e.to_string()))?;

    let mut req = ExecuteTransactionRequest::default();
    req.transaction = Some(tx.clone().into());
    req.signatures.push(signature.clone().into());
    // We need `effects.bcs` in order to decode `sui_sdk_types::TransactionEffects`.
    //
    // If no mask is provided, the server defaults to `effects.status,checkpoint` which does not
    // include the BCS bytes.
    let mut mask = FieldMaskTree::default();
    for path in ["digest", "effects.bcs", "effects.status", "checkpoint"] {
        mask.add_field_path(path);
    }
    req.read_mask = Some(mask.to_field_mask());

    let requested_finality = Finality::Checkpointed;

    let (response, checkpoint_wait, observed_finality) = match client
        .execute_transaction_and_wait_for_checkpoint(req, timeout)
        .await
    {
        Ok(response) => (
            response,
            CheckpointWaitOutcome::Ok,
            ObservedFinality::Checkpointed,
        ),
        Err(ExecuteAndWaitError::CheckpointTimeout(response)) => (
            response,
            CheckpointWaitOutcome::Timeout,
            ObservedFinality::Executed,
        ),
        Err(ExecuteAndWaitError::CheckpointStreamError { response, error }) => (
            response,
            CheckpointWaitOutcome::StreamError {
                error: error.to_string(),
            },
            ObservedFinality::Executed,
        ),
        Err(err) => {
            return Err(map_execute_wait_error(
                err,
                "execute_transaction_and_wait_for_checkpoint",
            ))
        }
    };

    let executed = response
        .into_inner()
        .transaction
        .ok_or(TxError::MissingExecuted)?;

    receipt_from_executed(
        executed,
        tx.digest(),
        signature,
        requested_finality,
        observed_finality,
        checkpoint_wait,
    )
}

async fn select_gas_coin(
    client: &mut sui_rpc::Client,
    owner: &Address,
) -> Result<ObjectReference, TxError> {
    let gas_tag = TypeTag::Struct(Box::new(StructTag::gas_coin()));
    let mut coins = client
        .select_up_to_n_largest_coins(owner, &gas_tag, 1, &[])
        .await
        .map_err(|e| TxError::Gas(e.to_string()))?;

    let coin = coins
        .pop()
        .ok_or_else(|| TxError::Gas(format!("no gas coin available for {}", owner)))?;

    let proto_ref = coin.object_reference();
    (&proto_ref)
        .try_into()
        .map_err(|e: TryFromProtoError| TxError::Proto(e.to_string()))
}

fn map_execute_wait_error(err: ExecuteAndWaitError, context_label: &'static str) -> TxError {
    match err {
        ExecuteAndWaitError::RpcError(e) => {
            TxError::Execute(format!("{context_label} rpc error: {e}"))
        }
        ExecuteAndWaitError::ProtoConversionError(e) => {
            TxError::Proto(format!("{context_label} proto conversion error: {e}"))
        }
        ExecuteAndWaitError::MissingTransaction => {
            TxError::Execute(format!("{context_label} missing transaction in request"))
        }
        _ => TxError::Execute(format!("{context_label} unexpected error")),
    }
}

fn receipt_from_executed(
    executed: sui_rpc::proto::sui::rpc::v2::ExecutedTransaction,
    fallback_digest: Digest,
    signature: UserSignature,
    requested_finality: Finality,
    observed_finality: ObservedFinality,
    checkpoint_wait: CheckpointWaitOutcome,
) -> Result<Receipt, TxError> {
    let digest = executed
        .digest
        .as_deref()
        .and_then(|d| d.parse::<Digest>().ok())
        .unwrap_or(fallback_digest);

    let (effects, status) = decode_effects_and_status(&executed)?;

    Ok(Receipt {
        digest,
        effects,
        status,
        signature,
        requested_finality,
        observed_finality,
        checkpoint_wait,
    })
}

fn decode_effects_and_status(
    executed: &sui_rpc::proto::sui::rpc::v2::ExecutedTransaction,
) -> Result<(Option<TransactionEffects>, Option<ExecutionStatus>), TxError> {
    let proto_effects = executed.effects.as_ref();

    let status_from_proto = proto_effects
        .and_then(|fx| fx.status.as_ref())
        .map(ExecutionStatus::try_from)
        .transpose()
        .map_err(|e: TryFromProtoError| TxError::Proto(e.to_string()))?;

    let effects = match proto_effects {
        Some(proto) if proto.bcs.is_some() => Some(
            TransactionEffects::try_from(proto)
                .map_err(|e: TryFromProtoError| TxError::Proto(e.to_string()))?,
        ),
        _ => None,
    };

    let status = effects
        .as_ref()
        .map(|fx| fx.status().clone())
        .or(status_from_proto);

    Ok((effects, status))
}

pub(crate) async fn simulate_ptb(
    client: &mut sui_rpc::Client,
    sender: Address,
    ptb: sui_sdk_types::ProgrammableTransaction,
    opts: SimulateOptions,
) -> Result<SimulationReceipt, SimulateError> {
    let tx = proto_transaction_for_ptb(sender, ptb);

    let mut req = SimulateTransactionRequest::new(tx);
    req.checks = Some(TransactionChecks::Enabled as i32);
    req.do_gas_selection = Some(opts.do_gas_selection);

    let mut mask = FieldMaskTree::default();
    for path in ["transaction.digest", "transaction.effects.bcs"] {
        mask.add_field_path(path);
    }
    req.read_mask = Some(mask.to_field_mask());

    let resp = client
        .execution_client()
        .simulate_transaction(req)
        .await
        .map_err(|e| SimulateError::Rpc(e.to_string()))?
        .into_inner();

    simulation_receipt_from_response(resp)
}

pub(crate) async fn inspect_ptb(
    client: &mut sui_rpc::Client,
    sender: Address,
    ptb: sui_sdk_types::ProgrammableTransaction,
    _opts: InspectOptions,
) -> Result<InspectReceipt, SimulateError> {
    let tx = proto_transaction_for_ptb(sender, ptb);

    let mut req = SimulateTransactionRequest::new(tx);
    req.checks = Some(TransactionChecks::Disabled as i32);

    let mut mask = FieldMaskTree::default();
    for path in [
        "transaction.digest",
        "transaction.effects.bcs",
        "command_outputs.return_values.value",
        "command_outputs.mutated_by_ref.value",
    ] {
        mask.add_field_path(path);
    }
    req.read_mask = Some(mask.to_field_mask());

    let resp = client
        .execution_client()
        .simulate_transaction(req)
        .await
        .map_err(|e| SimulateError::Rpc(e.to_string()))?
        .into_inner();

    inspect_receipt_from_response(resp)
}

fn proto_transaction_for_ptb(
    sender: Address,
    ptb: sui_sdk_types::ProgrammableTransaction,
) -> ProtoTransaction {
    let ptb_proto: ProtoProgrammableTransaction = ptb.into();
    let mut kind = ProtoTransactionKind::default();
    kind.kind = Some(transaction_kind::Kind::ProgrammableTransaction as i32);
    kind.data = Some(transaction_kind::Data::ProgrammableTransaction(ptb_proto));

    let mut tx = ProtoTransaction::default();
    tx.kind = Some(kind);
    tx.sender = Some(sender.to_string());
    tx
}

fn simulation_receipt_from_response(
    resp: SimulateTransactionResponse,
) -> Result<SimulationReceipt, SimulateError> {
    let executed = resp.transaction.ok_or(SimulateError::MissingExecuted)?;

    let (digest, effects) = decode_executed(&executed)?;
    Ok(SimulationReceipt { digest, effects })
}

fn inspect_receipt_from_response(
    resp: SimulateTransactionResponse,
) -> Result<InspectReceipt, SimulateError> {
    let executed = resp.transaction.ok_or(SimulateError::MissingExecuted)?;

    let (digest, effects) = decode_executed(&executed)?;

    let outputs = resp
        .command_outputs
        .into_iter()
        .map(|command| CommandOutputs {
            return_values: command
                .return_values
                .into_iter()
                .filter_map(bcs_value_from_command_output)
                .collect(),
            mutated_by_ref: command
                .mutated_by_ref
                .into_iter()
                .filter_map(bcs_value_from_command_output)
                .collect(),
        })
        .collect();

    Ok(InspectReceipt {
        digest,
        effects,
        outputs,
    })
}

fn decode_executed(
    executed: &sui_rpc::proto::sui::rpc::v2::ExecutedTransaction,
) -> Result<(Option<Digest>, Option<TransactionEffects>), SimulateError> {
    let digest = executed
        .digest
        .as_deref()
        .and_then(|d| d.parse::<Digest>().ok());

    let effects = executed
        .effects
        .as_ref()
        .map(TransactionEffects::try_from)
        .transpose()
        .map_err(|e: TryFromProtoError| SimulateError::Proto(e.to_string()))?;

    Ok((digest, effects))
}

fn bcs_value_from_command_output(
    output: sui_rpc::proto::sui::rpc::v2::CommandOutput,
) -> Option<BcsValue> {
    let bcs = output.value?;
    Some(BcsValue {
        name: bcs.name,
        bytes: bcs.value.map(|bytes| bytes.to_vec()).unwrap_or_default(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dummy_signature() -> UserSignature {
        let signature =
            sui_sdk_types::Ed25519Signature::new([0; sui_sdk_types::Ed25519Signature::LENGTH]);
        let public_key =
            sui_sdk_types::Ed25519PublicKey::new([0; sui_sdk_types::Ed25519PublicKey::LENGTH]);
        UserSignature::Simple(sui_sdk_types::SimpleSignature::Ed25519 {
            signature,
            public_key,
        })
    }

    #[test]
    fn simulate_receipt_decodes_effects() {
        let effects = TransactionEffects::V2(Box::new(sui_sdk_types::TransactionEffectsV2 {
            status: sui_sdk_types::ExecutionStatus::Success,
            epoch: 0,
            gas_used: sui_sdk_types::GasCostSummary::new(0, 0, 0, 0),
            transaction_digest: Digest::default(),
            gas_object_index: None,
            events_digest: None,
            dependencies: vec![],
            lamport_version: 1,
            changed_objects: vec![],
            unchanged_consensus_objects: vec![],
            auxiliary_data_digest: None,
        }));

        let bcs = sui_rpc::proto::sui::rpc::v2::Bcs::serialize(&effects).unwrap();
        let mut proto_effects = sui_rpc::proto::sui::rpc::v2::TransactionEffects::default();
        proto_effects.bcs = Some(bcs);

        let mut executed = sui_rpc::proto::sui::rpc::v2::ExecutedTransaction::default();
        executed.digest = Some(Digest::default().to_string());
        executed.effects = Some(proto_effects);

        let mut resp = SimulateTransactionResponse::default();
        resp.transaction = Some(executed);

        let receipt = simulation_receipt_from_response(resp).unwrap();
        assert_eq!(receipt.digest, Some(Digest::default()));
        assert!(receipt.effects.is_some());
    }

    #[test]
    fn inspect_receipt_extracts_command_outputs() {
        let executed = sui_rpc::proto::sui::rpc::v2::ExecutedTransaction::default();

        let mut value = sui_rpc::proto::sui::rpc::v2::Bcs::default();
        value.name = Some("u64".to_owned());
        value.value = Some(10u64.to_le_bytes().to_vec().into());

        let mut out = sui_rpc::proto::sui::rpc::v2::CommandOutput::default();
        out.value = Some(value);

        let mut cmd = sui_rpc::proto::sui::rpc::v2::CommandResult::default();
        cmd.return_values = vec![out];

        let mut resp = SimulateTransactionResponse::default();
        resp.transaction = Some(executed);
        resp.command_outputs = vec![cmd];

        let receipt = inspect_receipt_from_response(resp).unwrap();
        assert_eq!(receipt.outputs.len(), 1);
        assert_eq!(receipt.outputs[0].return_values.len(), 1);
        assert_eq!(
            receipt.outputs[0].return_values[0].name.as_deref(),
            Some("u64")
        );
        assert_eq!(
            receipt.outputs[0].return_values[0].bytes,
            10u64.to_le_bytes().to_vec()
        );
    }

    #[test]
    fn receipt_reports_checkpoint_wait_outcome_without_losing_effects() {
        let effects = TransactionEffects::V2(Box::new(sui_sdk_types::TransactionEffectsV2 {
            status: ExecutionStatus::Failure {
                error: sui_sdk_types::ExecutionError::InvariantViolation,
                command: None,
            },
            epoch: 0,
            gas_used: sui_sdk_types::GasCostSummary::new(0, 0, 0, 0),
            transaction_digest: Digest::default(),
            gas_object_index: None,
            events_digest: None,
            dependencies: vec![],
            lamport_version: 1,
            changed_objects: vec![],
            unchanged_consensus_objects: vec![],
            auxiliary_data_digest: None,
        }));

        let bcs = sui_rpc::proto::sui::rpc::v2::Bcs::serialize(&effects).unwrap();
        let mut proto_effects = sui_rpc::proto::sui::rpc::v2::TransactionEffects::default();
        proto_effects.bcs = Some(bcs);

        let mut executed = sui_rpc::proto::sui::rpc::v2::ExecutedTransaction::default();
        executed.digest = Some(Digest::default().to_string());
        executed.effects = Some(proto_effects);

        let signature = dummy_signature();
        let receipt = receipt_from_executed(
            executed,
            Digest::default(),
            signature.clone(),
            Finality::Checkpointed,
            ObservedFinality::Executed,
            CheckpointWaitOutcome::Timeout,
        )
        .unwrap();

        assert_eq!(receipt.requested_finality, Finality::Checkpointed);
        assert_eq!(receipt.observed_finality, ObservedFinality::Executed);
        assert_eq!(receipt.checkpoint_wait, CheckpointWaitOutcome::Timeout);
        assert_eq!(receipt.signature, signature);
        assert!(receipt.effects.is_some());
        assert!(matches!(
            receipt.ensure_success(),
            Err(EnsureSuccessError::Failure(_))
        ));
    }

    #[test]
    fn receipt_can_surface_status_without_effects_bcs() {
        let mut proto_status = sui_rpc::proto::sui::rpc::v2::ExecutionStatus::default();
        proto_status.success = Some(true);

        let mut proto_effects = sui_rpc::proto::sui::rpc::v2::TransactionEffects::default();
        proto_effects.status = Some(proto_status);

        let mut executed = sui_rpc::proto::sui::rpc::v2::ExecutedTransaction::default();
        executed.digest = Some(Digest::default().to_string());
        executed.effects = Some(proto_effects);

        let receipt = receipt_from_executed(
            executed,
            Digest::default(),
            dummy_signature(),
            Finality::Checkpointed,
            ObservedFinality::Executed,
            CheckpointWaitOutcome::Timeout,
        )
        .unwrap();

        assert!(receipt.effects.is_none());
        assert_eq!(receipt.status, Some(ExecutionStatus::Success));
        assert_eq!(receipt.ensure_success(), Ok(()));
    }
}
