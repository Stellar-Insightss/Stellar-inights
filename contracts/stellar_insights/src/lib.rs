#![no_std]

pub mod binding;
pub mod event;
mod submit;

use event::{SnapshotSubmitted, SNAPSHOT_SUBMITTED_SCHEMA_VERSION};
use soroban_sdk::{contract, contracterror, contractimpl, contracttype, Address, BytesN, Env, Map};

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum Error {
    AlreadyInitialized = 1,
    NotInitialized = 2,
    Unauthorized = 3,
    InvalidEpoch = 4,
    DuplicateEpoch = 5,
    EpochMonotonicityViolated = 6,
    SnapshotNotFound = 7,
    AdminNotSet = 8,
    SigningKeyNotSet = 9,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DataKey {
    Admin,
    SigningPublicKey,
    Snapshots,
    LatestEpoch,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Snapshot {
    pub epoch: u64,
    pub snapshot_hash: BytesN<32>,
    pub source_data_hash: BytesN<32>,
    pub submitted_at: u64,
    pub submitter: Address,
}

#[contract]
pub struct StellarInsights;

#[contractimpl]
impl StellarInsights {
    pub fn initialize(
        env: Env,
        admin: Address,
        signing_public_key: BytesN<32>,
    ) -> Result<(), Error> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(Error::AlreadyInitialized);
        }
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage()
            .instance()
            .set(&DataKey::SigningPublicKey, &signing_public_key);
        env.storage().instance().set(&DataKey::LatestEpoch, &0u64);
        Ok(())
    }

    pub fn submit_snapshot(
        env: Env,
        epoch: u64,
        snapshot_hash: BytesN<32>,
        source_data_hash: BytesN<32>,
        signature: BytesN<64>,
        caller: Address,
    ) -> Result<u64, Error> {
        caller.require_auth();
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(Error::AdminNotSet)?;
        if caller != admin {
            return Err(Error::Unauthorized);
        }
        if epoch == 0 {
            return Err(Error::InvalidEpoch);
        }

        // This check happens before storage mutation, so a forged submission
        // cannot reserve an epoch even if the caller is the authorized admin.
        submit::verify_signature(
            &env,
            epoch,
            &snapshot_hash,
            &source_data_hash,
            &signature,
        )?;

        let mut snapshots: Map<u64, Snapshot> = env
            .storage()
            .persistent()
            .get(&DataKey::Snapshots)
            .unwrap_or_else(|| Map::new(&env));
        if snapshots.contains_key(epoch) {
            return Err(Error::DuplicateEpoch);
        }
        let latest: u64 = env
            .storage()
            .instance()
            .get(&DataKey::LatestEpoch)
            .unwrap_or(0);
        if epoch <= latest {
            return Err(Error::EpochMonotonicityViolated);
        }

        let submitted_at = env.ledger().timestamp();
        let snapshot = Snapshot {
            epoch,
            snapshot_hash,
            source_data_hash,
            submitted_at,
            submitter: caller,
        };
        snapshots.set(epoch, snapshot.clone());
        env.storage()
            .persistent()
            .set(&DataKey::Snapshots, &snapshots);
        env.storage().instance().set(&DataKey::LatestEpoch, &epoch);

        SnapshotSubmitted {
            schema_version: SNAPSHOT_SUBMITTED_SCHEMA_VERSION,
            epoch: snapshot.epoch,
            snapshot_hash: snapshot.snapshot_hash,
            source_data_hash: snapshot.source_data_hash,
            submitted_at: snapshot.submitted_at,
            submitter: snapshot.submitter,
        }
        .publish(&env);

        Ok(submitted_at)
    }

    pub fn get_snapshot(env: Env, epoch: u64) -> Result<Snapshot, Error> {
        let snapshots: Map<u64, Snapshot> = env
            .storage()
            .persistent()
            .get(&DataKey::Snapshots)
            .unwrap_or_else(|| Map::new(&env));
        snapshots.get(epoch).ok_or(Error::SnapshotNotFound)
    }

    pub fn latest_epoch(env: Env) -> u64 {
        env.storage()
            .instance()
            .get(&DataKey::LatestEpoch)
            .unwrap_or(0)
    }
}
