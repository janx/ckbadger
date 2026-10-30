use axum::{
    extract::{Path, Query, State},
    routing::get,
    Router,
};
use ckbadger_store::types::FiberChannelState;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, sync::Arc};

use crate::response::{
    default_limit, ok, ApiError, ApiResult, ApiRouteError, CursorPaginatedResponse,
};
use crate::utils::script_resolution::registered_script_name;
use crate::utils::{address_to_lock_script_hash, is_ckb_address, parse_hash32, script_to_address};
use crate::AppState;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/fiber/channels", get(list_channels))
        .route("/fiber/channels/{channel_id}", get(get_channel))
        .route(
            "/fiber/channels/{channel_id}/evidence",
            get(get_channel_evidence),
        )
        .route(
            "/addresses/{addr}/fiber/channels",
            get(get_address_channels),
        )
        .route("/fiber/stats", get(get_stats))
        .route("/fiber/network", get(get_network))
}

mod evidence;
use evidence::get_channel_evidence;

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
    pub observed_parties: Vec<FiberParticipantResponse>,
    /// Duration from opening to close, or opening to the indexed tip for an
    /// open channel, using the monotone canonical chain-time clock.
    pub observed_lifetime_ms: Option<String>,
    pub lifetime_anchor_block: Option<i64>,
    pub funding_tx_hash: String,
    pub funding_output_index: u32,
    pub open_block: i64,
    pub open_timestamp: String,
    pub close_tx_hash: Option<String>,
    pub close_block: Option<i64>,
    pub close_timestamp: Option<String>,
    pub commitment_tx_hash: Option<String>,
    /// The commitment lock's `since`-encoded delay as an exact decimal string:
    /// its relative-epoch flag bits put it above 2^53, where a JSON number is
    /// rounded by every browser.
    pub delay_epoch: Option<String>,
    pub delay_epoch_decoded: Option<FiberEpochDelay>,
    pub settlement_tx_hash: Option<String>,
    pub settlement_block: Option<i64>,
    pub settlement_timestamp: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FiberParticipantResponse {
    pub lock_script_hash: String,
    pub address: Option<String>,
    /// The script registry's name for the party's lock code hash ("JoyID",
    /// "Default Lock", …); `None` when the lock is not indexed or unregistered.
    pub lock_script_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FiberEpochDelay {
    pub relative: bool,
    pub epochs: u64,
    pub index: u64,
    pub length: u64,
}

fn decode_epoch_delay(raw: u64) -> Result<FiberEpochDelay, String> {
    if raw & 0x6000_0000_0000_0000 != 0x2000_0000_0000_0000 || raw & 0x1f00_0000_0000_0000 != 0 {
        return Err(format!(
            "Fiber commitment delay is not a valid epoch since: {raw:#x}"
        ));
    }
    let index = (raw >> 24) & 0xffff;
    let length = (raw >> 40) & 0xffff;
    // RFC 0017 normalizes the encoded zero fraction 0/0 to 0/1.
    if (length == 0 && index != 0) || (length != 0 && index >= length) {
        return Err(format!(
            "Fiber commitment delay has invalid epoch fraction: {raw:#x} ({index}/{length})"
        ));
    }
    Ok(FiberEpochDelay {
        relative: raw >> 63 != 0,
        epochs: raw & 0xff_ffff,
        index,
        length: if length == 0 { 1 } else { length },
    })
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
    #[serde(rename = "blockNumber")]
    pub block: i64,
    pub timestamp: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FiberStatsResponse {
    pub total_channels: u64,
    pub open_channels: u64,
    pub total_capacity_locked: String,
    pub cooperatively_closed_channels: u64,
    pub force_closed_waiting_channels: u64,
    pub force_closed_settled_channels: u64,
    pub observed_parties: u64,
    pub observed_pairs: u64,
    pub first_open_timestamp: Option<String>,
    pub typical_closed_lifetime_ms: Option<String>,
    pub closed_within_hour: u64,
    pub tip_block_number: i64,
    pub tip_chain_time_ms: String,
    /// UDT-funded channels per token type hash ("1 in USDI"), by type hash.
    pub channels_by_token: Vec<FiberTokenChannelsResponse>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FiberTokenChannelsResponse {
    pub udt_type_hash: String,
    /// The token's indexed symbol (the token page's); `None` when not indexed.
    pub symbol: Option<String>,
    pub channels: u64,
    pub open_channels: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FiberPartyResponse {
    pub lock_script_hash: String,
    /// Unknown is a valid observation when the indexed lock script is absent.
    pub address: Option<String>,
    pub lock_script_name: Option<String>,
    pub channels: u64,
    pub open_channels: u64,
    pub cooperatively_closed_channels: u64,
    pub force_closed_waiting_channels: u64,
    pub force_closed_settled_channels: u64,
    pub open_capacity: String,
    /// The party's earliest channel opening (block and its header timestamp, ms).
    pub first_open_block: i64,
    pub first_open_timestamp: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FiberPairResponse {
    pub first_lock_script_hash: String,
    pub second_lock_script_hash: String,
    pub channels: u64,
    pub open_channels: u64,
    pub cooperatively_closed_channels: u64,
    pub force_closed_waiting_channels: u64,
    pub force_closed_settled_channels: u64,
    pub open_capacity: String,
    pub first_open_block: i64,
    pub last_open_block: i64,
    /// Header timestamps (ms) of `firstOpenBlock` / `lastOpenBlock`.
    pub first_open_timestamp: String,
    pub last_open_timestamp: String,
    pub typical_closed_lifetime_ms: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FiberNetworkResponse {
    pub summary: FiberStatsResponse,
    pub parties: Vec<FiberPartyResponse>,
    pub pairs: Vec<FiberPairResponse>,
}

// ── Query / path params ─────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct ListChannelsParams {
    #[serde(default = "default_limit")]
    limit: i64,
    cursor: Option<String>,
    state: Option<String>,
    /// `id` (default, channel id ascending) or `capacity` (capacity
    /// descending, channel id ascending among equals).
    sort: Option<String>,
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
) -> Result<FiberChannelResponse, String> {
    Ok(FiberChannelResponse {
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
        observed_parties: Vec::new(),
        observed_lifetime_ms: None,
        lifetime_anchor_block: None,
        funding_tx_hash: format_hex(&ch.funding_tx_hash),
        funding_output_index: ch.funding_output_index,
        open_block: ch.open_block,
        open_timestamp: ch.open_timestamp.to_string(),
        close_tx_hash: ch.close_tx_hash.as_ref().map(|h| format_hex(h)),
        close_block: ch.close_block,
        close_timestamp: ch.close_timestamp.map(|t| t.to_string()),
        commitment_tx_hash: ch.commitment_tx_hash.as_ref().map(|h| format_hex(h)),
        delay_epoch: ch.delay_epoch.map(|delay| delay.to_string()),
        delay_epoch_decoded: ch.delay_epoch.map(decode_epoch_delay).transpose()?,
        settlement_tx_hash: ch.settlement_tx_hash.as_ref().map(|h| format_hex(h)),
        settlement_block: ch.settlement_block,
        settlement_timestamp: ch.settlement_timestamp.map(|t| t.to_string()),
    })
}

fn build_timeline(
    ch: &ckbadger_store::types::FiberChannel,
) -> Result<Vec<FiberTimelineEvent>, String> {
    let mut timeline = Vec::new();

    // Open event is always present.
    timeline.push(FiberTimelineEvent {
        event: "open".to_string(),
        tx_hash: format_hex(&ch.funding_tx_hash),
        block: ch.open_block,
        timestamp: ch.open_timestamp.to_string(),
    });

    let close_event = match ch.state {
        FiberChannelState::Open => None,
        FiberChannelState::CooperativelyClosed => Some("close"),
        FiberChannelState::ForceClosed | FiberChannelState::Settled => Some("forceClose"),
    };
    if let Some(event) = close_event {
        let tx = ch
            .close_tx_hash
            .as_ref()
            .ok_or("closed channel has no close_tx_hash")?;
        let block = ch.close_block.ok_or("closed channel has no close_block")?;
        let timestamp = ch
            .close_timestamp
            .ok_or("closed channel has no close_timestamp")?;
        timeline.push(FiberTimelineEvent {
            event: event.to_string(),
            tx_hash: format_hex(tx),
            block,
            timestamp: timestamp.to_string(),
        });
    }
    if ch.state == FiberChannelState::Settled {
        let tx = ch
            .settlement_tx_hash
            .as_ref()
            .ok_or("settled channel has no settlement_tx_hash")?;
        let block = ch
            .settlement_block
            .ok_or("settled channel has no settlement_block")?;
        let timestamp = ch
            .settlement_timestamp
            .ok_or("settled channel has no settlement_timestamp")?;
        timeline.push(FiberTimelineEvent {
            event: "settlement".to_string(),
            tx_hash: format_hex(tx),
            block,
            timestamp: timestamp.to_string(),
        });
    }

    Ok(timeline)
}

fn observed_party(
    store: &ckbadger_store::CkbadgerStore,
    network: &str,
    lock_hash: &[u8],
) -> anyhow::Result<FiberParticipantResponse> {
    anyhow::ensure!(
        lock_hash.len() == 32,
        "Fiber party lock hash has {} bytes",
        lock_hash.len()
    );
    let (address, lock_script_name) = match store.get_lock_script(lock_hash)? {
        Some(lock) => {
            let hash_type = u8::try_from(lock.hash_type)?;
            anyhow::ensure!(
                crate::utils::address::compute_script_hash(&lock.code_hash, hash_type, &lock.args)
                    == lock_hash,
                "Fiber party lock payload does not hash to indexed lock 0x{}",
                hex::encode(lock_hash)
            );
            (
                Some(
                    script_to_address(&lock.code_hash, lock.hash_type, &lock.args, network)
                        .map_err(anyhow::Error::msg)?,
                ),
                registered_script_name(store, &lock.code_hash)?,
            )
        }
        None => (None, None),
    };
    Ok(FiberParticipantResponse {
        lock_script_hash: format_hex(lock_hash),
        address,
        lock_script_name,
    })
}

fn canonical_header(
    store: &ckbadger_store::CkbadgerStore,
    number: i64,
) -> anyhow::Result<ckbadger_store::types::CachedBlockHeader> {
    store.get_block_header(number)?.ok_or_else(|| {
        anyhow::anyhow!("Fiber channel requires missing canonical block header #{number}")
    })
}

fn channel_lifetime(
    store: &ckbadger_store::CkbadgerStore,
    id: &[u8],
    ch: &ckbadger_store::types::FiberChannel,
    tip_number: i64,
    tip_chain_time_ms: i64,
) -> anyhow::Result<(i64, i64)> {
    anyhow::ensure!(
        ch.open_block >= 0 && ch.open_block <= tip_number,
        "Fiber channel 0x{} has invalid open block {} at tip {tip_number}",
        hex::encode(id),
        ch.open_block
    );
    let open = canonical_header(store, ch.open_block)?;
    anyhow::ensure!(
        open.timestamp == ch.open_timestamp,
        "Fiber channel 0x{} open timestamp {} differs from block {} header {}",
        hex::encode(id),
        ch.open_timestamp,
        ch.open_block,
        open.timestamp
    );
    let (anchor_number, anchor_chain_time_ms) = match ch.state {
        FiberChannelState::Open => (tip_number, tip_chain_time_ms),
        _ => {
            let number = ch.close_block.ok_or_else(|| {
                anyhow::anyhow!(
                    "Fiber channel 0x{} {:?} missing close block",
                    hex::encode(id),
                    ch.state
                )
            })?;
            anyhow::ensure!(
                number >= ch.open_block && number <= tip_number,
                "Fiber channel 0x{} close block {number} outside open {}..tip {tip_number}",
                hex::encode(id),
                ch.open_block
            );
            let close = canonical_header(store, number)?;
            anyhow::ensure!(
                Some(close.timestamp) == ch.close_timestamp,
                "Fiber channel 0x{} close timestamp differs from block {number} header",
                hex::encode(id)
            );
            (number, close.chain_time_ms)
        }
    };
    let duration = anchor_chain_time_ms
        .checked_sub(open.chain_time_ms)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Fiber channel 0x{} chain time regressed from block {} to {anchor_number}",
                hex::encode(id),
                ch.open_block
            )
        })?;
    Ok((duration, anchor_number))
}

fn enrich_channel(
    store: &ckbadger_store::CkbadgerStore,
    network: &str,
    id: &[u8],
    ch: &ckbadger_store::types::FiberChannel,
    tip_number: i64,
    tip_chain_time_ms: i64,
) -> anyhow::Result<FiberChannelResponse> {
    let mut response = build_channel_response(id, ch).map_err(anyhow::Error::msg)?;
    response.observed_parties = ch
        .participants
        .iter()
        .map(|hash| observed_party(store, network, hash))
        .collect::<anyhow::Result<Vec<_>>>()?;
    let (duration, anchor) = channel_lifetime(store, id, ch, tip_number, tip_chain_time_ms)?;
    response.observed_lifetime_ms = Some(duration.to_string());
    response.lifetime_anchor_block = Some(anchor);
    Ok(response)
}

#[derive(Default)]
struct PairCounts {
    channels: u64,
    open: u64,
    cooperative: u64,
    waiting: u64,
    settled: u64,
    open_capacity: u128,
    first_open: Option<(i64, i64)>,
    last_open: Option<(i64, i64)>,
    closed_lifetimes: Vec<i64>,
}

#[derive(Default)]
struct PartyCounts {
    channels: u64,
    open: u64,
    cooperative: u64,
    waiting: u64,
    settled: u64,
    open_capacity: u128,
    first_open: Option<(i64, i64)>,
}

/// Every indexed channel, in channel-id order.
fn load_channels(
    store: &ckbadger_store::CkbadgerStore,
) -> anyhow::Result<Vec<(Vec<u8>, ckbadger_store::types::FiberChannel)>> {
    let mut channels = Vec::new();
    let mut cursor: Option<Vec<u8>> = None;
    loop {
        let rows = store.list_fiber_channels(500, cursor.as_deref(), None)?;
        let full = rows.len() == 500;
        cursor = rows.last().map(|(id, _)| id.clone());
        channels.extend(rows);
        if !full {
            break;
        }
    }
    Ok(channels)
}

/// One canonical tip's Fiber channels and network aggregate, built once in
/// one pinned read view and served from API memory (never written to a
/// store) until the tip moves: channels change only when blocks commit.
pub struct FiberView {
    tip_number: i64,
    tip_chain_time_ms: i64,
    channels: Vec<(Vec<u8>, ckbadger_store::types::FiberChannel)>,
    network: FiberNetworkResponse,
}

/// Call on a blocking thread.
fn fiber_view(state: &AppState) -> anyhow::Result<Arc<FiberView>> {
    let (tip_number, tip) = state
        .store
        .get_sync_tip_block()?
        .ok_or_else(|| anyhow::anyhow!("Fiber network unavailable before first committed block"))?;
    if let Some(view) = state.fiber_view.get(tip_number, &tip.hash) {
        return Ok(view);
    }
    let channels = load_channels(&state.store)?;
    let network = aggregate_network(
        &state.store,
        &state.ckb_network,
        &channels,
        tip_number,
        &tip,
    )?;
    let view = Arc::new(FiberView {
        tip_number,
        tip_chain_time_ms: tip.chain_time_ms,
        channels,
        network,
    });
    state.fiber_view.set(tip_number, tip.hash, view.clone());
    Ok(view)
}

/// `/fiber/channels` order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChannelSort {
    Id,
    Capacity,
}

/// A page cursor: the last row's channel id, plus its capacity under `sort=capacity`
/// (`"<capacity>:0x<channel id>"`).
fn parse_channel_cursor(
    sort: ChannelSort,
    raw: &str,
) -> Result<(Option<u64>, Vec<u8>), ApiRouteError> {
    match sort {
        ChannelSort::Id => Ok((None, parse_hash32(raw, "cursor (expected a channel_id)")?)),
        ChannelSort::Capacity => {
            let invalid = || {
                ApiError::bad_request(format!(
                    "Invalid capacity cursor (expected <capacity>:0x<channel_id>): {raw}"
                ))
            };
            let (capacity, id) = raw.split_once(':').ok_or_else(invalid)?;
            let canonical = capacity == "0"
                || (!capacity.is_empty()
                    && !capacity.starts_with('0')
                    && capacity.bytes().all(|b| b.is_ascii_digit()));
            if !canonical {
                return Err(invalid());
            }
            let capacity = capacity.parse::<u64>().map_err(|_| invalid())?;
            Ok((
                Some(capacity),
                parse_hash32(id, "cursor (expected <capacity>:0x<channel_id>)")?,
            ))
        }
    }
}

fn median_lifetime(mut durations: Vec<i64>) -> Option<String> {
    if durations.is_empty() {
        return None;
    }
    durations.sort_unstable();
    Some(durations[(durations.len() - 1) / 2].to_string())
}

fn aggregate_network(
    store: &ckbadger_store::CkbadgerStore,
    network: &str,
    channels: &[(Vec<u8>, ckbadger_store::types::FiberChannel)],
    tip_number: i64,
    tip: &ckbadger_store::types::CachedBlockHeader,
) -> anyhow::Result<FiberNetworkResponse> {
    let mut summary = FiberStatsResponse {
        total_channels: 0,
        open_channels: 0,
        total_capacity_locked: "0".into(),
        cooperatively_closed_channels: 0,
        force_closed_waiting_channels: 0,
        force_closed_settled_channels: 0,
        observed_parties: 0,
        observed_pairs: 0,
        first_open_timestamp: None,
        typical_closed_lifetime_ms: None,
        closed_within_hour: 0,
        tip_block_number: tip_number,
        tip_chain_time_ms: tip.chain_time_ms.to_string(),
        channels_by_token: Vec::new(),
    };
    let mut tokens = BTreeMap::<Vec<u8>, (u64, u64)>::new();
    let mut parties = BTreeMap::<Vec<u8>, PartyCounts>::new();
    let mut pairs = BTreeMap::<(Vec<u8>, Vec<u8>), PairCounts>::new();
    let mut all_closed_lifetimes = Vec::new();
    let mut first_open: Option<(i64, i64)> = None;
    let mut locked = 0u128;
    {
        for (id, ch) in channels {
            let (lifetime, _) = channel_lifetime(store, id, ch, tip_number, tip.chain_time_ms)?;
            summary.total_channels = summary
                .total_channels
                .checked_add(1)
                .ok_or_else(|| anyhow::anyhow!("Fiber channel count overflow"))?;
            if first_open.is_none_or(|(block, _)| ch.open_block < block) {
                first_open = Some((ch.open_block, ch.open_timestamp));
            }
            if let Some(udt) = &ch.udt_type_hash {
                let row = tokens.entry(udt.clone()).or_default();
                row.0 += 1;
                if ch.state == FiberChannelState::Open {
                    row.1 += 1;
                }
            }
            if ch.state == FiberChannelState::Open {
                summary.open_channels += 1;
                locked = locked
                    .checked_add(u128::from(ch.capacity))
                    .ok_or_else(|| anyhow::anyhow!("Fiber locked capacity overflow"))?;
            } else {
                all_closed_lifetimes.push(lifetime);
                if lifetime < 3_600_000 {
                    summary.closed_within_hour += 1;
                }
                match ch.state {
                    FiberChannelState::CooperativelyClosed => {
                        summary.cooperatively_closed_channels += 1
                    }
                    FiberChannelState::ForceClosed => summary.force_closed_waiting_channels += 1,
                    FiberChannelState::Settled => summary.force_closed_settled_channels += 1,
                    FiberChannelState::Open => unreachable!(),
                }
            }
            for hash in &ch.participants {
                anyhow::ensure!(
                    hash.len() == 32,
                    "Fiber channel 0x{} party hash has {} bytes",
                    hex::encode(id),
                    hash.len()
                );
                let row = parties.entry(hash.clone()).or_default();
                row.channels += 1;
                if row
                    .first_open
                    .is_none_or(|(block, _)| ch.open_block < block)
                {
                    row.first_open = Some((ch.open_block, ch.open_timestamp));
                }
                match ch.state {
                    FiberChannelState::Open => {
                        row.open += 1;
                        row.open_capacity = row
                            .open_capacity
                            .checked_add(u128::from(ch.capacity))
                            .ok_or_else(|| anyhow::anyhow!("Fiber party open capacity overflow"))?;
                    }
                    FiberChannelState::CooperativelyClosed => row.cooperative += 1,
                    FiberChannelState::ForceClosed => row.waiting += 1,
                    FiberChannelState::Settled => row.settled += 1,
                }
            }
            if ch.participants.len() == 2 && ch.participants[0] != ch.participants[1] {
                let mut key = [ch.participants[0].clone(), ch.participants[1].clone()];
                key.sort();
                let row = pairs.entry((key[0].clone(), key[1].clone())).or_default();
                row.channels += 1;
                if row
                    .first_open
                    .is_none_or(|(block, _)| ch.open_block < block)
                {
                    row.first_open = Some((ch.open_block, ch.open_timestamp));
                }
                if row.last_open.is_none_or(|(block, _)| ch.open_block > block) {
                    row.last_open = Some((ch.open_block, ch.open_timestamp));
                }
                match ch.state {
                    FiberChannelState::Open => {
                        row.open += 1;
                        row.open_capacity = row
                            .open_capacity
                            .checked_add(u128::from(ch.capacity))
                            .ok_or_else(|| anyhow::anyhow!("Fiber pair open capacity overflow"))?;
                    }
                    FiberChannelState::CooperativelyClosed => row.cooperative += 1,
                    FiberChannelState::ForceClosed => row.waiting += 1,
                    FiberChannelState::Settled => row.settled += 1,
                }
                if ch.state != FiberChannelState::Open {
                    row.closed_lifetimes.push(lifetime);
                }
            }
        }
    }
    summary.total_capacity_locked = locked.to_string();
    summary.first_open_timestamp = first_open.map(|(_, timestamp)| timestamp.to_string());
    summary.typical_closed_lifetime_ms = median_lifetime(all_closed_lifetimes);
    summary.observed_parties = parties.len() as u64;
    summary.observed_pairs = pairs.len() as u64;
    summary.channels_by_token = tokens
        .into_iter()
        .map(|(hash, (channels, open_channels))| {
            Ok(FiberTokenChannelsResponse {
                symbol: store.get_token(&hash)?.and_then(|token| token.symbol),
                udt_type_hash: format_hex(&hash),
                channels,
                open_channels,
            })
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    let parties = parties
        .into_iter()
        .map(|(hash, counts)| {
            let party = observed_party(store, network, &hash)?;
            let (first_open_block, first_open_timestamp) = counts
                .first_open
                .ok_or_else(|| anyhow::anyhow!("Fiber party has no first open block"))?;
            Ok(FiberPartyResponse {
                lock_script_hash: party.lock_script_hash,
                address: party.address,
                lock_script_name: party.lock_script_name,
                channels: counts.channels,
                open_channels: counts.open,
                cooperatively_closed_channels: counts.cooperative,
                force_closed_waiting_channels: counts.waiting,
                force_closed_settled_channels: counts.settled,
                open_capacity: counts.open_capacity.to_string(),
                first_open_block,
                first_open_timestamp: first_open_timestamp.to_string(),
            })
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    let pairs = pairs
        .into_iter()
        .map(|((first, second), counts)| {
            let (first_open_block, first_open_timestamp) = counts
                .first_open
                .ok_or_else(|| anyhow::anyhow!("Fiber pair has no first open block"))?;
            let (last_open_block, last_open_timestamp) = counts
                .last_open
                .ok_or_else(|| anyhow::anyhow!("Fiber pair has no last open block"))?;
            Ok(FiberPairResponse {
                first_lock_script_hash: format_hex(&first),
                second_lock_script_hash: format_hex(&second),
                channels: counts.channels,
                open_channels: counts.open,
                cooperatively_closed_channels: counts.cooperative,
                force_closed_waiting_channels: counts.waiting,
                force_closed_settled_channels: counts.settled,
                open_capacity: counts.open_capacity.to_string(),
                first_open_block,
                last_open_block,
                first_open_timestamp: first_open_timestamp.to_string(),
                last_open_timestamp: last_open_timestamp.to_string(),
                typical_closed_lifetime_ms: median_lifetime(counts.closed_lifetimes),
            })
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    Ok(FiberNetworkResponse {
        summary,
        parties,
        pairs,
    })
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
    let sort = match params.sort.as_deref() {
        None | Some("id") => ChannelSort::Id,
        Some("capacity") => ChannelSort::Capacity,
        Some(other) => {
            return Err(ApiError::bad_request(format!(
                "invalid sort '{other}': expected id|capacity"
            )))
        }
    };
    let cursor = params
        .cursor
        .as_deref()
        .map(|raw| parse_channel_cursor(sort, raw))
        .transpose()?;

    let (data, total, next_cursor) = tokio::task::spawn_blocking(move || {
        let view = fiber_view(&state)?;
        let mut rows: Vec<&(Vec<u8>, ckbadger_store::types::FiberChannel)> = view
            .channels
            .iter()
            .filter(|(_, ch)| state_filter.is_none_or(|wanted| ch.state == wanted))
            .collect();
        if sort == ChannelSort::Capacity {
            rows.sort_by(|(a_id, a), (b_id, b)| {
                b.capacity.cmp(&a.capacity).then_with(|| a_id.cmp(b_id))
            });
        }
        let total = i64::try_from(rows.len())?;
        let after_cursor = |id: &Vec<u8>, ch: &ckbadger_store::types::FiberChannel| match &cursor {
            None => true,
            Some((None, cursor_id)) => id > cursor_id,
            Some((Some(capacity), cursor_id)) => {
                ch.capacity < *capacity || (ch.capacity == *capacity && id > cursor_id)
            }
        };
        let page: Vec<_> = rows
            .into_iter()
            .filter(|(id, ch)| after_cursor(id, ch))
            .take(limit + 1)
            .collect();
        let has_more = page.len() > limit;
        let page = &page[..page.len().min(limit)];
        let next_cursor = if has_more {
            page.last().map(|(id, ch)| match sort {
                ChannelSort::Id => format_hex(id),
                ChannelSort::Capacity => format!("{}:{}", ch.capacity, format_hex(id)),
            })
        } else {
            None
        };
        let data = page
            .iter()
            .map(|(id, ch)| {
                enrich_channel(
                    &state.store,
                    &state.ckb_network,
                    id,
                    ch,
                    view.tip_number,
                    view.tip_chain_time_ms,
                )
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
        Ok::<_, anyhow::Error>((data, total, next_cursor))
    })
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?
    .map_err(|e| ApiError::internal(format!("{e:#}")))?;

    ok(CursorPaginatedResponse::new(
        data,
        total,
        limit as i64,
        next_cursor,
    ))
}

/// GET /fiber/channels/{channel_id}
async fn get_channel(
    State(state): State<Arc<AppState>>,
    Path(channel_id): Path<String>,
) -> ApiResult<FiberChannelDetailResponse> {
    let id_bytes = parse_hash32(&channel_id, "channel_id")?;

    let store = state.store.clone();
    let id_bytes_c = id_bytes.clone();
    let channel = tokio::task::spawn_blocking(move || store.get_fiber_channel(&id_bytes_c))
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .map_err(|e| ApiError::internal(e.to_string()))?
        .ok_or_else(|| ApiError::not_found("Fiber channel not found"))?;

    let (tip_number, tip) = state
        .store
        .get_sync_tip_block()
        .map_err(|e| ApiError::internal(e.to_string()))?
        .ok_or_else(|| ApiError::initializing("Fiber channel requires an indexed tip"))?;
    let resp = enrich_channel(
        &state.store,
        &state.ckb_network,
        &id_bytes,
        &channel,
        tip_number,
        tip.chain_time_ms,
    )
    .map_err(|e| ApiError::internal(e.to_string()))?;
    let timeline = build_timeline(&channel)
        .map_err(|error| ApiError::internal(format!("fiber channel {}: {error}", channel_id)))?;

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
        parse_hash32(&addr, "address/lock script hash")?
    };

    let store = state.store.clone();
    let rows =
        tokio::task::spawn_blocking(move || store.list_addr_fiber_channels(&lock_hash, limit))
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?
            .map_err(|e| ApiError::internal(e.to_string()))?;

    let (tip_number, tip) = state
        .store
        .get_sync_tip_block()
        .map_err(|e| ApiError::internal(e.to_string()))?
        .ok_or_else(|| ApiError::initializing("Fiber channels require an indexed tip"))?;
    let data: Vec<FiberChannelResponse> = rows
        .iter()
        .map(|(id, ch)| {
            enrich_channel(
                &state.store,
                &state.ckb_network,
                id,
                ch,
                tip_number,
                tip.chain_time_ms,
            )
        })
        .collect::<anyhow::Result<_>>()
        .map_err(|e| ApiError::internal(e.to_string()))?;

    ok(data)
}

/// GET /fiber/stats
async fn get_stats(State(state): State<Arc<AppState>>) -> ApiResult<FiberStatsResponse> {
    let view = tokio::task::spawn_blocking(move || fiber_view(&state))
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .map_err(|e| ApiError::internal(e.to_string()))?;
    ok(view.network.summary.clone())
}

async fn get_network(State(state): State<Arc<AppState>>) -> ApiResult<FiberNetworkResponse> {
    let view = tokio::task::spawn_blocking(move || fiber_view(&state))
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .map_err(|e| ApiError::internal(e.to_string()))?;
    ok(view.network.clone())
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
    fn zero_epoch_fraction_is_valid_and_nonzero_over_zero_is_not() {
        // RFC 0017 permits 0/0 as an encoding of 0/1.
        let zero = decode_epoch_delay(0xa000_0000_0000_0001).unwrap();
        assert_eq!((zero.epochs, zero.index, zero.length), (1, 0, 1));
        assert!(decode_epoch_delay(0xa000_0000_0100_0001).is_err());
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
        let resp = build_channel_response(&id, &channel).unwrap();

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
        let timeline = build_timeline(&channel).unwrap();
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
        let timeline = build_timeline(&channel).unwrap();
        assert_eq!(timeline.len(), 2);
        assert_eq!(timeline[0].event, "open");
        assert_eq!(timeline[1].event, "close");

        let mut missing_block = channel.clone();
        missing_block.close_block = None;
        assert!(build_timeline(&missing_block)
            .unwrap_err()
            .contains("close_block"));
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
            delay_epoch: Some(0xa000_0100_0000_000a),
            settlement_tx_hash: Some(vec![0x44; 32]),
            settlement_block: Some(30),
            settlement_timestamp: Some(3000),
            participants: vec![],
            funding_lock_args: vec![],
        };
        let timeline = build_timeline(&channel).unwrap();
        assert_eq!(timeline.len(), 3);
        assert_eq!(timeline[0].event, "open");
        assert_eq!(timeline[1].event, "forceClose");
        assert_eq!(timeline[2].event, "settlement");

        let mut missing_settlement = channel.clone();
        missing_settlement.settlement_block = None;
        assert!(build_timeline(&missing_settlement)
            .unwrap_err()
            .contains("settlement_block"));
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
            observed_parties: vec![],
            observed_lifetime_ms: None,
            lifetime_anchor_block: None,
            funding_tx_hash: "0x1111".to_string(),
            funding_output_index: 0,
            open_block: 100,
            open_timestamp: "1700000000".to_string(),
            close_tx_hash: None,
            close_block: None,
            close_timestamp: None,
            commitment_tx_hash: None,
            delay_epoch: None,
            delay_epoch_decoded: None,
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
            cooperatively_closed_channels: 20,
            force_closed_waiting_channels: 7,
            force_closed_settled_channels: 5,
            observed_parties: 2,
            observed_pairs: 1,
            first_open_timestamp: Some("1000".into()),
            typical_closed_lifetime_ms: Some("500".into()),
            closed_within_hour: 32,
            tip_block_number: 30,
            tip_chain_time_ms: "3000".into(),
            channels_by_token: Vec::new(),
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
            observed_parties: vec![],
            observed_lifetime_ms: None,
            lifetime_anchor_block: None,
            funding_tx_hash: "0xbb".to_string(),
            funding_output_index: 0,
            open_block: 1,
            open_timestamp: "1000".to_string(),
            close_tx_hash: None,
            close_block: None,
            close_timestamp: None,
            commitment_tx_hash: None,
            delay_epoch: None,
            delay_epoch_decoded: None,
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
