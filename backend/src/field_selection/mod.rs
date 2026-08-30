//! Sparse fieldset selection with explicit allowlisting.
//!
//! This module provides a security-focused infrastructure for filtering fields in API responses.
//! It is **transport-agnostic**: no HTTP status codes, no framework dependencies.
//!
//! Key design principles:
//! 1. **Explicit allowlist per endpoint** (not per global schema).
//! 2. **Allowlist is a subset** of the struct's fields — new struct fields don't auto-expose.
//! 3. **Client input is never interpolated** into queries — only used as a lookup key.
//! 4. **Errors are semantic types**, not HTTP status codes (those are the HTTP layer's responsibility).

pub mod allowlist;
pub mod parse;

use std::error::Error;
use std::fmt;

/// Sparse fieldset selection error.
///
/// This is a semantic error type with no HTTP coupling.
/// The HTTP layer (when it exists) should map `UnknownField` to 400 Bad Request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldSelectionError {
    /// A requested field is not in the allowlist for this endpoint.
    ///
    /// This rejects both:
    /// - Fields that don't exist in the struct
    /// - Fields that exist but are deliberately excluded from the API
    ///
    /// Failing early and explicitly on unknown fields prevents silent data loss
    /// and signals to the client that their request is malformed.
    UnknownField {
        endpoint: &'static str,
        field: String,
    },
}

impl fmt::Display for FieldSelectionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownField { endpoint, field } => {
                write!(
                    f,
                    "field '{}' is not available for endpoint '{}'",
                    field, endpoint
                )
            }
        }
    }
}

impl Error for FieldSelectionError {}

/// Public API exports
pub use allowlist::{AllowlistRegistry, get_allowlist};
pub use parse::parse_fields;
