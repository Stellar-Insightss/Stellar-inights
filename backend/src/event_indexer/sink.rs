use std::sync::Arc;

use async_trait::async_trait;
use thiserror::Error;
use tokio::sync::RwLock;

use super::dispatch::NormalizedContractEvent;

#[derive(Debug, Error)]
pub enum AnalyticsSinkError {
    #[error("analytics sink rejected event: {0}")]
    Rejected(String),
}

/// Destination for normalized contract events consumed by the indexer.
#[async_trait]
pub trait AnalyticsSink: Send + Sync {
    async fn record(&self, event: &NormalizedContractEvent) -> Result<(), AnalyticsSinkError>;
}

/// Deterministic analytics sink for tests and local pipelines.
#[derive(Clone, Default)]
pub struct InMemoryAnalyticsSink {
    events: Arc<RwLock<Vec<NormalizedContractEvent>>>,
}

impl InMemoryAnalyticsSink {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn events(&self) -> Vec<NormalizedContractEvent> {
        self.events.read().await.clone()
    }
}

#[async_trait]
impl AnalyticsSink for InMemoryAnalyticsSink {
    async fn record(&self, event: &NormalizedContractEvent) -> Result<(), AnalyticsSinkError> {
        self.events.write().await.push(event.clone());
        Ok(())
    }
}
