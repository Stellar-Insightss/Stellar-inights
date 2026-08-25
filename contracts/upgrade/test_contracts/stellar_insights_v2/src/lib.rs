#![no_std]

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, panic_with_error, Address, Bytes, BytesN,
    Env, Map,
};

const STORAGE_SCHEMA_VERSION: u32 = 2;

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
enum DataKey {
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
pub struct StellarInsightsV2;

#[contractimpl]
impl StellarInsightsV2 {
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
        env.storage()
            .instance()
            .set(&DataKey::StorageSchemaVersion, &STORAGE_SCHEMA_VERSION);
        Ok(())
    }

    pub fn set_upgrade_manager(env: Env, upgrade_manager: Address) -> Result<(), Error> {
        if env.storage().instance().has(&DataKey::UpgradeManager) {
            return Err(Error::UpgradeManagerAlreadySet);
        }
        let admin = admin(&env)?;
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
        ensure_schema(&env)?;
        if caller != admin(&env)? {
            return Err(Error::Unauthorized);
        }
        if epoch == 0 {
            return Err(Error::InvalidEpoch);
        }
        let public_key: BytesN<32> = env
            .storage()
            .instance()
            .get(&DataKey::SigningPublicKey)
            .ok_or(Error::SigningKeyNotSet)?;
        let payload = signed_payload(&env, epoch, &snapshot_hash, &source_data_hash);
        env.crypto()
            .ed25519_verify(&public_key, &payload, &signature);
        let mut snapshots: Map<u64, Snapshot> = env
            .storage()
            .persistent()
            .get(&DataKey::Snapshots)
            .unwrap_or_else(|| Map::new(&env));
        if snapshots.contains_key(epoch) {
            return Err(Error::DuplicateEpoch);
        }
        let latest = env
            .storage()
            .instance()
            .get(&DataKey::LatestEpoch)
            .unwrap_or(0);
        if epoch <= latest {
            return Err(Error::EpochMonotonicityViolated);
        }
        let submitted_at = env.ledger().timestamp();
        snapshots.set(
            epoch,
            Snapshot {
                epoch,
                snapshot_hash,
                source_data_hash,
                submitted_at,
                submitter: caller,
            },
        );
        env.storage()
            .persistent()
            .set(&DataKey::Snapshots, &snapshots);
        env.storage().instance().set(&DataKey::LatestEpoch, &epoch);
        Ok(submitted_at)
    }

    pub fn get_snapshot(env: Env, epoch: u64) -> Result<Snapshot, Error> {
        ensure_schema(&env)?;
        let snapshots: Map<u64, Snapshot> = env
            .storage()
            .persistent()
            .get(&DataKey::Snapshots)
            .unwrap_or_else(|| Map::new(&env));
        snapshots.get(epoch).ok_or(Error::SnapshotNotFound)
    }

    pub fn latest_epoch(env: Env) -> u64 {
        require_schema(&env);
        env.storage()
            .instance()
            .get(&DataKey::LatestEpoch)
            .unwrap_or(0)
    }

    pub fn governance_upgrade(
        env: Env,
        new_wasm_hash: BytesN<32>,
        expected_source_schema: u32,
        target_schema: u32,
    ) -> Result<(), Error> {
        let manager: Address = env
            .storage()
            .instance()
            .get(&DataKey::UpgradeManager)
            .ok_or(Error::UpgradeManagerNotSet)?;
        manager.require_auth();
        validate_transition(expected_source_schema, target_schema)?;
        let stored: Option<u32> = env.storage().instance().get(&DataKey::StorageSchemaVersion);
        match (stored, expected_source_schema) {
            (Some(actual), expected) if actual == expected => {}
            (None, 0) if has_state(&env) => {}
            _ => return Err(Error::SchemaMismatch),
        }
        env.deployer().update_current_contract_wasm(new_wasm_hash);
        Ok(())
    }

    pub fn migrate_schema(
        env: Env,
        expected_source_schema: u32,
        target_schema: u32,
    ) -> Result<(), Error> {
        let manager: Address = env
            .storage()
            .instance()
            .get(&DataKey::UpgradeManager)
            .ok_or(Error::UpgradeManagerNotSet)?;
        manager.require_auth();
        validate_transition(expected_source_schema, target_schema)?;
        let stored: Option<u32> = env.storage().instance().get(&DataKey::StorageSchemaVersion);
        match (stored, expected_source_schema) {
            (Some(actual), expected) if actual == expected => {}
            (None, 0) if has_state(&env) => {}
            _ => return Err(Error::SchemaMismatch),
        }
        if !has_state(&env) {
            return Err(Error::NotInitialized);
        }
        env.storage()
            .instance()
            .set(&DataKey::StorageSchemaVersion, &STORAGE_SCHEMA_VERSION);
        Ok(())
    }
}

fn ensure_schema(env: &Env) -> Result<(), Error> {
    let actual: Option<u32> = env.storage().instance().get(&DataKey::StorageSchemaVersion);
    match actual {
        Some(actual) if actual == STORAGE_SCHEMA_VERSION => Ok(()),
        _ => Err(Error::SchemaMismatch),
    }
}

fn require_schema(env: &Env) {
    if ensure_schema(env).is_err() {
        panic_with_error!(env, Error::SchemaMismatch);
    }
}

fn admin(env: &Env) -> Result<Address, Error> {
    env.storage()
        .instance()
        .get(&DataKey::Admin)
        .ok_or(Error::AdminNotSet)
}

fn has_state(env: &Env) -> bool {
    env.storage().instance().has(&DataKey::Admin)
        && env.storage().instance().has(&DataKey::SigningPublicKey)
        && env.storage().instance().has(&DataKey::LatestEpoch)
}

fn validate_transition(source: u32, target: u32) -> Result<(), Error> {
    // V2 explicitly models only these source layouts and writes only schema 2.
    match (source, target) {
        (0, STORAGE_SCHEMA_VERSION)
        | (1, STORAGE_SCHEMA_VERSION)
        | (STORAGE_SCHEMA_VERSION, STORAGE_SCHEMA_VERSION) => Ok(()),
        _ => Err(Error::InvalidSchemaTransition),
    }
}

fn signed_payload(
    env: &Env,
    epoch: u64,
    snapshot_hash: &BytesN<32>,
    source_data_hash: &BytesN<32>,
) -> Bytes {
    let mut payload = Bytes::from_slice(env, &epoch.to_be_bytes());
    payload.append(&Bytes::from_array(env, &snapshot_hash.to_array()));
    payload.append(&Bytes::from_array(env, &source_data_hash.to_array()));
    payload
}
