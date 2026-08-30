use stellar_insights_backend::network::{
    identity::{Network, NetworkSchema, TableSchema},
    NetworkStore,
};

#[test]
fn dual_network_ingestion_keeps_network_data_isolated() {
    let mut store = NetworkStore::new();

    store.ingest(
        "corridor_a".to_string(),
        Network::Testnet,
        0.85,
        100.0,
    );
    store.ingest(
        "corridor_b".to_string(),
        Network::Mainnet,
        0.92,
        220.0,
    );

    let testnet = store.for_network(Network::Testnet);
    let mainnet = store.for_network(Network::Mainnet);

    assert_eq!(testnet.len(), 1);
    assert_eq!(mainnet.len(), 1);
    assert_eq!(testnet[0].corridor, "corridor_a");
    assert_eq!(mainnet[0].corridor, "corridor_b");
    assert_ne!(testnet[0].reliability, mainnet[0].reliability);
}

#[test]
fn startup_schema_rejects_missing_network_discriminator() {
    let schema = vec![TableSchema {
        name: "corridor_metrics".to_string(),
        fields: vec!["corridor".to_string(), "reliability".to_string()],
    }];

    let err = NetworkSchema::validate(&schema).unwrap_err();
    assert!(err.contains("network"));
}
