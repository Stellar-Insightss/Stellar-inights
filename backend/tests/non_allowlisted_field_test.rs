//! Test: Verify that fields deliberately excluded from the allowlist are rejected.
//!
//! These tests validate that the allowlist enforces a **security boundary**,
//! not just a "field existence" check.
//!
//! The fields tested here exist in the real Rust structs but are intentionally
//! excluded from the API via the allowlist. A successful attack would be if a
//! client could request these fields and receive them in the response.
//!
//! These tests verify that the field_selection module rejects them.

use stellar_insights_backend::field_selection::parse_fields;

#[test]
fn test_snapshots_excludes_source_data_hash_from_allowlist() {
    // NormalizedSnapshotSubmitted has source_data_hash, but it's excluded from
    // the allowlist because it's internal reconciliation state.
    
    let result = parse_fields("epoch,source_data_hash", "snapshots");
    
    // Verify it's an error
    assert!(result.is_err(), "Expected error for excluded field source_data_hash");
    
    // Verify error message contains the field name
    let err_msg = result.unwrap_err().to_string();
    assert!(err_msg.contains("source_data_hash"));
    assert!(err_msg.contains("snapshots"));
}

#[test]
fn test_snapshots_excludes_submitter_from_allowlist() {
    // NormalizedSnapshotSubmitted has submitter, but it's excluded for privacy
    // (reveals operator identity).
    
    let result = parse_fields("epoch,submitter", "snapshots");
    
    assert!(result.is_err(), "Expected error for excluded field submitter");
    let err_msg = result.unwrap_err().to_string();
    assert!(err_msg.contains("submitter"));
}

#[test]
fn test_snapshots_allows_valid_fields_only() {
    // Allowed fields for snapshots endpoint
    let result = parse_fields("epoch,snapshot_hash,submitted_at", "snapshots");
    
    assert!(result.is_ok(), "Expected success for valid fields");
    let fields = result.unwrap();
    assert_eq!(fields.len(), 3);
}

#[test]
fn test_aggregates_excludes_source_data_hash_from_allowlist() {
    // OffChainAggregate has source_data_hash, but it's excluded because
    // clients don't request by it; they request by period and snapshot_hash.
    
    let result = parse_fields("period,source_data_hash", "aggregates");
    
    assert!(result.is_err(), "Expected error for excluded field source_data_hash");
    let err_msg = result.unwrap_err().to_string();
    assert!(err_msg.contains("source_data_hash"));
}

#[test]
fn test_aggregates_allows_valid_fields_only() {
    // Allowed fields for aggregates endpoint
    let result = parse_fields("period,snapshot_hash", "aggregates");
    
    assert!(result.is_ok(), "Expected success for valid fields");
    let fields = result.unwrap();
    assert_eq!(fields.len(), 2);
}

#[test]
fn test_partial_invalid_field_list_rejected_entirely() {
    // If even one field is invalid, the entire request is rejected.
    // This is fail-fast behavior: no partial results.
    
    let result = parse_fields("epoch,submitter,snapshot_hash", "snapshots");
    
    // Should fail on the second field (submitter)
    assert!(result.is_err(), "Expected fail-fast on invalid field in list");
    let err_msg = result.unwrap_err().to_string();
    assert!(err_msg.contains("submitter"));
}

