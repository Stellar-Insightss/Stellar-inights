#![cfg(feature = "testutils")]

use escrow::{
    arbiter::Resolution,
    state::{EscrowState, Terms},
    timeout, Error, Escrow, EscrowArgs, EscrowClient,
};
use soroban_sdk::{
    testutils::{Address as _, AuthorizedFunction, Ledger},
    token::{Client as TokenClient, StellarAssetClient},
    Address, Env, IntoVal,
};

const DEPOSIT_TIMEOUT: u64 = 10;
const BENEFICIARY_TIMEOUT: u64 = 20;
const RELEASE_TIMEOUT: u64 = 30;
const DISPUTE_TIMEOUT: u64 = 40;
const AMOUNT: i128 = 100;

struct Fixture {
    env: Env,
    escrow_id: Address,
    token_id: Address,
    depositor: Address,
    beneficiary: Address,
    arbiter: Address,
}

fn fixture() -> Fixture {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(1);

    let token_admin = Address::generate(&env);
    let depositor = Address::generate(&env);
    let beneficiary = Address::generate(&env);
    let arbiter = Address::generate(&env);
    let token_address = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();
    let token_admin_client = StellarAssetClient::new(&env, &token_address);
    token_admin_client.mint(&depositor, &1_000);

    let terms = Terms {
        depositor: depositor.clone(),
        beneficiary: beneficiary.clone(),
        arbiter: arbiter.clone(),
        token: token_address.clone(),
        amount: AMOUNT,
        deposit_timeout: DEPOSIT_TIMEOUT,
        beneficiary_timeout: BENEFICIARY_TIMEOUT,
        release_timeout: RELEASE_TIMEOUT,
        dispute_timeout: DISPUTE_TIMEOUT,
    };
    // Native registration invokes the constructor for setup, but automatically
    // authorizes it; this is not a constructor authorization test.
    let contract_id = env.register(Escrow, EscrowArgs::__constructor(&terms));

    Fixture {
        env,
        escrow_id: contract_id,
        token_id: token_address,
        depositor,
        beneficiary,
        arbiter,
    }
}

fn escrow(fixture: &Fixture) -> EscrowClient<'_> {
    EscrowClient::new(&fixture.env, &fixture.escrow_id)
}

fn token(fixture: &Fixture) -> TokenClient<'_> {
    TokenClient::new(&fixture.env, &fixture.token_id)
}

fn assert_auth(
    fixture: &Fixture,
    expected_address: &Address,
    function_name: &str,
    args: soroban_sdk::Vec<soroban_sdk::Val>,
) {
    let auths = fixture.env.auths();
    assert_eq!(auths.len(), 1);
    assert_eq!(auths[0].0, *expected_address);
    assert_eq!(
        auths[0].1.function,
        AuthorizedFunction::Contract((
            fixture.escrow_id.clone(),
            soroban_sdk::Symbol::new(&fixture.env, function_name),
            args,
        ))
    );
}

#[test]
fn silent_depositor_is_cancelled_before_funding() {
    let fixture = fixture();
    fixture.env.ledger().set_timestamp(1 + DEPOSIT_TIMEOUT);

    assert_eq!(escrow(&fixture).timeout(), EscrowState::Cancelled);
    assert_eq!(escrow(&fixture).get_state(), EscrowState::Cancelled);
    assert_eq!(token(&fixture).balance(&fixture.depositor), 1_000);
}

#[test]
fn silent_beneficiary_is_refunded_after_funding() {
    let fixture = fixture();
    escrow(&fixture).deposit(&AMOUNT);
    fixture
        .env
        .ledger()
        .set_timestamp(1 + DEPOSIT_TIMEOUT + BENEFICIARY_TIMEOUT);

    assert_eq!(escrow(&fixture).timeout(), EscrowState::Refunded);
    assert_eq!(token(&fixture).balance(&fixture.depositor), 1_000);
    assert_eq!(token(&fixture).balance(&fixture.beneficiary), 0);
}

