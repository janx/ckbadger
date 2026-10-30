//! API-based checks — validates data via the ckbadger REST API.
//!
//! Fast tier (F1-F6): few API calls, seconds.
//! Sampling tier (S1-S23): N API calls or chart validation, minutes.

use super::checks::*;
use super::report::format_number;
use super::sampling::LcgSampler;
use ckbadger_common::TokenBalance;
use std::collections::{BTreeSet, HashMap};

const SYNC_COMPLETE_MAX_LAG_BLOCKS: i64 = 100;
const SHANNONS_PER_CKB: i128 = 100_000_000;
const TOKEN_ACTIVITY_ADDRESS_LIMIT: usize = 20;
const TOKEN_ACTIVITY_PAGE_LIMIT: usize = 100;
const TOKEN_ACTIVITY_MAX_PAGES_PER_ADDRESS: usize = 3;
const TOKEN_TRANSFER_PAGE_LIMIT: usize = 100;
const SPORE_CLUSTER_LIST_LIMIT: usize = 100;
const SPORE_CLUSTER_SAMPLE_MAX: usize = 20;
const SPORE_CLUSTER_SPORE_PAGE_LIMIT: usize = 100;
const SPORE_PER_CLUSTER_SAMPLE: usize = 2;
const SPORE_OWNER_PAGE_LIMIT: usize = 100;
const SPORE_OWNER_MAX_PAGES: usize = 50;
const OBJECT_ASSET_LIST_LIMIT: usize = 100;
const OBJECT_COLLECTION_SAMPLE_MAX: usize = 20;
const TOP_ASSET_LIMIT: usize = 10;
const TOP_HOLDER_LIMIT: usize = 10;
const IDENTITY_HOLDER_SPOT_CHECK_LIMIT: usize = 10;
const ADDRESS_TOKENS_LIMIT: usize = 100;
const ADDRESS_BALANCE_CANDIDATE_LIMIT: usize = 500;
const ADDRESS_BALANCE_ACTIVE_DAYS: usize = 365;
const ADDRESS_BALANCE_SAMPLE_MAX: usize = 10;
const ADDRESS_BALANCE_CELL_PAGE_LIMIT: usize = 100;
/// The address-balance check is a sampling check, not an exhaustive scan.
/// Every selected address is still checked exactly, but candidate selection
/// rejects whale addresses so the total HTTP expansion has a fixed bound.
const ADDRESS_BALANCE_MAX_LIVE_CELLS_PER_SAMPLE: usize = 1_000;
const ADDRESS_BALANCE_MAX_TOTAL_LIVE_CELLS: usize = 5_000;
/// `cell_by_lock` contains historical outputs and filters consumed cells while
/// scanning. Bound address activity as well as returned live cells so a nearly
/// empty but extremely hot address is not an accidentally exhaustive query.
const ADDRESS_BALANCE_MAX_TXS_PER_SAMPLE: i64 = 10_000;
const ADDRESS_BALANCE_MAX_TOTAL_TXS: i64 = 50_000;
// Safety cap for paginating /addresses/{addr}/tokens when searching for a
// specific top-holder's token. 1000 pages x 100 = 100k tokens; exceeding this
// is almost certainly a data bug, so hitting the cap is reported as a finding.
const ADDRESS_TOKENS_MAX_PAGES: usize = 1000;

// ---------------------------------------------------------------------------
// Lightweight API response types (deserialized from ckbadger API JSON).
// These are intentionally minimal — only the fields we need for verification.
// ---------------------------------------------------------------------------

/// Wrapper for chart endpoints that return `{ "data": [...] }`.
#[derive(serde::Deserialize)]
struct ChartWrapper<T> {
    data: Vec<T>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct NetworkStats {
    latest_block: i64,
    sync_status: SyncStatus,
    deep_fork_status: DeepForkStatus,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct SyncStatus {
    is_syncing: bool,
    synced_block: i64,
    tip_block: i64,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeepForkStatus {
    detected: bool,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct BlockResponse {
    number: i64,
    hash: String,
    parent_hash: String,
    transactions_count: i32,
    proposals_count: i32,
    uncles_count: i32,
    compact_target: String,
    difficulty: String,
    #[serde(default)]
    miner_address: Option<String>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct DaoStatisticsResponse {
    total_deposited: String,
    estimated_apc: String,
    active_deposits: i64,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct DaoGenesisBaselineResponse {
    genesis_baseline_burnt: String,
}

fn genesis_baseline_burnt(ctx: &CheckContext) -> anyhow::Result<i128> {
    let stats: DaoGenesisBaselineResponse = api_get(ctx, "dao/statistics")?;
    parse_non_negative_i128(&stats.genesis_baseline_burnt).ok_or_else(|| {
        anyhow::anyhow!(
            "invalid /dao/statistics genesisBaselineBurnt '{}'",
            stats.genesis_baseline_burnt
        )
    })
}

#[derive(Clone, Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct AddressCandidateResponse {
    lock_script_hash: String,
    live_cells_count: i64,
    transactions_count: i64,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct CellResponse {
    capacity: String,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct CellListResponse {
    data: Vec<CellResponse>,
    next_cursor: Option<String>,
}

/// The `CursorPaginatedResponse` envelope every cursor-paged list endpoint
/// serves (`{data, limit, hasMore, nextCursor}`).
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct CursorPage<T> {
    pub(super) data: Vec<T>,
    pub(super) next_cursor: Option<String>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct CursorPageWithTotal<T> {
    data: Vec<T>,
    total: Option<i64>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct AddressActivityRecord {
    tx_hash: String,
    block_number: i64,
    item_deltas: Vec<serde_json::Value>,
}

/// One party of a global activity row.
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct GlobalParticipantApiRecord {
    lock_hash: Option<String>,
    lock_hash_prefix: Option<String>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct GlobalActivityApiRecord {
    tx_hash: String,
    block_number: i64,
    participants: Vec<GlobalParticipantApiRecord>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct AddressTxHashRecord {
    tx_hash: String,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct TokenTransferApiRecord {
    tx_hash: String,
    block_number: i64,
    from_lock_hash: Option<String>,
    to_lock_hash: String,
    amount: String,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct SporeClusterApiRecord {
    cluster_id: String,
}

#[derive(Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct SporeApiRecord {
    spore_id: String,
    owner_lock_hash: String,
    cluster_id: Option<String>,
    is_live: bool,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct AssetListApiRecord {
    id: String,
    asset_type: String,
    standard: String,
    holders_count: i64,
}

/// Object supply counts items ever created; liveCount counts surviving items.
/// transfersCount is token-only and is not part of the object wire contract.
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ObjectAssetListApiRecord {
    #[serde(flatten)]
    asset: AssetListApiRecord,
    total_supply: String,
    live_count: i64,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct AddressBalanceApiRecord {
    balance: String,
    live_cells_count: i64,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct NftCollectionDetailApiRecord {
    collection_id: String,
    total_count: i64,
    live_count: i64,
    holders_count: i64,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct TokenDetailApiRecord {
    type_script_hash: String,
    holders_count: i64,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct SporeClusterDetailApiRecord {
    cluster_id: String,
    holders_count: i64,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct TokenHolderApiRecord {
    lock_script_hash: String,
    balance: String,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct NftHolderApiRecord {
    lock_script_hash: String,
    item_count: i64,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct AddressDetailApiRecord {
    lock_script_hash: String,
    live_cells_count: i64,
    transactions_count: i64,
    recent_activities_count: i64,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct AddressTokenApiRecord {
    type_script_hash: String,
    balance: String,
}

/// Simple chart point with a single value (e.g. transaction-count).
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ChartDataPoint {
    date: String,
    value: String,
    #[serde(default)]
    value2: Option<String>,
}

/// Stacked chart point with named series (e.g. cell-count, total-supply).
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct StackedChartDataPoint {
    date: String,
    values: std::collections::HashMap<String, String>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct SecondaryIssuancePoint {
    date: String,
    values: std::collections::HashMap<String, String>,
    protocol_total_shannons: Option<String>,
    phase1_compensation_shannons: Option<String>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct MinerDistributionDataPoint {
    miner_lock_hash: String,
    address: Option<String>,
    blocks_mined: i64,
    percentage: String,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct MinerDistributionResponse {
    data: Vec<MinerDistributionDataPoint>,
    total_blocks: i64,
}

/// Fetch network stats (used by multiple fast checks).
fn fetch_network_stats(ctx: &CheckContext) -> anyhow::Result<NetworkStats> {
    api_get(ctx, "statistics/network")
}

fn sampling_tip_from_stats(stats: &NetworkStats) -> anyhow::Result<u64> {
    let candidates = [
        stats.latest_block,
        stats.sync_status.synced_block,
        stats.sync_status.tip_block,
    ];
    let tip = candidates
        .into_iter()
        .filter(|v| *v >= 0)
        .min()
        .ok_or_else(|| anyhow::anyhow!(
            "cannot sample API checks before genesis: latest_block={}, synced_block={}, tip_block={}",
            stats.latest_block, stats.sync_status.synced_block, stats.sync_status.tip_block
        ))?;
    Ok(tip as u64)
}

fn normalize_hex_key(raw: &str) -> String {
    let trimmed = raw.trim();
    trimmed
        .strip_prefix("0x")
        .or_else(|| trimmed.strip_prefix("0X"))
        .unwrap_or(trimmed)
        .to_ascii_lowercase()
}

fn address_balance_sample_bucket(live_cells_count: usize) -> usize {
    match live_cells_count {
        0..=1 => 0,
        2..=10 => 1,
        11..=100 => 2,
        _ => 3,
    }
}

/// Select a reproducible, cardinality-diverse set of addresses while placing
/// a hard bound on the work performed by the sampling-tier balance check.
///
/// Candidate data is deduplicated by lock hash before sampling. No cells are
/// sampled within an address: once selected, all of that address's live cells
/// are checked exactly.
fn select_address_balance_samples(
    candidates: Vec<AddressCandidateResponse>,
    requested: usize,
    seed: u64,
) -> anyhow::Result<Vec<AddressCandidateResponse>> {
    if requested == 0 {
        return Ok(Vec::new());
    }

    let mut seen = std::collections::HashMap::<String, (i64, i64)>::new();
    let mut buckets: [Vec<AddressCandidateResponse>; 4] = std::array::from_fn(|_| Vec::new());

    for candidate in candidates {
        let normalized_hash = normalize_hex_key(&candidate.lock_script_hash);
        let decoded_hash = hex::decode(&normalized_hash).map_err(|e| {
            anyhow::anyhow!(
                "invalid address candidate lock hash '{}': {}",
                candidate.lock_script_hash,
                e
            )
        })?;
        if decoded_hash.len() != 32 {
            anyhow::bail!(
                "invalid address candidate lock hash length: lock_hash={}, bytes={}",
                candidate.lock_script_hash,
                decoded_hash.len()
            );
        }
        if candidate.live_cells_count < 0 {
            anyhow::bail!(
                "negative address candidate liveCellsCount: lock_hash={}, live_cells_count={}",
                candidate.lock_script_hash,
                candidate.live_cells_count
            );
        }
        if candidate.transactions_count < 0 {
            anyhow::bail!(
                "negative address candidate transactionsCount: lock_hash={}, transactions_count={}",
                candidate.lock_script_hash,
                candidate.transactions_count
            );
        }

        let metrics = (candidate.live_cells_count, candidate.transactions_count);
        if let Some(previous) = seen.get(&normalized_hash) {
            if *previous != metrics {
                anyhow::bail!(
                    "conflicting duplicate address candidate: lock_hash={}, first_live_cells={}, first_transactions={}, duplicate_live_cells={}, duplicate_transactions={}",
                    candidate.lock_script_hash,
                    previous.0,
                    previous.1,
                    candidate.live_cells_count,
                    candidate.transactions_count
                );
            }
            continue;
        }
        seen.insert(normalized_hash, metrics);

        let live_cells_count = usize::try_from(candidate.live_cells_count).map_err(|e| {
            anyhow::anyhow!(
                "address candidate liveCellsCount does not fit usize: lock_hash={}, live_cells_count={}, error={}",
                candidate.lock_script_hash,
                candidate.live_cells_count,
                e
            )
        })?;
        if live_cells_count > ADDRESS_BALANCE_MAX_LIVE_CELLS_PER_SAMPLE
            || candidate.transactions_count > ADDRESS_BALANCE_MAX_TXS_PER_SAMPLE
        {
            continue;
        }

        buckets[address_balance_sample_bucket(live_cells_count)].push(candidate);
    }

    let mut sampler = LcgSampler::new(seed.wrapping_add(0xA44D_5233_5A11_9EED));
    for bucket in &mut buckets {
        sampler.shuffle(bucket);
    }

    let requested = requested.min(ADDRESS_BALANCE_SAMPLE_MAX);
    let mut selected = Vec::with_capacity(requested);
    let mut positions = [0usize; 4];
    let mut selected_live_cells = 0usize;
    let mut selected_transactions = 0i64;

    while selected.len() < requested {
        let mut selected_in_round = false;
        for bucket_index in 0..buckets.len() {
            while let Some(candidate) = buckets[bucket_index].get(positions[bucket_index]) {
                positions[bucket_index] += 1;
                let live_cells_count = usize::try_from(candidate.live_cells_count).map_err(|e| {
                    anyhow::anyhow!(
                        "selected liveCellsCount does not fit usize: lock_hash={}, live_cells_count={}, error={}",
                        candidate.lock_script_hash,
                        candidate.live_cells_count,
                        e
                    )
                })?;
                let next_live_cells = selected_live_cells
                    .checked_add(live_cells_count)
                    .ok_or_else(|| {
                        anyhow::anyhow!("address balance sample live-cell budget overflow")
                    })?;
                let next_transactions = selected_transactions
                    .checked_add(candidate.transactions_count)
                    .ok_or_else(|| {
                        anyhow::anyhow!("address balance sample transaction budget overflow")
                    })?;
                if next_live_cells > ADDRESS_BALANCE_MAX_TOTAL_LIVE_CELLS
                    || next_transactions > ADDRESS_BALANCE_MAX_TOTAL_TXS
                {
                    continue;
                }

                selected_live_cells = next_live_cells;
                selected_transactions = next_transactions;
                selected.push(candidate.clone());
                selected_in_round = true;
                break;
            }

            if selected.len() >= requested {
                break;
            }
        }
        if !selected_in_round {
            break;
        }
    }

    Ok(selected)
}

/// Parse an exact aggregate token balance. Holder balances may sum many u128 cells.
fn parse_token_balance_strict(raw: &str, field_name: &str) -> anyhow::Result<TokenBalance> {
    raw.parse::<TokenBalance>()
        .map_err(|e| anyhow::anyhow!("invalid {} '{}': {}", field_name, raw, e))
}

/// Parse a single UDT transfer amount, whose on-chain representation is u128.
fn parse_u128_strict(raw: &str, field_name: &str) -> anyhow::Result<u128> {
    raw.parse::<u128>()
        .map_err(|e| anyhow::anyhow!("invalid {} '{}': {}", field_name, raw, e))
}

/// Parse a signed UDT delta decimal string (e.g. "-1000", "222...784") into
/// (magnitude, negative). A ±u128 net delta does not fit i128, so it is carried as
/// signed-magnitude. "0"/"-0" normalize to (0, false).
fn parse_signed_decimal(raw: &str, field_name: &str) -> anyhow::Result<(u128, bool)> {
    let (neg, digits) = match raw.strip_prefix('-') {
        Some(d) => (true, d),
        None => (false, raw),
    };
    let magnitude = digits
        .parse::<u128>()
        .map_err(|e| anyhow::anyhow!("invalid {} '{}': {}", field_name, raw, e))?;
    Ok((magnitude, magnitude != 0 && neg))
}

/// Render a signed-magnitude value as a canonical decimal string ("0", "130", "-100").
fn signed_decimal_string(magnitude: u128, negative: bool) -> String {
    if negative && magnitude != 0 {
        format!("-{}", magnitude)
    } else {
        magnitude.to_string()
    }
}

#[derive(Clone)]
struct AddressCountSnapshot {
    lock_script_hash: String,
    live_cells_count: i64,
    transactions_count: i64,
    recent_activities_count: i64,
    tx_total: i64,
    live_cell_total: i64,
}

fn fetch_address_count_snapshot(
    ctx: &CheckContext,
    holder_lock_hash: &str,
) -> anyhow::Result<AddressCountSnapshot> {
    let address: AddressDetailApiRecord = api_get(ctx, &format!("addresses/{}", holder_lock_hash))?;

    // Read the pre-computed totals exposed by the transaction and live-cell
    // endpoints instead of paginating through every record.
    let tx_page: CursorPageWithTotal<serde_json::Value> = api_get(
        ctx,
        &format!("addresses/{}/transactions?limit=1", holder_lock_hash),
    )?;
    // Address activities are a different set from address transactions
    // (cellbase rows are absent); their `total` subtracts the persisted
    // cellbase count, which `address_cellbase_count_consistency` verifies.
    // Here, probe the endpoint and its cursor contract for every sampled holder.
    let _: CursorPage<serde_json::Value> = api_get(
        ctx,
        &format!("addresses/{}/activities?limit=1", holder_lock_hash),
    )?;
    let live_cell_page: CursorPageWithTotal<serde_json::Value> = api_get(
        ctx,
        &format!("cells/live?lock_script_hash={}&limit=1", holder_lock_hash),
    )?;

    let tx_total = tx_page.total.ok_or_else(|| {
        anyhow::anyhow!(
            "transactions endpoint missing total for {}",
            holder_lock_hash
        )
    })?;
    let live_cell_total = live_cell_page.total.ok_or_else(|| {
        anyhow::anyhow!("live cells endpoint missing total for {}", holder_lock_hash)
    })?;

    Ok(AddressCountSnapshot {
        lock_script_hash: address.lock_script_hash,
        live_cells_count: address.live_cells_count,
        transactions_count: address.transactions_count,
        recent_activities_count: address.recent_activities_count,
        tx_total,
        live_cell_total,
    })
}

fn address_count_mismatch_details(
    holder_lock_hash: &str,
    snapshot: &AddressCountSnapshot,
) -> Vec<String> {
    let mut details = vec![];
    if normalize_hex_key(&snapshot.lock_script_hash) != normalize_hex_key(holder_lock_hash) {
        details.push(format!(
            "address lock_script_hash mismatch: address=0x{}, holder=0x{}",
            normalize_hex_key(&snapshot.lock_script_hash),
            normalize_hex_key(holder_lock_hash)
        ));
    }
    if snapshot.transactions_count != snapshot.recent_activities_count {
        details.push(format!(
            "address transactionsCount={} != recentActivitiesCount={}",
            snapshot.transactions_count, snapshot.recent_activities_count
        ));
    }
    if snapshot.tx_total != snapshot.transactions_count {
        details.push(format!(
            "transactions endpoint total={} != address transactionsCount={}",
            snapshot.tx_total, snapshot.transactions_count
        ));
    }
    if snapshot.live_cell_total != snapshot.live_cells_count {
        details.push(format!(
            "live cells endpoint total={} != address liveCellsCount={}",
            snapshot.live_cell_total, snapshot.live_cells_count
        ));
    }
    details
}

fn token_holder_balance_mismatch_details(
    ctx: &CheckContext,
    token_type_hash: &str,
    holder_lock_hash: &str,
    holder_balance: &str,
) -> anyhow::Result<Vec<String>> {
    let mut details = vec![];
    let holder_balance_value = parse_token_balance_strict(holder_balance, "token holder balance")?;
    let token_key = normalize_hex_key(token_type_hash);

    // The address-tokens list is sorted by balance DESC, so a holder with
    // more than ADDRESS_TOKENS_LIMIT (100) distinct tokens can legitimately
    // have the target token beyond page 1 — e.g. a whale wallet whose other
    // positions have larger raw balances than this token. Paginate through
    // the full list until we find the target or exhaust the pages.
    let mut cursor: Option<String> = None;
    let mut pages_scanned = 0usize;
    let mut found_balance: Option<TokenBalance> = None;
    loop {
        let path = match cursor.as_deref() {
            Some(c) => format!(
                "addresses/{}/tokens?limit={}&cursor={}",
                holder_lock_hash, ADDRESS_TOKENS_LIMIT, c
            ),
            None => format!(
                "addresses/{}/tokens?limit={}",
                holder_lock_hash, ADDRESS_TOKENS_LIMIT
            ),
        };
        let page: CursorPage<AddressTokenApiRecord> = api_get(ctx, &path)?;
        pages_scanned += 1;

        if let Some(entry) = page
            .data
            .iter()
            .find(|entry| normalize_hex_key(&entry.type_script_hash) == token_key)
        {
            found_balance = Some(parse_token_balance_strict(
                &entry.balance,
                "address token balance",
            )?);
            break;
        }

        match page.next_cursor {
            Some(next) => {
                if pages_scanned >= ADDRESS_TOKENS_MAX_PAGES {
                    details.push(format!(
                        "address tokens scan exceeded cap of {} pages (~{} tokens) without finding 0x{}",
                        ADDRESS_TOKENS_MAX_PAGES,
                        ADDRESS_TOKENS_MAX_PAGES * ADDRESS_TOKENS_LIMIT,
                        token_key
                    ));
                    return Ok(details);
                }
                cursor = Some(next);
            }
            None => break,
        }
    }

    match found_balance {
        Some(address_balance) => {
            if address_balance != holder_balance_value {
                details.push(format!(
                    "token balance mismatch: holders={} address_tokens={}",
                    holder_balance_value, address_balance
                ));
            }
        }
        None => {
            details.push(format!(
                "address tokens list missing token 0x{} after scanning {} page(s)",
                token_key, pages_scanned
            ));
        }
    }

    Ok(details)
}

fn load_address_snapshot(
    ctx: &CheckContext,
    cache: &mut std::collections::HashMap<String, AddressCountSnapshot>,
    holder_lock_hash: &str,
) -> anyhow::Result<AddressCountSnapshot> {
    let holder_key = normalize_hex_key(holder_lock_hash);
    if holder_key.is_empty() {
        anyhow::bail!("holder lock script hash is empty");
    }
    if let Some(snapshot) = cache.get(&holder_key) {
        return Ok(snapshot.clone());
    }

    let snapshot = fetch_address_count_snapshot(ctx, holder_lock_hash)?;
    cache.insert(holder_key, snapshot.clone());
    Ok(snapshot)
}

fn extract_activity_token_deltas(
    activity: &AddressActivityRecord,
) -> anyhow::Result<Vec<(String, (u128, bool))>> {
    let mut deltas = Vec::new();

    for item in &activity.item_deltas {
        if item.get("kind").and_then(|v| v.as_str()) != Some("token") {
            continue;
        }

        let Some(type_hash) = item.get("typeScriptHash").and_then(|v| v.as_str()) else {
            continue;
        };
        let delta_raw = item.get("delta").and_then(|v| v.as_str()).ok_or_else(|| {
            anyhow::anyhow!("token activity missing delta: tx_hash={}", activity.tx_hash)
        })?;
        let delta = parse_signed_decimal(delta_raw, "token activity delta")?;

        deltas.push((normalize_hex_key(type_hash), delta));
    }

    Ok(deltas)
}

fn apply_transfer_delta_to_lookup(
    lookup: &mut std::collections::HashMap<(String, String, String), (u128, u128)>,
    token_type_hash: &str,
    transfer: &TokenTransferApiRecord,
) -> anyhow::Result<()> {
    let token_key = normalize_hex_key(token_type_hash);
    let tx_key = normalize_hex_key(&transfer.tx_hash);
    let amount = parse_u128_strict(&transfer.amount, "token transfer amount")?;

    // Per (token, tx, lock) net delta carried as (received, sent) u128 sums; the signed
    // net is derived by difference at comparison time (a ±u128 net does not fit i128).
    let to_lock_key = normalize_hex_key(&transfer.to_lock_hash);
    if !to_lock_key.is_empty() {
        let key = (token_key.clone(), tx_key.clone(), to_lock_key);
        let entry = lookup.entry(key).or_insert((0, 0));
        entry.0 = entry.0.checked_add(amount).ok_or_else(|| {
            anyhow::anyhow!(
                "token transfer received overflow: tx_hash={}, token_type_hash={}",
                transfer.tx_hash,
                token_type_hash
            )
        })?;
    }

    if let Some(from_lock_hash) = transfer.from_lock_hash.as_deref() {
        let from_lock_key = normalize_hex_key(from_lock_hash);
        if !from_lock_key.is_empty() {
            let key = (token_key, tx_key, from_lock_key);
            let entry = lookup.entry(key).or_insert((0, 0));
            entry.1 = entry.1.checked_add(amount).ok_or_else(|| {
                anyhow::anyhow!(
                    "token transfer sent overflow: tx_hash={}, token_type_hash={}",
                    transfer.tx_hash,
                    token_type_hash
                )
            })?;
        }
    }

    Ok(())
}

fn sample_indices_with_cap(seed: u64, total: usize, desired: usize, cap: usize) -> Vec<usize> {
    if total == 0 || desired == 0 || cap == 0 {
        return vec![];
    }
    let target = desired.min(cap).min(total);
    if target == total {
        return (0..total).collect();
    }
    let mut sampler = LcgSampler::new(seed);
    sampler
        .sample_range(target, total as u64)
        .into_iter()
        .map(|v| v as usize)
        .collect()
}

// ============================================
// FAST CHECKS (F1-F6)
// ============================================

/// F1: GET /statistics/network returns 200, latestBlock > 0.
pub struct ApiReachable;

impl Check for ApiReachable {
    fn name(&self) -> &'static str {
        "api_reachable"
    }
    fn description(&self) -> &'static str {
        "API reachable, latestBlock > 0"
    }
    fn tier(&self) -> CheckTier {
        CheckTier::Fast
    }
    fn run(&self, ctx: &CheckContext, _progress: &ProgressReporter) -> anyhow::Result<CheckResult> {
        let stats = fetch_network_stats(ctx)?;
        if stats.latest_block > 0 {
            Ok(CheckResult::pass_with_detail(
                1,
                format!(
                    "latestBlock = #{}",
                    format_number(stats.latest_block as u64)
                ),
            ))
        } else {
            Ok(CheckResult::fail(
                1,
                vec![Finding {
                    entity: "api".to_string(),
                    details: vec![format!(
                        "latestBlock = {}, expected > 0",
                        stats.latest_block
                    )],
                }],
            ))
        }
    }
}

/// F2: syncStatus.isSyncing == false, lag <= 100 blocks.
pub struct SyncComplete;

impl Check for SyncComplete {
    fn name(&self) -> &'static str {
        "sync_complete"
    }
    fn description(&self) -> &'static str {
        "Sync complete (isSyncing=false, lag <= 100 blocks)"
    }
    fn tier(&self) -> CheckTier {
        CheckTier::Fast
    }
    fn run(&self, ctx: &CheckContext, _progress: &ProgressReporter) -> anyhow::Result<CheckResult> {
        let stats = fetch_network_stats(ctx)?;
        let ss = &stats.sync_status;
        let lag = ss.tip_block - ss.synced_block;
        let findings = sync_complete_findings(ss);

        if findings.is_empty() {
            Ok(CheckResult::pass_with_detail(
                1,
                if lag == 0 {
                    format!("synced to #{}", format_number(ss.synced_block as u64))
                } else {
                    format!(
                        "near tip: synced #{} (lag {} blocks)",
                        format_number(ss.synced_block as u64),
                        format_number(lag as u64),
                    )
                },
            ))
        } else {
            Ok(CheckResult::fail(1, findings))
        }
    }
}

fn sync_complete_findings(ss: &SyncStatus) -> Vec<Finding> {
    let mut findings = vec![];
    let lag = ss.tip_block - ss.synced_block;
    if lag < 0 {
        findings.push(Finding {
            entity: "sync_status".to_string(),
            details: vec![format!(
                "synced block ahead of tip (synced={}, tip={}, lag={})",
                ss.synced_block, ss.tip_block, lag
            )],
        });
    }

    if ss.is_syncing {
        findings.push(Finding {
            entity: "sync_status".to_string(),
            details: vec![format!(
                "isSyncing=true (synced={}, tip={}, lag={})",
                ss.synced_block, ss.tip_block, lag,
            )],
        });
    }

    if lag > SYNC_COMPLETE_MAX_LAG_BLOCKS {
        findings.push(Finding {
            entity: "sync_status".to_string(),
            details: vec![format!(
                "lag {} > {} blocks (synced={}, tip={})",
                lag, SYNC_COMPLETE_MAX_LAG_BLOCKS, ss.synced_block, ss.tip_block,
            )],
        });
    }

    findings
}

/// F3: GET /blocks/0 returns valid genesis block.
pub struct GenesisBlock;

impl Check for GenesisBlock {
    fn name(&self) -> &'static str {
        "genesis_block"
    }
    fn description(&self) -> &'static str {
        "Block 0 exists with valid genesis hash"
    }
    fn tier(&self) -> CheckTier {
        CheckTier::Fast
    }
    fn run(&self, ctx: &CheckContext, _progress: &ProgressReporter) -> anyhow::Result<CheckResult> {
        let block: BlockResponse = api_get(ctx, "blocks/0")?;

        if block.number != 0 {
            return Ok(CheckResult::fail(
                1,
                vec![Finding {
                    entity: "block_0".to_string(),
                    details: vec![format!("number = {}, expected 0", block.number)],
                }],
            ));
        }
        if block.hash.is_empty() {
            return Ok(CheckResult::fail(
                1,
                vec![Finding {
                    entity: "block_0".to_string(),
                    details: vec!["hash is empty".to_string()],
                }],
            ));
        }

        Ok(CheckResult::pass_with_detail(
            1,
            format!("hash={}", &block.hash[..18]),
        ))
    }
}

/// F4: GET /blocks/{latestBlock} exists, transactionsCount > 0.
pub struct TipBlock;

impl Check for TipBlock {
    fn name(&self) -> &'static str {
        "tip_block"
    }
    fn description(&self) -> &'static str {
        "Tip block exists with transactionsCount > 0"
    }
    fn tier(&self) -> CheckTier {
        CheckTier::Fast
    }
    fn run(&self, ctx: &CheckContext, _progress: &ProgressReporter) -> anyhow::Result<CheckResult> {
        let stats = fetch_network_stats(ctx)?;
        let tip = stats.latest_block;

        let block: BlockResponse = api_get(ctx, &format!("blocks/{}", tip))?;

        let mut findings = vec![];
        if block.transactions_count <= 0 {
            findings.push(Finding {
                entity: format!("block_{}", tip),
                details: vec![format!(
                    "transactionsCount = {}, expected > 0",
                    block.transactions_count,
                )],
            });
        }

        if findings.is_empty() {
            Ok(CheckResult::pass_with_detail(
                1,
                format!(
                    "tip #{}, {} txs",
                    format_number(tip as u64),
                    block.transactions_count
                ),
            ))
        } else {
            Ok(CheckResult::fail(1, findings))
        }
    }
}

/// F5: GET /forks/recent → deepFork.detected == false.
pub struct DeepForkClear;

impl Check for DeepForkClear {
    fn name(&self) -> &'static str {
        "deep_fork_clear"
    }
    fn description(&self) -> &'static str {
        "No unresolved deep fork detected"
    }
    fn tier(&self) -> CheckTier {
        CheckTier::Fast
    }
    fn run(&self, ctx: &CheckContext, _progress: &ProgressReporter) -> anyhow::Result<CheckResult> {
        let stats = fetch_network_stats(ctx)?;
        if !stats.deep_fork_status.detected {
            Ok(CheckResult::pass(1))
        } else {
            Ok(CheckResult::fail(
                1,
                vec![Finding {
                    entity: "deep_fork".to_string(),
                    details: vec!["deep_fork_status.detected = true".to_string()],
                }],
            ))
        }
    }
}

/// F6: GET /dao/statistics → totalDeposited > 0, estimatedApc > 0, activeDeposits > 0.
pub struct DaoStatisticsSane;

impl Check for DaoStatisticsSane {
    fn name(&self) -> &'static str {
        "dao_statistics_sane"
    }
    fn description(&self) -> &'static str {
        "DAO statistics: deposits > 0, APC > 0, active deposits > 0"
    }
    fn tier(&self) -> CheckTier {
        CheckTier::Fast
    }
    fn run(&self, ctx: &CheckContext, _progress: &ProgressReporter) -> anyhow::Result<CheckResult> {
        let dao: DaoStatisticsResponse = api_get(ctx, "dao/statistics")?;
        let mut findings = vec![];

        let total_deposited: f64 = match dao.total_deposited.parse() {
            Ok(v) => v,
            Err(_) => {
                findings.push(Finding {
                    entity: "dao_statistics".to_string(),
                    details: vec![format!(
                        "could not parse totalDeposited: '{}'",
                        dao.total_deposited
                    )],
                });
                return Ok(CheckResult::fail(1, findings));
            }
        };
        if total_deposited <= 0.0 {
            findings.push(Finding {
                entity: "dao_statistics".to_string(),
                details: vec![format!("totalDeposited = {}", dao.total_deposited)],
            });
        }

        let apc: f64 = match dao.estimated_apc.parse() {
            Ok(v) => v,
            Err(_) => {
                findings.push(Finding {
                    entity: "dao_statistics".to_string(),
                    details: vec![format!(
                        "could not parse estimatedApc: '{}'",
                        dao.estimated_apc
                    )],
                });
                return Ok(CheckResult::fail(1, findings));
            }
        };
        if apc <= 0.0 {
            findings.push(Finding {
                entity: "dao_statistics".to_string(),
                details: vec![format!("estimatedApc = {}", dao.estimated_apc)],
            });
        }

        if dao.active_deposits <= 0 {
            findings.push(Finding {
                entity: "dao_statistics".to_string(),
                details: vec![format!("activeDeposits = {}", dao.active_deposits)],
            });
        }

        if findings.is_empty() {
            Ok(CheckResult::pass_with_detail(
                1,
                format!(
                    "deposited={}, apc={}%, deposits={}",
                    dao.total_deposited, dao.estimated_apc, dao.active_deposits
                ),
            ))
        } else {
            Ok(CheckResult::fail(1, findings))
        }
    }
}

/// F7: The latest completed-day burnt gap equals the active network's
/// persisted genesis economic baseline.
///
/// The verify harness is purely API-backed and has no store handle, so this
/// check observes the baseline through the API rather than reading
/// `store.get_genesis_baseline()` directly:
///  - Presence: both `/charts/total-supply` and `/charts/secondary-issuance`
///    are derived from `GenesisBaseline` on the API side and fail-fast if it
///    was never derived, so a successful fetch proves the baseline is present.
///  - Value: `total-supply.burnt - secondary-issuance.burnt` isolates the
///    genesis burnt and is compared with `/dao/statistics.genesisBaselineBurnt`,
///    read from the same canonical persisted baseline.
///
/// Both endpoints publish only complete UTC+8 days from the same indexed DAO
/// daily snapshot history. Their day boundaries depend on the indexed monotone
/// chain-time calendar; this check does not independently re-verify that clock.
///
/// This is the Fast-tier counterpart to S17 (`BurntSupplyGenesisInvariant`),
/// which checks every overlapping completed date.
pub struct GenesisBaselineBurntInvariant;

impl Check for GenesisBaselineBurntInvariant {
    fn name(&self) -> &'static str {
        "genesis_baseline_burnt_invariant"
    }
    fn description(&self) -> &'static str {
        "Latest completed-day burnt gap equals exact genesis burnt (shannons)"
    }
    fn tier(&self) -> CheckTier {
        CheckTier::Fast
    }
    fn run(&self, ctx: &CheckContext, _progress: &ProgressReporter) -> anyhow::Result<CheckResult> {
        let supply: ChartWrapper<StackedChartDataPoint> = api_get(ctx, "charts/total-supply")?;
        let secondary: ChartWrapper<StackedChartDataPoint> =
            api_get(ctx, "charts/secondary-issuance")?;
        let BurntGapEvidence { gaps, mut findings } =
            exact_burnt_gaps(ctx, &supply.data, &secondary.data)?;
        if let Some((date, gap)) = gaps.last() {
            let expected = genesis_baseline_burnt(ctx)?;
            if *gap != expected {
                findings.push(Finding {
                    entity: date.clone(),
                    details: vec![format!("genesis burnt gap = {gap} shannons, persisted genesis baseline = {expected} shannons")],
                });
            }
        }
        let checked = u64::from(!gaps.is_empty());
        if findings.is_empty() {
            Ok(CheckResult::pass_with_detail(
                checked,
                if let Some((date, gap)) = gaps.last() {
                    format!("genesis burnt = {gap} shannons at {date}; indexed calendar is the chart's day-boundary dependency")
                } else {
                    "no completed DAO snapshot day yet; supply, secondary and deposit charts are empty".to_string()
                },
            ))
        } else {
            Ok(CheckResult::fail(checked, findings))
        }
    }
}

// ============================================
// SAMPLING CHECKS (S1-S19)
// ============================================

/// S1: N random blocks: GET /blocks/{n} → hash, GET /blocks/{hash} → number matches.
pub struct BlockHashRoundtrip;

impl Check for BlockHashRoundtrip {
    fn name(&self) -> &'static str {
        "block_hash_roundtrip"
    }

    /// Reads the sample count: at 0 it has nothing to select.
    fn requires_sampling(&self) -> bool {
        true
    }
    fn description(&self) -> &'static str {
        "Block number → hash → number roundtrip consistency"
    }
    fn tier(&self) -> CheckTier {
        CheckTier::Sampling
    }
    fn estimated_total(&self, ctx: &CheckContext) -> Option<u64> {
        Some(ctx.sample_count as u64)
    }
    fn run(&self, ctx: &CheckContext, progress: &ProgressReporter) -> anyhow::Result<CheckResult> {
        let stats = fetch_network_stats(ctx)?;
        let tip = sampling_tip_from_stats(&stats)?;
        if tip == 0 {
            return Ok(CheckResult::pass(0));
        }

        let mut sampler = LcgSampler::new(ctx.seed);
        let blocks = sampler.sample_range(ctx.sample_count, tip + 1);
        let mut findings = vec![];

        for block_num in &blocks {
            let by_number: BlockResponse = api_get(ctx, &format!("blocks/{}", block_num))?;

            // Now look up by hash
            let by_hash: BlockResponse = api_get(ctx, &format!("blocks/{}", by_number.hash))?;

            if by_hash.number != *block_num as i64 {
                findings.push(Finding {
                    entity: format!("block #{}", block_num),
                    details: vec![format!(
                        "hash {} → block_number = {}, expected {}",
                        by_number.hash, by_hash.number, block_num,
                    )],
                });
            }
            progress.inc(1);
        }

        let checked = blocks.len() as u64;
        if findings.is_empty() {
            Ok(CheckResult::pass(checked))
        } else {
            Ok(CheckResult::fail(checked, findings))
        }
    }
}

/// S2: N random blocks: GET /blocks/{n} and /blocks/{n-1} → parentHash matches.
pub struct BlockParentChain;

impl Check for BlockParentChain {
    fn name(&self) -> &'static str {
        "block_parent_chain"
    }

    /// Reads the sample count: at 0 it has nothing to select.
    fn requires_sampling(&self) -> bool {
        true
    }
    fn description(&self) -> &'static str {
        "Block parentHash matches previous block hash"
    }
    fn tier(&self) -> CheckTier {
        CheckTier::Sampling
    }
    fn estimated_total(&self, ctx: &CheckContext) -> Option<u64> {
        Some(ctx.sample_count as u64)
    }
    fn run(&self, ctx: &CheckContext, progress: &ProgressReporter) -> anyhow::Result<CheckResult> {
        let stats = fetch_network_stats(ctx)?;
        let tip = sampling_tip_from_stats(&stats)?;
        if tip <= 1 {
            return Ok(CheckResult::pass(0));
        }

        // Sample from [1, tip] so we can always look at n-1
        let mut sampler = LcgSampler::new(ctx.seed.wrapping_add(2));
        let blocks = sampler.sample_range(ctx.sample_count, tip);
        let mut findings = vec![];

        for block_num in &blocks {
            let n = *block_num + 1; // shift range [0, tip-1) to [1, tip)
            let current: BlockResponse = api_get(ctx, &format!("blocks/{}", n))?;
            let parent: BlockResponse = api_get(ctx, &format!("blocks/{}", n - 1))?;

            if current.parent_hash != parent.hash {
                findings.push(Finding {
                    entity: format!("block #{}", n),
                    details: vec![format!(
                        "parentHash = {}, but block #{} hash = {}",
                        &current.parent_hash[..18],
                        n - 1,
                        &parent.hash[..18],
                    )],
                });
            }
            progress.inc(1);
        }

        let checked = blocks.len() as u64;
        if findings.is_empty() {
            Ok(CheckResult::pass(checked))
        } else {
            Ok(CheckResult::fail(checked, findings))
        }
    }
}

/// One address sample checked against the live-cell endpoint.
struct AddressSampleOutcome {
    /// Live cells actually paginated through for this attempt.
    cells_scanned: usize,
    /// Empty when stored state and the live-cell endpoint agree.
    details: Vec<String>,
}

/// Read one sampled address's stored state and reconcile it against every live
/// cell the endpoint reports for that lock hash.
///
/// Returns the disagreement rather than a `Finding`, so the caller can decide
/// whether it is a real bug or an artefact of the tip moving mid-check.
fn verify_address_balance_sample(
    ctx: &CheckContext,
    addr: &AddressCandidateResponse,
    cells_scanned_before: usize,
) -> anyhow::Result<AddressSampleOutcome> {
    let address_balance: AddressBalanceApiRecord =
        api_get(ctx, &format!("addresses/{}", addr.lock_script_hash))?;
    let stored_balance: i128 = address_balance.balance.parse().map_err(|e| {
        anyhow::anyhow!(
            "failed to parse balance '{}' for lock_hash {}: {}",
            address_balance.balance,
            addr.lock_script_hash,
            e
        )
    })?;

    if address_balance.live_cells_count < 0 {
        anyhow::bail!(
            "negative liveCellsCount for sampled address: lock_hash={}, live_cells_count={}",
            addr.lock_script_hash,
            address_balance.live_cells_count
        );
    }
    let stored_live_cells = usize::try_from(address_balance.live_cells_count).map_err(|e| {
        anyhow::anyhow!(
            "sampled address liveCellsCount does not fit usize: lock_hash={}, live_cells_count={}, error={}",
            addr.lock_script_hash,
            address_balance.live_cells_count,
            e
        )
    })?;
    let next_declared_total = cells_scanned_before
        .checked_add(stored_live_cells)
        .ok_or_else(|| anyhow::anyhow!("address balance sampled live-cell count overflow"))?;
    if stored_live_cells > ADDRESS_BALANCE_MAX_LIVE_CELLS_PER_SAMPLE
        || next_declared_total > ADDRESS_BALANCE_MAX_TOTAL_LIVE_CELLS
    {
        return Ok(AddressSampleOutcome {
            cells_scanned: 0,
            details: vec![format!(
                "sample grew beyond verification budget before expansion: candidate_live_cells={}, current_live_cells={}, per_address_limit={}, total_limit={}",
                addr.live_cells_count,
                stored_live_cells,
                ADDRESS_BALANCE_MAX_LIVE_CELLS_PER_SAMPLE,
                ADDRESS_BALANCE_MAX_TOTAL_LIVE_CELLS,
            )],
        });
    }

    // Paginate through all live cells for this lock_script_hash
    let mut computed_balance: i128 = 0;
    let mut computed_count = 0usize;
    let mut cursor: Option<String> = None;
    let mut scan_complete = true;
    let mut details = vec![];
    loop {
        let path = match &cursor {
            Some(c) => format!(
                "cells/live?lock_script_hash={}&limit={}&cursor={}",
                addr.lock_script_hash, ADDRESS_BALANCE_CELL_PAGE_LIMIT, c
            ),
            None => format!(
                "cells/live?lock_script_hash={}&limit={}",
                addr.lock_script_hash, ADDRESS_BALANCE_CELL_PAGE_LIMIT
            ),
        };
        let resp: CellListResponse = api_get(ctx, &path)?;
        for cell in &resp.data {
            let cap: i128 = cell.capacity.parse().map_err(|e| {
                anyhow::anyhow!(
                    "failed to parse cell capacity '{}' for lock_hash {}: {}",
                    cell.capacity,
                    addr.lock_script_hash,
                    e
                )
            })?;
            computed_balance = computed_balance.checked_add(cap).ok_or_else(|| {
                anyhow::anyhow!(
                    "computed address balance overflow: lock_hash={}, current={}, capacity={}",
                    addr.lock_script_hash,
                    computed_balance,
                    cap
                )
            })?;
        }
        computed_count = computed_count.checked_add(resp.data.len()).ok_or_else(|| {
            anyhow::anyhow!(
                "computed live-cell count overflow: lock_hash={}",
                addr.lock_script_hash
            )
        })?;
        let total_cells_after_page = cells_scanned_before
            .checked_add(computed_count)
            .ok_or_else(|| anyhow::anyhow!("address balance total scanned-cell count overflow"))?;

        cursor = resp.next_cursor;
        if computed_count > ADDRESS_BALANCE_MAX_LIVE_CELLS_PER_SAMPLE
            || total_cells_after_page > ADDRESS_BALANCE_MAX_TOTAL_LIVE_CELLS
        {
            details.push(format!(
                "live-cell endpoint exceeded verification budget: stored_live_cells={}, scanned_at_least={}, per_address_limit={}, total_limit={}",
                stored_live_cells,
                computed_count,
                ADDRESS_BALANCE_MAX_LIVE_CELLS_PER_SAMPLE,
                ADDRESS_BALANCE_MAX_TOTAL_LIVE_CELLS,
            ));
            scan_complete = false;
            break;
        }
        if cursor.is_some() && computed_count >= stored_live_cells {
            details.push(format!(
                "live cells: stored={}, endpoint has more than {}",
                stored_live_cells, computed_count
            ));
            scan_complete = false;
            break;
        }
        if cursor.is_none() {
            break;
        }
    }

    if scan_complete {
        if stored_balance != computed_balance {
            let delta = computed_balance
                .checked_sub(stored_balance)
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "address balance delta overflow: lock_hash={}, computed={}, stored={}",
                        addr.lock_script_hash,
                        computed_balance,
                        stored_balance
                    )
                })?;
            details.push(format!(
                "balance: stored={}, computed from cells={} (Δ {})",
                stored_balance, computed_balance, delta,
            ));
        }
        if stored_live_cells != computed_count {
            details.push(format!(
                "live cells: stored={}, actual={}",
                stored_live_cells, computed_count
            ));
        }
    }

    Ok(AddressSampleOutcome {
        cells_scanned: computed_count,
        details,
    })
}

/// S3: Select bounded address samples, then sum every live-cell capacity for each sample.
pub struct AddressBalanceSpotCheck;

impl Check for AddressBalanceSpotCheck {
    fn name(&self) -> &'static str {
        "address_balance_spot_check"
    }

    /// Reads the sample count: at 0 it has nothing to select.
    fn requires_sampling(&self) -> bool {
        true
    }
    fn description(&self) -> &'static str {
        "Bounded address samples match their exact live-cell balances"
    }
    fn tier(&self) -> CheckTier {
        CheckTier::Sampling
    }
    fn estimated_total(&self, ctx: &CheckContext) -> Option<u64> {
        Some(ctx.sample_count.min(ADDRESS_BALANCE_SAMPLE_MAX) as u64)
    }
    fn run(&self, ctx: &CheckContext, progress: &ProgressReporter) -> anyhow::Result<CheckResult> {
        let requested = ctx.sample_count.min(ADDRESS_BALANCE_SAMPLE_MAX);
        if requested == 0 {
            return Ok(CheckResult::pass_with_detail(0, "sample-count is zero"));
        }

        let candidates: Vec<AddressCandidateResponse> = api_get(
            ctx,
            &format!(
                "addresses/active?limit={}&days={}",
                ADDRESS_BALANCE_CANDIDATE_LIMIT, ADDRESS_BALANCE_ACTIVE_DAYS
            ),
        )?;
        let candidate_count = candidates.len();
        let sampled_addresses = select_address_balance_samples(candidates, requested, ctx.seed)?;
        if sampled_addresses.is_empty() {
            anyhow::bail!(
                "no bounded address-balance samples available: candidates={}, max_live_cells_per_address={}, max_transactions_per_address={}",
                candidate_count,
                ADDRESS_BALANCE_MAX_LIVE_CELLS_PER_SAMPLE,
                ADDRESS_BALANCE_MAX_TXS_PER_SAMPLE
            );
        }

        // The three reads below (candidates, address detail, paginated live
        // cells) are independent and race a live-syncing tip. Bracket the check
        // so a mismatch can be classified as a real bug or as a straddled block.
        let tip_before = sampling_tip_from_stats(&fetch_network_stats(ctx)?)?;

        let mut findings = vec![];
        let mut skipped: Vec<String> = vec![];
        let mut checked = 0u64;
        let mut cells_scanned = 0usize;
        let mut tip_after = tip_before;

        for addr in &sampled_addresses {
            let outcome = verify_address_balance_sample(ctx, addr, cells_scanned)?;
            cells_scanned = cells_scanned
                .checked_add(outcome.cells_scanned)
                .ok_or_else(|| {
                    anyhow::anyhow!("address balance total scanned-cell count overflow")
                })?;

            if !outcome.details.is_empty() {
                // Re-read exactly once. If the second read agrees, the first one
                // straddled a block — not a bug.
                let retry = verify_address_balance_sample(ctx, addr, cells_scanned)?;
                cells_scanned =
                    cells_scanned
                        .checked_add(retry.cells_scanned)
                        .ok_or_else(|| {
                            anyhow::anyhow!("address balance total scanned-cell count overflow")
                        })?;

                if !retry.details.is_empty() {
                    tip_after = sampling_tip_from_stats(&fetch_network_stats(ctx)?)?;
                    if tip_after > tip_before {
                        // The chain moved under the check. A stored-vs-actual
                        // difference is expected here and proves nothing.
                        skipped.push(format!(
                            "lock_hash {}: {}",
                            &addr.lock_script_hash[..18],
                            retry.details.join("; ")
                        ));
                    } else {
                        // Tip held still across both reads: the difference is real.
                        findings.push(Finding {
                            entity: format!("lock_hash: {}", &addr.lock_script_hash[..18]),
                            details: retry.details,
                        });
                    }
                }
            }

            checked += 1;
            progress.inc(1);
        }

        let skip_note = if skipped.is_empty() {
            String::new()
        } else {
            format!(
                ", {} skipped (tip advanced {}→{} during the check): {}",
                skipped.len(),
                tip_before,
                tip_after,
                skipped.join(" | ")
            )
        };
        let detail = format!(
            "{} addresses verified from {} live cells{}",
            checked,
            format_number(cells_scanned as u64),
            skip_note
        );

        if findings.is_empty() {
            Ok(CheckResult::pass_with_detail(checked, detail))
        } else {
            Ok(CheckResult {
                detail: Some(detail),
                ..CheckResult::fail(checked, findings)
            })
        }
    }
}

/// S4: GET /charts/transaction-count → all values > 0, monotonically ordered dates.
pub struct ChartTxCountPositive;

impl Check for ChartTxCountPositive {
    fn name(&self) -> &'static str {
        "chart_tx_count_positive"
    }
    fn description(&self) -> &'static str {
        "Transaction count chart: values > 0, dates ordered"
    }
    fn tier(&self) -> CheckTier {
        CheckTier::Sampling
    }
    fn run(&self, ctx: &CheckContext, _progress: &ProgressReporter) -> anyhow::Result<CheckResult> {
        let chart: ChartWrapper<ChartDataPoint> = api_get(ctx, "charts/transaction-count")?;
        let mut findings = vec![];
        let mut prev_date = String::new();

        for point in &chart.data {
            let value: i64 = point.value.parse().map_err(|e| {
                anyhow::anyhow!(
                    "invalid transaction count chart value on {}: {:?}: {e}",
                    point.date,
                    point.value
                )
            })?;
            if value <= 0 {
                findings.push(Finding {
                    entity: point.date.clone(),
                    details: vec![format!(
                        "transaction count = {} (expected > 0)",
                        point.value
                    )],
                });
            }
            if !prev_date.is_empty() && point.date <= prev_date {
                findings.push(Finding {
                    entity: point.date.clone(),
                    details: vec![format!(
                        "date {} <= previous date {} (not monotonically ordered)",
                        point.date, prev_date,
                    )],
                });
            }
            prev_date = point.date.clone();
        }

        let checked = chart.data.len() as u64;
        if findings.is_empty() {
            Ok(CheckResult::pass_with_detail(
                checked,
                format!("{} data points", checked),
            ))
        } else {
            Ok(CheckResult::fail(checked, findings))
        }
    }
}

