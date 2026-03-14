use axum::{
    extract::{Path, Query, State},
    routing::get,
    Router,
};
use ckbadger_store::types::FiberChannelState;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::response::{default_limit, ok, ApiError, ApiResult, CursorPaginatedResponse};
use crate::utils::{address_to_lock_script_hash, is_ckb_address};
use crate::AppState;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/fiber/channels", get(list_channels))
        .route("/fiber/channels/{channel_id}", get(get_channel))
        .route(
            "/addresses/{addr}/fiber/channels",
            get(get_address_channels),
        )
        .route("/fiber/stats", get(get_stats))
}

// ── Response types ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FiberChannelResponse {
    pub channel_id: String,
    pub state: String,
    pub capacity: String,
    pub udt_type_hash: Option<String>,
    pub udt_amount: Option<String>,
    /// Participant identifiers — CKB addresses when resolvable, hex lock_hash otherwise.
    pub participants: Vec<String>,
    pub funding_tx_hash: String,
    pub funding_output_index: u32,
    pub open_block: i64,
    pub open_timestamp: String,
    pub close_tx_hash: Option<String>,
    pub close_block: Option<i64>,
    pub close_timestamp: Option<String>,
    pub commitment_tx_hash: Option<String>,
    pub delay_epoch: Option<u64>,
    pub settlement_tx_hash: Option<String>,
    pub settlement_block: Option<i64>,
    pub settlement_timestamp: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FiberChannelDetailResponse {
    #[serde(flatten)]
    pub channel: FiberChannelResponse,
    pub timeline: Vec<FiberTimelineEvent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FiberTimelineEvent {
    pub event: String,
    pub tx_hash: String,
    pub block: i64,
    pub timestamp: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FiberStatsResponse {
    pub total_channels: u64,
    pub open_channels: u64,
    pub total_capacity_locked: String,
}

// ── Query / path params ─────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct ListChannelsParams {
    #[serde(default = "default_limit")]
    limit: i64,
    cursor: Option<String>,
    state: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct AddressChannelsParams {
    #[serde(default = "default_limit")]
    limit: i64,
}

// ── Helpers ─────────────────────────────────────────────────────────────

fn state_to_str(state: FiberChannelState) -> &'static str {
    match state {
        FiberChannelState::Open => "open",
        FiberChannelState::CooperativelyClosed => "cooperativelyClosed",
        FiberChannelState::ForceClosed => "forceClosed",
        FiberChannelState::Settled => "settled",
    }
}

fn parse_state_filter(s: &str) -> Result<FiberChannelState, String> {
    match s {
        "open" => Ok(FiberChannelState::Open),
        "closed" | "cooperativelyClosed" => Ok(FiberChannelState::CooperativelyClosed),
        "force_closed" | "forceClosed" => Ok(FiberChannelState::ForceClosed),
        "settled" => Ok(FiberChannelState::Settled),
        other => Err(format!(
            "invalid state filter '{}': expected open|closed|force_closed|settled",
            other
        )),
    }
}

fn format_hex(bytes: &[u8]) -> String {
    format!("0x{}", hex::encode(bytes))
}

fn format_participant(lock_hash: &[u8]) -> String {
    // FiberChannel stores raw lock_hashes (32 bytes), not lock script components.
    // We cannot reconstruct a CKB address from a lock_hash alone (we would need
    // code_hash + hash_type + args). Display as hex lock_hash.
    format_hex(lock_hash)
}

fn build_channel_response(
    channel_id: &[u8],
    ch: &ckbadger_store::types::FiberChannel,
) -> FiberChannelResponse {
    FiberChannelResponse {
        channel_id: format_hex(channel_id),
        state: state_to_str(ch.state).to_string(),
        capacity: ch.capacity.to_string(),
        udt_type_hash: ch.udt_type_hash.as_ref().map(|h| format_hex(h)),
        udt_amount: ch.udt_amount.map(|a| a.to_string()),
        participants: ch
            .participants
            .iter()
            .map(|p| format_participant(p))
            .collect(),
        funding_tx_hash: format_hex(&ch.funding_tx_hash),
        funding_output_index: ch.funding_output_index,
        open_block: ch.open_block,
        open_timestamp: ch.open_timestamp.to_string(),
        close_tx_hash: ch.close_tx_hash.as_ref().map(|h| format_hex(h)),
        close_block: ch.close_block,
        close_timestamp: ch.close_timestamp.map(|t| t.to_string()),
        commitment_tx_hash: ch.commitment_tx_hash.as_ref().map(|h| format_hex(h)),
        delay_epoch: ch.delay_epoch,
        settlement_tx_hash: ch.settlement_tx_hash.as_ref().map(|h| format_hex(h)),
        settlement_block: ch.settlement_block,
        settlement_timestamp: ch.settlement_timestamp.map(|t| t.to_string()),
    }
}

fn build_timeline(ch: &ckbadger_store::types::FiberChannel) -> Vec<FiberTimelineEvent> {
    let mut timeline = Vec::new();

    // Open event is always present.
    timeline.push(FiberTimelineEvent {
        event: "open".to_string(),
        tx_hash: format_hex(&ch.funding_tx_hash),
        block: ch.open_block,
        timestamp: ch.open_timestamp.to_string(),
    });

    // Close (cooperative) or force close.
    match ch.state {
        FiberChannelState::CooperativelyClosed => {
            if let Some(ref tx) = ch.close_tx_hash {
                timeline.push(FiberTimelineEvent {
                    event: "close".to_string(),
                    tx_hash: format_hex(tx),
                    block: ch.close_block.unwrap_or(0),
                    timestamp: ch
                        .close_timestamp
                        .map(|t| t.to_string())
                        .unwrap_or_default(),
                });
            }
        }
        FiberChannelState::ForceClosed => {
            if let Some(ref tx) = ch.close_tx_hash {
                timeline.push(FiberTimelineEvent {
                    event: "forceClose".to_string(),
                    tx_hash: format_hex(tx),
                    block: ch.close_block.unwrap_or(0),
                    timestamp: ch
                        .close_timestamp
                        .map(|t| t.to_string())
                        .unwrap_or_default(),
                });
            }
        }
        FiberChannelState::Settled => {
            // Force-closed channels that are now settled still have the forceClose event.
            if let Some(ref tx) = ch.close_tx_hash {
                timeline.push(FiberTimelineEvent {
                    event: "forceClose".to_string(),
                    tx_hash: format_hex(tx),
                    block: ch.close_block.unwrap_or(0),
                    timestamp: ch
                        .close_timestamp
                        .map(|t| t.to_string())
                        .unwrap_or_default(),
                });
            }
            if let Some(ref tx) = ch.settlement_tx_hash {
                timeline.push(FiberTimelineEvent {
                    event: "settlement".to_string(),
                    tx_hash: format_hex(tx),
                    block: ch.settlement_block.unwrap_or(0),
                    timestamp: ch
                        .settlement_timestamp
                        .map(|t| t.to_string())
                        .unwrap_or_default(),
                });
            }
        }
        FiberChannelState::Open => { /* no additional events */ }
    }

    timeline
}

// ── Handlers ────────────────────────────────────────────────────────────

/// GET /fiber/channels
async fn list_channels(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListChannelsParams>,
) -> ApiResult<CursorPaginatedResponse<FiberChannelResponse>> {
    let limit = params.limit.clamp(1, 100) as usize;

    let state_filter = match params.state.as_deref() {
        Some(s) => Some(parse_state_filter(s).map_err(ApiError::bad_request)?),
        None => None,
    };

    let cursor_bytes = match params.cursor.as_deref() {
        Some(c) => {
            let stripped = c.strip_prefix("0x").unwrap_or(c);
            let bytes = hex::decode(stripped)
                .map_err(|_| ApiError::bad_request("invalid cursor: expected hex channel_id"))?;
            Some(bytes)
        }
        None => None,
    };

    // Fetch limit+1 to determine has_more.
    let store = state.store.clone();
    let rows = tokio::task::spawn_blocking(move || {
        store.list_fiber_channels(limit + 1, cursor_bytes.as_deref(), state_filter)
    })
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?
    .map_err(|e| ApiError::internal(e.to_string()))?;

    let has_more = rows.len() > limit;
    let page: Vec<_> = rows.into_iter().take(limit).collect();

    let next_cursor = if has_more {
        page.last().map(|(id, _)| format_hex(id))
    } else {
        None
    };

    let data: Vec<FiberChannelResponse> = page
        .iter()
        .map(|(id, ch)| build_channel_response(id, ch))
        .collect();

    ok(CursorPaginatedResponse::without_total(
        data,
        limit as i64,
        next_cursor,
    ))
}

/// GET /fiber/channels/{channel_id}
async fn get_channel(
    State(state): State<Arc<AppState>>,
    Path(channel_id): Path<String>,
) -> ApiResult<FiberChannelDetailResponse> {
    let stripped = channel_id.strip_prefix("0x").unwrap_or(&channel_id);
    let id_bytes = hex::decode(stripped)
        .map_err(|_| ApiError::bad_request("invalid channel_id: expected hex string"))?;

    let store = state.store.clone();
    let id_bytes_c = id_bytes.clone();
    let channel = tokio::task::spawn_blocking(move || store.get_fiber_channel(&id_bytes_c))
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .map_err(|e| ApiError::internal(e.to_string()))?
        .ok_or_else(|| ApiError::not_found("Fiber channel not found"))?;

    let resp = build_channel_response(&id_bytes, &channel);
    let timeline = build_timeline(&channel);

    ok(FiberChannelDetailResponse {
        channel: resp,
        timeline,
    })
}

/// GET /addresses/{addr}/fiber/channels
async fn get_address_channels(
    State(state): State<Arc<AppState>>,
    Path(addr): Path<String>,
    Query(params): Query<AddressChannelsParams>,
) -> ApiResult<Vec<FiberChannelResponse>> {
    let limit = params.limit.clamp(1, 100) as usize;

    let lock_hash = if is_ckb_address(&addr) {
        address_to_lock_script_hash(&addr)
            .map_err(|e| ApiError::bad_request(format!("Invalid address: {}", e)))?
    } else {
        hex::decode(addr.strip_prefix("0x").unwrap_or(&addr))
            .map_err(|_| ApiError::bad_request("Invalid address/lock script hash"))?
    };

    let store = state.store.clone();
    let rows =
        tokio::task::spawn_blocking(move || store.list_addr_fiber_channels(&lock_hash, limit))
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?
            .map_err(|e| ApiError::internal(e.to_string()))?;

    let data: Vec<FiberChannelResponse> = rows
        .iter()
        .map(|(id, ch)| build_channel_response(id, ch))
        .collect();

    ok(data)
}

/// GET /fiber/stats
async fn get_stats(State(state): State<Arc<AppState>>) -> ApiResult<FiberStatsResponse> {
    // Iterate all channels to compute stats. For a small-to-moderate number of
    // Fiber channels this is fine. If the number grows significantly, a
    // pre-aggregated counter should be added to the store.
    let store = state.store.clone();
    let stats = tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
        let mut total_channels: u64 = 0;
        let mut open_channels: u64 = 0;
        let mut total_capacity_locked: u128 = 0;

        let batch_size = 500;
        let mut cursor: Option<Vec<u8>> = None;

        loop {
            let rows = store.list_fiber_channels(batch_size, cursor.as_deref(), None)?;

            if rows.is_empty() {
                break;
            }

            for (id, ch) in &rows {
                total_channels += 1;
                if ch.state == FiberChannelState::Open {
                    open_channels += 1;
                    total_capacity_locked += ch.capacity as u128;
                }
                cursor = Some(id.clone());
            }

            if rows.len() < batch_size {
                break;
            }
        }

        Ok(FiberStatsResponse {
            total_channels,
            open_channels,
            total_capacity_locked: total_capacity_locked.to_string(),
        })
    })
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?
    .map_err(|e| ApiError::internal(e.to_string()))?;

    ok(stats)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_state_filter_valid() {
        assert_eq!(parse_state_filter("open").unwrap(), FiberChannelState::Open);
        assert_eq!(
            parse_state_filter("closed").unwrap(),
            FiberChannelState::CooperativelyClosed
        );
        assert_eq!(
            parse_state_filter("cooperativelyClosed").unwrap(),
            FiberChannelState::CooperativelyClosed
        );
        assert_eq!(
            parse_state_filter("force_closed").unwrap(),
            FiberChannelState::ForceClosed
        );
        assert_eq!(
            parse_state_filter("forceClosed").unwrap(),
            FiberChannelState::ForceClosed
        );
        assert_eq!(
            parse_state_filter("settled").unwrap(),
            FiberChannelState::Settled
        );
    }

    #[test]
    fn test_parse_state_filter_invalid() {
        assert!(parse_state_filter("unknown").is_err());
        assert!(parse_state_filter("").is_err());
    }

    #[test]
    fn test_state_to_str() {
        assert_eq!(state_to_str(FiberChannelState::Open), "open");
        assert_eq!(
            state_to_str(FiberChannelState::CooperativelyClosed),
            "cooperativelyClosed"
        );
        assert_eq!(state_to_str(FiberChannelState::ForceClosed), "forceClosed");
        assert_eq!(state_to_str(FiberChannelState::Settled), "settled");
    }

    #[test]
    fn test_format_hex() {
        assert_eq!(format_hex(&[0xab, 0xcd]), "0xabcd");
        assert_eq!(format_hex(&[]), "0x");
    }

    #[test]
    fn test_build_channel_response() {
        let channel = ckbadger_store::types::FiberChannel {
            funding_tx_hash: vec![0x11; 32],
            funding_output_index: 0,
            state: FiberChannelState::Open,
            capacity: 500_00000000,
            udt_type_hash: None,
            udt_amount: None,
            open_block: 100,
            open_timestamp: 1_700_000_000,
            close_tx_hash: None,
            close_block: None,
            close_timestamp: None,
            commitment_tx_hash: None,
            commitment_output_index: None,
            delay_epoch: None,
            settlement_tx_hash: None,
            settlement_block: None,
            settlement_timestamp: None,
            participants: vec![vec![0xAA; 32], vec![0xBB; 32]],
            funding_lock_args: vec![0xCC; 20],
        };
        let id = [0xFF; 32];
        let resp = build_channel_response(&id, &channel);

        assert_eq!(resp.channel_id, format_hex(&id));
        assert_eq!(resp.state, "open");
        assert_eq!(resp.capacity, "50000000000");
        assert_eq!(resp.participants.len(), 2);
        assert_eq!(resp.participants[0], format_hex(&[0xAA; 32]));
        assert_eq!(resp.funding_output_index, 0);
        assert!(resp.close_tx_hash.is_none());
    }

    #[test]
    fn test_build_timeline_open() {
        let channel = ckbadger_store::types::FiberChannel {
            funding_tx_hash: vec![0x11; 32],
            funding_output_index: 0,
            state: FiberChannelState::Open,
            capacity: 100,
            udt_type_hash: None,
            udt_amount: None,
            open_block: 10,
            open_timestamp: 1000,
            close_tx_hash: None,
            close_block: None,
            close_timestamp: None,
            commitment_tx_hash: None,
            commitment_output_index: None,
            delay_epoch: None,
            settlement_tx_hash: None,
            settlement_block: None,
            settlement_timestamp: None,
            participants: vec![],
            funding_lock_args: vec![],
        };
        let timeline = build_timeline(&channel);
        assert_eq!(timeline.len(), 1);
        assert_eq!(timeline[0].event, "open");
    }

    #[test]
    fn test_build_timeline_cooperative_close() {
        let channel = ckbadger_store::types::FiberChannel {
            funding_tx_hash: vec![0x11; 32],
            funding_output_index: 0,
            state: FiberChannelState::CooperativelyClosed,
            capacity: 100,
            udt_type_hash: None,
            udt_amount: None,
            open_block: 10,
            open_timestamp: 1000,
            close_tx_hash: Some(vec![0x22; 32]),
            close_block: Some(20),
            close_timestamp: Some(2000),
            commitment_tx_hash: None,
            commitment_output_index: None,
            delay_epoch: None,
            settlement_tx_hash: None,
            settlement_block: None,
            settlement_timestamp: None,
            participants: vec![],
            funding_lock_args: vec![],
        };
        let timeline = build_timeline(&channel);
        assert_eq!(timeline.len(), 2);
        assert_eq!(timeline[0].event, "open");
        assert_eq!(timeline[1].event, "close");
    }

    #[test]
    fn test_build_timeline_settled() {
        let channel = ckbadger_store::types::FiberChannel {
            funding_tx_hash: vec![0x11; 32],
            funding_output_index: 0,
            state: FiberChannelState::Settled,
            capacity: 100,
            udt_type_hash: None,
            udt_amount: None,
            open_block: 10,
            open_timestamp: 1000,
            close_tx_hash: Some(vec![0x22; 32]),
            close_block: Some(20),
            close_timestamp: Some(2000),
            commitment_tx_hash: Some(vec![0x33; 32]),
            commitment_output_index: Some(0),
            delay_epoch: Some(10),
            settlement_tx_hash: Some(vec![0x44; 32]),
            settlement_block: Some(30),
            settlement_timestamp: Some(3000),
            participants: vec![],
            funding_lock_args: vec![],
        };
        let timeline = build_timeline(&channel);
        assert_eq!(timeline.len(), 3);
        assert_eq!(timeline[0].event, "open");
        assert_eq!(timeline[1].event, "forceClose");
        assert_eq!(timeline[2].event, "settlement");
    }

    #[test]
    fn test_fiber_channel_response_serialization() {
        let resp = FiberChannelResponse {
            channel_id: "0xabcd".to_string(),
            state: "open".to_string(),
            capacity: "50000000000".to_string(),
            udt_type_hash: None,
            udt_amount: None,
            participants: vec!["0xaa".to_string()],
            funding_tx_hash: "0x1111".to_string(),
            funding_output_index: 0,
            open_block: 100,
            open_timestamp: "1700000000".to_string(),
            close_tx_hash: None,
            close_block: None,
            close_timestamp: None,
            commitment_tx_hash: None,
            delay_epoch: None,
            settlement_tx_hash: None,
            settlement_block: None,
            settlement_timestamp: None,
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["channelId"], "0xabcd");
        assert_eq!(json["state"], "open");
        assert_eq!(json["fundingTxHash"], "0x1111");
        assert_eq!(json["openBlock"], 100);
        assert!(json.get("closeTxHash").unwrap().is_null());
    }

    #[test]
    fn test_fiber_stats_response_serialization() {
        let resp = FiberStatsResponse {
            total_channels: 42,
            open_channels: 10,
            total_capacity_locked: "500000000000".to_string(),
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["totalChannels"], 42);
        assert_eq!(json["openChannels"], 10);
        assert_eq!(json["totalCapacityLocked"], "500000000000");
    }

    #[test]
    fn test_fiber_detail_response_flattens() {
        let channel_resp = FiberChannelResponse {
            channel_id: "0xaa".to_string(),
            state: "open".to_string(),
            capacity: "100".to_string(),
            udt_type_hash: None,
            udt_amount: None,
            participants: vec![],
            funding_tx_hash: "0xbb".to_string(),
            funding_output_index: 0,
            open_block: 1,
            open_timestamp: "1000".to_string(),
            close_tx_hash: None,
            close_block: None,
            close_timestamp: None,
            commitment_tx_hash: None,
            delay_epoch: None,
            settlement_tx_hash: None,
            settlement_block: None,
            settlement_timestamp: None,
        };
        let detail = FiberChannelDetailResponse {
            channel: channel_resp,
            timeline: vec![FiberTimelineEvent {
                event: "open".to_string(),
                tx_hash: "0xbb".to_string(),
                block: 1,
                timestamp: "1000".to_string(),
            }],
        };
        let json = serde_json::to_value(&detail).unwrap();
        // Flattened: channelId should be at top level
        assert_eq!(json["channelId"], "0xaa");
        assert!(json["timeline"].is_array());
        assert_eq!(json["timeline"][0]["event"], "open");
    }
}
