use soroban_sdk::{BytesN, Env};

use crate::{binding::signed_payload, DataKey, Error};

/// Verify that the snapshot and its source-data reference were signed by the
/// configured analytics pipeline key.
pub fn verify_signature(
    env: &Env,
    epoch: u64,
    snapshot_hash: &BytesN<32>,
    source_data_hash: &BytesN<32>,
    signature: &BytesN<64>,
) -> Result<(), Error> {
    let public_key: BytesN<32> = env
        .storage()
        .instance()
        .get(&DataKey::SigningPublicKey)
        .ok_or(Error::SigningKeyNotSet)?;
    let payload = signed_payload(env, epoch, snapshot_hash, source_data_hash);

    // Soroban's host verifies Ed25519 signatures and aborts the invocation on
    // failure. A successful return is therefore the acceptance condition.
    env.crypto().ed25519_verify(&public_key, signature, &payload);
    Ok(())
}