/// S5: GET /charts/cell-count → live + dead values present and consistent.
pub struct ChartCellCountConsistency;

impl Check for ChartCellCountConsistency {
    fn name(&self) -> &'static str {
        "chart_cell_count_consistency"
    }
    fn description(&self) -> &'static str {
        "Cell count chart: live + dead values present, dates ordered"
    }
    fn tier(&self) -> CheckTier {
        CheckTier::Sampling
    }
    fn run(&self, ctx: &CheckContext, _progress: &ProgressReporter) -> anyhow::Result<CheckResult> {
        let chart: ChartWrapper<StackedChartDataPoint> = api_get(ctx, "charts/cell-count")?;
        let mut findings = vec![];
        let mut prev_date = String::new();
        let mut prev_dead: Option<i128> = None;
        let mut prev_total: Option<i128> = None;

        for point in &chart.data {
            // Check dates are ordered
            if !prev_date.is_empty() && point.date <= prev_date {
                findings.push(Finding {
                    entity: point.date.clone(),
                    details: vec![format!(
                        "date not monotonically ordered (prev={})",
                        prev_date
                    )],
                });
            }
            prev_date = point.date.clone();

            // Check both series exist (API uses liveCells/deadCells keys)
            let has_live = point.values.contains_key("liveCells");
            let has_dead = point.values.contains_key("deadCells");
            if !has_live || !has_dead {
                findings.push(Finding {
                    entity: point.date.clone(),
                    details: vec![format!(
                        "missing series: liveCells={}, deadCells={}",
                        has_live, has_dead
                    )],
                });
                continue;
            }

            let live = point
                .values
                .get("liveCells")
                .and_then(|value| value.parse::<i128>().ok());
            let dead = point
                .values
                .get("deadCells")
                .and_then(|value| value.parse::<i128>().ok());
            let (Some(live), Some(dead)) = (live, dead) else {
                findings.push(Finding {
                    entity: point.date.clone(),
                    details: vec!["liveCells/deadCells must be exact integers".to_string()],
                });
                continue;
            };
            if live < 0 || dead < 0 {
                findings.push(Finding {
                    entity: point.date.clone(),
                    details: vec![format!(
                        "negative cell count: liveCells={}, deadCells={}",
                        live, dead
                    )],
                });
                continue;
            }
            let Some(total) = live.checked_add(dead) else {
                findings.push(Finding {
                    entity: point.date.clone(),
                    details: vec![format!(
                        "cell count overflow: liveCells={}, deadCells={}",
                        live, dead
                    )],
                });
                continue;
            };
            if let Some(previous) = prev_dead {
                if dead < previous {
                    findings.push(Finding {
                        entity: point.date.clone(),
                        details: vec![format!(
                            "cumulative deadCells decreased: previous={}, current={}",
                            previous, dead
                        )],
                    });
                }
            }
            if let Some(previous) = prev_total {
                if total < previous {
                    findings.push(Finding {
                        entity: point.date.clone(),
                        details: vec![format!(
                            "cumulative outputs (live+dead) decreased: previous={}, current={}",
                            previous, total
                        )],
                    });
                }
            }
            prev_dead = Some(dead);
            prev_total = Some(total);
        }

        let checked = chart.data.len() as u64;
        if findings.is_empty() {
            Ok(CheckResult::pass_with_detail(
                checked,
                format!("{} data points", checked),
            ))
        } else {
            Ok(CheckResult::fail(checked, findings))
        }
    }
}

/// S6: GET /charts/total-supply → values never decrease across dates.
pub struct ChartTotalSupplyMonotonic;

impl Check for ChartTotalSupplyMonotonic {
    fn name(&self) -> &'static str {
        "chart_total_supply_monotonic"
    }
    fn description(&self) -> &'static str {
        "Total supply chart: values never decrease"
    }
    fn tier(&self) -> CheckTier {
        CheckTier::Sampling
    }
    fn run(&self, ctx: &CheckContext, _progress: &ProgressReporter) -> anyhow::Result<CheckResult> {
        let chart: ChartWrapper<StackedChartDataPoint> = api_get(ctx, "charts/total-supply")?;
        let mut findings = vec![];
        let dates: Vec<_> = chart.data.iter().map(|point| point.date.as_str()).collect();
        validate_chart_dates(&dates, "total-supply", &mut findings);
        if chart.data.is_empty() {
            validate_empty_dao_chart_history(ctx, &mut findings)?;
        }
        let mut previous: Option<(String, i128)> = None;

        for point in &chart.data {
            let mut total = 0i128;
            let mut valid = true;
            for key in ["circulating", "nervosdao", "burnt"] {
                match point
                    .values
                    .get(key)
                    .and_then(|raw| parse_ckb_to_shannons(raw))
                {
                    Some(value) => match total.checked_add(value) {
                        Some(sum) => total = sum,
                        None => {
                            valid = false;
                            findings.push(Finding {
                                entity: point.date.clone(),
                                details: vec![format!("total-supply {key} sum overflow")],
                            });
                        }
                    },
                    None => {
                        valid = false;
                        findings.push(Finding {
                            entity: point.date.clone(),
                            details: vec![format!("missing or invalid total-supply {key} value")],
                        });
                    }
                }
            }
            if !valid {
                continue;
            }
            if let Some((previous_date, previous_total)) = &previous {
                if total < *previous_total {
                    findings.push(Finding {
                        entity: point.date.clone(),
                        details: vec![format!(
                            "total supply decreased: {} → {} shannons (prev={})",
                            previous_total, total, previous_date,
                        )],
                    });
                }
            }
            previous = Some((point.date.clone(), total));
        }

        let checked = chart.data.len() as u64;
        if findings.is_empty() {
            Ok(CheckResult::pass_with_detail(
                checked,
                format!("{} data points", checked),
            ))
        } else {
            Ok(CheckResult::fail(checked, findings))
        }
    }
}

/// S7: GET /charts/block-time-distribution → buckets/ratios sane and sum ≈ 100%.
pub struct ChartBlockTimeDistributionSane;

impl Check for ChartBlockTimeDistributionSane {
    fn name(&self) -> &'static str {
        "chart_block_time_distribution_sane"
    }
    fn description(&self) -> &'static str {
        "Block time distribution: buckets ordered, ratios in range, sum near 100%"
    }
    fn tier(&self) -> CheckTier {
        CheckTier::Sampling
    }
    fn run(&self, ctx: &CheckContext, _progress: &ProgressReporter) -> anyhow::Result<CheckResult> {
        let chart: ChartWrapper<ChartDataPoint> = api_get(ctx, "charts/block-time-distribution")?;
        let mut findings = vec![];
        let mut prev_bucket: Option<f64> = None;
        let mut ratio_sum = 0.0;
        let mut has_non_zero = false;

        if chart.data.is_empty() {
            findings.push(Finding {
                entity: "chart".to_string(),
                details: vec!["no block-time distribution data points returned".to_string()],
            });
        }

        for point in &chart.data {
            match point.date.parse::<f64>() {
                Ok(bucket) if bucket >= 0.0 => {
                    if let Some(prev) = prev_bucket {
                        if bucket < prev {
                            findings.push(Finding {
                                entity: point.date.clone(),
                                details: vec![format!(
                                    "bucket {} < previous bucket {} (not ordered)",
                                    bucket, prev
                                )],
                            });
                        }
                    }
                    prev_bucket = Some(bucket);
                }
                _ => findings.push(Finding {
                    entity: point.date.clone(),
                    details: vec![format!("invalid bucket '{}'", point.date)],
                }),
            }

            match point.value.parse::<f64>() {
                Ok(ratio) if (0.0..=100.0).contains(&ratio) => {
                    ratio_sum += ratio;
                    if ratio > 0.0 {
                        has_non_zero = true;
                    }
                }
                Ok(ratio) => findings.push(Finding {
                    entity: point.date.clone(),
                    details: vec![format!("ratio out of range [0,100]: {}", ratio)],
                }),
                Err(_) => findings.push(Finding {
                    entity: point.date.clone(),
                    details: vec![format!("invalid ratio '{}'", point.value)],
                }),
            }
        }

        if !chart.data.is_empty() && (ratio_sum - 100.0).abs() > 1.0 {
            findings.push(Finding {
                entity: "chart".to_string(),
                details: vec![format!(
                    "ratio sum = {:.3}% (expected around 100%)",
                    ratio_sum
                )],
            });
        }

        if !chart.data.is_empty() && !has_non_zero {
            findings.push(Finding {
                entity: "chart".to_string(),
                details: vec!["all bucket ratios are zero".to_string()],
            });
        }

        let checked = chart.data.len() as u64;
        if findings.is_empty() {
            Ok(CheckResult::pass_with_detail(
                checked,
                format!("{} data points, ratio sum {:.3}%", checked, ratio_sum),
            ))
        } else {
            Ok(CheckResult::fail(checked, findings))
        }
    }
}

/// S8: GET /charts/epoch-time-distribution → buckets/counts sane.
pub struct ChartEpochTimeDistributionSane;

impl Check for ChartEpochTimeDistributionSane {
    fn name(&self) -> &'static str {
        "chart_epoch_time_distribution_sane"
    }
    fn description(&self) -> &'static str {
        "Epoch time distribution: buckets ordered, counts non-negative"
    }
    fn tier(&self) -> CheckTier {
        CheckTier::Sampling
    }
    fn run(&self, ctx: &CheckContext, _progress: &ProgressReporter) -> anyhow::Result<CheckResult> {
        let chart: ChartWrapper<ChartDataPoint> = api_get(ctx, "charts/epoch-time-distribution")?;
        let mut findings = vec![];
        let mut prev_bucket: Option<f64> = None;
        let mut total_count: i64 = 0;

        if chart.data.is_empty() {
            findings.push(Finding {
                entity: "chart".to_string(),
                details: vec!["no epoch-time distribution data points returned".to_string()],
            });
        }

        for point in &chart.data {
            match point.date.parse::<f64>() {
                Ok(bucket) if bucket > 0.0 => {
                    if let Some(prev) = prev_bucket {
                        if bucket < prev {
                            findings.push(Finding {
                                entity: point.date.clone(),
                                details: vec![format!(
                                    "bucket {} < previous bucket {} (not ordered)",
                                    bucket, prev
                                )],
                            });
                        }
                    }
                    prev_bucket = Some(bucket);
                }
                _ => findings.push(Finding {
                    entity: point.date.clone(),
                    details: vec![format!("invalid epoch-time bucket '{}'", point.date)],
                }),
            }

            match point.value.parse::<i64>() {
                Ok(count) if count >= 0 => total_count += count,
                Ok(count) => findings.push(Finding {
                    entity: point.date.clone(),
                    details: vec![format!("negative epoch count {}", count)],
                }),
                Err(_) => findings.push(Finding {
                    entity: point.date.clone(),
                    details: vec![format!("invalid epoch count '{}'", point.value)],
                }),
            }
        }

        if !chart.data.is_empty() && total_count <= 0 {
            findings.push(Finding {
                entity: "chart".to_string(),
                details: vec!["epoch-time distribution total count is zero".to_string()],
            });
        }

        let checked = chart.data.len() as u64;
        if findings.is_empty() {
            Ok(CheckResult::pass_with_detail(
                checked,
                format!("{} data points, total epochs {}", checked, total_count),
            ))
        } else {
            Ok(CheckResult::fail(checked, findings))
        }
    }
}

/// S9: GET /charts/epoch-time-length → epoch sequence and values sane.
pub struct ChartEpochTimeLengthSane;

impl Check for ChartEpochTimeLengthSane {
    fn name(&self) -> &'static str {
        "chart_epoch_time_length_sane"
    }
    fn description(&self) -> &'static str {
        "Epoch time length chart: epoch numbers ordered, hours/blocks positive"
    }
    fn tier(&self) -> CheckTier {
        CheckTier::Sampling
    }
    fn run(&self, ctx: &CheckContext, _progress: &ProgressReporter) -> anyhow::Result<CheckResult> {
        let chart: ChartWrapper<ChartDataPoint> = api_get(ctx, "charts/epoch-time-length")?;
        let mut findings = vec![];
        let mut prev_epoch: Option<i64> = None;

        if chart.data.is_empty() {
            findings.push(Finding {
                entity: "chart".to_string(),
                details: vec!["no epoch-time-length data points returned".to_string()],
            });
        }

        for point in &chart.data {
            match point.date.parse::<i64>() {
                Ok(epoch) if epoch >= 0 => {
                    if let Some(prev) = prev_epoch {
                        if epoch <= prev {
                            findings.push(Finding {
                                entity: point.date.clone(),
                                details: vec![format!(
                                    "epoch {} <= previous epoch {} (not strictly increasing)",
                                    epoch, prev
                                )],
                            });
                        }
                    }
                    prev_epoch = Some(epoch);
                }
                _ => findings.push(Finding {
                    entity: point.date.clone(),
                    details: vec![format!("invalid epoch number '{}'", point.date)],
                }),
            }

            match point.value.parse::<f64>() {
                Ok(hours) if hours > 0.0 => {}
                Ok(hours) => findings.push(Finding {
                    entity: point.date.clone(),
                    details: vec![format!("epoch duration hours must be > 0, got {}", hours)],
                }),
                Err(_) => findings.push(Finding {
                    entity: point.date.clone(),
                    details: vec![format!("invalid epoch duration '{}'", point.value)],
                }),
            }

            match point.value2.as_ref().and_then(|v| v.parse::<i64>().ok()) {
                Some(blocks) if blocks > 0 => {}
                Some(blocks) => findings.push(Finding {
                    entity: point.date.clone(),
                    details: vec![format!("epoch blocks must be > 0, got {}", blocks)],
                }),
                None => findings.push(Finding {
                    entity: point.date.clone(),
                    details: vec!["missing or invalid value2 (epoch blocks)".to_string()],
                }),
            }
        }

        let checked = chart.data.len() as u64;
        if findings.is_empty() {
            Ok(CheckResult::pass_with_detail(
                checked,
                format!("{} data points", checked),
            ))
        } else {
            Ok(CheckResult::fail(checked, findings))
        }
    }
}

/// Upper sanity bound for a daily average block time, in seconds.
///
/// CKB targets ~8s. Real chains stall far above that: testnet's 2020-05-22 daily
/// average is 311.43s (pinned by
/// `average_block_time_accepts_historical_testnet_stall`), so the bound must
/// leave generous headroom above it. 1800s (30 min) does, while still catching
/// the regression the bound exists for — a milliseconds value reported as
/// seconds, which lands three orders of magnitude too high.
const MAX_AVERAGE_BLOCK_TIME_SECONDS: f64 = 1800.0;

/// S10: GET /charts/average-block-time → positive values within a sane bound.
pub struct ChartAverageBlockTimeSane;

impl Check for ChartAverageBlockTimeSane {
    fn name(&self) -> &'static str {
        "chart_average_block_time_sane"
    }
    fn description(&self) -> &'static str {
        "Average block time chart: positive values, dates ordered"
    }
    fn tier(&self) -> CheckTier {
        CheckTier::Sampling
    }
    fn run(&self, ctx: &CheckContext, _progress: &ProgressReporter) -> anyhow::Result<CheckResult> {
        let chart: ChartWrapper<ChartDataPoint> = api_get(ctx, "charts/average-block-time")?;
        let mut findings = vec![];
        let mut prev_date = String::new();

        if chart.data.is_empty() {
            findings.push(Finding {
                entity: "chart".to_string(),
                details: vec!["no average-block-time data points returned".to_string()],
            });
        }

        for point in &chart.data {
            if !prev_date.is_empty() && point.date <= prev_date {
                findings.push(Finding {
                    entity: point.date.clone(),
                    details: vec![format!(
                        "date {} <= previous date {} (not ordered)",
                        point.date, prev_date
                    )],
                });
            }
            prev_date = point.date.clone();

            match point.value.parse::<f64>() {
                Ok(seconds)
                    if seconds.is_finite()
                        && seconds > 0.0
                        && seconds <= MAX_AVERAGE_BLOCK_TIME_SECONDS => {}
                Ok(seconds) => findings.push(Finding {
                    entity: point.date.clone(),
                    details: vec![format!(
                        "average block time must be finite and in (0, {}]s: {}s",
                        MAX_AVERAGE_BLOCK_TIME_SECONDS, seconds
                    )],
                }),
                Err(_) => findings.push(Finding {
                    entity: point.date.clone(),
                    details: vec![format!("invalid average block time '{}'", point.value)],
                }),
            }
        }

        let checked = chart.data.len() as u64;
        if findings.is_empty() {
            Ok(CheckResult::pass_with_detail(
                checked,
                format!("{} data points", checked),
            ))
        } else {
            Ok(CheckResult::fail(checked, findings))
        }
    }
}

/// S11: GET /charts/miner-address-distribution → top list internally consistent.
pub struct ChartMinerDistributionConsistency;

impl Check for ChartMinerDistributionConsistency {
    fn name(&self) -> &'static str {
        "chart_miner_distribution_consistency"
    }
    fn description(&self) -> &'static str {
        "Miner distribution: lock hash, optional address, totals, and percentages sane"
    }
    fn tier(&self) -> CheckTier {
        CheckTier::Sampling
    }
    fn run(&self, ctx: &CheckContext, _progress: &ProgressReporter) -> anyhow::Result<CheckResult> {
        let chart: MinerDistributionResponse = api_get(ctx, "charts/miner-address-distribution")?;
        let mut findings = vec![];
        let mut sum_blocks = 0i64;
        let mut sum_percentage = 0.0f64;

        if chart.data.len() > 100 {
            findings.push(Finding {
                entity: "chart".to_string(),
                details: vec![format!(
                    "returned {} miners (expected at most 100)",
                    chart.data.len()
                )],
            });
        }
        if chart.total_blocks < 0 {
            findings.push(Finding {
                entity: "chart".to_string(),
                details: vec![format!("totalBlocks is negative: {}", chart.total_blocks)],
            });
        }
        if chart.total_blocks > 0 && chart.data.is_empty() {
            findings.push(Finding {
                entity: "chart".to_string(),
                details: vec!["totalBlocks > 0 but data is empty".to_string()],
            });
        }

        for miner in &chart.data {
            if !miner.miner_lock_hash.starts_with("0x") || miner.miner_lock_hash.len() != 66 {
                findings.push(Finding {
                    entity: miner.miner_lock_hash.clone(),
                    details: vec![format!(
                        "invalid miner lock hash format: '{}'",
                        miner.miner_lock_hash
                    )],
                });
            }
            if let Some(address) = &miner.address {
                if !address.starts_with("ckb1") && !address.starts_with("ckt1") {
                    findings.push(Finding {
                        entity: miner.miner_lock_hash.clone(),
                        details: vec![format!("invalid resolved miner address: '{}'", address)],
                    });
                }
            }

            if miner.blocks_mined < 0 {
                findings.push(Finding {
                    entity: miner.miner_lock_hash.clone(),
                    details: vec![format!("negative blocksMined: {}", miner.blocks_mined)],
                });
            } else {
                sum_blocks += miner.blocks_mined;
            }

            match miner.percentage.parse::<f64>() {
                Ok(pct) if (0.0..=100.0).contains(&pct) => sum_percentage += pct,
                Ok(pct) => findings.push(Finding {
                    entity: miner.miner_lock_hash.clone(),
                    details: vec![format!("percentage out of range [0,100]: {}", pct)],
                }),
                Err(_) => findings.push(Finding {
                    entity: miner.miner_lock_hash.clone(),
                    details: vec![format!("invalid percentage '{}'", miner.percentage)],
                }),
            }
        }

        if chart.total_blocks >= 0 && sum_blocks > chart.total_blocks {
            findings.push(Finding {
                entity: "chart".to_string(),
                details: vec![format!(
                    "sum(blocksMined)={} > totalBlocks={}",
                    sum_blocks, chart.total_blocks
                )],
            });
        }
        if sum_percentage > 100.1 {
            findings.push(Finding {
                entity: "chart".to_string(),
                details: vec![format!("sum(percentage)={:.4}% > 100%", sum_percentage)],
            });
        }

        let checked = chart.data.len() as u64;
        if findings.is_empty() {
            Ok(CheckResult::pass_with_detail(
                checked,
                format!(
                    "{} miners, sum(blocks)={}, sum(percentage)={:.4}%",
                    checked, sum_blocks, sum_percentage
                ),
            ))
        } else {
            Ok(CheckResult::fail(checked, findings))
        }
    }
}

/// S12: GET /charts/nominal-apc → deterministic sequence sanity.
pub struct ChartNominalApcSane;

impl Check for ChartNominalApcSane {
    fn name(&self) -> &'static str {
        "chart_nominal_apc_sane"
    }
    fn description(&self) -> &'static str {
        "Nominal APC chart: expected point count, 0.25y step, non-increasing values"
    }
    fn tier(&self) -> CheckTier {
        CheckTier::Sampling
    }
    fn run(&self, ctx: &CheckContext, _progress: &ProgressReporter) -> anyhow::Result<CheckResult> {
        let chart: ChartWrapper<ChartDataPoint> = api_get(ctx, "charts/nominal-apc")?;
        let mut findings = vec![];
        let mut prev_year: Option<f64> = None;
        let mut prev_value: Option<f64> = None;

        if chart.data.len() != 81 {
            findings.push(Finding {
                entity: "chart".to_string(),
                details: vec![format!(
                    "expected 81 data points (0..20y at 0.25y), got {}",
                    chart.data.len()
                )],
            });
        }

        for point in &chart.data {
            let year = match point.date.parse::<f64>() {
                Ok(y) if y >= 0.0 => y,
                _ => {
                    findings.push(Finding {
                        entity: point.date.clone(),
                        details: vec![format!("invalid year '{}'", point.date)],
                    });
                    continue;
                }
            };
            let value = match point.value.parse::<f64>() {
                Ok(v) if (0.0..=10.0).contains(&v) => v,
                Ok(v) => {
                    findings.push(Finding {
                        entity: point.date.clone(),
                        details: vec![format!("APC out of range [0,10]: {}", v)],
                    });
                    continue;
                }
                Err(_) => {
                    findings.push(Finding {
                        entity: point.date.clone(),
                        details: vec![format!("invalid APC '{}'", point.value)],
                    });
                    continue;
                }
            };

            if let Some(prev) = prev_year {
                let step = year - prev;
                if (step - 0.25).abs() > 0.0001 {
                    findings.push(Finding {
                        entity: point.date.clone(),
                        details: vec![format!("year step is {}, expected 0.25", step)],
                    });
                }
            }
            if let Some(prev) = prev_value {
                if value > prev + 1e-9 {
                    findings.push(Finding {
                        entity: point.date.clone(),
                        details: vec![format!("APC increased: {:.6} -> {:.6}", prev, value)],
                    });
                }
            }

            prev_year = Some(year);
            prev_value = Some(value);
        }

        let checked = chart.data.len() as u64;
        if findings.is_empty() {
            Ok(CheckResult::pass_with_detail(
                checked,
                format!("{} data points", checked),
            ))
        } else {
            Ok(CheckResult::fail(checked, findings))
        }
    }
}

/// S13: GET /charts/inflation-rate → realized nominal/real relationship and date sanity.
pub struct ChartInflationRateSane;

