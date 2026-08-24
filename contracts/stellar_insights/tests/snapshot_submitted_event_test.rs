#![cfg(feature = "testutils")]

use ed25519_dalek::{Signer, SigningKey};
use soroban_sdk::{
    testutils::{Address as _, Events as _, Ledger as _},
    Address, BytesN, Env, Event,
};
use stellar_insights::{
    binding::signed_payload,
    event::{SnapshotSubmitted, SNAPSHOT_SUBMITTED_SCHEMA_VERSION},
    StellarInsights, StellarInsightsClient,
};

#[test]
fn successful_submission_emits_versioned_snapshot_event() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(1_724_512_400);

    let admin = Address::generate(&env);
    let signing_key = SigningKey::from_bytes(&[7; 32]);
    let signing_public_key = BytesN::from_array(&env, &signing_key.verifying_key().to_bytes());
    let contract_id = env.register(StellarInsights, ());
    let client = StellarInsightsClient::new(&env, &contract_id);
    client.initialize(&admin, &signing_public_key);

    let snapshot_hash = BytesN::from_array(&env, &[0xaa; 32]);
    let source_data_hash = BytesN::from_array(&env, &[0xbb; 32]);
    let payload = signed_payload(&env, 42, &snapshot_hash, &source_data_hash);
    let signature = BytesN::from_array(
        &env,
        &signing_key
            .sign(payload.to_buffer::<72>().as_slice())
            .to_bytes(),
    );

    let submitted_at =
        client.submit_snapshot(&42, &snapshot_hash, &source_data_hash, &signature, &admin);

    let expected = SnapshotSubmitted {
        schema_version: SNAPSHOT_SUBMITTED_SCHEMA_VERSION,
        epoch: 42,
        snapshot_hash,
        source_data_hash,
        submitted_at,
        submitter: admin,
    };

    assert_eq!(
        env.events().all(),
        std::vec![expected.to_xdr(&env, &contract_id)]
    );
}
