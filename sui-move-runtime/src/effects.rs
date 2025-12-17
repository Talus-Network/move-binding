use std::collections::HashMap;

use sui_sdk_types::{
    ChangedObject, IdOperation, ObjectOut, ObjectReference, ObjectReferenceWithOwner, Owner,
    TransactionEffects,
};

#[derive(Clone, Debug)]
pub(crate) struct ReferenceUpdate {
    pub(crate) object_id: sui_sdk_types::Address,
    pub(crate) reference: ObjectReference,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum TombstoneReason {
    Deleted,
    Wrapped,
    UnwrappedThenDeleted,
    NotExist,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Tombstone {
    pub(crate) object_id: sui_sdk_types::Address,
    pub(crate) reason: TombstoneReason,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Upsert {
    pub(crate) object_id: sui_sdk_types::Address,
    pub(crate) reference: ObjectReference,
    pub(crate) owner: Option<Owner>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct EffectsPatch {
    pub(crate) upserts: Vec<Upsert>,
    pub(crate) tombstones: Vec<Tombstone>,
}

impl EffectsPatch {
    pub(crate) fn from_effects(effects: &TransactionEffects) -> Self {
        match effects {
            TransactionEffects::V2(v2) => patch_from_v2(v2),
            TransactionEffects::V1(v1) => patch_from_v1(v1),
        }
    }
}

pub(crate) fn updated_references(effects: &TransactionEffects) -> Vec<ReferenceUpdate> {
    let patch = EffectsPatch::from_effects(effects);
    patch
        .upserts
        .into_iter()
        .map(|upsert| ReferenceUpdate {
            object_id: upsert.object_id,
            reference: upsert.reference,
        })
        .collect()
}

fn patch_from_v1(effects: &sui_sdk_types::TransactionEffectsV1) -> EffectsPatch {
    let mut upserts = HashMap::<sui_sdk_types::Address, Upsert>::new();
    extend_v1_upserts(&mut upserts, &effects.created);
    extend_v1_upserts(&mut upserts, &effects.mutated);
    extend_v1_upserts(&mut upserts, &effects.unwrapped);
    extend_v1_upserts(&mut upserts, std::slice::from_ref(&effects.gas_object));

    let mut tombstones = HashMap::<sui_sdk_types::Address, TombstoneReason>::new();
    for r in &effects.deleted {
        tombstones.insert(*r.object_id(), TombstoneReason::Deleted);
    }
    for r in &effects.unwrapped_then_deleted {
        tombstones.insert(*r.object_id(), TombstoneReason::UnwrappedThenDeleted);
    }
    for r in &effects.wrapped {
        tombstones.insert(*r.object_id(), TombstoneReason::Wrapped);
    }

    EffectsPatch {
        upserts: upserts.into_values().collect(),
        tombstones: tombstones
            .into_iter()
            .map(|(object_id, reason)| Tombstone { object_id, reason })
            .collect(),
    }
}

fn extend_v1_upserts(
    out: &mut HashMap<sui_sdk_types::Address, Upsert>,
    refs: &[ObjectReferenceWithOwner],
) {
    for r in refs {
        let object_id = *r.reference.object_id();
        match out.get(&object_id) {
            Some(existing) if existing.reference.version() >= r.reference.version() => {}
            _ => {
                out.insert(
                    object_id,
                    Upsert {
                        object_id,
                        reference: r.reference.clone(),
                        owner: Some(r.owner),
                    },
                );
            }
        }
    }
}

fn patch_from_v2(effects: &sui_sdk_types::TransactionEffectsV2) -> EffectsPatch {
    let lamport_version = effects.lamport_version;

    let mut upserts = HashMap::<sui_sdk_types::Address, Upsert>::new();
    let mut tombstones = HashMap::<sui_sdk_types::Address, TombstoneReason>::new();

    for change in &effects.changed_objects {
        match v2_change_to_upsert(change, lamport_version) {
            Some(upsert) => match upserts.get(&upsert.object_id) {
                Some(existing) if existing.reference.version() >= upsert.reference.version() => {}
                _ => {
                    upserts.insert(upsert.object_id, upsert);
                }
            },
            None => {
                if let Some(reason) = v2_change_to_tombstone_reason(change) {
                    tombstones.insert(change.object_id, reason);
                }
            }
        }
    }

    // An object should not be both "live" and tombstoned.
    for id in tombstones.keys() {
        upserts.remove(id);
    }

    EffectsPatch {
        upserts: upserts.into_values().collect(),
        tombstones: tombstones
            .into_iter()
            .map(|(object_id, reason)| Tombstone { object_id, reason })
            .collect(),
    }
}

fn v2_change_to_tombstone_reason(change: &ChangedObject) -> Option<TombstoneReason> {
    if matches!(change.output_state, ObjectOut::NotExist) {
        return Some(TombstoneReason::NotExist);
    }
    if matches!(change.id_operation, IdOperation::Deleted) {
        return Some(TombstoneReason::Deleted);
    }
    None
}

fn v2_change_to_upsert(
    change: &ChangedObject,
    lamport_version: sui_sdk_types::Version,
) -> Option<Upsert> {
    match &change.output_state {
        ObjectOut::ObjectWrite { digest, owner } => Some(Upsert {
            object_id: change.object_id,
            reference: ObjectReference::new(change.object_id, lamport_version, *digest),
            owner: Some(*owner),
        }),
        ObjectOut::PackageWrite { version, digest } => Some(Upsert {
            object_id: change.object_id,
            reference: ObjectReference::new(change.object_id, *version, *digest),
            owner: None,
        }),
        ObjectOut::NotExist => None,
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use sui_sdk_types::{
        Digest, ExecutionStatus, GasCostSummary, ModifiedAtVersion, ObjectIn, TransactionEffectsV1,
        TransactionEffectsV2,
    };

    #[test]
    fn patch_from_v1_collects_upserts_and_tombstones() {
        let a = sui_sdk_types::Address::from_hex("0x1").unwrap();
        let b = sui_sdk_types::Address::from_hex("0x2").unwrap();
        let c = sui_sdk_types::Address::from_hex("0x3").unwrap();
        let d = sui_sdk_types::Address::from_hex("0x4").unwrap();

        let created = ObjectReferenceWithOwner {
            reference: ObjectReference::new(a, 1, Digest::default()),
            owner: Owner::Immutable,
        };
        let mutated = ObjectReferenceWithOwner {
            reference: ObjectReference::new(b, 2, Digest::default()),
            owner: Owner::Immutable,
        };

        let effects = TransactionEffects::V1(Box::new(TransactionEffectsV1 {
            status: ExecutionStatus::Success,
            epoch: 0,
            gas_used: GasCostSummary::new(0, 0, 0, 0),
            modified_at_versions: vec![ModifiedAtVersion {
                object_id: b,
                version: 1,
            }],
            consensus_objects: vec![],
            transaction_digest: Digest::default(),
            created: vec![created.clone()],
            mutated: vec![mutated.clone()],
            unwrapped: vec![],
            deleted: vec![ObjectReference::new(c, 3, Digest::default())],
            unwrapped_then_deleted: vec![ObjectReference::new(d, 4, Digest::default())],
            wrapped: vec![ObjectReference::new(a, 5, Digest::default())],
            gas_object: mutated,
            events_digest: None,
            dependencies: vec![],
        }));

        let patch = EffectsPatch::from_effects(&effects);

        let upsert_ids: HashSet<_> = patch.upserts.iter().map(|u| u.object_id).collect();
        assert!(upsert_ids.contains(&a));
        assert!(upsert_ids.contains(&b));

        let tombstones: HashMap<_, _> = patch
            .tombstones
            .iter()
            .map(|t| (t.object_id, t.reason))
            .collect();
        assert_eq!(tombstones.get(&c), Some(&TombstoneReason::Deleted));
        assert_eq!(
            tombstones.get(&d),
            Some(&TombstoneReason::UnwrappedThenDeleted)
        );
        assert_eq!(tombstones.get(&a), Some(&TombstoneReason::Wrapped));
    }

    #[test]
    fn patch_from_v2_collects_upserts_and_tombstones() {
        let a = sui_sdk_types::Address::from_hex("0x1").unwrap();
        let b = sui_sdk_types::Address::from_hex("0x2").unwrap();
        let c = sui_sdk_types::Address::from_hex("0x3").unwrap();

        let effects = TransactionEffects::V2(Box::new(TransactionEffectsV2 {
            status: ExecutionStatus::Success,
            epoch: 0,
            gas_used: GasCostSummary::new(0, 0, 0, 0),
            transaction_digest: Digest::default(),
            gas_object_index: None,
            events_digest: None,
            dependencies: vec![],
            lamport_version: 7,
            changed_objects: vec![
                ChangedObject {
                    object_id: a,
                    input_state: ObjectIn::NotExist,
                    output_state: ObjectOut::ObjectWrite {
                        digest: Digest::default(),
                        owner: Owner::Immutable,
                    },
                    id_operation: IdOperation::Created,
                },
                ChangedObject {
                    object_id: b,
                    input_state: ObjectIn::NotExist,
                    output_state: ObjectOut::PackageWrite {
                        version: 42,
                        digest: Digest::default(),
                    },
                    id_operation: IdOperation::Created,
                },
                ChangedObject {
                    object_id: c,
                    input_state: ObjectIn::Exist {
                        version: 1,
                        digest: Digest::default(),
                        owner: Owner::Immutable,
                    },
                    output_state: ObjectOut::NotExist,
                    id_operation: IdOperation::Deleted,
                },
            ],
            unchanged_consensus_objects: vec![],
            auxiliary_data_digest: None,
        }));

        let patch = EffectsPatch::from_effects(&effects);

        let upserts: HashMap<_, _> = patch
            .upserts
            .iter()
            .map(|u| (u.object_id, u.clone()))
            .collect();

        let upsert_a = upserts.get(&a).expect("missing upsert a");
        assert_eq!(upsert_a.reference.version(), 7);
        assert_eq!(upsert_a.owner, Some(Owner::Immutable));

        let upsert_b = upserts.get(&b).expect("missing upsert b");
        assert_eq!(upsert_b.reference.version(), 42);
        assert_eq!(upsert_b.owner, None);

        let tombstones: HashMap<_, _> = patch
            .tombstones
            .iter()
            .map(|t| (t.object_id, t.reason))
            .collect();
        assert_eq!(tombstones.get(&c), Some(&TombstoneReason::NotExist));
    }
}
