use soroban_sdk::{contracttype, Address};

/// The participant whose action started a dispute. The value is retained so
/// the arbiter-silence timeout can select the counterparty's outcome.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Participant {
    Depositor,
    Beneficiary,
}

/// Complete escrow state machine.
///
/// Non-terminal states carry their own absolute deadline. The only way to
/// enter a terminal state is the corresponding participant action or the
/// permissionless timeout for that state:
///
/// - `AwaitingDeposit`: the depositor may deposit before `deadline`; timeout
///   cancels the unfunded escrow.
/// - `AwaitingBeneficiary`: the beneficiary may accept, or timeout refunds the
///   depositor. No dispute is available before beneficiary acceptance.
/// - `AwaitingRelease`: the depositor may release, or either participant may
///   open one dispute; timeout releases to the beneficiary after acceptance.
/// - `Disputed`: only the configured arbiter may choose one of the two fixed
///   outcomes before `deadline`; timeout chooses the outcome opposite the
///   dispute initiator.
/// - `Released`, `Refunded`, and `Cancelled` are terminal. No action or
///   timeout transition is valid from them.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EscrowState {
    AwaitingDeposit(u64),
    AwaitingBeneficiary(u64),
    AwaitingRelease(u64),
    Disputed(u64, Participant),
    Released,
    Refunded,
    Cancelled,
}

/// Immutable terms committed when the escrow is constructed.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Terms {
    pub depositor: Address,
    pub beneficiary: Address,
    pub arbiter: Address,
    pub token: Address,
    pub amount: i128,
    pub deposit_timeout: u64,
    pub beneficiary_timeout: u64,
    pub release_timeout: u64,
    pub dispute_timeout: u64,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DataKey {
    Terms,
    State,
}
