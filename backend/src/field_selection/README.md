# Field Selection Module

## Why This Module Exists Before We Have an HTTP Server

The backend is currently a Rust library without an HTTP server. However, the frontend is already written to expect REST endpoints (e.g., `/api/snapshots?fields=epoch,snapshot_hash`). 

This module **preemptively implements the validation and allowlisting infrastructure** for sparse fieldsets, so that when the HTTP layer is added, it can use this module immediately without retrofitting security logic later.

## Security Model

This module addresses **semantic access control**, not cryptographic input escaping:

### What It Does ✅

- **Explicit allowlist per endpoint**: Each endpoint defines exactly which fields can be requested.
- **Allowlist is a subset**: Adding a new field to a Rust struct does **NOT** automatically expose it via the API.
- **Client input is never interpolated**: Field names from the client are **only** used as keys in HashMap lookups. The value returned by the HashMap is the only string that reaches any downstream layer.

### What It Does NOT Do ❌

- **SQL escaping**: This module contains no SQL layer (that comes when the HTTP server is added).
- **Encoding/sanitization**: Input is validated by allowlist, not by escaping or encoding.
- **Authorization**: This module does NOT check if the user has permission to request these fields. That's the HTTP layer's job.

## Example: Two Real Domains

### Snapshots Endpoint

Based on `backend/src/event_indexer/dispatch.rs::NormalizedSnapshotSubmitted`:

**Rust struct has:**
```rust
pub struct NormalizedSnapshotSubmitted {
    pub epoch: u64,
    pub snapshot_hash: String,
    pub source_data_hash: String,      // ← Internal reconciliation state
    pub submitted_at: u64,
    pub submitter: String,             // ← Operator identity (privacy concern)
}
```

**Allowlist exposes (only):**
```
epoch
snapshot_hash
submitted_at
```

**Rationale:**
- `epoch` and `submitted_at` are audit metadata, safe for public consumption.
- `snapshot_hash` is the canonical identifier clients need for verification.
- `source_data_hash` is excluded: it's internal state used for reconciliation, not a public concern.
- `submitter` is excluded: reveals private operator identity.

**Result:**
A client requesting `?fields=epoch,submitter` receives a 400 error:
```
field 'submitter' is not available for endpoint 'snapshots'
```

### Aggregates Endpoint

Based on `backend/src/reconciliation/spec.rs::OffChainAggregate`:

**Rust struct has:**
```rust
pub struct OffChainAggregate {
    pub period: u64,
    pub snapshot_hash: [u8; 32],
    pub source_data_hash: [u8; 32],    // ← Internal reconciliation state
}
```

**Allowlist exposes (only):**
```
period
snapshot_hash
```

**Rationale:**
- Clients request aggregates by their period and snapshot hash.
- `source_data_hash` is internal; clients don't request by it.

## How to Integrate When the HTTP Server Exists

### Step 1: Add the HTTP Handler

```rust
use stellar_insights_backend::field_selection::{parse_fields, FieldSelectionError};

async fn get_snapshots(
    Query(params): Query<QueryParams>,
) -> Result<Json<SnapshotResponse>, ApiError> {
    // If `?fields=...` is provided, validate and filter
    let selected_fields = if let Some(fields_param) = params.fields {
        parse_fields(&fields_param, "snapshots")
            .map_err(|e| ApiError::BadRequest(e.to_string()))?
    } else {
        // Default to a standard set if no fields requested
        vec!["epoch", "snapshot_hash", "submitted_at"]
    };
    
    // Now build the response with only selected_fields
    // ...
}
```

### Step 2: Map Errors to HTTP

```rust
use stellar_insights_backend::field_selection::FieldSelectionError;

impl From<FieldSelectionError> for ApiError {
    fn from(e: FieldSelectionError) -> Self {
        // FieldSelectionError::UnknownField -> 400 Bad Request
        ApiError::BadRequest(e.to_string())
    }
}
```

