#![cfg(feature = "testutils")]

use soroban_sdk::{testutils::Address as _, Address, BytesN, Env};
use stellar_insights::{binding::signed_payload, StellarInsights, StellarInsightsClient};

#[test]
#[should_panic]
fn tampered_content_with_valid_signature_is_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let signing_key = BytesN::from_array(&env, &[7; 32]);
    let contract_id = env.register_contract(None, StellarInsights);
    let client = StellarInsightsClient::new(&env, &contract_id);
    client.initialize(&admin, &signing_key);
    let original_hash = BytesN::from_array(&env, &[1; 32]);
    let tampered_hash = BytesN::from_array(&env, &[9; 32]);
    let source_hash = BytesN::from_array(&env, &[2; 32]);
    let signature = env.crypto().ed25519_sign(
        &signing_key,
        &signed_payload(&env, 1, &original_hash, &source_hash),
    );

    let _ = client.submit_snapshot(&1, &tampered_hash, &source_hash, &signature, &admin);
}