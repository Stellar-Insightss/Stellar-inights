use analytics::{AnalyticsContract, AnalyticsContractClient, PERSISTENT_LIVE_KEYS};
use soroban_sdk::{testutils::Address as _, Address, BytesN, Env, Map, Symbol};

fn metrics(env: &Env, daa: i128, volume: i128) -> Map<Symbol, i128> {
    let mut m = Map::new(env);
    m.set(Symbol::new(env, "daa"), daa);
    m.set(Symbol::new(env, "volume"), volume);
    m
}

fn hash(env: &Env, seed: u8) -> BytesN<32> {
    BytesN::from_array(env, &[seed; 32])
}

fn setup<'a>(env: &'a Env) -> (AnalyticsContractClient<'a>, Address) {
    env.mock_all_auths();
    let admin = Address::generate(env);
    let id = env.register(AnalyticsContract, ());
    let client = AnalyticsContractClient::new(env, &id);
    client.initialize(&admin);
    (client, admin)
}

#[test]
fn storage_footprint_stays_bounded_across_many_snapshots() {
    let env = Env::default();
    let (client, admin) = setup(&env);

    assert_eq!(client.persistent_entry_count(), 0);

    const N: u64 = 64;
    let mut last_entries = 0u32;

    for epoch in 1..=N {
        let receipt = client.submit_snapshot(
            &admin,
            &epoch,
            &metrics(&env, epoch as i128 * 10, epoch as i128 * 1_000),
            &hash(&env, epoch as u8),
            &hash(&env, 200),
        );

        assert_eq!(receipt.persistent_entries, PERSISTENT_LIVE_KEYS);
        assert_eq!(client.persistent_entry_count(), PERSISTENT_LIVE_KEYS);

        // Footprint must not grow with submission count.
        if epoch > 1 {
            assert_eq!(
                receipt.persistent_entries, last_entries,
                "persistent entries grew at epoch {epoch}"
            );
        }
        last_entries = receipt.persistent_entries;

        let prev = client.previous_snapshot().expect("retained snapshot");
        assert_eq!(prev.epoch, epoch, "only the latest snapshot is retained");
        assert_eq!(client.latest_proof().unwrap().epoch, epoch);
    }

    // Linear history would retain N snapshots; we retain exactly one.
    assert_eq!(client.persistent_entry_count(), 1);
    assert_eq!(client.previous_snapshot().unwrap().epoch, N);
}

#[test]
fn first_ingest_is_genesis_diff_then_overwrite_in_place() {
    let env = Env::default();
    let (client, admin) = setup(&env);

    let r1 = client.submit_snapshot(
        &admin,
        &1,
        &metrics(&env, 10, 100),
        &hash(&env, 1),
        &hash(&env, 9),
    );
    assert_eq!(r1.from_epoch, 0);
    assert_eq!(r1.added_count, 2);
    assert_eq!(r1.changed_count, 0);
    assert_eq!(r1.persistent_entries, 1);

    let r2 = client.submit_snapshot(
        &admin,
        &2,
        &metrics(&env, 11, 100),
        &hash(&env, 2),
        &hash(&env, 9),
    );
    assert_eq!(r2.from_epoch, 1);
    assert_eq!(r2.changed_count, 1);
    assert_eq!(r2.added_count, 0);
    assert_eq!(r2.persistent_entries, 1);
    assert_eq!(client.previous_snapshot().unwrap().epoch, 2);
}

#[test]
fn pause_blocks_ingest() {
    let env = Env::default();
    let (client, admin) = setup(&env);
    client.pause(&admin);
    assert!(client.is_paused());
    let result = client.try_submit_snapshot(
        &admin,
        &1,
        &metrics(&env, 1, 1),
        &hash(&env, 1),
        &hash(&env, 1),
    );
    assert!(result.is_err());
    client.unpause(&admin);
    client.submit_snapshot(
        &admin,
        &1,
        &metrics(&env, 1, 1),
        &hash(&env, 1),
        &hash(&env, 1),
    );
}
