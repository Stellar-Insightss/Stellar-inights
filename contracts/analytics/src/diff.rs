//! Diff incoming snapshot metrics against the retained previous snapshot.
//!
//! Required on-chain read: **only** `storage::PreviousSnapshot` (or none on
//! the first ingest). Historical epochs are never consulted.

use soroban_sdk::{Env, Map, Symbol};

use crate::storage::Snapshot;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SnapshotDiff {
    pub from_epoch: u64,
    pub to_epoch: u64,
    pub added: Map<Symbol, i128>,
    pub removed: Map<Symbol, i128>,
    /// Values are (old, new).
    pub changed: Map<Symbol, (i128, i128)>,
}

pub fn diff_against_previous(
    env: &Env,
    previous: Option<&Snapshot>,
    next: &Snapshot,
) -> SnapshotDiff {
    let mut added = Map::new(env);
    let mut removed = Map::new(env);
    let mut changed = Map::new(env);

    let Some(prev) = previous else {
        for key in next.metrics.keys().iter() {
            added.set(key.clone(), next.metrics.get(key).unwrap());
        }
        return SnapshotDiff {
            from_epoch: 0,
            to_epoch: next.epoch,
            added,
            removed,
            changed,
        };
    };

    for key in next.metrics.keys().iter() {
        let new_val = next.metrics.get(key.clone()).unwrap();
        match prev.metrics.get(key.clone()) {
            None => added.set(key, new_val),
            Some(old_val) if old_val != new_val => changed.set(key, (old_val, new_val)),
            Some(_) => {}
        }
    }
    for key in prev.metrics.keys().iter() {
        if next.metrics.get(key.clone()).is_none() {
            removed.set(key.clone(), prev.metrics.get(key).unwrap());
        }
    }

    SnapshotDiff {
        from_epoch: prev.epoch,
        to_epoch: next.epoch,
        added,
        removed,
        changed,
    }
}
