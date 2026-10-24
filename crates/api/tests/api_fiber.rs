mod common;
use common::*;

use ckbadger_store::types::{FiberChannel, FiberChannelState};

/// Regression (API-020, typed responses): a force-closed channel's
/// `delay_epoch` is the commitment lock's `since` value with its relative-epoch
/// flag bits set (`0xa001000000000006` on real commitment locks), far above
/// 2^53. Served as a JSON number it was rounded in the browser; it is an exact
/// decimal string now, in the list and the detail.
#[tokio::test]
async fn test_fiber_channel_delay_epoch_is_an_exact_decimal_string() {
    let store = test_store();
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
