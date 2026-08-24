use serde_json::json;
use stellar_insights_backend::event_indexer::{dispatch, DispatchError};

const SNAPSHOT_SUBMITTED_TOPIC: &str = "snapshot_submitted";

fn snapshot_submitted_v1_fixture() -> serde_json::Value {
    // Mirrors the contract's SnapshotSubmitted data map field-for-field.
    json!({
        "schema_version": 1,
        "epoch": 42,
        "snapshot_hash": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "source_data_hash": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        "submitted_at": 1_724_512_400,
        "submitter": "GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAWHF"
    })
}

fn snapshot_submitted_v2_fixture() -> serde_json::Value {
    // Hypothetical v2 adds a field while preserving the same logical event.
    json!({
        "schema_version": 2,
        "epoch": 42,
        "snapshot_hash": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "source_data_hash": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        "submitted_at": 1_724_512_400,
        "submitter": "GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAWHF",
        "snapshot_size_bytes": 8_192
    })
}

#[test]
fn mixed_versions_normalize_to_the_same_event() {
    let v1 = dispatch(SNAPSHOT_SUBMITTED_TOPIC, snapshot_submitted_v1_fixture()).unwrap();
    let v2 = dispatch(SNAPSHOT_SUBMITTED_TOPIC, snapshot_submitted_v2_fixture()).unwrap();

    assert_eq!(v1, v2);
}

#[test]
fn unknown_schema_version_is_rejected_explicitly() {
    let mut unknown = snapshot_submitted_v1_fixture();
    unknown["schema_version"] = json!(999);

    let error = dispatch(SNAPSHOT_SUBMITTED_TOPIC, unknown).unwrap_err();
    assert!(matches!(
        error,
        DispatchError::UnsupportedSchemaVersion(999)
    ));
}
