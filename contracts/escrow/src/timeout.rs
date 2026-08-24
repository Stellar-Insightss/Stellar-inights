use soroban_sdk::Env;

use crate::Error;

/// A zero-duration phase would make a participant transition impossible and
/// would not provide a meaningful liveness window.
pub fn validate_duration(duration: u64) -> Result<(), Error> {
    if duration == 0 {
        Err(Error::InvalidTimeout)
    } else {
        Ok(())
    }
}

/// Derive a phase deadline from the current Soroban ledger timestamp.
///
/// An overflowing timestamp plus duration is rejected instead of being
/// silently converted into a deadline at `u64::MAX`.
pub fn from_now(env: &Env, duration: u64) -> Result<u64, Error> {
    validate_duration(duration)?;
    env.ledger()
        .timestamp()
        .checked_add(duration)
        .ok_or(Error::InvalidTimeout)
}

pub fn ensure_before(env: &Env, deadline: u64) -> Result<(), Error> {
    if env.ledger().timestamp() >= deadline {
        Err(Error::DeadlinePassed)
    } else {
        Ok(())
    }
}

pub fn ensure_reached(env: &Env, deadline: u64) -> Result<(), Error> {
    if env.ledger().timestamp() < deadline {
        Err(Error::DeadlineNotReached)
    } else {
        Ok(())
    }
}
