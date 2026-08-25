//! Time validation for scheduled transfers.
//!
//! The authoritative clock is [`soroban_sdk::Ledger::timestamp`], which is
//! the UNIX timestamp at which the current ledger was closed. It is paired
//! with [`soroban_sdk::Ledger::sequence`] when a transfer is created so that
//! execution can reject impossible local ledger progressions before applying
//! the timestamp-based unlock rule. See the official Stellar ledger
//! documentation: <https://developers.stellar.org/docs/learn/fundamentals/stellar-data-structures/ledgers>.
//!
//! Stellar Core's `HerderSCPDriver::checkCloseTime` rejects a proposed close
//! time that is not strictly greater than the previous close time, and also
//! rejects a close time more than
//! `Herder::MAX_TIME_SLIP_SECONDS = 60` seconds ahead of the validator's
//! local clock. Strictly increasing close timestamps are the ledger/network
//! guarantee used by this contract. The two named checks are Stellar Core
//! validation rules applied using each validator's local wall clock; the
//! contract cannot independently enforce the 60-second local-clock rule
//! because it has no independent validator wall clock.
//!
//! Ledger-close timing introduced in protocol 23 is a configurable
//! approximately four-to-five-second operational target, not a consensus
//! maximum. Network stalls can therefore produce a much larger timestamp jump
//! between consecutive ledgers. The progression check intentionally enforces
//! only the actual guarantees: sequence and timestamp cannot regress, a
//! timestamp cannot change without a new ledger, and each elapsed ledger must
//! account for at least one elapsed integer timestamp second.

use crate::Error;

/// Validate that a scheduled unlock is strictly after the current ledger
/// timestamp. No duration arithmetic is needed, so `u64::MAX` remains a
/// valid future absolute timestamp when the current timestamp is lower.
pub fn validate_schedule(current_timestamp: u64, unlock_time: u64) -> Result<(), Error> {
    if unlock_time <= current_timestamp {
        Err(Error::InvalidUnlockTime)
    } else {
        Ok(())
    }
}

/// Validate ledger progression from scheduling to execution.
///
/// The checks use only protocol guarantees and deliberately do not impose an
/// upper bound on elapsed timestamp per ledger. A long timestamp jump can be
/// legitimate after network downtime.
pub fn validate_progression(
    created_at: u64,
    created_ledger: u32,
    current_timestamp: u64,
    current_ledger: u32,
) -> Result<(), Error> {
    if current_ledger < created_ledger || current_timestamp < created_at {
        return Err(Error::InvalidLedgerProgression);
    }

    if current_ledger == created_ledger && current_timestamp != created_at {
        return Err(Error::InvalidLedgerProgression);
    }

    let elapsed_timestamp = current_timestamp
        .checked_sub(created_at)
        .ok_or(Error::InvalidLedgerProgression)?;
    let elapsed_ledgers = u64::from(
        current_ledger
            .checked_sub(created_ledger)
            .ok_or(Error::InvalidLedgerProgression)?,
    );

    if elapsed_timestamp < elapsed_ledgers {
        return Err(Error::InvalidLedgerProgression);
    }

    Ok(())
}

/// Apply the exact absolute-time unlock rule after progression is validated.
pub fn ensure_reached(current_timestamp: u64, unlock_time: u64) -> Result<(), Error> {
    if current_timestamp < unlock_time {
        Err(Error::NotUnlockedYet)
    } else {
        Ok(())
    }
}
