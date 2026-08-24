use soroban_sdk::{contracttype, Address};

use crate::state::Terms;

/// The only two outcomes an arbiter can select. There is deliberately no
/// arbitrary recipient or amount in this type.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Resolution {
    ReleaseToBeneficiary,
    RefundToDepositor,
}

/// Resolve the enum choice to an address already committed in `Terms`.
///
/// Keeping this mapping inside the contract means arbiter authority cannot
/// redirect funds, alter the parties/token/amount, or create a new outcome.
pub fn destination(terms: &Terms, resolution: Resolution) -> Address {
    match resolution {
        Resolution::ReleaseToBeneficiary => terms.beneficiary.clone(),
        Resolution::RefundToDepositor => terms.depositor.clone(),
    }
}
