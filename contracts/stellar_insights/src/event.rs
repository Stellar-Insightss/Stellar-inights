use soroban_sdk::{contractevent, Address, BytesN};

/// Schema version for [`SnapshotSubmitted`] events emitted by this contract.
pub const SNAPSHOT_SUBMITTED_SCHEMA_VERSION: u32 = 1;

/// Emitted after a snapshot has been persisted successfully.
///
/// The fixed Soroban topic is `snapshot_submitted`. All fields are encoded in
/// the event data map so downstream decoders receive the complete versioned
/// payload in one value.
#[contractevent(topics = ["snapshot_submitted"])]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SnapshotSubmitted {
    pub schema_version: u32,
    pub epoch: u64,
    pub snapshot_hash: BytesN<32>,
    pub source_data_hash: BytesN<32>,
    pub submitted_at: u64,
    pub submitter: Address,
}
