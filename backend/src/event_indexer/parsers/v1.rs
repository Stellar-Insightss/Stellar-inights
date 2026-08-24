use stellar_xdr::ScMap;

use crate::event_indexer::{
    dispatch::{DispatchError, NormalizedEvent, NormalizedSnapshotSubmitted},
    xdr,
};

const SCHEMA_VERSION: u32 = 1;
const FIELDS: &[&str] = &[
    "epoch",
    "schema_version",
    "snapshot_hash",
    "source_data_hash",
    "submitted_at",
    "submitter",
];

pub fn parse_snapshot_submitted(data: &ScMap) -> Result<NormalizedEvent, DispatchError> {
    xdr::validate_fields(data, FIELDS, SCHEMA_VERSION)?;

    let schema_version = xdr::u32_field(data, "schema_version")?;
    if schema_version != SCHEMA_VERSION {
        return Err(DispatchError::ParserVersionMismatch {
            expected: SCHEMA_VERSION,
            actual: schema_version,
        });
    }

    Ok(NormalizedEvent::SnapshotSubmitted(
        NormalizedSnapshotSubmitted {
            epoch: xdr::u64_field(data, "epoch")?,
            snapshot_hash: xdr::bytes32_field(data, "snapshot_hash")?,
            source_data_hash: xdr::bytes32_field(data, "source_data_hash")?,
            submitted_at: xdr::u64_field(data, "submitted_at")?,
            submitter: xdr::address_field(data, "submitter")?,
        },
    ))
}
