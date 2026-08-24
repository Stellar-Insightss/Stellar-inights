//! Synthetic v2 parser used to prove mixed-version replay. A real v2 contract
//! rollout must replace this fixture schema with its finalized event shape.

use stellar_xdr::ScMap;

use crate::event_indexer::{
    dispatch::{DispatchError, NormalizedEvent, NormalizedSnapshotSubmitted},
    xdr,
};

const SCHEMA_VERSION: u32 = 2;
const FIELDS: &[&str] = &[
    "epoch",
    "schema_version",
    "snapshot_hash",
    "snapshot_size_bytes",
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

    // Parse the added field so this parser proves a genuinely different XDR
    // schema while retaining the same logical analytics representation.
    let _snapshot_size_bytes = xdr::u64_field(data, "snapshot_size_bytes")?;

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
