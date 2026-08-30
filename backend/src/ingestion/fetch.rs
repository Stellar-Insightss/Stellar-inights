//! Sources of ledger data for the ingestion pipeline: a real Horizon-backed
//! wrapper, and a deterministic in-memory fake used to drive the pipeline in
//! tests (including simulated reorgs) without a live network.

use async_trait::async_trait;
use serde::Deserialize;
use std::collections::BTreeMap;

use crate::network::NetworkClient;
use crate::snapshot::generator::RawSnapshotRow;

use super::{fnv1a_hex, IngestionError};

/// One fetched ledger: its position in the chain, its hash linkage, and the
/// derived rows this pipeline computed from it.
#[derive(Clone, Debug, PartialEq)]
pub struct FetchedLedger {
    pub sequence: u64,
    pub hash: String,
    pub prev_hash: String,
    pub rows: Vec<RawSnapshotRow>,
}

/// A source of ledger data, fetchable one sequence at a time.
///
/// `fetch_ledger` returning `Ok(None)` means the ledger has not closed yet —
/// distinct from an error, since "not yet available" is the pipeline's
/// normal steady state once it has caught up to the chain tip.
#[async_trait]
pub trait LedgerSource: Send + Sync {
    async fn fetch_ledger(&self, sequence: u64) -> Result<Option<FetchedLedger>, IngestionError>;
}

/// Fetches ledgers from a Horizon-compatible REST API.
///
/// Derives one coarse, ledger-level [`RawSnapshotRow`] per ledger from its
/// transaction success/failure counts. This is intentionally a proxy, not
/// the full per-corridor asset-pair breakdown a real analytics engine would
/// compute from individual payment operations — that computation doesn't
/// exist in this codebase yet and is out of scope here; this wrapper's job
/// is to prove the checkpointing and reorg-handling machinery around it
/// works, with a real (if coarse) row flowing through it end to end.
pub struct HorizonLedgerSource {
    client: reqwest::Client,
    base_url: String,
    network_label: String,
}

impl HorizonLedgerSource {
    pub fn new(network: &NetworkClient) -> Self {
        Self {
            client: reqwest::Client::new(),
            base_url: network.horizon_url.clone(),
            network_label: network.network.to_string(),
        }
    }

    #[cfg(test)]
    fn with_base_url(base_url: String, network_label: String) -> Self {
        Self {
            client: reqwest::Client::new(),
            base_url,
            network_label,
        }
    }
}

#[derive(Debug, Deserialize)]
struct HorizonLedgerResponse {
    sequence: u64,
    hash: String,
    prev_hash: String,
    successful_transaction_count: u64,
    failed_transaction_count: u64,
}

#[async_trait]
impl LedgerSource for HorizonLedgerSource {
    async fn fetch_ledger(&self, sequence: u64) -> Result<Option<FetchedLedger>, IngestionError> {
        let url = format!("{}/ledgers/{sequence}", self.base_url.trim_end_matches('/'));

        let response =
            self.client
                .get(&url)
                .send()
                .await
                .map_err(|error| IngestionError::Fetch {
                    sequence,
                    message: error.to_string(),
                })?;

        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }

        if !response.status().is_success() {
            return Err(IngestionError::Fetch {
                sequence,
                message: format!("Horizon returned HTTP {}", response.status()),
            });
        }

        let body: HorizonLedgerResponse =
            response
                .json()
                .await
                .map_err(|error| IngestionError::Fetch {
                    sequence,
                    message: format!("failed to parse Horizon ledger response: {error}"),
                })?;

        let total = body.successful_transaction_count + body.failed_transaction_count;
        let reliability = if total == 0 {
            1.0
        } else {
            body.successful_transaction_count as f64 / total as f64
        };

        let row = RawSnapshotRow {
            ledger_sequence: body.sequence,
            corridor: "__ledger_aggregate__".to_string(),
            source: self.network_label.clone(),
            reliability,
            volume: body.successful_transaction_count as f64,
            latency_ms: 0.0,
        };

