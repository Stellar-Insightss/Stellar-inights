#![no_std]

pub mod arbiter;
pub mod state;
pub mod timeout;

use arbiter::Resolution;
use soroban_sdk::{contract, contracterror, contractimpl, Address, Env};
use state::{DataKey, EscrowState, Participant, Terms};

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum Error {
    NotInitialized = 1,
    Unauthorized = 2,
    InvalidAmount = 3,
    InvalidTimeout = 4,
    InvalidParticipants = 5,
    AmountMismatch = 6,
    DeadlineNotReached = 7,
    DeadlinePassed = 8,
    InvalidTransition = 9,
}

#[contract]
pub struct Escrow;

#[contractimpl]
impl Escrow {
    /// Commit immutable parties, token, amount, and finite phase durations
    /// atomically as part of contract deployment. The depositor authorization
    /// binds the configured depositor to the deployment-time terms; native
    /// test registration automatically authorizes this constructor.
    pub fn __constructor(env: Env, terms: Terms) {
        if terms.amount <= 0 {
            soroban_sdk::panic_with_error!(&env, Error::InvalidAmount);
        }
        if terms.depositor == terms.beneficiary
            || terms.depositor == terms.arbiter
            || terms.beneficiary == terms.arbiter
        {
            soroban_sdk::panic_with_error!(&env, Error::InvalidParticipants);
        }
        for duration in [
            terms.deposit_timeout,
            terms.beneficiary_timeout,
            terms.release_timeout,
            terms.dispute_timeout,
        ] {
            if let Err(error) = timeout::validate_duration(duration) {
                soroban_sdk::panic_with_error!(&env, error);
            }
        }

        terms.depositor.require_auth();
        let initial_deadline = match timeout::from_now(&env, terms.deposit_timeout) {
            Ok(deadline) => deadline,
            Err(error) => soroban_sdk::panic_with_error!(&env, error),
        };
        env.storage().instance().set(&DataKey::Terms, &terms);
        env.storage().instance().set(
            &DataKey::State,
            &EscrowState::AwaitingDeposit(initial_deadline),
        );
    }

    /// The depositor funds the escrow with exactly the committed amount.
    ///
    /// The state is committed before the external token call to make a
    /// re-entrant token unable to deposit twice. Soroban rolls back the whole
    /// invocation if the token transfer fails.
    pub fn deposit(env: Env, amount: i128) -> Result<(), Error> {
        let terms = read_terms(&env)?;
        let state = read_state(&env)?;
        let deadline = match state {
            EscrowState::AwaitingDeposit(deadline) => deadline,
            _ => return Err(Error::InvalidTransition),
        };
        timeout::ensure_before(&env, deadline)?;
        if amount != terms.amount {
            return Err(Error::AmountMismatch);
        }

        terms.depositor.require_auth();
        let next_deadline = timeout::from_now(&env, terms.beneficiary_timeout)?;
        env.storage().instance().set(
            &DataKey::State,
            &EscrowState::AwaitingBeneficiary(next_deadline),
        );
        let contract_address = env.current_contract_address();
        soroban_sdk::token::Client::new(&env, &terms.token).transfer(
            &terms.depositor,
            &contract_address,
            &terms.amount,
        );
        Ok(())
    }

    /// The beneficiary accepts the funded escrow. This only advances to the
    /// release grace period; it does not itself transfer funds.
    pub fn accept(env: Env) -> Result<(), Error> {
        let terms = read_terms(&env)?;
        let state = read_state(&env)?;
        let deadline = match state {
            EscrowState::AwaitingBeneficiary(deadline) => deadline,
            _ => return Err(Error::InvalidTransition),
        };
        timeout::ensure_before(&env, deadline)?;
        terms.beneficiary.require_auth();
        let next_deadline = timeout::from_now(&env, terms.release_timeout)?;
        env.storage().instance().set(
            &DataKey::State,
            &EscrowState::AwaitingRelease(next_deadline),
        );
        Ok(())
    }

    /// The depositor releases the exact escrow amount to the fixed
    /// beneficiary during the release grace period.
    pub fn release(env: Env) -> Result<(), Error> {
        let terms = read_terms(&env)?;
        let state = read_state(&env)?;
        let deadline = match state {
            EscrowState::AwaitingRelease(deadline) => deadline,
            _ => return Err(Error::InvalidTransition),
        };
        timeout::ensure_before(&env, deadline)?;
        terms.depositor.require_auth();
        settle(
            &env,
            &terms,
            EscrowState::Released,
            Resolution::ReleaseToBeneficiary,
        );
        Ok(())
    }

