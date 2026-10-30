//! Collection feeds — `/spore/clusters/{id}`, `/assets/objects/{id}` and
//! `/assets/identities/{id}` `…/activity-feed` and `…/transactions`:
//! paging at the source (ID-COL-1) and the collection-perspective row context
//! `collectionContext` (OB-C13).

use crate::common::*;
use ckbadger_store::types::{
    IdentityCollectionAggregate, IdentityStandard, ItemDelta, ItemKind,
    ObjectCollectionActivityEntry, ParticipantDelta, ParticipantId, DOTBIT_SENTINEL_COLLECTION,
};

/// The header hash `seed_block_headers_at` writes for `block`.
fn header_hash(block: i64) -> Vec<u8> {
    vec![(block % 251) as u8; 32]
}

fn seeded_hash(tag: u8, seed: u32) -> [u8; 32] {
    let mut hash = [tag; 32];
    hash[..4].copy_from_slice(&seed.to_be_bytes());
    hash
}

fn identity_delta(id: &[u8], standard: IdentityStandard, delta: i8) -> ItemDelta {
    ItemDelta {
        item_id: id.to_vec(),
        kind: ItemKind::Identity(standard),
        magnitude: 1,
        negative: delta < 0,
    }
}

fn participant(lock: u8, item_deltas: Vec<ItemDelta>) -> ParticipantDelta {
    ParticipantDelta {
        id: ParticipantId::lock(&[lock; 32]).unwrap(),
        ckb_delta: 0,
        used_delta: 0,
        item_deltas,
        tags: 0,
        roles: 0,
    }
}

/// One committed transaction of a collection feed fixture.
struct FeedTx {
    block: i64,
    tx_idx: i32,
    hash: [u8; 32],
    outputs: i16,
    participants: Vec<ParticipantDelta>,
    protocol_actions: Vec<ProtocolAction>,
}

/// A plain output payload owned by `lock` (no type script).
fn plain_cell(lock: u8) -> LiveCellInfo {
    let code = vec![0x41; 32];
    let args = vec![lock; 20];
    LiveCellInfo {
        capacity: 200_00000000,
        occupied_capacity: 61_00000000,
        lock_script_hash: compute_script_hash(&code, 1, &args),
        lock_code_hash: code,
        lock_hash_type: 1,
        lock_args: args,
        type_script_hash: None,
        type_code_hash: None,
        type_hash_type: None,
        type_args: None,
        data_size: 0,
        udt_amount: None,
        data_hash: None,
    }
}

/// Location, index row, `TxActions` and output payloads of `tx` — what the
/// canonical feed projections read for one row.
fn put_feed_tx(batch: &mut StoreBatch, tx: &FeedTx) {
    let timestamp = 1_700_000_000_000 + tx.block * 1_000;
    for output in 0..tx.outputs {
        batch.put_cell_payload_by_outpoint(&tx.hash, output, &plain_cell(0x11));
    }
    batch.put_tx_hash_map(&tx.hash, tx.block, tx.tx_idx);
    batch.put_tx_index(
        tx.block,
        tx.tx_idx,
        &TxIndexEntry {
            tx_hash: tx.hash,
            is_cellbase: false,
            timestamp,
            inputs_count: 1,
            outputs_count: tx.outputs,
            fee: 1_000,
            tx_size: 300,
            cycles: Some(1_000),
            semantic_tags: 0,
        },
    );
    batch.put_tx_actions(&TxActions {
        tx_hash: tx.hash.to_vec(),
        block_hash: header_hash(tx.block),
        block_number: tx.block,
        tx_index: tx.tx_idx,
        timestamp,
        is_cellbase: false,
        protocol_actions: tx.protocol_actions.clone(),
        type_calls: vec![],
        lock_calls: vec![],
        participants: tx.participants.clone(),
    });
}

fn collection_activity(tx: &FeedTx, actions: Vec<AssetAction>) -> ObjectCollectionActivityEntry {
    ObjectCollectionActivityEntry {
        tx_hash: tx.hash.to_vec(),
        block_hash: header_hash(tx.block),
        timestamp_ms: 1_700_000_000_000 + tx.block * 1_000,
        actions,
    }
}

