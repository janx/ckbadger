use crate::common::*;

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

/// FF-C1: the commitment evidence carries the decoded `since` delay itself —
/// the frontend reads `currentCommitment.delayEpochDecoded`, not the channel's.
#[tokio::test]
async fn fiber_force_close_evidence_serializes_decoded_commitment_delay() {
    use ckb_types::{
        bytes::Bytes,
        core::{
            BlockBuilder, Capacity, EpochNumberWithFraction, ScriptHashType, TransactionBuilder,
        },
        packed,
        prelude::*,
    };
    let script = |code: &[u8], args: &[u8]| {
        packed::Script::new_builder()
            .code_hash(packed::Byte32::from_slice(code).unwrap())
            .hash_type(ScriptHashType::Type.into())
            .args(Bytes::from(args.to_vec()).pack())
            .build()
    };
    let out_point = |hash: [u8; 32], index: u32| {
        packed::CellInput::new_builder()
            .previous_output(
                packed::OutPoint::new_builder()
                    .tx_hash(packed::Byte32::new(hash))
                    .index(index.pack())
                    .build(),
            )
            .build()
    };
    let output = |capacity: u64, lock: &packed::Script| {
        packed::CellOutput::new_builder()
            .capacity(Capacity::shannons(capacity).pack())
            .lock(lock.clone())
            .build()
    };
    let owner_code = vec![0x11; 32];
    let owner_args = vec![0xaa; 20];
    let funding_code = hex::decode(
        ckbadger_indexer::parser::fiber::FUNDING_LOCK_CODE_HASH_MAINNET.trim_start_matches("0x"),
    )
    .unwrap();
    let commitment_code = hex::decode(
        ckbadger_indexer::parser::fiber::COMMITMENT_LOCK_CODE_HASH_MAINNET.trim_start_matches("0x"),
    )
    .unwrap();
    let funding_args = vec![0x44; 20];
    // The delay real force-close commitments carry: relative, 6 + 0/256 epochs.
    let delay: u64 = 0xa001_0000_0000_0006;
    let mut commitment_args = vec![0x55; 20];
    commitment_args.extend(delay.to_le_bytes());
    commitment_args.extend(1u64.to_be_bytes());
    commitment_args.extend([0x66; 20]);
    commitment_args.push(0);
    assert_eq!(commitment_args.len(), 57);
    let owner = script(&owner_code, &owner_args);
    let funding_lock = script(&funding_code, &funding_args);
    let commitment_lock = script(&commitment_code, &commitment_args);

    let previous_tx = TransactionBuilder::default()
        .output(output(100_000_000_000, &owner))
        .output_data(Bytes::new().pack())
        .build();
    let previous_hash: [u8; 32] = previous_tx.hash().unpack();
    let opening_tx = TransactionBuilder::default()
        .input(out_point(previous_hash, 0))
        .output(output(30_000_000_000, &funding_lock))
        .output_data(Bytes::new().pack())
        .output(output(69_900_000_000, &owner))
        .output_data(Bytes::new().pack())
        .build();
    let opening_hash: [u8; 32] = opening_tx.hash().unpack();
    let close_tx = TransactionBuilder::default()
        .input(out_point(opening_hash, 0))
        .output(output(29_900_000_000, &commitment_lock))
        .output_data(Bytes::new().pack())
        .build();
    let close_hash: [u8; 32] = close_tx.hash().unpack();
    let epoch = EpochNumberWithFraction::new(1, 0, 1800).pack();
    let blocks: Vec<_> = [(9u64, previous_tx), (10, opening_tx), (11, close_tx)]
        .into_iter()
        .map(|(number, tx)| {
            BlockBuilder::default()
                .number(number.pack())
                .epoch(epoch.clone())
                .transaction(tx)
                .build()
        })
        .collect();
    let chain = seed_ckb_chain(&blocks);

    let store = test_store();
    seed_block_headers_at(
        &store,
        &[
            (9, 1_700_000_000_000),
            (10, 1_700_000_001_000),
            (11, 1_700_000_002_000),
        ],
    );
    store
        .update_sync_status(|status| status.tip_block_number = 11)
        .unwrap();
    let empty_data_hash =
        ckbadger_indexer::parser::script::ScriptParser::compute_data_hash(&[]).to_vec();
    let cell = |code: &[u8], args: &[u8], capacity: i64| LiveCellInfo {
        capacity,
        occupied_capacity: i64::try_from(8 + 33 + args.len()).unwrap() * 100_000_000,
        lock_script_hash: compute_script_hash(code, 1, args),
        lock_code_hash: code.to_vec(),
        lock_hash_type: 1,
        lock_args: args.to_vec(),
        type_script_hash: None,
        type_code_hash: None,
        type_hash_type: None,
        type_args: None,
        data_size: 0,
        udt_amount: None,
        data_hash: Some(empty_data_hash.clone()),
    };
    let owner_hash = compute_script_hash(&owner_code, 1, &owner_args);
    let channel_id = [0xf1; 32];
    let channel = FiberChannel {
        funding_tx_hash: opening_hash.to_vec(),
        funding_output_index: 0,
        state: FiberChannelState::ForceClosed,
        capacity: 30_000_000_000,
        udt_type_hash: None,
        udt_amount: None,
        open_block: 10,
        open_timestamp: 1_700_000_001_000,
        close_tx_hash: Some(close_hash.to_vec()),
        close_block: Some(11),
        close_timestamp: Some(1_700_000_002_000),
        commitment_tx_hash: Some(close_hash.to_vec()),
        commitment_output_index: Some(0),
        delay_epoch: Some(delay),
        settlement_tx_hash: None,
        settlement_block: None,
        settlement_timestamp: None,
        participants: vec![owner_hash.clone()],
        funding_lock_args: funding_args.clone(),
    };
    let mut batch = StoreBatch::new(&store);
    for (hash, block, inputs, outputs) in [
        (previous_hash, 9, 0, 1),
        (opening_hash, 10, 1, 2),
        (close_hash, 11, 1, 1),
    ] {
        batch.put_tx_hash_map(&hash, block, 0);
        batch.put_tx_index(
            block,
            0,
            &TxIndexEntry {
                tx_hash: hash,
                is_cellbase: false,
                timestamp: 1_700_000_000_000,
                inputs_count: inputs,
                outputs_count: outputs,
                fee: 100_000_000,
                tx_size: 200,
                cycles: None,
                semantic_tags: 0,
            },
        );
    }
    batch.put_consumed_cell_with_consumer(
        &previous_hash,
        0,
        &cell(&owner_code, &owner_args, 100_000_000_000),
        9,
        10,
        Some(&opening_hash),
    );
    batch.put_consumed_cell_with_consumer(
        &opening_hash,
        0,
        &cell(&funding_code, &funding_args, 30_000_000_000),
        10,
        11,
        Some(&close_hash),
    );
    batch.put_cell(
        &close_hash,
        0,
        &cell(&commitment_code, &commitment_args, 29_900_000_000),
        11,
    );
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
    let commitment = &body["currentCommitment"];
    assert_eq!(commitment["delayEpoch"], delay.to_string());
    assert_eq!(
        commitment["delayEpochDecoded"],
        serde_json::json!({"relative": true, "epochs": 6, "index": 0, "length": 256})
    );
    // The same decoder serves the channel detail: one decoded value.
    let (status, detail) = get_json(
        &app,
        &format!("/fiber/channels/0x{}", hex::encode(channel_id)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{detail}");
    assert_eq!(detail["delayEpochDecoded"], commitment["delayEpochDecoded"]);
    assert_eq!(commitment["epochLock"], "firstSettlementDelay");
    assert_eq!(commitment["readyEpoch"], "7+0/1");
    assert_eq!(commitment["epochLockMetAtTip"], false);
    export_fiber_fixture("fiber-evidence-force-closed", &body);
}

// ---- Lane R2b-b: channel ordering, totals and network facts (FF-F3, FF-F4, FF-F5, N5) ----

fn fiber_secp_party(args: u8) -> (Vec<u8>, ckbadger_store::types::LockScriptEntry) {
    let code_hash =
        hex::decode("9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8").unwrap();
    let entry = ckbadger_store::types::LockScriptEntry {
        code_hash: code_hash.clone(),
        hash_type: 1,
        args: vec![args; 20],
    };
    let hash = ckbadger_api::utils::address::compute_script_hash(&code_hash, 1, &[args; 20]);
    (hash, entry)
}

#[tokio::test]
async fn fiber_channels_sort_by_capacity_with_totals_and_network_names_parties_pairs_tokens() {
    let store = test_store();
    seed_fiber_headers(&store);
    let (party_a, lock_a) = fiber_secp_party(0x11);
    let (party_b, lock_b) = fiber_secp_party(0x22);
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
    let mut usdi = base.clone();
    usdi.capacity = 500_000_000_000;
    usdi.udt_type_hash = Some(vec![0x77; 32]);
    usdi.udt_amount = Some(1_000_000);
    usdi.open_block = 20;
    usdi.open_timestamp = 1_700_000_100_000;
    let mut late = base.clone();
    late.capacity = 100_000_000_000;
    late.open_block = 30;
    late.open_timestamp = 1_700_000_200_000;
    let mut batch = StoreBatch::new(&store);
    batch.put_fiber_channel(&[0x01; 32], &base);
    batch.put_fiber_channel(&[0x02; 32], &cooperative);
    batch.put_fiber_channel(&[0x03; 32], &waiting);
    batch.put_fiber_channel(&[0x04; 32], &usdi);
    batch.put_fiber_channel(&[0x05; 32], &late);
    batch.put_lock_script(&party_a, &lock_a);
    batch.put_lock_script(&party_b, &lock_b);
    batch.put_script_info(
        &lock_a.code_hash,
        &ckbadger_store::types::ScriptInfo {
            code_hash: lock_a.code_hash.clone(),
            hash_type: 1,
            name: Some("Default Lock".into()),
            ..Default::default()
        },
    );
    batch.put_token(
        &[0x77; 32],
        &ckbadger_store::types::TokenInfo {
            type_code_hash: vec![0x78; 32],
            hash_type: 1,
            type_args: vec![],
            standard: "xudt".into(),
            name: Some("USD Interest".into()),
            symbol: Some("USDI".into()),
            decimals: Some(6),
            max_supply: None,
            first_seen_block: 1,
            icon_url: None,
            description: None,
            transfers_count: 0,
            metadata_source: Some(ckbadger_store::types::TokenMetadataSource::Label),
            chain_metadata: None,
        },
    );
    batch.commit().unwrap();
    let app = create_router(test_config(store)).await;

    // FF-F4 / N5: open channels by capacity, with the filtered total.
    let (status, page1) = get_json(&app, "/fiber/channels?state=open&sort=capacity&limit=2").await;
    assert_eq!(status, StatusCode::OK, "{page1}");
    assert_eq!(page1["total"], 3, "{page1}");
    let ids: Vec<_> = page1["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["capacity"].clone())
        .collect();
    assert_eq!(
        ids,
        vec![
            serde_json::json!("500000000000"),
            serde_json::json!("300000000000")
        ]
    );
    let cursor = page1["nextCursor"].as_str().unwrap().to_string();
    assert_eq!(cursor, format!("300000000000:0x{}", "01".repeat(32)));
    let (status, page2) = get_json(
        &app,
        &format!("/fiber/channels?state=open&sort=capacity&limit=2&cursor={cursor}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{page2}");
    assert_eq!(page2["data"].as_array().unwrap().len(), 1);
    assert_eq!(
        page2["data"][0]["channelId"],
        format!("0x{}", "05".repeat(32))
    );
    assert_eq!(page2["nextCursor"], serde_json::Value::Null);
    // The default order keeps channel-id order and now carries the total too.
    let (status, all) = get_json(&app, "/fiber/channels?limit=2").await;
    assert_eq!(status, StatusCode::OK, "{all}");
    assert_eq!(all["total"], 5);
    assert_eq!(
        all["data"][0]["channelId"],
        format!("0x{}", "01".repeat(32))
    );
    for bad in [
        "/fiber/channels?sort=size",
        "/fiber/channels?sort=capacity&cursor=0x0101",
        "/fiber/channels?sort=capacity&cursor=abc:0x01",
    ] {
        let (status, body) = get_json(&app, bad).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{bad}: {body}");
    }

    // FF-F5: token-funded channels by token; FF-F3: party and pair facts.
    let (status, network) = get_json(&app, "/fiber/network").await;
    assert_eq!(status, StatusCode::OK, "{network}");
    assert_eq!(
        network["summary"]["channelsByToken"],
        serde_json::json!([{ "udtTypeHash": format!("0x{}", "77".repeat(32)), "symbol": "USDI",
            "channels": 1, "openChannels": 1 }])
    );
    let party = network["parties"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["lockScriptHash"] == format!("0x{}", hex::encode(&party_a)))
        .unwrap();
    assert_eq!(party["cooperativelyClosedChannels"], 1, "{party}");
    assert_eq!(party["forceClosedWaitingChannels"], 1, "{party}");
    assert_eq!(party["forceClosedSettledChannels"], 0, "{party}");
    assert_eq!(party["firstOpenBlock"], 10, "{party}");
    assert_eq!(party["firstOpenTimestamp"], "1700000000000", "{party}");
    assert_eq!(party["lockScriptName"], "Default Lock", "{party}");
    assert_eq!(network["pairs"][0]["firstOpenTimestamp"], "1700000000000");
    assert_eq!(network["pairs"][0]["lastOpenTimestamp"], "1700000200000");
}
