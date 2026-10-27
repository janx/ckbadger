mod common;
use common::*;

use ckbadger_store::types::{FiberChannel, FiberChannelState};

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
    if let Ok(dir) = std::env::var("D7_FIXTURE_DIR") {
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            format!("{dir}/fiber-force-closed.json"),
            serde_json::to_vec_pretty(&body).unwrap(),
        )
        .unwrap();
    }
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
    if let Ok(dir) = std::env::var("D7_FIXTURE_DIR") {
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            format!("{dir}/fiber-network.json"),
            serde_json::to_vec_pretty(&body).unwrap(),
        )
        .unwrap();
        std::fs::write(
            format!("{dir}/fiber-open.json"),
            serde_json::to_vec_pretty(&open).unwrap(),
        )
        .unwrap();
    }
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