impl Check for ChartInflationRateSane {
    fn name(&self) -> &'static str {
        "chart_inflation_rate_sane"
    }
    fn description(&self) -> &'static str {
        "Inflation chart: chronological daily dates and nominal >= real"
    }
    fn tier(&self) -> CheckTier {
        CheckTier::Sampling
    }
    fn run(&self, ctx: &CheckContext, _progress: &ProgressReporter) -> anyhow::Result<CheckResult> {
        let chart: ChartWrapper<ChartDataPoint> = api_get(ctx, "charts/inflation-rate")?;
        let mut findings = vec![];
        let mut prev_date: Option<chrono::NaiveDate> = None;

        for point in &chart.data {
            let date = match chrono::NaiveDate::parse_from_str(&point.date, "%Y-%m-%d") {
                Ok(date) => date,
                _ => {
                    findings.push(Finding {
                        entity: point.date.clone(),
                        details: vec![format!("invalid YYYY-MM-DD date '{}'", point.date)],
                    });
                    continue;
                }
            };
            let nominal = match point.value.parse::<f64>() {
                Ok(v) if v >= 0.0 => v,
                Ok(v) => {
                    findings.push(Finding {
                        entity: point.date.clone(),
                        details: vec![format!("nominal inflation is negative: {}", v)],
                    });
                    continue;
                }
                Err(_) => {
                    findings.push(Finding {
                        entity: point.date.clone(),
                        details: vec![format!("invalid nominal inflation '{}'", point.value)],
                    });
                    continue;
                }
            };
            let real = match point.value2.as_ref().and_then(|v| v.parse::<f64>().ok()) {
                Some(v) if v >= 0.0 => v,
                Some(v) => {
                    findings.push(Finding {
                        entity: point.date.clone(),
                        details: vec![format!("real inflation is negative: {}", v)],
                    });
                    continue;
                }
                None => {
                    findings.push(Finding {
                        entity: point.date.clone(),
                        details: vec!["missing or invalid real inflation value2".to_string()],
                    });
                    continue;
                }
            };

            if real > nominal + 1e-9 {
                findings.push(Finding {
                    entity: point.date.clone(),
                    details: vec![format!(
                        "real inflation {:.6} > nominal inflation {:.6}",
                        real, nominal
                    )],
                });
            }
            if let Some(prev) = prev_date {
                let step = date.signed_duration_since(prev).num_days();
                if step != 1 {
                    findings.push(Finding {
                        entity: point.date.clone(),
                        details: vec![format!("date step is {} days, expected 1", step)],
                    });
                }
            }

            prev_date = Some(date);
        }

        let checked = chart.data.len() as u64;
        if findings.is_empty() {
            Ok(CheckResult::pass_with_detail(
                checked,
                format!("{} data points", checked),
            ))
        } else {
            Ok(CheckResult::fail(checked, findings))
        }
    }
}

const HODL_WAVE_BANDS: [&str; 8] = [
    "24h", "1d1w", "1w1m", "1m3m", "3m6m", "6m1y", "1y3y", "gt3y",
];

fn validate_required_holder_count(
    values: &std::collections::HashMap<String, String>,
) -> Option<String> {
    match values.get("holderCount") {
        Some(raw) if raw.parse::<u64>().is_ok() => None,
        Some(raw) => Some(format!("invalid holderCount '{}'", raw)),
        None => Some("missing holderCount".to_string()),
    }
}

fn parse_non_negative_i128(raw: &str) -> Option<i128> {
    let value = raw.trim().parse::<i128>().ok()?;
    (value >= 0).then_some(value)
}

fn parse_ckb_to_shannons(raw: &str) -> Option<i128> {
    let s = raw.trim();
    if s.is_empty() || s.starts_with('-') {
        return None;
    }

    let mut parts = s.split('.');
    let whole = parts.next()?;
    let frac = parts.next();
    if parts.next().is_some() {
        return None;
    }

    let whole_part = whole.parse::<i128>().ok()?;
    let frac_part = match frac {
        Some(f) => {
            if f.len() > 8 || !f.chars().all(|c| c.is_ascii_digit()) {
                return None;
            }
            let mut padded = f.to_string();
            while padded.len() < 8 {
                padded.push('0');
            }
            padded.parse::<i128>().ok()?
        }
        None => 0,
    };

    whole_part
        .checked_mul(SHANNONS_PER_CKB)?
        .checked_add(frac_part)
}

/// S14: GET /charts/hodl-wave → required series present and percentage sum sane.
pub struct ChartHodlWaveConsistency;

impl Check for ChartHodlWaveConsistency {
    fn name(&self) -> &'static str {
        "chart_hodl_wave_consistency"
    }
    fn description(&self) -> &'static str {
        "HODL wave chart: required bands + holderCount present, percentages sum to ~100%"
    }
    fn tier(&self) -> CheckTier {
        CheckTier::Sampling
    }
    fn run(&self, ctx: &CheckContext, _progress: &ProgressReporter) -> anyhow::Result<CheckResult> {
        let chart: ChartWrapper<StackedChartDataPoint> = api_get(ctx, "charts/hodl-wave")?;
        let mut findings = vec![];
        let mut prev_date = String::new();

        if chart.data.is_empty() {
            findings.push(Finding {
                entity: "chart".to_string(),
                details: vec!["no hodl-wave data points returned".to_string()],
            });
        }

        for point in &chart.data {
            if !prev_date.is_empty() && point.date <= prev_date {
                findings.push(Finding {
                    entity: point.date.clone(),
                    details: vec![format!(
                        "date {} <= previous date {} (not ordered)",
                        point.date, prev_date
                    )],
                });
            }
            prev_date = point.date.clone();

            let mut band_sum = 0.0f64;
            for band in HODL_WAVE_BANDS {
                match point.values.get(band).and_then(|v| v.parse::<f64>().ok()) {
                    Some(v) if v >= 0.0 => band_sum += v,
                    Some(v) => findings.push(Finding {
                        entity: point.date.clone(),
                        details: vec![format!("band {} has negative value {}", band, v)],
                    }),
                    None => findings.push(Finding {
                        entity: point.date.clone(),
                        details: vec![format!("missing or invalid band {}", band)],
                    }),
                }
            }

            if let Some(detail) = validate_required_holder_count(&point.values) {
                findings.push(Finding {
                    entity: point.date.clone(),
                    details: vec![detail],
                });
            }

            if band_sum > 0.0 && (band_sum - 100.0).abs() > 0.3 {
                findings.push(Finding {
                    entity: point.date.clone(),
                    details: vec![format!(
                        "band percentage sum is {:.4}% (expected around 100%)",
                        band_sum
                    )],
                });
            }
        }

        let checked = chart.data.len() as u64;
        if findings.is_empty() {
            Ok(CheckResult::pass_with_detail(
                checked,
                format!("{} data points", checked),
            ))
        } else {
            Ok(CheckResult::fail(checked, findings))
        }
    }
}

/// S15: GET /charts/knowledge-size and /charts/common-knowledge-composition are exactly aligned.
pub struct ChartKnowledgeCompositionExact;

impl Check for ChartKnowledgeCompositionExact {
    fn name(&self) -> &'static str {
        "chart_knowledge_composition_exact"
    }
    fn description(&self) -> &'static str {
        "Knowledge size equals exact sum of composition series (shannons)"
    }
    fn tier(&self) -> CheckTier {
        CheckTier::Sampling
    }
    fn run(&self, ctx: &CheckContext, _progress: &ProgressReporter) -> anyhow::Result<CheckResult> {
        let knowledge: ChartWrapper<ChartDataPoint> = api_get(ctx, "charts/knowledge-size")?;
        let composition: ChartWrapper<StackedChartDataPoint> =
            api_get(ctx, "charts/common-knowledge-composition")?;

        let mut findings = vec![];
        let mut knowledge_by_date = std::collections::HashMap::<String, i128>::new();
        for point in &knowledge.data {
            match parse_ckb_to_shannons(&point.value) {
                Some(v) => {
                    knowledge_by_date.insert(point.date.clone(), v);
                }
                None => findings.push(Finding {
                    entity: point.date.clone(),
                    details: vec![format!("invalid knowledge-size value '{}'", point.value)],
                }),
            }
        }

        let mut checked = 0u64;
        for point in &composition.data {
            let mut sum = 0i128;
            for key in ["transfer", "dao", "udt", "nftSpore", "otherContracts"] {
                let raw = point.values.get(key).map(|v| v.as_str()).unwrap_or("");
                match parse_ckb_to_shannons(raw) {
                    Some(v) => sum += v,
                    None => findings.push(Finding {
                        entity: point.date.clone(),
                        details: vec![format!(
                            "missing or invalid composition series '{}' value '{}'",
                            key, raw
                        )],
                    }),
                }
            }

            if let Some(expected) = knowledge_by_date.get(&point.date) {
                checked += 1;
                if sum != *expected {
                    findings.push(Finding {
                        entity: point.date.clone(),
                        details: vec![format!(
                            "composition sum {} shannons != knowledge size {} shannons",
                            sum, expected
                        )],
                    });
                }
            }
        }

        if checked == 0 {
            return Ok(CheckResult::pass_with_detail(
                0,
                "no overlapping dates between knowledge and composition charts".to_string(),
            ));
        }

        if findings.is_empty() {
            Ok(CheckResult::pass_with_detail(
                checked,
                format!("{} overlapping date points", checked),
            ))
        } else {
            Ok(CheckResult::fail(checked, findings))
        }
    }
}

/// The API's indexed chain-time calendar selects completed days. An HTTP-only
/// verifier cannot re-derive that boundary from its wall clock: local sync can
/// lag, and fixture chains can run ahead. This checks the chart's date syntax
/// and ordering, leaving completion to the API's explicit calendar dependency.
fn validate_chart_dates(dates: &[&str], chart: &str, findings: &mut Vec<Finding>) {
    let mut previous: Option<&str> = None;
    for &date in dates {
        let canonical = chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d")
            .is_ok_and(|parsed| parsed.format("%Y-%m-%d").to_string() == date);
        if !canonical {
            findings.push(Finding {
                entity: date.to_string(),
                details: vec![format!("{chart} contains an invalid date {date}")],
            });
        }
        if let Some(before) = previous.filter(|before| date <= *before) {
            findings.push(Finding {
                entity: date.to_string(),
                details: vec![format!(
                    "{chart} dates are not strictly increasing after {before}"
                )],
            });
        }
        previous = Some(date);
    }
}

fn validate_empty_dao_chart_history(
    ctx: &CheckContext,
    findings: &mut Vec<Finding>,
) -> anyhow::Result<()> {
    let deposit: ChartWrapper<ChartDataPoint> = api_get(ctx, "dao/charts/total-deposit")?;
    if !deposit.data.is_empty() {
        findings.push(Finding {
            entity: "chart-date-coverage".to_string(),
            details: vec![format!(
                "DAO deposit chart has {} completed days but supply/secondary history is empty",
                deposit.data.len()
            )],
        });
    }
    Ok(())
}

struct BurntGapEvidence {
    gaps: Vec<(String, i128)>,
    findings: Vec<Finding>,
}

fn exact_burnt_gaps(
    ctx: &CheckContext,
    supply: &[StackedChartDataPoint],
    secondary: &[StackedChartDataPoint],
) -> anyhow::Result<BurntGapEvidence> {
    // The public endpoints emit indexed-calendar-complete days; this verifier
    // checks date syntax/order and exact same-date amounts without a wall-clock
    // claim about completion.
    let supply_dates: Vec<_> = supply.iter().map(|point| point.date.as_str()).collect();
    let secondary_dates: Vec<_> = secondary.iter().map(|point| point.date.as_str()).collect();
    let mut findings = vec![];
    validate_chart_dates(&supply_dates, "total-supply", &mut findings);
    validate_chart_dates(&secondary_dates, "secondary-issuance", &mut findings);
    if supply_dates != secondary_dates {
        findings.push(Finding {
            entity: "chart-date-coverage".to_string(),
            details: vec![format!(
                "total-supply dates {:?} differ from secondary-issuance dates {:?}",
                supply_dates, secondary_dates
            )],
        });
    }
    if supply.is_empty() && secondary.is_empty() {
        validate_empty_dao_chart_history(ctx, &mut findings)?;
    }
    let mut gaps = vec![];
    for (left, right) in supply.iter().zip(secondary) {
        if left.date != right.date {
            continue;
        }
        let total = left
            .values
            .get("burnt")
            .and_then(|raw| parse_ckb_to_shannons(raw));
        let secondary_burnt = right
            .values
            .get("burnt")
            .and_then(|raw| parse_ckb_to_shannons(raw));
        match (total, secondary_burnt) {
            (Some(total), Some(secondary_burnt)) => {
                gaps.push((left.date.clone(), total - secondary_burnt))
            }
            (None, _) => findings.push(Finding {
                entity: left.date.clone(),
                details: vec!["missing or invalid total-supply burnt CKB value".to_string()],
            }),
            (_, None) => findings.push(Finding {
                entity: left.date.clone(),
                details: vec!["missing or invalid secondary-issuance burnt CKB value".to_string()],
            }),
        }
    }
    Ok(BurntGapEvidence { gaps, findings })
}

/// S16: Each complete-day secondary issuance partition conserves the
/// independently materialized protocol secondary total at that date. This
/// checks partition identity, not independent historical category attribution.
pub struct SecondaryIssuancePartitionConservation;

impl Check for SecondaryIssuancePartitionConservation {
    fn name(&self) -> &'static str {
        "secondary_issuance_partition_conservation"
    }
    fn description(&self) -> &'static str {
        "Each completed-day secondary issuance partition equals its exact protocol total (shannons)"
    }
    fn tier(&self) -> CheckTier {
        CheckTier::Sampling
    }
    fn run(&self, ctx: &CheckContext, _progress: &ProgressReporter) -> anyhow::Result<CheckResult> {
        let secondary: ChartWrapper<SecondaryIssuancePoint> =
            api_get(ctx, "charts/secondary-issuance")?;
        let supply: ChartWrapper<StackedChartDataPoint> = api_get(ctx, "charts/total-supply")?;
        let mut findings = vec![];
        let secondary_dates: Vec<_> = secondary
            .data
            .iter()
            .map(|point| point.date.as_str())
            .collect();
        let supply_dates: Vec<_> = supply
            .data
            .iter()
            .map(|point| point.date.as_str())
            .collect();
        validate_chart_dates(&secondary_dates, "secondary-issuance", &mut findings);
        validate_chart_dates(&supply_dates, "total-supply", &mut findings);
        if secondary_dates != supply_dates {
            findings.push(Finding {
                entity: "chart-date-coverage".to_string(),
                details: vec![format!(
                    "secondary-issuance dates {:?} differ from total-supply dates {:?}",
                    secondary_dates, supply_dates
                )],
            });
        }
        if secondary.data.is_empty() {
            validate_empty_dao_chart_history(ctx, &mut findings)?;
        }
        for point in &secondary.data {
            let mut components = [0i128; 3];
            if point.values.len() != 3 {
                findings.push(Finding {
                    entity: point.date.clone(),
                    details: vec![format!("secondary-issuance requires exactly compensation, mining and burnt; got {:?}", point.values.keys().collect::<Vec<_>>())],
                });
            }
            let mut valid = true;
            for (index, key) in ["compensation", "mining", "burnt"].iter().enumerate() {
                match point
                    .values
                    .get(*key)
                    .and_then(|raw| parse_ckb_to_shannons(raw))
                {
                    Some(value) => components[index] = value,
                    None => {
                        valid = false;
                        findings.push(Finding {
                            entity: point.date.clone(),
                            details: vec![format!(
                                "missing or invalid secondary-issuance {key} CKB value"
                            )],
                        });
                    }
                }
            }
            let protocol = point
                .protocol_total_shannons
                .as_deref()
                .and_then(parse_non_negative_i128);
            let phase1 = point
                .phase1_compensation_shannons
                .as_deref()
                .and_then(parse_non_negative_i128);
            if protocol.is_none() {
                findings.push(Finding {
                    entity: point.date.clone(),
                    details: vec!["missing or invalid protocolTotalShannons".to_string()],
                });
            }
            if phase1.is_none() {
                findings.push(Finding {
                    entity: point.date.clone(),
                    details: vec!["missing or invalid phase1CompensationShannons".to_string()],
                });
            }
            if let Some(phase1) = phase1 {
                if valid && phase1 > components[0] {
                    findings.push(Finding { entity: point.date.clone(), details: vec![format!("phase1CompensationShannons {phase1} exceeds cumulative DAO compensation {}", components[0])] });
                }
            }
            if valid {
                let sum = components
                    .into_iter()
                    .try_fold(0i128, |sum, value| sum.checked_add(value));
                match (sum, protocol) {
                    (Some(sum), Some(protocol)) if sum != protocol => findings.push(Finding {
                        entity: point.date.clone(),
                        details: vec![format!("completed-day secondary partition {sum} shannons != protocolTotalShannons {protocol} shannons")],
                    }),
                    (None, _) => findings.push(Finding { entity: point.date.clone(), details: vec!["secondary-issuance partition overflow".to_string()] }),
                    _ => {}
                }
            }
        }
        let checked = secondary.data.len() as u64;
        if findings.is_empty() {
            Ok(CheckResult::pass_with_detail(checked, format!("{checked} exact completed-day partitions; indexed calendar is the chart's day-boundary dependency")))
        } else {
            Ok(CheckResult::fail(checked, findings))
        }
    }
}

/// S17: Every completed date has an exact burnt gap equal to the persisted
/// active-network genesis burnt.
/// The indexed monotone chain-time calendar defines the day boundaries; this
/// chart-level check does not independently prove the calendar itself.
pub struct BurntSupplyGenesisInvariant;

impl Check for BurntSupplyGenesisInvariant {
    fn name(&self) -> &'static str {
        "burnt_supply_genesis_invariant"
    }
    fn description(&self) -> &'static str {
        "Every completed-day total-supply.burnt minus secondary-issuance.burnt equals exact genesis burnt (shannons)"
    }
    fn tier(&self) -> CheckTier {
        CheckTier::Sampling
    }
    fn run(&self, ctx: &CheckContext, _progress: &ProgressReporter) -> anyhow::Result<CheckResult> {
        let supply: ChartWrapper<StackedChartDataPoint> = api_get(ctx, "charts/total-supply")?;
        let secondary: ChartWrapper<StackedChartDataPoint> =
            api_get(ctx, "charts/secondary-issuance")?;
        let BurntGapEvidence { gaps, mut findings } =
            exact_burnt_gaps(ctx, &supply.data, &secondary.data)?;
        if !gaps.is_empty() {
            let expected = genesis_baseline_burnt(ctx)?;
            for (date, gap) in &gaps {
                if *gap != expected {
                    findings.push(Finding {
                        entity: date.clone(),
                        details: vec![format!("burnt gap={gap} shannons, persisted genesis baseline={expected} shannons")],
                    });
                }
            }
        }
        let checked = gaps.len() as u64;
        if findings.is_empty() {
            Ok(CheckResult::pass_with_detail(
                checked,
                if checked == 0 {
                    "no completed DAO snapshot day yet; supply, secondary and deposit charts are empty".to_string()
                } else {
                    format!("{checked} exact completed-day burnt gaps; indexed calendar is the chart's day-boundary dependency")
                },
            ))
        } else {
            Ok(CheckResult::fail(checked, findings))
        }
    }
}

/// S18: N random blocks: compare API vs CKB RPC (hash, txCount). Skipped without --rpc-url.
pub struct RpcBlockSpotCheck;

impl Check for RpcBlockSpotCheck {
    fn name(&self) -> &'static str {
        "rpc_block_spot_check"
    }

    /// Reads the sample count: at 0 it has nothing to select.
    fn requires_sampling(&self) -> bool {
        true
    }
    fn description(&self) -> &'static str {
        "Compare block data against CKB RPC node"
    }
    fn tier(&self) -> CheckTier {
        CheckTier::Sampling
    }
    fn requires_rpc(&self) -> bool {
        true
    }
    fn estimated_total(&self, ctx: &CheckContext) -> Option<u64> {
        Some(ctx.sample_count as u64)
    }
    fn run(&self, ctx: &CheckContext, progress: &ProgressReporter) -> anyhow::Result<CheckResult> {
        let rpc_url = ctx
            .rpc_url
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("RPC URL not available"))?;

        let stats = fetch_network_stats(ctx)?;
        let tip = sampling_tip_from_stats(&stats)?;
        if tip == 0 {
            return Ok(CheckResult::pass(0));
        }

        let mut sampler = LcgSampler::new(ctx.seed.wrapping_add(3));
        let blocks = sampler.sample_range(ctx.sample_count, tip + 1);
        let mut findings = vec![];

        for block_num in &blocks {
            // Fetch from our API
            let our_block: BlockResponse = api_get(ctx, &format!("blocks/{}", block_num))?;

            // One RPC fetch covers every compared field.
            let rpc_block = rpc_get_block(ctx, rpc_url, *block_num)?;

            let mut details = vec![];
            if let Some(rpc_block) = rpc_block {
                compare_block_vs_rpc(ctx, &our_block, &rpc_block, *block_num, &mut details);
            }

            if !details.is_empty() {
                findings.push(Finding {
                    entity: format!("block #{}", block_num),
                    details,
                });
            }
            progress.inc(1);
        }

        let checked = blocks.len() as u64;
        if findings.is_empty() {
            Ok(CheckResult::pass(checked))
        } else {
            Ok(CheckResult::fail(checked, findings))
        }
    }
}

#[derive(Clone)]
struct TokenActivitySample {
    lock_hash: String,
    tx_hash: String,
    block_number: i64,
    token_type_hash: String,
    /// Signed net delta as (magnitude, negative) — a ±u128 net does not fit i128.
    delta: (u128, bool),
}

/// S19: sampled token activity entries must match token transfers for tx/token/address net delta.
pub struct TokenActivityTransferBidirectional;

impl Check for TokenActivityTransferBidirectional {
    fn name(&self) -> &'static str {
        "token_activity_transfer_bidirectional"
    }

    /// Reads the sample count: at 0 it has nothing to select.
    fn requires_sampling(&self) -> bool {
        true
    }
    fn description(&self) -> &'static str {
        "Token activity net delta matches token transfers (address/tx/token)"
    }
    fn tier(&self) -> CheckTier {
        CheckTier::Sampling
    }
    fn estimated_total(&self, ctx: &CheckContext) -> Option<u64> {
        Some(ctx.sample_count as u64)
    }
    fn run(&self, ctx: &CheckContext, progress: &ProgressReporter) -> anyhow::Result<CheckResult> {
        let top_addresses: Vec<AddressCandidateResponse> = api_get(ctx, "addresses/top")?;
        if top_addresses.is_empty() || ctx.sample_count == 0 {
            return Ok(CheckResult::pass_with_detail(
                0,
                "no addresses available for token activity sampling".to_string(),
            ));
        }

        let mut candidates = Vec::<TokenActivitySample>::new();
        for addr in top_addresses.iter().take(TOKEN_ACTIVITY_ADDRESS_LIMIT) {
            let mut cursor: Option<String> = None;
            let lock_key = normalize_hex_key(&addr.lock_script_hash);

            for _ in 0..TOKEN_ACTIVITY_MAX_PAGES_PER_ADDRESS {
                let path = match cursor.as_ref() {
                    Some(c) => format!(
                        "addresses/{}/activities?filter=token&limit={}&cursor={}",
                        addr.lock_script_hash, TOKEN_ACTIVITY_PAGE_LIMIT, c
                    ),
                    None => format!(
                        "addresses/{}/activities?filter=token&limit={}",
                        addr.lock_script_hash, TOKEN_ACTIVITY_PAGE_LIMIT
                    ),
                };
                let page: CursorPage<AddressActivityRecord> = api_get(ctx, &path)?;

                for activity in &page.data {
                    for (token_type_hash, delta) in extract_activity_token_deltas(activity)? {
                        candidates.push(TokenActivitySample {
                            lock_hash: lock_key.clone(),
                            tx_hash: normalize_hex_key(&activity.tx_hash),
                            block_number: activity.block_number,
                            token_type_hash,
                            delta,
                        });
                    }
                }

                cursor = page.next_cursor;
                if cursor.is_none() {
                    break;
                }
            }

            if candidates.len() >= ctx.sample_count {
                break;
            }
        }

        if candidates.is_empty() {
            return Ok(CheckResult::pass_with_detail(
                0,
                "no token activity entries found in sampled addresses".to_string(),
            ));
        }

        let target = ctx.sample_count.min(candidates.len());
        let samples = if target == candidates.len() {
            candidates
        } else {
            let mut sampler = LcgSampler::new(ctx.seed.wrapping_add(19));
            let idxs = sampler.sample_range(target, candidates.len() as u64);
            idxs.into_iter()
                .map(|i| candidates[i as usize].clone())
                .collect()
        };

        let mut min_block_by_token = std::collections::HashMap::<String, i64>::new();
        for sample in &samples {
            min_block_by_token
                .entry(sample.token_type_hash.clone())
                .and_modify(|current| *current = (*current).min(sample.block_number))
                .or_insert(sample.block_number);
        }

        let mut transfer_delta_lookup =
            std::collections::HashMap::<(String, String, String), (u128, u128)>::new();

        for (token_type_hash, min_block) in min_block_by_token {
            let mut cursor: Option<String> = None;
            let mut seen_cursors = std::collections::HashSet::<String>::new();
            loop {
                let path = match cursor.as_ref() {
                    Some(c) => format!(
                        "tokens/0x{}/transfers?limit={}&cursor={}",
                        token_type_hash, TOKEN_TRANSFER_PAGE_LIMIT, c
                    ),
                    None => format!(
                        "tokens/0x{}/transfers?limit={}",
                        token_type_hash, TOKEN_TRANSFER_PAGE_LIMIT
                    ),
                };
                let page: CursorPage<TokenTransferApiRecord> = api_get(ctx, &path)?;

                let mut reached_min_block = false;
                for transfer in &page.data {
                    apply_transfer_delta_to_lookup(
                        &mut transfer_delta_lookup,
                        &token_type_hash,
                        transfer,
                    )?;
                    if transfer.block_number < min_block {
                        reached_min_block = true;
                    }
                }

                if reached_min_block {
                    break;
                }

                let Some(next_cursor) = page.next_cursor else {
                    break;
                };
                if !seen_cursors.insert(next_cursor.clone()) {
                    anyhow::bail!(
                        "repeated transfer cursor while scanning token 0x{}: {}",
                        token_type_hash,
                        next_cursor
                    );
                }
                cursor = Some(next_cursor);
            }
        }

        let mut findings = vec![];
        let mut checked = 0u64;

        for sample in &samples {
            let (recv, sent) = transfer_delta_lookup
                .get(&(
                    sample.token_type_hash.clone(),
                    sample.tx_hash.clone(),
                    sample.lock_hash.clone(),
                ))
                .copied()
                .unwrap_or((0, 0));
            // Net-difference (no i128 wrap); normalize a zero net to (0, false).
            let (amag, aneg) = if recv >= sent {
                (recv - sent, false)
            } else {
                (sent - recv, true)
            };
            let actual = (amag, amag != 0 && aneg);
            if actual != sample.delta {
                findings.push(Finding {
                    entity: format!(
                        "tx=0x{} lock=0x{} token=0x{}",
                        sample.tx_hash, sample.lock_hash, sample.token_type_hash
                    ),
                    details: vec![format!(
                        "activity delta={} but transfer delta={}",
                        signed_decimal_string(sample.delta.0, sample.delta.1),
                        signed_decimal_string(actual.0, actual.1)
                    )],
                });
            }
            checked += 1;
            progress.inc(1);
        }

        if findings.is_empty() {
            Ok(CheckResult::pass_with_detail(
                checked,
                format!("{} sampled token activity entries", checked),
            ))
        } else {
            Ok(CheckResult::fail(checked, findings))
        }
    }
}

fn fetch_owner_spore_map(
    ctx: &CheckContext,
    owner_lock_hash: &str,
) -> anyhow::Result<std::collections::HashMap<String, Option<String>>> {
    let mut map = std::collections::HashMap::<String, Option<String>>::new();
    let mut cursor: Option<String> = None;
    let mut seen_cursors = std::collections::HashSet::<String>::new();

    for _ in 0..SPORE_OWNER_MAX_PAGES {
        let path = match cursor.as_ref() {
            Some(c) => format!(
                "spore/owner/{}?limit={}&cursor={}",
                owner_lock_hash, SPORE_OWNER_PAGE_LIMIT, c
            ),
            None => format!(
                "spore/owner/{}?limit={}",
                owner_lock_hash, SPORE_OWNER_PAGE_LIMIT
            ),
        };
        let page: CursorPage<SporeApiRecord> = api_get(ctx, &path)?;

        for spore in &page.data {
            map.insert(
                normalize_hex_key(&spore.spore_id),
                spore.cluster_id.as_ref().map(|v| normalize_hex_key(v)),
            );
        }

        let Some(next_cursor) = page.next_cursor else {
            break;
        };
        if !seen_cursors.insert(next_cursor.clone()) {
            anyhow::bail!(
                "repeated owner cursor while scanning spore owner {}: {}",
                owner_lock_hash,
                next_cursor
            );
        }
        cursor = Some(next_cursor);
    }

    Ok(map)
}

/// S20: sampled live spores in clusters must be discoverable via owner endpoint (roundtrip).
pub struct SporeOwnerRoundtrip;

impl Check for SporeOwnerRoundtrip {
    fn name(&self) -> &'static str {
        "spore_owner_roundtrip"
    }

    /// Reads the sample count: at 0 it has nothing to select.
    fn requires_sampling(&self) -> bool {
        true
    }
    fn description(&self) -> &'static str {
        "Spore cluster items roundtrip through owner endpoint"
    }
    fn tier(&self) -> CheckTier {
        CheckTier::Sampling
    }
    fn estimated_total(&self, ctx: &CheckContext) -> Option<u64> {
        let cap = SPORE_CLUSTER_SAMPLE_MAX.saturating_mul(SPORE_PER_CLUSTER_SAMPLE);
        Some(ctx.sample_count.min(cap) as u64)
    }
    fn run(&self, ctx: &CheckContext, progress: &ProgressReporter) -> anyhow::Result<CheckResult> {
        let clusters: CursorPage<SporeClusterApiRecord> = api_get(
            ctx,
            &format!("spore/clusters?limit={}", SPORE_CLUSTER_LIST_LIMIT),
        )?;
        if clusters.data.is_empty() || ctx.sample_count == 0 {
            return Ok(CheckResult::pass_with_detail(
                0,
                "no spore clusters available for sampling".to_string(),
            ));
        }

        let cluster_indices = sample_indices_with_cap(
            ctx.seed.wrapping_add(20),
            clusters.data.len(),
            ctx.sample_count,
            SPORE_CLUSTER_SAMPLE_MAX,
        );

        let mut owner_cache = std::collections::HashMap::<
            String,
            std::collections::HashMap<String, Option<String>>,
        >::new();
        let mut findings = vec![];
        let mut checked = 0u64;

        for cluster_idx in cluster_indices {
            let cluster_id_raw = &clusters.data[cluster_idx].cluster_id;
            let cluster_id = normalize_hex_key(cluster_id_raw);
            let spores_path = format!(
                "spore/clusters/{}/spores?limit={}",
                cluster_id_raw, SPORE_CLUSTER_SPORE_PAGE_LIMIT
            );
            let spores_page: CursorPage<SporeApiRecord> = api_get(ctx, &spores_path)?;
            if spores_page.data.is_empty() {
                continue;
            }

            let spore_indices = sample_indices_with_cap(
                ctx.seed.wrapping_add(21) ^ (cluster_idx as u64),
                spores_page.data.len(),
                SPORE_PER_CLUSTER_SAMPLE,
                SPORE_PER_CLUSTER_SAMPLE,
            );

            for spore_idx in spore_indices {
                let spore = &spores_page.data[spore_idx];
                let spore_id = normalize_hex_key(&spore.spore_id);
                if !spore.is_live {
                    findings.push(Finding {
                        entity: format!("spore=0x{}", spore_id),
                        details: vec![
                            "cluster spores endpoint returned non-live spore entry".to_string()
                        ],
                    });
                    checked += 1;
                    progress.inc(1);
                    continue;
                }

                match spore.cluster_id.as_ref().map(|v| normalize_hex_key(v)) {
                    Some(ref cid) if cid == &cluster_id => {}
                    other => {
                        findings.push(Finding {
                            entity: format!("spore=0x{}", spore_id),
                            details: vec![format!(
                                "cluster mismatch: expected 0x{}, endpoint returned {:?}",
                                cluster_id, other
                            )],
                        });
                    }
                }

                let owner_key = normalize_hex_key(&spore.owner_lock_hash);
                if owner_key.is_empty() {
                    findings.push(Finding {
                        entity: format!("spore=0x{}", spore_id),
                        details: vec!["spore owner lock hash is empty".to_string()],
                    });
                    checked += 1;
                    progress.inc(1);
                    continue;
                }

                let owner_spores = if let Some(cached) = owner_cache.get(&owner_key) {
                    cached
                } else {
                    let fetched = fetch_owner_spore_map(ctx, &spore.owner_lock_hash)?;
                    owner_cache.insert(owner_key.clone(), fetched);
                    owner_cache
                        .get(&owner_key)
                        .ok_or_else(|| anyhow::anyhow!("owner cache insert failed"))?
                };

                match owner_spores.get(&spore_id) {
                    None => findings.push(Finding {
                        entity: format!(
                            "spore=0x{} owner=0x{} cluster=0x{}",
                            spore_id, owner_key, cluster_id
                        ),
                        details: vec!["owner endpoint missing sampled cluster spore".to_string()],
                    }),
                    Some(owner_cluster) => {
                        if owner_cluster.as_deref() != Some(cluster_id.as_str()) {
                            findings.push(Finding {
                                entity: format!(
                                    "spore=0x{} owner=0x{} cluster=0x{}",
                                    spore_id, owner_key, cluster_id
                                ),
                                details: vec![format!(
                                    "owner endpoint cluster mismatch: {:?}",
                                    owner_cluster
                                )],
                            });
                        }
                    }
                }

                checked += 1;
                progress.inc(1);
            }
        }

        if checked == 0 {
            return Ok(CheckResult::pass_with_detail(
                0,
                "no live spores sampled from selected clusters".to_string(),
            ));
        }

        if findings.is_empty() {
            Ok(CheckResult::pass_with_detail(
                checked,
                format!("{} sampled spores roundtripped", checked),
            ))
        } else {
            Ok(CheckResult::fail(checked, findings))
        }
    }
}

/// S21: Object asset list/detail/items totals must stay internally consistent.
pub struct ObjectAssetCollectionConsistency;

impl Check for ObjectAssetCollectionConsistency {
    fn name(&self) -> &'static str {
        "object_asset_collection_consistency"
    }

    /// Reads the sample count: at 0 it has nothing to select.
    fn requires_sampling(&self) -> bool {
        true
    }
    fn description(&self) -> &'static str {
        "Object list/detail/items totals are consistent"
    }
    fn tier(&self) -> CheckTier {
        CheckTier::Sampling
    }
    fn estimated_total(&self, ctx: &CheckContext) -> Option<u64> {
        Some(ctx.sample_count.min(OBJECT_COLLECTION_SAMPLE_MAX) as u64)
    }
    fn run(&self, ctx: &CheckContext, progress: &ProgressReporter) -> anyhow::Result<CheckResult> {
        let assets: CursorPageWithTotal<ObjectAssetListApiRecord> = api_get(
            ctx,
            &format!("assets?type=object&limit={}", OBJECT_ASSET_LIST_LIMIT),
        )?;
        if assets.data.is_empty() || ctx.sample_count == 0 {
            return Ok(CheckResult::pass_with_detail(
                0,
                "no object collections available for sampling".to_string(),
            ));
        }

        let sample_indices = sample_indices_with_cap(
            ctx.seed.wrapping_add(30),
            assets.data.len(),
            ctx.sample_count,
            OBJECT_COLLECTION_SAMPLE_MAX,
        );

        let mut findings = vec![];
        let mut checked = 0u64;

        for idx in sample_indices {
            let object = &assets.data[idx];
            let asset = &object.asset;

            if asset.asset_type != "object" {
                findings.push(Finding {
                    entity: format!("asset={}", asset.id),
                    details: vec![format!(
                        "unexpected asset type '{}' in object asset list",
                        asset.asset_type
                    )],
                });
                checked += 1;
                progress.inc(1);
                continue;
            }

            if asset.standard.eq_ignore_ascii_case("spore") {
                progress.inc(1);
                continue;
            }

            let total_count = object.total_supply.parse::<i64>().map_err(|error| {
                anyhow::anyhow!(
                    "asset={} invalid list totalSupply '{}': {}",
                    asset.id,
                    object.total_supply,
                    error
                )
            })?;
            anyhow::ensure!(
                total_count >= 0,
                "asset={} invalid negative list totalSupply={}",
                asset.id,
                total_count
            );
            anyhow::ensure!(
                object.live_count >= 0,
                "asset={} invalid negative list liveCount={}",
                asset.id,
                object.live_count
            );

            let collection_detail: NftCollectionDetailApiRecord =
                api_get(ctx, &format!("assets/objects/{}", asset.id))?;
            let detail_id = normalize_hex_key(&collection_detail.collection_id);
            let asset_id = normalize_hex_key(&asset.id);

            if detail_id != asset_id {
                findings.push(Finding {
                    entity: format!("asset={}", asset.id),
                    details: vec![format!(
                        "detail collectionId mismatch: detail=0x{}, list=0x{}",
                        detail_id, asset_id
                    )],
                });
            }
            if collection_detail.total_count != total_count {
                findings.push(Finding {
                    entity: format!("asset={}", asset.id),
                    details: vec![format!(
                        "detail totalCount={} != list totalSupply={}",
                        collection_detail.total_count, total_count
                    )],
                });
            }
            if collection_detail.live_count != object.live_count {
                findings.push(Finding {
                    entity: format!("asset={}", asset.id),
                    details: vec![format!(
                        "detail liveCount={} != list liveCount={}",
                        collection_detail.live_count, object.live_count
                    )],
                });
            }
            if collection_detail.holders_count != asset.holders_count {
                findings.push(Finding {
                    entity: format!("asset={}", asset.id),
                    details: vec![format!(
                        "detail holdersCount={} != list holdersCount={}",
                        collection_detail.holders_count, asset.holders_count
                    )],
                });
            }

            let items: CursorPageWithTotal<serde_json::Value> =
                api_get(ctx, &format!("assets/objects/{}/items?limit=1", asset.id))?;
            match items.total {
                Some(total) if total == collection_detail.total_count => {}
                Some(total) => findings.push(Finding {
                    entity: format!("asset={}", asset.id),
                    details: vec![format!(
                        "items total={} != detail totalCount={}",
                        total, collection_detail.total_count
                    )],
                }),
                None => findings.push(Finding {
                    entity: format!("asset={}", asset.id),
                    details: vec!["items response missing total".to_string()],
                }),
            }

            checked += 1;
            progress.inc(1);
        }

        if checked == 0 {
            return Ok(CheckResult::pass_with_detail(
                0,
                "no non-spore object collections sampled".to_string(),
            ));
        }

        if findings.is_empty() {
            Ok(CheckResult::pass_with_detail(
                checked,
                format!("{} sampled object collections", checked),
            ))
        } else {
            Ok(CheckResult::fail(checked, findings))
        }
    }
}

/// S22: Top token/object asset holders must align with address-page invariants.
pub struct AssetTopHoldersAddressConsistency;

