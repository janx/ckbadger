#![allow(clippy::type_complexity)]

use axum::{
    extract::{Path, Query, State},
    routing::get,
    Router,
};
use ckb_types::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

use crate::response::{ok, ApiError, ApiResult, ApiRouteError, CursorPaginatedResponse};
use crate::utils::script_to_address;
use crate::utils::stats_bucket::ChainTime;
use crate::utils::sync_status::deep_fork_info_if_consistent;
use crate::AppState;
use ckb_store_reader::CkbChainReader;
use ckbadger_store::types::{CachedBlockHeader, ReorgEventKind, ReorgEventRecord};
use ckbadger_store::CkbadgerStore;

/// How long after detection a persisted reorg still counts as "recent".
///
/// `/forks/recent` answers "did the chain just reorganize?", so the horizon has
/// to be stated explicitly rather than implied by whatever happens to be the
/// newest row in a history that is never pruned.
const RECENT_REORG_WINDOW_SECS: i64 = 24 * 60 * 60;

const DEFAULT_FORKS_LIMIT: i64 = 20;
const MAX_FORKS_LIMIT: i64 = 100;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReorgEventResponse {
    /// Detection time in milliseconds; the stable id of the event.
    pub id: i64,
    pub detected_at: String,
    /// The later switch that restored this event's abandoned tip, by its D7
    /// address; `None` when nothing reversed it.
    pub reverted_by_event: Option<ForkEventRefResponse>,
    /// The earlier switches this one reversed, by their D7 addresses.
    pub reverses_events: Vec<ForkEventRefResponse>,
    /// The first block this switch dropped (height `forkPointNumber + 1` on the
    /// abandoned branch): the second half of the event's D7 address
    /// `/forks/<forkPointNumber>/<first 8 hex digits>`. `None` exactly when
    /// `firstOrphanStatus` is not `recorded` or `traced`.
    pub first_orphan_hash: Option<String>,
    pub first_orphan_status: &'static str,
    pub fork_point_number: i64,
    pub fork_point_hash: String,
    pub old_tip_number: i64,
    pub old_tip_hash: String,
    pub new_tip_number: i64,
    pub new_tip_hash: String,
    pub superseded_by_tip_hash: Option<String>,
    pub reconcile_interrupted: bool,
    pub depth: i32,
    pub orphaned_blocks_count: i64,
    pub orphaned_txs_count: i64,
    pub event_type: String,
    pub resolved_at: Option<String>,
    pub resolved_by: Option<String>,
    pub resolution_action: Option<String>,
    pub resolution_notes: Option<String>,
    pub preview_status: String,
    pub preview: Option<ForkEventPreviewResponse>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ForkEventPreviewResponse {
    pub old_block_hash: String,
    pub old_miner_lock_hash: Option<String>,
    pub old_miner_address: Option<String>,
    pub competing_miner_lock_hash: Option<String>,
    pub competing_miner_address: Option<String>,
    pub fate: String,
    pub uncle_in_block: Option<i64>,
    pub timestamp_gap_ms: Option<i64>,
    pub noncellbase_transactions: i64,
    pub committed_later_transactions: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OrphanedBlockResponse {
    pub number: i64,
    pub hash: String,
    pub parent_hash: String,
    pub timestamp: String,
    pub transactions_count: i32,
    pub miner_lock_hash: Option<String>,
    /// The miner lock's address, from the cellbase witness (as on the block page).
    pub miner_address: Option<String>,
    /// Fate of this historical old-branch block in the current indexed chain.
    pub fate: String,
    pub uncle_in: Option<UncleInclusionResponse>,
    pub competing_block: Option<CanonicalBranchBlockResponse>,
    /// Historical branch timestamp minus the current canonical block at its height.
    pub timestamp_gap_ms: Option<i64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UncleInclusionResponse {
    pub block_number: i64,
    pub block_hash: String,
    pub distance: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CanonicalBranchBlockResponse {
    pub number: i64,
    pub hash: String,
    pub timestamp: String,
    pub transactions_count: i32,
    pub miner_lock_hash: Option<String>,
    /// The miner lock's address, from the cellbase witness (as on the block page).
    pub miner_address: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CanonicalTxLocationResponse {
    pub block_number: i64,
    pub block_hash: String,
    pub tx_index: i32,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OrphanedTransactionResponse {
    pub hash: String,
    pub block_number: i64,
    pub block_hash: String,
    pub tx_index: i32,
    pub inputs_count: Option<i16>,
    pub outputs_count: Option<i16>,
    pub total_capacity: Option<String>,
    pub fate: String,
    pub committed_at: Option<CanonicalTxLocationResponse>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReorgDetailResponse {
    pub event: ReorgEventResponse,
    /// Historical branch evidence retained by the node, even after local rollback.
    pub orphaned_blocks: Vec<OrphanedBlockResponse>,
    pub orphaned_transactions: Vec<OrphanedTransactionResponse>,
    /// The branch adopted by this switch, traced from its persisted new tip.
    pub adopted_branch: Vec<CanonicalBranchBlockResponse>,
    /// Today's indexed canonical blocks at the affected heights. A later switch
    /// may have reversed the original adoption.
    pub canonical_branch: Vec<CanonicalBranchBlockResponse>,
    pub canonical_tip_number: Option<i64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeepForkStatusResponse {
    pub detected: bool,
    pub detected_at: Option<String>,
    pub db_tip: Option<i64>,
    pub db_tip_hash: Option<String>,
    pub chain_tip: Option<i64>,
    pub chain_tip_hash: Option<String>,
    pub depth: Option<i32>,
    pub fork_point: Option<i64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecentReorgResponse {
    /// True while the newest persisted reorg is inside
    /// `recentWindowSeconds`, or while a deep fork is active.
    pub has_recent_reorg: bool,
    /// The newest persisted reorg event, recent or not.
    pub reorg: Option<ReorgEventResponse>,
    pub recent_window_seconds: i64,
    pub deep_fork: DeepForkStatusResponse,
}

#[derive(Debug, Deserialize)]
pub struct ListForksParams {
    pub limit: Option<i64>,
    pub cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ForkOverviewParams {
    pub days: Option<u16>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ForkDayResponse {
    /// Canonical indexed UTC+8 calendar day, never the raw header timestamp.
    pub date: String,
    pub canonical_blocks: i32,
    pub uncles: i32,
    /// Switches observed by this indexer's node on the same UTC+8 day.
    pub node_switches: i32,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ForkOverviewResponse {
    pub scope: &'static str,
    pub through_date: Option<String>,
    pub days: Vec<ForkDayResponse>,
    pub uncles_in_window: i64,
    pub node_switches_in_window: i64,
    pub node_switches_all_time: i64,
    /// Everything this node's persisted switch history says, over all events.
    pub node_record: NodeSwitchRecordResponse,
}

/// A fork event named by its D7 address.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ForkEventRefResponse {
    pub fork_point_number: i64,
    pub first_orphan_hash: Option<String>,
    pub first_orphan_status: &'static str,
}

/// The node's switch history as a whole (Forks overview, FF-L1 / FF-L3).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeSwitchRecordResponse {
    /// When this indexer's live sync began (the bulk → live handoff): switches
    /// are only observed from then on. `None` while no live sync has begun.
    pub observed_since: Option<String>,
    pub observed_since_block: Option<i64>,
    /// Detection time of the oldest persisted automatic switch.
    pub first_switch_detected_at: Option<String>,
    /// Automatic switches that dropped more than one block.
    pub switches_deeper_than_one_block: i64,
    /// Deep-fork detections (sync paused, nothing rolled back).
    pub deep_detections: i64,
    /// Blocks rolled back by automatic switches (persisted per event).
    pub orphaned_blocks: i64,
    /// Transactions rolled back by automatic switches, each dropped block's
    /// cellbase included (persisted per event).
    pub orphaned_transactions: i64,
    /// `available` when the node store is open; `node_reader_unavailable` otherwise.
    pub evidence_status: &'static str,
    /// What became of every dropped block and transaction, read from the
    /// node's copy of each old branch. `None` without the node reader.
    pub orphan_fates: Option<OrphanFateTotalsResponse>,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OrphanFateTotalsResponse {
    /// Automatic switches whose old branch the node still has.
    pub events_with_evidence: i64,
    /// Automatic switches whose old branch the node no longer has (not counted below).
    pub events_old_branch_unavailable: i64,
    pub blocks: OrphanBlockFateTotalsResponse,
    /// Non-cellbase transactions of the dropped blocks.
    pub user_transactions: OrphanTxFateTotalsResponse,
    pub competing: CompetingBlockTotalsResponse,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OrphanBlockFateTotalsResponse {
    pub total: i64,
    pub included_as_uncle: i64,
    /// Included uncles by distance (inclusion height − orphan height), ascending.
    pub uncle_distances: Vec<UncleDistanceCountResponse>,
    pub canonical_now: i64,
    pub not_included_as_uncle: i64,
    pub uncle_fate_pending: i64,
    pub awaiting_canonical_tip: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UncleDistanceCountResponse {
    pub distance: i64,
    pub count: i64,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OrphanTxFateTotalsResponse {
    pub total: i64,
    pub committed_later: i64,
    pub canonical_now: i64,
    pub committed_on_canonical_branch: i64,
    pub not_committed_as_of_tip: i64,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompetingBlockTotalsResponse {
    /// Dropped blocks with a different canonical block at their height.
    pub compared: i64,
    /// Of those, both miners known and the same lock ("one miner against itself").
    pub same_miner: i64,
    /// Of those, a miner unknown on either side (no cellbase witness).
    pub miner_unknown: i64,
    /// Exact median of |dropped − canonical header time| in ms over `compared`
    /// (the mean of the two middle values when even, as an exact decimal).
    pub gap_median_ms: Option<String>,
}

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/forks", get(list_forks))
        .route("/forks/overview", get(get_forks_overview))
        .route("/forks/recent", get(get_recent_reorg))
        .route("/forks/{fork_point}/{orphan}", get(get_fork_at))
}

fn hex0x(bytes: &[u8]) -> String {
    format!("0x{}", hex::encode(bytes))
}

fn rfc3339_from_unix_seconds(seconds: i64, context: &str) -> Result<String, ApiRouteError> {
    chrono::DateTime::<chrono::Utc>::from_timestamp(seconds, 0)
        .map(|dt| dt.to_rfc3339())
        .ok_or_else(|| {
            ApiError::internal(format!(
                "invalid reorg detected_at timestamp in {}: {}",
                context, seconds
            ))
        })
}

/// The first block a switch dropped: height `fork_point + 1` on the abandoned
/// branch, i.e. the second half of the event's D7 address.
///
/// One rule: walk the recorded old branch from `old_tip_hash` down to
/// `fork_point + 1`. A one-block switch needs no step — its old tip *is* the
/// first orphan, which the event row records. A deeper switch needs one node
/// read per step, so without the node reader it stays unresolved (the event row
/// keeps only the old tip's hash; persisting the first orphan is lane W's).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FirstOrphan {
    Recorded([u8; 32]),
    Traced([u8; 32]),
    NotObserved,
    NeedsNodeReader,
    OldBranchUnavailable,
}

impl FirstOrphan {
    fn hash(&self) -> Option<&[u8; 32]> {
        match self {
            FirstOrphan::Recorded(hash) | FirstOrphan::Traced(hash) => Some(hash),
            _ => None,
        }
    }

    fn status(&self) -> &'static str {
        match self {
            FirstOrphan::Recorded(_) => "recorded",
            FirstOrphan::Traced(_) => "traced",
            FirstOrphan::NotObserved => "not_observed",
            FirstOrphan::NeedsNodeReader => "needs_node_reader",
            FirstOrphan::OldBranchUnavailable => "old_branch_unavailable",
        }
    }
}

fn first_orphan(
    record: &ReorgEventRecord,
    reader: Option<&CkbChainReader>,
) -> anyhow::Result<FirstOrphan> {
    let event = &record.event;
    if event.orphaned_blocks == 0 {
        return Ok(FirstOrphan::NotObserved);
    }
    let span = event.old_tip.checked_sub(event.fork_point).ok_or_else(|| {
        anyhow::anyhow!(
            "fork {}: old tip {} below fork point {}",
            record.key,
            event.old_tip,
            event.fork_point
        )
    })?;
    anyhow::ensure!(
        event.orphaned_blocks == span,
        "fork {} recorded {} orphaned blocks but its old branch spans {} heights (fork point {}, old tip {})",
        record.key,
        event.orphaned_blocks,
        span,
        event.fork_point,
        event.old_tip
    );
    let old_tip: [u8; 32] = event.old_tip_hash.as_slice().try_into().map_err(|_| {
        anyhow::anyhow!(
            "fork {}: old tip hash has {} bytes",
            record.key,
            event.old_tip_hash.len()
        )
    })?;
    if span == 1 {
        return Ok(FirstOrphan::Recorded(old_tip));
    }
    let Some(reader) = reader else {
        return Ok(FirstOrphan::NeedsNodeReader);
    };
    match walk_recorded_branch(
        reader,
        "old",
        event.old_tip,
        &event.old_tip_hash,
        event.fork_point,
        &event.fork_point_hash,
    ) {
        Ok(branch) => {
            let (hash, _) = branch.last().ok_or_else(|| {
                anyhow::anyhow!("fork {}: empty old branch above fork point", record.key)
            })?;
            Ok(FirstOrphan::Traced(*hash))
        }
        Err(e) if is_old_branch_unavailable(&e) => Ok(FirstOrphan::OldBranchUnavailable),
        Err(e) => Err(e.context(format!("tracing first orphan of fork {}", record.key))),
    }
}

/// The whole persisted switch history of one request, with every event's
/// first orphan resolved once, so rows can name each other by D7 address.
struct ForkHistory {
    records: Vec<ReorgEventRecord>,
    first_orphans: HashMap<String, FirstOrphan>,
}

impl ForkHistory {
    fn load(store: &CkbadgerStore, reader: Option<&CkbChainReader>) -> anyhow::Result<Self> {
        let records = store.list_reorg_events(usize::MAX, None)?;
        let mut first_orphans = HashMap::with_capacity(records.len());
        for record in &records {
            first_orphans.insert(record.key.clone(), first_orphan(record, reader)?);
        }
        Ok(Self {
            records,
            first_orphans,
        })
    }

    fn first_orphan_of(&self, key: &str) -> anyhow::Result<FirstOrphan> {
        self.first_orphans
            .get(key)
            .copied()
            .ok_or_else(|| anyhow::anyhow!("fork history has no event {key}"))
    }

    fn reference(&self, key: &str, context: &str) -> anyhow::Result<ForkEventRefResponse> {
        let record = self
            .records
            .iter()
            .find(|record| record.key == key)
            .ok_or_else(|| anyhow::anyhow!("{context} names missing fork event {key}"))?;
        let orphan = self.first_orphan_of(key)?;
        Ok(ForkEventRefResponse {
            fork_point_number: record.event.fork_point,
            first_orphan_hash: orphan.hash().map(|hash| hex0x(hash)),
            first_orphan_status: orphan.status(),
        })
    }
}

/// Render one persisted reorg event. Every field comes from the record the
/// writer captured at detection time, so nothing here is re-derived from chain
/// state that may have moved since.
fn reorg_event_response(
    record: &ReorgEventRecord,
    history: &ForkHistory,
) -> Result<ReorgEventResponse, ApiRouteError> {
    let internal = |e: anyhow::Error| ApiError::internal(format!("{e:#}"));
    let detected_at = rfc3339_from_unix_seconds(record.event.detected_at, &record.key)?;
    let first_orphan = history.first_orphan_of(&record.key).map_err(internal)?;
    Ok(ReorgEventResponse {
        reverted_by_event: record
            .event
            .reverted_by_event
            .as_deref()
            .map(|key| history.reference(key, &format!("fork {} revertedByEvent", record.key)))
            .transpose()
            .map_err(internal)?,
        reverses_events: record
            .event
            .reverses_events
            .iter()
            .map(|key| history.reference(key, &format!("fork {} reversesEvents", record.key)))
            .collect::<Result<_, _>>()
            .map_err(internal)?,
        first_orphan_hash: first_orphan.hash().map(|hash| hex0x(hash)),
        first_orphan_status: first_orphan.status(),
        id: record.detected_at_ms,
        detected_at,
        fork_point_number: record.event.fork_point,
        fork_point_hash: hex0x(&record.event.fork_point_hash),
        old_tip_number: record.event.old_tip,
        old_tip_hash: hex0x(&record.event.old_tip_hash),
        new_tip_number: record.event.new_tip,
        new_tip_hash: hex0x(&record.event.new_tip_hash),
        superseded_by_tip_hash: record.event.superseded_by_tip_hash.as_deref().map(hex0x),
        reconcile_interrupted: record.event.reconcile_interrupted,
        depth: record.event.depth,
        orphaned_blocks_count: record.event.orphaned_blocks,
        orphaned_txs_count: record.event.orphaned_txs,
        event_type: match record.event.kind {
            ReorgEventKind::Automatic => "reorg".to_string(),
            ReorgEventKind::Deep => "deep".to_string(),
        },
        resolved_at: None,
        resolved_by: None,
        resolution_action: None,
        resolution_notes: None,
        preview_status: if record.event.orphaned_blocks == 0 {
            "no_old_branch"
        } else {
            "not_loaded"
        }
        .to_string(),
        preview: None,
    })
}

/// The miner a block's cellbase witness names: its lock hash and address,
/// encoded exactly as the block page encodes its miner (`script_to_address`
/// over the witness lock). `None` only for a block whose cellbase carries no
/// witness.
fn cellbase_miner(
    reader: &CkbChainReader,
    hash: &[u8; 32],
    network: &str,
) -> anyhow::Result<Option<(String, String)>> {
    let Some((lock, _)) = reader.get_cellbase_witness(hash) else {
        return Ok(None);
    };
    let address = script_to_address(
        lock.code_hash().as_slice(),
        i16::from(lock.hash_type().as_slice()[0]),
        &lock.args().raw_data(),
        network,
    )
    .map_err(|e| anyhow::anyhow!("block 0x{}: miner witness lock: {e}", hex::encode(hash)))?;
    Ok(Some((hex0x(lock.calc_script_hash().as_slice()), address)))
}

/// Reject anything that is not a well-formed history key before it reaches the
/// store, so a bad cursor is a 400 and never a silently shifted page.
fn parse_forks_cursor(cursor: Option<&str>) -> Result<Option<Vec<u8>>, ApiRouteError> {
    let Some(raw) = cursor else {
        return Ok(None);
    };
    let bytes = raw.as_bytes().to_vec();
    ckbadger_store::keys::decode_reorg_event_key(&bytes)
        .map_err(|_| ApiError::bad_request("Invalid forks cursor"))?;
    Ok(Some(bytes))
}

/// The chain's uncle record and this node's observed switches share only a
/// calendar axis. They are distinct populations: most uncles were never a tip
/// of this node, while a switch can later be reversed.
async fn get_forks_overview(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ForkOverviewParams>,
) -> ApiResult<ForkOverviewResponse> {
    let days = params.days.unwrap_or(30);
    if !(1..=90).contains(&days) {
        return Err(ApiError::bad_request("days must be between 1 and 90"));
    }
    let store = state.store.clone();
    let reader = state.ckb_store.clone();
    let network = state.ckb_network.clone();
    let response = tokio::task::spawn_blocking(move || -> Result<_, ApiRouteError> {
        let chain = ChainTime::load(&store)?;
        let rows = store.list_daily_block_stats().map_err(|e| ApiError::internal(e.to_string()))?;
        let events = store.list_reorg_events(usize::MAX, None).map_err(|e| ApiError::internal(e.to_string()))?;
        let node_switches_all_time = i64::try_from(events.iter().filter(|record| record.event.kind == ReorgEventKind::Automatic).count())
            .map_err(|_| ApiError::internal("node switch count exceeds i64"))?;
        let node_record = node_switch_record(&store, reader.as_deref(), &network, &events)
            .map_err(|e| ApiError::internal(format!("fork node record: {e:#}")))?;
        let Some(chain) = chain else {
            if !rows.is_empty() || !events.is_empty() {
                return Err(ApiError::internal("fork evidence exists without an indexed canonical chain"));
            }
            return Ok(ForkOverviewResponse {
                scope: "complete UTC+8 days; uncles use indexed chain time, node switches use this indexer's detection wall time",
                through_date: None,
                days: Vec::new(),
                uncles_in_window: 0,
                node_switches_in_window: 0,
                node_switches_all_time: 0,
                node_record,
            });
        };
        let genesis = store.get_block_header(0).map_err(|e| ApiError::internal(e.to_string()))?
            .ok_or_else(|| ApiError::internal("indexed tip exists without genesis header"))?;
        let genesis_day = ckbadger_common::stats_bucket::stats_day(genesis.chain_time_ms)
            .map_err(|e| ApiError::internal(format!("invalid genesis chain day: {e}")))?;
        let last_day = chain.last_complete_day()?;
        if last_day < genesis_day {
            return Ok(ForkOverviewResponse {
                scope: "complete UTC+8 days; uncles use indexed chain time, node switches use this indexer's detection wall time",
                through_date: None,
                days: Vec::new(),
                uncles_in_window: 0,
                node_switches_in_window: 0,
                node_switches_all_time,
                node_record,
            });
        }
        let requested_start = last_day - chrono::Duration::days(i64::from(days) - 1);
        let start_day = requested_start.max(genesis_day);
        let mut by_day = HashMap::new();
        for (key, stats) in rows {
            let date = chrono::NaiveDate::parse_from_str(&key, "%Y%m%d").map_err(|e| ApiError::internal(format!("invalid daily block stats key {key}: {e}")))?;
            if date < start_day || date > last_day { continue; }
            if stats.block_count <= 0 || stats.total_uncles < 0 {
                return Err(ApiError::internal(format!("invalid canonical daily block stats on {date}: blocks={} uncles={}", stats.block_count, stats.total_uncles)));
            }
            if by_day.insert(date, stats).is_some() {
                return Err(ApiError::internal(format!("duplicate daily block stats on {date}")));
            }
        }
        let mut switches_by_day: HashMap<chrono::NaiveDate, i32> = HashMap::new();
        let utc8 = chrono::FixedOffset::east_opt(ckbadger_common::CKB_UTC8_OFFSET)
            .ok_or_else(|| ApiError::internal("invalid CKB UTC+8 offset"))?;
        for record in events.iter().filter(|record| record.event.kind == ReorgEventKind::Automatic) {
            let detected = chrono::DateTime::<chrono::Utc>::from_timestamp_millis(record.detected_at_ms)
                .ok_or_else(|| ApiError::internal(format!("invalid fork event time {}", record.key)))?;
            let date = detected.with_timezone(&utc8).date_naive();
            if date < start_day || date > last_day { continue; }
            let count = switches_by_day.entry(date).or_insert(0);
            *count = count.checked_add(1).ok_or_else(|| ApiError::internal(format!("node switch count overflow on {date}")))?;
        }
        let mut day = start_day;
        let mut data = Vec::new();
        let mut uncles_in_window = 0i64;
        let mut node_switches_in_window = 0i64;
        while day <= last_day {
            let (canonical_blocks, uncles) = match by_day.remove(&day) {
                Some(stats) => (stats.block_count, stats.total_uncles),
                None => {
                    // A missing daily row is valid only if the indexed chain
                    // proves the day contained no canonical block.
                    let preceding = day.pred_opt().ok_or_else(|| ApiError::internal("fork day underflow"))?;
                    let start_ms = ckbadger_common::stats_bucket::utc8_day_end_ms(preceding)
                        .map_err(|e| ApiError::internal(format!("invalid fork day {day}: {e}")))?;
                    if let Some(number) = store.find_first_block_at_or_after_chain_time_ms(start_ms)
                        .map_err(|e| ApiError::internal(format!("fork day {day} coverage: {e}")))? {
                        let header = store.get_block_header(number).map_err(|e| ApiError::internal(e.to_string()))?
                            .ok_or_else(|| ApiError::internal(format!("missing canonical header {number} for fork day {day}")))?;
                        let first_day = ckbadger_common::stats_bucket::stats_day(header.chain_time_ms)
                            .map_err(|e| ApiError::internal(format!("invalid chain day at block {number}: {e}")))?;
                        if first_day == day {
                            return Err(ApiError::internal(format!("missing daily block stats on block-bearing fork day {day}; first block={number}")));
                        }
                    }
                    (0, 0)
                }
            };
            let node_switches = switches_by_day.remove(&day).unwrap_or(0);
            uncles_in_window = uncles_in_window.checked_add(i64::from(uncles)).ok_or_else(|| ApiError::internal("uncle count overflow"))?;
            node_switches_in_window = node_switches_in_window.checked_add(i64::from(node_switches)).ok_or_else(|| ApiError::internal("node switch count overflow"))?;
            data.push(ForkDayResponse { date: day.format("%Y-%m-%d").to_string(), canonical_blocks, uncles, node_switches });
            day = day.succ_opt().ok_or_else(|| ApiError::internal("fork day overflow"))?;
        }
        Ok(ForkOverviewResponse {
            scope: "complete UTC+8 days; uncles use indexed chain time, node switches use this indexer's detection wall time",
            through_date: Some(last_day.format("%Y-%m-%d").to_string()),
            days: data,
            uncles_in_window,
            node_switches_in_window,
            node_switches_all_time,
            node_record,
        })
    }).await.map_err(|e| ApiError::internal(e.to_string()))??;
    ok(response)
}

fn count_i64(value: usize, what: &str) -> anyhow::Result<i64> {
    i64::try_from(value).map_err(|_| anyhow::anyhow!("{what} count exceeds i64"))
}

fn add_one(counter: &mut i64, what: &str) -> anyhow::Result<()> {
    *counter = counter
        .checked_add(1)
        .ok_or_else(|| anyhow::anyhow!("{what} count overflow"))?;
    Ok(())
}

/// The node's switch history as a whole: persisted counts over every event,
/// and — with the node reader — what became of every dropped block and
/// transaction, from the same branch evidence the list and detail read.
fn node_switch_record(
    store: &CkbadgerStore,
    reader: Option<&CkbChainReader>,
    network: &str,
    events: &[ReorgEventRecord],
) -> anyhow::Result<NodeSwitchRecordResponse> {
    let status = store.get_sync_status()?;
    let observed_since = status
        .bulk_sync_completed_at
        .map(|seconds| {
            chrono::DateTime::<chrono::Utc>::from_timestamp(seconds, 0)
                .map(|at| at.to_rfc3339())
                .ok_or_else(|| anyhow::anyhow!("invalid bulk sync completion time {seconds}"))
        })
        .transpose()?;
    let automatic: Vec<&ReorgEventRecord> = events
        .iter()
        .filter(|record| record.event.kind == ReorgEventKind::Automatic)
        .collect();
    let first_switch_detected_at = automatic
        .iter()
        .map(|record| record.detected_at_ms)
        .min()
        .map(|ms| {
            chrono::DateTime::<chrono::Utc>::from_timestamp_millis(ms)
                .map(|at| at.to_rfc3339())
                .ok_or_else(|| anyhow::anyhow!("invalid fork event time {ms}"))
        })
        .transpose()?;
    let mut orphaned_blocks = 0i64;
    let mut orphaned_transactions = 0i64;
    for record in &automatic {
        orphaned_blocks = orphaned_blocks
            .checked_add(record.event.orphaned_blocks)
            .ok_or_else(|| anyhow::anyhow!("orphaned block total overflow at {}", record.key))?;
        orphaned_transactions = orphaned_transactions
            .checked_add(record.event.orphaned_txs)
            .ok_or_else(|| anyhow::anyhow!("orphaned tx total overflow at {}", record.key))?;
    }
    let switches_deeper_than_one_block = count_i64(
        automatic
            .iter()
            .filter(|record| record.event.old_tip - record.event.fork_point > 1)
            .count(),
        "deeper switch",
    )?;
    let deep_detections = count_i64(
        events
            .iter()
            .filter(|record| record.event.kind == ReorgEventKind::Deep)
            .count(),
        "deep detection",
    )?;
    let (evidence_status, orphan_fates) = match reader {
        None => ("node_reader_unavailable", None),
        Some(reader) => {
            let mut totals = OrphanFateTotalsResponse::default();
            let mut distances: std::collections::BTreeMap<i64, i64> = Default::default();
            let mut gaps: Vec<i64> = Vec::new();
            for record in automatic
                .iter()
                .filter(|record| record.event.orphaned_blocks > 0)
            {
                let Some((blocks, transactions)) =
                    read_branch_evidence(store, reader, network, &record.event)
                        .map_err(|e| e.context(format!("fork {}", record.key)))?
                else {
                    add_one(
                        &mut totals.events_old_branch_unavailable,
                        "unavailable event",
                    )?;
                    continue;
                };
                add_one(&mut totals.events_with_evidence, "evidence event")?;
                for block in &blocks {
                    add_one(&mut totals.blocks.total, "orphan block")?;
                    let counter = match block.fate.as_str() {
                        "included_as_uncle" => {
                            let uncle = block.uncle_in.as_ref().ok_or_else(|| {
                                anyhow::anyhow!("uncle fate without inclusion: {}", block.hash)
                            })?;
                            add_one(distances.entry(uncle.distance).or_insert(0), "distance")?;
                            &mut totals.blocks.included_as_uncle
                        }
                        "canonical_now" => &mut totals.blocks.canonical_now,
                        "not_included_as_uncle" => &mut totals.blocks.not_included_as_uncle,
                        "uncle_fate_pending" => &mut totals.blocks.uncle_fate_pending,
                        "awaiting_canonical_tip" => &mut totals.blocks.awaiting_canonical_tip,
                        other => anyhow::bail!("orphan {} has unexpected fate {other}", block.hash),
                    };
                    add_one(counter, "orphan fate")?;
                    if let Some(competing) = &block.competing_block {
                        add_one(&mut totals.competing.compared, "compared")?;
                        match (&block.miner_lock_hash, &competing.miner_lock_hash) {
                            (Some(old), Some(new)) => {
                                if old == new {
                                    add_one(&mut totals.competing.same_miner, "same miner")?;
                                }
                            }
                            _ => add_one(&mut totals.competing.miner_unknown, "miner unknown")?,
                        }
                        let gap = block.timestamp_gap_ms.ok_or_else(|| {
                            anyhow::anyhow!("competing block without gap: {}", block.hash)
                        })?;
                        gaps.push(
                            gap.checked_abs()
                                .ok_or_else(|| anyhow::anyhow!("gap overflow at {}", block.hash))?,
                        );
                    }
                }
                for tx in transactions.iter().filter(|tx| tx.tx_index > 0) {
                    add_one(&mut totals.user_transactions.total, "orphan tx")?;
                    let counter = match tx.fate.as_str() {
                        "committed_later" => &mut totals.user_transactions.committed_later,
                        "canonical_now" => &mut totals.user_transactions.canonical_now,
                        "committed_on_canonical_branch" => {
                            &mut totals.user_transactions.committed_on_canonical_branch
                        }
                        "not_committed_as_of_tip" => {
                            &mut totals.user_transactions.not_committed_as_of_tip
                        }
                        other => anyhow::bail!("orphan tx {} has unexpected fate {other}", tx.hash),
                    };
                    add_one(counter, "orphan tx fate")?;
                }
            }
            totals.blocks.uncle_distances = distances
                .into_iter()
                .map(|(distance, count)| UncleDistanceCountResponse { distance, count })
                .collect();
            gaps.sort_unstable();
            totals.competing.gap_median_ms = match gaps.len() {
                0 => None,
                n if n % 2 == 1 => Some(gaps[n / 2].to_string()),
                n => {
                    let sum = i128::from(gaps[n / 2 - 1]) + i128::from(gaps[n / 2]);
                    Some(if sum % 2 == 0 {
                        (sum / 2).to_string()
                    } else {
                        format!("{}.5", sum / 2)
                    })
                }
            };
            ("available", Some(totals))
        }
    };
    Ok(NodeSwitchRecordResponse {
        observed_since,
        observed_since_block: status.bulk_sync_completed_block,
        first_switch_detected_at,
        switches_deeper_than_one_block,
        deep_detections,
        orphaned_blocks,
        orphaned_transactions,
        evidence_status,
        orphan_fates,
    })
}

/// List the persisted reorg-event history, newest first.
///
/// Both automatic reorgs and deep-fork detections are persisted by the indexer,
/// so both appear here, tagged by `eventType`.
async fn list_forks(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListForksParams>,
) -> ApiResult<CursorPaginatedResponse<ReorgEventResponse>> {
    let limit = params
        .limit
        .unwrap_or(DEFAULT_FORKS_LIMIT)
        .clamp(1, MAX_FORKS_LIMIT);
    let cursor = parse_forks_cursor(params.cursor.as_deref())?;

    let store = state.store.clone();
    let reader = state.ckb_store.clone();
    let network = state.ckb_network.clone();
    let page_size = limit as usize;
    let (events, total, next_cursor) =
        tokio::task::spawn_blocking(move || -> Result<_, ApiRouteError> {
            let internal = |e: anyhow::Error| ApiError::internal(format!("fork list: {e:#}"));
            let mut records = store
                .list_reorg_events(page_size + 1, cursor.as_deref())
                .map_err(internal)?;
            let total = store.count_reorg_events().map_err(internal)?;
            let history = ForkHistory::load(&store, reader.as_deref()).map_err(internal)?;
            let has_more = records.len() > page_size;
            if has_more {
                records.truncate(page_size);
            }
            let next_cursor = if has_more {
                records.last().map(|record| record.key.clone())
            } else {
                None
            };
            let mut events = Vec::with_capacity(records.len());
            for record in &records {
                let mut event = reorg_event_response(record, &history)?;
                match reader.as_deref() {
                    Some(reader) => {
                        let (status, preview) =
                            event_preview(&store, reader, &network, record, &history)
                                .map_err(internal)?;
                        event.preview_status = status.to_string();
                        event.preview = preview;
                    }
                    None => {
                        event.preview_status = if record.event.orphaned_blocks == 0 {
                            "no_old_branch"
                        } else {
                            "node_reader_unavailable"
                        }
                        .to_string();
                    }
                }
                events.push(event);
            }
            Ok((events, total, next_cursor))
        })
        .await
        .map_err(|e| ApiError::internal(e.to_string()))??;

    ok(CursorPaginatedResponse::new(
        events,
        total,
        limit,
        next_cursor,
    ))
}

/// One event's dropped branch with every block's and transaction's fate, read
/// from the node's copy of it; `None` when the node no longer has the branch.
fn read_branch_evidence(
    store: &CkbadgerStore,
    reader: &CkbChainReader,
    network: &str,
    event: &ckbadger_store::ReorgEvent,
) -> anyhow::Result<Option<(Vec<OrphanedBlockResponse>, Vec<OrphanedTransactionResponse>)>> {
    let (mut blocks, mut transactions) = match read_orphaned_branch(reader, network, event) {
        Ok(branch) => branch,
        Err(e) if is_old_branch_unavailable(&e) => return Ok(None),
        Err(e) => return Err(e),
    };
    match enrich_branch_fate(
        store,
        reader,
        network,
        event,
        &mut blocks,
        &mut transactions,
    ) {
        Ok(_) => Ok(Some((blocks, transactions))),
        Err(e) if is_old_branch_unavailable(&e) => Ok(None),
        Err(e) => Err(e),
    }
}

/// The list row's preview: the first dropped block and its competitor.
fn event_preview(
    store: &CkbadgerStore,
    reader: &CkbChainReader,
    network: &str,
    record: &ReorgEventRecord,
    history: &ForkHistory,
) -> anyhow::Result<(&'static str, Option<ForkEventPreviewResponse>)> {
    if record.event.orphaned_blocks == 0 {
        return Ok(("no_old_branch", None));
    }
    let Some((blocks, transactions)) = read_branch_evidence(store, reader, network, &record.event)?
    else {
        return Ok(("old_branch_unavailable", None));
    };
    let first = blocks.first().ok_or_else(|| {
        anyhow::anyhow!(
            "fork {} recorded old blocks but node branch is empty",
            record.key
        )
    })?;
    ensure_first_orphan_matches(record, history, &first.hash)?;
    let noncellbase_transactions = i64::try_from(
        transactions
            .iter()
            .filter(|tx| tx.block_hash == first.hash && tx.tx_index > 0)
            .count(),
    )?;
    let committed_later_transactions = i64::try_from(
        transactions
            .iter()
            .filter(|tx| tx.block_hash == first.hash && tx.fate == "committed_later")
            .count(),
    )?;
    Ok((
        "available",
        Some(ForkEventPreviewResponse {
            old_block_hash: first.hash.clone(),
            old_miner_lock_hash: first.miner_lock_hash.clone(),
            old_miner_address: first.miner_address.clone(),
            competing_miner_lock_hash: first
                .competing_block
                .as_ref()
                .and_then(|block| block.miner_lock_hash.clone()),
            competing_miner_address: first
                .competing_block
                .as_ref()
                .and_then(|block| block.miner_address.clone()),
            fate: first.fate.clone(),
            uncle_in_block: first.uncle_in.as_ref().map(|uncle| uncle.block_number),
            timestamp_gap_ms: first.timestamp_gap_ms,
            noncellbase_transactions,
            committed_later_transactions,
        }),
    ))
}

/// The node's branch must start at the block the event's D7 address names.
fn ensure_first_orphan_matches(
    record: &ReorgEventRecord,
    history: &ForkHistory,
    branch_first_hash: &str,
) -> anyhow::Result<()> {
    let orphan = history.first_orphan_of(&record.key)?;
    let expected = orphan.hash().ok_or_else(|| {
        anyhow::anyhow!(
            "fork {}: node branch starts at {branch_first_hash} but the first orphan is {}",
            record.key,
            orphan.status()
        )
    })?;
    anyhow::ensure!(
        hex0x(expected) == branch_first_hash,
        "fork {}: node branch starts at {branch_first_hash}, the event's first orphan is {}",
        record.key,
        hex0x(expected)
    );
    Ok(())
}

/// A D7 path segment: the fork point height in canonical decimal.
fn parse_fork_point_segment(raw: &str) -> Result<i64, ApiRouteError> {
    let canonical = raw == "0"
        || (!raw.is_empty() && !raw.starts_with('0') && raw.bytes().all(|b| b.is_ascii_digit()));
    if !canonical {
        return Err(ApiError::bad_request(format!(
            "Invalid fork point height: {raw}"
        )));
    }
    raw.parse::<i64>()
        .map_err(|_| ApiError::bad_request(format!("Invalid fork point height: {raw}")))
}

/// A D7 path segment: the first 8 lowercase hex digits of the first orphan.
fn parse_orphan_prefix_segment(raw: &str) -> Result<String, ApiRouteError> {
    if raw.len() != 8 || !raw.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
        return Err(ApiError::bad_request(format!(
            "Invalid orphan hash prefix (8 lowercase hex digits): {raw}"
        )));
    }
    Ok(raw.to_string())
}

/// The one event a D7 address names. Exactly one event may match; a second
/// match, or an event at the same fork point whose first orphan cannot be
/// resolved, means the address cannot be proved unique and is an error.
fn resolve_fork_address<'a>(
    history: &'a ForkHistory,
    height: i64,
    prefix: &str,
) -> Result<&'a ReorgEventRecord, ApiRouteError> {
    let mut matches = Vec::new();
    for record in history
        .records
        .iter()
        .filter(|record| record.event.fork_point == height)
    {
        let orphan = history
            .first_orphan_of(&record.key)
            .map_err(|e| ApiError::internal(format!("{e:#}")))?;
        match orphan {
            FirstOrphan::Recorded(hash) | FirstOrphan::Traced(hash) => {
                if hex::encode(hash).starts_with(prefix) {
                    matches.push(record);
                }
            }
            FirstOrphan::NotObserved => {}
            FirstOrphan::NeedsNodeReader | FirstOrphan::OldBranchUnavailable => {
                return Err(ApiError::internal(format!(
                    "/forks/{height}/{prefix}: event {} at this fork point has an unresolved first orphan ({}); the address cannot be proved to name one event",
                    record.key,
                    orphan.status()
                )));
            }
        }
    }
    match matches.as_slice() {
        [] => Err(ApiError::not_found(format!(
            "No fork event at #{height} dropped a block starting 0x{prefix}"
        ))),
        [one] => Ok(one),
        many => Err(ApiError::internal(format!(
            "/forks/{height}/{prefix} names {} events (one fork event per fork point and first orphan is an invariant): {}",
            many.len(),
            many.iter()
                .map(|record| record.key.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ))),
    }
}

/// D7: one fork event by its fork point and the first 8 hex digits of the
/// first block it dropped (`/forks/<fork_point>/<orphan_hash_prefix8>`).
async fn get_fork_at(
    State(state): State<Arc<AppState>>,
    Path((fork_point, orphan)): Path<(String, String)>,
) -> ApiResult<ReorgDetailResponse> {
    let height = parse_fork_point_segment(&fork_point)?;
    let prefix = parse_orphan_prefix_segment(&orphan)?;
    let store = state.store.clone();
    let reader = state.ckb_store.clone();
    let network = state.ckb_network.clone();
    let response = tokio::task::spawn_blocking(move || -> Result<_, ApiRouteError> {
        let internal =
            |e: anyhow::Error| ApiError::internal(format!("fork /forks/{height}/{prefix}: {e:#}"));
        let history = ForkHistory::load(&store, reader.as_deref()).map_err(internal)?;
        let record = resolve_fork_address(&history, height, &prefix)?;
        let event = reorg_event_response(record, &history)?;
        let reader = reader.ok_or_else(|| {
            ApiError::internal(format!(
                "fork {} evidence unavailable: CKB RocksDB reader is not open",
                record.key
            ))
        })?;
        let (mut blocks, mut transactions) =
            read_orphaned_branch(&reader, &network, &record.event).map_err(internal)?;
        let adopted_branch =
            read_adopted_branch(&reader, &network, &record.event).map_err(internal)?;
        let (canonical_branch, canonical_tip_number) = enrich_branch_fate(
            &store,
            &reader,
            &network,
            &record.event,
            &mut blocks,
            &mut transactions,
        )
        .map_err(internal)?;
        let first = blocks
            .first()
            .ok_or_else(|| internal(anyhow::anyhow!("node branch is empty")))?;
        ensure_first_orphan_matches(record, &history, &first.hash).map_err(internal)?;
        Ok(ReorgDetailResponse {
            event,
            orphaned_blocks: blocks,
            orphaned_transactions: transactions,
            adopted_branch,
            canonical_branch,
            canonical_tip_number,
        })
    })
    .await
    .map_err(|e| ApiError::internal(e.to_string()))??;
    ok(response)
}

fn read_adopted_branch(
    reader: &CkbChainReader,
    network: &str,
    event: &ckbadger_store::ReorgEvent,
) -> anyhow::Result<Vec<CanonicalBranchBlockResponse>> {
    let mut branch = Vec::new();
    for (hash, block) in walk_recorded_branch(
        reader,
        "adopted",
        event.new_tip,
        &event.new_tip_hash,
        event.fork_point,
        &event.fork_point_hash,
    )? {
        let expected_number = i64::try_from(block.number())?;
        let timestamp = chrono::DateTime::from_timestamp_millis(i64::try_from(block.timestamp())?)
            .ok_or_else(|| anyhow::anyhow!("invalid adopted timestamp at block {expected_number}"))?
            .to_rfc3339();
        let (miner_lock_hash, miner_address) = cellbase_miner(reader, &hash, network)?.unzip();
        branch.push(CanonicalBranchBlockResponse {
            number: expected_number,
            hash: hex0x(&hash),
            timestamp,
            transactions_count: i32::try_from(block.transactions().len())?,
            miner_lock_hash,
            miner_address,
        });
    }
    branch.reverse();
    Ok(branch)
}

/// The single strict node-store traversal for both persisted event branches.
/// It proves each parent edge and the recorded fork-point hash, independent of
/// what the domain store currently considers canonical.
fn walk_recorded_branch(
    reader: &ckb_store_reader::CkbChainReader,
    label: &str,
    tip_number: i64,
    tip_hash: &[u8],
    fork_point: i64,
    fork_point_hash: &[u8],
) -> anyhow::Result<Vec<([u8; 32], ckb_types::core::BlockView)>> {
    anyhow::ensure!(
        tip_number > fork_point,
        "{label} branch tip {tip_number} does not exceed fork point {fork_point}"
    );
    let mut hash: [u8; 32] = tip_hash
        .try_into()
        .map_err(|_| anyhow::anyhow!("invalid {label} tip hash at block {tip_number}"))?;
    let mut branch = Vec::new();
    for expected_number in ((fork_point + 1)..=tip_number).rev() {
        let block = reader.get_block(&hash).ok_or_else(|| {
            anyhow::anyhow!(
                "CKB RocksDB is missing {label} block {expected_number} 0x{}",
                hex::encode(hash)
            )
        })?;
        anyhow::ensure!(
            block.number() == u64::try_from(expected_number)?,
            "{label} block height mismatch at 0x{}: expected={expected_number} actual={}",
            hex::encode(hash),
            block.number()
        );
        let parent: [u8; 32] = block.parent_hash().as_slice().try_into().map_err(|_| {
            anyhow::anyhow!("invalid {label} parent hash at block {expected_number}")
        })?;
        branch.push((hash, block));
        hash = parent;
    }
    anyhow::ensure!(
        hash.as_slice() == fork_point_hash,
        "{label} branch does not reach recorded fork point {fork_point}"
    );
    Ok(branch)
}

fn read_orphaned_branch(
    reader: &CkbChainReader,
    network: &str,
    event: &ckbadger_store::ReorgEvent,
) -> anyhow::Result<(Vec<OrphanedBlockResponse>, Vec<OrphanedTransactionResponse>)> {
    let mut blocks = Vec::new();
    let mut transactions = Vec::new();
    for (hash, block) in walk_recorded_branch(
        reader,
        "old",
        event.old_tip,
        &event.old_tip_hash,
        event.fork_point,
        &event.fork_point_hash,
    )? {
        let expected_number = i64::try_from(block.number())?;
        let timestamp = chrono::DateTime::from_timestamp_millis(i64::try_from(block.timestamp())?)
            .ok_or_else(|| {
                anyhow::anyhow!("invalid historical block timestamp at {}", expected_number)
            })?
            .to_rfc3339();
        let (miner_lock_hash, miner_address) = cellbase_miner(reader, &hash, network)?.unzip();
        blocks.push(OrphanedBlockResponse {
            number: expected_number,
            hash: hex0x(&hash),
            parent_hash: hex0x(block.parent_hash().as_slice()),
            timestamp,
            transactions_count: i32::try_from(block.transactions().len())?,
            miner_lock_hash,
            miner_address,
            fate: "canonical_evidence_unavailable".to_string(),
            uncle_in: None,
            competing_block: None,
            timestamp_gap_ms: None,
        });
        for (index, tx) in block.transactions().iter().enumerate() {
            let total_capacity = tx.outputs().into_iter().try_fold(0u128, |total, output| {
                let capacity: u64 = output.capacity().unpack();
                total
                    .checked_add(u128::from(capacity))
                    .ok_or_else(|| anyhow::anyhow!("orphan capacity overflow"))
            })?;
            transactions.push(OrphanedTransactionResponse {
                hash: hex0x(tx.hash().as_slice()),
                block_number: expected_number,
                block_hash: hex0x(&hash),
                tx_index: i32::try_from(index)?,
                inputs_count: Some(i16::try_from(tx.inputs().len())?),
                outputs_count: Some(i16::try_from(tx.outputs().len())?),
                total_capacity: Some(total_capacity.to_string()),
                fate: "canonical_evidence_unavailable".to_string(),
                committed_at: None,
            });
        }
    }
    anyhow::ensure!(
        blocks.len() as i64 == event.orphaned_blocks,
        "historical branch block count mismatch: recorded={} actual={}",
        event.orphaned_blocks,
        blocks.len()
    );
    blocks.reverse();
    transactions.sort_by_key(|tx| (tx.block_number, tx.tx_index));
    Ok((blocks, transactions))
}

/// A canonical block of the fork view. Its miner lock hash is the indexed
/// header's; the address comes from the node's cellbase witness, which must
/// name the same lock.
fn canonical_block_response(
    reader: &CkbChainReader,
    network: &str,
    number: i64,
    header: &CachedBlockHeader,
) -> anyhow::Result<CanonicalBranchBlockResponse> {
    let timestamp = chrono::DateTime::from_timestamp_millis(header.timestamp)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "invalid canonical timestamp at block {number}: {}",
                header.timestamp
            )
        })?
        .to_rfc3339();
    anyhow::ensure!(
        header.transactions_count >= 0,
        "negative tx count at canonical block {number}"
    );
    let miner_lock_hash = header.miner_lock_hash.as_ref().map(|hash| hex0x(hash));
    let miner_address = match &miner_lock_hash {
        None => None,
        Some(indexed) => {
            let hash: [u8; 32] = header
                .hash
                .as_slice()
                .try_into()
                .map_err(|_| anyhow::anyhow!("invalid canonical hash at block {number}"))?;
            let (witness_lock, address) = cellbase_miner(reader, &hash, network)?.ok_or_else(|| {
                anyhow::anyhow!(
                    "canonical block {number} indexes miner {indexed} but the node has no cellbase witness"
                )
            })?;
            anyhow::ensure!(
                &witness_lock == indexed,
                "canonical block {number}: indexed miner {indexed}, node cellbase witness names {witness_lock}"
            );
            Some(address)
        }
    };
    Ok(CanonicalBranchBlockResponse {
        number,
        hash: hex0x(&header.hash),
        timestamp,
        transactions_count: header.transactions_count,
        miner_lock_hash,
        miner_address,
    })
}

fn cached_canonical_header(
    store: &CkbadgerStore,
    cache: &mut HashMap<i64, CachedBlockHeader>,
    number: i64,
    context: &str,
) -> anyhow::Result<CachedBlockHeader> {
    if let Some(header) = cache.get(&number) {
        return Ok(header.clone());
    }
    let header = store
        .get_block_header(number)?
        .ok_or_else(|| anyhow::anyhow!("missing canonical block {number} while {context}"))?;
    cache.insert(number, header.clone());
    Ok(header)
}

/// Node RocksDB may lack pruned historical old-branch blocks. Both the
/// walk-back miss (`CKB RocksDB is missing`) and the enrich miss
/// (`lost historical old branch`) map to the same client status.
fn is_old_branch_unavailable(err: &anyhow::Error) -> bool {
    err.chain().any(|cause| {
        let msg = cause.to_string();
        msg.contains("CKB RocksDB is missing") || msg.contains("lost historical old branch")
    })
}

/// Read one current canonical view for the whole historical branch. Re-entry
/// after a later switch is identified by the exact hash at the same height;
/// the persisted event still records what this node dropped at that moment.
fn enrich_branch_fate(
    store: &CkbadgerStore,
    reader: &CkbChainReader,
    network: &str,
    event: &ckbadger_store::ReorgEvent,
    blocks: &mut [OrphanedBlockResponse],
    transactions: &mut [OrphanedTransactionResponse],
) -> anyhow::Result<(Vec<CanonicalBranchBlockResponse>, Option<i64>)> {
    let Some((tip, _)) = store.get_sync_tip_block()? else {
        // A retained node block can be inspected before this API's index has
        // a canonical view. The unavailable state is explicit on every row.
        return Ok((Vec::new(), None));
    };
    let mut canonical_branch = Vec::new();
    let mut headers = HashMap::new();
    let branch_end = event.old_tip.max(event.new_tip).min(tip);
    for number in (event.fork_point + 1)..=branch_end {
        let header = cached_canonical_header(
            store,
            &mut headers,
            number,
            &format!("reading fork {} branch view at tip {tip}", event.fork_point),
        )?;
        canonical_branch.push(canonical_block_response(reader, network, number, &header)?);
    }
    let mut candidates = Vec::new();
    for (index, block) in blocks.iter_mut().enumerate() {
        if block.number > tip {
            block.fate = "awaiting_canonical_tip".to_string();
            continue;
        }
        let current = cached_canonical_header(
            store,
            &mut headers,
            block.number,
            &format!("resolving old branch block {}", block.hash),
        )?;
        if hex0x(&current.hash) == block.hash {
            block.fate = "canonical_now".to_string();
            continue;
        }
        block.timestamp_gap_ms = Some(
            chrono::DateTime::parse_from_rfc3339(&block.timestamp)?
                .timestamp_millis()
                .checked_sub(current.timestamp)
                .ok_or_else(|| {
                    anyhow::anyhow!("timestamp gap overflow for old block {}", block.hash)
                })?,
        );
        block.competing_block = Some(canonical_block_response(
            reader,
            network,
            block.number,
            &current,
        )?);
        let hash_bytes = hex::decode(
            block
                .hash
                .strip_prefix("0x")
                .ok_or_else(|| anyhow::anyhow!("old block lacks 0x prefix: {}", block.hash))?,
        )?;
        let hash: [u8; 32] = hash_bytes
            .as_slice()
            .try_into()
            .map_err(|_| anyhow::anyhow!("old block has invalid hash length: {}", block.hash))?;
        let historical = reader.get_block(&hash).ok_or_else(|| {
            anyhow::anyhow!(
                "CKB RocksDB lost historical old branch block {}",
                block.hash
            )
        })?;
        let old_epoch = historical.epoch().number();
        candidates.push((index, hash, old_epoch, block.number));
    }
    if !candidates.is_empty() {
        let tip_header =
            cached_canonical_header(store, &mut headers, tip, "reading uncle fate tip")?;
        anyhow::ensure!(
            tip_header.epoch_number >= 0,
            "negative canonical epoch at uncle fate tip {tip}"
        );
        let max_epoch = candidates
            .iter()
            .map(|(_, _, epoch, _)| *epoch)
            .max()
            .ok_or_else(|| anyhow::anyhow!("missing uncle candidate epoch"))?;
        let first_scan = candidates
            .iter()
            .map(|(_, _, _, height)| *height)
            .min()
            .ok_or_else(|| anyhow::anyhow!("missing uncle candidate height"))?
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("uncle scan height overflow"))?;
        let mut by_hash = HashMap::new();
        for &(index, hash, epoch, height) in &candidates {
            anyhow::ensure!(
                by_hash.insert(hash, (index, epoch, height)).is_none(),
                "duplicate historical old block hash 0x{}",
                hex::encode(hash)
            );
        }
        let mut unresolved = candidates.len();
        // One canonical pass per event, even if several old blocks share an
        // epoch. Only uncle-bearing blocks require a node-store block read.
        for number in first_scan..=tip {
            let header =
                cached_canonical_header(store, &mut headers, number, "scanning uncle fate")?;
            anyhow::ensure!(
                header.epoch_number >= 0 && header.uncles_count >= 0,
                "invalid canonical epoch or uncle count at block {number}"
            );
            if header.epoch_number > i64::try_from(max_epoch)? {
                break;
            }
            if header.uncles_count == 0 {
                continue;
            }
            let canonical_hash: [u8; 32] = header
                .hash
                .as_slice()
                .try_into()
                .map_err(|_| anyhow::anyhow!("invalid canonical hash at block {number}"))?;
            let canonical = reader.get_block(&canonical_hash).ok_or_else(|| {
                anyhow::anyhow!(
                    "CKB RocksDB missing canonical uncle-bearing block {number} 0x{}",
                    hex::encode(canonical_hash)
                )
            })?;
            anyhow::ensure!(
                canonical.data().uncles().len() == header.uncles_count as usize,
                "uncle count mismatch at canonical block {number}: indexed={} node={}",
                header.uncles_count,
                canonical.data().uncles().len()
            );
            for uncle in canonical.uncles() {
                let uncle_hash: [u8; 32] = uncle.hash().as_slice().try_into().map_err(|_| {
                    anyhow::anyhow!("invalid uncle hash at canonical block {number}")
                })?;
                let Some(&(index, old_epoch, old_height)) = by_hash.get(&uncle_hash) else {
                    continue;
                };
                anyhow::ensure!(number > old_height && header.epoch_number == i64::try_from(old_epoch)?,
                    "invalid inclusion of old block 0x{} at canonical block {number}: old_height={old_height} old_epoch={old_epoch} canonical_epoch={}",
                    hex::encode(uncle_hash), header.epoch_number);
                anyhow::ensure!(
                    blocks[index].uncle_in.is_none(),
                    "old block 0x{} included as uncle more than once",
                    hex::encode(uncle_hash)
                );
                blocks[index].uncle_in = Some(UncleInclusionResponse {
                    block_number: number,
                    block_hash: hex0x(&header.hash),
                    distance: number.checked_sub(old_height).ok_or_else(|| {
                        anyhow::anyhow!(
                            "uncle distance underflow for 0x{}",
                            hex::encode(uncle_hash)
                        )
                    })?,
                });
                unresolved -= 1;
            }
            if unresolved == 0 {
                break;
            }
        }
        for (index, _, old_epoch, _) in candidates {
            blocks[index].fate = if blocks[index].uncle_in.is_some() {
                "included_as_uncle"
            } else if tip_header.epoch_number > i64::try_from(old_epoch)? {
                "not_included_as_uncle"
            } else {
                "uncle_fate_pending"
            }
            .to_string();
        }
    }
    for tx in transactions.iter_mut() {
        if tx.tx_index == 0 {
            tx.fate = "cellbase".to_string();
            continue;
        }
        let tx_hash = hex::decode(
            tx.hash
                .strip_prefix("0x")
                .ok_or_else(|| anyhow::anyhow!("old tx lacks 0x prefix: {}", tx.hash))?,
        )?;
        match store.get_tx_by_hash(&tx_hash)? {
            Some((number, index, _)) => {
                let header = store.get_block_header(number)?.ok_or_else(|| {
                    anyhow::anyhow!("canonical tx {} points to missing block {number}", tx.hash)
                })?;
                tx.committed_at = Some(CanonicalTxLocationResponse {
                    block_number: number,
                    block_hash: hex0x(&header.hash),
                    tx_index: index,
                });
                tx.fate = if number == tx.block_number
                    && header.hash == hex::decode(&tx.block_hash[2..])?
                {
                    "canonical_now"
                } else if number >= tx.block_number {
                    "committed_later"
                } else {
                    "committed_on_canonical_branch"
                }
                .to_string();
            }
            None => tx.fate = "not_committed_as_of_tip".to_string(),
        }
    }
    Ok((canonical_branch, Some(tip)))
}

async fn get_recent_reorg(State(state): State<Arc<AppState>>) -> ApiResult<RecentReorgResponse> {
    let store = state.store.clone();
    let reader = state.ckb_store.clone();
    let (sync_status, latest, reorg) =
        tokio::task::spawn_blocking(move || -> Result<_, ApiRouteError> {
            let internal = |e: anyhow::Error| ApiError::internal(format!("{e:#}"));
            let sync_status = store.get_sync_status().map_err(internal)?;
            let latest = store.get_latest_reorg_event().map_err(internal)?;
            let reorg = match &latest {
                Some(record) => {
                    let history = ForkHistory::load(&store, reader.as_deref()).map_err(internal)?;
                    Some(reorg_event_response(record, &history)?)
                }
                None => None,
            };
            Ok((sync_status, latest, reorg))
        })
        .await
        .map_err(|e| ApiError::internal(e.to_string()))??;

    let deep_fork_info = deep_fork_info_if_consistent(&sync_status)
        .map_err(|error| ApiError::internal(error.to_string()))?;

    let deep_fork = match deep_fork_info {
        Some(info) => {
            // A deep fork is only ever flagged together with its persisted
            // event, so a missing or mismatched record is an invariant
            // violation rather than a reason to report an unknown time.
            let record = latest.as_ref().ok_or_else(|| {
                ApiError::internal(
                    "sync_status reports an active deep fork but no reorg event is persisted; \
                     check the sync_meta reorg history",
                )
            })?;
            if record.event.kind != ReorgEventKind::Deep {
                return Err(ApiError::internal(format!(
                    "sync_status reports an active deep fork but the newest persisted reorg event {} is not a deep-fork record",
                    record.key
                )));
            }
            DeepForkStatusResponse {
                detected: true,
                detected_at: Some(rfc3339_from_unix_seconds(
                    record.event.detected_at,
                    &record.key,
                )?),
                db_tip: Some(info.db_tip),
                db_tip_hash: Some(hex0x(&info.db_tip_hash)),
                chain_tip: Some(info.chain_tip),
                chain_tip_hash: Some(hex0x(&info.chain_tip_hash)),
                depth: Some(info.depth),
                fork_point: Some(info.fork_point),
            }
        }
        None => DeepForkStatusResponse {
            detected: false,
            detected_at: None,
            db_tip: None,
            db_tip_hash: None,
            chain_tip: None,
            chain_tip_hash: None,
            depth: None,
            fork_point: None,
        },
    };

    let now = chrono::Utc::now().timestamp();
    let within_window = latest
        .as_ref()
        .is_some_and(|record| now - record.event.detected_at <= RECENT_REORG_WINDOW_SECS);

    ok(RecentReorgResponse {
        has_recent_reorg: deep_fork.detected || within_window,
        reorg,
        recent_window_seconds: RECENT_REORG_WINDOW_SECS,
        deep_fork,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ckbadger_store::types::ReorgEvent;

    fn record(detected_at_ms: i64, kind: ReorgEventKind) -> ReorgEventRecord {
        ReorgEventRecord {
            key: format!("reorg:{:013}:{}", detected_at_ms, "ab".repeat(16)),
            detected_at_ms,
            event: ReorgEvent {
                detected_at: detected_at_ms / 1000,
                kind,
                fork_point: 17_100,
                fork_point_hash: vec![0x01; 32],
                old_tip: 17_103,
                old_tip_hash: vec![0x02; 32],
                new_tip: 17_104,
                new_tip_hash: vec![0x03; 32],
                depth: 3,
                orphaned_blocks: 3,
                orphaned_txs: 7,
                reverted_by_event: None,
                superseded_by_tip_hash: None,
                reconcile_interrupted: false,
                reverses_events: Vec::new(),
            },
        }
    }

    fn history(records: &[ReorgEventRecord]) -> ForkHistory {
        ForkHistory {
            records: records.to_vec(),
            first_orphans: records
                .iter()
                .map(|record| (record.key.clone(), first_orphan(record, None).unwrap()))
                .collect(),
        }
    }

    fn respond(record: &ReorgEventRecord) -> Result<ReorgEventResponse, ApiRouteError> {
        reorg_event_response(record, &history(std::slice::from_ref(record)))
    }

    #[test]
    fn event_links_name_the_other_events_by_d7_address() {
        let mut event = record(1_700_000_000_000, ReorgEventKind::Automatic);
        let mut later = record(1_700_000_000_123, ReorgEventKind::Automatic);
        later.key = format!("reorg:1700000000123:{}", "ab".repeat(16));
        later.event.old_tip = later.event.fork_point + 1;
        later.event.orphaned_blocks = 1;
        event.event.reverted_by_event = Some(later.key.clone());
        event.event.reverses_events = vec![later.key.clone()];
        let both = history(&[event.clone(), later.clone()]);
        let json = serde_json::to_value(reorg_event_response(&event, &both).unwrap()).unwrap();
        let reference = serde_json::json!({
            "forkPointNumber": 17_100,
            "firstOrphanHash": format!("0x{}", "02".repeat(32)),
            "firstOrphanStatus": "recorded",
        });
        assert_eq!(json["revertedByEvent"], reference);
        assert_eq!(json["reversesEvents"], serde_json::json!([reference]));
        // The three-block event itself needs the node to trace its first orphan.
        assert_eq!(json["firstOrphanStatus"], "needs_node_reader");
        assert_eq!(json["firstOrphanHash"], serde_json::Value::Null);
        event.event.reverted_by_event = Some("reorg:missing".into());
        assert!(reorg_event_response(&event, &both).is_err());
    }

    #[test]
    fn first_orphan_rejects_a_branch_span_the_event_did_not_roll_back() {
        let mut event = record(1_700_000_000_000, ReorgEventKind::Automatic);
        event.event.orphaned_blocks = 2;
        let err = first_orphan(&event, None).unwrap_err().to_string();
        assert!(
            err.contains("recorded 2 orphaned blocks but its old branch spans 3"),
            "{err}"
        );
    }

    #[test]
    fn d7_segments_accept_only_canonical_height_and_lowercase_prefix() {
        assert_eq!(parse_fork_point_segment("0").unwrap(), 0);
        assert_eq!(parse_fork_point_segment("20617704").unwrap(), 20_617_704);
        for bad in ["", "-1", "007", "1e3", "99999999999999999999"] {
            assert!(parse_fork_point_segment(bad).is_err(), "{bad}");
        }
        assert_eq!(parse_orphan_prefix_segment("1a2b3c4d").unwrap(), "1a2b3c4d");
        for bad in ["1A2B3C4D", "1a2b3c4", "1a2b3c4d5", "0x1a2b3c", "1a2b3c4g"] {
            assert!(parse_orphan_prefix_segment(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn superseded_tip_hash_is_reported_from_persisted_event() {
        let mut event = record(1_700_000_000_000, ReorgEventKind::Automatic);
        event.event.superseded_by_tip_hash = Some(vec![0xdd; 32]);
        let json = serde_json::to_value(respond(&event).unwrap()).unwrap();
        assert_eq!(
            json["supersededByTipHash"],
            format!("0x{}", "dd".repeat(32))
        );
    }

    #[test]
    fn test_reorg_event_response_reports_persisted_context() {
        let response =
            respond(&record(1_700_000_000_000, ReorgEventKind::Automatic)).expect("response");
        assert_eq!(response.id, 1_700_000_000_000);
        assert_eq!(response.event_type, "reorg");
        assert_eq!(response.fork_point_number, 17_100);
        assert_eq!(response.old_tip_number, 17_103);
        assert_eq!(response.new_tip_number, 17_104);
        assert_eq!(response.orphaned_blocks_count, 3);
        assert_eq!(response.orphaned_txs_count, 7);
        assert_eq!(response.fork_point_hash, format!("0x{}", "01".repeat(32)));
    }

    #[test]
    fn test_reorg_event_response_tags_deep_forks() {
        let response = respond(&record(1_700_000_000_000, ReorgEventKind::Deep)).expect("resp");
        assert_eq!(response.event_type, "deep");
    }

    #[test]
    fn test_parse_forks_cursor_rejects_malformed_keys() {
        assert!(parse_forks_cursor(None).unwrap().is_none());
        let valid = format!("reorg:{:013}:{}", 1_700_000_000_000i64, "ab".repeat(16));
        assert_eq!(
            parse_forks_cursor(Some(&valid)).unwrap(),
            Some(valid.as_bytes().to_vec())
        );

        for bad in ["-1", "reorg:1:2", "1700000000000", ""] {
            let err = parse_forks_cursor(Some(bad)).unwrap_err();
            assert_eq!(err.1 .0.message, "Invalid forks cursor", "input: {bad}");
        }
    }

    #[test]
    fn test_rfc3339_from_unix_seconds_rejects_unrepresentable_timestamps() {
        assert!(rfc3339_from_unix_seconds(1_700_000_000, "ctx").is_ok());
        let err = rfc3339_from_unix_seconds(i64::MAX, "reorg:key").unwrap_err();
        assert!(err.1 .0.message.contains("invalid reorg detected_at"));
    }

    #[test]
    fn is_old_branch_unavailable_matches_walk_and_enrich_messages() {
        let walk = anyhow::anyhow!(
            "CKB RocksDB is missing old branch block 17101 0x{}",
            "ab".repeat(32)
        );
        let enrich = anyhow::anyhow!(
            "CKB RocksDB lost historical old branch block 0x{}",
            "cd".repeat(32)
        );
        let other = anyhow::anyhow!("CKB RocksDB missing canonical uncle-bearing block 1");
        assert!(is_old_branch_unavailable(&walk));
        assert!(is_old_branch_unavailable(&enrich));
        assert!(!is_old_branch_unavailable(&other));
        // Wrapped display still carries the marker through the cause chain.
        let wrapped = walk.context("reading orphaned branch");
        assert!(is_old_branch_unavailable(&wrapped));
    }
}
