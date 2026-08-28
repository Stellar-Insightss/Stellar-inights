#![no_std]

pub mod binding;
pub mod event;
mod submit;

use event::{SnapshotSubmitted, SNAPSHOT_SUBMITTED_SCHEMA_VERSION};
use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, panic_with_error, Address, BytesN, Env,
    Map,
};

pub const CURRENT_STORAGE_SCHEMA_VERSION: u32 = 1;

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
    SchemaMismatch = 10,
    UpgradeManagerNotSet = 11,
    InvalidSchemaTransition = 12,
    UpgradeManagerAlreadySet = 13,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DataKey {
    Admin,
    SigningPublicKey,
    Snapshots,
    LatestEpoch,
    StorageSchemaVersion,
    UpgradeManager,
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
        env.storage().instance().set(
            &DataKey::StorageSchemaVersion,
            &CURRENT_STORAGE_SCHEMA_VERSION,
        );
        Ok(())
    }

    pub fn set_upgrade_manager(env: Env, upgrade_manager: Address) -> Result<(), Error> {
        if env.storage().instance().has(&DataKey::UpgradeManager) {
            return Err(Error::UpgradeManagerAlreadySet);
        }
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(Error::AdminNotSet)?;
        admin.require_auth();
        env.storage()
            .instance()
            .set(&DataKey::UpgradeManager, &upgrade_manager);
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
        ensure_schema(&env, CURRENT_STORAGE_SCHEMA_VERSION)?;
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
        submit::verify_signature(&env, epoch, &snapshot_hash, &source_data_hash, &signature)?;

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
        ensure_schema(&env, CURRENT_STORAGE_SCHEMA_VERSION)?;
        let snapshots: Map<u64, Snapshot> = env
            .storage()
            .persistent()
            .get(&DataKey::Snapshots)
            .unwrap_or_else(|| Map::new(&env));
        snapshots.get(epoch).ok_or(Error::SnapshotNotFound)
    }

    pub fn latest_epoch(env: Env) -> u64 {
        require_schema(&env, CURRENT_STORAGE_SCHEMA_VERSION);
        env.storage()
            .instance()
            .get(&DataKey::LatestEpoch)
            .unwrap_or(0)
    }

    /// The manager calls this stable entrypoint before the executable changes.
    /// The update itself takes effect only after the outer invocation succeeds.
    pub fn governance_upgrade(
        env: Env,
        new_wasm_hash: BytesN<32>,
        expected_schema_version: u32,
        target_schema_version: u32,
    ) -> Result<(), Error> {
        let manager: Address = env
            .storage()
            .instance()
            .get(&DataKey::UpgradeManager)
            .ok_or(Error::UpgradeManagerNotSet)?;
        manager.require_auth();
        validate_upgrade_transition(expected_schema_version, target_schema_version)?;

        let current_schema: Option<u32> =
            env.storage().instance().get(&DataKey::StorageSchemaVersion);
        match (current_schema, expected_schema_version) {
            (Some(current), expected) if current == expected => {}
            (None, 0) if has_legacy_state(&env) => {}
            _ => return Err(Error::SchemaMismatch),
        }

        env.deployer().update_current_contract_wasm(new_wasm_hash);
        Ok(())
    }

    /// Migration is separate from the WASM update because the new executable
    /// is not active until the upgrade invocation commits successfully.
    pub fn migrate_schema(
        env: Env,
        expected_source_schema: u32,
        target_schema_version: u32,
    ) -> Result<(), Error> {
        let manager: Address = env
            .storage()
            .instance()
            .get(&DataKey::UpgradeManager)
            .ok_or(Error::UpgradeManagerNotSet)?;
        manager.require_auth();
        validate_migration_transition(expected_source_schema, target_schema_version)?;

        let current_schema: Option<u32> =
            env.storage().instance().get(&DataKey::StorageSchemaVersion);
        match (current_schema, expected_source_schema) {
            (Some(current), expected) if current == expected => {}
            (None, 0) if has_legacy_state(&env) => {}
            _ => return Err(Error::SchemaMismatch),
        }

        // These values are required by every supported schema. Refusing to
        // write the marker when they are absent prevents a partial legacy
        // layout from being declared migrated.
        if !has_legacy_state(&env) {
            return Err(Error::NotInitialized);
        }
        env.storage().instance().set(
            &DataKey::StorageSchemaVersion,
            &CURRENT_STORAGE_SCHEMA_VERSION,
        );
        Ok(())
    }
}

fn ensure_schema(env: &Env, expected: u32) -> Result<(), Error> {
    let actual: Option<u32> = env.storage().instance().get(&DataKey::StorageSchemaVersion);
    match actual {
        Some(actual) if actual == expected => Ok(()),
        _ => Err(Error::SchemaMismatch),
    }
}

fn require_schema(env: &Env, expected: u32) {
    if ensure_schema(env, expected).is_err() {
        panic_with_error!(env, Error::SchemaMismatch);
    }
}

fn has_legacy_state(env: &Env) -> bool {
    env.storage().instance().has(&DataKey::Admin)
        && env.storage().instance().has(&DataKey::SigningPublicKey)
        && env.storage().instance().has(&DataKey::LatestEpoch)
}

fn validate_upgrade_transition(source: u32, target: u32) -> Result<(), Error> {
    // The current executable can verify source schemas 0 and 1. The candidate
    // executable remains responsible for validating its own target schema.
    if target == 0 || source > CURRENT_STORAGE_SCHEMA_VERSION {
        return Err(Error::InvalidSchemaTransition);
    }
    Ok(())
}

fn validate_migration_transition(source: u32, target: u32) -> Result<(), Error> {
    // This executable implements schema 1. It can validate the markerless
    // legacy layout (0 -> 1) and the already-current no-op (1 -> 1), but it
    // cannot truthfully write any other final schema marker.
    match (source, target) {
        (0, CURRENT_STORAGE_SCHEMA_VERSION)
        | (CURRENT_STORAGE_SCHEMA_VERSION, CURRENT_STORAGE_SCHEMA_VERSION) => Ok(()),
        _ => Err(Error::InvalidSchemaTransition),
    }
}