### Step 3: Serialize Only Selected Fields

If using `serde_json`, you can dynamically include/exclude fields or use a custom serializer. Or implement a simple projection struct.

## Testing

Two test files validate the security properties:

### `backend/tests/non_allowlisted_field_test.rs`

For each endpoint with an allowlist, this test requests a field that **exists in the Rust struct** but is **deliberately excluded** from the allowlist:

```rust
// Attempt to request `submitter` from snapshots endpoint
let result = parse_fields("epoch,submitter", "snapshots");
assert_eq!(
    result,
    Err(FieldSelectionError::UnknownField {
        endpoint: "snapshots",
        field: "submitter".to_string(),
    })
);
```

**This validates:** The allowlist enforces a security boundary, not just rejects truly nonexistent fields.

### `backend/tests/injection_shaped_field_test.rs`

This test attempts a field name that contains SQL injection patterns:

```rust
// Client tries to inject SQL
let result = parse_fields(r#"epoch,"; DROP TABLE users; --"#, "snapshots");
assert_eq!(
    result,
    Err(FieldSelectionError::UnknownField {
        endpoint: "snapshots",
        field: r#""; DROP TABLE users; --"#.to_string(),
    })
);
```

**This validates:** The injection attempt is caught at the allowlist lookup stage (HashMap::get returns None), **not** by a SQL sanitizer. The comment in the test explains that this is the correct defense: when the HTTP server adds a SQL layer later, it will never receive this malformed field name because it was rejected at the allowlist layer.

## Design Rationale

### Why Not Auto-Reflect the Struct?

❌ **Bad:** 
```rust
// ← DON'T DO THIS
let allowlist = derive_allowlist_from_struct::<NormalizedSnapshotSubmitted>();
// Now any new field in the struct is automatically exposed.
// Adding `submitter` to the struct = it leaks to the API.
```

✅ **Good:**
```rust
// ← DO THIS
lazy_static! {
    static ref SNAPSHOTS_ALLOWLIST: HashMap<&'static str, &'static str> = {
        [("epoch", "epoch"), ("snapshot_hash", "snapshot_hash"), ...]
        .iter()
        .copied()
        .collect()
    };
}
// New fields in the struct are NOT exposed until explicitly added to the allowlist.
```

### Why Not Just Escape?

❌ **Wrong security model:**
```rust
// ← DON'T DO THIS
let field = client_input; // e.g., "submitter"; DROP TABLE users; --"
let sql = format!("SELECT {} FROM ...", escape_sql(field));
// The field passed validation, so we build the query.
// escaping prevents injection, but the field was never supposed to exist.
```

✅ **Correct security model:**
```rust
// ← DO THIS
let field = client_input; // e.g., "submitter"; DROP TABLE users; --"
let allowed_fields = get_allowlist("snapshots")?;
let sql_expr = allowed_fields.get(field)?; // Returns None; request fails at 400
// The field never reaches SQL at all.
```

The first model escapes at the wrong layer. The second model prevents the problem entirely.

## Future Extensions

### Per-Tenant Allowlists

If you need different field sets per customer/tenant:

```rust
pub trait AllowlistRegistry {
    fn get_allowlist(&self, endpoint: &str, tenant: &str) -> Option<&'static HashMap<&'static str, &'static str>>;
}
```

### Dynamic Field Aliases

If the Rust field name doesn't match the public API name:

```rust
static ref SNAPSHOTS_ALLOWLIST: HashMap<&'static str, &'static str> = {
    [
        ("submittedAt", "submitted_at"),  // ← client sees "submittedAt", Rust uses "submitted_at"
        ("epoch", "epoch"),
    ]
    .iter()
    .copied()
    .collect()
};
```

This already works! The HashMap values can differ from the keys.

---

**Last updated:** 2026-08-30  
**Status:** Ready for HTTP layer integration
