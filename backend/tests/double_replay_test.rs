use std::{env, io::Write, process::Command};

use stellar_insights_backend::replay::{replay_historical_range, LedgerRange};
use stellar_insights_backend::snapshot::generator::{generate_snapshot, RawSnapshotRow};

fn fixture_rows() -> Vec<RawSnapshotRow> {
    vec![
        RawSnapshotRow {
            ledger_sequence: 1_400,
            corridor: "eur/usd".to_string(),
            source: "mainnet".to_string(),
            reliability: 0.7,
            volume: 10.0,
            latency_ms: 45.0,
        },
        RawSnapshotRow {
            ledger_sequence: 1_401,
            corridor: "eur/usd".to_string(),
            source: "mainnet".to_string(),
            reliability: 0.8,
            volume: 30.0,
            latency_ms: 40.0,
        },
        RawSnapshotRow {
            ledger_sequence: 1_420,
            corridor: "btc/usd".to_string(),
            source: "mainnet".to_string(),
            reliability: 0.95,
            volume: 50.0,
            latency_ms: 66.0,
        },
        RawSnapshotRow {
            ledger_sequence: 1_410,
            corridor: "btc/usd".to_string(),
            source: "mainnet".to_string(),
            reliability: 0.9,
            volume: 20.0,
            latency_ms: 60.0,
        },
    ]
}

#[test]
fn double_replay_is_byte_identical() {
    if env::var_os("STELLAR_INSIGHTS_REPLAY_HELPER").is_some() {
        let rows = fixture_rows();
        let output = replay_historical_range(
            &LedgerRange {
                start_ledger: 1_400,
                end_ledger: 1_420,
            },
            &rows,
        )
        .unwrap();
        std::io::stdout().write_all(&output).unwrap();
        return;
    }

    let first = run_helper();
    let second = run_helper();

    assert_eq!(first, second, "replay output changed between runs");
    assert!(!first.is_empty());
}

fn run_helper() -> Vec<u8> {
    let output = Command::new(env::current_exe().unwrap())
        .arg("--exact")
        .arg("double_replay_is_byte_identical")
        .env("STELLAR_INSIGHTS_REPLAY_HELPER", "1")
        .output()
        .expect("helper process should run");

    assert!(output.status.success(), "helper replay failed: {output:?}");
    output.stdout
}

#[test]
fn replay_snapshot_is_deterministic_for_same_input_order() {
    let rows = fixture_rows();
    let first = generate_snapshot(&rows).unwrap();
    let mut shuffled = rows.clone();
    shuffled.reverse();
    let second = generate_snapshot(&shuffled).unwrap();
    assert_eq!(first, second);
}

#[test]
fn replay_path_has_no_wall_clock_reads() {
    let files = [
        "src/snapshot/generator.rs",
        "src/snapshot/model_version.rs",
        "src/replay/mod.rs",
    ];

    let combined = files
        .into_iter()
        .map(|p| std::fs::read_to_string(p).unwrap_or_default())
        .collect::<String>();

    for banned in [
        "SystemTime::now",
        "std::time::SystemTime::now",
        "chrono::Utc::now",
        "chrono::Local::now",
        "Instant::now",
        "std::time::Instant::now",
    ] {
        assert!(
            !combined.contains(banned),
            "replay path must not contain wall-clock read: {banned}"
        );
    }
}