impl Check for AssetTopHoldersAddressConsistency {
    fn name(&self) -> &'static str {
        "asset_top_holders_address_consistency"
    }
    fn description(&self) -> &'static str {
        "Top asset holders align with address, activity-page, cell, and transaction invariants"
    }
    fn tier(&self) -> CheckTier {
        CheckTier::Sampling
    }
    fn estimated_total(&self, _ctx: &CheckContext) -> Option<u64> {
        Some((TOP_ASSET_LIMIT * TOP_HOLDER_LIMIT * 2) as u64)
    }
    fn run(&self, ctx: &CheckContext, progress: &ProgressReporter) -> anyhow::Result<CheckResult> {
        let token_assets: CursorPageWithTotal<AssetListApiRecord> =
            api_get(ctx, &format!("assets?type=token&limit={}", TOP_ASSET_LIMIT))?;
        let object_assets: CursorPageWithTotal<AssetListApiRecord> = api_get(
            ctx,
            &format!("assets?type=object&limit={}", TOP_ASSET_LIMIT),
        )?;

        if token_assets.data.is_empty() && object_assets.data.is_empty() {
            return Ok(CheckResult::pass_with_detail(
                0,
                "no token/object assets available for holder consistency checks".to_string(),
            ));
        }

        let mut findings = vec![];
        let mut checked = 0u64;
        let mut address_cache = std::collections::HashMap::<String, AddressCountSnapshot>::new();

        for asset in token_assets.data.iter().take(TOP_ASSET_LIMIT) {
            let mut asset_details = vec![];
            if asset.asset_type != "token" {
                asset_details.push(format!(
                    "unexpected asset type '{}' in token list",
                    asset.asset_type
                ));
            }

            let detail: TokenDetailApiRecord = api_get(ctx, &format!("tokens/{}", asset.id))?;
            let detail_id = normalize_hex_key(&detail.type_script_hash);
            let asset_id = normalize_hex_key(&asset.id);
            if detail_id != asset_id {
                asset_details.push(format!(
                    "detail typeScriptHash mismatch: detail=0x{}, list=0x{}",
                    detail_id, asset_id
                ));
            }
            if detail.holders_count != asset.holders_count {
                asset_details.push(format!(
                    "detail holdersCount={} != list holdersCount={}",
                    detail.holders_count, asset.holders_count
                ));
            }

            let holders: CursorPageWithTotal<TokenHolderApiRecord> = api_get(
                ctx,
                &format!("tokens/{}/holders?limit={}", asset.id, TOP_HOLDER_LIMIT),
            )?;
            match holders.total {
                Some(total) if total == detail.holders_count => {}
                Some(total) => asset_details.push(format!(
                    "holders total={} != detail holdersCount={}",
                    total, detail.holders_count
                )),
                None => asset_details.push("holders response missing total".to_string()),
            }

            if !asset_details.is_empty() {
                findings.push(Finding {
                    entity: format!("token={}", asset.id),
                    details: asset_details,
                });
            }

            for holder in holders.data.iter().take(TOP_HOLDER_LIMIT) {
                let mut holder_details = vec![];
                if normalize_hex_key(&holder.lock_script_hash).is_empty() {
                    holder_details.push("holder lock script hash is empty".to_string());
                } else {
                    let snapshot =
                        load_address_snapshot(ctx, &mut address_cache, &holder.lock_script_hash)?;
                    holder_details.extend(address_count_mismatch_details(
                        &holder.lock_script_hash,
                        &snapshot,
                    ));
                }
                holder_details.extend(token_holder_balance_mismatch_details(
                    ctx,
                    &asset.id,
                    &holder.lock_script_hash,
                    &holder.balance,
                )?);

                if !holder_details.is_empty() {
                    findings.push(Finding {
                        entity: format!("token={} holder={}", asset.id, holder.lock_script_hash),
                        details: holder_details,
                    });
                }
                checked += 1;
                progress.inc(1);
            }
        }

        for asset in object_assets.data.iter().take(TOP_ASSET_LIMIT) {
            if asset.asset_type != "object" {
                findings.push(Finding {
                    entity: format!("object={} standard={}", asset.id, asset.standard),
                    details: vec![format!(
                        "unexpected asset type '{}' in object list",
                        asset.asset_type
                    )],
                });
                checked += 1;
                progress.inc(1);
                continue;
            }
            let mut asset_details = vec![];

            let (detail_id, detail_holders_count, holders): (
                String,
                i64,
                CursorPageWithTotal<NftHolderApiRecord>,
            ) = if asset.standard.eq_ignore_ascii_case("spore") {
                let detail: SporeClusterDetailApiRecord =
                    api_get(ctx, &format!("spore/clusters/{}", asset.id))?;
                let holders_page: CursorPageWithTotal<NftHolderApiRecord> = api_get(
                    ctx,
                    &format!(
                        "spore/clusters/{}/holders?limit={}",
                        asset.id, TOP_HOLDER_LIMIT
                    ),
                )?;
                (detail.cluster_id, detail.holders_count, holders_page)
            } else {
                let detail: NftCollectionDetailApiRecord =
                    api_get(ctx, &format!("assets/objects/{}", asset.id))?;
                let holders_page: CursorPageWithTotal<NftHolderApiRecord> = api_get(
                    ctx,
                    &format!(
                        "assets/objects/{}/holders?limit={}",
                        asset.id, TOP_HOLDER_LIMIT
                    ),
                )?;
                (detail.collection_id, detail.holders_count, holders_page)
            };

            let detail_id_key = normalize_hex_key(&detail_id);
            let asset_id_key = normalize_hex_key(&asset.id);
            if detail_id_key != asset_id_key {
                asset_details.push(format!(
                    "detail id mismatch: detail=0x{}, list=0x{}",
                    detail_id_key, asset_id_key
                ));
            }
            if detail_holders_count != asset.holders_count {
                asset_details.push(format!(
                    "detail holdersCount={} != list holdersCount={}",
                    detail_holders_count, asset.holders_count
                ));
            }
            match holders.total {
                Some(total) if total == detail_holders_count => {}
                Some(total) => asset_details.push(format!(
                    "holders total={} != detail holdersCount={}",
                    total, detail_holders_count
                )),
                None => asset_details.push("holders response missing total".to_string()),
            }

            if !asset_details.is_empty() {
                findings.push(Finding {
                    entity: format!("object={} standard={}", asset.id, asset.standard),
                    details: asset_details,
                });
            }

            for holder in holders.data.iter().take(TOP_HOLDER_LIMIT) {
                let mut holder_details = vec![];
                if holder.item_count <= 0 {
                    holder_details.push(format!(
                        "holder itemCount={} expected > 0",
                        holder.item_count
                    ));
                }
                if normalize_hex_key(&holder.lock_script_hash).is_empty() {
                    holder_details.push("holder lock script hash is empty".to_string());
                } else {
                    let snapshot =
                        load_address_snapshot(ctx, &mut address_cache, &holder.lock_script_hash)?;
                    holder_details.extend(address_count_mismatch_details(
                        &holder.lock_script_hash,
                        &snapshot,
                    ));
                }

                if !holder_details.is_empty() {
                    findings.push(Finding {
                        entity: format!(
                            "object={} holder={} standard={}",
                            asset.id, holder.lock_script_hash, asset.standard
                        ),
                        details: holder_details,
                    });
                }
                checked += 1;
                progress.inc(1);
            }
        }

        if checked == 0 && findings.is_empty() {
            return Ok(CheckResult::pass_with_detail(
                0,
                "no holders returned by top token/object assets".to_string(),
            ));
        }

        if findings.is_empty() {
            Ok(CheckResult::pass_with_detail(
                checked,
                format!(
                    "{} holder address checks across top {} token + top {} object assets",
                    checked, TOP_ASSET_LIMIT, TOP_ASSET_LIMIT
                ),
            ))
        } else {
            Ok(CheckResult::fail(checked, findings))
        }
    }
}

/// S23: Identity collection holder counts match aggregate, and top holders'
/// identity counts do not exceed their live cell counts (each identity = 1 cell).
pub struct IdentityCollectionHolderConsistency;

impl Check for IdentityCollectionHolderConsistency {
    fn name(&self) -> &'static str {
        "identity_collection_holder_consistency"
    }
    fn description(&self) -> &'static str {
        "Identity holder counts match aggregate and do not exceed live cell counts"
    }
    fn tier(&self) -> CheckTier {
        CheckTier::Sampling
    }
    fn estimated_total(&self, _ctx: &CheckContext) -> Option<u64> {
        Some(2) // dotbit + did:ckb
    }
    fn run(&self, ctx: &CheckContext, progress: &ProgressReporter) -> anyhow::Result<CheckResult> {
        let identity_collections = ["dotbit", "did_ckb"];
        let mut findings = vec![];
        let mut checked = 0u64;

        for collection_slug in &identity_collections {
            // 1. Fetch the collection aggregate.
            let detail: NftCollectionDetailApiRecord =
                match api_get(ctx, &format!("assets/identities/{}", collection_slug)) {
                    Ok(d) => d,
                    Err(_) => {
                        progress.inc(1);
                        continue; // Collection may not exist yet
                    }
                };

            // 2. Fetch top holders (already sorted by count desc).
            let holders: CursorPageWithTotal<NftHolderApiRecord> = api_get(
                ctx,
                &format!(
                    "assets/identities/{}/holders?limit={}",
                    collection_slug, IDENTITY_HOLDER_SPOT_CHECK_LIMIT
                ),
            )?;

            // 3. Check holders_count consistency with aggregate.
            match holders.total {
                Some(total) if total == detail.holders_count => {}
                Some(total) => findings.push(Finding {
                    entity: format!("identity_collection={}", collection_slug),
                    details: vec![format!(
                        "holders total={} != aggregate holders_count={}",
                        total, detail.holders_count
                    )],
                }),
                None => findings.push(Finding {
                    entity: format!("identity_collection={}", collection_slug),
                    details: vec!["holders response missing total".to_string()],
                }),
            }

            // 4. For each top holder, verify identity count <= live cell count.
            // Each identity (dotbit/.bit account, did:ckb) is stored in a cell
            // locked by the owner's lock script, so the address must have at
            // least as many live cells as identity items.
            for holder in holders.data.iter().take(IDENTITY_HOLDER_SPOT_CHECK_LIMIT) {
                if holder.item_count <= 0 {
                    findings.push(Finding {
                        entity: format!(
                            "identity_collection={} holder={}",
                            collection_slug, holder.lock_script_hash
                        ),
                        details: vec![format!(
                            "holder item_count={} expected > 0",
                            holder.item_count
                        )],
                    });
                    continue;
                }

                let addr: AddressDetailApiRecord =
                    match api_get(ctx, &format!("addresses/{}", holder.lock_script_hash)) {
                        Ok(a) => a,
                        Err(_) => continue, // address may not be resolvable
                    };

                if holder.item_count > addr.live_cells_count {
                    findings.push(Finding {
                        entity: format!(
                            "identity_collection={} holder={}",
                            collection_slug, holder.lock_script_hash
                        ),
                        details: vec![format!(
                            "identity count={} > live_cells_count={} \
                             (each identity is a cell, indicates over-counting)",
                            holder.item_count, addr.live_cells_count
                        )],
                    });
                }
            }

            checked += 1;
            progress.inc(1);
        }

        if checked == 0 {
            return Ok(CheckResult::pass_with_detail(
                0,
                "no identity collections found".to_string(),
            ));
        }

        if findings.is_empty() {
            Ok(CheckResult::pass_with_detail(
                checked,
                format!("{} identity collections validated", checked),
            ))
        } else {
            Ok(CheckResult::fail(checked, findings))
        }
    }
}

// ---------------------------------------------------------------------------
// CKB RPC helpers (blocking, used only by S18)
// ---------------------------------------------------------------------------

#[derive(serde::Serialize)]
struct JsonRpcRequest {
    jsonrpc: &'static str,
    id: u64,
    method: &'static str,
    params: Vec<serde_json::Value>,
}

#[derive(serde::Deserialize)]
struct JsonRpcResponse {
    result: Option<serde_json::Value>,
}

fn rpc_call(
    ctx: &CheckContext,
    rpc_url: &str,
    method: &'static str,
    params: Vec<serde_json::Value>,
) -> anyhow::Result<Option<serde_json::Value>> {
    let req = JsonRpcRequest {
        jsonrpc: "2.0",
        id: 1,
        method,
        params,
    };
    let resp: JsonRpcResponse = ctx.http.post(rpc_url).json(&req).send()?.json()?;
    Ok(resp.result)
}

fn rpc_get_block(
    ctx: &CheckContext,
    rpc_url: &str,
    block_num: u64,
) -> anyhow::Result<Option<serde_json::Value>> {
    let hex_num = format!("0x{:x}", block_num);
    rpc_call(
        ctx,
        rpc_url,
        "get_block_by_number",
        vec![serde_json::Value::String(hex_num)],
    )
}

/// Compare a block response against the node's `get_block_by_number` JSON:
/// hash, tx/proposal/uncle counts, compact_target, exact difficulty, and the
/// miner address rendered from the cellbase witness lock. Fields the RPC JSON
/// does not carry are skipped (mock servers may return partial blocks).
fn compare_block_vs_rpc(
    ctx: &CheckContext,
    ours: &BlockResponse,
    rpc_block: &serde_json::Value,
    block_num: u64,
    details: &mut Vec<String>,
) {
    if let Some(rpc_hash) = rpc_block
        .get("header")
        .and_then(|h| h.get("hash"))
        .and_then(|h| h.as_str())
    {
        if ours.hash != rpc_hash {
            details.push(format!(
                "hash mismatch: ours={}, rpc={}",
                &ours.hash[..ours.hash.len().min(18)],
                &rpc_hash[..rpc_hash.len().min(18)],
            ));
        }
    }

    if let Some(txs) = rpc_block.get("transactions").and_then(|t| t.as_array()) {
        let rpc_tc = txs.len() as i32;
        if ours.transactions_count != rpc_tc {
            details.push(format!(
                "tx_count: ours={}, rpc={}",
                ours.transactions_count, rpc_tc,
            ));
        }
    }

    if let Some(proposals) = rpc_block.get("proposals").and_then(|p| p.as_array()) {
        let rpc_pc = proposals.len() as i32;
        if ours.proposals_count != rpc_pc {
            details.push(format!(
                "proposals_count: ours={}, rpc={}",
                ours.proposals_count, rpc_pc,
            ));
        }
    }

    if let Some(uncles) = rpc_block.get("uncles").and_then(|u| u.as_array()) {
        let rpc_uc = uncles.len() as i32;
        if ours.uncles_count != rpc_uc {
            details.push(format!(
                "uncles_count: ours={}, rpc={}",
                ours.uncles_count, rpc_uc,
            ));
        }
    }

    if let Some(rpc_compact_hex) = rpc_block
        .get("header")
        .and_then(|h| h.get("compact_target"))
        .and_then(|c| c.as_str())
    {
        if let Ok(rpc_compact) = u32::from_str_radix(rpc_compact_hex.trim_start_matches("0x"), 16) {
            let ours_compact = ours.compact_target.trim_start_matches("0x").to_string();
            if u32::from_str_radix(&ours_compact, 16) != Ok(rpc_compact) {
                details.push(format!(
                    "compact_target: ours={}, rpc={}",
                    ours.compact_target, rpc_compact_hex,
                ));
            }
            let rpc_difficulty =
                ckb_types::utilities::compact_to_difficulty(rpc_compact).to_string();
            if ours.difficulty != rpc_difficulty {
                details.push(format!(
                    "difficulty: ours={}, rpc={}",
                    ours.difficulty, rpc_difficulty,
                ));
            }
        }
    }

    // Miner address: rendered from the cellbase witness lock (the block's own
    // miner per RFC-0022). Skipped when the API omits it (direct-read mode
    // disabled) and for the unmined genesis block.
    if block_num > 0 {
        if let Some(ours_miner) = ours.miner_address.as_deref() {
            let rpc_witness = rpc_block
                .get("transactions")
                .and_then(|t| t.as_array())
                .and_then(|txs| txs.first())
                .and_then(|cellbase| cellbase.get("witnesses"))
                .and_then(|w| w.as_array())
                .and_then(|w| w.first())
                .and_then(|w| w.as_str());
            if let Some(witness_hex) = rpc_witness {
                if let Some(expected) = miner_address_from_witness(witness_hex, ctx.network) {
                    if ours_miner != expected {
                        details.push(format!(
                            "miner_address: ours={}, rpc_witness={}",
                            ours_miner, expected,
                        ));
                    }
                }
            }
        }
    }
}

/// Render the miner address from a cellbase first-witness hex string.
fn miner_address_from_witness(witness_hex: &str, network: &str) -> Option<String> {
    use ckb_types::prelude::*;
    let bytes = hex::decode(witness_hex.trim_start_matches("0x")).ok()?;
    let reader = ckb_types::packed::CellbaseWitnessReader::from_slice(&bytes).ok()?;
    let lock = reader.to_entity().lock();
    let code_hash = lock.code_hash().raw_data().to_vec();
    let hash_type = i16::from(lock.hash_type().as_bytes()[0]);
    let args = lock.args().raw_data().to_vec();
    ckbadger_common::script_to_address(&code_hash, hash_type, &args, network).ok()
}

// ============================================
// Registration
// ============================================

/// Return all API-based checks (fast + sampling).
/// S24: every DAO deposit is listed under its own status, and no status listing
/// carries an outpoint the deposit table does not.
///
/// `dao_by_status_block` keys embed a *mutable* field, so every 0→1→2
/// transition has to delete the old index row and write the new one in the same
/// atomic batch. A leaked row is invisible until someone pages onto it — and
/// then it fails that request outright — so this is a full-table cross-check
/// rather than a sample: coverage cannot depend on which pages a user happens
/// to open.
///
/// Exact on live data. The chain advances during the walk and DAO status only
/// moves forward, so walking the unfiltered listing first and the status
/// listings in ascending order makes a concurrent transition unambiguous: the
/// deposit turns up in a listing at or after its recorded status, never before
/// it. A reorg can move status backwards, so one is detected and reported
/// instead of being folded into findings.
pub struct DaoStatusIndexMatchesDeposits;

const DAO_DEPOSITS_PAGE_LIMIT: usize = 100;
/// Runaway guard: mainnet holds ~92k deposits (~920 pages). Reaching this means
/// the cursor is not advancing, which is itself a defect — so it errors instead
/// of silently truncating the walk.
const DAO_DEPOSITS_MAX_PAGES: usize = 50_000;

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct DaoDepositRecord {
    tx_hash: String,
    output_index: i32,
    status: String,
    deposit_block_number: i64,
}

fn dao_status_label(status: i16) -> &'static str {
    match status {
        0 => "deposited",
        1 => "withdrawing",
        2 => "withdrawn",
        _ => "unknown",
    }
}

fn dao_status_rank(label: &str) -> Option<i16> {
    match label {
        "deposited" => Some(0),
        "withdrawing" => Some(1),
        "withdrawn" => Some(2),
        _ => None,
    }
}

fn dao_outpoint_key(tx_hash: &str, output_index: i32) -> String {
    format!("{}:{}", tx_hash.to_lowercase(), output_index)
}

/// Walk `/dao/deposits` (one status, or every deposit) to its last page.
///
/// Generic over the row type so a check decodes exactly the fields it reads.
pub(super) fn walk_dao_deposits<T: serde::de::DeserializeOwned>(
    ctx: &CheckContext,
    progress: &ProgressReporter,
    status: Option<i16>,
) -> anyhow::Result<Vec<T>> {
    let status_query = match status {
        Some(status) => format!("status={status}&"),
        None => String::new(),
    };
    let mut rows: Vec<T> = Vec::new();
    let mut cursor: Option<String> = None;

    for _ in 0..DAO_DEPOSITS_MAX_PAGES {
        let path = match cursor.as_deref() {
            Some(cursor) => format!(
                "dao/deposits?{status_query}limit={DAO_DEPOSITS_PAGE_LIMIT}&cursor={cursor}"
            ),
            None => format!("dao/deposits?{status_query}limit={DAO_DEPOSITS_PAGE_LIMIT}"),
        };
        let page: CursorPage<T> = api_get(ctx, &path)?;
        progress.inc(page.data.len() as u64);
        rows.extend(page.data);
        match page.next_cursor {
            Some(next) => cursor = Some(next),
            None => return Ok(rows),
        }
    }

    anyhow::bail!(
        "dao/deposits walk did not terminate within {} pages (status={:?}, rows collected={})",
        DAO_DEPOSITS_MAX_PAGES,
        status,
        rows.len()
    )
}

/// One row observed in a status listing.
#[derive(Clone, Copy)]
struct DaoIndexRow {
    status: i16,
    deposit_block: i64,
}

/// What the walks observed, in the form the verdict is derived from.
struct DaoStatusIndexObservation {
    /// outpoint -> (status rank reported by the unfiltered listing, deposit block)
    recorded: HashMap<String, (i16, i64)>,
    /// outpoint -> every status listing row it appeared in, in ascending order
    indexed: HashMap<String, Vec<DaoIndexRow>>,
    /// Deposit block of the newest deposit at the moment the walk started.
    /// Anything above it entered the table mid-check.
    newest_deposit_block: i64,
}

/// Absorbed observations that are legitimate on a live chain, not findings.
#[derive(Default, PartialEq, Eq, Debug)]
struct DaoStatusIndexAbsorbed {
    /// Deposits that moved to a later status while the check was running.
    transitions: u64,
    /// Deposits created after the unfiltered walk read its newest page.
    created_mid_check: u64,
}

/// Classify the observation. Pure so the live-chain rules are testable.
fn evaluate_dao_status_index(
    observed: &DaoStatusIndexObservation,
) -> (Vec<Finding>, DaoStatusIndexAbsorbed) {
    let mut findings = Vec::new();
    let mut absorbed = DaoStatusIndexAbsorbed::default();

    for (outpoint, (recorded_rank, deposit_block)) in &observed.recorded {
        match observed.indexed.get(outpoint) {
            None => findings.push(Finding {
                entity: outpoint.clone(),
                details: vec![format!(
                    "deposit appears in no status listing: status={}, deposit_block={}",
                    dao_status_label(*recorded_rank),
                    deposit_block
                )],
            }),
            Some(rows) => {
                for row in rows {
                    match row.status.cmp(recorded_rank) {
                        std::cmp::Ordering::Equal => {}
                        // Status only moves forward, so a later listing means the
                        // deposit transitioned while this check was running.
                        std::cmp::Ordering::Greater => absorbed.transitions += 1,
                        std::cmp::Ordering::Less => findings.push(Finding {
                            entity: outpoint.clone(),
                            details: vec![format!(
                                "status listing is behind the deposit: indexed under {}, deposit reports {}",
                                dao_status_label(row.status),
                                dao_status_label(*recorded_rank)
                            )],
                        }),
                    }
                }
            }
        }
    }

    for (outpoint, rows) in &observed.indexed {
        if observed.recorded.contains_key(outpoint) {
            continue;
        }
        // A deposit created after the unfiltered walk read its newest page is
        // expected to be missing from it.
        if rows
            .iter()
            .all(|row| row.deposit_block > observed.newest_deposit_block)
        {
            absorbed.created_mid_check += 1;
            continue;
        }
        findings.push(Finding {
            entity: outpoint.clone(),
            details: vec![format!(
                "status listing {} has an outpoint the deposit listing does not",
                rows.iter()
                    .map(|row| dao_status_label(row.status))
                    .collect::<Vec<_>>()
                    .join(", ")
            )],
        });
    }

    (findings, absorbed)
}

impl Check for DaoStatusIndexMatchesDeposits {
    fn name(&self) -> &'static str {
        "dao_status_index_matches_deposits"
    }

    fn description(&self) -> &'static str {
        "Every DAO deposit is listed under its own status and no status listing is stale"
    }

    fn tier(&self) -> CheckTier {
        CheckTier::Sampling
    }

    fn run(&self, ctx: &CheckContext, progress: &ProgressReporter) -> anyhow::Result<CheckResult> {
        let tip_height = fetch_network_stats(ctx)?.latest_block;
        let tip_before: BlockResponse = api_get(ctx, &format!("blocks/{tip_height}"))?;

        progress.set_message("dao/deposits (all)");
        let deposits = walk_dao_deposits::<DaoDepositRecord>(ctx, progress, None)?;

        // The unfiltered listing is ordered by deposit block DESC.
        let newest_deposit_block = deposits
            .first()
            .map(|record| record.deposit_block_number)
            .unwrap_or(i64::MIN);

        let mut recorded: HashMap<String, (i16, i64)> = HashMap::with_capacity(deposits.len());
        let mut findings: Vec<Finding> = Vec::new();
        for record in &deposits {
            let outpoint = dao_outpoint_key(&record.tx_hash, record.output_index);
            let Some(rank) = dao_status_rank(&record.status) else {
                findings.push(Finding {
                    entity: outpoint,
                    details: vec![format!("unknown deposit status '{}'", record.status)],
                });
                continue;
            };
            if recorded
                .insert(outpoint.clone(), (rank, record.deposit_block_number))
                .is_some()
            {
                findings.push(Finding {
                    entity: outpoint,
                    details: vec!["deposit listed twice by the unfiltered listing".to_string()],
                });
            }
        }

        let mut indexed: HashMap<String, Vec<DaoIndexRow>> = HashMap::with_capacity(recorded.len());
        let mut index_rows = 0u64;
        for status in [0i16, 1, 2] {
            progress.set_message(&format!("dao/deposits?status={status}"));
            for record in walk_dao_deposits::<DaoDepositRecord>(ctx, progress, Some(status))? {
                index_rows += 1;
                let outpoint = dao_outpoint_key(&record.tx_hash, record.output_index);
                let expected = dao_status_label(status);
                if record.status != expected {
                    findings.push(Finding {
                        entity: outpoint.clone(),
                        details: vec![format!(
                            "status listing {status} returned a deposit reporting '{}' (expected '{expected}')",
                            record.status
                        )],
                    });
                }
                indexed.entry(outpoint).or_default().push(DaoIndexRow {
                    status,
                    deposit_block: record.deposit_block_number,
                });
            }
        }

        let observed = DaoStatusIndexObservation {
            recorded,
            indexed,
            newest_deposit_block,
        };
        let (cross_findings, absorbed) = evaluate_dao_status_index(&observed);
        findings.extend(cross_findings);

        // A reorg can move a deposit's status backwards, which is the one thing
        // that looks like the defect this check hunts. Detect it and say so
        // rather than reporting findings the chain, not the index, produced.
        let tip_after: BlockResponse = api_get(ctx, &format!("blocks/{tip_height}"))?;
        let reorged = tip_after.hash != tip_before.hash;

        let deposits_checked = observed.recorded.len() as u64;
        if reorged {
            // The anchor this walk was pinned to no longer exists, so neither
            // the findings nor their absence say anything about the index. An
            // anchor change is inconclusive on its own evidence — reporting a
            // green line with the word INCONCLUSIVE inside it hid exactly that.
            return Ok(CheckResult::inconclusive(format!(
                "block {} changed hash during the walk (reorg); {} deposit(s) and {} finding(s) \
                 discarded because the anchor moved — rerun",
                tip_height,
                deposits_checked,
                findings.len()
            )));
        }

        if findings.is_empty() {
            Ok(CheckResult::pass_with_detail(
                deposits_checked,
                format!(
                    "{} deposits, {} status index rows, {} concurrent transition(s) and {} new deposit(s) absorbed",
                    format_number(deposits_checked),
                    format_number(index_rows),
                    absorbed.transitions,
                    absorbed.created_mid_check
                ),
            ))
        } else {
            Ok(CheckResult::fail(deposits_checked, findings))
        }
    }
}

/// Every participant of a transaction must be able to find it again.
///
/// The whole point of the participant model is that `addr_txs` rows are derived
/// from `TxActions.participants` rather than from cells: one derivation, no
/// join to drift. This check reads it back from the other end — for each party
/// a recent activity lists, the party's own transaction index must contain that
/// transaction. A `Lock` party is looked up by address, a protocol-named party
/// by its 20-byte prefix.
pub struct ParticipantRowsConsistency;

/// How many recent activities the check walks, and how deep it pages each
/// party's transaction list looking for the transaction.
const PARTICIPANT_ROWS_ACTIVITY_LIMIT: usize = 50;
const PARTICIPANT_ROWS_TX_LOOKUP_LIMIT: usize = 100;

impl Check for ParticipantRowsConsistency {
    fn name(&self) -> &'static str {
        "participant_rows_consistency"
    }
    fn description(&self) -> &'static str {
        "Every participant of a recent activity finds that transaction in its own tx index"
    }
    fn tier(&self) -> CheckTier {
        CheckTier::Sampling
    }
    fn requires_rpc(&self) -> bool {
        false
    }
    /// The recent-activity sample is sized by `--sample-count`.
    fn requires_sampling(&self) -> bool {
        true
    }
    fn estimated_total(&self, ctx: &CheckContext) -> Option<u64> {
        Some(ctx.sample_count.min(PARTICIPANT_ROWS_ACTIVITY_LIMIT) as u64)
    }
    fn run(&self, ctx: &CheckContext, progress: &ProgressReporter) -> anyhow::Result<CheckResult> {
        let limit = ctx.sample_count.min(PARTICIPANT_ROWS_ACTIVITY_LIMIT);
        if limit == 0 {
            anyhow::bail!(
                "participant_rows_consistency was run with --sample-count 0: an empty sample \
                 verifies nothing"
            );
        }
        let page: CursorPage<GlobalActivityApiRecord> =
            api_get(ctx, &format!("activities?limit={}", limit))?;

        let mut findings = vec![];
        let mut checked = 0u64;
        for activity in &page.data {
            for participant in &activity.participants {
                let (path, entity) = match (&participant.lock_hash, &participant.lock_hash_prefix) {
                    // Resolved or plain cell participant: its full lock hash is
                    // the address index key.
                    (Some(lock_hash), _) => (
                        format!(
                            "addresses/{}/transactions?limit={}",
                            lock_hash, PARTICIPANT_ROWS_TX_LOOKUP_LIMIT
                        ),
                        format!("lock_hash={}", lock_hash),
                    ),
                    // Named but unresolved: only the prefix index can answer.
                    (None, Some(prefix)) => (
                        format!(
                            "addresses/prefix/{}/transactions?limit={}",
                            prefix, PARTICIPANT_ROWS_TX_LOOKUP_LIMIT
                        ),
                        format!("lock_hash_prefix={}", prefix),
                    ),
                    (None, None) => {
                        findings.push(Finding {
                            entity: format!("tx={}", activity.tx_hash),
                            details: vec![
                                "participant carries neither lockHash nor lockHashPrefix"
                                    .to_string(),
                            ],
                        });
                        continue;
                    }
                };
                let rows: CursorPage<AddressTxHashRecord> = api_get(ctx, &path)?;
                if !rows.data.iter().any(|row| row.tx_hash == activity.tx_hash) {
                    findings.push(Finding {
                        entity,
                        details: vec![format!(
                            "participant of tx={} (block {}) has no row for it in its own transaction index",
                            activity.tx_hash, activity.block_number
                        )],
                    });
                }
            }
            checked += 1;
            progress.inc(1);
        }

        if findings.is_empty() {
            Ok(CheckResult::pass(checked))
        } else {
            Ok(CheckResult::fail(checked, findings))
        }
    }
}

// ── Token live cell count ──────────────────────────────────────────────────

/// A token's persisted live cell count (`cellsCount`: the running sum of its
/// `TokenDailyDelta.live_cells_delta` rows, IV-27) equals the live cells the
/// API lists behind the token's type index (`/cells/live?type_script_hash=`,
/// each listed row checked to carry that type script).
///
/// The API no longer walks a token's cells per request; this check is where
/// the persisted count and the type index meet. A live cell missing from the
/// index, or a count that drifted from the cells, is a finding naming the
/// token. Tokens whose count exceeds the per-token budget are named in the
/// pass detail as not sampled, never counted as verified.
pub struct TokenLiveCellsConsistency;

const TOKEN_LIVE_CELLS_SAMPLE_CAP: usize = 20;
const TOKEN_LIVE_CELLS_MAX_PER_TOKEN: i64 = 25_000;
const TOKEN_LIVE_CELLS_MAX_TOTAL: i64 = 60_000;
const TOKEN_LIVE_CELLS_PAGE_LIMIT: usize = 100;

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct TokenCellsCountRecord {
    type_script_hash: String,
    cells_count: Option<i64>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct TypedCellRecord {
    tx_hash: String,
    output_index: i32,
    type_script_hash: Option<String>,
}

/// The token's live cells as `/cells/live` lists them by its type index, or
/// the details of every listed row that does not carry the token's type.
fn token_listed_live_cells(ctx: &CheckContext, token: &str) -> anyhow::Result<(i64, Vec<String>)> {
    let rows: Vec<TypedCellRecord> = all_cursor_pages(
        ctx,
        &format!("cells/live?type_script_hash={token}&limit={TOKEN_LIVE_CELLS_PAGE_LIMIT}"),
    )?;
    let mut details = vec![];
    for row in &rows {
        if row.type_script_hash.as_deref().map(normalize_hex_key) != Some(normalize_hex_key(token))
        {
            details.push(format!(
                "type index lists cell {}:{} whose type script hash is {:?}",
                row.tx_hash, row.output_index, row.type_script_hash
            ));
        }
    }
    let count = i64::try_from(rows.len())
        .map_err(|_| anyhow::anyhow!("listed live cell count overflow: token={token}"))?;
    Ok((count, details))
}

impl Check for TokenLiveCellsConsistency {
    fn name(&self) -> &'static str {
        "token_live_cells_consistency"
    }
    fn description(&self) -> &'static str {
        "Each sampled token's persisted live cell count equals the live cells behind its type index"
    }
    fn tier(&self) -> CheckTier {
        CheckTier::Sampling
    }
    fn requires_rpc(&self) -> bool {
        false
    }
    fn requires_sampling(&self) -> bool {
        true
    }
    fn estimated_total(&self, ctx: &CheckContext) -> Option<u64> {
        Some(ctx.sample_count.min(TOKEN_LIVE_CELLS_SAMPLE_CAP) as u64)
    }
    fn run(&self, ctx: &CheckContext, progress: &ProgressReporter) -> anyhow::Result<CheckResult> {
        if ctx.sample_count == 0 {
            anyhow::bail!(
                "token_live_cells_consistency was run with --sample-count 0: an empty sample \
                 verifies nothing"
            );
        }
        let tokens: CursorPage<TokenCellsCountRecord> = api_get(ctx, "tokens?limit=100")?;
        let mut findings = vec![];
        let mut checked = 0u64;
        let mut scanned_cells = 0i64;
        let mut not_sampled = vec![];
        for token in &tokens.data {
            if checked as usize >= ctx.sample_count.min(TOKEN_LIVE_CELLS_SAMPLE_CAP) {
                break;
            }
            let hash = token.type_script_hash.as_str();
            let listed_count = token
                .cells_count
                .ok_or_else(|| anyhow::anyhow!("/tokens row {hash} has no cellsCount"))?;
            let detail: TokenCellsCountRecord = api_get(ctx, &format!("tokens/{hash}"))?;
            let stored = detail
                .cells_count
                .ok_or_else(|| anyhow::anyhow!("/tokens/{hash} has no cellsCount"))?;
            let mut details = vec![];
            if stored != listed_count {
                details.push(format!(
                    "detail cellsCount={stored} != /tokens row cellsCount={listed_count}"
                ));
            }
            if stored > TOKEN_LIVE_CELLS_MAX_PER_TOKEN
                || scanned_cells + stored > TOKEN_LIVE_CELLS_MAX_TOTAL
            {
                not_sampled.push(format!("{hash} ({stored} cells)"));
                if !details.is_empty() {
                    findings.push(Finding {
                        entity: format!("token={hash}"),
                        details,
                    });
                }
                continue;
            }
            let (indexed, mut row_details) = token_listed_live_cells(ctx, hash)?;
            scanned_cells += indexed;
            details.append(&mut row_details);
            if indexed != stored {
                details.push(format!(
                    "persisted cellsCount={stored} != live cells behind the type index={indexed}"
                ));
            }
            if !details.is_empty() {
                findings.push(Finding {
                    entity: format!("token={hash}"),
                    details,
                });
            }
            checked += 1;
            progress.inc(1);
        }
        if checked == 0 && not_sampled.is_empty() {
            return Ok(CheckResult::pass_with_detail(
                0,
                "no tokens to sample".to_string(),
            ));
        }
        if !findings.is_empty() {
            return Ok(CheckResult::fail(checked, findings));
        }
        let mut detail = format!("{checked} sampled tokens, {scanned_cells} live cells listed");
        if !not_sampled.is_empty() {
            detail.push_str(&format!(
                "; over the {TOKEN_LIVE_CELLS_MAX_PER_TOKEN}-cell / {TOKEN_LIVE_CELLS_MAX_TOTAL}-total budget, not sampled: {}",
                not_sampled.join(", ")
            ));
        }
        Ok(CheckResult::pass_with_detail(checked, detail))
    }
}

// ── Collection daily activity counts ────────────────────────────────────────

/// A collection's persisted `collection_activity_daily` rows — served folded
/// into completed weeks as the insights `weeklyActivity` — equal its activity
/// rows folded by the same rule (`CollectionActivityDailyCounts::of_actions`)
/// into the UTC+8 week of each row's `chainTime`; and the rows the activity
/// list pages equal the collection's `activitiesCount`.
///
/// The API no longer walks a collection's activity rows for its calendar;
/// this check is where the daily rows meet them. Weeks after `completeThrough`
/// are on neither side. Collections above the row budget are named in the pass
/// detail as not sampled, never counted as verified.
pub struct CollectionDailyActivityConsistency;

