#![allow(dead_code)]

use ed25519_dalek::{Signer, SigningKey};
use soroban_sdk::{
    contractclient, contracterror, contracttype, Address, Bytes, BytesN, Env, InvokeError,
};

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Snapshot {
    pub epoch: u64,
    pub snapshot_hash: BytesN<32>,
    pub source_data_hash: BytesN<32>,
    pub submitted_at: u64,
    pub submitter: Address,
}

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum TargetError {
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

#[contractclient(name = "ProductionClient")]
#[allow(dead_code)]
pub trait ProductionTarget {
    fn initialize(
        env: Env,
        admin: Address,
        signing_public_key: BytesN<32>,
    ) -> Result<(), TargetError>;
    fn set_upgrade_manager(env: Env, upgrade_manager: Address) -> Result<(), TargetError>;
    fn submit_snapshot(
        env: Env,
        epoch: u64,
        snapshot_hash: BytesN<32>,
        source_data_hash: BytesN<32>,
        signature: BytesN<64>,
        caller: Address,
    ) -> Result<u64, TargetError>;
    fn get_snapshot(env: Env, epoch: u64) -> Result<Snapshot, TargetError>;
    fn latest_epoch(env: Env) -> u64;
    fn governance_upgrade(
        env: Env,
        new_wasm_hash: BytesN<32>,
        expected_source_schema: u32,
        target_schema: u32,
    ) -> Result<(), TargetError>;
    fn migrate_schema(
        env: Env,
        expected_source_schema: u32,
        target_schema: u32,
    ) -> Result<(), TargetError>;
}

#[contractclient(name = "V2Client")]
#[allow(dead_code)]
pub trait V2Target {
    fn set_upgrade_manager(env: Env, upgrade_manager: Address) -> Result<(), TargetError>;
    fn submit_snapshot(
        env: Env,
        epoch: u64,
        snapshot_hash: BytesN<32>,
        source_data_hash: BytesN<32>,
        signature: BytesN<64>,
        caller: Address,
    ) -> Result<u64, TargetError>;
    fn get_snapshot(env: Env, epoch: u64) -> Result<Snapshot, TargetError>;
    fn latest_epoch(env: Env) -> u64;
    fn migrate_schema(
        env: Env,
        expected_source_schema: u32,
        target_schema: u32,
    ) -> Result<(), TargetError>;
    fn governance_upgrade(
        env: Env,
        new_wasm_hash: BytesN<32>,
        expected_source_schema: u32,
        target_schema: u32,
    ) -> Result<(), TargetError>;
}

#[contractclient(name = "LegacyClient")]
#[allow(dead_code)]
pub trait LegacyTarget {
    fn initialize_legacy(
        env: Env,
        admin: Address,
        signing_public_key: BytesN<32>,
    ) -> Result<(), TargetError>;
    fn set_upgrade_manager(env: Env, upgrade_manager: Address) -> Result<(), TargetError>;
    fn submit_snapshot(
        env: Env,
        epoch: u64,
        snapshot_hash: BytesN<32>,
        source_data_hash: BytesN<32>,
        signature: BytesN<64>,
        caller: Address,
    ) -> Result<u64, TargetError>;
    fn get_snapshot(env: Env, epoch: u64) -> Result<Snapshot, TargetError>;
    fn latest_epoch(env: Env) -> u64;
}

pub fn fixture_wasm(name: &str) -> std::vec::Vec<u8> {
    let target_dir = std::env::var_os("CARGO_TARGET_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../target"));
    let path = target_dir
        .join("wasm32v1-none")
        .join("release")
        .join(format!("{name}.wasm"));
    std::fs::read(&path).unwrap_or_else(|error| {
        panic!(
            "missing {name} fixture at {} ({error}); build with --target wasm32v1-none --release",
            path.display()
        )
    })
}

pub fn hash(env: &Env, value: u8) -> BytesN<32> {
    BytesN::from_array(env, &[value; 32])
}

pub fn signed_payload(
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

pub fn signing_key(env: &Env) -> (BytesN<32>, SigningKey) {
    let key = SigningKey::from_bytes(&[7; 32]);
    (
        BytesN::from_array(env, &key.verifying_key().to_bytes()),
        key,
    )
}

pub fn sign(env: &Env, key: &SigningKey, payload: &Bytes) -> BytesN<64> {
    let signature = key.sign(payload.to_buffer::<128>().as_slice());
    BytesN::from_array(env, &signature.to_bytes())
}

pub fn expect_manager_error<T: core::fmt::Debug, E: core::fmt::Debug>(
    result: Result<Result<T, E>, Result<upgrade::Error, InvokeError>>,
    expected: upgrade::Error,
) {
    match result {
        Err(Ok(error)) => assert_eq!(error, expected),
        other => panic!("expected manager error {expected:?}, got {other:?}"),
    }
}

#[allow(dead_code)]
pub fn expect_target_error<T: core::fmt::Debug, E: core::fmt::Debug>(
    result: Result<Result<T, E>, Result<TargetError, InvokeError>>,
    expected: TargetError,
) {
    match result {
        Err(Ok(error)) => assert_eq!(error, expected),
        other => panic!("expected target error {expected:?}, got {other:?}"),
    }
}

pub fn expect_target_panic<T: core::fmt::Debug>(
    result: Result<Result<T, soroban_sdk::Error>, Result<soroban_sdk::Error, InvokeError>>,
    expected: TargetError,
) {
    match result {
        Err(Ok(error)) => assert_eq!(
            error,
            soroban_sdk::Error::from_contract_error(expected as u32)
        ),
        other => panic!("expected target panic error {expected:?}, got {other:?}"),
    }
}
