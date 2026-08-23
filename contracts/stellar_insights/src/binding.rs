use soroban_sdk::{Bytes, BytesN, Env};

/// Canonical bytes signed by the analytics pipeline.
///
/// Including the source reference in the signed message prevents a valid
/// snapshot signature from being reused with a different source record.
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