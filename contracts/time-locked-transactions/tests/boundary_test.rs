#![cfg(feature = "testutils")]

use soroban_sdk::{
    testutils::{Address as _, AuthorizedFunction, Ledger as _},
    token::{Client as TokenClient, StellarAssetClient},
    Address, Env, IntoVal, Symbol,
};
use time_locked_transactions::{
    Error, ScheduledTransfer, TimeLockedTransactionsContract, TimeLockedTransactionsContractClient,
    TransferState,
};

const CREATED_AT: u64 = 1_000;
const CREATED_LEDGER: u32 = 100;
const UNLOCK_TIME: u64 = 1_100;
const AMOUNT: i128 = 100;

struct Fixture {
    env: Env,
    contract_id: Address,
    token_id: Address,
    recipient: Address,
    transfer_id: u64,
}

fn fixture() -> Fixture {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_sequence_number(CREATED_LEDGER);
    env.ledger().set_timestamp(CREATED_AT);

    let token_admin = Address::generate(&env);
    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);
    let token_id = env
        .register_stellar_asset_contract_v2(token_admin)
        .address();
    StellarAssetClient::new(&env, &token_id).mint(&sender, &AMOUNT);

    let contract_id = env.register(TimeLockedTransactionsContract, ());
    let client = TimeLockedTransactionsContractClient::new(&env, &contract_id);
    let transfer_id =
        client.schedule_transfer(&sender, &recipient, &token_id, &AMOUNT, &UNLOCK_TIME);

    Fixture {
        env,
        contract_id,
        token_id,
        recipient,
        transfer_id,
    }
}

fn client(fixture: &Fixture) -> TimeLockedTransactionsContractClient<'_> {
    TimeLockedTransactionsContractClient::new(&fixture.env, &fixture.contract_id)
}

fn token_balance(fixture: &Fixture, address: &Address) -> i128 {
    TokenClient::new(&fixture.env, &fixture.token_id).balance(address)
}

fn token_balance_for(env: &Env, token_id: &Address, address: &Address) -> i128 {
    TokenClient::new(env, token_id).balance(address)
}

fn assert_pending_and_escrowed(fixture: &Fixture) {
    let transfer: ScheduledTransfer = client(fixture).get_transfer(&fixture.transfer_id);
    assert_eq!(transfer.state, TransferState::Pending);
    assert_eq!(token_balance(fixture, &fixture.recipient), 0);
    assert_eq!(token_balance(fixture, &fixture.contract_id), AMOUNT);
}

#[test]
fn scheduling_requires_the_sender_authorization() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_sequence_number(CREATED_LEDGER);
    env.ledger().set_timestamp(CREATED_AT);

    let token_admin = Address::generate(&env);
    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);
    let token_id = env
        .register_stellar_asset_contract_v2(token_admin)
        .address();
    StellarAssetClient::new(&env, &token_id).mint(&sender, &AMOUNT);
    let contract_id = env.register(TimeLockedTransactionsContract, ());
    let client = TimeLockedTransactionsContractClient::new(&env, &contract_id);

    client.schedule_transfer(&sender, &recipient, &token_id, &AMOUNT, &UNLOCK_TIME);

    let auths = env.auths();
    assert_eq!(auths.len(), 1);
    assert_eq!(auths[0].0, sender);
    assert_eq!(
        auths[0].1.function,
        AuthorizedFunction::Contract((
            contract_id,
            Symbol::new(&env, "schedule_transfer"),
            (&sender, &recipient, &token_id, &AMOUNT, &UNLOCK_TIME).into_val(&env),
        ))
    );
}

#[test]
fn failed_token_transfer_does_not_consume_id_or_persist_schedule() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_sequence_number(CREATED_LEDGER);
    env.ledger().set_timestamp(CREATED_AT);

    let token_admin = Address::generate(&env);
    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);
    let token_id = env
        .register_stellar_asset_contract_v2(token_admin)
        .address();
    let contract_id = env.register(TimeLockedTransactionsContract, ());
    let client = TimeLockedTransactionsContractClient::new(&env, &contract_id);

    // The sender has no tokens, so the external escrow transfer fails after
    // the pending record and next-ID update have been staged.
    assert!(client
        .try_schedule_transfer(&sender, &recipient, &token_id, &AMOUNT, &UNLOCK_TIME)
        .is_err());
    assert_eq!(
        client.try_get_transfer(&1),
        Err(Ok(Error::TransferNotFound))
    );

    StellarAssetClient::new(&env, &token_id).mint(&sender, &AMOUNT);
    let id = client.schedule_transfer(&sender, &recipient, &token_id, &AMOUNT, &UNLOCK_TIME);

    assert_eq!(id, 1);
    assert_eq!(client.get_transfer(&id).state, TransferState::Pending);
    assert_eq!(token_balance_for(&env, &token_id, &contract_id), AMOUNT);
}

#[test]
fn one_ledger_before_unlock_stays_pending() {
    let fixture = fixture();
    fixture
        .env
        .ledger()
        .set_sequence_number(CREATED_LEDGER + 99);
    fixture.env.ledger().set_timestamp(UNLOCK_TIME - 1);

    assert_eq!(
        client(&fixture).try_execute_transfer(&fixture.transfer_id),
        Err(Ok(Error::NotUnlockedYet))
    );
    assert_pending_and_escrowed(&fixture);
}