const COLLECTION_DAILY_SAMPLE_CAP: usize = 10;
const COLLECTION_DAILY_MAX_ROWS_PER_COLLECTION: i64 = 5_000;
const COLLECTION_DAILY_MAX_ROWS_TOTAL: i64 = 20_000;
const COLLECTION_DAILY_PAGE_LIMIT: usize = 100;

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct CollectionCountRecord {
    id: String,
    activities_count: Option<i64>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct CollectionDetailCountRecord {
    activities_count: i64,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct CollectionWeekRecord {
    week_start: String,
    mint_transactions: u64,
    transfer_transactions: u64,
    burn_transactions: u64,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct CollectionCalendarRecord {
    weekly_activity: Vec<CollectionWeekRecord>,
    complete_through: Option<String>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct CollectionActivityRowRecord {
    tx_hash: String,
    chain_time: String,
    actions: Vec<String>,
}

fn parse_collection_action(action: &str) -> anyhow::Result<ckbadger_store::AssetAction> {
    use ckbadger_store::AssetAction;
    Ok(match action {
        "mint" => AssetAction::Mint,
        "transfer" => AssetAction::Transfer,
        "burn" => AssetAction::Burn,
        "recycle" => AssetAction::Recycle,
        "renew" => AssetAction::Renew,
        "update" => AssetAction::Update,
        other => anyhow::bail!("unknown collection activity action {other:?}"),
    })
}

/// The Monday (UTC+8) of the stats day of `chain_time_ms`.
fn utc8_week_start(chain_time_ms: i64) -> anyhow::Result<chrono::NaiveDate> {
    use chrono::Datelike;
    let day = ckbadger_common::stats_bucket::stats_day(chain_time_ms)?;
    Ok(day - chrono::Duration::days(i64::from(day.weekday().num_days_from_monday())))
}

/// Per completed week, the collection's rows folded by the daily-count rule,
/// and the number of rows listed (every week).
fn collection_weeks_from_rows(
    ctx: &CheckContext,
    id: &str,
    complete_through: Option<chrono::NaiveDate>,
) -> anyhow::Result<(std::collections::BTreeMap<chrono::NaiveDate, [u64; 3]>, i64)> {
    let rows: Vec<CollectionActivityRowRecord> = all_cursor_pages(
        ctx,
        &format!("assets/objects/{id}/activities?limit={COLLECTION_DAILY_PAGE_LIMIT}"),
    )?;
    let mut weeks = std::collections::BTreeMap::new();
    for row in &rows {
        let chain_time: i64 = row.chain_time.parse().map_err(|e| {
            anyhow::anyhow!(
                "collection {id} row {} has chainTime {:?}: {e}",
                row.tx_hash,
                row.chain_time
            )
        })?;
        let day = ckbadger_common::stats_bucket::stats_day(chain_time)?;
        if complete_through.is_none_or(|through| day > through) {
            continue;
        }
        let actions = row
            .actions
            .iter()
            .map(|action| parse_collection_action(action))
            .collect::<anyhow::Result<Vec<_>>>()?;
        let counts = ckbadger_store::CollectionActivityDailyCounts::of_actions(&actions);
        let week: &mut [u64; 3] = weeks.entry(utc8_week_start(chain_time)?).or_default();
        week[0] += counts.mint_transactions;
        week[1] += counts.transfer_transactions;
        week[2] += counts.burn_transactions;
    }
    let listed = i64::try_from(rows.len())
        .map_err(|_| anyhow::anyhow!("collection {id} row count overflow"))?;
    Ok((weeks, listed))
}

impl Check for CollectionDailyActivityConsistency {
    fn name(&self) -> &'static str {
        "collection_daily_activity_consistency"
    }
    fn description(&self) -> &'static str {
        "Each sampled collection's weekly calendar (persisted daily counts) equals its activity rows folded by chain-time week"
    }
    fn tier(&self) -> CheckTier {
        CheckTier::Sampling
    }
    fn requires_rpc(&self) -> bool {
        false
    }
    fn requires_sampling(&self) -> bool {
        true
    }
    fn estimated_total(&self, ctx: &CheckContext) -> Option<u64> {
        Some(ctx.sample_count.min(COLLECTION_DAILY_SAMPLE_CAP) as u64)
    }
    fn run(&self, ctx: &CheckContext, progress: &ProgressReporter) -> anyhow::Result<CheckResult> {
        if ctx.sample_count == 0 {
            anyhow::bail!(
                "collection_daily_activity_consistency was run with --sample-count 0: an empty \
                 sample verifies nothing"
            );
        }
        let mut candidates: Vec<CollectionCountRecord> = Vec::new();
        for kind in ["object", "identity"] {
            let page: CursorPage<CollectionCountRecord> =
                api_get(ctx, &format!("assets?type={kind}&limit=100"))?;
            candidates.extend(page.data);
        }
        let cap = ctx.sample_count.min(COLLECTION_DAILY_SAMPLE_CAP);
        let mut findings = vec![];
        let mut checked = 0u64;
        let mut listed_rows = 0i64;
        let mut not_sampled = vec![];
        for collection in &candidates {
            if checked as usize >= cap {
                break;
            }
            let id = collection.id.as_str();
            let count = collection
                .activities_count
                .ok_or_else(|| anyhow::anyhow!("/assets row {id} has no activitiesCount"))?;
            if count > COLLECTION_DAILY_MAX_ROWS_PER_COLLECTION
                || listed_rows + count > COLLECTION_DAILY_MAX_ROWS_TOTAL
            {
                not_sampled.push(format!("{id} ({count} rows)"));
                continue;
            }
            let calendar: CollectionCalendarRecord =
                api_get(ctx, &format!("assets/objects/{id}/insights"))?;
            let complete_through = calendar
                .complete_through
                .as_deref()
                .map(|day| chrono::NaiveDate::parse_from_str(day, "%Y-%m-%d"))
                .transpose()
                .map_err(|e| anyhow::anyhow!("collection {id} completeThrough: {e}"))?;
            let (from_rows, listed) = collection_weeks_from_rows(ctx, id, complete_through)?;
            listed_rows += listed;
            let mut served = std::collections::BTreeMap::new();
            for week in &calendar.weekly_activity {
                let start = chrono::NaiveDate::parse_from_str(&week.week_start, "%Y-%m-%d")
                    .map_err(|e| anyhow::anyhow!("collection {id} weekStart: {e}"))?;
                served.insert(
                    start,
                    [
                        week.mint_transactions,
                        week.transfer_transactions,
                        week.burn_transactions,
                    ],
                );
            }
            let mut details = vec![];
            let weeks: BTreeSet<_> = served.keys().chain(from_rows.keys()).copied().collect();
            for week in weeks {
                let served_counts = served.get(&week).copied().unwrap_or_default();
                let row_counts = from_rows.get(&week).copied().unwrap_or_default();
                if served_counts != row_counts {
                    details.push(format!(
                        "week {week}: daily counts [mint, transfer, burn] = {served_counts:?}, activity rows = {row_counts:?}"
                    ));
                }
            }
            if listed != count {
                // The tip may have moved while paging: only a count that the
                // detail confirms after the walk is a finding.
                let detail: CollectionDetailCountRecord =
                    api_get(ctx, &format!("assets/objects/{id}"))?;
                if detail.activities_count == count {
                    details.push(format!(
                        "activitiesCount={count} != activity rows listed={listed}"
                    ));
                }
            }
            if !details.is_empty() {
                findings.push(Finding {
                    entity: format!("collection={id}"),
                    details,
                });
            }
            checked += 1;
            progress.inc(1);
        }
        if checked == 0 && not_sampled.is_empty() {
            return Ok(CheckResult::pass_with_detail(
                0,
                "no collections to sample".to_string(),
            ));
        }
        if !findings.is_empty() {
            return Ok(CheckResult::fail(checked, findings));
        }
        let mut detail = format!("{checked} sampled collections, {listed_rows} activity rows");
        if !not_sampled.is_empty() {
            detail.push_str(&format!(
                "; over the {COLLECTION_DAILY_MAX_ROWS_PER_COLLECTION}-row / {COLLECTION_DAILY_MAX_ROWS_TOTAL}-total budget, not sampled: {}",
                not_sampled.join(", ")
            ));
        }
        Ok(CheckResult::pass_with_detail(checked, detail))
    }
}

// ── Global activity bucket index ───────────────────────────────────────────

/// `CF_TX_ACTIONS_BY_BUCKET` agrees with the classification of the
/// transactions it indexes.
///
/// For each sampled block and each bucket, two API reads must name the same
/// transactions: the block listing filtered by the bucket (which classifies
/// every canonical transaction of the block from its own `TxActions` row) and
/// the bucket listing positioned at that block (which reads the index). A
/// transaction only in the first has no bucket row; one only in the second is
/// an index row for a transaction that does not classify there. An index row
/// whose `TxActions` row is missing makes the bucket listing itself fail with
/// its position, which this check reports as an error.
pub struct GlobalActivityBucketIndexConsistency;

/// How many distinct recent blocks the check walks, at most.
const GLOBAL_BUCKET_SAMPLE_BLOCK_CAP: usize = 20;
const GLOBAL_BUCKET_PAGE_LIMIT: usize = 100;

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct BucketActivityRecord {
    tx_hash: String,
    block_number: i64,
    tx_index: i32,
}

/// The `(tx_index, tx_hash)` rows of `block` that the bucket listing (the
/// index) serves for `filter`, paging from just above the block until the
/// listing moves below it.
fn bucket_index_rows_for_block(
    ctx: &CheckContext,
    filter: &str,
    block: i64,
) -> anyhow::Result<BTreeSet<(i32, String)>> {
    let mut rows = BTreeSet::new();
    let mut cursor = format!("{}:0", block + 1);
    loop {
        let page: CursorPage<BucketActivityRecord> = api_get(
            ctx,
            &format!("activities?filter={filter}&limit={GLOBAL_BUCKET_PAGE_LIMIT}&cursor={cursor}"),
        )?;
        for row in &page.data {
            if row.block_number > block {
                anyhow::bail!(
                    "bucket listing filter={filter} returned block {} above its cursor {cursor}",
                    row.block_number
                );
            }
            if row.block_number < block {
                return Ok(rows);
            }
            rows.insert((row.tx_index, row.tx_hash.clone()));
        }
        match page.next_cursor {
            Some(next) if !page.data.is_empty() => cursor = next,
            _ => return Ok(rows),
        }
    }
}

impl Check for GlobalActivityBucketIndexConsistency {
    fn name(&self) -> &'static str {
        "global_activity_bucket_index_consistency"
    }
    fn description(&self) -> &'static str {
        "Every sampled block's transactions sit in exactly the global activity buckets they classify into"
    }
    fn tier(&self) -> CheckTier {
        CheckTier::Sampling
    }
    fn requires_rpc(&self) -> bool {
        false
    }
    /// The recent-activity sample that picks the blocks is sized by
    /// `--sample-count`.
    fn requires_sampling(&self) -> bool {
        true
    }
    fn estimated_total(&self, ctx: &CheckContext) -> Option<u64> {
        Some(ctx.sample_count.min(GLOBAL_BUCKET_SAMPLE_BLOCK_CAP) as u64)
    }
    fn run(&self, ctx: &CheckContext, progress: &ProgressReporter) -> anyhow::Result<CheckResult> {
        if ctx.sample_count == 0 {
            anyhow::bail!(
                "global_activity_bucket_index_consistency was run with --sample-count 0: an \
                 empty sample verifies nothing"
            );
        }
        let recent: CursorPage<BucketActivityRecord> = api_get(
            ctx,
            &format!("activities?limit={}", ctx.sample_count.min(100)),
        )?;
        let mut blocks: Vec<i64> = Vec::new();
        for row in &recent.data {
            if !blocks.contains(&row.block_number) {
                blocks.push(row.block_number);
            }
        }
        blocks.truncate(ctx.sample_count.min(GLOBAL_BUCKET_SAMPLE_BLOCK_CAP));
        if blocks.is_empty() {
            anyhow::bail!(
                "global_activity_bucket_index_consistency found no recent activity to sample"
            );
        }

        let mut findings = vec![];
        let mut checked = 0u64;
        for &block in &blocks {
            for bucket in ckbadger_store::GlobalBucket::ALL {
                let filter = bucket.as_filter();
                let classified: Vec<BucketActivityRecord> = all_cursor_pages(
                    ctx,
                    &format!(
                        "activities?blockNumber={block}&filter={filter}&limit={GLOBAL_BUCKET_PAGE_LIMIT}"
                    ),
                )?;
                let mut by_classification = BTreeSet::new();
                for row in classified {
                    if row.block_number != block {
                        anyhow::bail!(
                            "block listing blockNumber={block} filter={filter} returned a row of block {}",
                            row.block_number
                        );
                    }
                    by_classification.insert((row.tx_index, row.tx_hash));
                }
                let by_index = bucket_index_rows_for_block(ctx, filter, block)?;

                let mut details = vec![];
                for (tx_index, tx_hash) in by_classification.difference(&by_index) {
                    details.push(format!(
                        "tx={tx_hash} (index {tx_index}) classifies as {filter} but has no {filter} bucket row"
                    ));
                }
                for (tx_index, tx_hash) in by_index.difference(&by_classification) {
                    details.push(format!(
                        "{filter} bucket row at index {tx_index} names tx={tx_hash}, which does not classify as {filter} in this block"
                    ));
                }
                if !details.is_empty() {
                    findings.push(Finding {
                        entity: format!("block={block} bucket={filter}"),
                        details,
                    });
                }
            }
            checked += 1;
            progress.inc(1);
        }

        if findings.is_empty() {
            Ok(CheckResult::pass(checked))
        } else {
            Ok(CheckResult::fail(checked, findings))
        }
    }
}

// ── Address cellbase counts ────────────────────────────────────────────────

/// `AddressBalance.cellbase_txs_count` — what the unfiltered address activity
/// total subtracts from the address's transaction count — against the block
/// headers of a sampled set of blocks.
///
/// For each sampled block whose reward is already paid, the block's miner
/// (`minerAddress`, its cellbase-witness lock) must hold the paying cellbase
/// (`miningRewardTxHash`, at `rewardPaidAtBlock`) in its own transaction index,
/// flagged as a cellbase. For a miner with at most 10,000 transactions the
/// check then pages its whole index: the committed rows must equal
/// `transactionsCount`, and the persisted cellbase count — `transactionsCount`
/// minus the activity `total` — must equal the cellbase rows exactly. A larger
/// miner gets the row check and `1 <= cellbase count <= transactionsCount`.
pub struct AddressCellbaseCountConsistency;

/// How many distinct sampled blocks the check walks, at most.
const CELLBASE_COUNT_SAMPLE_BLOCK_CAP: usize = 20;
const CELLBASE_COUNT_PAGE_LIMIT: usize = 100;
/// Separates this check's block sample from other checks run with the same seed.
const CELLBASE_COUNT_SEED_SALT: u64 = 0xCE11_BA5E_C0FF_EE05;

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct BlockRewardApiRecord {
    miner_address: Option<String>,
    mining_reward_tx_hash: Option<String>,
    reward_paid_at_block: Option<i64>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct AddressTxRowApiRecord {
    tx_hash: String,
    /// Null for a tx-pool row (page one only): not a committed transaction.
    block_number: Option<i64>,
    is_cellbase: bool,
}

/// Whether `miner`'s transaction index holds `cellbase` at block `paid`, as a
/// cellbase row. Pages from just above `paid` until the listing moves below it.
fn miner_holds_reward_cellbase(
    ctx: &CheckContext,
    miner: &str,
    paid: i64,
    cellbase: &str,
) -> anyhow::Result<bool> {
    let mut cursor = format!("{}:0", paid + 1);
    loop {
        let page: CursorPage<AddressTxRowApiRecord> = api_get(
            ctx,
            &format!(
                "addresses/{miner}/transactions?limit={CELLBASE_COUNT_PAGE_LIMIT}&cursor={cursor}"
            ),
        )?;
        for row in &page.data {
            let block = row.block_number.ok_or_else(|| {
                anyhow::anyhow!(
                    "transactions of {miner} at cursor {cursor} served a pool row (pool rows are page-one only)"
                )
            })?;
            if block > paid {
                anyhow::bail!(
                    "transactions of {miner} returned block {block} above its cursor {cursor}"
                );
            }
            if block < paid {
                return Ok(false);
            }
            if normalize_hex_key(&row.tx_hash) == normalize_hex_key(cellbase) {
                return Ok(row.is_cellbase);
            }
        }
        match page.next_cursor {
            Some(next) if !page.data.is_empty() => cursor = next,
            _ => return Ok(false),
        }
    }
}

/// The findings for one sampled block, and whether its miner was counted
/// exactly (bounded) or checked by position only.
struct CellbaseCountOutcome {
    details: Vec<String>,
    exact: bool,
}

fn verify_cellbase_count_sample(
    ctx: &CheckContext,
    block: u64,
    miner: &str,
    paid: i64,
    cellbase: &str,
) -> anyhow::Result<CellbaseCountOutcome> {
    let mut details = vec![];
    if !miner_holds_reward_cellbase(ctx, miner, paid, cellbase)? {
        details.push(format!(
            "block {block}'s reward cellbase {cellbase} (block {paid}) is not a cellbase row of its miner's transaction index"
        ));
    }
    let address: AddressDetailApiRecord = api_get(ctx, &format!("addresses/{miner}"))?;
    let activities: CursorPageWithTotal<serde_json::Value> =
        api_get(ctx, &format!("addresses/{miner}/activities?limit=1"))?;
    let activity_total = activities
        .total
        .ok_or_else(|| anyhow::anyhow!("unfiltered activities of {miner} declare no total"))?;
    let cellbase_count = address
        .transactions_count
        .checked_sub(activity_total)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "{miner}: transactionsCount {} - activities total {activity_total} overflows",
                address.transactions_count
            )
        })?;
    if cellbase_count < 1 || cellbase_count > address.transactions_count {
        details.push(format!(
            "persisted cellbase count {cellbase_count} (transactionsCount {} - activities total {activity_total}) is outside 1..=transactionsCount for a miner paid at block {paid}",
            address.transactions_count
        ));
    }
    if address.transactions_count > ADDRESS_BALANCE_MAX_TXS_PER_SAMPLE {
        return Ok(CellbaseCountOutcome {
            details,
            exact: false,
        });
    }
    let rows: Vec<AddressTxRowApiRecord> = all_cursor_pages(
        ctx,
        &format!("addresses/{miner}/transactions?limit={CELLBASE_COUNT_PAGE_LIMIT}"),
    )?;
    let committed: Vec<&AddressTxRowApiRecord> = rows
        .iter()
        .filter(|row| row.block_number.is_some())
        .collect();
    let committed_rows = i64::try_from(committed.len())?;
    let cellbase_rows = i64::try_from(committed.iter().filter(|row| row.is_cellbase).count())?;
    if committed_rows != address.transactions_count {
        details.push(format!(
            "transactionsCount {} != {committed_rows} committed transaction rows",
            address.transactions_count
        ));
    }
    if cellbase_count != cellbase_rows {
        details.push(format!(
            "persisted cellbase count {cellbase_count} (transactionsCount {} - activities total {activity_total}) != {cellbase_rows} cellbase rows",
            address.transactions_count
        ));
    }
    Ok(CellbaseCountOutcome {
        details,
        exact: true,
    })
}

impl Check for AddressCellbaseCountConsistency {
    fn name(&self) -> &'static str {
        "address_cellbase_count_consistency"
    }
    fn description(&self) -> &'static str {
        "Sampled blocks' reward cellbases sit in their miners' transaction indexes, and bounded miners' persisted cellbase counts equal their cellbase rows"
    }
    fn tier(&self) -> CheckTier {
        CheckTier::Sampling
    }
    fn requires_rpc(&self) -> bool {
        false
    }
    /// The block sample is sized by `--sample-count`.
    fn requires_sampling(&self) -> bool {
        true
    }
    fn estimated_total(&self, ctx: &CheckContext) -> Option<u64> {
        Some(ctx.sample_count.min(CELLBASE_COUNT_SAMPLE_BLOCK_CAP) as u64)
    }
    fn run(&self, ctx: &CheckContext, progress: &ProgressReporter) -> anyhow::Result<CheckResult> {
        if ctx.sample_count == 0 {
            anyhow::bail!(
                "address_cellbase_count_consistency was run with --sample-count 0: an empty \
                 sample verifies nothing"
            );
        }
        let tip_before = sampling_tip_from_stats(&fetch_network_stats(ctx)?)?;
        let tip_i64 = i64::try_from(tip_before)?;
        // Blocks 1..=tip: genesis has no miner.
        let blocks: Vec<u64> = LcgSampler::new(ctx.seed.wrapping_add(CELLBASE_COUNT_SEED_SALT))
            .sample_range(
                ctx.sample_count.min(CELLBASE_COUNT_SAMPLE_BLOCK_CAP),
                tip_before,
            )
            .into_iter()
            .map(|offset| offset + 1)
            .collect();

        let mut findings = vec![];
        let mut skipped: Vec<String> = vec![];
        let (mut exact, mut positional, mut unpaid) = (0u64, 0u64, 0u64);
        for &block in &blocks {
            let record: BlockRewardApiRecord = api_get(ctx, &format!("blocks/{block}"))?;
            let (Some(miner), Some(cellbase), Some(paid)) = (
                record.miner_address.as_deref(),
                record.mining_reward_tx_hash.as_deref(),
                record.reward_paid_at_block,
            ) else {
                unpaid += 1;
                progress.inc(1);
                continue;
            };
            if paid > tip_i64 {
                unpaid += 1;
                progress.inc(1);
                continue;
            }
            let mut outcome = verify_cellbase_count_sample(ctx, block, miner, paid, cellbase)?;
            if !outcome.details.is_empty() {
                // Re-read once: the reads race a live-syncing tip.
                outcome = verify_cellbase_count_sample(ctx, block, miner, paid, cellbase)?;
            }
            if !outcome.details.is_empty() {
                let tip_after = sampling_tip_from_stats(&fetch_network_stats(ctx)?)?;
                if tip_after > tip_before {
                    skipped.push(format!(
                        "block {block} (tip advanced {tip_before}→{tip_after}): {}",
                        outcome.details.join("; ")
                    ));
                } else {
                    findings.push(Finding {
                        entity: format!("block={block} miner={miner}"),
                        details: outcome.details,
                    });
                }
            }
            if outcome.exact {
                exact += 1;
            } else {
                positional += 1;
            }
            progress.inc(1);
        }
        let checked = exact + positional;
        if checked == 0 {
            anyhow::bail!(
                "address_cellbase_count_consistency found no sampled block with a paid reward: \
                 tip={tip_before}, sampled={}, unpaid={unpaid}",
                blocks.len()
            );
        }
        let skip_note = if skipped.is_empty() {
            String::new()
        } else {
            format!(", {} skipped: {}", skipped.len(), skipped.join(" | "))
        };
        let detail = format!(
            "{checked} sampled blocks: {exact} miners counted exactly, {positional} checked by \
             position only (more than {ADDRESS_BALANCE_MAX_TXS_PER_SAMPLE} transactions), \
             {unpaid} rewards not yet paid{skip_note}"
        );
        if findings.is_empty() {
            Ok(CheckResult::pass_with_detail(checked, detail))
        } else {
            Ok(CheckResult {
                detail: Some(detail),
                ..CheckResult::fail(checked, findings)
            })
        }
    }
}

// ── `.cell` (DotCell) names ────────────────────────────────────────────────

/// How many `.cell` names a records-parity sample may read back from the node.
const DOTCELL_RECORDS_SAMPLE_CAP: usize = 20;

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct DotCellItemListRecord {
    nft_id: String,
    name: Option<String>,
    is_live: bool,
}

/// The address name list has its own wire contract, distinct from collection
/// items (`nftId`, `isLive`). Require its identity field even when only counting.
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct DotCellOwnerNameApiRecord {
    #[serde(rename = "identityId")]
    _identity_id: String,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct DotCellRingApiRecord {
    first_id: String,
    live_count: i64,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct DotCellItemDetailApiRecord {
    identity_id: String,
    label: String,
    is_live: bool,
    next_id: String,
    records_hash: String,
    live_out_point: Option<DotCellOutPointApiRecord>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct DotCellOutPointApiRecord {
    tx_hash: String,
    index: i16,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct DotCellHolderApiRecord {
    owner_hash_prefix: Option<String>,
    address: Option<String>,
    item_count: i64,
}

/// Why a `.cell` check has nothing to verify on this network.
const DOTCELL_NOT_DEPLOYED: &str =
    "GET assets/identities/dotcell returned 404: no .cell deployment on this network";

/// The `.cell` collection, or `None` when this network has no deployment.
///
/// Only a 404 means "not deployed". A 5xx, a transport failure or a body that
/// does not decode is an error: reading those as "no collection" turned every
/// broken API into four green checks over zero items.
fn dotcell_collection(ctx: &CheckContext) -> anyhow::Result<Option<NftCollectionDetailApiRecord>> {
    api_get_or_not_found(ctx, "assets/identities/dotcell")
}

/// Every row of a cursor-paged listing: `first_path` (which already carries
/// its query string), then `&cursor=<next>` until the API stops handing one out.
fn all_cursor_pages<T: serde::de::DeserializeOwned>(
    ctx: &CheckContext,
    first_path: &str,
) -> anyhow::Result<Vec<T>> {
    let mut rows = Vec::new();
    let mut cursor: Option<String> = None;
    loop {
        let path = match &cursor {
            Some(cursor) => format!("{first_path}&cursor={cursor}"),
            None => first_path.to_string(),
        };
        let page: CursorPage<T> = api_get(ctx, &path)?;
        let page_len = page.data.len();
        rows.extend(page.data);
        match page.next_cursor {
            Some(next) if page_len > 0 => cursor = Some(next),
            _ => break,
        }
    }
    Ok(rows)
}

/// Every live `.cell` name, paged out of the collection listing.
fn dotcell_live_items(ctx: &CheckContext) -> anyhow::Result<Option<Vec<DotCellItemListRecord>>> {
    if dotcell_collection(ctx)?.is_none() {
        return Ok(None);
    }
    all_cursor_pages(ctx, "assets/identities/dotcell/items?limit=100&status=live").map(Some)
}

fn parse_hex20(label: &str, value: &str) -> anyhow::Result<[u8; 20]> {
    let bytes = hex::decode(value.trim_start_matches("0x"))
        .map_err(|e| anyhow::anyhow!("{label} is not hex: {value}: {e}"))?;
    bytes
        .try_into()
        .map_err(|_| anyhow::anyhow!("{label} is not 20 bytes: {value}"))
}

pub struct DotCellRingIntegrity;

impl Check for DotCellRingIntegrity {
    fn name(&self) -> &'static str {
        "dotcell_ring_integrity"
    }
    fn description(&self) -> &'static str {
        "Walking the .cell uniqueness ring from its root visits every live name exactly once, in ascending id order"
    }
    fn tier(&self) -> CheckTier {
        CheckTier::Sampling
    }
    fn run(&self, ctx: &CheckContext, progress: &ProgressReporter) -> anyhow::Result<CheckResult> {
        let Some(items) = dotcell_live_items(ctx)? else {
            return Ok(CheckResult::not_applicable(DOTCELL_NOT_DEPLOYED));
        };
        let ring: DotCellRingApiRecord = api_get(ctx, "assets/identities/dotcell/ring")?;

        // The listing gives only ids; `next` comes from each item's detail.
        let mut next_by_id: std::collections::HashMap<[u8; 20], [u8; 20]> =
            std::collections::HashMap::new();
        for item in &items {
            let detail: DotCellItemDetailApiRecord = api_get(
                ctx,
                &format!("assets/identities/dotcell/items/{}", item.nft_id),
            )?;
            next_by_id.insert(
                parse_hex20("item id", &detail.identity_id)?,
                parse_hex20("next id", &detail.next_id)?,
            );
            progress.inc(1);
        }

        let mut findings = vec![];
        let mut visited: std::collections::HashSet<[u8; 20]> = std::collections::HashSet::new();
        let mut current = parse_hex20("ring first id", &ring.first_id)?;
        let mut previous: Option<[u8; 20]> = None;
        while current != [0u8; 20] {
            if !visited.insert(current) {
                findings.push(Finding {
                    entity: format!("dotcell_id=0x{}", hex::encode(current)),
                    details: vec!["the ring revisits this name: it is a cycle".to_string()],
                });
                break;
            }
            if let Some(previous) = previous {
                if current <= previous {
                    findings.push(Finding {
                        entity: format!("dotcell_id=0x{}", hex::encode(current)),
                        details: vec![format!(
                            "ring is not ascending: previous=0x{}",
                            hex::encode(previous)
                        )],
                    });
                }
            }
            let Some(next) = next_by_id.get(&current).copied() else {
                findings.push(Finding {
                    entity: format!("dotcell_id=0x{}", hex::encode(current)),
                    details: vec!["the ring points at a name that is not live".to_string()],
                });
                break;
            };
            previous = Some(current);
            current = next;
        }

        let visited_count = visited.len() as i64;
        if visited_count != ring.live_count {
            findings.push(Finding {
                entity: "dotcell_ring".to_string(),
                details: vec![format!(
                    "walk visited {} names, ring reports live_count={}",
                    visited_count, ring.live_count
                )],
            });
        }
        if visited_count != items.len() as i64 {
            findings.push(Finding {
                entity: "dotcell_ring".to_string(),
                details: vec![format!(
                    "walk visited {} names, the collection lists {} live",
                    visited_count,
                    items.len()
                )],
            });
        }

        let checked = items.len() as u64;
        if findings.is_empty() {
            Ok(CheckResult::pass(checked))
        } else {
            Ok(CheckResult::fail(checked, findings))
        }
    }
}

pub struct DotCellIdIsLabelHash;

fn required_dotcell_label<'a>(name: Option<&'a str>, nft_id: &str) -> anyhow::Result<&'a str> {
    let name = name.filter(|name| !name.is_empty()).ok_or_else(|| {
        anyhow::anyhow!("live .cell item has no name while verifying id: nft_id={nft_id}")
    })?;
    name.strip_suffix(".cell").ok_or_else(|| {
        anyhow::anyhow!("live .cell item name lacks .cell suffix: nft_id={nft_id} name={name:?}")
    })
}

impl Check for DotCellIdIsLabelHash {
    fn name(&self) -> &'static str {
        "dotcell_id_is_label_hash"
    }
    fn description(&self) -> &'static str {
        "Every .cell name's id is blake2b(label)[..20]"
    }
    fn tier(&self) -> CheckTier {
        CheckTier::Sampling
    }
    fn run(&self, ctx: &CheckContext, progress: &ProgressReporter) -> anyhow::Result<CheckResult> {
        let Some(items) = dotcell_live_items(ctx)? else {
            return Ok(CheckResult::not_applicable(DOTCELL_NOT_DEPLOYED));
        };
        let mut findings = vec![];
        for item in &items {
            // The listing's `name` is the label plus ".cell".
            let label = required_dotcell_label(item.name.as_deref(), &item.nft_id)?;
            let expected = ckbadger_store::types::derive_dotcell_id(label);
            let actual = parse_hex20("item id", &item.nft_id)?;
            if expected != actual {
                findings.push(Finding {
                    entity: format!("dotcell_id={}", item.nft_id),
                    details: vec![format!(
                        "label {:?} hashes to 0x{}",
                        label,
                        hex::encode(expected)
                    )],
                });
            }
            progress.inc(1);
        }
        let checked = items.len() as u64;
        if findings.is_empty() {
            Ok(CheckResult::pass(checked))
        } else {
            Ok(CheckResult::fail(checked, findings))
        }
    }
}

pub struct DotCellOwnerIndexConsistency;

impl Check for DotCellOwnerIndexConsistency {
    fn name(&self) -> &'static str {
        "dotcell_owner_index_consistency"
    }
    fn description(&self) -> &'static str {
        "Per-owner .cell counts sum to live_count, and each resolvable owner's name list matches its count"
    }
    fn tier(&self) -> CheckTier {
        CheckTier::Sampling
    }
    fn run(&self, ctx: &CheckContext, progress: &ProgressReporter) -> anyhow::Result<CheckResult> {
        let Some(detail) = dotcell_collection(ctx)? else {
            return Ok(CheckResult::not_applicable(DOTCELL_NOT_DEPLOYED));
        };

        let holders: Vec<DotCellHolderApiRecord> =
            all_cursor_pages(ctx, "assets/identities/dotcell/holders?limit=100")?;

        let mut findings = vec![];
        let sum: i64 = holders.iter().map(|holder| holder.item_count).sum();
        if sum != detail.live_count {
            findings.push(Finding {
                entity: "dotcell_holders".to_string(),
                details: vec![format!(
                    "per-owner counts sum to {}, aggregate live_count={}",
                    sum, detail.live_count
                )],
            });
        }
        if holders.len() as i64 != detail.holders_count {
            findings.push(Finding {
                entity: "dotcell_holders".to_string(),
                details: vec![format!(
                    "{} holder rows, aggregate holders_count={}",
                    holders.len(),
                    detail.holders_count
                )],
            });
        }

        // Spot-check the resolvable owners: their own name list must match the
        // counter. An owner whose 20-byte prefix resolves to no known lock has
        // no address page to ask, and that is not a defect.
        let mut checked = 0u64;
        for holder in holders.iter().take(IDENTITY_HOLDER_SPOT_CHECK_LIMIT) {
            progress.inc(1);
            let Some(address) = holder.address.as_deref() else {
                continue;
            };
            // The owner's whole list, not its first page: an owner with more
            // names than one page holds would otherwise always disagree.
            let names: Vec<DotCellOwnerNameApiRecord> = all_cursor_pages(
                ctx,
                &format!("addresses/{}/dotcell-names?limit=100", address),
            )?;
            checked += 1;
            if names.len() as i64 != holder.item_count {
                findings.push(Finding {
                    entity: format!(
                        "dotcell_owner={}",
                        holder.owner_hash_prefix.as_deref().unwrap_or(address)
                    ),
                    details: vec![format!(
                        "address lists {} names, counter says {}",
                        names.len(),
                        holder.item_count
                    )],
                });
            }
        }

        if findings.is_empty() {
            Ok(CheckResult::pass(checked.max(holders.len() as u64)))
        } else {
            Ok(CheckResult::fail(
                checked.max(holders.len() as u64),
                findings,
            ))
        }
    }
}

pub struct DotCellRecordsHashParity;

impl Check for DotCellRecordsHashParity {
    fn name(&self) -> &'static str {
        "dotcell_records_hash_parity"
    }
    fn description(&self) -> &'static str {
        "A sampled .cell name's live cell data and its creating witness agree with the stored records hash"
    }
    fn tier(&self) -> CheckTier {
        CheckTier::Sampling
    }
    fn requires_rpc(&self) -> bool {
        true
    }
    /// The names read back from the node are sized by `--sample-count`.
    fn requires_sampling(&self) -> bool {
        true
    }
    fn run(&self, ctx: &CheckContext, progress: &ProgressReporter) -> anyhow::Result<CheckResult> {
        let Some(rpc_url) = ctx.rpc_url.clone() else {
            anyhow::bail!("dotcell_records_hash_parity needs a CKB RPC URL");
        };
        let Some(items) = dotcell_live_items(ctx)? else {
            return Ok(CheckResult::not_applicable(DOTCELL_NOT_DEPLOYED));
        };

        let sample = ctx.sample_count.min(DOTCELL_RECORDS_SAMPLE_CAP);
        let mut findings = vec![];
        let mut checked = 0u64;
        for item in items.iter().filter(|item| item.is_live).take(sample) {
            progress.inc(1);
            let detail: DotCellItemDetailApiRecord = api_get(
                ctx,
                &format!("assets/identities/dotcell/items/{}", item.nft_id),
            )?;
            checked += 1;
            if !detail.is_live {
                findings.push(Finding {
                    entity: format!("dotcell_id={}", item.nft_id),
                    details: vec![
                        "the live listing returned a name the detail route calls recycled"
                            .to_string(),
                    ],
                });
                continue;
            }
            let Some(outpoint) = detail.live_out_point.as_ref() else {
                findings.push(Finding {
                    entity: format!("dotcell_id={}", item.nft_id),
                    details: vec!["a live name has no live outpoint".to_string()],
                });
                continue;
            };
            let (tx_hash, output_index) = (outpoint.tx_hash.as_str(), outpoint.index);

            let cell = rpc_call(
                ctx,
                &rpc_url,
                "get_live_cell",
                vec![
                    serde_json::json!({
                        "tx_hash": tx_hash,
                        "index": format!("0x{:x}", output_index),
                    }),
                    serde_json::Value::Bool(true),
                ],
            )?;
            let data_hex = cell
                .as_ref()
                .and_then(|cell| cell.get("cell"))
                .and_then(|cell| cell.get("data"))
                .and_then(|data| data.get("content"))
                .and_then(|content| content.as_str());
            let Some(data_hex) = data_hex else {
                findings.push(Finding {
                    entity: format!("dotcell_id={}", item.nft_id),
                    details: vec![format!(
                        "node has no live cell at {}:{}",
                        tx_hash, output_index
                    )],
                });
                continue;
            };
            let data = hex::decode(data_hex.trim_start_matches("0x"))?;
            if data.len() < 33 {
                findings.push(Finding {
                    entity: format!("dotcell_id={}", item.nft_id),
                    details: vec![format!("cell data is {} bytes", data.len())],
                });
                continue;
            }
            let on_chain_records_hash = format!("0x{}", hex::encode(&data[1..33]));
            if on_chain_records_hash != detail.records_hash {
                findings.push(Finding {
                    entity: format!("dotcell_id={}", item.nft_id),
                    details: vec![format!(
                        "stored recordsHash={} but cell data[1..33]={}",
                        detail.records_hash, on_chain_records_hash
                    )],
                });
            }

            // The records payload itself lives in the witness at this output's
            // own index in the creating transaction.
            let tx = rpc_call(
                ctx,
                &rpc_url,
                "get_transaction",
                vec![serde_json::Value::String(tx_hash.to_string())],
            )?;
            let witness = tx
                .as_ref()
                .and_then(|tx| tx.get("transaction"))
                .and_then(|tx| tx.get("witnesses"))
                .and_then(|witnesses| witnesses.as_array())
                .and_then(|witnesses| witnesses.get(output_index as usize))
                .and_then(|witness| witness.as_str());
            let Some(witness) = witness else {
                findings.push(Finding {
                    entity: format!("dotcell_id={}", item.nft_id),
                    details: vec![format!(
                        "creating tx {} has no witness at index {}",
                        tx_hash, output_index
                    )],
                });
                continue;
            };
            let expected: [u8; 32] = hex::decode(detail.records_hash.trim_start_matches("0x"))?
                .try_into()
                .map_err(|_| {
                    anyhow::anyhow!("recordsHash is not 32 bytes: {}", detail.records_hash)
                })?;
            if let Err(e) = crate::parser::DotCellParser::parse_witness_records(witness, &expected)
            {
                findings.push(Finding {
                    entity: format!("dotcell_id={} label={}", item.nft_id, detail.label),
                    details: vec![format!("witness records do not verify: {e}")],
                });
            }
        }

        if findings.is_empty() {
            Ok(CheckResult::pass(checked))
        } else {
            Ok(CheckResult::fail(checked, findings))
        }
    }
}

