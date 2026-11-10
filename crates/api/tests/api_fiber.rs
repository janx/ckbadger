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
    let owner_code = [0x11; 32];
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
        lock_code_hash: owner_code,
        lock_hash_type: 1,
        lock_args: owner_args,
        type_code_hash: None,
        type_hash_type: None,
        type_args: None,
        data_size: 0,
        udt_amount: None,
        data_hash: Some(empty_data_hash.clone().try_into().unwrap()),
    };
    let funding_cell = LiveCellInfo {
        capacity: 30_000_000_000,
        lock_code_hash: funding_code.try_into().unwrap(),
        lock_hash_type: 1,
        lock_args: funding_args.clone(),
        type_code_hash: None,
        type_hash_type: None,
        type_args: None,
        data_size: 0,
        udt_amount: None,
        data_hash: Some(empty_data_hash.try_into().unwrap()),
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
    // Consumed by the opening transaction, seeded at (10, 0).
    batch.put_consumed_cell(&previous_hash, 0, &previous_cell, 9, 10, 0);
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

/// A force-closed channel as the indexer writes it: an opening that spends one
/// owner cell into a funding cell with lock args `funding_args`, and a force
/// close that spends the funding cell into one commitment-lock output with
/// `commitment_args`. The row stores what the parser's arbiters read (the
/// funding pubkey hash, the commitment delay); the node holds the three
/// transactions. Returns the API, the channel id and the owner's lock hash.
async fn force_closed_channel(
    funding_args: Vec<u8>,
    commitment_args: Vec<u8>,
) -> (axum::Router, [u8; 32], Vec<u8>, TestCkbChain) {
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
    let delay = ckbadger_indexer::parser::fiber::parse_commitment_lock_args(&commitment_args)
        .expect("the fixture's commitment args fit a deployed layout")
        .delay_epoch;
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
        lock_code_hash: code.to_vec().try_into().unwrap(),
        lock_hash_type: 1,
        lock_args: args.to_vec(),
        type_code_hash: None,
        type_hash_type: None,
        type_args: None,
        data_size: 0,
        udt_amount: None,
        data_hash: Some(empty_data_hash.clone().try_into().unwrap()),
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
        funding_lock_args: funding_args[..20].to_vec(),
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
    // Consumed by the opening (10, 0) and the close (11, 0) transactions.
    batch.put_consumed_cell(
        &previous_hash,
        0,
        &cell(&owner_code, &owner_args, 100_000_000_000),
        9,
        10,
        0,
    );
    batch.put_consumed_cell(
        &opening_hash,
        0,
        &cell(&funding_code, &funding_args, 30_000_000_000),
        10,
        11,
        0,
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
    (app, channel_id, owner_hash, chain)
}

/// FF-C1: the commitment evidence carries the decoded `since` delay itself —
/// the frontend reads `currentCommitment.delayEpochDecoded`, not the channel's.
#[tokio::test]
async fn fiber_force_close_evidence_serializes_decoded_commitment_delay() {
    // The delay real force-close commitments carry: relative, 6 + 0/256 epochs.
    let delay: u64 = 0xa001_0000_0000_0006;
    let mut commitment_args = vec![0x55; 20];
    commitment_args.extend(delay.to_le_bytes());
    commitment_args.extend(1u64.to_be_bytes());
    commitment_args.extend([0x66; 20]);
    commitment_args.push(0);
    assert_eq!(commitment_args.len(), 57);
    let (app, channel_id, owner_hash, _chain) =
        force_closed_channel(vec![0x44; 20], commitment_args).await;
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
    // FF-C4 / FF-C5: each stage's epoch and exact accounting.
    let stages = body["stages"].as_array().unwrap();
    assert_eq!(stages.len(), 2, "{body}");
    assert_eq!(stages[0]["stage"], "opening");
    assert_eq!(stages[0]["epoch"], "1+0/1800", "{body}");
    assert_eq!(stages[0]["fee"], "100000000", "{body}");
    assert_eq!(stages[0]["channelInput"], serde_json::Value::Null);
    assert_eq!(stages[0]["feeSource"], "outside_inputs");
    assert_eq!(
        stages[0]["partyFlows"],
        serde_json::json!([{ "lockScriptHash": format!("0x{}", hex::encode(&owner_hash)),
            "inputCapacity": "100000000000", "outputCapacity": "69900000000",
            "netReceived": "-30100000000" }])
    );
    assert_eq!(stages[1]["stage"], "close");
    assert_eq!(stages[1]["epoch"], "1+0/1800");
    assert_eq!(stages[1]["fee"], "100000000");
    assert_eq!(stages[1]["channelInput"], "30000000000");
    // Only the channel cell went in: the fee came out of the channel balance,
    // whose split between the parties is not on chain.
    assert_eq!(stages[1]["feeSource"], "channel_only");
    assert_eq!(
        stages[1]["partyFlows"],
        serde_json::json!([{ "lockScriptHash": format!("0x{}", hex::encode(&owner_hash)),
            "inputCapacity": "0", "outputCapacity": "0", "netReceived": "0" }])
    );
    export_fiber_fixture("fiber-evidence-force-closed", &body);
}

/// Review 45fd finding 8: the evidence reads the tracked commitment through
/// the parser's one commitment-lock arbiter, so every layout a deployed lock
/// accepts gets evidence (36 and 56 bytes: daric locks; 57: v0.6.0; 58 ending
/// in 0x01: the 0.7.0 lock live on mainnet since 2026-11-09). A settlement
/// flag other than 0 is a later settlement, as the lock reads it
/// (`is_first_settlement = args[56] == 0x00`).
#[tokio::test]
async fn fiber_force_close_evidence_reads_every_deployed_commitment_layout() {
    let delay: u64 = 0xa001_0000_0000_0006;
    let head = || {
        let mut args = vec![0x55; 20];
        args.extend(delay.to_le_bytes());
        args.extend(1u64.to_be_bytes());
        args
    };
    let with = |tail: &[u8]| [head().as_slice(), tail].concat();
    let settlement_hash = format!("0x{}", hex::encode([0x66; 20]));
    /// (args, lockFormat, settlementHash, settlementFlag, epochLock)
    type LayoutCase<'a> = (Vec<u8>, &'a str, Option<&'a str>, Option<u8>, &'a str);
    let cases: Vec<LayoutCase<'_>> = vec![
        (head(), "legacy36", None, None, "legacyFormatRuleUnverified"),
        (
            with(&[0x77; 20]),
            "legacy56",
            None,
            None,
            "legacyFormatRuleUnverified",
        ),
        (
            with(&[[0x66; 20].as_slice(), &[0x01, 0x01]].concat()),
            "full58",
            Some(&settlement_hash),
            Some(1),
            "subsequentSettlementNoReappliedDelay",
        ),
        (
            with(&[[0x66; 20].as_slice(), &[0x00, 0x01]].concat()),
            "full58",
            Some(&settlement_hash),
            Some(0),
            "firstSettlementDelay",
        ),
        (
            with(&[[0x66; 20].as_slice(), &[0x02]].concat()),
            "full57",
            Some(&settlement_hash),
            Some(2),
            "subsequentSettlementNoReappliedDelay",
        ),
    ];
    for (args, lock_format, hash, flag, epoch_lock) in cases {
        let len = args.len();
        let (app, channel_id, _, _chain) = force_closed_channel(vec![0x44; 20], args).await;
        let (status, body) = get_json(
            &app,
            &format!("/fiber/channels/0x{}/evidence", hex::encode(channel_id)),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{len}-byte args: {body}");
        let commitment = &body["currentCommitment"];
        assert_eq!(commitment["lockFormat"], lock_format, "{len}: {body}");
        assert_eq!(
            commitment["settlementHash"],
            serde_json::json!(hash),
            "{len}"
        );
        assert_eq!(
            commitment["settlementFlag"],
            serde_json::json!(flag),
            "{len}"
        );
        assert_eq!(commitment["epochLock"], epoch_lock, "{len}");
        assert_eq!(
            commitment["pubkeyHash"],
            format!("0x{}", hex::encode([0x55; 20]))
        );
        assert_eq!(commitment["delayEpoch"], delay.to_string());
        assert_eq!(commitment["version"], "1");
    }
}

/// Every deployed funding lock reads `args[0..20]` and ignores the rest, and
/// the indexer stores those 20 bytes on the channel row: the evidence compares
/// the row with the funding cell through the same arbiter, so a funding cell
/// with 24-byte args has evidence (the old raw-args comparison failed).
#[tokio::test]
async fn fiber_evidence_reads_funding_args_longer_than_20_bytes() {
    let mut commitment_args = vec![0x55; 20];
    commitment_args.extend(0xa001_0000_0000_0006u64.to_le_bytes());
    commitment_args.extend(1u64.to_be_bytes());
    let funding_args = [[0x44; 20].as_slice(), &[0x99; 4]].concat();
    let (app, channel_id, _, _chain) = force_closed_channel(funding_args, commitment_args).await;
    let (status, body) = get_json(
        &app,
        &format!("/fiber/channels/0x{}/evidence", hex::encode(channel_id)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["funding"]["lockArgs"],
        format!("0x{}99999999", hex::encode([0x44; 20]))
    );
}

/// One real node transaction (`get_transaction` result, verbosity 2) from
/// `tests/fixtures/fiber_e1a939e4/`; its hash is the node's own.
fn fiber_mainnet_tx(json: &str) -> ckb_types::core::TransactionView {
    use ckb_types::{packed, prelude::*};
    let view: ckb_jsonrpc_types::TransactionView = serde_json::from_str(json).unwrap();
    let tx = packed::Transaction::from(view.inner).into_view();
    assert_eq!(tx.hash(), view.hash.pack());
    tx
}

/// Review 45fd finding 8, live on mainnet: channel 0xe1a939e4… is settled
/// through the 0.7.0 commitment lock's 58-byte v1 args, and its evidence
/// answered 500 ("has unsupported args length 58"). The channel row is the
/// mainnet API's record (captured 2026-11-10); the node holds the four real
/// transactions the evidence reads (open 0xc4693771…, force close
/// 0x263875f6…, the tracked partial settlement 0x255f83e7…, the settlement
/// 0x37bcfbd7…) at their real block numbers; the store holds every cell those
/// stages spend, with the real capacities and locks.
#[tokio::test]
async fn fiber_evidence_for_real_58_byte_v1_mainnet_channel() {
    use ckb_types::{core::BlockBuilder, core::EpochNumberWithFraction, prelude::*};
    let hash32 = |hex_str: &str| -> [u8; 32] {
        hex::decode(hex_str.trim_start_matches("0x"))
            .unwrap()
            .try_into()
            .unwrap()
    };
    let bytes = |hex_str: &str| hex::decode(hex_str.trim_start_matches("0x")).unwrap();
    let open = fiber_mainnet_tx(include_str!("fixtures/fiber_e1a939e4/c4693771.json"));
    let close = fiber_mainnet_tx(include_str!("fixtures/fiber_e1a939e4/263875f6.json"));
    let tracked = fiber_mainnet_tx(include_str!("fixtures/fiber_e1a939e4/255f83e7.json"));
    let settle = fiber_mainnet_tx(include_str!("fixtures/fiber_e1a939e4/37bcfbd7.json"));
    // (tx, block, tx_index, timestamp) — block and position from the node.
    let placed = [
        (&open, 20_967_540i64, 4i32, 1_794_219_736_030i64),
        (&close, 20_967_551, 1, 1_794_219_855_039),
        (&tracked, 20_967_938, 1, 1_794_224_960_000),
        (&settle, 20_967_943, 1, 1_794_225_002_832),
    ];
    let chain = seed_ckb_chain(
        &placed
            .iter()
            .map(|(tx, block, _, _)| {
                BlockBuilder::default()
                    .number((*block as u64).pack())
                    .epoch(EpochNumberWithFraction::new(1, 0, 1800).pack())
                    .transaction((*tx).clone())
                    .build()
            })
            .collect::<Vec<_>>(),
    );

    let store = test_store();
    seed_block_headers_at(
        &store,
        &placed
            .iter()
            .map(|(_, block, _, timestamp)| (*block, *timestamp))
            .collect::<Vec<_>>(),
    );
    store
        .update_sync_status(|status| status.tip_block_number = 20_967_943)
        .unwrap();

    let secp = bytes("0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8");
    let funding_code = bytes(ckbadger_indexer::parser::fiber::FUNDING_LOCK_CODE_HASH_MAINNET);
    let commitment_code = bytes(ckbadger_indexer::parser::fiber::COMMITMENT_LOCK_CODE_HASH_MAINNET);
    let secp_a = bytes("0x7f0a30a2da9ee266d6f901daa52605d855c04449");
    let secp_b = bytes("0xe2d5f53fcb8fde1a17cc8f81bde5ef9a92bcf7fc");
    let funding_args = bytes("0x5c494edc86cce3a4d5692364453193e6ed9c6954");
    let tracked_args = bytes("0x5c494edc86cce3a4d5692364453193e6ed9c695401000000000100a00000000000000002f4bb16e4d4b49bb983db6d5ec191cd860c06712b0101");
    let ed1f_args = bytes("0x5c494edc86cce3a4d5692364453193e6ed9c695401000000000100a000000000000000023430dc58c3d22031b35419c4ec79e5996f2e24db0101");
    let empty_data_hash =
        ckbadger_indexer::parser::script::ScriptParser::compute_data_hash(&[]).to_vec();
    let cell = |code: &[u8], args: &[u8], capacity: i64| LiveCellInfo {
        capacity,
        lock_code_hash: code.to_vec().try_into().unwrap(),
        lock_hash_type: 1,
        lock_args: args.to_vec(),
        type_code_hash: None,
        type_hash_type: None,
        type_args: None,
        data_size: 0,
        udt_amount: None,
        data_hash: Some(empty_data_hash.clone().try_into().unwrap()),
    };
    let party_a = compute_script_hash(&secp, 1, &secp_a);
    let party_b = compute_script_hash(&secp, 1, &secp_b);
    // The participants the mainnet API serves for this channel.
    assert_eq!(
        format!("0x{}", hex::encode(&party_a)),
        "0x9a040b1fa0257fb0abe427dcec79441e4dc277a6eed602a290b75acaa2d059e2"
    );
    assert_eq!(
        format!("0x{}", hex::encode(&party_b)),
        "0xc281bed43192de71ffdc38397919cfc4623463bb3e75e3a6a10aa96e90d1502a"
    );

    let open_hash = hash32("0xc46937718fdb92ad66feb99c64b676aa220ea896477d1a3a5dc14ddf17b6bfb9");
    let close_hash = hash32("0x263875f6184c3de6a09bd1457f4a40c123fb768c576a5fcd1d82dc0c8488a4b6");
    let ed1f_hash = hash32("0xed1fbf13dc59cf255e9b6728e45a96dce3aaeacab98d893c7b9ba2437d527abd");
    let tracked_hash = hash32("0x255f83e718cd6ab33be6a7a2581a82632853fc2fe89b75283815042d85f37fe9");
    let settle_hash = hash32("0x37bcfbd70d84f39b5c8f0bfe0744cb91477f5a75317119b9d00e590459def47f");
    let mut batch = StoreBatch::new(&store);
    for (tx, block, tx_index, timestamp) in placed {
        let hash: [u8; 32] = tx.hash().unpack();
        batch.put_tx_hash_map(&hash, block, tx_index);
        batch.put_tx_index(
            block,
            tx_index,
            &TxIndexEntry {
                tx_hash: hash,
                is_cellbase: false,
                timestamp,
                inputs_count: tx.inputs().len() as i16,
                outputs_count: tx.outputs().len() as i16,
                fee: 0,
                tx_size: 0,
                cycles: None,
                semantic_tags: 0,
            },
        );
    }
    // The transactions that created the other cells the stages spend, at
    // their node positions: (hash, block, tx_index, inputs, outputs).
    for (hash, block, tx_index, inputs, outputs) in [
        (
            "0xee1d07e6a6e1e7aa4b1c5e2d2e1a6a12c030f3e2f35e26c9921d03b1741c17b3",
            20_927_625,
            1,
            1,
            2,
        ),
        (
            "0xfd1ff084b679a6dcf71b115c5744d62c2419c5ae9d4cb8a4c54855fad81d362d",
            20_964_111,
            2,
            4,
            3,
        ),
        (
            "0xed1fbf13dc59cf255e9b6728e45a96dce3aaeacab98d893c7b9ba2437d527abd",
            20_967_931,
            2,
            2,
            2,
        ),
        (
            "0x00e9735c26998020df1e0a8272f2801c8b446880e329f744f550c10c874e6f6d",
            20_945_165,
            2,
            1,
            2,
        ),
    ] {
        let hash = hash32(hash);
        batch.put_tx_hash_map(&hash, block, tx_index);
        batch.put_tx_index(
            block,
            tx_index,
            &TxIndexEntry {
                tx_hash: hash,
                is_cellbase: false,
                timestamp: 0,
                inputs_count: inputs,
                outputs_count: outputs,
                fee: 0,
                tx_size: 0,
                cycles: None,
                semantic_tags: 0,
            },
        );
    }
    // Cells the four stages spend: (out point, cell, created at, consumed by).
    let consumed = [
        (
            hash32("0xee1d07e6a6e1e7aa4b1c5e2d2e1a6a12c030f3e2f35e26c9921d03b1741c17b3"),
            0,
            cell(&secp, &secp_a, 49_999_999_456),
            20_927_625,
            (20_967_540, 4),
        ),
        (
            hash32("0xfd1ff084b679a6dcf71b115c5744d62c2419c5ae9d4cb8a4c54855fad81d362d"),
            1,
            cell(&secp, &secp_b, 30_300_001_849),
            20_964_111,
            (20_967_540, 4),
        ),
        (
            open_hash,
            0,
            cell(&funding_code, &funding_args, 35_000_000_000),
            20_967_540,
            (20_967_551, 1),
        ),
        (
            open_hash,
            2,
            cell(&secp, &secp_b, 20_300_001_603),
            20_967_540,
            (20_967_943, 1),
        ),
        (
            ed1f_hash,
            0,
            cell(&commitment_code, &ed1f_args, 33_999_999_543),
            20_967_931,
            (20_967_938, 1),
        ),
        (
            hash32("0x00e9735c26998020df1e0a8272f2801c8b446880e329f744f550c10c874e6f6d"),
            0,
            cell(&secp, &secp_a, 49_999_999_456),
            20_945_165,
            (20_967_938, 1),
        ),
        (
            tracked_hash,
            0,
            cell(&commitment_code, &tracked_args, 9_999_999_543),
            20_967_938,
            (20_967_943, 1),
        ),
    ];
    for (hash, index, info, created, (block, tx_index)) in &consumed {
        batch.put_consumed_cell(hash, *index, info, *created, *block, *tx_index);
    }
    batch.put_cell(
        &open_hash,
        1,
        &cell(&secp, &secp_a, 24_999_998_992),
        20_967_540,
    );
    batch.put_cell(
        &tracked_hash,
        1,
        &cell(&secp, &secp_a, 73_999_998_671),
        20_967_938,
    );
    batch.put_cell(
        &settle_hash,
        0,
        &cell(&secp, &secp_b, 30_300_000_508),
        20_967_943,
    );
    let channel_id = hash32("0xe1a939e4375f0f0c6590c44e5e7e73d63351777599b1d46c47cd010797a8bbac");
    batch.put_fiber_channel(
        &channel_id,
        &FiberChannel {
            funding_tx_hash: open_hash.to_vec(),
            funding_output_index: 0,
            state: FiberChannelState::Settled,
            capacity: 35_000_000_000,
            udt_type_hash: None,
            udt_amount: None,
            open_block: 20_967_540,
            open_timestamp: 1_794_219_736_030,
            close_tx_hash: Some(close_hash.to_vec()),
            close_block: Some(20_967_551),
            close_timestamp: Some(1_794_219_855_039),
            commitment_tx_hash: Some(tracked_hash.to_vec()),
            commitment_output_index: Some(0),
            delay_epoch: Some(11_529_216_145_580_097_537),
            settlement_tx_hash: Some(settle_hash.to_vec()),
            settlement_block: Some(20_967_943),
            settlement_timestamp: Some(1_794_225_002_832),
            participants: vec![party_a.clone(), party_b.clone()],
            funding_lock_args: funding_args.clone(),
        },
    );
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
    assert_eq!(
        commitment["txHash"],
        format!("0x{}", hex::encode(tracked_hash))
    );
    assert_eq!(commitment["outputIndex"], 0);
    assert_eq!(
        commitment["lockArgs"],
        format!("0x{}", hex::encode(&tracked_args))
    );
    assert_eq!(commitment["lockFormat"], "full58");
    assert_eq!(
        commitment["pubkeyHash"],
        format!("0x{}", hex::encode(&funding_args))
    );
    assert_eq!(commitment["delayEpoch"], "11529216145580097537");
    assert_eq!(
        commitment["delayEpochDecoded"],
        serde_json::json!({"relative": true, "epochs": 1, "index": 0, "length": 1})
    );
    assert_eq!(commitment["version"], "2");
    assert_eq!(
        commitment["settlementHash"],
        "0xf4bb16e4d4b49bb983db6d5ec191cd860c06712b"
    );
    assert_eq!(commitment["settlementFlag"], 1);
    assert_eq!(
        commitment["epochLock"],
        "subsequentSettlementNoReappliedDelay"
    );
    assert_eq!(commitment["readyEpoch"], serde_json::Value::Null);
    assert_eq!(commitment["isLive"], false);
    assert_eq!(
        commitment["consumedByTx"],
        format!("0x{}", hex::encode(settle_hash))
    );
    let stages: Vec<_> = body["stages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|stage| {
            (
                stage["stage"].as_str().unwrap().to_string(),
                stage["fee"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    let stage = |name: &str, fee: &str| (name.to_string(), fee.to_string());
    assert_eq!(
        stages,
        vec![
            stage("opening", "710"),
            stage("close", "457"),
            stage("currentCommitment", "785"),
            stage("settlement", "638"),
        ]
    );
    assert_eq!(
        body["settlementOutputs"],
        serde_json::json!([{
            "txHash": format!("0x{}", hex::encode(settle_hash)),
            "outputIndex": 0,
            "ownerLockHash": format!("0x{}", hex::encode(&party_b)),
            "observedParty": true,
            "capacity": "30300000508",
        }])
    );
}

/// A three-block chain written through the indexer's real live path
/// (`live_test_support`), with the node database seeded from the same blocks:
/// genesis funds owner 0x21, block 1 opens a channel (funding lock, args
/// `[0x44; 20]`), block 2 spends the funding cell into one output carrying the
/// commitment-lock code hash with `close_args`. Real transaction and header
/// hashes throughout, as in `api_layout_pins.rs`.
mod fiber_live_chain {
    use crate::common::*;
    use ckbadger_indexer::rpc::{
        BlockResponseWithCycles, BlockView as RpcBlock, CellInput, CellOutput, HeaderView,
        OutPoint, Script, TransactionView,
    };
    use ckbadger_indexer::sync::live_test_support::{restart, write_live_batch};

    const GENESIS_C: u64 = 3_360_000_000_000_000_000;
    const SECONDARY_EPOCH_REWARD: u64 = 61_369_863_013_698;
    const AR_BASE: u64 = 10_000_000_000_000_000;
    const SECP_CODE_HASH: &str =
        "0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8";
    /// `CellbaseWitness { lock: secp256k1 / args 0x8211…, message }`.
    const CELLBASE_WITNESS: &str = "0x7a0000000c00000055000000490000001000000030000000310000009bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce801140000008211f1b938a107cd53b6302cc752a6fc3965638d210000000000000020302e3131332e3020283832383731613320323032342d30312d303929";
    const T0: u64 = 1_790_038_800_000;
    const FUND: u64 = 100_000;

    fn hex32(bytes: [u8; 32]) -> String {
        format!("0x{}", hex::encode(bytes))
    }

    fn script(code_hash: &str, args: &[u8]) -> Script {
        Script {
            code_hash: code_hash.to_string(),
            hash_type: "type".to_string(),
            args: format!("0x{}", hex::encode(args)),
        }
    }

    fn output(capacity_ckb: u64, lock: Script) -> (CellOutput, String) {
        (
            CellOutput {
                capacity: format!("0x{:x}", capacity_ckb * 100_000_000),
                lock,
                type_: None,
            },
            "0x".to_string(),
        )
    }

    fn jsonrpc_header(h: &HeaderView) -> ckb_jsonrpc_types::HeaderView {
        serde_json::from_value(serde_json::json!({
            "version": h.version, "compact_target": h.compact_target,
            "timestamp": h.timestamp, "number": h.number, "epoch": h.epoch,
            "parent_hash": h.parent_hash, "transactions_root": h.transactions_root,
            "proposals_hash": h.proposals_hash, "extra_hash": h.extra_hash,
            "dao": h.dao, "nonce": h.nonce, "hash": h.hash,
        }))
        .unwrap()
    }

    fn tx(
        inputs: Vec<CellInput>,
        outputs: Vec<(CellOutput, String)>,
        witness: &str,
    ) -> TransactionView {
        use ckb_types::{packed, prelude::*};
        let (outputs, outputs_data): (Vec<_>, Vec<_>) = outputs.into_iter().unzip();
        let mut tx = TransactionView {
            hash: hex32([0; 32]),
            version: "0x0".to_string(),
            cell_deps: vec![],
            header_deps: vec![],
            witnesses: vec![witness.to_string(); inputs.len().max(1)],
            inputs,
            outputs,
            outputs_data,
        };
        let json: ckb_jsonrpc_types::TransactionView =
            serde_json::from_value(serde_json::to_value(&tx).unwrap()).unwrap();
        let hash: [u8; 32] = packed::Transaction::from(json.inner)
            .calc_tx_hash()
            .unpack();
        tx.hash = hex32(hash);
        tx
    }

    fn input(tx: &TransactionView, index: u32) -> CellInput {
        CellInput {
            since: "0x0".to_string(),
            previous_output: OutPoint {
                tx_hash: tx.hash.clone(),
                index: format!("0x{index:x}"),
            },
        }
    }

    fn block(
        number: u64,
        parent: [u8; 32],
        cellbase_outputs: Vec<(CellOutput, String)>,
        txs: Vec<TransactionView>,
    ) -> (BlockResponseWithCycles, [u8; 32]) {
        use ckb_types::{packed, prelude::*};
        let cellbase = tx(
            vec![CellInput {
                since: format!("0x{number:x}"),
                previous_output: OutPoint {
                    tx_hash: hex32([0; 32]),
                    index: "0xffffffff".to_string(),
                },
            }],
            cellbase_outputs,
            CELLBASE_WITNESS,
        );
        let mut transactions = vec![cellbase];
        transactions.extend(txs);
        let mut dao = [0u8; 32];
        dao[0..8].copy_from_slice(&GENESIS_C.to_le_bytes());
        dao[8..16].copy_from_slice(&(AR_BASE + number * 1_000_000_000).to_le_bytes());
        dao[16..24].copy_from_slice(&10_000_000_000u64.to_le_bytes());
        dao[24..32].copy_from_slice(&100_000_000_000_000u64.to_le_bytes());
        let mut header = HeaderView {
            version: "0x0".to_string(),
            compact_target: "0x1a08a97e".to_string(),
            timestamp: format!("0x{:x}", T0 + number * 600_000),
            number: format!("0x{number:x}"),
            epoch: format!("0x{:x}", (1800u64 << 40) | (number << 24)),
            parent_hash: hex32(parent),
            transactions_root: hex32([0x22; 32]),
            proposals_hash: hex32([0x33; 32]),
            extra_hash: hex32([0x44; 32]),
            dao: format!("0x{}", hex::encode(dao)),
            nonce: "0x1".to_string(),
            hash: hex32([0; 32]),
        };
        let hash: [u8; 32] = packed::Header::from(jsonrpc_header(&header).inner)
            .calc_header_hash()
            .unpack();
        header.hash = hex32(hash);
        (
            BlockResponseWithCycles {
                block: RpcBlock {
                    header,
                    uncles: vec![],
                    transactions,
                    proposals: vec![],
                },
                cycles: None,
            },
            hash,
        )
    }

    fn node_block(block: &BlockResponseWithCycles) -> ckb_types::core::BlockView {
        use ckb_types::{core, packed, prelude::*};
        let header = jsonrpc_header(&block.block.header);
        let header_view = packed::Header::from(header.inner).into_view();
        let transactions = block
            .block
            .transactions
            .iter()
            .map(|tx| {
                let json: ckb_jsonrpc_types::TransactionView =
                    serde_json::from_value(serde_json::to_value(tx).unwrap()).unwrap();
                packed::Transaction::from(json.inner).into_view()
            })
            .collect();
        core::BlockView::new_unchecked(
            header_view,
            packed::UncleBlockVecView::default().unpack(),
            transactions,
            packed::ProposalShortIdVec::default(),
        )
    }

    pub(super) struct LiveFiberChain {
        pub app: axum::Router,
        pub channel_id: Vec<u8>,
        pub open_hash: [u8; 32],
        pub close_hash: [u8; 32],
        _dirs: (tempfile::TempDir, tempfile::TempDir),
        _chain: TestCkbChain,
    }

    pub(super) async fn open_then_close_into(close_args: &[u8]) -> LiveFiberChain {
        let domain_dir = tempfile::tempdir().unwrap();
        let append_dir = tempfile::tempdir().unwrap();
        let domain = Arc::new(CkbadgerStore::open_domain(domain_dir.path()).unwrap());
        let append = Arc::new(CkbadgerStore::open_append_only(append_dir.path()).unwrap());
        domain
            .set_genesis_baseline(&ckbadger_store::GenesisBaseline {
                total_issuance: GENESIS_C as i128,
                burnt: 0,
                virtual_occupied: 0,
            })
            .unwrap();
        domain
            .set_secondary_epoch_reward(SECONDARY_EPOCH_REWARD)
            .unwrap();

        let owner = script(SECP_CODE_HASH, &[0x21; 20]);
        let (genesis, genesis_hash) = block(0, [0; 32], vec![output(FUND, owner.clone())], vec![]);
        let open = tx(
            vec![input(&genesis.block.transactions[0], 0)],
            vec![
                output(
                    500,
                    script(
                        ckbadger_indexer::parser::fiber::FUNDING_LOCK_CODE_HASH_MAINNET,
                        &[0x44; 20],
                    ),
                ),
                output(FUND - 501, owner.clone()),
            ],
            "0x",
        );
        let (b1, b1_hash) = block(
            1,
            genesis_hash,
            vec![output(1_000, owner.clone())],
            vec![open.clone()],
        );
        let close = tx(
            vec![input(&open, 0)],
            vec![output(
                499,
                script(
                    ckbadger_indexer::parser::fiber::COMMITMENT_LOCK_CODE_HASH_MAINNET,
                    close_args,
                ),
            )],
            "0x",
        );
        let (b2, _) = block(2, b1_hash, vec![output(1_000, owner)], vec![close.clone()]);
        let blocks = vec![genesis, b1, b2];

        let indexer = restart(domain.clone(), append.clone(), "mainnet").unwrap();
        for block in &blocks {
            write_live_batch(&indexer, vec![block.clone()])
                .await
                .unwrap();
        }
        drop(indexer);
        let chain = seed_ckb_chain(&blocks.iter().map(node_block).collect::<Vec<_>>());
        let app = create_router(test_config_with_ckb_db_path(
            domain,
            append,
            chain.path.clone(),
            Some(chain.cleanup.clone()),
        ))
        .await;
        let parse =
            |hash: &str| -> [u8; 32] { hex::decode(&hash[2..]).unwrap().try_into().unwrap() };
        let open_hash = parse(&open.hash);
        LiveFiberChain {
            app,
            channel_id: ckbadger_store::keys::encode_fiber_channel_id(&open_hash, 0),
            open_hash,
            close_hash: parse(&close.hash),
            _dirs: (domain_dir, append_dir),
            _chain: chain,
        }
    }
}

/// Review 45fd finding 8 + N1, end to end: a tracked channel's funding spent
/// into a commitment-code-hash output with 40-byte args (no deployed layout).
/// Before: the indexer force-closed the channel tracking that cell, and the
/// evidence answered 500 ("has unsupported args length 40"). Now the output is
/// an ordinary cell, the spend is a plain close, and the evidence is 200.
#[tokio::test]
async fn fiber_close_into_malformed_commitment_output_is_a_plain_close_with_evidence() {
    let mut args = vec![0x55; 20];
    args.extend(0xa001_0000_0000_0006u64.to_le_bytes());
    args.extend(1u64.to_be_bytes());
    args.extend([0x66; 4]);
    assert_eq!(args.len(), 40);
    let chain = fiber_live_chain::open_then_close_into(&args).await;
    let id = hex::encode(&chain.channel_id);

    let (status, detail) = get_json(&chain.app, &format!("/fiber/channels/0x{id}")).await;
    assert_eq!(status, StatusCode::OK, "{detail}");
    assert_eq!(detail["state"], "cooperativelyClosed", "{detail}");
    assert_eq!(
        detail["fundingTxHash"],
        format!("0x{}", hex::encode(chain.open_hash))
    );
    assert_eq!(
        detail["closeTxHash"],
        format!("0x{}", hex::encode(chain.close_hash))
    );
    assert_eq!(detail["commitmentTxHash"], serde_json::Value::Null);
    assert_eq!(detail["delayEpoch"], serde_json::Value::Null);

    let (status, evidence) =
        get_json(&chain.app, &format!("/fiber/channels/0x{id}/evidence")).await;
    assert_eq!(status, StatusCode::OK, "{evidence}");
    assert_eq!(evidence["currentCommitment"], serde_json::Value::Null);
    let stages: Vec<_> = evidence["stages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|stage| stage["stage"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(stages, vec!["opening", "close"]);
    assert_eq!(evidence["stages"][1]["fee"], "100000000");
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
