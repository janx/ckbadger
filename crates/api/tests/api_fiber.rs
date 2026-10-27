mod common;
use common::*;

use ckbadger_store::types::{FiberChannel, FiberChannelState};

fn export_fiber_fixture(name: &str, body: &serde_json::Value) {
    if let Ok(dir) = std::env::var("D7_FIXTURE_DIR") {
        std::fs::create_dir_all(&dir).unwrap();
        let mut bytes = serde_json::to_vec_pretty(body).unwrap();
        bytes.push(b'\n');
        std::fs::write(format!("{dir}/{name}.json"), bytes).unwrap();
    }
}

fn seed_fiber_headers(store: &Arc<CkbadgerStore>) {
    seed_block_headers_at(
        store,
        &[
            (10, 1_700_000_000_000),
            (20, 1_700_000_100_000),
            (30, 1_700_000_200_000),
        ],
    );
    store
        .update_sync_status(|status| status.tip_block_number = 30)
        .unwrap();
}

/// Regression (API-020, typed responses): a force-closed channel's
/// `delay_epoch` is the commitment lock's `since` value with its relative-epoch
/// flag bits set (`0xa001000000000006` on real commitment locks), far above
/// 2^53. Served as a JSON number it was rounded in the browser; it is an exact
/// decimal string now, in the list and the detail.
#[tokio::test]
async fn test_fiber_channel_delay_epoch_is_an_exact_decimal_string() {
    let store = test_store();
    seed_fiber_headers(&store);
    let channel_id = vec![0x5a; 32];
    let channel = FiberChannel {
        funding_tx_hash: vec![0x11; 32],
        funding_output_index: 0,
        state: FiberChannelState::ForceClosed,
        capacity: 100_000_000_000,
        udt_type_hash: None,
        udt_amount: None,
        open_block: 10,
        open_timestamp: 1_700_000_000_000,
        close_tx_hash: Some(vec![0x22; 32]),
        close_block: Some(20),
        close_timestamp: Some(1_700_000_100_000),
        commitment_tx_hash: Some(vec![0x33; 32]),
        commitment_output_index: Some(0),
        delay_epoch: Some(0xa001_0000_0000_0006),
        settlement_tx_hash: None,
        settlement_block: None,
        settlement_timestamp: None,
        participants: vec![],
        funding_lock_args: vec![0x44; 20],
    };
    let mut batch = StoreBatch::new(store.as_ref());
    batch.put_fiber_channel(&channel_id, &channel);
    batch.commit().unwrap();
    let app = create_router(test_config(store)).await;

    let (status, list) = get_json(&app, "/fiber/channels").await;
    assert_eq!(status, StatusCode::OK, "{list}");
    assert_eq!(
        list["data"][0]["delayEpoch"], "11529496521045180422",
        "{list}"
    );

    let (status, detail) = get_json(
        &app,
        &format!("/fiber/channels/0x{}", hex::encode(&channel_id)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{detail}");
    assert_eq!(detail["delayEpoch"], "11529496521045180422", "{detail}");
}

#[tokio::test]
async fn redesign_fiber_timeline_matches_frontend_and_delay_is_decoded() {
    let store = test_store();
    seed_fiber_headers(&store);
    let id = [0x6b; 32];
    let channel = FiberChannel {
        funding_tx_hash: vec![0x11; 32],
        funding_output_index: 0,
        state: FiberChannelState::ForceClosed,
        capacity: 100_000_000_000,
        udt_type_hash: None,
        udt_amount: None,
        open_block: 10,
        open_timestamp: 1_700_000_000_000,
        close_tx_hash: Some(vec![0x22; 32]),
        close_block: Some(20),
        close_timestamp: Some(1_700_000_100_000),
        commitment_tx_hash: Some(vec![0x33; 32]),
        commitment_output_index: Some(0),
        delay_epoch: Some(0xa000_0100_0000_0001),
        settlement_tx_hash: None,
        settlement_block: None,
        settlement_timestamp: None,
        participants: vec![],
        funding_lock_args: vec![0x44; 20],
    };
    let mut batch = StoreBatch::new(&store);
    batch.put_fiber_channel(&id, &channel);
    batch.commit().unwrap();
    let app = create_router(test_config(store)).await;
    let (status, body) = get_json(&app, &format!("/fiber/channels/0x{}", hex::encode(id))).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["timeline"][0]["blockNumber"], 10);
    assert_eq!(body["delayEpochDecoded"]["relative"], true);
    assert_eq!(body["delayEpochDecoded"]["epochs"], 1);
    assert_eq!(body["delayEpochDecoded"]["index"], 0);
    assert_eq!(body["delayEpochDecoded"]["length"], 1);
    export_fiber_fixture("fiber-force-closed", &body);
}

#[tokio::test]
async fn fiber_network_aggregates_exact_pairs_states_and_chain_lifetimes() {
    let store = test_store();
    seed_fiber_headers(&store);
    let party_a = vec![0xaa; 32];
    let party_b = vec![0xbb; 32];
    let base = FiberChannel {
        funding_tx_hash: vec![0x11; 32],
        funding_output_index: 0,
        state: FiberChannelState::Open,
        capacity: 300_000_000_000,
        udt_type_hash: None,
        udt_amount: None,
        open_block: 10,
        open_timestamp: 1_700_000_000_000,
        close_tx_hash: None,
        close_block: None,
        close_timestamp: None,
        commitment_tx_hash: None,
        commitment_output_index: None,
        delay_epoch: None,
        settlement_tx_hash: None,
        settlement_block: None,
        settlement_timestamp: None,
        participants: vec![party_a.clone(), party_b.clone()],
        funding_lock_args: vec![0x44; 20],
    };
    let mut cooperative = base.clone();
    cooperative.state = FiberChannelState::CooperativelyClosed;
    cooperative.capacity = 200_000_000_000;
    cooperative.close_tx_hash = Some(vec![0x22; 32]);
    cooperative.close_block = Some(20);
    cooperative.close_timestamp = Some(1_700_000_100_000);
    let mut waiting = cooperative.clone();
    waiting.state = FiberChannelState::ForceClosed;
    waiting.capacity = 100_000_000_000;
    waiting.close_tx_hash = Some(vec![0x33; 32]);
    waiting.commitment_tx_hash = Some(vec![0x44; 32]);
    waiting.commitment_output_index = Some(0);
    waiting.delay_epoch = Some(0xa000_0100_0000_0001);
    waiting.participants = vec![party_a.clone()];
    let mut batch = StoreBatch::new(&store);
    batch.put_fiber_channel(&[0x01; 32], &base);
    batch.put_fiber_channel(&[0x02; 32], &cooperative);
    batch.put_fiber_channel(&[0x03; 32], &waiting);
    batch.commit().unwrap();
    let app = create_router(test_config(store)).await;
    let (status, body) = get_json(&app, "/fiber/network").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["summary"]["totalChannels"], 3);
    assert_eq!(body["summary"]["openChannels"], 1);
    assert_eq!(body["summary"]["totalCapacityLocked"], "300000000000");
    assert_eq!(body["summary"]["cooperativelyClosedChannels"], 1);
    assert_eq!(body["summary"]["forceClosedWaitingChannels"], 1);
    assert_eq!(body["summary"]["typicalClosedLifetimeMs"], "100000");
    assert_eq!(body["summary"]["observedParties"], 2);
    assert_eq!(body["summary"]["observedPairs"], 1);
    assert_eq!(body["pairs"][0]["channels"], 2);
    assert_eq!(body["pairs"][0]["openCapacity"], "300000000000");
    assert_eq!(body["parties"][0]["address"], serde_json::Value::Null);
    let (status, open) = get_json(
        &app,
        &format!("/fiber/channels/0x{}", hex::encode([0x01; 32])),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{open}");
    assert_eq!(open["observedLifetimeMs"], "200000");
    assert_eq!(open["lifetimeAnchorBlock"], 30);
    let (evidence_status, evidence_error) = get_json(
        &app,
        &format!("/fiber/channels/0x{}/evidence", hex::encode([0x01; 32])),
    )
    .await;
    assert_eq!(
        evidence_status,
        StatusCode::INTERNAL_SERVER_ERROR,
        "{evidence_error}"
    );
    export_fiber_fixture("fiber-network", &body);
    export_fiber_fixture("fiber-open", &open);
}

#[tokio::test]
async fn fiber_lifetime_uses_chain_time_when_raw_header_clock_rolls_back() {
    let store = test_store();
    seed_block_headers_at(&store, &[(10, 2_000), (20, 1_500), (30, 3_000)]);
    store
        .update_sync_status(|status| status.tip_block_number = 30)
        .unwrap();
    let channel = FiberChannel {
        funding_tx_hash: vec![0x11; 32],
        funding_output_index: 0,
        state: FiberChannelState::CooperativelyClosed,
        capacity: 100_000_000_000,
        udt_type_hash: None,
        udt_amount: None,
        open_block: 10,
        open_timestamp: 2_000,
        close_tx_hash: Some(vec![0x22; 32]),
        close_block: Some(20),
        close_timestamp: Some(1_500),
        commitment_tx_hash: None,
        commitment_output_index: None,
        delay_epoch: None,
        settlement_tx_hash: None,
        settlement_block: None,
        settlement_timestamp: None,
        participants: vec![],
        funding_lock_args: vec![0x44; 20],
    };
    let mut batch = StoreBatch::new(&store);
    batch.put_fiber_channel(&[0x04; 32], &channel);
    batch.commit().unwrap();
    let app = create_router(test_config(store)).await;
    let (status, body) = get_json(
        &app,
        &format!("/fiber/channels/0x{}", hex::encode([0x04; 32])),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["observedLifetimeMs"], "0");
    assert_eq!(body["closeTimestamp"], "1500");
}

/// A node-backed producer proves the complete read-only evidence contract:
/// canonical previous owner input, exact funding output, and the local node's
/// original transaction bytes. Net input includes both funding and tx fee.
#[tokio::test]
async fn fiber_open_evidence_uses_canonical_node_bytes_and_party_flows() {
    use ckb_types::{
        bytes::Bytes,
        core::{
            BlockBuilder, Capacity, EpochNumberWithFraction, ScriptHashType, TransactionBuilder,
        },
        packed,
        prelude::*,
    };
    let owner_code = vec![0x11; 32];
    let owner_args = vec![0xaa; 20];
    let funding_code = hex::decode(
        ckbadger_indexer::parser::fiber::FUNDING_LOCK_CODE_HASH_MAINNET.trim_start_matches("0x"),
    )
    .unwrap();
    let funding_args = vec![0x44; 20];
    let script = |code: &[u8], args: &[u8]| {
        packed::Script::new_builder()
            .code_hash(packed::Byte32::from_slice(code).unwrap())
            .hash_type(ScriptHashType::Type.into())
            .args(Bytes::from(args.to_vec()).pack())
            .build()
    };
    let owner = script(&owner_code, &owner_args);
    let funding_lock = script(&funding_code, &funding_args);
    let previous_tx = TransactionBuilder::default()
        .output(
            packed::CellOutput::new_builder()
                .capacity(Capacity::shannons(100_000_000_000).pack())
                .lock(owner.clone())
                .build(),
        )
        .output_data(Bytes::new().pack())
        .build();
    let previous_hash: [u8; 32] = previous_tx.hash().unpack();
    let input = packed::CellInput::new_builder()
        .previous_output(
            packed::OutPoint::new_builder()
                .tx_hash(packed::Byte32::new(previous_hash))
                .index(0u32.pack())
                .build(),
        )
        .build();
    let opening_tx = TransactionBuilder::default()
        .input(input)
        .output(
            packed::CellOutput::new_builder()
                .capacity(Capacity::shannons(30_000_000_000).pack())
                .lock(funding_lock)
                .build(),
        )
        .output_data(Bytes::new().pack())
        .output(
            packed::CellOutput::new_builder()
                .capacity(Capacity::shannons(69_900_000_000).pack())
                .lock(owner)
                .build(),
        )
        .output_data(Bytes::new().pack())
        .build();
    let opening_hash: [u8; 32] = opening_tx.hash().unpack();
    let epoch = EpochNumberWithFraction::new(1, 0, 1800).pack();
    let previous_block = BlockBuilder::default()
        .number(9u64.pack())
        .epoch(epoch.clone())
        .transaction(previous_tx)
        .build();
    let opening_block = BlockBuilder::default()
        .number(10u64.pack())
        .epoch(epoch)
        .transaction(opening_tx)
        .build();
    let chain = seed_ckb_chain(&[previous_block, opening_block]);
    let store = test_store();
    seed_block_headers_at(&store, &[(9, 1_700_000_000_000), (10, 1_700_000_001_000)]);
    store
        .update_sync_status(|status| status.tip_block_number = 10)
        .unwrap();
    let owner_hash = compute_script_hash(&owner_code, 1, &owner_args);
    // The indexer stores the CKB Blake2b digest even for empty data. The
    // protocol's CellOutput::calc_data_hash instead uses a zero sentinel.
    let empty_data_hash =
        ckbadger_indexer::parser::script::ScriptParser::compute_data_hash(&[]).to_vec();
    assert_ne!(empty_data_hash, vec![0; 32]);
    let previous_cell = LiveCellInfo {
        capacity: 100_000_000_000,
        occupied_capacity: 6_100_000_000,
        lock_script_hash: owner_hash.clone(),
        lock_code_hash: owner_code,
        lock_hash_type: 1,
        lock_args: owner_args,
        type_script_hash: None,
        type_code_hash: None,
        type_hash_type: None,
        type_args: None,
        data_size: 0,
        udt_amount: None,
        data_hash: Some(empty_data_hash.clone()),
    };
    let funding_cell = LiveCellInfo {
        capacity: 30_000_000_000,
        occupied_capacity: 6_100_000_000,
        lock_script_hash: compute_script_hash(&funding_code, 1, &funding_args),
        lock_code_hash: funding_code,
        lock_hash_type: 1,
        lock_args: funding_args.clone(),
        type_script_hash: None,
        type_code_hash: None,
        type_hash_type: None,
        type_args: None,
        data_size: 0,
        udt_amount: None,
        data_hash: Some(empty_data_hash),
    };
    let channel_id = [0xf0; 32];
    let channel = FiberChannel {
        funding_tx_hash: opening_hash.to_vec(),
        funding_output_index: 0,
        state: FiberChannelState::Open,
        capacity: 30_000_000_000,
        udt_type_hash: None,
        udt_amount: None,
        open_block: 10,
        open_timestamp: 1_700_000_001_000,
        close_tx_hash: None,
        close_block: None,
        close_timestamp: None,
        commitment_tx_hash: None,
        commitment_output_index: None,
        delay_epoch: None,
        settlement_tx_hash: None,
        settlement_block: None,
        settlement_timestamp: None,
        participants: vec![owner_hash.clone()],
        funding_lock_args: funding_args,
    };
    let mut batch = StoreBatch::new(&store);
    batch.put_tx_hash_map(&previous_hash, 9, 0);
    batch.put_tx_index(
        9,
        0,
        &TxIndexEntry {
            tx_hash: previous_hash,
            is_cellbase: false,
            timestamp: 1_700_000_000_000,
            inputs_count: 0,
            outputs_count: 1,
            fee: 0,
            tx_size: 100,
            cycles: None,
            semantic_tags: 0,
        },
    );
    batch.put_tx_hash_map(&opening_hash, 10, 0);
    batch.put_tx_index(
        10,
        0,
        &TxIndexEntry {
            tx_hash: opening_hash,
            is_cellbase: false,
            timestamp: 1_700_000_001_000,
            inputs_count: 1,
            outputs_count: 2,
            fee: 100_000_000,
            tx_size: 200,
            cycles: None,
            semantic_tags: 0,
        },
    );
    batch.put_consumed_cell_with_consumer(
        &previous_hash,
        0,
        &previous_cell,
        9,
        10,
        Some(&opening_hash),
    );
    batch.put_cell(&opening_hash, 0, &funding_cell, 10);
    batch.put_fiber_channel(&channel_id, &channel);
    batch.commit().unwrap();
    let app = create_router(test_config_with_ckb_db_path(
        store.clone(),
        store,
        chain.path.clone(),
        Some(chain.cleanup.clone()),
    ))
    .await;
    let (status, body) = get_json(
        &app,
        &format!("/fiber/channels/0x{}/evidence", hex::encode(channel_id)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["funding"]["txHash"],
        format!("0x{}", hex::encode(opening_hash))
    );
    assert_eq!(
        body["funding"]["lockArgs"],
        format!("0x{}", hex::encode([0x44; 20]))
    );
    assert_eq!(body["funding"]["isLive"], true);
    assert_eq!(
        body["openingPartyFlows"][0]["lockScriptHash"],
        format!("0x{}", hex::encode(owner_hash))
    );
    assert_eq!(
        body["openingPartyFlows"][0]["openingInputCapacity"],
        "100000000000"
    );
    assert_eq!(
        body["openingPartyFlows"][0]["openingOutputCapacity"],
        "69900000000"
    );
    assert_eq!(
        body["openingPartyFlows"][0]["observedNetInput"],
        "30100000000"
    );
    assert_eq!(body["stages"][0]["outputs"], 2);
    assert_eq!(body["currentCommitment"], serde_json::Value::Null);
    let (detail_status, detail) = get_json(
        &app,
        &format!("/fiber/channels/0x{}", hex::encode(channel_id)),
    )
    .await;
    assert_eq!(detail_status, StatusCode::OK, "{detail}");
    assert_eq!(detail["channelId"], body["channelId"]);
    assert_eq!(detail["fundingTxHash"], body["funding"]["txHash"]);
    export_fiber_fixture("fiber-evidence-open", &body);
    export_fiber_fixture("fiber-open-detail", &detail);
}