/// A `.bit` collection whose history is `n` registrations, two per block,
/// each minting one account to lock `0x11…`. Returns the rows newest first as
/// `(block, tx_index, tx_hash)`.
fn seed_dotbit_registrations(store: &Arc<CkbadgerStore>, n: u32) -> Vec<(i64, i32, String)> {
    let blocks: Vec<(i64, i64)> = (0..=i64::from(n / 2 + 1))
        .map(|block| (block, 1_700_000_000_000 + block * 1_000))
        .collect();
    seed_block_headers_at(store, &blocks);
    let mut batch = StoreBatch::new(store.as_ref());
    let mut rows = Vec::new();
    for seed in 0..n {
        let account = &seeded_hash(0xA0, seed)[..20];
        let tx = FeedTx {
            block: 1 + i64::from(seed / 2),
            tx_idx: (seed % 2) as i32,
            hash: seeded_hash(0xB0, seed),
            outputs: 1,
            participants: vec![participant(
                0x11,
                vec![identity_delta(account, IdentityStandard::DotBit, 1)],
            )],
            protocol_actions: vec![],
        };
        put_feed_tx(&mut batch, &tx);
        batch.put_dotbit_account_outpoint(&tx.hash, 0, account);
        batch.put_dotbit_outpoint_by_account_id(account, &tx.hash, 0, AssetAction::Mint);
        batch.put_identity_collection_activity(
            &DOTBIT_SENTINEL_COLLECTION,
            tx.block,
            tx.tx_idx,
            &collection_activity(&tx, vec![AssetAction::Mint]),
        );
        rows.push((tx.block, tx.tx_idx, format!("0x{}", hex::encode(tx.hash))));
    }
    batch.put_identity_collection_aggregate(
        &DOTBIT_SENTINEL_COLLECTION,
        &IdentityCollectionAggregate {
            name: Some(".bit".to_string()),
            standard: IdentityStandard::DotBit,
            total_count: i64::from(n),
            live_count: i64::from(n),
            holders_count: 1,
            activities_count: i64::from(n),
        },
    );
    batch.commit().unwrap();
    rows.sort_by_key(|(block, index, _)| std::cmp::Reverse((*block, *index)));
    rows
}

/// A router over `store` with no warmup and no background loops, so the
/// store's read counters see only the request under test.
fn quiet_router(store: Arc<CkbadgerStore>) -> axum::Router {
    router_from_state(test_app_state(test_config(store)))
}

/// ID-COL-1: the `.bit` collection page (653,018 activities) never answered
/// because the feed read the collection's entire history before paging. A
/// page's store reads must depend on `limit`, not on how long the history is.
#[tokio::test]
async fn collection_feed_page_reads_do_not_grow_with_collection_history() {
    let mut reads = Vec::new();
    for n in [30, 2_100] {
        let store = test_store();
        seed_dotbit_registrations(&store, n);
        let app = quiet_router(store.clone());
        for route in ["activity-feed", "transactions"] {
            store.reset_read_call_counters();
            let (status, page) =
                get_json(&app, &format!("/assets/identities/dotbit/{route}?limit=10")).await;
            assert_eq!(status, StatusCode::OK, "{route} n={n}: {page}");
            assert_eq!(page["data"].as_array().unwrap().len(), 10, "{page}");
            assert_eq!(page["total"], n, "{route} n={n}");
            reads.push((route, n, store.read_call_counts()));
        }
    }
    for route in ["activity-feed", "transactions"] {
        let counts: Vec<_> = reads
            .iter()
            .filter(|(r, _, _)| *r == route)
            .map(|(_, n, counts)| (*n, *counts))
            .collect();
        assert_eq!(
            counts[0].1, counts[1].1,
            "{route}: page-1 (get_cf, multi_get_cf) reads grew with history: {counts:?}"
        );
    }
}

/// Walking every page from `nextCursor` yields the whole history newest
/// first, with no duplicate and no gap, and the last page has no cursor.
#[tokio::test]
async fn collection_feed_cursor_walks_the_history_without_gaps() {
    let store = test_store();
    let expected = seed_dotbit_registrations(&store, 35);
    let app = quiet_router(store);
    for route in ["activity-feed", "transactions"] {
        let hash_key = if route == "transactions" {
            "hash"
        } else {
            "txHash"
        };
        let mut seen = Vec::new();
        let mut cursor: Option<String> = None;
        let mut pages = 0;
        loop {
            let path = match &cursor {
                Some(cursor) => {
                    format!("/assets/identities/dotbit/{route}?limit=10&cursor={cursor}")
                }
                None => format!("/assets/identities/dotbit/{route}?limit=10"),
            };
            let (status, page) = get_json(&app, &path).await;
            assert_eq!(status, StatusCode::OK, "{path}: {page}");
            pages += 1;
            for row in page["data"].as_array().unwrap() {
                seen.push(row[hash_key].as_str().unwrap().to_string());
            }
            assert_eq!(page["hasMore"], !page["nextCursor"].is_null(), "{page}");
            match page["nextCursor"].as_str() {
                Some(next) => cursor = Some(next.to_string()),
                None => break,
            }
            assert!(pages < 10, "{route}: cursor never ends");
        }
        assert_eq!(pages, 4, "{route}");
        let expected: Vec<_> = expected.iter().map(|(_, _, hash)| hash.clone()).collect();
        assert_eq!(seen, expected, "{route}: pages must tile the history");
    }
}

