#![cfg(test)]

use soroban_sdk::{
    testutils::Address as _,
    token::{Client as TokenClient, StellarAssetClient},
    Address, Env,
};

use token_swap::{TokenSwapContract, TokenSwapContractClient};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn create_token<'a>(env: &Env, admin: &Address) -> (Address, TokenClient<'a>, StellarAssetClient<'a>) {
    let addr = env.register_stellar_asset_contract_v2(admin.clone()).address();
    let client = TokenClient::new(env, &addr);
    let admin_client = StellarAssetClient::new(env, &addr);
    (addr, client, admin_client)
}

struct Setup<'a> {
    env: Env,
    contract: TokenSwapContractClient<'a>,
    maker: Address,
    taker: Address,
    token_in: Address,
    token_in_admin: StellarAssetClient<'a>,
    token_out: Address,
    token_out_admin: StellarAssetClient<'a>,
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

        let (token_in, _ti_client, ti_admin) = create_token(&env, &token_admin);
        let (token_out, _to_client, to_admin) = create_token(&env, &token_admin);

        // Mint 1000 units to maker (token_in) and 1000 to taker (token_out).
        ti_admin.mint(&maker, &1_000_i128);
        to_admin.mint(&taker, &1_000_i128);

        Setup {
            env,
            contract,
            maker,
            taker,
            token_in,
            token_in_admin: ti_admin,
            token_out,
            token_out_admin: to_admin,
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// Happy path: settle at exactly min_output.
#[test]
fn settle_at_exact_minimum() {
    let s = Setup::new();
    let id = s.contract.create_offer(
        &s.maker,
        &s.token_in,
        &100_i128,
        &s.token_out,
        &200_i128,
    );

    s.contract.settle_offer(&s.taker, &s.maker, &id, &200_i128);

    let offer = s.contract.get_offer(&s.maker, &id);
    assert!(offer.filled);

    // maker received token_out.
    let maker_out = TokenClient::new(&s.env, &s.token_out).balance(&s.maker);
    assert_eq!(maker_out, 200);

    // taker received token_in.
    let taker_in = TokenClient::new(&s.env, &s.token_in).balance(&s.taker);
    assert_eq!(taker_in, 100);
}

/// Settler offering above minimum is accepted (price-improvement scenario).
#[test]
fn settle_above_minimum_accepted() {
    let s = Setup::new();
    let id = s.contract.create_offer(
        &s.maker,
        &s.token_in,
        &100_i128,
        &s.token_out,
        &200_i128,
    );
    // Taker offers 250 > 200 minimum.
    s.contract.settle_offer(&s.taker, &s.maker, &id, &250_i128);
    assert!(s.contract.get_offer(&s.maker, &id).filled);
}

/// Slippage guard: settler offering less than min_output must fail.
#[test]
fn settle_below_minimum_rejected() {
    let s = Setup::new();
    let id = s.contract.create_offer(
        &s.maker,
        &s.token_in,
        &100_i128,
        &s.token_out,
        &200_i128,
    );
    let result = s.contract.try_settle_offer(&s.taker, &s.maker, &id, &199_i128);
    assert!(result.is_err());
    // Offer must still be open.
    assert!(!s.contract.get_offer(&s.maker, &id).filled);
}

/// Settling a filled offer must fail.
#[test]
fn double_settle_rejected() {
    let s = Setup::new();
    let id = s.contract.create_offer(
        &s.maker,
        &s.token_in,
        &100_i128,
        &s.token_out,
        &200_i128,
    );
    s.contract.settle_offer(&s.taker, &s.maker, &id, &200_i128);
    let result = s.contract.try_settle_offer(&s.taker, &s.maker, &id, &200_i128);
    assert!(result.is_err());
}

/// Maker can cancel and get escrowed tokens back.
#[test]
fn cancel_refunds_maker() {
    let s = Setup::new();
    let id = s.contract.create_offer(
        &s.maker,
        &s.token_in,
        &100_i128,
        &s.token_out,
        &200_i128,
    );

    let bal_before = TokenClient::new(&s.env, &s.token_in).balance(&s.maker);

    s.contract.cancel_offer(&s.maker, &id);

    let bal_after = TokenClient::new(&s.env, &s.token_in).balance(&s.maker);
    assert_eq!(bal_after, bal_before + 100);

    // Settling a cancelled offer must fail.
    let result = s.contract.try_settle_offer(&s.taker, &s.maker, &id, &200_i128);
    assert!(result.is_err());
}

/// Front-running scenario: a price-moving tx that drops the effective rate
/// below min_output is harmless because the swap reverts.
#[test]
fn front_run_price_drop_is_blocked() {
    let s = Setup::new();
    // Maker posts 100 token_in, wants at least 300 token_out.
    let id = s.contract.create_offer(
        &s.maker,
        &s.token_in,
        &100_i128,
        &s.token_out,
        &300_i128,
    );

    // Attacker drives the price down; now taker can only offer 250.
    // (In practice the tx would be re-ordered, but the check happens at execution time.)
    let result = s.contract.try_settle_offer(&s.taker, &s.maker, &id, &250_i128);
    assert!(result.is_err());

    // The offer is untouched.
    assert!(!s.contract.get_offer(&s.maker, &id).filled);
}