        Ok(Some(FetchedLedger {
            sequence: body.sequence,
            hash: body.hash,
            prev_hash: body.prev_hash,
            rows: vec![row],
        }))
    }
}

#[derive(Clone, Debug)]
struct ChainLedger {
    hash: String,
    prev_hash: String,
    rows: Vec<RawSnapshotRow>,
}

/// Deterministic, in-memory chain of ledgers for tests.
///
/// Ledgers are appended one at a time on top of the current tip, with each
/// ledger's hash derived from its own tag plus its predecessor's hash — so
/// [`FakeLedgerSource::fork_from`] naturally produces a chain whose hash
/// linkage genuinely diverges from the original at and after the fork
/// point, exactly like a real reorg, rather than a canned response the
/// pipeline can't meaningfully detect anything from.
#[derive(Default)]
pub struct FakeLedgerSource {
    ledgers: tokio::sync::Mutex<BTreeMap<u64, ChainLedger>>,
}

const GENESIS_HASH: &str = "genesis";

impl FakeLedgerSource {
    pub fn new() -> Self {
        Self::default()
    }

    fn hash_of(sequence: u64, prev_hash: &str, tag: &str) -> String {
        fnv1a_hex(&format!("{sequence}:{prev_hash}:{tag}"))
    }

    /// Appends one ledger on top of the current tip and returns its
    /// sequence number. `tag` only needs to be distinct per fork branch —
    /// its value has no meaning beyond feeding the hash.
    pub async fn append(&self, tag: &str, rows: Vec<RawSnapshotRow>) -> u64 {
        let mut guard = self.ledgers.lock().await;
        let (tip_sequence, tip_hash) = guard
            .iter()
            .next_back()
            .map(|(seq, ledger)| (*seq, ledger.hash.clone()))
            .unwrap_or((0, GENESIS_HASH.to_string()));

        let sequence = tip_sequence + 1;
        let hash = Self::hash_of(sequence, &tip_hash, tag);
        guard.insert(
            sequence,
            ChainLedger {
                hash,
                prev_hash: tip_hash,
                rows,
            },
        );
        sequence
    }

    /// Simulates a reorg: discards every ledger at or after `from_sequence`
    /// and replaces them with a new fork built from `replacement`, chained
    /// on top of whatever ledger remains at `from_sequence - 1`.
    pub async fn fork_from(
        &self,
        from_sequence: u64,
        replacement: Vec<(&str, Vec<RawSnapshotRow>)>,
    ) {
        let mut guard = self.ledgers.lock().await;
        guard.retain(|seq, _| *seq < from_sequence);

        let mut tip_hash = guard
            .get(&(from_sequence - 1))
            .map(|ledger| ledger.hash.clone())
            .unwrap_or_else(|| GENESIS_HASH.to_string());

        for (index, (tag, rows)) in replacement.into_iter().enumerate() {
            let sequence = from_sequence + index as u64;
            let hash = Self::hash_of(sequence, &tip_hash, tag);
            guard.insert(
                sequence,
                ChainLedger {
                    hash: hash.clone(),
                    prev_hash: tip_hash,
                    rows,
                },
            );
            tip_hash = hash;
        }
    }
}

