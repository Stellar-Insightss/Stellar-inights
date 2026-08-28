#![no_std]

mod diff;
mod pause;
mod storage;

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, Address, BytesN, Env, Map, Symbol,
};

pub use storage::{AvailabilityProof, Snapshot, PERSISTENT_LIVE_KEYS};

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum Error {
    AlreadyInitialized = 1,
    NotInitialized = 2,
    Unauthorized = 3,
    InvalidEpoch = 4,
    EpochMonotonicityViolated = 5,
    ContractPaused = 6,
    EmptyMetrics = 7,
}

/// Return value of a successful ingest. Diff is **not** persisted (that would
/// grow storage); the backend records it off-chain from this receipt.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IngestReceipt {
    pub epoch: u64,
    pub from_epoch: u64,
    pub added_count: u32,
    pub removed_count: u32,
    pub changed_count: u32,
    /// Persistent keys after this ingest. Always 1 once a snapshot exists.
    pub persistent_entries: u32,
}

#[contract]
pub struct AnalyticsContract;

#[contractimpl]
impl AnalyticsContract {
    pub fn initialize(env: Env, admin: Address) -> Result<(), Error> {
        if env.storage().instance().has(&storage::DataKey::Admin) {
            return Err(Error::AlreadyInitialized);
        }
        env.storage()
            .instance()
            .set(&storage::DataKey::Admin, &admin);
        pause::set_paused(&env, false);
        Ok(())
    }

    pub fn pause(env: Env, caller: Address) -> Result<(), Error> {
        caller.require_auth();
        storage::require_admin(&env, &caller)?;
        pause::set_paused(&env, true);
        Ok(())
    }

    pub fn unpause(env: Env, caller: Address) -> Result<(), Error> {
        caller.require_auth();
        storage::require_admin(&env, &caller)?;
        pause::set_paused(&env, false);
        Ok(())
    }

    pub fn is_paused(env: Env) -> bool {
        pause::is_paused(&env)
    }

    /// Ingest a batched snapshot. Diffs against the retained previous snapshot,
    /// then **overwrites** that same persistent key. History is not kept on-chain.
    pub fn submit_snapshot(
        env: Env,
        caller: Address,
        epoch: u64,
        metrics: Map<Symbol, i128>,
        snapshot_hash: BytesN<32>,
        source_data_hash: BytesN<32>,
    ) -> Result<IngestReceipt, Error> {
        caller.require_auth();
        storage::require_admin(&env, &caller)?;
        if pause::is_paused(&env) {
            return Err(Error::ContractPaused);
        }
        if epoch == 0 {
            return Err(Error::InvalidEpoch);
        }
        if metrics.is_empty() {
            return Err(Error::EmptyMetrics);
        }

        let previous = storage::load_previous(&env);
        if let Some(ref prev) = previous {
            if epoch <= prev.epoch {
                return Err(Error::EpochMonotonicityViolated);
            }
        }

        let next = Snapshot {
            epoch,
            metrics,
            snapshot_hash,
            source_data_hash,
            submitted_at: env.ledger().timestamp(),
        };
        let d = diff::diff_against_previous(&env, previous.as_ref(), &next);
        storage::retain_as_previous(&env, &next);

        Ok(IngestReceipt {
            epoch,
            from_epoch: d.from_epoch,
            added_count: d.added.len(),
            removed_count: d.removed.len(),
            changed_count: d.changed.len(),
            persistent_entries: storage::persistent_entry_count(&env),
        })
    }

    pub fn previous_snapshot(env: Env) -> Option<Snapshot> {
        storage::load_previous(&env)
    }

    pub fn latest_proof(env: Env) -> Option<AvailabilityProof> {
        env.storage()
            .instance()
            .get(&storage::DataKey::LatestProof)
    }

    /// Live persistent working-set size. Bounded at 1; independent of ingest count.
    pub fn persistent_entry_count(env: Env) -> u32 {
        storage::persistent_entry_count(&env)
    }
}
