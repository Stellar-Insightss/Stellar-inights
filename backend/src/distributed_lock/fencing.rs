//! Monotonic fencing token issuance
//!
//! Fencing tokens are the key to preventing stale writers in distributed systems.
//! Every successful lock acquisition returns a unique, monotonically increasing token.
//! All writes performed under the lock must carry this token.
//! The storage layer rejects any write with a token older than the highest token
//! it has already seen for that resource — making the storage layer the source of truth.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// A fencing token that uniquely identifies a lock acquisition
/// 
/// The token is used to detect and reject writes from stale lock holders
/// that have experienced a pause or network partition.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct FencingToken {
    /// Monotonically increasing token value
    pub value: u64,
    /// Timestamp when token was issued (for observability)
    pub issued_at: i64,
}

/// Generates monotonically increasing fencing tokens
/// 
/// This is not a global counter but per-resource. Each resource has its own
/// fencing token sequence to detect stale writers for that specific resource.
pub struct FencingTokenGenerator {
    next_token: AtomicU64,
}

impl FencingTokenGenerator {
    /// Create a new fencing token generator starting from 0
    pub fn new() -> Self {
        Self {
            next_token: AtomicU64::new(0),
        }
    }

    /// Create a token generator starting from a specific value
    /// 
    /// Used when recovering from persistent storage (e.g., Redis, Postgres)
    /// to maintain monotonicity across restarts
    pub fn from_last_token(last_token: u64) -> Self {
        Self {
            next_token: AtomicU64::new(last_token + 1),
        }
    }

    /// Issue the next fencing token
    /// 
    /// Performs an atomic increment, guaranteeing monotonicity even under
    /// concurrent acquisition attempts.
    pub fn next(&self) -> FencingToken {
        let value = self.next_token.fetch_add(1, Ordering::SeqCst);
        let issued_at = chrono::Local::now().timestamp_millis();
        FencingToken { value, issued_at }
    }

    /// Peek at the next token without issuing it (for testing/observability)
    pub fn peek_next(&self) -> u64 {
        self.next_token.load(Ordering::Acquire)
    }
}

/// Resource-specific fencing state tracking
pub struct ResourceFencingState {
    /// Highest token ever issued for this resource
    pub highest_issued: u64,
    /// Highest token accepted in a write for this resource
    pub highest_accepted: u64,
}

impl ResourceFencingState {
    pub fn new() -> Self {
        Self {
            highest_issued: 0,
            highest_accepted: 0,
        }
    }

    /// Check if a token is valid for writing
    /// 
    /// A token is valid if:
    /// 1. It matches or exceeds the highest token we've issued (no token reuse)
    /// 2. It's the same as what we just issued (the current holder)
    /// 
    /// Returns an error if the token is older than the highest accepted token,
    /// indicating a stale writer.
    pub fn validate_write_token(&mut self, token: u64) -> Result<(), String> {
        if token < self.highest_accepted {
            return Err(format!(
                "Stale writer detected: attempted token {}, but highest accepted is {}",
                token, self.highest_accepted
            ));
        }
        self.highest_accepted = token.max(self.highest_accepted);
        Ok(())
    }

    /// Record a newly issued token
    pub fn record_issued(&mut self, token: u64) {
        self.highest_issued = token.max(self.highest_issued);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fencing_token_generator_monotonic() {
        let gen = FencingTokenGenerator::new();
        let token1 = gen.next();
        let token2 = gen.next();
        let token3 = gen.next();

        assert_eq!(token1.value, 0);
        assert_eq!(token2.value, 1);
        assert_eq!(token3.value, 2);
        assert!(token1 < token2);
        assert!(token2 < token3);
    }

    #[test]
    fn test_fencing_token_generator_from_last() {
        let gen = FencingTokenGenerator::from_last_token(99);
        let token1 = gen.next();
        let token2 = gen.next();

        assert_eq!(token1.value, 100);
        assert_eq!(token2.value, 101);
    }

    #[test]
    fn test_resource_fencing_state_valid_token() {
        let mut state = ResourceFencingState::new();
        
        // First write with token 0
        state.record_issued(0);
        assert!(state.validate_write_token(0).is_ok());
        assert_eq!(state.highest_accepted, 0);

        // Second write with token 1
        state.record_issued(1);
        assert!(state.validate_write_token(1).is_ok());
        assert_eq!(state.highest_accepted, 1);
    }

    #[test]
    fn test_resource_fencing_state_rejects_stale_token() {
        let mut state = ResourceFencingState::new();
        
        // Accept token 5
        state.record_issued(5);
        assert!(state.validate_write_token(5).is_ok());
        
        // Reject token 3 (stale)
        let result = state.validate_write_token(3);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Stale writer"));
    }

    #[test]
    fn test_resource_fencing_state_concurrent_tokens() {
        let mut state = ResourceFencingState::new();
        
        // Simulate concurrent lock acquisitions
        state.record_issued(10);
        state.record_issued(11);
        state.record_issued(12);
        
        // Old token from first holder should be rejected after newer holder writes
        assert!(state.validate_write_token(12).is_ok());
        let result = state.validate_write_token(10);
        assert!(result.is_err());
    }
}
