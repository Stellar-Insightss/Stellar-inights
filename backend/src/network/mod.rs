pub mod client;
pub mod identity;

use std::collections::HashMap;

pub use client::NetworkClient;
pub use identity::{Network, NetworkSchema, TableSchema};

#[derive(Clone, Debug, PartialEq)]
pub struct MetricRecord {
    pub corridor: String,
    pub network: Network,
    pub reliability: f64,
    pub volume: f64,
}

#[derive(Clone, Debug, Default)]
pub struct NetworkStore {
    by_network: HashMap<Network, Vec<MetricRecord>>,
}

impl NetworkStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn ingest(&mut self, corridor: String, network: Network, reliability: f64, volume: f64) {
        self.by_network
            .entry(network)
            .or_default()
            .push(MetricRecord {
                corridor,
                network,
                reliability,
                volume,
            });
    }

    pub fn for_network(&self, network: Network) -> Vec<MetricRecord> {
        self.by_network.get(&network).cloned().unwrap_or_default()
    }
}
