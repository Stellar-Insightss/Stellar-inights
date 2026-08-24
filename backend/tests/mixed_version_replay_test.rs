use async_trait::async_trait;
use soroban_sdk::{contractevent, testutils::Address as _, Address, BytesN, Env, Event};
use stellar_insights::event::{SnapshotSubmitted, SNAPSHOT_SUBMITTED_SCHEMA_VERSION};
use stellar_insights_backend::event_indexer::{
    AnalyticsSink, AnalyticsSinkError, DispatchError, EventIndexer, InMemoryAnalyticsSink,
    IndexerError, NormalizedContractEvent, NormalizedEvent, NormalizedSnapshotSubmitted,
};
use stellar_xdr::ContractEvent;

#[contractevent(topics = ["snapshot_submitted"])]
#[derive(Clone, Debug, Eq, PartialEq)]
struct SnapshotSubmittedV2Fixture {
    schema_version: u32,
    epoch: u64,
    snapshot_hash: BytesN<32>,
    source_data_hash: BytesN<32>,
    submitted_at: u64,
    submitter: Address,
    snapshot_size_bytes: u64,
}

struct RejectingAnalyticsSink;

#[async_trait]
impl AnalyticsSink for RejectingAnalyticsSink {
    async fn record(&self, _: &NormalizedContractEvent) -> Result<(), AnalyticsSinkError> {
        Err(AnalyticsSinkError::Rejected("storage unavailable".into()))
    }
}

struct ReplayFixtures {
    v1: ContractEvent,
    v2: ContractEvent,
    unknown: ContractEvent,
    expected: NormalizedContractEvent,
}

fn fixtures() -> ReplayFixtures {
    let env = Env::default();
    let contract_id = env.register(stellar_insights::StellarInsights, ());
    let submitter = Address::generate(&env);
    let snapshot_hash = BytesN::from_array(&env, &[0xaa; 32]);
    let source_data_hash = BytesN::from_array(&env, &[0xbb; 32]);

    let v1 = SnapshotSubmitted {
        schema_version: SNAPSHOT_SUBMITTED_SCHEMA_VERSION,
        epoch: 42,
        snapshot_hash: snapshot_hash.clone(),
        source_data_hash: source_data_hash.clone(),
        submitted_at: 1_724_512_400,
        submitter: submitter.clone(),
    }
    .to_xdr(&env, &contract_id);

    let v2 = SnapshotSubmittedV2Fixture {
        schema_version: 2,
        epoch: 42,
        snapshot_hash: snapshot_hash.clone(),
        source_data_hash: source_data_hash.clone(),
        submitted_at: 1_724_512_400,
        submitter: submitter.clone(),
        snapshot_size_bytes: 8_192,
    }
    .to_xdr(&env, &contract_id);

    let unknown = SnapshotSubmittedV2Fixture {
        schema_version: 999,
        epoch: 42,
        snapshot_hash,
        source_data_hash,
        submitted_at: 1_724_512_400,
        submitter: submitter.clone(),
        snapshot_size_bytes: 8_192,
    }
    .to_xdr(&env, &contract_id);

    let expected = NormalizedContractEvent {
        contract_id: v1.contract_id.as_ref().unwrap().to_string(),
        event: NormalizedEvent::SnapshotSubmitted(NormalizedSnapshotSubmitted {
            epoch: 42,
            snapshot_hash: "aa".repeat(32),
            source_data_hash: "bb".repeat(32),
            submitted_at: 1_724_512_400,
            submitter: format!("{}", submitter.to_string()),
        }),
    };

    ReplayFixtures {
        v1,
        v2,
        unknown,
        expected,
    }
}

#[tokio::test]
async fn mixed_xdr_versions_feed_the_same_logical_analytics() {
    let fixtures = fixtures();
    let sink = InMemoryAnalyticsSink::new();
    let indexer = EventIndexer::new(sink.clone());

    let normalized = indexer
        .index_batch([&fixtures.v1, &fixtures.v2])
        .await
        .unwrap();

    assert_eq!(normalized[0], normalized[1]);
    assert_eq!(normalized[0], fixtures.expected);
    assert_eq!(sink.events().await, normalized);
}

#[tokio::test]
async fn unknown_schema_version_is_rejected_without_an_analytics_write() {
    let fixtures = fixtures();
    let sink = InMemoryAnalyticsSink::new();
    let indexer = EventIndexer::new(sink.clone());

    let error = indexer.index(&fixtures.unknown).await.unwrap_err();
    assert!(matches!(
        error,
        IndexerError::Dispatch(DispatchError::UnsupportedSchemaVersion(999))
    ));
    assert!(sink.events().await.is_empty());
}

#[tokio::test]
async fn analytics_sink_failures_are_propagated() {
    let fixtures = fixtures();
    let indexer = EventIndexer::new(RejectingAnalyticsSink);

    let error = indexer.index(&fixtures.v1).await.unwrap_err();
    assert!(matches!(
        error,
        IndexerError::Analytics(AnalyticsSinkError::Rejected(message))
            if message == "storage unavailable"
    ));
}
