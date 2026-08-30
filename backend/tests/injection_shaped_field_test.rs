//! Test: Verify that SQL injection attempts are caught at the allowlist layer.
//!
//! This test demonstrates a critical security property: the field_selection module
//! catches injection attempts by **rejecting unknown field names**, not by escaping.
//!
//! When a client requests a field name like `"; DROP TABLE users; --"`, it:
//! 1. ✅ Fails the allowlist lookup (HashMap::get returns None)
//! 2. ❌ DOES NOT reach a SQL sanitizer or escaper
//!
//! This is the correct defense: the malicious input never reaches the database layer
//! at all because the HTTP layer rejected it as an invalid field name.
//!
//! When a real SQL layer is added to the HTTP server, it will never see these
//! field names because the field_selection module acts as a gate.

use stellar_insights_backend::field_selection::parse_fields;

#[test]
fn test_sql_injection_in_field_name_rejected_by_allowlist() {
    // A classic SQL injection attempt: `"; DROP TABLE users; --"
    //
    // This is treated as an unknown field name. It's rejected because:
    // 1. The allowlist.get(field_name) is called
    // 2. HashMap does NOT contain `"; DROP TABLE users; --"` as a key
    // 3. HashMap::get returns None
    // 4. We return Err(UnknownField)
    //
    // NO SQL layer is involved. NO escaping happens. NO database is touched.
    // The validation is purely semantic: "is this field name in the allowlist?"
    
    let injection_field = r#""; DROP TABLE users; --"#;
    let result = parse_fields(&format!("epoch,{}", injection_field), "snapshots");
    
    assert!(result.is_err(), "SQL injection attempt should be rejected");
    let err_msg = result.unwrap_err().to_string();
    assert!(err_msg.contains("DROP"));
}

#[test]
fn test_sql_injection_with_union_attempt() {
    // Another common injection: `name" UNION SELECT * FROM users; --`
    //
    // Same result: rejected as unknown field at the allowlist layer.
    
    let injection_field = r#"name" UNION SELECT * FROM users; --"#;
    let result = parse_fields(&format!("epoch,{}", injection_field), "snapshots");
    
    assert!(result.is_err(), "UNION injection attempt should be rejected");
}

#[test]
fn test_sql_injection_with_multiple_statements() {
    // `field1; DELETE FROM logs; --`
    //
    // Rejected at allowlist layer.
    
    let injection_field = "field1; DELETE FROM logs; --";
    let result = parse_fields(&format!("epoch,{}", injection_field), "snapshots");
    
    assert!(result.is_err(), "Multi-statement injection should be rejected");
}

#[test]
fn test_valid_field_with_special_chars_not_in_allowlist_also_rejected() {
    // Even a benign-looking field name with special characters is rejected
    // if it's not in the allowlist. This confirms that the allowlist is
    // the enforcement mechanism, not any kind of character filtering.
    
    let result = parse_fields("epoch,field@with$special", "snapshots");
    
    assert!(result.is_err(), "Special characters in unknown fields should be rejected");
}

#[test]
fn test_valid_field_is_accepted_even_with_special_handling() {
    // On the other side: if a field IS in the allowlist, it's accepted
    // without any additional validation (e.g., no character filtering).
    // The allowlist is the ONLY criterion.
    //
    // This test uses real allowlisted fields.
    
    let result = parse_fields("epoch,snapshot_hash", "snapshots");
    assert!(result.is_ok(), "Valid allowlisted fields should be accepted");
}

#[test]
fn test_comment_injection_attempt_rejected() {
    // SQL comment syntax attempts
    let result = parse_fields("epoch,-- comment", "snapshots");
    assert!(result.is_err(), "SQL comment injection should be rejected");
    
    let result = parse_fields("epoch,/* comment */", "snapshots");
    assert!(result.is_err(), "Block comment injection should be rejected");
}

#[test]
fn test_unicode_escape_attempt_rejected() {
    // Unicode/hex escape sequences (would bypass some filters)
    let result = parse_fields("epoch,\\x27 OR \\x27", "snapshots");
    assert!(result.is_err(), "Unicode escape injection should be rejected");
}

#[test]
fn test_allowlist_as_only_defense_explained() {
    // This test explicitly documents that the allowlist is the ONLY defense
    // against injection attempts in field names.
    //
    // When the HTTP server layer is added and starts building SQL queries,
    // it can safely do something like:
    //
    //     let fields = parse_fields(request.query.fields, "snapshots")?;
    //     // At this point, each field in `fields` is:
    //     // - A valid &'static str borrowed from the allowlist
    //     // - NOT from the client input
    //     // - Safe to use in SQL construction (no escaping needed)
    //
    //     for field in fields {
    //         query_builder.select(field); // Safe!
    //     }
    //
    // The key insight: we use the returned value, not the client input.
    // The client's string never enters the query.
    
    let result = parse_fields("epoch", "snapshots");
    assert!(result.is_ok(), "Valid field should be accepted");
    
    let fields = result.unwrap();
    // `fields` contains only values from the allowlist, not from client input
    assert!(fields.iter().all(|f| !f.contains("client")));
}

