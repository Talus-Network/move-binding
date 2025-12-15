use sui_sdk_types::{
    ChangedObject, ObjectOut, ObjectReference, ObjectReferenceWithOwner, TransactionEffects,
};

#[derive(Clone, Debug)]
pub(crate) struct ReferenceUpdate {
    pub(crate) object_id: sui_sdk_types::Address,
    pub(crate) reference: ObjectReference,
}

pub(crate) fn updated_references(effects: &TransactionEffects) -> Vec<ReferenceUpdate> {
    match effects {
        TransactionEffects::V2(v2) => updated_references_v2(v2),
        TransactionEffects::V1(v1) => updated_references_v1(v1),
    }
}

fn updated_references_v2(effects: &sui_sdk_types::TransactionEffectsV2) -> Vec<ReferenceUpdate> {
    let version = effects.lamport_version;
    effects
        .changed_objects
        .iter()
        .filter_map(|change| ref_from_v2_change(change, version))
        .collect()
}

fn ref_from_v2_change(
    change: &ChangedObject,
    lamport_version: sui_sdk_types::Version,
) -> Option<ReferenceUpdate> {
    match &change.output_state {
        ObjectOut::ObjectWrite { digest, .. } => Some(ReferenceUpdate {
            object_id: change.object_id,
            reference: ObjectReference::new(change.object_id, lamport_version, *digest),
        }),
        ObjectOut::PackageWrite { version, digest } => Some(ReferenceUpdate {
            object_id: change.object_id,
            reference: ObjectReference::new(change.object_id, *version, *digest),
        }),
        ObjectOut::NotExist | ObjectOut::AccumulatorWrite(_) => None,
        _ => None,
    }
}

fn updated_references_v1(effects: &sui_sdk_types::TransactionEffectsV1) -> Vec<ReferenceUpdate> {
    let mut out = Vec::new();
    extend_v1_refs(&mut out, &effects.created);
    extend_v1_refs(&mut out, &effects.mutated);
    extend_v1_refs(&mut out, &effects.unwrapped);
    out
}

fn extend_v1_refs(out: &mut Vec<ReferenceUpdate>, refs: &[ObjectReferenceWithOwner]) {
    out.extend(refs.iter().map(|r| ReferenceUpdate {
        object_id: *r.reference.object_id(),
        reference: r.reference.clone(),
    }));
}