#[test]
fn exactly_at_unlock_succeeds() {
    let fixture = fixture();
    fixture
        .env
        .ledger()
        .set_sequence_number(CREATED_LEDGER + 100);
    fixture.env.ledger().set_timestamp(UNLOCK_TIME);

    assert_eq!(
        client(&fixture).try_execute_transfer(&fixture.transfer_id),
        Ok(Ok(()))
    );
    let transfer = client(&fixture).get_transfer(&fixture.transfer_id);
    assert_eq!(transfer.state, TransferState::Executed);
    assert_eq!(token_balance(&fixture, &fixture.recipient), AMOUNT);
    assert_eq!(token_balance(&fixture, &fixture.contract_id), 0);
}

#[test]
fn one_ledger_after_unlock_succeeds() {
    let fixture = fixture();
    fixture
        .env
        .ledger()
        .set_sequence_number(CREATED_LEDGER + 101);
    fixture.env.ledger().set_timestamp(UNLOCK_TIME + 1);

    client(&fixture).execute_transfer(&fixture.transfer_id);

    let transfer = client(&fixture).get_transfer(&fixture.transfer_id);
    assert_eq!(transfer.state, TransferState::Executed);
    assert_eq!(token_balance(&fixture, &fixture.recipient), AMOUNT);
    assert_eq!(token_balance(&fixture, &fixture.contract_id), 0);
}

#[test]
fn timestamp_regression_is_rejected() {
    let fixture = fixture();
    fixture.env.ledger().set_sequence_number(CREATED_LEDGER + 1);
    fixture.env.ledger().set_timestamp(CREATED_AT - 1);

    assert_eq!(
        client(&fixture).try_execute_transfer(&fixture.transfer_id),
        Err(Ok(Error::InvalidLedgerProgression))
    );
    assert_pending_and_escrowed(&fixture);
}

#[test]
fn sequence_regression_is_rejected() {
    let fixture = fixture();
    fixture.env.ledger().set_sequence_number(CREATED_LEDGER - 1);
    fixture.env.ledger().set_timestamp(CREATED_AT + 1);

    assert_eq!(
        client(&fixture).try_execute_transfer(&fixture.transfer_id),
        Err(Ok(Error::InvalidLedgerProgression))
    );
    assert_pending_and_escrowed(&fixture);
}

#[test]
fn timestamp_change_without_new_ledger_is_rejected() {
    let fixture = fixture();
    fixture.env.ledger().set_timestamp(CREATED_AT + 1);

    assert_eq!(
        client(&fixture).try_execute_transfer(&fixture.transfer_id),
        Err(Ok(Error::InvalidLedgerProgression))
    );
    assert_pending_and_escrowed(&fixture);
}

#[test]
fn elapsed_ledgers_cannot_exceed_elapsed_seconds() {
    let fixture = fixture();
    fixture.env.ledger().set_sequence_number(CREATED_LEDGER + 2);
    fixture.env.ledger().set_timestamp(CREATED_AT + 1);

    assert_eq!(
        client(&fixture).try_execute_transfer(&fixture.transfer_id),
        Err(Ok(Error::InvalidLedgerProgression))
    );
    assert_pending_and_escrowed(&fixture);
}

#[test]
fn large_timestamp_jump_is_allowed() {
    let fixture = fixture();
    fixture.env.ledger().set_sequence_number(CREATED_LEDGER + 1);
    fixture.env.ledger().set_timestamp(UNLOCK_TIME + 1_000_000);

    client(&fixture).execute_transfer(&fixture.transfer_id);

    assert_eq!(
        client(&fixture).get_transfer(&fixture.transfer_id).state,
        TransferState::Executed
    );
    assert_eq!(token_balance(&fixture, &fixture.recipient), AMOUNT);
}

#[test]
fn invalid_schedules_and_double_execution_are_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_sequence_number(CREATED_LEDGER);
    env.ledger().set_timestamp(CREATED_AT);

    let token_admin = Address::generate(&env);
    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);
    let token_id = env
        .register_stellar_asset_contract_v2(token_admin)
        .address();
    StellarAssetClient::new(&env, &token_id).mint(&sender, &AMOUNT);
    let contract_id = env.register(TimeLockedTransactionsContract, ());
    let client = TimeLockedTransactionsContractClient::new(&env, &contract_id);

    assert_eq!(
        client.try_schedule_transfer(&sender, &recipient, &token_id, &0, &UNLOCK_TIME,),
        Err(Ok(Error::InvalidAmount))
    );
    assert_eq!(
        client.try_schedule_transfer(&sender, &recipient, &token_id, &AMOUNT, &CREATED_AT,),
        Err(Ok(Error::InvalidUnlockTime))
    );

    let id = client.schedule_transfer(&sender, &recipient, &token_id, &AMOUNT, &UNLOCK_TIME);
    env.ledger().set_sequence_number(CREATED_LEDGER + 100);
    env.ledger().set_timestamp(UNLOCK_TIME);
    client.execute_transfer(&id);
    assert_eq!(
        client.try_execute_transfer(&id),
        Err(Ok(Error::AlreadyExecuted))
    );
}

#[test]
fn missing_transfer_is_rejected() {
    let env = Env::default();
    let contract_id = env.register(TimeLockedTransactionsContract, ());
    let client = TimeLockedTransactionsContractClient::new(&env, &contract_id);

    assert_eq!(
        client.try_get_transfer(&1),
        Err(Ok(Error::TransferNotFound))
    );
}
