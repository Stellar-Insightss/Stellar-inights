//! Field selection parsing and validation.
//!
//! Parses the `fields` query parameter (comma-separated list) and validates
//! each field against the endpoint's allowlist.

use crate::field_selection::{FieldSelectionError, allowlist};

/// Parses and validates a comma-separated field list for a given endpoint.
///
/// # Arguments
///
/// * `fields_param` - The comma-separated field names requested by the client (e.g., "epoch,snapshot_hash")
/// * `endpoint` - The endpoint identifier (e.g., "snapshots", "aggregates")
///
/// # Returns
///
/// - `Ok(Vec<&'static str>)` if all fields are valid and in the allowlist
/// - `Err(FieldSelectionError::UnknownField)` if any field is unknown or not allowed
///
/// # Behavior
///
/// - **Fail-fast**: Returns immediately on the first invalid field.
/// - **Never silent**: If any field is invalid, the request is rejected entirely.
///   (No partial success or graceful downgrade to a default set.)
/// - **No interpolation**: Field names are only used as keys in HashMap lookups.
///
/// # Example
///
/// ```ignore
/// // Valid request
/// let fields = parse_fields("epoch,snapshot_hash", "snapshots")?;
/// // Returns: vec!["epoch", "snapshot_hash"]
///
/// // Request with unknown field
/// let fields = parse_fields("epoch,submitter", "snapshots")?;
/// // Returns Err: UnknownField { endpoint: "snapshots", field: "submitter" }
///
/// // Request with injection attempt (treated as unknown field)
/// let fields = parse_fields(r#"epoch,"; DROP TABLE users; --"#, "snapshots")?;
/// // Returns Err: UnknownField { endpoint: "snapshots", field: "...DROP..." }
/// // (rejected by allowlist lookup, not by SQL escaping logic)
/// ```
pub fn parse_fields(
    fields_param: &str,
    endpoint: &'static str,
) -> Result<Vec<&'static str>, FieldSelectionError> {
    // Get the allowlist for this endpoint.
    let allowlist = allowlist::get_allowlist(endpoint).ok_or_else(|| {
        FieldSelectionError::UnknownField {
            endpoint,
            field: "(endpoint not found)".to_string(),
        }
    })?;

    // Split the field list and validate each field.
    let mut validated_fields = Vec::new();

    for field_name in fields_param.split(',').map(|s| s.trim()) {
        if field_name.is_empty() {
            continue; // Skip empty segments (e.g., "a,,b" -> skip middle empty string)
        }

        // The only operation on the client-provided field name is a lookup in the allowlist.
        // If the field is not in the allowlist, we reject it.
        // This is where SQL injection attempts are caught (as unknown fields).
        match allowlist.get(field_name) {
            Some(&sql_expr) => {
                validated_fields.push(sql_expr);
            }
            None => {
                // Client requested a field that doesn't exist or isn't allowed.
                // Return immediately with the original field name for diagnostics.
                return Err(FieldSelectionError::UnknownField {
                    endpoint,
                    field: field_name.to_string(),
                });
            }
        }
    }

    Ok(validated_fields)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_valid_fields() {
        let result = parse_fields("epoch,snapshot_hash", "snapshots");
        assert!(result.is_ok());
        let fields = result.unwrap();
        assert_eq!(fields.len(), 2);
        assert!(fields.contains(&"epoch"));
        assert!(fields.contains(&"snapshot_hash"));
    }

    #[test]
    fn test_parse_with_whitespace() {
        let result = parse_fields("epoch, snapshot_hash , submitted_at", "snapshots");
        assert!(result.is_ok());
        let fields = result.unwrap();
        assert_eq!(fields.len(), 3);
    }

    #[test]
    fn test_parse_unknown_field() {
        let result = parse_fields("epoch,submitter", "snapshots");
        assert!(matches!(
            result,
            Err(FieldSelectionError::UnknownField {
                endpoint: "snapshots",
                field
            }) if field == "submitter"
        ));
    }

    #[test]
    fn test_parse_injection_attempt_rejected() {
        // SQL injection attempt is treated as an unknown field name.
        // It fails at the allowlist lookup, not at a SQL layer.
        let result = parse_fields(r#"epoch,"; DROP TABLE users; --"#, "snapshots");
        assert!(matches!(result, Err(FieldSelectionError::UnknownField { .. })));
    }

    #[test]
    fn test_parse_unknown_endpoint() {
        let result = parse_fields("some_field", "nonexistent_endpoint");
        assert!(matches!(
            result,
            Err(FieldSelectionError::UnknownField {
                endpoint: "nonexistent_endpoint",
                ..
            })
        ));
    }

    #[test]
    fn test_parse_empty_string() {
        let result = parse_fields("", "snapshots");
        assert!(result.is_ok());
        let fields = result.unwrap();
        assert_eq!(fields.len(), 0);
    }

    #[test]
    fn test_parse_only_whitespace() {
        let result = parse_fields("   ,  ,  ", "snapshots");
        assert!(result.is_ok());
        let fields = result.unwrap();
        assert_eq!(fields.len(), 0);
    }
}