#[test]
fn silent_depositor_after_acceptance_releases_to_beneficiary() {
    let fixture = fixture();
    escrow(&fixture).deposit(&AMOUNT);
    escrow(&fixture).accept();
    fixture
        .env
        .ledger()
        .set_timestamp(1 + DEPOSIT_TIMEOUT + BENEFICIARY_TIMEOUT + RELEASE_TIMEOUT);

    assert_eq!(escrow(&fixture).timeout(), EscrowState::Released);
    assert_eq!(token(&fixture).balance(&fixture.depositor), 900);
    assert_eq!(token(&fixture).balance(&fixture.beneficiary), AMOUNT);
}

#[test]
fn silent_arbiter_gets_deterministic_counterparty_outcome() {
    let fixture = fixture();
    escrow(&fixture).deposit(&AMOUNT);
    escrow(&fixture).accept();
    escrow(&fixture).open_dispute(&fixture.depositor);
    fixture
        .env
        .ledger()
        .set_timestamp(1 + DEPOSIT_TIMEOUT + DISPUTE_TIMEOUT);

    assert_eq!(escrow(&fixture).timeout(), EscrowState::Released);
    assert_eq!(token(&fixture).balance(&fixture.beneficiary), AMOUNT);
    assert_eq!(token(&fixture).balance(&fixture.depositor), 900);
    assert_eq!(token(&fixture).balance(&fixture.arbiter), 0);
}

#[test]
fn silent_arbiter_refunds_when_beneficiary_initiates_dispute() {
    let fixture = fixture();
    escrow(&fixture).deposit(&AMOUNT);
    escrow(&fixture).accept();
    escrow(&fixture).open_dispute(&fixture.beneficiary);
    fixture
        .env
        .ledger()
        .set_timestamp(1 + DEPOSIT_TIMEOUT + DISPUTE_TIMEOUT);

    assert_eq!(escrow(&fixture).timeout(), EscrowState::Refunded);
    assert_eq!(token(&fixture).balance(&fixture.depositor), 1_000);
    assert_eq!(token(&fixture).balance(&fixture.beneficiary), 0);
    assert_eq!(token(&fixture).balance(&fixture.arbiter), 0);
}

#[test]
fn dispute_before_beneficiary_acceptance_is_rejected() {
    let fixture = fixture();
    escrow(&fixture).deposit(&AMOUNT);

    assert_eq!(
        escrow(&fixture).try_open_dispute(&fixture.beneficiary),
        Err(Ok(Error::InvalidTransition))
    );
}

#[test]
fn arbiter_can_only_choose_fixed_outcomes() {
    let fixture = fixture();
    escrow(&fixture).deposit(&AMOUNT);
    escrow(&fixture).accept();
    escrow(&fixture).open_dispute(&fixture.beneficiary);
    escrow(&fixture).resolve_dispute(&Resolution::RefundToDepositor);

    assert_eq!(escrow(&fixture).get_state(), EscrowState::Refunded);
    assert_eq!(token(&fixture).balance(&fixture.depositor), 1_000);
    assert_eq!(token(&fixture).balance(&fixture.beneficiary), 0);
    assert_eq!(token(&fixture).balance(&fixture.arbiter), 0);
}

#[test]
fn unrelated_address_cannot_open_dispute() {
    let fixture = fixture();
    let unrelated = Address::generate(&fixture.env);
    escrow(&fixture).deposit(&AMOUNT);
    escrow(&fixture).accept();
    assert_eq!(
        escrow(&fixture).try_open_dispute(&unrelated),
        Err(Ok(Error::Unauthorized))
    );
}

#[test]
fn timeout_before_deadline_is_rejected() {
    let fixture = fixture();
    assert_eq!(
        escrow(&fixture).try_timeout(),
        Err(Ok(Error::DeadlineNotReached))
    );
}