    /// Either escrow party may open one dispute during the release grace
    /// period. The caller is authenticated and must equal the configured
    /// depositor or beneficiary; the arbiter is intentionally not an escrow
    /// party. Before beneficiary acceptance, the deterministic refund timeout
    /// remains the only path out of that phase.
    pub fn open_dispute(env: Env, caller: Address) -> Result<(), Error> {
        let terms = read_terms(&env)?;
        let state = read_state(&env)?;
        let phase_deadline = match state {
            EscrowState::AwaitingRelease(deadline) => deadline,
            _ => return Err(Error::InvalidTransition),
        };
        timeout::ensure_before(&env, phase_deadline)?;

        let initiator = if caller == terms.depositor {
            Participant::Depositor
        } else if caller == terms.beneficiary {
            Participant::Beneficiary
        } else {
            return Err(Error::Unauthorized);
        };
        caller.require_auth();
        let dispute_deadline = timeout::from_now(&env, terms.dispute_timeout)?;
        env.storage().instance().set(
            &DataKey::State,
            &EscrowState::Disputed(dispute_deadline, initiator),
        );
        Ok(())
    }

    /// Only the configured arbiter can choose one predefined resolution while
    /// the dispute deadline is live. The recipient and amount come only from
    /// immutable terms and the `Resolution` enum.
    pub fn resolve_dispute(env: Env, resolution: Resolution) -> Result<(), Error> {
        let terms = read_terms(&env)?;
        let state = read_state(&env)?;
        let deadline = match state {
            EscrowState::Disputed(deadline, ..) => deadline,
            _ => return Err(Error::InvalidTransition),
        };
        timeout::ensure_before(&env, deadline)?;
        terms.arbiter.require_auth();
        let terminal_state = match resolution {
            Resolution::ReleaseToBeneficiary => EscrowState::Released,
            Resolution::RefundToDepositor => EscrowState::Refunded,
        };
        settle(&env, &terms, terminal_state, resolution);
        Ok(())
    }

    /// Permissionless liveness transition. At or after the current state's
    /// deadline, the fallback is completely determined by state and the
    /// recorded dispute initiator; callers cannot redirect the funds.
    pub fn timeout(env: Env) -> Result<EscrowState, Error> {
        let terms = read_terms(&env)?;
        let state = read_state(&env)?;
        let deadline = match &state {
            EscrowState::AwaitingDeposit(deadline)
            | EscrowState::AwaitingBeneficiary(deadline)
            | EscrowState::AwaitingRelease(deadline)
            | EscrowState::Disputed(deadline, ..) => *deadline,
            _ => return Err(Error::InvalidTransition),
        };
        timeout::ensure_reached(&env, deadline)?;

        match state {
            EscrowState::AwaitingDeposit(_) => {
                let terminal = EscrowState::Cancelled;
                env.storage().instance().set(&DataKey::State, &terminal);
                Ok(terminal)
            }
            EscrowState::AwaitingBeneficiary(_) => {
                settle(
                    &env,
                    &terms,
                    EscrowState::Refunded,
                    Resolution::RefundToDepositor,
                );
                Ok(EscrowState::Refunded)
            }
            EscrowState::AwaitingRelease(_) => {
                settle(
                    &env,
                    &terms,
                    EscrowState::Released,
                    Resolution::ReleaseToBeneficiary,
                );
                Ok(EscrowState::Released)
            }
            EscrowState::Disputed(_, initiator) => {
                let (terminal, resolution) = match initiator {
                    Participant::Depositor => {
                        (EscrowState::Released, Resolution::ReleaseToBeneficiary)
                    }
                    Participant::Beneficiary => {
                        (EscrowState::Refunded, Resolution::RefundToDepositor)
                    }
                };
                settle(&env, &terms, terminal.clone(), resolution);
                Ok(terminal)
            }
            _ => Err(Error::InvalidTransition),
        }
    }

    pub fn get_state(env: Env) -> Result<EscrowState, Error> {
        read_state(&env)
    }

    pub fn get_terms(env: Env) -> Result<Terms, Error> {
        read_terms(&env)
    }
}

fn read_terms(env: &Env) -> Result<Terms, Error> {
    env.storage()
        .instance()
        .get(&DataKey::Terms)
        .ok_or(Error::NotInitialized)
}

fn read_state(env: &Env) -> Result<EscrowState, Error> {
    env.storage()
        .instance()
        .get(&DataKey::State)
        .ok_or(Error::NotInitialized)
}

/// Set the terminal state before the token call. The recipient is derived
/// from immutable terms, and Soroban transaction rollback preserves the
/// invariant if the token transfer fails.
fn settle(env: &Env, terms: &Terms, terminal_state: EscrowState, resolution: Resolution) {
    let destination = arbiter::destination(terms, resolution);
    env.storage()
        .instance()
        .set(&DataKey::State, &terminal_state);
    let contract_address = env.current_contract_address();
    soroban_sdk::token::Client::new(env, &terms.token).transfer(
        &contract_address,
        &destination,
        &terms.amount,
    );
}
