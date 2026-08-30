use super::identity::Network;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NetworkClient {
    pub network: Network,
    pub rpc_url: String,
    pub horizon_url: String,
}

impl NetworkClient {
    pub fn for_network(network: Network) -> Self {
        let (rpc_url, horizon_url) = match network {
            Network::Mainnet => (
                "https://horizon-mainnet.stellar.org".to_string(),
                "https://horizon-mainnet.stellar.org".to_string(),
            ),
            Network::Testnet => (
                "https://horizon-testnet.stellar.org".to_string(),
                "https://horizon-testnet.stellar.org".to_string(),
            ),
            Network::Futurenet => (
                "https://horizon-futurenet.stellar.org".to_string(),
                "https://horizon-futurenet.stellar.org".to_string(),
            ),
        };

        Self {
            network,
            rpc_url,
            horizon_url,
        }
    }
}
