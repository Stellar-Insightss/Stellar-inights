//! Deployment requirement: any contract change that alters an emitted event's
//! shape must bump `schema_version` and ship the matching parser in `parsers/`
//! BEFORE that contract upgrade is deployed. Contract and indexer deployments
//! run on independent schedules, and nothing else enforces this ordering.

use serde::Deserialize;
use serde_json::Value;
use thiserror::Error;

use super::parsers;

pub const SNAPSHOT_SUBMITTED_TOPIC: &str = "snapshot_submitted";

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum NormalizedEvent {
    SnapshotSubmitted(NormalizedSnapshotSubmitted),
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct NormalizedSnapshotSubmitted {
    pub epoch: u64,
    pub snapshot_hash: String,
    pub source_data_hash: String,
    pub submitted_at: u64,
    pub submitter: String,
}

#[derive(Debug, Error)]
pub enum DispatchError {
    #[error("event payload is missing schema_version")]
    MissingSchemaVersion,
    #[error("event payload has a non-u32 schema_version")]
    InvalidSchemaVersion,
    #[error("unsupported event schema_version {0}")]
    UnsupportedSchemaVersion(u32),
    #[error("unsupported event topic {0}")]
    UnsupportedTopic(String),
    #[error("invalid {topic} payload for schema_version {schema_version}: {source}")]
    InvalidPayload {
        topic: &'static str,
        schema_version: u32,
        #[source]
        source: serde_json::Error,
    },
    #[error("parser version mismatch: expected {expected}, got {actual}")]
    ParserVersionMismatch { expected: u32, actual: u32 },
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum SchemaVersion {
    V1,
    /// Synthetic version used by the replay test to prove independent parser
    /// registration before a future contract schema is deployed.
    V2,
}

impl TryFrom<u32> for SchemaVersion {
    type Error = DispatchError;

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        if value == 1 {
            Ok(Self::V1)
        } else if value == 2 {
            Ok(Self::V2)
        } else {
            Err(DispatchError::UnsupportedSchemaVersion(value))
        }
    }
}

/// Dispatch a decoded Soroban event data map by topic and schema version.
pub fn dispatch(topic: &str, payload: Value) -> Result<NormalizedEvent, DispatchError> {
    if topic != SNAPSHOT_SUBMITTED_TOPIC {
        return Err(DispatchError::UnsupportedTopic(topic.to_owned()));
    }

    let schema_version = payload
        .get("schema_version")
        .ok_or(DispatchError::MissingSchemaVersion)?
        .as_u64()
        .and_then(|value| u32::try_from(value).ok())
        .ok_or(DispatchError::InvalidSchemaVersion)?;

    match SchemaVersion::try_from(schema_version)? {
        SchemaVersion::V1 => parsers::v1::parse_snapshot_submitted(payload),
        SchemaVersion::V2 => parse_snapshot_submitted_v2(payload),
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SnapshotSubmittedV2 {
    schema_version: u32,
    epoch: u64,
    snapshot_hash: String,
    source_data_hash: String,
    submitted_at: u64,
    submitter: String,
    snapshot_size_bytes: u64,
}

/// Synthetic v2 parser registered solely to exercise version routing. A real
/// v2 rollout must move its finalized schema into `parsers/v2.rs` before the
/// matching contract upgrade is deployed.
fn parse_snapshot_submitted_v2(payload: Value) -> Result<NormalizedEvent, DispatchError> {
    let event: SnapshotSubmittedV2 =
        serde_json::from_value(payload).map_err(|source| DispatchError::InvalidPayload {
            topic: SNAPSHOT_SUBMITTED_TOPIC,
            schema_version: 2,
            source,
        })?;

    if event.schema_version != 2 {
        return Err(DispatchError::ParserVersionMismatch {
            expected: 2,
            actual: event.schema_version,
        });
    }

    // A future parser may use this field. Reading it here ensures the fixture
    // is genuinely v2-shaped while retaining the v1 logical normalization.
    let _snapshot_size_bytes = event.snapshot_size_bytes;

    Ok(NormalizedEvent::SnapshotSubmitted(
        NormalizedSnapshotSubmitted {
            epoch: event.epoch,
            snapshot_hash: event.snapshot_hash,
            source_data_hash: event.source_data_hash,
            submitted_at: event.submitted_at,
            submitter: event.submitter,
        },
    ))
}
