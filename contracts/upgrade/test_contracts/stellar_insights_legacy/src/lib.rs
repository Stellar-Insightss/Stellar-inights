#![no_std]

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, Address, Bytes, BytesN, Env, Map,
};

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
pub struct StellarInsightsLegacy;

#[contractimpl]
impl StellarInsightsLegacy {
    /// Models the old deployment path that predates the schema marker.
    pub fn initialize_legacy(
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
        validate_upgrade_transition(expected_source_schema, target_schema)?;
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
        validate_migration_transition(expected_source_schema, target_schema)?;
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
            .set(&DataKey::StorageSchemaVersion, &1u32);
        Ok(())
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

fn validate_upgrade_transition(source: u32, target: u32) -> Result<(), Error> {
    if source != 0 || target != 2 {
        return Err(Error::InvalidSchemaTransition);
    }
    Ok(())
}

fn validate_migration_transition(source: u32, target: u32) -> Result<(), Error> {
    if source == 0 && target == 1 {
        Ok(())
    } else {
        Err(Error::InvalidSchemaTransition)
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