#[test]
fn invalid_transition_before_funding_is_rejected() {
    let fixture = fixture();
    assert_eq!(
        escrow(&fixture).try_accept(),
        Err(Ok(Error::InvalidTransition))
    );
}

#[test]
fn double_resolution_is_rejected() {
    let fixture = fixture();
    escrow(&fixture).deposit(&AMOUNT);
    escrow(&fixture).accept();
    escrow(&fixture).open_dispute(&fixture.depositor);
    escrow(&fixture).resolve_dispute(&Resolution::ReleaseToBeneficiary);

    assert_eq!(
        escrow(&fixture).try_resolve_dispute(&Resolution::RefundToDepositor),
        Err(Ok(Error::InvalidTransition))
    );
}

#[test]
fn action_after_terminal_state_is_rejected() {
    let fixture = fixture();
    fixture.env.ledger().set_timestamp(1 + DEPOSIT_TIMEOUT);
    escrow(&fixture).timeout();

    assert_eq!(
        escrow(&fixture).try_timeout(),
        Err(Ok(Error::InvalidTransition))
    );
    assert_eq!(
        escrow(&fixture).try_deposit(&AMOUNT),
        Err(Ok(Error::InvalidTransition))
    );
}

#[test]
fn participant_authorizations_are_recorded() {
    let initial_fixture = fixture();

    escrow(&initial_fixture).deposit(&AMOUNT);
    assert_auth(
        &initial_fixture,
        &initial_fixture.depositor,
        "deposit",
        (&AMOUNT,).into_val(&initial_fixture.env),
    );

    escrow(&initial_fixture).accept();
    assert_auth(
        &initial_fixture,
        &initial_fixture.beneficiary,
        "accept",
        ().into_val(&initial_fixture.env),
    );

    let release_fixture = fixture();
    escrow(&release_fixture).deposit(&AMOUNT);
    escrow(&release_fixture).accept();
    escrow(&release_fixture).release();
    assert_auth(
        &release_fixture,
        &release_fixture.depositor,
        "release",
        ().into_val(&release_fixture.env),
    );

    let depositor_dispute_fixture = fixture();
    escrow(&depositor_dispute_fixture).deposit(&AMOUNT);
    escrow(&depositor_dispute_fixture).accept();
    escrow(&depositor_dispute_fixture).open_dispute(&depositor_dispute_fixture.depositor);
    assert_auth(
        &depositor_dispute_fixture,
        &depositor_dispute_fixture.depositor,
        "open_dispute",
        (&depositor_dispute_fixture.depositor,).into_val(&depositor_dispute_fixture.env),
    );

    let beneficiary_dispute_fixture = fixture();
    escrow(&beneficiary_dispute_fixture).deposit(&AMOUNT);
    escrow(&beneficiary_dispute_fixture).accept();
    escrow(&beneficiary_dispute_fixture).open_dispute(&beneficiary_dispute_fixture.beneficiary);
    assert_auth(
        &beneficiary_dispute_fixture,
        &beneficiary_dispute_fixture.beneficiary,
        "open_dispute",
        (&beneficiary_dispute_fixture.beneficiary,).into_val(&beneficiary_dispute_fixture.env),
    );

    escrow(&beneficiary_dispute_fixture).resolve_dispute(&Resolution::ReleaseToBeneficiary);
    assert_auth(
        &beneficiary_dispute_fixture,
        &beneficiary_dispute_fixture.arbiter,
        "resolve_dispute",
        (&Resolution::ReleaseToBeneficiary,).into_val(&beneficiary_dispute_fixture.env),
    );
}

#[test]
fn deadline_overflow_is_rejected() {
    let fixture = fixture();
    fixture.env.ledger().set_timestamp(u64::MAX);

    assert_eq!(
        timeout::from_now(&fixture.env, 1),
        Err(Error::InvalidTimeout)
    );
}