fn object_delta(id: &[u8], delta: i8) -> ItemDelta {
    ItemDelta {
        item_id: id.to_vec(),
        kind: ItemKind::Object,
        magnitude: 1,
        negative: delta < 0,
    }
}

fn spore_entry(cluster: &[u8]) -> ObjectEntry {
    ObjectEntry {
        standard: ObjectStandard::Spore,
        collection_id: Some(cluster.to_vec()),
        token_id: None,
        owner_lock_hash: Some(vec![0x22; 32]),
        name: None,
        description: None,
        is_live: true,
        created_at_block: 1,
        created_at_tx: vec![0x99; 32],
        extra: ObjectExtra::Spore {
            content_type: "image/png".to_string(),
            content_length: 10,
            media_profile: Default::default(),
        },
    }
}

fn cluster_entry(name: &str) -> ObjectEntry {
    ObjectEntry {
        standard: ObjectStandard::SporeCluster,
        collection_id: None,
        token_id: None,
        owner_lock_hash: Some(vec![0x11; 32]),
        name: Some(name.to_string()),
        description: None,
        is_live: true,
        created_at_block: 0,
        created_at_tx: vec![0x98; 32],
        extra: ObjectExtra::SporeCluster,
    }
}

fn row_of<'a>(page: &'a serde_json::Value, key: &str, hash: &[u8; 32]) -> &'a serde_json::Value {
    let hash = format!("0x{}", hex::encode(hash));
    page["data"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row[key] == hash.as_str())
        .unwrap_or_else(|| panic!("row {hash} missing: {page}"))
}

const NERVAPE: [u8; 32] = [0xCA; 32];
const OTHER_CLUSTER: [u8; 32] = [0xCB; 32];
/// Block 18,883,769's shape: one transaction moves Nervape spores (12
/// transferred, 1 burned) and mints one spore of another cluster.
const MIXED_TX: [u8; 32] = [0xF9; 32];

fn nervape_spore(n: u8) -> [u8; 32] {
    let mut id = [0x51; 32];
    id[31] = n;
    id
}
const OTHER_SPORE: [u8; 32] = [0x85; 32];

fn seed_two_cluster_transaction(store: &Arc<CkbadgerStore>) {
    seed_block_headers_at(store, &[(0, 1_700_000_000_000), (5, 1_700_000_005_000)]);
    let mut batch = StoreBatch::new(store.as_ref());
    for (cluster, name, total) in [(NERVAPE, "Nervape", 13), (OTHER_CLUSTER, "Other", 1)] {
        batch.put_spore(&cluster, &cluster_entry(name));
        batch.put_cluster_aggregate(
            &cluster,
            &ClusterAggregate {
                name: Some(name.to_string()),
                total_count: total,
                live_count: total,
                ..Default::default()
            },
        );
    }
    let mut sender = Vec::new();
    let mut receiver = Vec::new();
    for n in 0..13u8 {
        let id = nervape_spore(n);
        batch.put_spore(&id, &spore_entry(&NERVAPE));
        batch.put_spore_by_cluster(&NERVAPE, &id);
        sender.push(object_delta(&id, -1));
        // Spores 0..12 move to the receiver; spore 12 is burned.
        if n < 12 {
            receiver.push(object_delta(&id, 1));
            batch.put_spore_outpoint(&MIXED_TX, i16::from(n), &id);
        }
    }
    batch.put_spore(&OTHER_SPORE, &spore_entry(&OTHER_CLUSTER));
    batch.put_spore_by_cluster(&OTHER_CLUSTER, &OTHER_SPORE);
    batch.put_spore_outpoint(&MIXED_TX, 12, &OTHER_SPORE);
    let tx = FeedTx {
        block: 5,
        tx_idx: 3,
        hash: MIXED_TX,
        outputs: 13,
        participants: vec![
            participant(0x31, sender),
            participant(0x32, receiver),
            participant(0x33, vec![object_delta(&OTHER_SPORE, 1)]),
        ],
        protocol_actions: vec![],
    };
    put_feed_tx(&mut batch, &tx);
    batch.put_object_collection_activity(
        &NERVAPE,
        5,
        3,
        &collection_activity(&tx, vec![AssetAction::Transfer, AssetAction::Burn]),
    );
    batch.put_object_collection_activity(
        &OTHER_CLUSTER,
        5,
        3,
        &collection_activity(&tx, vec![AssetAction::Mint]),
    );
    batch.commit().unwrap();
}