pub fn api_checks() -> Vec<Box<dyn Check>> {
    vec![
        // Fast
        Box::new(ApiReachable),
        Box::new(SyncComplete),
        Box::new(GenesisBlock),
        Box::new(TipBlock),
        Box::new(DeepForkClear),
        Box::new(DaoStatisticsSane),
        Box::new(GenesisBaselineBurntInvariant),
        // Sampling
        Box::new(BlockHashRoundtrip),
        Box::new(BlockParentChain),
        Box::new(AddressBalanceSpotCheck),
        Box::new(ChartTxCountPositive),
        Box::new(ChartCellCountConsistency),
        Box::new(ChartTotalSupplyMonotonic),
        Box::new(ChartBlockTimeDistributionSane),
        Box::new(ChartEpochTimeDistributionSane),
        Box::new(ChartEpochTimeLengthSane),
        Box::new(ChartAverageBlockTimeSane),
        Box::new(ChartMinerDistributionConsistency),
        Box::new(ChartNominalApcSane),
        Box::new(ChartInflationRateSane),
        Box::new(ChartHodlWaveConsistency),
        Box::new(ChartKnowledgeCompositionExact),
        Box::new(SecondaryIssuancePartitionConservation),
        Box::new(BurntSupplyGenesisInvariant),
        Box::new(RpcBlockSpotCheck),
        Box::new(TokenActivityTransferBidirectional),
        Box::new(SporeOwnerRoundtrip),
        Box::new(ObjectAssetCollectionConsistency),
        Box::new(AssetTopHoldersAddressConsistency),
        Box::new(IdentityCollectionHolderConsistency),
        Box::new(DaoStatusIndexMatchesDeposits),
        Box::new(ParticipantRowsConsistency),
        Box::new(GlobalActivityBucketIndexConsistency),
        Box::new(TokenLiveCellsConsistency),
        Box::new(CollectionDailyActivityConsistency),
        Box::new(AddressCellbaseCountConsistency),
        Box::new(DotCellRingIntegrity),
        Box::new(DotCellIdIsLabelHash),
        Box::new(DotCellOwnerIndexConsistency),
        Box::new(DotCellRecordsHashParity),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dotcell_id_check_rejects_missing_or_malformed_label() {
        assert_eq!(
            required_dotcell_label(Some("alice.cell"), "0xaa").unwrap(),
            "alice"
        );
        for name in [None, Some(""), Some("alice")] {
            let err = required_dotcell_label(name, "0xaa").unwrap_err();
            assert!(err.to_string().contains("nft_id=0xaa"), "{err:#}");
        }
    }

    fn dao_observation(
        recorded: &[(&str, i16, i64)],
        indexed: &[(&str, i16, i64)],
        newest_deposit_block: i64,
    ) -> DaoStatusIndexObservation {
        let mut recorded_map: HashMap<String, (i16, i64)> = HashMap::new();
        for (outpoint, status, block) in recorded {
            recorded_map.insert((*outpoint).to_string(), (*status, *block));
        }
        let mut indexed_map: HashMap<String, Vec<DaoIndexRow>> = HashMap::new();
        for (outpoint, status, block) in indexed {
            indexed_map
                .entry((*outpoint).to_string())
                .or_default()
                .push(DaoIndexRow {
                    status: *status,
                    deposit_block: *block,
                });
        }
        DaoStatusIndexObservation {
            recorded: recorded_map,
            indexed: indexed_map,
            newest_deposit_block,
        }
    }

    #[test]
    fn dao_status_index_accepts_a_consistent_observation() {
        let observed = dao_observation(
            &[("0xaa:0", 0, 30), ("0xbb:0", 1, 20), ("0xcc:0", 2, 10)],
            &[("0xaa:0", 0, 30), ("0xbb:0", 1, 20), ("0xcc:0", 2, 10)],
            30,
        );
        let (findings, absorbed) = evaluate_dao_status_index(&observed);
        assert!(findings.is_empty(), "unexpected findings: {findings:?}");
        assert_eq!(absorbed, DaoStatusIndexAbsorbed::default());
    }

    #[test]
    fn dao_status_index_flags_a_listing_behind_the_deposit() {
        // The reported bug: the deposit moved to withdrawing, the status-0
        // listing still carries it.
        let observed = dao_observation(&[("0xaa:0", 1, 30)], &[("0xaa:0", 0, 30)], 30);
        let (findings, absorbed) = evaluate_dao_status_index(&observed);
        assert_eq!(findings.len(), 1);
        assert!(
            findings[0].details[0].contains("status listing is behind the deposit"),
            "{:?}",
            findings[0]
        );
        assert_eq!(absorbed, DaoStatusIndexAbsorbed::default());
    }

    #[test]
    fn dao_status_index_absorbs_a_transition_that_lands_mid_walk() {
        // Recorded as deposited, found in the withdrawing listing walked later:
        // status only moves forward, so this is the chain, not the index.
        let observed = dao_observation(&[("0xaa:0", 0, 30)], &[("0xaa:0", 1, 30)], 30);
        let (findings, absorbed) = evaluate_dao_status_index(&observed);
        assert!(findings.is_empty(), "unexpected findings: {findings:?}");
        assert_eq!(absorbed.transitions, 1);
    }

    #[test]
    fn dao_status_index_flags_a_deposit_missing_from_every_listing() {
        let observed = dao_observation(&[("0xaa:0", 0, 30)], &[], 30);
        let (findings, _) = evaluate_dao_status_index(&observed);
        assert_eq!(findings.len(), 1);
        assert!(
            findings[0].details[0].contains("appears in no status listing"),
            "{:?}",
            findings[0]
        );
    }

    #[test]
    fn dao_status_index_flags_an_index_row_with_no_deposit() {
        let observed = dao_observation(&[], &[("0xaa:0", 0, 25)], 30);
        let (findings, absorbed) = evaluate_dao_status_index(&observed);
        assert_eq!(findings.len(), 1);
        assert!(
            findings[0].details[0].contains("the deposit listing does not"),
            "{:?}",
            findings[0]
        );
        assert_eq!(absorbed.created_mid_check, 0);
    }

    #[test]
    fn dao_status_index_absorbs_a_deposit_created_mid_check() {
        // Newer than the newest deposit the unfiltered walk could have seen.
        let observed = dao_observation(&[], &[("0xaa:0", 0, 31)], 30);
        let (findings, absorbed) = evaluate_dao_status_index(&observed);
        assert!(findings.is_empty(), "unexpected findings: {findings:?}");
        assert_eq!(absorbed.created_mid_check, 1);
    }
    use serde_json::json;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

    fn test_ctx() -> CheckContext {
        CheckContext {
            network: ckbadger_common::hardfork::NETWORK_MAINNET,
            api_url: "http://localhost:3001/api/v1".to_string(),
            rpc_url: None,
            explorer_url: None,
            http: reqwest::blocking::Client::new(),
            sample_count: 10,
            seed: 42,
            tolerance: 0.001,
            cache_dir: None,
            entities: Vec::new(),
            verify_source_path: None,
            evidence_dir: None,
            entity_budget: Default::default(),
            source_profile: std::sync::Mutex::new(None),
        }
    }

    /// Vector: mainnet block 12,000,000 cellbase witness. The rendered miner
    /// address must equal the true miner (args 0x8211f1b9…) — the exact
    /// address the official explorer reports for that block.
    #[test]
    fn test_miner_address_from_witness_mainnet_vector() {
        let witness = "0x7a0000000c00000055000000490000001000000030000000310000009bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce801140000008211f1b938a107cd53b6302cc752a6fc3965638d210000000000000020302e3131332e3020283832383731613320323032342d30312d303929";
        let addr = miner_address_from_witness(witness, "mainnet").unwrap();
        assert_eq!(
            addr,
            "ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsqvzz8cmjw9pqlx48d3s9nr49fhu89jk8rgycece5"
        );
    }

    #[test]
    fn test_compare_block_vs_rpc_flags_field_mismatches() {
        let ctx = test_ctx();
        let ours = BlockResponse {
            number: 42,
            hash: "0xaa".to_string(),
            parent_hash: "0x".to_string(),
            transactions_count: 1,
            proposals_count: 0,
            uncles_count: 0,
            compact_target: "0x190df964".to_string(),
            difficulty: "1320058941807520729".to_string(),
            miner_address: None,
        };
        let rpc_block = serde_json::json!({
            "header": {"hash": "0xaa", "compact_target": "0x190df964"},
            "transactions": [{"witnesses": []}],
            "proposals": ["0x1234567890abcdef1234"],
            "uncles": [],
        });
        let mut details = vec![];
        compare_block_vs_rpc(&ctx, &ours, &rpc_block, 42, &mut details);
        assert_eq!(
            details,
            vec!["proposals_count: ours=0, rpc=1".to_string()],
            "only the proposals mismatch should be flagged"
        );

        // Matching values produce no findings.
        let rpc_clean = serde_json::json!({
            "header": {"hash": "0xaa", "compact_target": "0x190df964"},
            "transactions": [{"witnesses": []}],
            "proposals": [],
            "uncles": [],
        });
        let mut clean = vec![];
        compare_block_vs_rpc(&ctx, &ours, &rpc_clean, 42, &mut clean);
        assert!(clean.is_empty(), "unexpected findings: {clean:?}");
    }

    #[test]
    fn test_api_checks_registered() {
        let checks = api_checks();
        assert_eq!(checks.len(), 40);
        // Verify names are unique
        let names: Vec<&str> = checks.iter().map(|c| c.name()).collect();
        let unique: std::collections::HashSet<&str> = names.iter().copied().collect();
        assert_eq!(names.len(), unique.len(), "Duplicate check names found");
        assert!(names.contains(&"genesis_baseline_burnt_invariant"));
        assert!(names.contains(&"chart_block_time_distribution_sane"));
        assert!(names.contains(&"chart_epoch_time_distribution_sane"));
        assert!(names.contains(&"chart_epoch_time_length_sane"));
        assert!(names.contains(&"chart_average_block_time_sane"));
        assert!(names.contains(&"chart_miner_distribution_consistency"));
        assert!(names.contains(&"chart_nominal_apc_sane"));
        assert!(names.contains(&"chart_inflation_rate_sane"));
        assert!(names.contains(&"chart_hodl_wave_consistency"));
        assert!(names.contains(&"chart_knowledge_composition_exact"));
        assert!(names.contains(&"secondary_issuance_partition_conservation"));
        assert!(names.contains(&"burnt_supply_genesis_invariant"));
        assert!(names.contains(&"token_activity_transfer_bidirectional"));
        assert!(names.contains(&"spore_owner_roundtrip"));
        assert!(names.contains(&"object_asset_collection_consistency"));
        assert!(names.contains(&"asset_top_holders_address_consistency"));
        assert!(names.contains(&"identity_collection_holder_consistency"));
        assert!(names.contains(&"dao_status_index_matches_deposits"));
        assert!(names.contains(&"participant_rows_consistency"));
        assert!(names.contains(&"global_activity_bucket_index_consistency"));
        assert!(names.contains(&"token_live_cells_consistency"));
        assert!(names.contains(&"collection_daily_activity_consistency"));
        assert!(names.contains(&"address_cellbase_count_consistency"));
        assert!(names.contains(&"dotcell_ring_integrity"));
        assert!(names.contains(&"dotcell_id_is_label_hash"));
        assert!(names.contains(&"dotcell_owner_index_consistency"));
        assert!(names.contains(&"dotcell_records_hash_parity"));
    }

    /// The `.cell` checks are Sampling, and only the one that reads cells and
    /// witnesses back from the node needs RPC.
    #[test]
    fn dotcell_checks_declare_their_tier_and_rpc_need() {
        let checks = api_checks();
        for name in [
            "dotcell_ring_integrity",
            "dotcell_id_is_label_hash",
            "dotcell_owner_index_consistency",
            "dotcell_records_hash_parity",
        ] {
            let check = checks
                .iter()
                .find(|c| c.name() == name)
                .unwrap_or_else(|| panic!("missing check {name}"));
            assert_eq!(check.tier(), CheckTier::Sampling, "{name}");
            assert_eq!(
                check.requires_rpc(),
                name == "dotcell_records_hash_parity",
                "{name}"
            );
        }
    }

    /// `.cell` names live under the protocol's own Account Lock, so the
    /// "an owner holds at least as many live cells as items" premise of
    /// `identity_collection_holder_consistency` does not hold for them.
    /// `dotcell_owner_index_consistency` replaces it.
    #[test]
    fn identity_holder_consistency_does_not_cover_dotcell() {
        let source = include_str!("api_checks.rs");
        let start = source
            .find("impl Check for IdentityCollectionHolderConsistency")
            .expect("the check exists");
        let body = &source[start..start + 2_000];
        assert!(
            !body.contains("dotcell"),
            "identity_collection_holder_consistency must not claim .cell: its premise is that \
             each identity sits in a cell under the owner's own lock, and every .cell name sits \
             under the protocol's Account Lock"
        );
    }

    #[test]
    fn test_check_tiers() {
        let checks = api_checks();
        let fast_count = checks
            .iter()
            .filter(|c| c.tier() == CheckTier::Fast)
            .count();
        let sampling_count = checks
            .iter()
            .filter(|c| c.tier() == CheckTier::Sampling)
            .count();
        assert_eq!(fast_count, 7);
        assert_eq!(sampling_count, 33);
    }

    #[test]
    fn average_block_time_accepts_historical_testnet_stall() {
        let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
        let server = runtime.block_on(MockServer::start());
        runtime.block_on(async {
            Mock::given(method("GET"))
                .and(path("/api/v1/charts/average-block-time"))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                    "data": [{ "date": "20200522", "value": "311.43" }]
                })))
                .mount(&server)
                .await;
        });

        let result = ChartAverageBlockTimeSane
            .run(&mock_ctx(&server), &ProgressReporter::new(None))
            .unwrap();
        assert!(result.passed(), "findings: {:?}", result.findings);
    }

    /// S3 reads three independent endpoints (active-address candidates →
    /// address detail → paginated live cells). Against a live-syncing tip those
    /// reads can straddle a block, so a mismatch is not automatically a bug.
    ///
    /// Mount S3's endpoints with a persistent stored/actual mismatch. `tips`
    /// supplies the network-stats tip for the before and after reads.
    fn network_stats_body(tip: i64) -> serde_json::Value {
        json!({
            "latestBlock": tip,
            "syncStatus": { "isSyncing": true, "syncedBlock": tip, "tipBlock": tip },
            "deepForkStatus": { "detected": false }
        })
    }

    /// S3 brackets itself with the node tip to tell a real mismatch apart from a
    /// straddled block, so its tests must serve `statistics/network`.
    fn mount_static_network_tip(runtime: &tokio::runtime::Runtime, server: &MockServer, tip: i64) {
        runtime.block_on(async {
            Mock::given(method("GET"))
                .and(path("/api/v1/statistics/network"))
                .respond_with(ResponseTemplate::new(200).set_body_json(network_stats_body(tip)))
                .mount(server)
                .await;
        });
    }

    fn mount_address_balance_race_fixture(
        runtime: &tokio::runtime::Runtime,
        server: &MockServer,
        tip_before: i64,
        tip_after: i64,
    ) {
        let lock_hash = format!("0x{}", "aa".repeat(32));
        runtime.block_on(async {
            Mock::given(method("GET"))
                .and(path("/api/v1/statistics/network"))
                .respond_with(
                    ResponseTemplate::new(200).set_body_json(network_stats_body(tip_before)),
                )
                .up_to_n_times(1)
                .with_priority(1)
                .mount(server)
                .await;
            Mock::given(method("GET"))
                .and(path("/api/v1/statistics/network"))
                .respond_with(
                    ResponseTemplate::new(200).set_body_json(network_stats_body(tip_after)),
                )
                .with_priority(2)
                .mount(server)
                .await;

            Mock::given(method("GET"))
                .and(path("/api/v1/addresses/active"))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!([{
                    "lockScriptHash": lock_hash,
                    "liveCellsCount": 2,
                    "transactionsCount": 5
                }])))
                .mount(server)
                .await;

            // Stored state claims 2 live cells / 300 shannons...
            Mock::given(method("GET"))
                .and(path(format!("/api/v1/addresses/{lock_hash}")))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                    "balance": "300",
                    "liveCellsCount": 2
                })))
                .mount(server)
                .await;

            // ...while the live-cell endpoint returns only one 100-shannon cell.
            Mock::given(method("GET"))
                .and(path("/api/v1/cells/live"))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                    "data": [{ "capacity": "100" }],
                    "nextCursor": null
                })))
                .mount(server)
                .await;
        });
    }

    #[test]
    fn address_balance_mismatch_with_static_tip_is_a_hard_failure() {
        let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
        let server = runtime.block_on(MockServer::start());
        mount_address_balance_race_fixture(&runtime, &server, 5_000, 5_000);

        let result = AddressBalanceSpotCheck
            .run(&mock_ctx(&server), &ProgressReporter::new(None))
            .unwrap();

        assert!(
            !result.passed(),
            "a persistent mismatch at a static tip is a real bug"
        );
        assert!(result.items_failed > 0);
    }

    #[test]
    fn address_balance_mismatch_while_tip_advances_is_skipped() {
        let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
        let server = runtime.block_on(MockServer::start());
        mount_address_balance_race_fixture(&runtime, &server, 5_000, 5_004);

        let result = AddressBalanceSpotCheck
            .run(&mock_ctx(&server), &ProgressReporter::new(None))
            .unwrap();

        assert!(
            result.passed(),
            "a mismatch against a moving tip must not be reported as a failure: {:?}",
            result.findings
        );
        let detail = result.detail.unwrap_or_default();
        assert!(
            detail.contains("skipped"),
            "the skip must be noted, not silent: {detail}"
        );
    }

    /// The upper bound exists to catch unit regressions (milliseconds reported
    /// as seconds). Without it the check accepted any finite positive number.
    #[test]
    fn average_block_time_rejects_milliseconds_as_seconds() {
        let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
        let server = runtime.block_on(MockServer::start());
        runtime.block_on(async {
            Mock::given(method("GET"))
                .and(path("/api/v1/charts/average-block-time"))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                    "data": [{ "date": "20260720", "value": "8000" }]
                })))
                .mount(&server)
                .await;
        });

        let result = ChartAverageBlockTimeSane
            .run(&mock_ctx(&server), &ProgressReporter::new(None))
            .unwrap();
        assert!(
            !result.passed(),
            "8000s per block must fail the sanity bound"
        );
    }

    #[test]
    fn cell_count_consistency_allows_live_decline_when_chain_totals_grow() {
        let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
        let server = runtime.block_on(MockServer::start());
        runtime.block_on(async {
            Mock::given(method("GET"))
                .and(path("/api/v1/charts/cell-count"))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                    "data": [
                        { "date": "20260720", "values": { "liveCells": "100", "deadCells": "50" } },
                        { "date": "20260721", "values": { "liveCells": "90", "deadCells": "70" } }
                    ]
                })))
                .mount(&server)
                .await;
        });

        let result = ChartCellCountConsistency
            .run(&mock_ctx(&server), &ProgressReporter::new(None))
            .unwrap();
        assert!(result.passed(), "findings: {:?}", result.findings);
    }

    #[test]
    fn cell_count_consistency_rejects_cumulative_regression() {
        let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
        let server = runtime.block_on(MockServer::start());
        runtime.block_on(async {
            Mock::given(method("GET"))
                .and(path("/api/v1/charts/cell-count"))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                    "data": [
                        { "date": "20260720", "values": { "liveCells": "100", "deadCells": "50" } },
                        { "date": "20260721", "values": { "liveCells": "90", "deadCells": "49" } }
                    ]
                })))
                .mount(&server)
                .await;
        });

        let result = ChartCellCountConsistency
            .run(&mock_ctx(&server), &ProgressReporter::new(None))
            .unwrap();
        assert!(!result.passed());
        assert!(result.findings.iter().any(|finding| finding
            .details
            .iter()
            .any(|detail| detail.contains("cumulative deadCells decreased"))));
        assert!(result.findings.iter().any(|finding| finding
            .details
            .iter()
            .any(|detail| detail.contains("cumulative outputs (live+dead) decreased"))));
    }

    fn mock_ctx(server: &MockServer) -> CheckContext {
        CheckContext {
            network: ckbadger_common::hardfork::NETWORK_MAINNET,
            api_url: format!("{}/api/v1", server.uri()),
            rpc_url: None,
            explorer_url: None,
            http: reqwest::blocking::Client::new(),
            sample_count: 10,
            seed: 42,
            tolerance: 0.001,
            cache_dir: None,
            entities: Vec::new(),
            verify_source_path: None,
            evidence_dir: None,
            entity_budget: Default::default(),
            source_profile: std::sync::Mutex::new(None),
        }
    }

    fn asset_contract_object() -> serde_json::Value {
        json!({
            "id": "0x1234", "assetType": "object", "standard": "m-nft",
            "holdersCount": 0, "totalSupply": "7", "liveCount": 0,
            "activitiesCount": 19
        })
    }

    fn mount_asset_contract(
        runtime: &tokio::runtime::Runtime,
        server: &MockServer,
        object: serde_json::Value,
        items_total: i64,
        holders_total: i64,
    ) {
        runtime.block_on(async {
            for (asset_type, data) in [("token", json!([])), ("object", json!([object]))] {
                Mock::given(method("GET"))
                    .and(path("/api/v1/assets"))
                    .and(query_param("type", asset_type))
                    .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                        "data": data, "total": data.as_array().unwrap().len(), "nextCursor": null
                    })))
                    .mount(server)
                    .await;
            }
            for (endpoint, response) in [
                (
                    "/api/v1/assets/objects/0x1234",
                    json!({
                        "collectionId": "0x1234", "totalCount": 7,
                        "liveCount": 0, "holdersCount": 0, "activitiesCount": 19
                    }),
                ),
                (
                    "/api/v1/assets/objects/0x1234/items",
                    json!({
                        "data": [], "total": items_total, "nextCursor": null
                    }),
                ),
                (
                    "/api/v1/assets/objects/0x1234/holders",
                    json!({
                        "data": [], "total": holders_total, "nextCursor": null
                    }),
                ),
            ] {
                Mock::given(method("GET"))
                    .and(path(endpoint))
                    .respond_with(ResponseTemplate::new(200).set_body_json(response))
                    .mount(server)
                    .await;
            }
        });
    }

    #[test]
    fn asset_contract_object_counts_use_total_supply_and_live_count() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let server = runtime.block_on(MockServer::start());
        mount_asset_contract(&runtime, &server, asset_contract_object(), 7, 0);
        let ctx = mock_ctx(&server);
        let progress = ProgressReporter::new(None);
        let result = ObjectAssetCollectionConsistency
            .run(&ctx, &progress)
            .unwrap();
        assert!(result.passed(), "{:?}", result.findings);
        assert_eq!(result.items_checked, 1);
        let result = AssetTopHoldersAddressConsistency
            .run(&ctx, &progress)
            .unwrap();
        assert!(result.passed(), "{:?}", result.findings);
        assert_eq!(result.items_checked, 0);
    }

    #[test]
    fn asset_contract_object_count_mismatches_are_failures() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let server = runtime.block_on(MockServer::start());
        let mut object = asset_contract_object();
        object["totalSupply"] = json!("9");
        object["liveCount"] = json!(2);
        mount_asset_contract(&runtime, &server, object, 6, 0);
        let result = ObjectAssetCollectionConsistency
            .run(&mock_ctx(&server), &ProgressReporter::new(None))
            .unwrap();
        assert!(!result.passed());
        assert_eq!(result.items_checked, 1);
        for expected in [
            "detail totalCount=7 != list totalSupply=9",
            "detail liveCount=0 != list liveCount=2",
            "items total=6 != detail totalCount=7",
        ] {
            assert!(
                result
                    .findings
                    .iter()
                    .flat_map(|finding| &finding.details)
                    .any(|detail| detail == expected),
                "missing {expected}: {:?}",
                result.findings
            );
        }
    }

    #[test]
    fn asset_contract_required_object_counts_reject_missing_and_malformed_values() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        for (field, value, expected_error) in [
            ("totalSupply", None, "totalSupply"),
            ("totalSupply", Some(json!(null)), "invalid type"),
            ("totalSupply", Some(json!(7)), "invalid type"),
            ("totalSupply", Some(json!("-1")), "totalSupply"),
            ("totalSupply", Some(json!("7.5")), "totalSupply"),
            (
                "totalSupply",
                Some(json!("9223372036854775808")),
                "totalSupply",
            ),
            ("liveCount", None, "liveCount"),
            ("liveCount", Some(json!(null)), "invalid type"),
            ("liveCount", Some(json!("0")), "invalid type"),
            ("liveCount", Some(json!(-1)), "liveCount"),
        ] {
            let server = runtime.block_on(MockServer::start());
            let mut object = asset_contract_object();
            if let Some(value) = value {
                object[field] = value;
            } else {
                object.as_object_mut().unwrap().remove(field);
            }
            mount_asset_contract(&runtime, &server, object, 7, 0);
            let completed = execute_check(
                &ObjectAssetCollectionConsistency,
                &mock_ctx(&server),
                &ProgressReporter::new(None),
            );
            assert_eq!(
                completed.status,
                CheckStatus::Error,
                "{field}: {completed:?}"
            );
            assert!(
                completed
                    .status_reason
                    .as_deref()
                    .unwrap()
                    .contains(expected_error),
                "{completed:?}"
            );
        }
    }

    #[test]
    fn asset_contract_empty_holder_page_does_not_discard_mismatches() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let server = runtime.block_on(MockServer::start());
        let mut object = asset_contract_object();
        object["holdersCount"] = json!(1);
        // Isolate the pre-existing early-Pass bug from the separate old DTO bug:
        // this extra legacy field lets the old consumer reach the result branch.
        object["transfersCount"] = json!(7);
        mount_asset_contract(&runtime, &server, object, 7, 2);
        let result = AssetTopHoldersAddressConsistency
            .run(&mock_ctx(&server), &ProgressReporter::new(None))
            .unwrap();
        assert!(
            !result.passed(),
            "empty holders must not erase count mismatches"
        );
        assert_eq!(result.items_checked, 0);
        for expected in [
            "detail holdersCount=0 != list holdersCount=1",
            "holders total=2 != detail holdersCount=0",
        ] {
            assert!(
                result
                    .findings
                    .iter()
                    .flat_map(|finding| &finding.details)
                    .any(|detail| detail == expected),
                "missing {expected}: {:?}",
                result.findings
            );
        }
    }

    #[test]
    fn asset_contract_object_filter_rejects_identity_rows() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let server = runtime.block_on(MockServer::start());
        let mut object = asset_contract_object();
        object["assetType"] = json!("identity");
        mount_asset_contract(&runtime, &server, object, 7, 0);
        let ctx = mock_ctx(&server);
        let progress = ProgressReporter::new(None);
        for check in [
            &ObjectAssetCollectionConsistency as &dyn Check,
            &AssetTopHoldersAddressConsistency,
        ] {
            let result = check.run(&ctx, &progress).unwrap();
            assert!(!result.passed(), "{}", check.name());
            assert!(
                result
                    .findings
                    .iter()
                    .flat_map(|finding| &finding.details)
                    .any(|detail| detail.contains("unexpected asset type 'identity'")),
                "{:?}",
                result.findings
            );
        }
    }

    fn mount_burnt_charts(
        runtime: &tokio::runtime::Runtime,
        server: &MockServer,
        total_supply_burnt_ckb: &str,
        secondary_burnt_ckb: &str,
    ) {
        mount_burnt_chart_data(
            runtime,
            server,
            json!([{ "date": "2024-01-01", "values": { "burnt": total_supply_burnt_ckb } }]),
            json!([{ "date": "2024-01-01", "values": { "burnt": secondary_burnt_ckb } }]),
        );
    }

    fn mount_burnt_chart_data(
        runtime: &tokio::runtime::Runtime,
        server: &MockServer,
        total_supply_data: serde_json::Value,
        secondary_data: serde_json::Value,
    ) {
        mount_burnt_chart_data_with_baseline(
            runtime,
            server,
            total_supply_data,
            secondary_data,
            "840000000000000000",
        );
    }

    fn mount_burnt_chart_data_with_baseline(
        runtime: &tokio::runtime::Runtime,
        server: &MockServer,
        total_supply_data: serde_json::Value,
        secondary_data: serde_json::Value,
        baseline_burnt: &str,
    ) {
        let total = json!({ "data": total_supply_data });
        let secondary = json!({ "data": secondary_data });
        runtime.block_on(async {
            Mock::given(method("GET"))
                .and(path("/api/v1/dao/statistics"))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                    "genesisBaselineBurnt": baseline_burnt
                })))
                .mount(server)
                .await;
            Mock::given(method("GET"))
                .and(path("/api/v1/charts/total-supply"))
                .respond_with(ResponseTemplate::new(200).set_body_json(total))
                .mount(server)
                .await;
            Mock::given(method("GET"))
                .and(path("/api/v1/charts/secondary-issuance"))
                .respond_with(ResponseTemplate::new(200).set_body_json(secondary))
                .mount(server)
                .await;
        });
    }

    fn mount_secondary_partition_data(
        runtime: &tokio::runtime::Runtime,
        server: &MockServer,
        supply_data: serde_json::Value,
        secondary_data: serde_json::Value,
    ) {
        runtime.block_on(async {
            Mock::given(method("GET"))
                .and(path("/api/v1/charts/total-supply"))
                .respond_with(
                    ResponseTemplate::new(200).set_body_json(json!({"data": supply_data})),
                )
                .mount(server)
                .await;
            Mock::given(method("GET"))
                .and(path("/api/v1/charts/secondary-issuance"))
                .respond_with(
                    ResponseTemplate::new(200).set_body_json(json!({"data": secondary_data})),
                )
                .mount(server)
                .await;
        });
    }

    #[test]
    fn test_genesis_baseline_burnt_invariant_passes_when_gap_is_8_4b() {
        let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
        let server = runtime.block_on(MockServer::start());
        // total burnt - secondary burnt = 8,400,000,000 CKB (the network invariant).
        mount_burnt_charts(&runtime, &server, "8400001000", "1000");

        let ctx = mock_ctx(&server);
        let progress = ProgressReporter::new(None);
        let result = GenesisBaselineBurntInvariant.run(&ctx, &progress).unwrap();
        assert!(
            result.passed(),
            "expected pass, got findings: {:?}",
            result.findings
        );
    }

    #[test]
    fn test_genesis_baseline_burnt_invariant_fails_on_wrong_gap() {
        let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
        let server = runtime.block_on(MockServer::start());
        // Gap is 8,399,999,000 CKB — off by 1000, must fail.
        mount_burnt_charts(&runtime, &server, "8400000000", "1000");

        let ctx = mock_ctx(&server);
        let progress = ProgressReporter::new(None);
        let result = GenesisBaselineBurntInvariant.run(&ctx, &progress).unwrap();
        assert!(!result.passed());
        assert_eq!(result.findings.len(), 1);
        assert!(result.findings[0].details[0].contains("persisted genesis baseline"));

        let sampling = BurntSupplyGenesisInvariant.run(&ctx, &progress).unwrap();
        assert!(!sampling.passed(), "completed-day mismatch must still fail");
        assert_eq!(sampling.items_checked, 1);
        assert_eq!(sampling.findings.len(), 1);
    }

    #[test]
    fn burnt_gap_uses_active_network_baseline_instead_of_mainnet_literal() {
        let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
        let server = runtime.block_on(MockServer::start());
        mount_burnt_chart_data_with_baseline(
            &runtime,
            &server,
            json!([{"date": "2024-01-01", "values": {"burnt": "1.23456789"}}]),
            json!([{"date": "2024-01-01", "values": {"burnt": "1"}}]),
            "23456789",
        );
        let ctx = mock_ctx(&server);
        let progress = ProgressReporter::new(None);
        assert!(GenesisBaselineBurntInvariant
            .run(&ctx, &progress)
            .unwrap()
            .passed());
        assert!(BurntSupplyGenesisInvariant
            .run(&ctx, &progress)
            .unwrap()
            .passed());
    }

    #[test]
    fn burnt_genesis_checks_reject_mismatch_on_wall_current_day() {
        let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
        let server = runtime.block_on(MockServer::start());
        let today = ckbadger_common::now_datetime_utc8().date();
        let yesterday = today - chrono::Duration::days(1);

        mount_burnt_chart_data(
            &runtime,
            &server,
            json!([
                {
                    "date": yesterday.format("%Y-%m-%d").to_string(),
                    "values": { "burnt": "8400001000" }
                },
                {
                    "date": today.format("%Y-%m-%d").to_string(),
                    "values": { "burnt": "8400000698" }
                }
            ]),
            json!([
                {
                    "date": yesterday.format("%Y-%m-%d").to_string(),
                    "values": { "burnt": "1000" }
                },
                {
                    "date": today.format("%Y-%m-%d").to_string(),
                    "values": { "burnt": "1000" }
                }
            ]),
        );

        let ctx = mock_ctx(&server);
        let progress = ProgressReporter::new(None);

        let fast = GenesisBaselineBurntInvariant.run(&ctx, &progress).unwrap();
        assert!(
            !fast.passed(),
            "the wall-current chain day has a wrong exact burnt gap"
        );
        assert!(fast
            .findings
            .iter()
            .flat_map(|finding| &finding.details)
            .any(|detail| detail.contains("persisted genesis baseline")));

        let sampling = BurntSupplyGenesisInvariant.run(&ctx, &progress).unwrap();
        assert!(!sampling.passed());
        assert!(sampling
            .findings
            .iter()
            .flat_map(|finding| &finding.details)
            .any(|detail| detail.contains("persisted genesis baseline")));
    }

    #[test]
    fn chart_dates_do_not_depend_on_verifier_wall_clock() {
        let today = ckbadger_common::now_datetime_utc8()
            .date()
            .format("%Y-%m-%d")
            .to_string();
        let tomorrow = (ckbadger_common::now_datetime_utc8().date() + chrono::Duration::days(1))
            .format("%Y-%m-%d")
            .to_string();
        let mut findings = Vec::new();
        validate_chart_dates(
            &[today.as_str(), tomorrow.as_str()],
            "total-supply",
            &mut findings,
        );
        assert!(
            findings.is_empty(),
            "wall clock rejected chain dates: {findings:?}"
        );
    }

    #[test]
    fn secondary_partition_uses_exact_completed_day_not_newer_live_statistics() {
        let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
        let server = runtime.block_on(MockServer::start());
        runtime.block_on(async {
            Mock::given(method("GET"))
                .and(path("/api/v1/charts/secondary-issuance"))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                    "data": [{
                        "date": "2024-01-01",
                        "values": {"compensation": "1.00000001", "mining": "2.00000002", "burnt": "3.00000003"},
                        "protocolTotalShannons": "600000005",
                        "phase1CompensationShannons": "100000000"
                    }]
                })))
                .mount(&server)
                .await;
            Mock::given(method("GET"))
                .and(path("/api/v1/charts/total-supply"))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": [{"date": "2024-01-01", "values": {"circulating": "1", "nervosdao": "1", "burnt": "8400000000"}}]})))
                .mount(&server)
                .await;
            Mock::given(method("GET"))
                .and(path("/api/v1/dao/statistics"))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                    "totalDeposited": "1000000000", "estimatedApc": "2.0", "activeDeposits": 1,
                    "miningReward": "900000000000", "depositCompensation": "900000000000", "burnt": "900000000000"
                })))
                .mount(&server)
                .await;
        });
        let result = SecondaryIssuancePartitionConservation
            .run(&mock_ctx(&server), &ProgressReporter::new(None))
            .unwrap();
        assert!(
            !result.passed(),
            "one-shannon historical mismatch must fail"
        );
        assert!(result
            .findings
            .iter()
            .any(|finding| finding.entity == "2024-01-01"));
        assert!(
            result
                .findings
                .iter()
                .flat_map(|finding| &finding.details)
                .any(|detail| detail.contains("600000005") && detail.contains("600000006")),
            "must fail at the completed-day protocol total: {:?}",
            result.findings
        );
    }

    #[test]
    fn secondary_partition_accepts_exact_fractional_conservation_without_live_stats() {
        let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
        let server = runtime.block_on(MockServer::start());
        mount_secondary_partition_data(
            &runtime,
            &server,
            json!([{"date": "2024-01-01", "values": {"circulating": "10", "nervosdao": "2", "burnt": "8400000000"}}]),
            json!([{"date": "2024-01-01", "values": {"compensation": "1.00000001", "mining": "2.00000002", "burnt": "3.00000003"}, "protocolTotalShannons": "600000006", "phase1CompensationShannons": "100000000"}]),
        );
        let result = SecondaryIssuancePartitionConservation
            .run(&mock_ctx(&server), &ProgressReporter::new(None))
            .unwrap();
        assert!(
            result.passed(),
            "exact completed-day partition: {:?}",
            result.findings
        );
        assert_eq!(result.items_checked, 1);
    }

    #[test]
    fn secondary_partition_cannot_prove_a_redistribution_with_unchanged_total() {
        let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
        let server = runtime.block_on(MockServer::start());
        mount_secondary_partition_data(
            &runtime,
            &server,
            json!([{"date": "2024-01-01", "values": {"circulating": "10", "nervosdao": "2", "burnt": "8400000000"}}]),
            json!([{"date": "2024-01-01", "values": {"compensation": "0", "mining": "3", "burnt": "3"}, "protocolTotalShannons": "600000000", "phase1CompensationShannons": "0"}]),
        );
        let result = SecondaryIssuancePartitionConservation
            .run(&mock_ctx(&server), &ProgressReporter::new(None))
            .unwrap();
        assert!(
            result.passed(),
            "conservation alone cannot attribute each category: {:?}",
            result.findings
        );
    }

    #[test]
    fn secondary_partition_rejects_phase1_overflow_and_missing_protocol_total() {
        let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
        let server = runtime.block_on(MockServer::start());
        mount_secondary_partition_data(
            &runtime,
            &server,
            json!([{"date": "2024-01-01", "values": {"circulating": "10", "nervosdao": "2", "burnt": "8400000000"}}]),
            json!([{"date": "2024-01-01", "values": {"compensation": "1", "mining": "2", "burnt": "3"}, "protocolTotalShannons": "600000000", "phase1CompensationShannons": "100000001"}]),
        );
        let result = SecondaryIssuancePartitionConservation
            .run(&mock_ctx(&server), &ProgressReporter::new(None))
            .unwrap();
        assert!(!result.passed());
        assert!(result
            .findings
            .iter()
            .flat_map(|finding| &finding.details)
            .any(|detail| detail.contains("phase1CompensationShannons")));
    }

    #[test]
    fn secondary_partition_rejects_missing_required_wire_field() {
        let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
        let server = runtime.block_on(MockServer::start());
        mount_secondary_partition_data(
            &runtime,
            &server,
            json!([{"date": "2024-01-01", "values": {"circulating": "10", "nervosdao": "2", "burnt": "8400000000"}}]),
            json!([{"date": "2024-01-01", "values": {"compensation": "1", "mining": "2", "burnt": "3"}, "phase1CompensationShannons": "0"}]),
        );
        let result = SecondaryIssuancePartitionConservation
            .run(&mock_ctx(&server), &ProgressReporter::new(None))
            .unwrap();
        assert!(!result.passed());
        assert!(result
            .findings
            .iter()
            .flat_map(|finding| &finding.details)
            .any(|detail| detail.contains("protocolTotalShannons")));
    }

    #[test]
    fn secondary_partition_rejects_duplicate_dates() {
        let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
        let server = runtime.block_on(MockServer::start());
        let row = json!({"date": "2024-01-01", "values": {"compensation": "1", "mining": "2", "burnt": "3"}, "protocolTotalShannons": "600000000", "phase1CompensationShannons": "0"});
        mount_secondary_partition_data(
            &runtime,
            &server,
            json!([{"date": "2024-01-01", "values": {"circulating": "10", "nervosdao": "2", "burnt": "8400000000"}}]),
            json!([row.clone(), row]),
        );
        let result = SecondaryIssuancePartitionConservation
            .run(&mock_ctx(&server), &ProgressReporter::new(None))
            .unwrap();
        assert!(!result.passed());
        assert!(result
            .findings
            .iter()
            .flat_map(|finding| &finding.details)
            .any(|detail| detail.contains("not strictly increasing")));
    }

    #[test]
    fn dao_chart_checks_reject_empty_history_when_deposit_history_exists() {
        let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
        let server = runtime.block_on(MockServer::start());
        mount_secondary_partition_data(&runtime, &server, json!([]), json!([]));
        runtime.block_on(async {
            Mock::given(method("GET"))
                .and(path("/api/v1/dao/statistics"))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                    "genesisBaselineBurnt": "840000000000000000"
                })))
                .mount(&server)
                .await;
            Mock::given(method("GET"))
                .and(path("/api/v1/dao/charts/total-deposit"))
                .respond_with(
                    ResponseTemplate::new(200)
                        .set_body_json(json!({"data": [{"date": "2024-01-01", "value": "0"}]})),
                )
                .mount(&server)
                .await;
        });
        let ctx = mock_ctx(&server);
        let progress = ProgressReporter::new(None);
        for check in [
            &SecondaryIssuancePartitionConservation as &dyn Check,
            &GenesisBaselineBurntInvariant,
            &BurntSupplyGenesisInvariant,
        ] {
            let result = check.run(&ctx, &progress).unwrap();
            assert!(
                !result.passed(),
                "{} accepted missing completed history",
                check.name()
            );
            assert!(result
                .findings
                .iter()
                .flat_map(|finding| &finding.details)
                .any(|detail| detail.contains("history is empty")));
        }
    }

    #[test]
    fn fractional_genesis_burnt_gap_fails_by_one_shannon() {
        let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
        let server = runtime.block_on(MockServer::start());
        mount_burnt_charts(&runtime, &server, "8400001000.00000001", "1000.00000002");
        let ctx = mock_ctx(&server);
        let progress = ProgressReporter::new(None);
        assert!(!GenesisBaselineBurntInvariant
            .run(&ctx, &progress)
            .unwrap()
            .passed());
        assert!(!BurntSupplyGenesisInvariant
            .run(&ctx, &progress)
            .unwrap()
            .passed());
    }

    #[test]
    fn burnt_gap_checks_reject_different_completed_date_coverage() {
        let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
        let server = runtime.block_on(MockServer::start());
        mount_burnt_chart_data(
            &runtime,
            &server,
            json!([{"date": "2024-01-01", "values": {"burnt": "8400000001"}}]),
            json!([{"date": "2024-01-02", "values": {"burnt": "1"}}]),
        );
        let ctx = mock_ctx(&server);
        let progress = ProgressReporter::new(None);
        for check in [
            &GenesisBaselineBurntInvariant as &dyn Check,
            &BurntSupplyGenesisInvariant,
        ] {
            let result = check.run(&ctx, &progress).unwrap();
            assert!(
                !result.passed(),
                "{} silently skipped disjoint dates",
                check.name()
            );
            assert!(result
                .findings
                .iter()
                .flat_map(|finding| &finding.details)
                .any(|detail| detail.contains("dates")));
        }
    }

    #[test]
    fn total_supply_monotonic_detects_one_shannon_decrease_above_f64_precision() {
        let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
        let server = runtime.block_on(MockServer::start());
        runtime.block_on(async {
            Mock::given(method("GET"))
                .and(path("/api/v1/charts/total-supply"))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": [
                    {"date": "2024-01-01", "values": {"circulating": "9007199254740992.00000001", "nervosdao": "0", "burnt": "0"}},
                    {"date": "2024-01-02", "values": {"circulating": "9007199254740992", "nervosdao": "0", "burnt": "0"}}
                ]})))
                .mount(&server)
                .await;
        });
        let result = ChartTotalSupplyMonotonic
            .run(&mock_ctx(&server), &ProgressReporter::new(None))
            .unwrap();
        assert!(
            !result.passed(),
            "one-shannon decrease must remain visible above f64 precision"
        );
    }

    #[test]
    fn total_supply_monotonic_rejects_malformed_component_instead_of_default_zero() {
        let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
        let server = runtime.block_on(MockServer::start());
        runtime.block_on(async {
            Mock::given(method("GET"))
                .and(path("/api/v1/charts/total-supply"))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": [
                    {"date": "2024-01-01", "values": {"circulating": "1", "nervosdao": "0", "burnt": "0"}},
                    {"date": "2024-01-02", "values": {"circulating": "broken", "nervosdao": "0", "burnt": "0"}}
                ]})))
                .mount(&server)
                .await;
        });
        let result = ChartTotalSupplyMonotonic
            .run(&mock_ctx(&server), &ProgressReporter::new(None))
            .unwrap();
        assert!(!result.passed());
        assert!(result
            .findings
            .iter()
            .flat_map(|finding| &finding.details)
            .any(|detail| detail.contains("invalid total-supply circulating")));
    }

    #[test]
    fn test_rpc_spot_check_requires_rpc() {
        let check = RpcBlockSpotCheck;
        assert!(check.requires_rpc());
        // Should be skipped when no rpc_url
        let ctx = test_ctx();
        let progress = ProgressReporter::new(None);
        let completed = execute_check(&check, &ctx, &progress);
        assert!(completed.skipped());
    }

    #[test]
    fn test_sync_complete_allows_small_lag_when_not_syncing() {
        let ss = SyncStatus {
            is_syncing: false,
            synced_block: 1_000,
            tip_block: 1_005,
        };

        let findings = sync_complete_findings(&ss);
        assert!(findings.is_empty());
    }

    #[test]
    fn test_sync_complete_fails_when_lag_exceeds_threshold() {
        let ss = SyncStatus {
            is_syncing: false,
            synced_block: 1_000,
            tip_block: 1_101,
        };

        let findings = sync_complete_findings(&ss);
        assert_eq!(findings.len(), 1);
        assert!(findings[0].details[0].contains("lag 101 > 100 blocks"));
    }

    #[test]
    fn test_sync_complete_fails_when_is_syncing_true() {
        let ss = SyncStatus {
            is_syncing: true,
            synced_block: 1_000,
            tip_block: 1_005,
        };

        let findings = sync_complete_findings(&ss);
        assert_eq!(findings.len(), 1);
        assert!(findings[0].details[0].contains("isSyncing=true"));
    }

    struct WarmupPendingThenOk {
        calls: Arc<AtomicUsize>,
    }

    impl Respond for WarmupPendingThenOk {
        fn respond(&self, _request: &Request) -> ResponseTemplate {
            let call = self.calls.fetch_add(1, Ordering::SeqCst);
            if call == 0 {
                ResponseTemplate::new(503).set_body_json(json!({
                    "error": "warmup_pending",
                    "message": "object cache unavailable; warmup in progress"
                }))
            } else {
                ResponseTemplate::new(200).set_body_json(json!({
                    "data": [],
                    "total": 0
                }))
            }
        }
    }

    #[test]
    fn test_api_get_retries_warmup_pending_until_cache_ready() {
        let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
        let server = runtime.block_on(MockServer::start());
        let calls = Arc::new(AtomicUsize::new(0));

        runtime.block_on(async {
            Mock::given(method("GET"))
                .and(path("/api/v1/assets"))
                .respond_with(WarmupPendingThenOk {
                    calls: calls.clone(),
                })
                .mount(&server)
                .await;
        });

        let ctx = CheckContext {
            network: ckbadger_common::hardfork::NETWORK_MAINNET,
            api_url: format!("{}/api/v1", server.uri()),
            rpc_url: None,
            explorer_url: None,
            http: reqwest::blocking::Client::new(),
            sample_count: 10,
            seed: 42,
            tolerance: 0.001,
            cache_dir: None,
            entities: Vec::new(),
            verify_source_path: None,
            evidence_dir: None,
            entity_budget: Default::default(),
            source_profile: std::sync::Mutex::new(None),
        };

        let response: CursorPageWithTotal<serde_json::Value> =
            api_get(&ctx, "assets?type=object&limit=100").expect("api_get should retry warmup");

        assert!(response.data.is_empty());
        assert_eq!(response.total, Some(0));
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    /// Mock responder that returns a pre-computed list of response bodies
    /// in call order. Used to simulate multi-page paginated endpoints.
    struct SequentialPagesResponder {
        calls: Arc<AtomicUsize>,
        pages: Vec<serde_json::Value>,
    }

    impl Respond for SequentialPagesResponder {
        fn respond(&self, _: &Request) -> ResponseTemplate {
            let n = self.calls.fetch_add(1, Ordering::SeqCst);
            let body = self
                .pages
                .get(n)
                .cloned()
                .unwrap_or_else(|| json!({ "data": [], "nextCursor": null }));
            ResponseTemplate::new(200).set_body_json(body)
        }
    }

    /// Serves one page for every call, counting them. Unlike
    /// `SequentialPagesResponder` this is stateless across calls, so a re-scan
    /// sees the same page a real endpoint would return.
    struct RepeatingPageResponder {
        calls: Arc<AtomicUsize>,
        page: serde_json::Value,
    }

    impl Respond for RepeatingPageResponder {
        fn respond(&self, _: &Request) -> ResponseTemplate {
            self.calls.fetch_add(1, Ordering::SeqCst);
            ResponseTemplate::new(200).set_body_json(self.page.clone())
        }
    }

    fn address_balance_candidate(hash_byte: u8, live_cells_count: i64) -> serde_json::Value {
        json!({
            "lockScriptHash": format!("0x{}", format!("{:02x}", hash_byte).repeat(32)),
            "balance": "100",
            "liveCellsCount": live_cells_count,
            "transactionsCount": 1,
            "lastActivityBlock": 1,
        })
    }

    fn address_balance_candidate_record(
        hash_byte: u8,
        live_cells_count: i64,
        transactions_count: i64,
    ) -> AddressCandidateResponse {
        AddressCandidateResponse {
            lock_script_hash: format!("0x{}", format!("{:02x}", hash_byte).repeat(32)),
            live_cells_count,
            transactions_count,
        }
    }

    #[test]
    fn test_select_address_balance_samples_is_deterministic_and_bounded() {
        let candidates = vec![
            address_balance_candidate_record(0x01, 9_714_014, 9_738_668),
            address_balance_candidate_record(0x02, 1, 1),
            address_balance_candidate_record(0x03, 5, 20),
            address_balance_candidate_record(0x04, 50, 100),
            address_balance_candidate_record(0x05, 500, 1_000),
            address_balance_candidate_record(0x06, 900, 2_000),
            address_balance_candidate_record(0x07, 2, 20_000),
            address_balance_candidate_record(0x08, 10, 30),
            address_balance_candidate_record(0x09, 100, 200),
            address_balance_candidate_record(0x0a, 1_000, 3_000),
        ];

        let first = select_address_balance_samples(candidates.clone(), 10, 42).unwrap();
        let second = select_address_balance_samples(candidates, 10, 42).unwrap();
        let first_hashes: Vec<&str> = first
            .iter()
            .map(|candidate| candidate.lock_script_hash.as_str())
            .collect();
        let second_hashes: Vec<&str> = second
            .iter()
            .map(|candidate| candidate.lock_script_hash.as_str())
            .collect();

        assert_eq!(first_hashes, second_hashes);
        assert!(!first_hashes
            .iter()
            .any(|hash| hash.contains(&"01".repeat(32))));
        assert!(!first_hashes
            .iter()
            .any(|hash| hash.contains(&"07".repeat(32))));
        assert!(first.iter().all(|candidate| {
            candidate.live_cells_count <= ADDRESS_BALANCE_MAX_LIVE_CELLS_PER_SAMPLE as i64
                && candidate.transactions_count <= ADDRESS_BALANCE_MAX_TXS_PER_SAMPLE
        }));
        assert!(
            first
                .iter()
                .map(|candidate| candidate.live_cells_count as usize)
                .sum::<usize>()
                <= ADDRESS_BALANCE_MAX_TOTAL_LIVE_CELLS
        );
        assert!(
            first
                .iter()
                .map(|candidate| candidate.transactions_count)
                .sum::<i64>()
                <= ADDRESS_BALANCE_MAX_TOTAL_TXS
        );
    }

    /// Regression: the sampling-tier address check must not expand a whale
    /// address into millions of `/cells/live` records. Candidate discovery may
    /// see the address, but the bounded sampler must select the small address.
    #[test]
    fn test_address_balance_spot_check_skips_unbounded_whale_address() {
        let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
        let server = runtime.block_on(MockServer::start());
        let whale_hash = format!("0x{}", "11".repeat(32));
        let small_hash = format!("0x{}", "22".repeat(32));
        let whale_cell_calls = Arc::new(AtomicUsize::new(0));
        let small_cell_calls = Arc::new(AtomicUsize::new(0));

        runtime.block_on(async {
            let candidates = json!([
                address_balance_candidate(0x11, 9_714_014),
                address_balance_candidate(0x22, 1),
            ]);
            Mock::given(method("GET"))
                .and(path("/api/v1/addresses/top"))
                .respond_with(ResponseTemplate::new(200).set_body_json(candidates.clone()))
                .mount(&server)
                .await;
            Mock::given(method("GET"))
                .and(path("/api/v1/addresses/active"))
                .respond_with(ResponseTemplate::new(200).set_body_json(candidates))
                .mount(&server)
                .await;

            for (hash, live_cells_count) in [(&whale_hash, 9_714_014), (&small_hash, 1)] {
                Mock::given(method("GET"))
                    .and(path(format!("/api/v1/addresses/{}", hash)))
                    .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                        "balance": "100",
                        "liveCellsCount": live_cells_count,
                    })))
                    .mount(&server)
                    .await;
            }

            Mock::given(method("GET"))
                .and(path("/api/v1/cells/live"))
                .and(query_param("lock_script_hash", whale_hash.clone()))
                .respond_with(SequentialPagesResponder {
                    calls: whale_cell_calls.clone(),
                    pages: vec![json!({
                        "data": [{ "capacity": "100" }],
                        "nextCursor": null,
                    })],
                })
                .mount(&server)
                .await;
            Mock::given(method("GET"))
                .and(path("/api/v1/cells/live"))
                .and(query_param("lock_script_hash", small_hash.clone()))
                .respond_with(SequentialPagesResponder {
                    calls: small_cell_calls.clone(),
                    pages: vec![json!({
                        "data": [{ "capacity": "100" }],
                        "nextCursor": null,
                    })],
                })
                .mount(&server)
                .await;
        });

        mount_static_network_tip(&runtime, &server, 5_000);
        let mut ctx = mock_ctx(&server);
        ctx.sample_count = 10;
        let result = AddressBalanceSpotCheck
            .run(&ctx, &ProgressReporter::new(None))
            .expect("address balance check should run");

        assert!(
            result.passed(),
            "unexpected findings: {:?}",
            result.findings
        );
        assert_eq!(result.items_checked, 1);
        assert_eq!(whale_cell_calls.load(Ordering::SeqCst), 0);
        assert_eq!(small_cell_calls.load(Ordering::SeqCst), 1);
    }

    /// Regression: the original check compared only capacity. A stale
    /// `live_cells_count` therefore passed whenever the remaining cells happened
    /// to sum to the stored balance.
    #[test]
    fn test_address_balance_spot_check_detects_live_cell_count_mismatch() {
        let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
        let server = runtime.block_on(MockServer::start());
        let address_hash = format!("0x{}", "33".repeat(32));

        runtime.block_on(async {
            let candidates = json!([address_balance_candidate(0x33, 2)]);
            Mock::given(method("GET"))
                .and(path("/api/v1/addresses/top"))
                .respond_with(ResponseTemplate::new(200).set_body_json(candidates.clone()))
                .mount(&server)
                .await;
            Mock::given(method("GET"))
                .and(path("/api/v1/addresses/active"))
                .respond_with(ResponseTemplate::new(200).set_body_json(candidates))
                .mount(&server)
                .await;
            Mock::given(method("GET"))
                .and(path(format!("/api/v1/addresses/{}", address_hash)))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                    "balance": "100",
                    "liveCellsCount": 2,
                })))
                .mount(&server)
                .await;
            Mock::given(method("GET"))
                .and(path("/api/v1/cells/live"))
                .and(query_param("lock_script_hash", address_hash))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                    "data": [{ "capacity": "100" }],
                    "nextCursor": null,
                })))
                .mount(&server)
                .await;
        });

        mount_static_network_tip(&runtime, &server, 5_000);
        let result = AddressBalanceSpotCheck
            .run(&mock_ctx(&server), &ProgressReporter::new(None))
            .expect("address balance check should run");

        assert!(!result.passed());
        assert!(result.findings[0]
            .details
            .iter()
            .any(|detail| detail.contains("live cells: stored=2, actual=1")));
    }

    /// The resource bound must not trust a corrupt, under-reported count. Once
    /// the endpoint proves there are more cells than declared, the check stops
    /// immediately and reports the invariant violation.
    #[test]
    fn test_address_balance_spot_check_stops_when_actual_cells_exceed_declared_count() {
        let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
        let server = runtime.block_on(MockServer::start());
        let address_hash = format!("0x{}", "44".repeat(32));
        let cell_calls = Arc::new(AtomicUsize::new(0));
        let second_page_calls = Arc::new(AtomicUsize::new(0));

        runtime.block_on(async {
            let candidates = json!([address_balance_candidate(0x44, 1)]);
            Mock::given(method("GET"))
                .and(path("/api/v1/addresses/active"))
                .respond_with(ResponseTemplate::new(200).set_body_json(candidates))
                .mount(&server)
                .await;
            Mock::given(method("GET"))
                .and(path(format!("/api/v1/addresses/{}", address_hash)))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                    "balance": "100",
                    "liveCellsCount": 1,
                })))
                .mount(&server)
                .await;

            let first_page_cells: Vec<_> = (0..ADDRESS_BALANCE_CELL_PAGE_LIMIT)
                .map(|_| json!({ "capacity": "1" }))
                .collect();
            // Cursor-driven pages, like the real endpoint: every fresh scan
            // starts at page 1, so a re-read sees the same first page.
            Mock::given(method("GET"))
                .and(path("/api/v1/cells/live"))
                .and(query_param("lock_script_hash", address_hash.clone()))
                .and(query_param("cursor", "more_cells_exist"))
                .respond_with(RepeatingPageResponder {
                    calls: second_page_calls.clone(),
                    page: json!({
                        "data": [{ "capacity": "1" }],
                        "nextCursor": null,
                    }),
                })
                .with_priority(1)
                .mount(&server)
                .await;
            Mock::given(method("GET"))
                .and(path("/api/v1/cells/live"))
                .and(query_param("lock_script_hash", address_hash))
                .respond_with(RepeatingPageResponder {
                    calls: cell_calls.clone(),
                    page: json!({
                        "data": first_page_cells,
                        "nextCursor": "more_cells_exist",
                    }),
                })
                .with_priority(2)
                .mount(&server)
                .await;
        });

        mount_static_network_tip(&runtime, &server, 5_000);
        let result = AddressBalanceSpotCheck
            .run(&mock_ctx(&server), &ProgressReporter::new(None))
            .expect("address balance check should run");

        assert!(!result.passed());
        // Two attempts (the mismatch triggers exactly one tip-aware re-read),
        // each stopping after its FIRST page: the second page is never fetched.
        assert_eq!(cell_calls.load(Ordering::SeqCst), 2);
        assert_eq!(second_page_calls.load(Ordering::SeqCst), 0);
        assert!(
            result.findings[0]
                .details
                .iter()
                .any(|detail| detail.contains("stored=1, endpoint has more than 100")),
            "findings: {:?}",
            result.findings
        );
    }

    fn decoy_address_tokens_page(count: usize) -> Vec<serde_json::Value> {
        (0..count)
            .map(|i| {
                json!({
                    "typeScriptHash": format!("0x{:064x}", i + 1),
                    "balance": "99999999999999",
                })
            })
            .collect()
    }

    fn address_tokens_ctx(server: &MockServer) -> CheckContext {
        CheckContext {
            network: ckbadger_common::hardfork::NETWORK_MAINNET,
            api_url: format!("{}/api/v1", server.uri()),
            rpc_url: None,
            explorer_url: None,
            http: reqwest::blocking::Client::new(),
            sample_count: 10,
            seed: 42,
            tolerance: 0.001,
            cache_dir: None,
            entities: Vec::new(),
            verify_source_path: None,
            evidence_dir: None,
            entity_budget: Default::default(),
            source_profile: std::sync::Mutex::new(None),
        }
    }

    /// Regression: a top holder of token T may own more distinct UDTs than
    /// fit on a single page of `/addresses/{addr}/tokens` (which is sorted
    /// by raw balance DESC). The check used to fetch only page 1 and then
    /// spuriously report "truncated and missing" whenever the target token
    /// sat beyond position 100 in the holder's own balance ranking.
    /// The fix paginates until the token is found.
    #[test]
    fn test_token_holder_balance_paginates_across_pages_until_found() {
        let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
        let server = runtime.block_on(MockServer::start());
        let calls = Arc::new(AtomicUsize::new(0));

        let holder = "0xdeadbeef";
        let target_hash = "0x8328e4b543901b123b17f4e5f5b5af2a98a3901627feddef21a0a539b3a3fe35";
        let target_balance = "12345";

        let pages = vec![
            json!({
                "data": decoy_address_tokens_page(100),
                "nextCursor": "cursor_to_page_2",
            }),
            json!({
                "data": [
                    {
                        "typeScriptHash": target_hash,
                        "balance": target_balance,
                    }
                ],
                "nextCursor": null,
            }),
        ];

        runtime.block_on(async {
            Mock::given(method("GET"))
                .and(path(format!("/api/v1/addresses/{}/tokens", holder)))
                .respond_with(SequentialPagesResponder {
                    calls: calls.clone(),
                    pages,
                })
                .mount(&server)
                .await;
        });

        let ctx = address_tokens_ctx(&server);
        let details =
            token_holder_balance_mismatch_details(&ctx, target_hash, holder, target_balance)
                .expect("no transport error");
        assert!(
            details.is_empty(),
            "expected no findings when token is found on a later page, got: {:?}",
            details
        );
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    /// The check must still flag a truly missing token after exhausting all
    /// pages (next_cursor = null). This guards against the fix silently
    /// masking a real indexer inconsistency between
    /// `token_holders_by_balance` and `addr_tokens_by_balance`.
    #[test]
    fn test_token_holder_balance_reports_missing_after_full_scan() {
        let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
        let server = runtime.block_on(MockServer::start());
        let calls = Arc::new(AtomicUsize::new(0));

        let holder = "0xcafef00d";
        let target_hash = "0x8328e4b543901b123b17f4e5f5b5af2a98a3901627feddef21a0a539b3a3fe35";

        let pages = vec![
            json!({
                "data": decoy_address_tokens_page(100),
                "nextCursor": "c1",
            }),
            json!({
                "data": decoy_address_tokens_page(50),
                "nextCursor": null,
            }),
        ];

        runtime.block_on(async {
            Mock::given(method("GET"))
                .and(path(format!("/api/v1/addresses/{}/tokens", holder)))
                .respond_with(SequentialPagesResponder {
                    calls: calls.clone(),
                    pages,
                })
                .mount(&server)
                .await;
        });

        let ctx = address_tokens_ctx(&server);
        let details = token_holder_balance_mismatch_details(&ctx, target_hash, holder, "1000")
            .expect("no transport error");
        assert_eq!(details.len(), 1);
        assert!(
            details[0].contains("missing token") && details[0].contains("after scanning 2 page(s)"),
            "unexpected details: {:?}",
            details
        );
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    /// When the target token is found on a later page but its balance does
    /// not match the value reported by the token holders list, the check
    /// must still flag the mismatch.
    #[test]
    fn test_token_holder_balance_detects_mismatch_on_later_page() {
        let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
        let server = runtime.block_on(MockServer::start());
        let calls = Arc::new(AtomicUsize::new(0));

        let holder = "0xbeefcafe";
        let target_hash = "0x8328e4b543901b123b17f4e5f5b5af2a98a3901627feddef21a0a539b3a3fe35";

        let pages = vec![
            json!({
                "data": decoy_address_tokens_page(100),
                "nextCursor": "c1",
            }),
            json!({
                "data": [
                    {
                        "typeScriptHash": target_hash,
                        "balance": "500",
                    }
                ],
                "nextCursor": null,
            }),
        ];

        runtime.block_on(async {
            Mock::given(method("GET"))
                .and(path(format!("/api/v1/addresses/{}/tokens", holder)))
                .respond_with(SequentialPagesResponder {
                    calls: calls.clone(),
                    pages,
                })
                .mount(&server)
                .await;
        });

        let ctx = address_tokens_ctx(&server);
        let details = token_holder_balance_mismatch_details(&ctx, target_hash, holder, "1000")
            .expect("no transport error");
        assert_eq!(details.len(), 1);
        assert!(
            details[0].contains("token balance mismatch")
                && details[0].contains("holders=1000")
                && details[0].contains("address_tokens=500"),
            "unexpected details: {:?}",
            details
        );
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn test_validate_required_holder_count_missing_is_error() {
        let values = std::collections::HashMap::new();
        assert_eq!(
            validate_required_holder_count(&values),
            Some("missing holderCount".to_string())
        );
    }

    #[test]
    fn test_validate_required_holder_count_invalid_is_error() {
        let mut values = std::collections::HashMap::new();
        values.insert("holderCount".to_string(), "not-a-number".to_string());
        assert_eq!(
            validate_required_holder_count(&values),
            Some("invalid holderCount 'not-a-number'".to_string())
        );
    }

    #[test]
    fn test_validate_required_holder_count_valid_is_ok() {
        let mut values = std::collections::HashMap::new();
        values.insert("holderCount".to_string(), "42".to_string());
        assert_eq!(validate_required_holder_count(&values), None);
    }

    #[test]
    fn test_parse_ckb_to_shannons_parses_exact_values() {
        assert_eq!(parse_ckb_to_shannons("1"), Some(100_000_000));
        assert_eq!(parse_ckb_to_shannons("1.5"), Some(150_000_000));
        assert_eq!(parse_ckb_to_shannons("1.00000001"), Some(100_000_001));
        assert_eq!(parse_ckb_to_shannons("0"), Some(0));
    }

    #[test]
    fn test_parse_ckb_to_shannons_rejects_invalid_values() {
        assert_eq!(parse_ckb_to_shannons(""), None);
        assert_eq!(parse_ckb_to_shannons("-1"), None);
        assert_eq!(parse_ckb_to_shannons("1.123456789"), None);
        assert_eq!(parse_ckb_to_shannons("1.2.3"), None);
        assert_eq!(parse_ckb_to_shannons("abc"), None);
    }

    #[test]
    fn test_exact_ckb_parser_rejects_overflow_instead_of_rounding() {
        assert_eq!(parse_ckb_to_shannons("1.49999999"), Some(149_999_999));
        assert_eq!(
            parse_ckb_to_shannons("170141183460469231731687303715884105727"),
            None
        );
    }

    #[test]
    fn test_sampling_tip_from_stats_uses_smallest_non_negative_tip() {
        let stats = NetworkStats {
            latest_block: 120,
            sync_status: SyncStatus {
                is_syncing: false,
                synced_block: 100,
                tip_block: 105,
            },
            deep_fork_status: DeepForkStatus { detected: false },
        };
        assert_eq!(sampling_tip_from_stats(&stats).unwrap(), 100);
        let empty = NetworkStats {
            latest_block: -1,
            sync_status: SyncStatus {
                is_syncing: true,
                synced_block: -1,
                tip_block: -1,
            },
            deep_fork_status: DeepForkStatus { detected: false },
        };
        assert!(sampling_tip_from_stats(&empty)
            .unwrap_err()
            .to_string()
            .contains("before genesis"));
    }

    #[test]
    fn block_response_requires_all_reported_chain_fields() {
        let complete = serde_json::json!({
            "number": 1,
            "hash": "0x01",
            "parentHash": "0x00",
            "transactionsCount": 0,
            "proposalsCount": 0,
            "unclesCount": 0,
            "compactTarget": "0x190df964",
            "difficulty": "1"
        });
        assert!(serde_json::from_value::<BlockResponse>(complete.clone()).is_ok());
        for field in [
            "proposalsCount",
            "unclesCount",
            "compactTarget",
            "difficulty",
        ] {
            let mut missing = complete.clone();
            missing.as_object_mut().unwrap().remove(field);
            assert!(
                serde_json::from_value::<BlockResponse>(missing).is_err(),
                "missing {field} must be a response error"
            );
        }
    }

    #[test]
    fn dotcell_id_check_requires_the_name_it_hashes() {
        assert_eq!(
            required_dotcell_label(Some("alice.cell"), "0x01").unwrap(),
            "alice"
        );
        let error = required_dotcell_label(None, "0x01")
            .unwrap_err()
            .to_string();
        assert!(error.contains("0x01") && error.contains("name"), "{error}");
    }

    #[test]
    fn test_extract_activity_token_deltas_skips_non_token_changes() {
        let activity = AddressActivityRecord {
            tx_hash: "0xabc".to_string(),
            block_number: 123,
            item_deltas: vec![
                serde_json::json!({
                    "kind": "token",
                    "typeScriptHash": "0xAABB",
                    "delta": "-10"
                }),
                serde_json::json!({
                    "kind": "object",
                    "objectId": "0x1234",
                    "delta": 1
                }),
                serde_json::json!({
                    "kind": "token",
                    "typeScriptHash": "0xccdd",
                    "delta": "25"
                }),
            ],
        };

        let parsed = extract_activity_token_deltas(&activity).unwrap();
        assert_eq!(
            parsed,
            vec![
                ("aabb".to_string(), (10u128, true)),
                ("ccdd".to_string(), (25u128, false)),
            ]
        );
    }

    #[test]
    fn test_extract_activity_token_deltas_requires_delta_field() {
        let activity = AddressActivityRecord {
            tx_hash: "0xabc".to_string(),
            block_number: 123,
            item_deltas: vec![serde_json::json!({
                "kind": "token",
                "typeScriptHash": "0xAABB"
            })],
        };

        let err = extract_activity_token_deltas(&activity)
            .unwrap_err()
            .to_string();
        assert!(err.contains("missing delta"));
        assert!(err.contains("0xabc"));
    }

    #[test]
    fn test_extract_activity_token_deltas_skips_missing_type_script_hash() {
        let activity = AddressActivityRecord {
            tx_hash: "0xabc".to_string(),
            block_number: 123,
            item_deltas: vec![serde_json::json!({
                "kind": "token",
                "delta": "10"
            })],
        };

        let parsed = extract_activity_token_deltas(&activity).unwrap();
        assert!(parsed.is_empty());
    }

    #[test]
    fn test_apply_transfer_delta_to_lookup_handles_transfer_mint_and_burn() {
        let mut lookup = std::collections::HashMap::<(String, String, String), (u128, u128)>::new();

        let transfer = TokenTransferApiRecord {
            tx_hash: "0x01".to_string(),
            block_number: 10,
            from_lock_hash: Some("0xaaaa".to_string()),
            to_lock_hash: "0xbbbb".to_string(),
            amount: "100".to_string(),
        };
        apply_transfer_delta_to_lookup(&mut lookup, "0xTT", &transfer).unwrap();

        let mint = TokenTransferApiRecord {
            tx_hash: "0x01".to_string(),
            block_number: 10,
            from_lock_hash: None,
            to_lock_hash: "0xbbbb".to_string(),
            amount: "50".to_string(),
        };
        apply_transfer_delta_to_lookup(&mut lookup, "0xTT", &mint).unwrap();

        let burn = TokenTransferApiRecord {
            tx_hash: "0x01".to_string(),
            block_number: 10,
            from_lock_hash: Some("0xbbbb".to_string()),
            to_lock_hash: "0x".to_string(),
            amount: "20".to_string(),
        };
        apply_transfer_delta_to_lookup(&mut lookup, "0xTT", &burn).unwrap();

        // aaaa: sent 100 -> (received 0, sent 100), net -100.
        assert_eq!(
            lookup
                .get(&("tt".to_string(), "01".to_string(), "aaaa".to_string()))
                .copied(),
            Some((0u128, 100u128))
        );
        // bbbb: received 100+50=150, sent 20 -> net +130.
        assert_eq!(
            lookup
                .get(&("tt".to_string(), "01".to_string(), "bbbb".to_string()))
                .copied(),
            Some((150u128, 20u128))
        );
    }

    #[test]
    fn test_apply_transfer_delta_to_lookup_ignores_empty_lock_hashes() {
        let mut lookup = std::collections::HashMap::<(String, String, String), (u128, u128)>::new();
        let transfer = TokenTransferApiRecord {
            tx_hash: "0x01".to_string(),
            block_number: 10,
            from_lock_hash: Some("0x".to_string()),
            to_lock_hash: "0x".to_string(),
            amount: "100".to_string(),
        };

        apply_transfer_delta_to_lookup(&mut lookup, "0xTT", &transfer).unwrap();
        assert!(lookup.is_empty());
    }

    #[test]
    fn test_parse_signed_decimal_and_render() {
        assert_eq!(parse_signed_decimal("-1000", "x").unwrap(), (1000, true));
        assert_eq!(parse_signed_decimal("25", "x").unwrap(), (25, false));
        assert_eq!(parse_signed_decimal("0", "x").unwrap(), (0, false));
        assert_eq!(parse_signed_decimal("-0", "x").unwrap(), (0, false)); // -0 normalizes
        let big = "222044604925031325468940491728862838784"; // 2.22e38 > i128::MAX
        assert_eq!(
            parse_signed_decimal(big, "x").unwrap(),
            (
                222_044_604_925_031_325_468_940_491_728_862_838_784u128,
                false
            )
        );
        assert!(parse_signed_decimal("nope", "x").is_err());
        assert_eq!(signed_decimal_string(1000, true), "-1000");
        assert_eq!(signed_decimal_string(25, false), "25");
        assert_eq!(signed_decimal_string(0, true), "0"); // zero never shows a sign
    }

    #[test]
    fn test_extract_activity_token_deltas_handles_amount_above_i128_max() {
        // A canonical sUDT can have a per-tx net delta > i128::MAX (block 4743232, 2.22e38);
        // the activity delta string must parse without the old i128 error.
        let big = "222044604925031325468940491728862838784";
        let activity = AddressActivityRecord {
            tx_hash: "0xabc".to_string(),
            block_number: 4_743_232,
            item_deltas: vec![
                serde_json::json!({ "kind": "token", "typeScriptHash": "0xDD", "delta": big }),
                serde_json::json!({ "kind": "token", "typeScriptHash": "0xEE", "delta": format!("-{}", big) }),
            ],
        };
        let parsed = extract_activity_token_deltas(&activity).unwrap();
        assert_eq!(
            parsed,
            vec![
                (
                    "dd".to_string(),
                    (
                        222_044_604_925_031_325_468_940_491_728_862_838_784u128,
                        false
                    )
                ),
                (
                    "ee".to_string(),
                    (
                        222_044_604_925_031_325_468_940_491_728_862_838_784u128,
                        true
                    )
                ),
            ]
        );
    }

    #[test]
    fn test_apply_transfer_delta_to_lookup_handles_amount_above_i128_max() {
        let mut lookup = std::collections::HashMap::<(String, String, String), (u128, u128)>::new();
        let mint = TokenTransferApiRecord {
            tx_hash: "0x01".to_string(),
            block_number: 10,
            from_lock_hash: None,
            to_lock_hash: "0xbbbb".to_string(),
            amount: "222044604925031325468940491728862838784".to_string(), // 2.22e38 > i128::MAX
        };
        // Under the old parse_u128_to_i128_strict this errored on `i128::try_from`; now it accumulates.
        apply_transfer_delta_to_lookup(&mut lookup, "0xTT", &mint).unwrap();
        assert_eq!(
            lookup
                .get(&("tt".to_string(), "01".to_string(), "bbbb".to_string()))
                .copied(),
            Some((
                222_044_604_925_031_325_468_940_491_728_862_838_784u128,
                0u128
            ))
        );
    }

    #[test]
    fn test_sample_indices_with_cap_respects_bounds() {
        let indices = sample_indices_with_cap(42, 100, 50, 10);
        assert_eq!(indices.len(), 10);
        assert!(indices.iter().all(|i| *i < 100));
    }

    #[test]
    fn test_sample_indices_with_cap_is_deterministic() {
        let a = sample_indices_with_cap(7, 20, 8, 8);
        let b = sample_indices_with_cap(7, 20, 8, 8);
        assert_eq!(a, b);
    }

    #[test]
    fn test_fetch_address_count_snapshot_accepts_activity_page_without_total() {
        let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
        let server = runtime.block_on(MockServer::start());
        let lock_hash = "0x45da319453058ab9c22b145107070b77ec97269c1816397f4881d727232c049c";

        runtime.block_on(async {
            Mock::given(method("GET"))
                .and(path(format!("/api/v1/addresses/{lock_hash}")))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                    "lockScriptHash": lock_hash,
                    "liveCellsCount": 3,
                    "transactionsCount": 5,
                    "recentActivitiesCount": 5
                })))
                .mount(&server)
                .await;
            Mock::given(method("GET"))
                .and(path(format!("/api/v1/addresses/{lock_hash}/transactions")))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                    "data": [],
                    "total": 5
                })))
                .mount(&server)
                .await;
            Mock::given(method("GET"))
                .and(path(format!("/api/v1/addresses/{lock_hash}/activities")))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                    "data": [],
                    "limit": 1,
                    "hasMore": false,
                    "nextCursor": null
                })))
                .expect(1)
                .mount(&server)
                .await;
            Mock::given(method("GET"))
                .and(path("/api/v1/cells/live"))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                    "data": [],
                    "total": 3
                })))
                .mount(&server)
                .await;
        });

        let snapshot = fetch_address_count_snapshot(&mock_ctx(&server), lock_hash)
            .expect("cursor-only activities response is the declared API contract");

        assert_eq!(snapshot.lock_script_hash, lock_hash);
        assert_eq!(snapshot.transactions_count, 5);
        assert_eq!(snapshot.tx_total, 5);
        assert_eq!(snapshot.live_cells_count, 3);
        assert_eq!(snapshot.live_cell_total, 3);
        runtime.block_on(server.verify());
    }

    #[test]
    fn test_address_count_mismatch_details_empty_when_consistent() {
        let snapshot = AddressCountSnapshot {
            lock_script_hash: "0xabc".to_string(),
            live_cells_count: 3,
            transactions_count: 5,
            recent_activities_count: 5,
            tx_total: 5,
            live_cell_total: 3,
        };

        let details = address_count_mismatch_details("0xabc", &snapshot);
        assert!(details.is_empty());
    }

    #[test]
    fn test_address_count_mismatch_details_reports_mismatches() {
        let snapshot = AddressCountSnapshot {
            lock_script_hash: "0xdef".to_string(),
            live_cells_count: 7,
            transactions_count: 6,
            recent_activities_count: 4,
            tx_total: 3,
            live_cell_total: 9,
        };

        let details = address_count_mismatch_details("0xabc", &snapshot);
        assert!(details
            .iter()
            .any(|line| line.contains("lock_script_hash mismatch")));
        assert!(details
            .iter()
            .any(|line| line.contains("transactionsCount=6 != recentActivitiesCount=4")));
        assert!(details
            .iter()
            .any(|line| line
                .contains("transactions endpoint total=3 != address transactionsCount=6")));
        assert!(details
            .iter()
            .any(|line| line.contains("live cells endpoint total=9 != address liveCellsCount=7")));
    }

    #[test]
    fn test_normalize_hex_key_handles_uppercase_prefix() {
        assert_eq!(normalize_hex_key("0XABcd"), "abcd");
    }

    fn block_body(number: i64, hash: &str) -> serde_json::Value {
        json!({
            "number": number,
            "hash": hash,
            "parentHash": format!("0x{}", "00".repeat(32)),
            "transactionsCount": 0,
            "proposalsCount": 0,
            "unclesCount": 0,
            "compactTarget": "0x190df964",
            "difficulty": "1",
        })
    }

    /// An anchor that moved mid-walk is `Inconclusive`, whether or not the walk
    /// produced findings. The previous shape returned a *green* result whose
    /// detail line began with the word INCONCLUSIVE, so a run over a reorging
    /// chain exited 0 and counted as a passed check.
    #[test]
    fn dao_status_index_reports_an_anchor_change_as_inconclusive_not_pass() {
        let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
        let server = runtime.block_on(MockServer::start());
        let before = format!("0x{}", "11".repeat(32));
        let after = format!("0x{}", "22".repeat(32));

        runtime.block_on(async {
            Mock::given(method("GET"))
                .and(path("/api/v1/statistics/network"))
                .respond_with(ResponseTemplate::new(200).set_body_json(network_stats_body(100)))
                .mount(&server)
                .await;
            Mock::given(method("GET"))
                .and(path("/api/v1/blocks/100"))
                .respond_with(ResponseTemplate::new(200).set_body_json(block_body(100, &before)))
                .up_to_n_times(1)
                .with_priority(1)
                .mount(&server)
                .await;
            Mock::given(method("GET"))
                .and(path("/api/v1/blocks/100"))
                .respond_with(ResponseTemplate::new(200).set_body_json(block_body(100, &after)))
                .with_priority(2)
                .mount(&server)
                .await;
            // No deposits at all: the walk itself finds nothing wrong.
            Mock::given(method("GET"))
                .and(path("/api/v1/dao/deposits"))
                .respond_with(
                    ResponseTemplate::new(200)
                        .set_body_json(json!({ "data": [], "nextCursor": null })),
                )
                .mount(&server)
                .await;
        });

        let result = DaoStatusIndexMatchesDeposits
            .run(&mock_ctx(&server), &ProgressReporter::new(None))
            .expect("the check itself must not error on a reorg");

        assert_eq!(result.status, CheckStatus::Inconclusive);
        assert!(!result.passed(), "a moved anchor proves nothing");
        let detail = result.detail.clone().unwrap_or_default();
        assert!(detail.contains("changed hash during the walk"), "{detail}");
    }

    // ---- `.cell` checks: only a 404 means "no deployment" (review #8) ----

    const DOTCELL_CHECK_NAMES: [&str; 4] = [
        "dotcell_ring_integrity",
        "dotcell_id_is_label_hash",
        "dotcell_owner_index_consistency",
        "dotcell_records_hash_parity",
    ];

    fn registered_check(name: &str) -> Box<dyn Check> {
        api_checks()
            .into_iter()
            .find(|check| check.name() == name)
            .unwrap_or_else(|| panic!("missing check {name}"))
    }

    /// The node and the API are one mock server: RPC posts to `/`.
    fn dotcell_ctx(server: &MockServer) -> CheckContext {
        CheckContext {
            rpc_url: Some(server.uri()),
            ..mock_ctx(server)
        }
    }

    fn mount_dotcell_collection_status(
        runtime: &tokio::runtime::Runtime,
        server: &MockServer,
        status: u16,
    ) {
        runtime.block_on(async {
            Mock::given(method("GET"))
                .and(path("/api/v1/assets/identities/dotcell"))
                .respond_with(ResponseTemplate::new(status).set_body_json(json!({
                    "error": "status",
                    "message": format!("fixture status {status}"),
                })))
                .mount(server)
                .await;
        });
    }

    /// Answers the two node calls the records-parity check makes with the
    /// real creating transaction of mainnet name `support` (M2, output 1).
    struct DotCellNodeResponder;

    impl Respond for DotCellNodeResponder {
        fn respond(&self, request: &Request) -> ResponseTemplate {
            use crate::parser::test_helpers::real_dotcell as fixture;
            let body: serde_json::Value = serde_json::from_slice(&request.body).unwrap();
            let tx = &fixture::M2_REGISTER_SUPPORT;
            let result = match body["method"].as_str().unwrap_or_default() {
                "get_live_cell" => json!({
                    "cell": {"data": {"content": tx.outputs[1].data}},
                    "status": "live",
                }),
                "get_transaction" => json!({
                    "transaction": {"hash": tx.tx_hash, "witnesses": tx.witnesses},
                    "tx_status": {"status": "committed"},
                }),
                other => panic!("unexpected RPC method {other}"),
            };
            ResponseTemplate::new(200)
                .set_body_json(json!({"jsonrpc": "2.0", "id": 1, "result": result}))
        }
    }

    /// One real, internally consistent `.cell` deployment with a single live
    /// name, served by every route the four checks read.
    fn mount_dotcell_single_name(runtime: &tokio::runtime::Runtime, server: &MockServer) {
        use crate::parser::test_helpers::real_dotcell as fixture;
        let tx = &fixture::M2_REGISTER_SUPPORT;
        let data = hex::decode(tx.outputs[1].data.trim_start_matches("0x")).unwrap();
        let name = crate::parser::DotCellParser::parse_name_data(&data).unwrap();
        let id = format!("0x{}", hex::encode(name.id));
        let item = json!({
            "nftId": id,
            "name": format!("{}.cell", name.label),
            "isLive": true,
        });
        let address = "ckb1qfixtureowner";
        runtime.block_on(async {
            let get = |route: &str, body: serde_json::Value| {
                Mock::given(method("GET"))
                    .and(path(format!("/api/v1/{route}")))
                    .respond_with(ResponseTemplate::new(200).set_body_json(body))
            };
            get(
                "assets/identities/dotcell",
                json!({
                    "collectionId": "dotcell",
                    "totalCount": 1,
                    "liveCount": 1,
                    "holdersCount": 1,
                }),
            )
            .mount(server)
            .await;
            get(
                "assets/identities/dotcell/items",
                json!({"data": [item.clone()], "nextCursor": null}),
            )
            .mount(server)
            .await;
            get(
                "assets/identities/dotcell/ring",
                json!({"firstId": id, "liveCount": 1}),
            )
            .mount(server)
            .await;
            get(
                &format!("assets/identities/dotcell/items/{id}"),
                json!({
                    "identityId": id,
                    "label": name.label,
                    "isLive": true,
                    "nextId": format!("0x{}", "00".repeat(20)),
                    "recordsHash": format!("0x{}", hex::encode(name.records_hash)),
                    "liveOutPoint": {"txHash": tx.tx_hash, "index": 1},
                }),
            )
            .mount(server)
            .await;
            get(
                "assets/identities/dotcell/holders",
                json!({
                    "data": [{
                        "ownerHashPrefix": format!("0x{}", hex::encode(name.owner_hash20)),
                        "address": address,
                        "itemCount": 1,
                    }],
                    "nextCursor": null,
                }),
            )
            .mount(server)
            .await;
            get(
                &format!("addresses/{address}/dotcell-names"),
                json!({"data": [{
                    "identityId": id,
                    "label": name.label,
                    "name": format!("{}.cell", name.label),
                    "expiredAt": 1_821_507_678u64,
                }], "nextCursor": null}),
            )
            .mount(server)
            .await;
            Mock::given(method("POST"))
                .and(path("/"))
                .respond_with(DotCellNodeResponder)
                .mount(server)
                .await;
        });
    }

    /// A network with no `.cell` deployment answers 404 on the collection:
    /// that, and only that, is "not applicable".
    #[test]
    fn dotcell_checks_are_not_applicable_on_a_404_collection() {
        for name in DOTCELL_CHECK_NAMES {
            let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
            let server = runtime.block_on(MockServer::start());
            mount_dotcell_collection_status(&runtime, &server, 404);

            let check = registered_check(name);
            let completed = execute_check(
                check.as_ref(),
                &dotcell_ctx(&server),
                &ProgressReporter::new(None),
            );
            assert_eq!(completed.status, CheckStatus::NotApplicable, "{name}");
            let reason = completed.status_reason.unwrap_or_default();
            assert!(reason.contains("404"), "{name}: {reason}");
        }
    }

    /// A 500 on the collection is a broken API, never an absent protocol: the
    /// check must end Error, not a vacuous pass over zero items.
    #[test]
    fn dotcell_checks_error_on_a_500_collection() {
        for name in DOTCELL_CHECK_NAMES {
            let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
            let server = runtime.block_on(MockServer::start());
            mount_dotcell_collection_status(&runtime, &server, 500);

            let check = registered_check(name);
            let completed = execute_check(
                check.as_ref(),
                &dotcell_ctx(&server),
                &ProgressReporter::new(None),
            );
            assert_eq!(completed.status, CheckStatus::Error, "{name}");
            let reason = completed.status_reason.unwrap_or_default();
            assert!(reason.contains("500"), "{name}: {reason}");
        }
    }

    /// A deployment that answers 200 is verified item by item.
    #[test]
    fn dotcell_checks_run_on_a_200_collection() {
        for name in DOTCELL_CHECK_NAMES {
            let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
            let server = runtime.block_on(MockServer::start());
            mount_dotcell_single_name(&runtime, &server);

            let check = registered_check(name);
            let completed = execute_check(
                check.as_ref(),
                &dotcell_ctx(&server),
                &ProgressReporter::new(None),
            );
            let result = completed.result.expect("a completed run has a result");
            assert_eq!(
                completed.status,
                CheckStatus::Pass,
                "{name}: {:?} {:?}",
                completed.status_reason,
                result.findings
            );
            assert_eq!(result.items_checked, 1, "{name}");
        }
    }

    /// Both checks size their sample from `--sample-count`; at 0 they used to
    /// pass over an empty selection. Declaring it hands 0 to the framework,
    /// which refuses the run instead of reporting a vacuous Pass.
    #[test]
    fn sample_count_readers_refuse_a_zero_sample() {
        for name in [
            "participant_rows_consistency",
            "dotcell_records_hash_parity",
        ] {
            let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
            let server = runtime.block_on(MockServer::start());
            mount_dotcell_single_name(&runtime, &server);
            let ctx = CheckContext {
                sample_count: 0,
                ..dotcell_ctx(&server)
            };
            let check = registered_check(name);
            assert!(check.requires_sampling(), "{name}");
            let completed = execute_check(check.as_ref(), &ctx, &ProgressReporter::new(None));
            assert_eq!(completed.status, CheckStatus::Error, "{name}");
            assert!(
                completed
                    .status_reason
                    .unwrap_or_default()
                    .contains("--sample-count 0"),
                "{name}"
            );
        }
    }

    /// An owner holding more names than one `dotcell-names` page must be read
    /// to the end before its list is compared with its counter. The address
    /// endpoint returns `identityId`, `label`, `name`, and `expiredAt`, not the
    /// collection items' `nftId` and `isLive` fields.
    #[test]
    fn dotcell_owner_index_pages_through_an_owners_names() {
        const NAMES: usize = 101;
        let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
        let server = runtime.block_on(MockServer::start());
        let address = "ckb1qfixturebigowner";
        let item = |i: usize| {
            json!({
                "identityId": format!("0x{i:040x}"),
                "label": format!("name{i}"),
                "name": format!("name{i}.cell"),
                "expiredAt": 1_821_507_678u64,
            })
        };
        let first_page: Vec<_> = (0..100).map(item).collect();
        let second_page: Vec<_> = (100..NAMES).map(item).collect();
        let cursor = format!("0x{:040x}", 99);
        runtime.block_on(async {
            Mock::given(method("GET"))
                .and(path("/api/v1/assets/identities/dotcell"))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                    "collectionId": "dotcell",
                    "totalCount": NAMES,
                    "liveCount": NAMES,
                    "holdersCount": 1,
                })))
                .mount(&server)
                .await;
            Mock::given(method("GET"))
                .and(path("/api/v1/assets/identities/dotcell/holders"))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                    "data": [{
                        "ownerHashPrefix": format!("0x{}", "ab".repeat(20)),
                        "address": address,
                        "itemCount": NAMES,
                    }],
                    "nextCursor": null,
                })))
                .mount(&server)
                .await;
            Mock::given(method("GET"))
                .and(path(format!("/api/v1/addresses/{address}/dotcell-names")))
                .and(query_param("cursor", cursor.as_str()))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                    "data": second_page,
                    "nextCursor": null,
                })))
                .with_priority(1)
                .mount(&server)
                .await;
            Mock::given(method("GET"))
                .and(path(format!("/api/v1/addresses/{address}/dotcell-names")))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                    "data": first_page,
                    "nextCursor": cursor,
                })))
                .mount(&server)
                .await;
        });

        let result = registered_check("dotcell_owner_index_consistency")
            .run(&dotcell_ctx(&server), &ProgressReporter::new(None))
            .unwrap();
        assert_eq!(
            result.status,
            CheckStatus::Pass,
            "an owner's second page of names must be read: {:?}",
            result.findings
        );
    }

    #[test]
    fn dotcell_owner_index_rejects_a_wrong_count_or_malformed_name() {
        for (data, expected_status) in [
            (json!([]), CheckStatus::Fail),
            (json!([{"name": "support.cell"}]), CheckStatus::Error),
        ] {
            let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
            let server = runtime.block_on(MockServer::start());
            mount_dotcell_single_name(&runtime, &server);
            runtime.block_on(async {
                Mock::given(method("GET"))
                    .and(path("/api/v1/addresses/ckb1qfixtureowner/dotcell-names"))
                    .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                        "data": data,
                        "nextCursor": null,
                    })))
                    .with_priority(1)
                    .mount(&server)
                    .await;
            });

            let check = registered_check("dotcell_owner_index_consistency");
            let completed = execute_check(
                check.as_ref(),
                &dotcell_ctx(&server),
                &ProgressReporter::new(None),
            );
            assert_eq!(completed.status, expected_status);
            if expected_status == CheckStatus::Fail {
                let result = completed.result.unwrap();
                assert!(
                    result.findings[0].details[0].contains("address lists 0 names, counter says 1")
                );
            } else {
                assert!(completed.status_reason.unwrap().contains("identityId"));
            }
        }
    }

    // ── address_cellbase_count_consistency ─────────────────────────────────

    const CELLBASE_MINER: &str = "ckb1qcellbaseminer";

    fn cellbase_hash() -> String {
        format!("0x{}", "cb".repeat(32))
    }

    fn address_tx_row(byte: u8, block: Option<i64>, is_cellbase: bool) -> serde_json::Value {
        json!({
            "txHash": format!("0x{}", format!("{byte:02x}").repeat(32)),
            "blockNumber": block,
            "isCellbase": is_cellbase,
        })
    }

    /// Every sampled block (tip 100) was mined by `CELLBASE_MINER`, whose
    /// reward is paid at block 50 by `cellbase_hash()`. Returns the call
    /// counter of the miner's full (cursor-less) transaction listing.
    fn mount_cellbase_count_fixture(
        runtime: &tokio::runtime::Runtime,
        server: &MockServer,
        transactions_count: i64,
        activities_total: i64,
        rows_at_paid_block: serde_json::Value,
        full_listing: serde_json::Value,
    ) -> Arc<AtomicUsize> {
        use wiremock::matchers::{path_regex, query_param_is_missing};
        let full_listing_calls = Arc::new(AtomicUsize::new(0));
        let base = format!("/api/v1/addresses/{CELLBASE_MINER}");
        runtime.block_on(async {
            Mock::given(method("GET"))
                .and(path_regex(r"^/api/v1/blocks/[0-9]+$"))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                    "minerAddress": CELLBASE_MINER,
                    "miningRewardTxHash": cellbase_hash(),
                    "rewardPaidAtBlock": 50,
                })))
                .mount(server)
                .await;
            Mock::given(method("GET"))
                .and(path(format!("{base}/transactions")))
                .and(query_param("cursor", "51:0"))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                    "data": rows_at_paid_block,
                    "nextCursor": null,
                })))
                .mount(server)
                .await;
            Mock::given(method("GET"))
                .and(path(format!("{base}/transactions")))
                .and(query_param_is_missing("cursor"))
                .respond_with(RepeatingPageResponder {
                    calls: full_listing_calls.clone(),
                    page: json!({ "data": full_listing, "nextCursor": null }),
                })
                .mount(server)
                .await;
            Mock::given(method("GET"))
                .and(path(base.clone()))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                    "lockScriptHash": format!("0x{}", "aa".repeat(32)),
                    "liveCellsCount": 1,
                    "transactionsCount": transactions_count,
                    "recentActivitiesCount": transactions_count,
                })))
                .mount(server)
                .await;
            Mock::given(method("GET"))
                .and(path(format!("{base}/activities")))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                    "data": [],
                    "total": activities_total,
                    "nextCursor": null,
                })))
                .mount(server)
                .await;
        });
        mount_static_network_tip(runtime, server, 100);
        full_listing_calls
    }

    /// A bounded miner whose activity total leaves exactly its cellbase rows:
    /// three committed transactions, two of them cellbases (one the sampled
    /// reward), one activity. The page-one pool row is not a committed row.
    fn bounded_miner_rows() -> (serde_json::Value, serde_json::Value) {
        let at_paid = json!([address_tx_row(0x45, Some(50), false), {
            "txHash": cellbase_hash(), "blockNumber": 50, "isCellbase": true,
        }]);
        let full = json!([
            address_tx_row(0xee, None, false),
            address_tx_row(0x45, Some(50), false),
            { "txHash": cellbase_hash(), "blockNumber": 50, "isCellbase": true },
            address_tx_row(0x40, Some(40), true),
        ]);
        (at_paid, full)
    }

    #[test]
    fn cellbase_count_check_passes_when_the_counter_equals_the_cellbase_rows() {
        let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
        let server = runtime.block_on(MockServer::start());
        let (at_paid, full) = bounded_miner_rows();
        let calls = mount_cellbase_count_fixture(&runtime, &server, 3, 1, at_paid, full);
        let mut ctx = mock_ctx(&server);
        ctx.sample_count = 1;
        let result = AddressCellbaseCountConsistency
            .run(&ctx, &ProgressReporter::new(None))
            .expect("check runs");
        assert!(result.passed(), "{:?}", result.findings);
        assert_eq!(result.items_checked, 1);
        assert!(
            result
                .detail
                .as_deref()
                .is_some_and(|d| d.contains("1 miners counted exactly")),
            "{:?}",
            result.detail
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn cellbase_count_check_flags_a_counter_that_disagrees_with_the_cellbase_rows() {
        let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
        let server = runtime.block_on(MockServer::start());
        let (at_paid, full) = bounded_miner_rows();
        // Activity total 2 declares one cellbase; the index holds two.
        mount_cellbase_count_fixture(&runtime, &server, 3, 2, at_paid, full);
        let mut ctx = mock_ctx(&server);
        ctx.sample_count = 1;
        let result = AddressCellbaseCountConsistency
            .run(&ctx, &ProgressReporter::new(None))
            .expect("check runs");
        assert!(!result.passed());
        assert!(
            result.findings[0].details.iter().any(|d| d.contains(
                "persisted cellbase count 1 (transactionsCount 3 - activities total 2) != 2 cellbase rows"
            )),
            "{:?}",
            result.findings
        );
    }

    #[test]
    fn cellbase_count_check_flags_a_missing_reward_row_without_paging_a_large_miner() {
        let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
        let server = runtime.block_on(MockServer::start());
        // The listing at the paying block holds no cellbase: it jumps to block 49.
        let at_paid = json!([address_tx_row(0x49, Some(49), true)]);
        let calls =
            mount_cellbase_count_fixture(&runtime, &server, 4_727_769, 1_682, at_paid, json!([]));
        let mut ctx = mock_ctx(&server);
        ctx.sample_count = 1;
        let result = AddressCellbaseCountConsistency
            .run(&ctx, &ProgressReporter::new(None))
            .expect("check runs");
        assert!(!result.passed());
        assert!(
            result.findings[0]
                .details
                .iter()
                .any(|d| d.contains("is not a cellbase row of its miner's transaction index")),
            "{:?}",
            result.findings
        );
        assert_eq!(
            calls.load(Ordering::SeqCst),
            0,
            "a miner above the bound is never paged in full"
        );
    }

    // ── token_live_cells_consistency ──────────────────────────────────────

    /// Serves `/cells/live?type_script_hash=…` for one token: `cells` rows of
    /// `(tx byte, type script hash)`, paged by `limit` with an index cursor.
    struct FakeTypeIndex {
        cells: Vec<(u8, String)>,
    }

    impl Respond for FakeTypeIndex {
        fn respond(&self, request: &Request) -> ResponseTemplate {
            let query: HashMap<String, String> = request.url.query_pairs().into_owned().collect();
            let limit: usize = query["limit"].parse().unwrap();
            let start: usize = query.get("cursor").map_or(0, |c| c.parse().unwrap());
            let page: Vec<serde_json::Value> = self
                .cells
                .iter()
                .skip(start)
                .take(limit)
                .map(|(tx, type_hash)| {
                    json!({ "txHash": format!("0x{}", hex::encode([*tx; 32])), "outputIndex": 0,
                            "capacity": "100", "typeScriptHash": type_hash })
                })
                .collect();
            let next = (start + limit < self.cells.len()).then(|| (start + limit).to_string());
            ResponseTemplate::new(200).set_body_json(json!({ "data": page, "nextCursor": next }))
        }
    }

    /// One token whose persisted count is `stored` and whose type index lists
    /// `cells` (count, then any row whose type differs).
    fn run_token_live_cells_check(stored: i64, cells: Vec<(u8, String)>) -> CompletedCheck {
        let token = format!("0x{}", "71".repeat(32));
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let server = runtime.block_on(MockServer::start());
        runtime.block_on(async {
            Mock::given(method("GET"))
                .and(path("/api/v1/tokens"))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                    "data": [{ "typeScriptHash": token, "cellsCount": stored }],
                    "nextCursor": null
                })))
                .mount(&server)
                .await;
            Mock::given(method("GET"))
                .and(path(format!("/api/v1/tokens/{token}")))
                .respond_with(
                    ResponseTemplate::new(200)
                        .set_body_json(json!({ "typeScriptHash": token, "cellsCount": stored })),
                )
                .mount(&server)
                .await;
            Mock::given(method("GET"))
                .and(path("/api/v1/cells/live"))
                .and(query_param("type_script_hash", token.as_str()))
                .respond_with(FakeTypeIndex { cells })
                .mount(&server)
                .await;
        });
        execute_check(
            registered_check("token_live_cells_consistency").as_ref(),
            &mock_ctx(&server),
            &ProgressReporter::new(None),
        )
    }

    fn token_cells(count: u8) -> Vec<(u8, String)> {
        (0..count)
            .map(|tx| (tx, format!("0x{}", "71".repeat(32))))
            .collect()
    }

    #[test]
    fn token_live_cells_check_passes_when_the_count_matches_the_type_index() {
        // 250 cells span three pages of the listing.
        let completed = run_token_live_cells_check(250, token_cells(250));
        let result = completed.result.expect("a completed run has a result");
        assert_eq!(completed.status, CheckStatus::Pass, "{:?}", result.findings);
        assert_eq!(result.items_checked, 1);
    }

    #[test]
    fn token_live_cells_check_fails_naming_the_token_on_a_drifted_count_or_a_foreign_row() {
        let token = format!("0x{}", "71".repeat(32));
        // The persisted count says 3; the index lists two of the token's cells.
        let completed = run_token_live_cells_check(3, token_cells(2));
        assert_eq!(completed.status, CheckStatus::Fail);
        let findings = completed.result.expect("result").findings;
        assert_eq!(findings[0].entity, format!("token={token}"));
        assert!(
            findings[0]
                .details
                .iter()
                .any(|d| d == "persisted cellsCount=3 != live cells behind the type index=2"),
            "{:?}",
            findings[0].details
        );

        // A listed row whose payload carries another type script.
        let mut cells = token_cells(2);
        cells[1].1 = format!("0x{}", "55".repeat(32));
        let completed = run_token_live_cells_check(2, cells);
        assert_eq!(completed.status, CheckStatus::Fail);
        let findings = completed.result.expect("result").findings;
        assert!(
            findings[0]
                .details
                .iter()
                .any(|d| d.starts_with("type index lists cell 0x0101") && d.contains("5555")),
            "{:?}",
            findings[0].details
        );
    }

    // ── collection_daily_activity_consistency ─────────────────────────────

    /// Serves `/assets/objects/{id}/activities` for one collection: rows of
    /// `(chainTime ms, actions)`, newest first, paged by `limit`.
    struct FakeCollectionRows {
        rows: Vec<(i64, Vec<&'static str>)>,
    }

    impl Respond for FakeCollectionRows {
        fn respond(&self, request: &Request) -> ResponseTemplate {
            let query: HashMap<String, String> = request.url.query_pairs().into_owned().collect();
            let limit: usize = query["limit"].parse().unwrap();
            let start: usize = query.get("cursor").map_or(0, |c| c.parse().unwrap());
            let page: Vec<serde_json::Value> = self
                .rows
                .iter()
                .enumerate()
                .skip(start)
                .take(limit)
                .map(|(index, (chain_time, actions))| {
                    json!({ "txHash": format!("0x{index:064x}"), "blockNumber": index,
                            "txIndex": 0, "timestamp": chain_time.to_string(),
                            "chainTime": chain_time.to_string(), "actions": actions })
                })
                .collect();
            let next = (start + limit < self.rows.len()).then(|| (start + limit).to_string());
            ResponseTemplate::new(200).set_body_json(json!({ "data": page, "nextCursor": next }))
        }
    }

    /// UTC midnight of `day` plus 4 h: well inside that UTC+8 stats day.
    fn utc8_noonish(day: &str) -> i64 {
        chrono::NaiveDate::parse_from_str(day, "%Y-%m-%d")
            .unwrap()
            .and_hms_opt(4, 0, 0)
            .unwrap()
            .and_utc()
            .timestamp_millis()
    }

    /// One m-NFT class whose calendar serves `weekly` through 2026-10-25 and
    /// whose activity list holds `rows`; `activities_count` on the list row
    /// and the detail.
    fn run_collection_daily_check(
        weekly: serde_json::Value,
        rows: Vec<(i64, Vec<&'static str>)>,
        activities_count: i64,
    ) -> CompletedCheck {
        let id = format!("0x{}", "3c".repeat(24));
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let server = runtime.block_on(MockServer::start());
        runtime.block_on(async {
            Mock::given(method("GET"))
                .and(path("/api/v1/assets"))
                .and(query_param("type", "object"))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                    "data": [{ "id": id, "activitiesCount": activities_count }],
                    "nextCursor": null
                })))
                .mount(&server)
                .await;
            Mock::given(method("GET"))
                .and(path("/api/v1/assets"))
                .and(query_param("type", "identity"))
                .respond_with(
                    ResponseTemplate::new(200)
                        .set_body_json(json!({ "data": [], "nextCursor": null })),
                )
                .mount(&server)
                .await;
            Mock::given(method("GET"))
                .and(path(format!("/api/v1/assets/objects/{id}/insights")))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                    "weeklyActivity": weekly, "completeThrough": "2026-10-25"
                })))
                .mount(&server)
                .await;
            Mock::given(method("GET"))
                .and(path(format!("/api/v1/assets/objects/{id}/activities")))
                .respond_with(FakeCollectionRows { rows })
                .mount(&server)
                .await;
            Mock::given(method("GET"))
                .and(path(format!("/api/v1/assets/objects/{id}")))
                .respond_with(ResponseTemplate::new(200).set_body_json(
                    json!({ "collectionId": id, "activitiesCount": activities_count }),
                ))
                .mount(&server)
                .await;
        });
        execute_check(
            registered_check("collection_daily_activity_consistency").as_ref(),
            &mock_ctx(&server),
            &ProgressReporter::new(None),
        )
    }

    /// 150 rows (two pages): a mint + burn on 10-06, 149 transfers on 10-13,
    /// one update-only row (counts nowhere), and a row in the tip's
    /// incomplete week (after `completeThrough`, on neither side).
    fn collection_rows() -> Vec<(i64, Vec<&'static str>)> {
        let mut rows = vec![(utc8_noonish("2026-10-27"), vec!["mint"])];
        rows.extend((0..147).map(|_| (utc8_noonish("2026-10-13"), vec!["transfer"])));
        rows.push((utc8_noonish("2026-10-12"), vec!["update"]));
        rows.push((utc8_noonish("2026-10-06"), vec!["mint", "burn"]));
        rows
    }

    fn collection_weekly(transfers: u64) -> serde_json::Value {
        json!([
            {"weekStart": "2026-10-05", "mintTransactions": 1, "transferTransactions": 0, "burnTransactions": 1},
            {"weekStart": "2026-10-12", "mintTransactions": 0, "transferTransactions": transfers, "burnTransactions": 0},
            {"weekStart": "2026-10-19", "mintTransactions": 0, "transferTransactions": 0, "burnTransactions": 0},
        ])
    }

    #[test]
    fn collection_daily_check_passes_when_the_calendar_folds_the_rows() {
        let completed = run_collection_daily_check(collection_weekly(147), collection_rows(), 150);
        let result = completed.result.expect("a completed run has a result");
        assert_eq!(completed.status, CheckStatus::Pass, "{:?}", result.findings);
        assert_eq!(result.items_checked, 1);
    }

    #[test]
    fn collection_daily_check_fails_naming_the_week_and_the_count() {
        let id = format!("0x{}", "3c".repeat(24));
        // The daily rows lost one transfer of week 10-12; the list row
        // claims a row more than the activity list holds.
        let completed = run_collection_daily_check(collection_weekly(146), collection_rows(), 151);
        assert_eq!(completed.status, CheckStatus::Fail);
        let findings = completed.result.expect("result").findings;
        assert_eq!(findings[0].entity, format!("collection={id}"));
        assert_eq!(
            findings[0].details,
            vec![
                "week 2026-10-12: daily counts [mint, transfer, burn] = [0, 146, 0], activity rows = [0, 147, 0]"
                    .to_string(),
                "activitiesCount=151 != activity rows listed=150".to_string(),
            ]
        );
    }

    // ── global_activity_bucket_index_consistency ──────────────────────────

    /// One transaction of the fake chain: where the API's block listing
    /// classifies it, and which bucket row (if any) the index holds for it.
    #[derive(Clone)]
    struct FakeBucketTx {
        block: i64,
        index: i32,
        hash: String,
        classified: &'static str,
        indexed: Option<&'static str>,
    }

    /// Serves `/activities` the way the API does: the recent feed, the block
    /// listing filtered by classification, and the bucket listing from the
    /// index (newest first, strictly older than `block:index` cursors).
    struct FakeBucketApi {
        txs: Vec<FakeBucketTx>,
    }

    fn fake_bucket_row(tx: &FakeBucketTx) -> serde_json::Value {
        json!({ "txHash": tx.hash, "blockNumber": tx.block, "txIndex": tx.index,
                "participants": [] })
    }

    impl Respond for FakeBucketApi {
        fn respond(&self, request: &Request) -> ResponseTemplate {
            let query: HashMap<String, String> = request.url.query_pairs().into_owned().collect();
            let limit: usize = query["limit"].parse().unwrap();
            let cursor = query.get("cursor").map(|c| {
                let (b, i) = c.split_once(':').unwrap();
                (b.parse::<i64>().unwrap(), i.parse::<i32>().unwrap())
            });
            let mut txs = self.txs.clone();
            txs.sort_by_key(|t| std::cmp::Reverse((t.block, t.index)));
            let selected: Vec<&FakeBucketTx> = match (query.get("blockNumber"), query.get("filter"))
            {
                (Some(block), Some(filter)) => {
                    let block: i64 = block.parse().unwrap();
                    txs.iter()
                        .filter(|t| t.block == block && t.classified == filter)
                        .collect()
                }
                (None, Some(filter)) => txs
                    .iter()
                    .filter(|t| t.indexed == Some(filter.as_str()))
                    .filter(|t| cursor.is_none_or(|c| (t.block, t.index) < c))
                    .collect(),
                (None, None) => txs.iter().collect(),
                (Some(_), None) => panic!("unfiltered block listing is not used"),
            };
            let page: Vec<serde_json::Value> = selected
                .iter()
                .take(limit)
                .map(|t| fake_bucket_row(t))
                .collect();
            let next = (selected.len() > limit).then(|| {
                let last = selected[limit - 1];
                format!("{}:{}", last.block, last.index)
            });
            ResponseTemplate::new(200).set_body_json(json!({ "data": page, "nextCursor": next }))
        }
    }

    fn fake_bucket_chain() -> Vec<FakeBucketTx> {
        let tx = |block, index, classified, indexed| FakeBucketTx {
            block,
            index,
            hash: format!("0x{:064x}", block * 100 + i64::from(index)),
            classified,
            indexed,
        };
        let mut chain = vec![
            tx(900, 1, "dao", Some("dao")),
            tx(900, 2, "token", Some("token")),
            tx(899, 1, "ckb", Some("ckb")),
            tx(898, 3, "object", Some("object")),
            tx(898, 4, "identity", Some("identity")),
        ];
        // A long token run below the sampled blocks: the bucket listing must
        // stop at the first row of an older block, not page through them.
        for block in 700..760 {
            chain.push(tx(block, 1, "token", Some("token")));
        }
        chain
    }

    fn run_bucket_check(chain: Vec<FakeBucketTx>, sample_count: usize) -> CompletedCheck {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let server = runtime.block_on(MockServer::start());
        runtime.block_on(
            Mock::given(method("GET"))
                .and(path("/api/v1/activities"))
                .respond_with(FakeBucketApi { txs: chain })
                .mount(&server),
        );
        let ctx = CheckContext {
            sample_count,
            ..mock_ctx(&server)
        };
        execute_check(
            registered_check("global_activity_bucket_index_consistency").as_ref(),
            &ctx,
            &ProgressReporter::new(None),
        )
    }

    #[test]
    fn global_bucket_index_check_passes_when_the_index_matches_classification() {
        // The 5 most recent transactions span blocks 900, 899 and 898.
        let completed = run_bucket_check(fake_bucket_chain(), 5);
        let result = completed.result.expect("a completed run has a result");
        assert_eq!(completed.status, CheckStatus::Pass, "{:?}", result.findings);
        assert_eq!(result.items_checked, 3, "blocks 900, 899, 898");
    }

    #[test]
    fn global_bucket_index_check_fails_with_block_tx_context_on_a_missing_or_stray_row() {
        let mut chain = fake_bucket_chain();
        // 898:3 classifies as object but its bucket row is missing; 899:1 has
        // a stray token row though it classifies as ckb.
        chain[3].indexed = None;
        chain[2].indexed = Some("token");
        let completed = run_bucket_check(chain, 10);
        assert_eq!(completed.status, CheckStatus::Fail);
        let findings = completed.result.expect("result").findings;
        let rendered: Vec<String> = findings
            .iter()
            .map(|f| format!("{}: {}", f.entity, f.details.join("; ")))
            .collect();
        assert!(
            rendered
                .iter()
                .any(|f| f.starts_with("block=898 bucket=object: tx=0x")
                    && f.contains("(index 3) classifies as object but has no object bucket row")),
            "{rendered:#?}"
        );
        assert!(
            rendered.iter().any(|f| f
                .starts_with("block=899 bucket=token: token bucket row at index 1")
                && f.contains("does not classify as token in this block")),
            "{rendered:#?}"
        );
        assert!(
            rendered
                .iter()
                .any(|f| f.starts_with("block=899 bucket=ckb: tx=0x")),
            "{rendered:#?}"
        );
        assert_eq!(findings.len(), 3, "{rendered:#?}");
    }
}
