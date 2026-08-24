#![cfg(test)]

use soroban_sdk::{
    testutils::Address as _,
    token::{Client as TokenClient, StellarAssetClient},
    Address, Bytes, Env,
};

use token_swap::{TokenSwapContract, TokenSwapContractClient};

fn create_token<'a>(
    env: &Env,
    admin: &Address,
) -> (Address, TokenClient<'a>, StellarAssetClient<'a>) {
    let addr = env.register_stellar_asset_contract_v2(admin.clone()).address();
    (
        addr.clone(),
        TokenClient::new(env, &addr),
        StellarAssetClient::new(env, &addr),
    )
}

struct Setup<'a> {
    env: Env,
    contract: TokenSwapContractClient<'a>,
    maker: Address,
    taker: Address,
    token_in: Address,
    token_out: Address,
}

impl<'a> Setup<'a> {
    fn new() -> Self {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(TokenSwapContract, ());
        let contract = TokenSwapContractClient::new(&env, &contract_id);
        let token_admin = Address::generate(&env);
        let maker = Address::generate(&env);
        let taker = Address::generate(&env);
        let (token_in, _, ti_admin) = create_token(&env, &token_admin);
        let (token_out, _, to_admin) = create_token(&env, &token_admin);
        ti_admin.mint(&maker, &1_000_i128);
        to_admin.mint(&taker, &1_000_i128);
        Setup { env, contract, maker, taker, token_in, token_out }
    }
}

fn empty_bytes(env: &Env) -> Bytes {
    Bytes::new(env)
}

#[test]
fn settle_at_exact_minimum() {
    let s = Setup::new();
    let id = s.contract.create_offer(&s.maker, &s.token_in, &100, &s.token_out, &200);
    s.contract.settle_offer(&s.taker, &s.maker, &id, &200);
    assert!(s.contract.get_offer(&s.maker, &id).filled);
    assert_eq!(TokenClient::new(&s.env, &s.token_out).balance(&s.maker), 200);
    assert_eq!(TokenClient::new(&s.env, &s.token_in).balance(&s.taker), 100);
}

#[test]
fn settle_above_minimum_accepted() {
    let s = Setup::new();
    let id = s.contract.create_offer(&s.maker, &s.token_in, &100, &s.token_out, &200);
    s.contract.settle_offer(&s.taker, &s.maker, &id, &250);
    assert!(s.contract.get_offer(&s.maker, &id).filled);
}

#[test]
fn settle_below_minimum_rejected() {
    let s = Setup::new();
    let id = s.contract.create_offer(&s.maker, &s.token_in, &100, &s.token_out, &200);
    assert!(s.contract.try_settle_offer(&s.taker, &s.maker, &id, &199).is_err());
    assert!(!s.contract.get_offer(&s.maker, &id).filled);
}

#[test]
fn double_settle_rejected() {
    let s = Setup::new();
    let id = s.contract.create_offer(&s.maker, &s.token_in, &100, &s.token_out, &200);
    s.contract.settle_offer(&s.taker, &s.maker, &id, &200);
    assert!(s.contract.try_settle_offer(&s.taker, &s.maker, &id, &200).is_err());
}

#[test]
fn cancel_refunds_maker() {
    let s = Setup::new();
    let id = s.contract.create_offer(&s.maker, &s.token_in, &100, &s.token_out, &200);
    let bal_before = TokenClient::new(&s.env, &s.token_in).balance(&s.maker);
    s.contract.cancel_offer(&s.maker, &id);
    assert_eq!(TokenClient::new(&s.env, &s.token_in).balance(&s.maker), bal_before + 100);
    assert!(s.contract.try_settle_offer(&s.taker, &s.maker, &id, &200).is_err());
}

#[test]
fn front_run_price_drop_is_blocked() {
    let s = Setup::new();
    let id = s.contract.create_offer(&s.maker, &s.token_in, &100, &s.token_out, &300);
    // Attacker front-runs, taker can now only offer 250 < 300.
    assert!(s.contract.try_settle_offer(&s.taker, &s.maker, &id, &250).is_err());
    assert!(!s.contract.get_offer(&s.maker, &id).filled);
}