/// OB-C13: a Nervape collection row read "Object mint" because the frontend
/// labelled it from the transaction's first item delta — another cluster's
/// mint. Each collection feed row now says what the transaction did to the
/// items of THAT collection, computed once by the API.
#[tokio::test]
async fn collection_feed_rows_carry_their_own_collection_context() {
    let store = test_store();
    seed_two_cluster_transaction(&store);
    let app = quiet_router(store);
    let nervape_items: Vec<_> = (0..13u8)
        .map(|n| {
            serde_json::json!({
                "id": format!("0x{}", hex::encode(nervape_spore(n))),
                "change": if n < 12 { "transferred" } else { "burned" },
            })
        })
        .collect();
    for (prefix, cluster, expected) in [
        (
            "spore/clusters",
            NERVAPE,
            serde_json::json!({
                "action": "mixed", "minted": 0, "burned": 1, "transferred": 12, "updated": 0,
                "items": nervape_items,
            }),
        ),
        (
            "assets/objects",
            NERVAPE,
            serde_json::json!({
                "action": "mixed", "minted": 0, "burned": 1, "transferred": 12, "updated": 0,
                "items": nervape_items,
            }),
        ),
        (
            "assets/objects",
            OTHER_CLUSTER,
            serde_json::json!({
                "action": "mint", "minted": 1, "burned": 0, "transferred": 0, "updated": 0,
                "items": [{"id": format!("0x{}", hex::encode(OTHER_SPORE)), "change": "minted"}],
            }),
        ),
    ] {
        let base = format!("/{prefix}/0x{}", hex::encode(cluster));
        for (route, key) in [("activity-feed", "txHash"), ("transactions", "hash")] {
            let (status, page) = get_json(&app, &format!("{base}/{route}")).await;
            assert_eq!(status, StatusCode::OK, "{base}/{route}: {page}");
            assert_eq!(
                row_of(&page, key, &MIXED_TX)["collectionContext"],
                expected,
                "{base}/{route}"
            );
        }
    }
}

/// A `.bit` renewal keeps the account with its owner (no item delta); from
/// the collection's side the account was updated.
#[tokio::test]
async fn identity_collection_renewal_is_an_update() {
    let store = test_store();
    seed_block_headers_at(
        store.as_ref(),
        &[(0, 1_700_000_000_000), (7, 1_700_000_007_000)],
    );
    let account = [0x2D; 20];
    let renew_tx = [0xE7; 32];
    let mut batch = StoreBatch::new(store.as_ref());
    let tx = FeedTx {
        block: 7,
        tx_idx: 1,
        hash: renew_tx,
        outputs: 1,
        participants: vec![participant(0x11, vec![])],
        protocol_actions: vec![],
    };
    put_feed_tx(&mut batch, &tx);
    batch.put_dotbit_account_outpoint(&renew_tx, 0, &account);
    batch.put_dotbit_outpoint_by_account_id(&account, &renew_tx, 0, AssetAction::Renew);
    batch.put_identity_collection_activity(
        &DOTBIT_SENTINEL_COLLECTION,
        7,
        1,
        &collection_activity(&tx, vec![AssetAction::Renew]),
    );
    batch.put_identity_collection_aggregate(
        &DOTBIT_SENTINEL_COLLECTION,
        &IdentityCollectionAggregate {
            name: Some(".bit".to_string()),
            standard: IdentityStandard::DotBit,
            total_count: 1,
            live_count: 1,
            holders_count: 1,
            activities_count: 1,
        },
    );
    batch.commit().unwrap();
    let app = quiet_router(store);
    for (route, key) in [("activity-feed", "txHash"), ("transactions", "hash")] {
        let (status, page) = get_json(&app, &format!("/assets/identities/dotbit/{route}")).await;
        assert_eq!(status, StatusCode::OK, "{page}");
        assert_eq!(
            row_of(&page, key, &renew_tx)["collectionContext"],
            serde_json::json!({
                "action": "update", "minted": 0, "burned": 0, "transferred": 0, "updated": 1,
                "items": [{"id": format!("0x{}", hex::encode(account)), "change": "updated"}],
            }),
            "{route}"
        );
    }
}
