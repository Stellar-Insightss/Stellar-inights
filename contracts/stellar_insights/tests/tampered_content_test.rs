#![cfg(feature = "testutils")]

use ed25519_dalek::{Signer, SigningKey};
use soroban_sdk::{testutils::Address as _, Address, BytesN, Env};
use stellar_insights::{binding::signed_payload, StellarInsights, StellarInsightsClient};

#[test]
#[should_panic]
fn tampered_content_with_valid_signature_is_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let signing_key = SigningKey::from_bytes(&[7; 32]);
    let signing_public_key = BytesN::from_array(&env, &signing_key.verifying_key().to_bytes());
    let contract_id = env.register(StellarInsights, ());
    let client = StellarInsightsClient::new(&env, &contract_id);
    client.initialize(&admin, &signing_public_key);
    let original_hash = BytesN::from_array(&env, &[1; 32]);
    let tampered_hash = BytesN::from_array(&env, &[9; 32]);
    let source_hash = BytesN::from_array(&env, &[2; 32]);
    let payload = signed_payload(&env, 1, &original_hash, &source_hash);
    let signature = BytesN::from_array(
        &env,
        &signing_key
            .sign(payload.to_buffer::<72>().as_slice())
            .to_bytes(),
    );

    let _ = client.submit_snapshot(&1, &tampered_hash, &source_hash, &signature, &admin);
}
