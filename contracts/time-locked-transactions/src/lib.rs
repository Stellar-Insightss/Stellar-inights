#![no_std]

pub mod unlock;

use soroban_sdk::{contract, contracterror, contractimpl, contracttype, token, Address, Env};

const TRANSFER_TTL_THRESHOLD: u32 = 100_000;
const TRANSFER_TTL_EXTEND_TO: u32 = 535_680;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum Error {
    InvalidAmount = 1,
    InvalidUnlockTime = 2,
    TransferNotFound = 3,
    NotUnlockedYet = 4,
    AlreadyExecuted = 5,
    InvalidLedgerProgression = 6,
    TransferIdOverflow = 7,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum TransferState {
    Pending = 0,
    Executed = 1,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScheduledTransfer {
    pub id: u64,
    pub sender: Address,
    pub recipient: Address,
    pub token: Address,
    pub amount: i128,
    pub unlock_time: u64,
    pub created_at: u64,
    pub created_ledger: u32,
    pub state: TransferState,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DataKey {
    NextTransferId,
    Transfer(u64),
}

#[contract]
pub struct TimeLockedTransactionsContract;

#[contractimpl]
impl TimeLockedTransactionsContract {
    /// Escrow tokens now for release to the fixed recipient at `unlock_time`.
    pub fn schedule_transfer(
        env: Env,
        sender: Address,
        recipient: Address,
        token: Address,
        amount: i128,
        unlock_time: u64,
    ) -> Result<u64, Error> {
        if amount <= 0 {
            return Err(Error::InvalidAmount);
        }

        let created_at = env.ledger().timestamp();
        unlock::validate_schedule(created_at, unlock_time)?;
        sender.require_auth();

        let id = env
            .storage()
            .instance()
            .get::<DataKey, u64>(&DataKey::NextTransferId)
            .unwrap_or(1);
        let next_id = id.checked_add(1).ok_or(Error::TransferIdOverflow)?;
        let transfer = ScheduledTransfer {
            id,
            sender: sender.clone(),
            recipient,
            token: token.clone(),
            amount,
            unlock_time,
            created_at,
            created_ledger: env.ledger().sequence(),
            state: TransferState::Pending,
        };

        env.storage()
            .persistent()
            .set(&DataKey::Transfer(id), &transfer);
        env.storage().persistent().extend_ttl(
            &DataKey::Transfer(id),
            TRANSFER_TTL_THRESHOLD,
            TRANSFER_TTL_EXTEND_TO,
        );
        env.storage()
            .instance()
            .set(&DataKey::NextTransferId, &next_id);

        let contract_address = env.current_contract_address();
        token::Client::new(&env, &token).transfer(&sender, &contract_address, &amount);

        Ok(id)
    }

    /// Permissionlessly execute a pending transfer once its absolute unlock
    /// timestamp has been reached. The stored recipient and amount are used;
    /// no caller can supply settlement parameters.
    pub fn execute_transfer(env: Env, transfer_id: u64) -> Result<(), Error> {
        let mut transfer = load_transfer(&env, transfer_id)?;
        if transfer.state != TransferState::Pending {
            return Err(Error::AlreadyExecuted);
        }

        unlock::validate_progression(
            transfer.created_at,
            transfer.created_ledger,
            env.ledger().timestamp(),
            env.ledger().sequence(),
        )?;
        unlock::ensure_reached(env.ledger().timestamp(), transfer.unlock_time)?;

        // Commit the terminal state before the external token call. If the
        // token transfer fails, Soroban rolls back this invocation and the
        // pending record remains retryable.
        transfer.state = TransferState::Executed;
        env.storage()
            .persistent()
            .set(&DataKey::Transfer(transfer_id), &transfer);
        env.storage().persistent().extend_ttl(
            &DataKey::Transfer(transfer_id),
            TRANSFER_TTL_THRESHOLD,
            TRANSFER_TTL_EXTEND_TO,
        );

        let contract_address = env.current_contract_address();
        token::Client::new(&env, &transfer.token).transfer(
            &contract_address,
            &transfer.recipient,
            &transfer.amount,
        );

        Ok(())
    }

    pub fn get_transfer(env: Env, transfer_id: u64) -> Result<ScheduledTransfer, Error> {
        load_transfer(&env, transfer_id)
    }
}

fn load_transfer(env: &Env, transfer_id: u64) -> Result<ScheduledTransfer, Error> {
    env.storage()
        .persistent()
        .get(&DataKey::Transfer(transfer_id))
        .ok_or(Error::TransferNotFound)
}