#[async_trait]
impl LedgerSource for FakeLedgerSource {
    async fn fetch_ledger(&self, sequence: u64) -> Result<Option<FetchedLedger>, IngestionError> {
        Ok(self
            .ledgers
            .lock()
            .await
            .get(&sequence)
            .map(|ledger| FetchedLedger {
                sequence,
                hash: ledger.hash.clone(),
                prev_hash: ledger.prev_hash.clone(),
                rows: ledger.rows.clone(),
            }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn row(corridor: &str) -> RawSnapshotRow {
        RawSnapshotRow {
            ledger_sequence: 0,
            corridor: corridor.to_string(),
            source: "testnet".to_string(),
            reliability: 1.0,
            volume: 1.0,
            latency_ms: 1.0,
        }
    }

    #[tokio::test]
    async fn fake_source_chains_appended_ledgers() {
        let source = FakeLedgerSource::new();
        let seq1 = source.append("a", vec![row("eur/usd")]).await;
        let seq2 = source.append("a", vec![row("eur/usd")]).await;

        assert_eq!((seq1, seq2), (1, 2));

        let ledger1 = source.fetch_ledger(1).await.unwrap().unwrap();
        let ledger2 = source.fetch_ledger(2).await.unwrap().unwrap();

        assert_eq!(ledger1.prev_hash, GENESIS_HASH);
        assert_eq!(ledger2.prev_hash, ledger1.hash);
    }

    #[tokio::test]
    async fn fetching_beyond_the_tip_returns_none() {
        let source = FakeLedgerSource::new();
        source.append("a", vec![row("eur/usd")]).await;

        assert_eq!(source.fetch_ledger(2).await.unwrap(), None);
    }

    #[tokio::test]
    async fn fork_from_changes_hash_linkage_from_the_fork_point() {
        let source = FakeLedgerSource::new();
        source.append("a", vec![row("eur/usd")]).await; // 1
        let before = source.append("a", vec![row("eur/usd")]).await; // 2
        source.append("a", vec![row("eur/usd")]).await; // 3

        let ledger1_before = source.fetch_ledger(1).await.unwrap().unwrap();

        source
            .fork_from(
                2,
                vec![("b", vec![row("gbp/usd")]), ("b", vec![row("gbp/usd")])],
            )
            .await;

        let ledger1_after = source.fetch_ledger(1).await.unwrap().unwrap();
        let ledger2_after = source.fetch_ledger(2).await.unwrap().unwrap();
        let ledger3_after = source.fetch_ledger(3).await.unwrap().unwrap();

        // Ledgers before the fork point are untouched.
        assert_eq!(ledger1_before, ledger1_after);
        assert_eq!(before, 2);
        // The forked ledgers carry the new fork's rows and a hash chain
        // that no longer matches the original branch.
        assert_eq!(ledger2_after.rows[0].corridor, "gbp/usd");
        assert_eq!(ledger3_after.prev_hash, ledger2_after.hash);
    }

    #[tokio::test]
    async fn horizon_source_parses_a_real_response_shape() {
        let mock_server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/ledgers/100"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "sequence": 100,
                "hash": "deadbeef",
                "prev_hash": "cafebabe",
                "successful_transaction_count": 9,
                "failed_transaction_count": 1
            })))
            .mount(&mock_server)
            .await;

        let source = HorizonLedgerSource::with_base_url(mock_server.uri(), "testnet".to_string());
        let fetched = source.fetch_ledger(100).await.unwrap().unwrap();

        assert_eq!(fetched.sequence, 100);
        assert_eq!(fetched.hash, "deadbeef");
        assert_eq!(fetched.prev_hash, "cafebabe");
        assert_eq!(fetched.rows.len(), 1);
        assert!((fetched.rows[0].reliability - 0.9).abs() < f64::EPSILON);
    }

    #[tokio::test]
    async fn horizon_source_treats_404_as_not_yet_closed() {
        let mock_server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/ledgers/999"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&mock_server)
            .await;

        let source = HorizonLedgerSource::with_base_url(mock_server.uri(), "testnet".to_string());
        assert_eq!(source.fetch_ledger(999).await.unwrap(), None);
    }

    #[tokio::test]
    async fn horizon_source_surfaces_server_errors() {
        let mock_server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/ledgers/1"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&mock_server)
            .await;

        let source = HorizonLedgerSource::with_base_url(mock_server.uri(), "testnet".to_string());
        assert!(source.fetch_ledger(1).await.is_err());
    }
}
