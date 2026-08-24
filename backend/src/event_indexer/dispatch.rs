//! Deployment requirement: any contract change that alters an emitted event's
//! shape must bump `schema_version` and ship the matching parser in `parsers/`
//! BEFORE that contract upgrade is deployed. Contract and indexer deployments
//! run on independent schedules, and nothing else enforces this ordering.

use stellar_xdr::ContractEvent;
use thiserror::Error;

use super::{parsers, xdr};

pub const SNAPSHOT_SUBMITTED_TOPIC: &str = "snapshot_submitted";

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct NormalizedContractEvent {
    pub contract_id: String,
    pub event: NormalizedEvent,
}

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
    #[error("only contract events can be indexed, got {0}")]
    UnsupportedEventType(String),
    #[error("contract event is missing its contract ID")]
    MissingContractId,
    #[error("contract event must have exactly one fixed topic, got {0}")]
    InvalidTopicCount(usize),
    #[error("contract event topic must be an XDR symbol")]
    InvalidTopicType,
    #[error("contract event topic is not valid UTF-8")]
    InvalidTopicEncoding,
    #[error("unsupported event topic {0}")]
    UnsupportedTopic(String),
    #[error("contract event data must be a non-empty XDR map")]
    InvalidEventData,
    #[error("event payload key must be an XDR symbol")]
    InvalidFieldNameType,
    #[error("event payload field name is not valid UTF-8")]
    InvalidFieldNameEncoding,
    #[error("event payload contains duplicate field {0}")]
    DuplicateField(String),
    #[error("event payload contains unexpected field {field} for schema_version {schema_version}")]
    UnexpectedField { field: String, schema_version: u32 },
    #[error("event payload is missing field {0}")]
    MissingField(&'static str),
    #[error("event payload field {field} must be {expected}")]
    InvalidFieldType {
        field: &'static str,
        expected: &'static str,
    },
    #[error("unsupported event schema_version {0}")]
    UnsupportedSchemaVersion(u32),
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

/// Decode and dispatch a protocol-native Soroban contract event.
pub fn dispatch(event: &ContractEvent) -> Result<NormalizedContractEvent, DispatchError> {
    let envelope = xdr::decode_event(event)?;

    if envelope.topic != SNAPSHOT_SUBMITTED_TOPIC {
        return Err(DispatchError::UnsupportedTopic(envelope.topic));
    }

    let schema_version = xdr::u32_field(envelope.data, "schema_version")?;
    let event = match SchemaVersion::try_from(schema_version)? {
        SchemaVersion::V1 => parsers::v1::parse_snapshot_submitted(envelope.data),
        SchemaVersion::V2 => parsers::v2::parse_snapshot_submitted(envelope.data),
    }?;

    Ok(NormalizedContractEvent {
        contract_id: envelope.contract_id,
        event,
    })
}
