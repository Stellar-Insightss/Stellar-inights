//! On-chain storage lifecycle for the analytics contract.
//!
//! # What `diff.rs` needs at ingest time
//!
//! Diffing is always "incoming batch vs immediately previous snapshot".
//! That is the **only** snapshot that must be readable on-chain. Full
//! history is *not* required for a correct ingest.
//!
//! # TTL / bump policy
//!
//! | Entry | Storage | Bumped? | Why |
//! |---|---|---|---|
//! | Admin, paused | instance | yes, every ingest / admin op | Tiny, always-hot control plane. Instance TTL is cheap vs restoring. |
//! | Latest availability proof (epoch + hash) | instance | yes, every ingest | Compact proof that a given epoch was committed; overwritten, not appended. |
//! | Previous snapshot (metrics + hashes) | persistent | yes, every ingest | The sole working set `diff` reads. Same key is overwritten. |
//! | Historical snapshots (epoch N-2, N-3, …) | **not stored** | n/a | Would make persistent keys and rent grow linearly. Durable copy lives off-chain. |
//!
//! Archived entries: we never create extra persistent keys, so there is
//! nothing historical to restore. If `PreviousSnapshot` ever archived
//! (TTL not extended because ingest stopped), the next ingest treats it
//! as missing genesis-equivalent state and the off-chain store is the
//! source of truth for reconstructing the last snapshot before resuming.
//!
//! # On-chain vs off-chain
//!
//! - **On-chain:** live working set = previous snapshot + availability proof.
//!   Bounded: O(1) keys regardless of ingest count.
//! - **Off-chain (this repo's backend):** full snapshot history, diffs, and
//!   query API. Contract events / return values are the ingest receipt.

use soroban_sdk::{contracttype, Address, BytesN, Env, Map, Symbol};

use crate::Error;

/// Ledgers to keep hot state alive (~31 days at 5s ledgers).
///
/// Bumped on every successful ingest so continuously operated deployments
/// never archive the working set. Idle deployments may archive; see module
/// docs for the restore/off-chain fallback.
pub const HOT_TTL_EXTEND_TO: u32 = 535_680;
/// Only bump when remaining TTL is below this (avoids paying extend every call
/// once already far out).
pub const HOT_TTL_THRESHOLD: u32 = 100_000;

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DataKey {
    Admin,
    Paused,
    /// Compact (epoch, hash) overwritten each ingest — not a growing log.
    LatestProof,
    /// The only persistent snapshot. Overwritten each ingest.
    PreviousSnapshot,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Snapshot {
    pub epoch: u64,
    pub metrics: Map<Symbol, i128>,
    pub snapshot_hash: BytesN<32>,
    pub source_data_hash: BytesN<32>,
    pub submitted_at: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AvailabilityProof {
    pub epoch: u64,
    pub snapshot_hash: BytesN<32>,
}

/// Count of persistent keys this contract ever writes.
/// Always 0 or 1 (`PreviousSnapshot`). Used by growth tests.
pub const PERSISTENT_LIVE_KEYS: u32 = 1;

pub fn require_admin(env: &Env, caller: &Address) -> Result<(), Error> {
    let admin: Address = env
        .storage()
        .instance()
        .get(&DataKey::Admin)
        .ok_or(Error::NotInitialized)?;
    if caller != &admin {
        return Err(Error::Unauthorized);
    }
    Ok(())
}

pub fn bump_hot_state(env: &Env) {
    env.storage()
        .instance()
        .extend_ttl(HOT_TTL_THRESHOLD, HOT_TTL_EXTEND_TO);
    if env.storage().persistent().has(&DataKey::PreviousSnapshot) {
        env.storage().persistent().extend_ttl(
            &DataKey::PreviousSnapshot,
            HOT_TTL_THRESHOLD,
            HOT_TTL_EXTEND_TO,
        );
    }
}

pub fn load_previous(env: &Env) -> Option<Snapshot> {
    env.storage().persistent().get(&DataKey::PreviousSnapshot)
}

/// Replace the previous snapshot in place. Does **not** allocate a new key
/// per epoch — this is what keeps growth O(1).
pub fn retain_as_previous(env: &Env, snapshot: &Snapshot) {
    env.storage()
        .persistent()
        .set(&DataKey::PreviousSnapshot, snapshot);
    env.storage().instance().set(
        &DataKey::LatestProof,
        &AvailabilityProof {
            epoch: snapshot.epoch,
            snapshot_hash: snapshot.snapshot_hash.clone(),
        },
    );
    bump_hot_state(env);
}

/// Persistent entries that currently exist (0 before first ingest, 1 after).
pub fn persistent_entry_count(env: &Env) -> u32 {
    if env.storage().persistent().has(&DataKey::PreviousSnapshot) {
        PERSISTENT_LIVE_KEYS
    } else {
        0
    }
}
