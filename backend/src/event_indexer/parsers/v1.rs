use serde::Deserialize;
use serde_json::Value;

use crate::event_indexer::dispatch::{
    DispatchError, NormalizedEvent, NormalizedSnapshotSubmitted, SNAPSHOT_SUBMITTED_TOPIC,
};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SnapshotSubmittedV1 {
    schema_version: u32,
    epoch: u64,
    snapshot_hash: String,
    source_data_hash: String,
    submitted_at: u64,
    submitter: String,
}

pub fn parse_snapshot_submitted(payload: Value) -> Result<NormalizedEvent, DispatchError> {
    let event: SnapshotSubmittedV1 =
        serde_json::from_value(payload).map_err(|source| DispatchError::InvalidPayload {
            topic: SNAPSHOT_SUBMITTED_TOPIC,
            schema_version: 1,
            source,
        })?;

    if event.schema_version != 1 {
        return Err(DispatchError::ParserVersionMismatch {
            expected: 1,
            actual: event.schema_version,
        });
    }

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
