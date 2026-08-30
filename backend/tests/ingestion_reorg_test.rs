//! Replays a synthetic reorg — two conflicting ledgers at the same height —
//! through the full ingestion pipeline (fetch -> checkpoint -> upsert) and
//! asserts the final state matches only the canonical chain, with no
//! manual intervention. This is the acceptance test for issue #319.

use stellar_insights_backend::ingestion::{
    DerivedStore, FakeLedgerSource, InMemoryDerivedStore, InMemoryWatermarkStore, IngestOutcome,
    IngestionPipeline, WatermarkStore,
};
use stellar_insights_backend::snapshot::generator::RawSnapshotRow;

fn row(sequence: u64, corridor: &str, volume: f64) -> RawSnapshotRow {
    RawSnapshotRow {
        ledger_sequence: sequence,
        corridor: corridor.to_string(),
        source: "testnet".to_string(),
        reliability: 1.0,
        volume,
        latency_ms: 12.0,
    }
}

async fn run_to_idle(
    pipeline: &IngestionPipeline<FakeLedgerSource, InMemoryWatermarkStore, InMemoryDerivedStore>,
) {
    loop {
        if pipeline.run_once().await.expect("run_once must not error") == IngestOutcome::Idle {
            break;
        }
    }
}

#[tokio::test]
async fn a_reorg_fixture_replays_to_only_the_canonical_chain_with_no_manual_intervention() {
    let source = FakeLedgerSource::new();
    // Canonical-looking chain, fork A: ledgers 1..=6.
    for seq in 1..=6u64 {
        source
            .append("fork-a", vec![row(seq, "eur/usd", seq as f64 * 10.0)])
            .await;
    }

    let pipeline = IngestionPipeline::new(
        source,
        InMemoryWatermarkStore::new(),
        InMemoryDerivedStore::new(),
        1,
        3,
    );

    run_to_idle(&pipeline).await;
    assert_eq!(
        pipeline
            .derived_store()
            .rows_in_range(1, 6)
            .await
            .unwrap()
            .len(),
        6
    );

    // A reorg replaces ledgers 5 and 6 with a competing fork B, which also
    // extends one ledger past the old tip (7) — the only way the pipeline
    // can discover a reorg is by observing a new ledger whose ancestry
    // doesn't match what it already committed.
    pipeline
        .source()
        .fork_from(
            5,
            vec![
                ("fork-b", vec![row(5, "gbp/usd", 500.0)]),
                ("fork-b", vec![row(6, "gbp/usd", 600.0)]),
                ("fork-b", vec![row(7, "gbp/usd", 700.0)]),
            ],
        )
        .await;

    // No manual intervention: just keep calling run_once, exactly as a
    // production poll loop would.
    run_to_idle(&pipeline).await;

    let final_rows = pipeline.derived_store().rows_in_range(1, 7).await.unwrap();
    let mut by_sequence: Vec<(u64, &str)> = final_rows
        .iter()
        .map(|row| (row.ledger_sequence, row.corridor.as_str()))
        .collect();
    by_sequence.sort_unstable();

    assert_eq!(
        by_sequence,
        vec![
            (1, "eur/usd"),
            (2, "eur/usd"),
            (3, "eur/usd"),
            (4, "eur/usd"),
            (5, "gbp/usd"),
            (6, "gbp/usd"),
            (7, "gbp/usd"),
        ],
        "final state must reflect only the canonical (fork-b) chain, with no \
         duplicate or orphaned rows from the abandoned fork-a ledgers"
    );

    // The watermark itself must also point at the canonical chain's tip.
    let watermark = pipeline
        .watermark_store()
        .load()
        .await
        .unwrap()
        .expect("watermark must be set");
    assert_eq!(watermark.sequence, 7);
}

#[tokio::test]
async fn re_running_ingestion_over_an_already_processed_range_is_byte_identical() {
    let source = FakeLedgerSource::new();
    for seq in 1..=4u64 {
        source
            .append("fork-a", vec![row(seq, "eur/usd", seq as f64)])
            .await;
    }

    let pipeline = IngestionPipeline::new(
        source,
        InMemoryWatermarkStore::new(),
        InMemoryDerivedStore::new(),
        1,
        3,
    );
    run_to_idle(&pipeline).await;
    let first_pass = pipeline.derived_store().rows_in_range(1, 4).await.unwrap();

    pipeline.watermark_store().clear().await.unwrap();
    run_to_idle(&pipeline).await;
    let second_pass = pipeline.derived_store().rows_in_range(1, 4).await.unwrap();

    assert_eq!(first_pass, second_pass);
}
