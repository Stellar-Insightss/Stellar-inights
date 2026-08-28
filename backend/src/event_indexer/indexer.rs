use stellar_xdr::ContractEvent;
use thiserror::Error;

use super::{
    dispatch::{dispatch, DispatchError, NormalizedContractEvent},
    sink::{AnalyticsSink, AnalyticsSinkError},
};

#[derive(Debug, Error)]
pub enum IndexerError {
    #[error(transparent)]
    Dispatch(#[from] DispatchError),
    #[error(transparent)]
    Analytics(#[from] AnalyticsSinkError),
}

/// Consumes protocol-native Soroban events and feeds normalized analytics.
pub struct EventIndexer<S> {
    sink: S,
}

impl<S> EventIndexer<S>
where
    S: AnalyticsSink,
{
    pub fn new(sink: S) -> Self {
        Self { sink }
    }

    pub fn sink(&self) -> &S {
        &self.sink
    }

    pub async fn index(
        &self,
        event: &ContractEvent,
    ) -> Result<NormalizedContractEvent, IndexerError> {
        let normalized = dispatch(event)?;
        self.sink.record(&normalized).await?;
        Ok(normalized)
    }

    /// Index events sequentially to preserve their ledger-provided order.
    pub async fn index_batch<'a, I>(
        &self,
        events: I,
    ) -> Result<Vec<NormalizedContractEvent>, IndexerError>
    where
        I: IntoIterator<Item = &'a ContractEvent>,
    {
        let mut normalized = Vec::new();
        for event in events {
            normalized.push(self.index(event).await?);
        }
        Ok(normalized)
    }
}
