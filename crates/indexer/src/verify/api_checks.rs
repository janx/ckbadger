//! API-based checks — validates data via the ckbadger REST API.
//!
//! Fast tier (F1-F6): few API calls, seconds.
//! Sampling tier (S1-S23): N API calls or chart validation, minutes.

use super::checks::*;
use super::report::format_number;
use super::sampling::LcgSampler;
use ckbadger_common::TokenBalance;

const SYNC_COMPLETE_MAX_LAG_BLOCKS: i64 = 100;
const SHANNONS_PER_CKB: i128 = 100_000_000;
/// Genesis burnt supply in shannons — a network invariant shared by mainnet and
/// testnet (8.4B CKB, the unspendable Satoshi gift). The authoritative source is
/// the persisted `GenesisBaseline::burnt` (derived from block 0); these
/// API-backed verify checks have no store handle, so they assert against this
/// documented network-invariant literal instead of the constant it derives to.
const GENESIS_BURNT_SHANNONS: i128 = 840_000_000_000_000_000;
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
const SECONDARY_ISSUANCE_MAX_DRIFT_CKB: i128 = 10_000;
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
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct DaoStatisticsResponse {
    total_deposited: String,
    estimated_apc: String,
    active_deposits: i64,
    mining_reward: String,
    deposit_compensation: String,
    burnt: String,
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

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct CursorPage<T> {
    data: Vec<T>,
    next_cursor: Option<String>,
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
    transfers_count: i64,
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
    #[allow(dead_code)]
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
struct MinerDistributionDataPoint {
    address: String,
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

fn sampling_tip_from_stats(stats: &NetworkStats) -> u64 {
    let candidates = [
        stats.latest_block,
        stats.sync_status.synced_block,
        stats.sync_status.tip_block,
    ];
    candidates
        .into_iter()
        .filter(|v| *v >= 0)
        .min()
        .unwrap_or(0) as u64
}

fn exceeds_drift_limit(chart_value: i128, stats_value: i128, max_drift: i128) -> bool {
    (chart_value - stats_value).abs() > max_drift
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
    activity_total: i64,
    live_cell_total: i64,
}

fn fetch_address_count_snapshot(
    ctx: &CheckContext,
    holder_lock_hash: &str,
) -> anyhow::Result<AddressCountSnapshot> {
    let address: AddressDetailApiRecord = api_get(ctx, &format!("addresses/{}", holder_lock_hash))?;

    // Read pre-computed totals from endpoint responses (single request each)
    // instead of paginating through all records.
    let tx_page: CursorPageWithTotal<serde_json::Value> = api_get(
        ctx,
        &format!("addresses/{}/transactions?limit=1", holder_lock_hash),
    )?;
    let activity_page: CursorPageWithTotal<serde_json::Value> = api_get(
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
    let activity_total = activity_page.total.ok_or_else(|| {
        anyhow::anyhow!("activities endpoint missing total for {}", holder_lock_hash)
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
        activity_total,
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
    if snapshot.activity_total != snapshot.transactions_count {
        details.push(format!(
            "activities endpoint total={} != address transactionsCount={}",
            snapshot.activity_total, snapshot.transactions_count
        ));
    }
    if snapshot.tx_total != snapshot.activity_total {
        details.push(format!(
            "transactions endpoint total={} != activities endpoint total={}",
            snapshot.tx_total, snapshot.activity_total
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

/// F7: The persisted genesis economic baseline is present and its burnt supply
/// equals the network invariant (8.4B CKB).
///
/// The verify harness is purely API-backed and has no store handle, so this
/// check observes the baseline through the API rather than reading
/// `store.get_genesis_baseline()` directly:
///  - Presence: both `/charts/total-supply` and `/charts/secondary-issuance`
///    are derived from `GenesisBaseline` on the API side and fail-fast if it
///    was never derived, so a successful fetch proves the baseline is present.
///  - Value: `total-supply.burnt - secondary-issuance.burnt` isolates the
///    genesis burnt, which must equal 8.4B CKB (mainnet and testnet share it).
///
/// This is the Fast-tier, latest-point counterpart to S17
/// (`BurntSupplyGenesisInvariant`), which checks every overlapping date.
pub struct GenesisBaselineBurntInvariant;

impl Check for GenesisBaselineBurntInvariant {
    fn name(&self) -> &'static str {
        "genesis_baseline_burnt_invariant"
    }
    fn description(&self) -> &'static str {
        "genesis baseline present and burnt equals network invariant (8.4B CKB)"
    }
    fn tier(&self) -> CheckTier {
        CheckTier::Fast
    }
    fn run(&self, ctx: &CheckContext, _progress: &ProgressReporter) -> anyhow::Result<CheckResult> {
        // A successful fetch of these endpoints proves the baseline is present:
        // both derive `burnt` from `GenesisBaseline` and error if it is missing.
        let total_supply: ChartWrapper<StackedChartDataPoint> =
            api_get(ctx, "charts/total-supply")?;
        let secondary: ChartWrapper<StackedChartDataPoint> =
            api_get(ctx, "charts/secondary-issuance")?;

        let mut secondary_burnt_by_date = std::collections::HashMap::<&str, i128>::new();
        for point in &secondary.data {
            if let Some(burnt) = point
                .values
                .get("burnt")
                .and_then(|v| parse_non_negative_i128(v))
            {
                secondary_burnt_by_date.insert(point.date.as_str(), burnt);
            }
        }

        // Latest date present in both charts (data is date-ascending).
        let latest = total_supply.data.iter().rev().find_map(|point| {
            let total_burnt = point
                .values
                .get("burnt")
                .and_then(|v| parse_non_negative_i128(v))?;
            let secondary_burnt = secondary_burnt_by_date.get(point.date.as_str())?;
            Some((point.date.as_str(), total_burnt - secondary_burnt))
        });

        let Some((date, gap_ckb)) = latest else {
            return Ok(CheckResult::pass_with_detail(
                0,
                "no overlapping dates between total-supply and secondary-issuance charts"
                    .to_string(),
            ));
        };

        let expected_gap_ckb = GENESIS_BURNT_SHANNONS / SHANNONS_PER_CKB;
        if gap_ckb == expected_gap_ckb {
            Ok(CheckResult::pass_with_detail(
                1,
                format!("genesis burnt = {} CKB at {}", gap_ckb, date),
            ))
        } else {
            Ok(CheckResult::fail(
                1,
                vec![Finding {
                    entity: date.to_string(),
                    details: vec![format!(
                        "genesis burnt = {} CKB, expected {} CKB (8.4B network invariant)",
                        gap_ckb, expected_gap_ckb
                    )],
                }],
            ))
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
        let tip = sampling_tip_from_stats(&stats);
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
        let tip = sampling_tip_from_stats(&stats);
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
        let tip_before = sampling_tip_from_stats(&fetch_network_stats(ctx)?);

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
                    tip_after = sampling_tip_from_stats(&fetch_network_stats(ctx)?);
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
            let value: i64 = point.value.parse().unwrap_or(0);
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
        let mut prev_total: f64 = 0.0;
        let mut prev_date = String::new();

        for point in &chart.data {
            // total-supply is a stacked area; sum all series values for total
            let total: f64 = point
                .values
                .values()
                .filter_map(|v| v.parse::<f64>().ok())
                .sum();

            if total < prev_total && !prev_date.is_empty() {
                findings.push(Finding {
                    entity: point.date.clone(),
                    details: vec![format!(
                        "total supply decreased: {:.2} → {:.2} (prev={})",
                        prev_total, total, prev_date,
                    )],
                });
            }
            prev_total = total;
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
        "Miner distribution: address format, totals, and percentages sane"
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
            if !miner.address.starts_with("0x") || miner.address.len() != 66 {
                findings.push(Finding {
                    entity: miner.address.clone(),
                    details: vec![format!(
                        "invalid miner lock hash format: '{}'",
                        miner.address
                    )],
                });
            }

            if miner.blocks_mined < 0 {
                findings.push(Finding {
                    entity: miner.address.clone(),
                    details: vec![format!("negative blocksMined: {}", miner.blocks_mined)],
                });
            } else {
                sum_blocks += miner.blocks_mined;
            }

            match miner.percentage.parse::<f64>() {
                Ok(pct) if (0.0..=100.0).contains(&pct) => sum_percentage += pct,
                Ok(pct) => findings.push(Finding {
                    entity: miner.address.clone(),
                    details: vec![format!("percentage out of range [0,100]: {}", pct)],
                }),
                Err(_) => findings.push(Finding {
                    entity: miner.address.clone(),
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

/// S13: GET /charts/inflation-rate → nominal/real relationship and timeline sanity.
pub struct ChartInflationRateSane;

impl Check for ChartInflationRateSane {
    fn name(&self) -> &'static str {
        "chart_inflation_rate_sane"
    }
    fn description(&self) -> &'static str {
        "Inflation chart: expected point count, 0.5y step, nominal >= real"
    }
    fn tier(&self) -> CheckTier {
        CheckTier::Sampling
    }
    fn run(&self, ctx: &CheckContext, _progress: &ProgressReporter) -> anyhow::Result<CheckResult> {
        let chart: ChartWrapper<ChartDataPoint> = api_get(ctx, "charts/inflation-rate")?;
        let mut findings = vec![];
        let mut prev_year: Option<f64> = None;
        let mut prev_nominal: Option<f64> = None;

        if chart.data.len() != 101 {
            findings.push(Finding {
                entity: "chart".to_string(),
                details: vec![format!(
                    "expected 101 data points (0..50y at 0.5y), got {}",
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
            if let Some(prev) = prev_year {
                let step = year - prev;
                if (step - 0.5).abs() > 0.0001 {
                    findings.push(Finding {
                        entity: point.date.clone(),
                        details: vec![format!("year step is {}, expected 0.5", step)],
                    });
                }
            }
            if let Some(prev) = prev_nominal {
                if nominal > prev + 1e-9 {
                    findings.push(Finding {
                        entity: point.date.clone(),
                        details: vec![format!(
                            "nominal inflation increased: {:.6} -> {:.6}",
                            prev, nominal
                        )],
                    });
                }
            }

            prev_year = Some(year);
            prev_nominal = Some(nominal);
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

    Some(whole_part * SHANNONS_PER_CKB + frac_part)
}

fn shannons_to_rounded_whole_ckb(shannons: i128) -> Option<i128> {
    if shannons < 0 {
        return None;
    }
    Some((shannons + 50_000_000) / SHANNONS_PER_CKB)
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

/// S16: Latest /charts/secondary-issuance must match /dao/statistics cumulative fields.
pub struct SecondaryIssuanceMatchesDaoStatistics;

impl Check for SecondaryIssuanceMatchesDaoStatistics {
    fn name(&self) -> &'static str {
        "secondary_issuance_matches_dao_statistics"
    }
    fn description(&self) -> &'static str {
        "Latest secondary issuance chart equals DAO statistics (same rounded CKB)"
    }
    fn tier(&self) -> CheckTier {
        CheckTier::Sampling
    }
    fn run(&self, ctx: &CheckContext, _progress: &ProgressReporter) -> anyhow::Result<CheckResult> {
        let dao: DaoStatisticsResponse = api_get(ctx, "dao/statistics")?;
        let secondary: ChartWrapper<StackedChartDataPoint> =
            api_get(ctx, "charts/secondary-issuance")?;

        let Some(latest) = secondary.data.last() else {
            return Ok(CheckResult::pass_with_detail(
                0,
                "secondary-issuance chart is empty".to_string(),
            ));
        };

        let mut findings = vec![];

        let chart_compensation = latest
            .values
            .get("compensation")
            .and_then(|v| parse_non_negative_i128(v));
        let chart_mining = latest
            .values
            .get("mining")
            .and_then(|v| parse_non_negative_i128(v));
        let chart_burnt = latest
            .values
            .get("burnt")
            .and_then(|v| parse_non_negative_i128(v));

        let stats_compensation = parse_non_negative_i128(&dao.deposit_compensation)
            .and_then(shannons_to_rounded_whole_ckb);
        let stats_mining =
            parse_non_negative_i128(&dao.mining_reward).and_then(shannons_to_rounded_whole_ckb);
        let stats_burnt =
            parse_non_negative_i128(&dao.burnt).and_then(shannons_to_rounded_whole_ckb);

        match (chart_compensation, stats_compensation) {
            (Some(chart), Some(stats))
                if exceeds_drift_limit(chart, stats, SECONDARY_ISSUANCE_MAX_DRIFT_CKB) =>
            {
                findings.push(Finding {
                    entity: latest.date.clone(),
                    details: vec![format!(
                        "compensation chart={} != dao_statistics={}",
                        chart, stats
                    )],
                })
            }
            (None, _) | (_, None) => findings.push(Finding {
                entity: latest.date.clone(),
                details: vec!["missing or invalid compensation value".to_string()],
            }),
            _ => {}
        }

        match (chart_mining, stats_mining) {
            (Some(chart), Some(stats))
                if exceeds_drift_limit(chart, stats, SECONDARY_ISSUANCE_MAX_DRIFT_CKB) =>
            {
                findings.push(Finding {
                    entity: latest.date.clone(),
                    details: vec![format!(
                        "mining chart={} != dao_statistics={}",
                        chart, stats
                    )],
                })
            }
            (None, _) | (_, None) => findings.push(Finding {
                entity: latest.date.clone(),
                details: vec!["missing or invalid mining value".to_string()],
            }),
            _ => {}
        }

        match (chart_burnt, stats_burnt) {
            (Some(chart), Some(stats))
                if exceeds_drift_limit(chart, stats, SECONDARY_ISSUANCE_MAX_DRIFT_CKB) =>
            {
                findings.push(Finding {
                    entity: latest.date.clone(),
                    details: vec![format!("burnt chart={} != dao_statistics={}", chart, stats)],
                })
            }
            (None, _) | (_, None) => findings.push(Finding {
                entity: latest.date.clone(),
                details: vec!["missing or invalid burnt value".to_string()],
            }),
            _ => {}
        }

        if findings.is_empty() {
            Ok(CheckResult::pass_with_detail(
                1,
                format!("matched at {}", latest.date),
            ))
        } else {
            Ok(CheckResult::fail(1, findings))
        }
    }
}

/// S17: For overlapping dates, total-supply burnt minus secondary burnt must equal genesis burnt.
pub struct BurntSupplyGenesisInvariant;

impl Check for BurntSupplyGenesisInvariant {
    fn name(&self) -> &'static str {
        "burnt_supply_genesis_invariant"
    }
    fn description(&self) -> &'static str {
        "total-supply.burnt - secondary-issuance.burnt equals genesis burnt (8.4B CKB)"
    }
    fn tier(&self) -> CheckTier {
        CheckTier::Sampling
    }
    fn run(&self, ctx: &CheckContext, _progress: &ProgressReporter) -> anyhow::Result<CheckResult> {
        let total_supply: ChartWrapper<StackedChartDataPoint> =
            api_get(ctx, "charts/total-supply")?;
        let secondary: ChartWrapper<StackedChartDataPoint> =
            api_get(ctx, "charts/secondary-issuance")?;

        let mut findings = vec![];
        let mut secondary_burnt_by_date = std::collections::HashMap::<String, i128>::new();
        for point in &secondary.data {
            if let Some(burnt) = point
                .values
                .get("burnt")
                .and_then(|v| parse_non_negative_i128(v))
            {
                secondary_burnt_by_date.insert(point.date.clone(), burnt);
            } else {
                findings.push(Finding {
                    entity: point.date.clone(),
                    details: vec!["missing or invalid secondary burnt value".to_string()],
                });
            }
        }

        let expected_gap_ckb = GENESIS_BURNT_SHANNONS / SHANNONS_PER_CKB;
        let mut checked = 0u64;

        for point in &total_supply.data {
            let Some(secondary_burnt) = secondary_burnt_by_date.get(&point.date) else {
                continue;
            };

            let total_burnt = match point
                .values
                .get("burnt")
                .and_then(|v| parse_non_negative_i128(v))
            {
                Some(v) => v,
                None => {
                    findings.push(Finding {
                        entity: point.date.clone(),
                        details: vec!["missing or invalid total-supply burnt value".to_string()],
                    });
                    continue;
                }
            };

            checked += 1;
            let gap = total_burnt - *secondary_burnt;
            if gap != expected_gap_ckb {
                findings.push(Finding {
                    entity: point.date.clone(),
                    details: vec![format!(
                        "burnt gap={} CKB, expected {} CKB",
                        gap, expected_gap_ckb
                    )],
                });
            }
        }

        if checked == 0 {
            return Ok(CheckResult::pass_with_detail(
                0,
                "no overlapping dates with secondary issuance chart".to_string(),
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

/// S18: N random blocks: compare API vs CKB RPC (hash, txCount). Skipped without --rpc-url.
pub struct RpcBlockSpotCheck;

impl Check for RpcBlockSpotCheck {
    fn name(&self) -> &'static str {
        "rpc_block_spot_check"
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
        let tip = sampling_tip_from_stats(&stats);
        if tip == 0 {
            return Ok(CheckResult::pass(0));
        }

        let mut sampler = LcgSampler::new(ctx.seed.wrapping_add(3));
        let blocks = sampler.sample_range(ctx.sample_count, tip + 1);
        let mut findings = vec![];

        for block_num in &blocks {
            // Fetch from our API
            let our_block: BlockResponse = api_get(ctx, &format!("blocks/{}", block_num))?;

            // Fetch from CKB RPC
            let rpc_hash = rpc_get_block_hash(ctx, rpc_url, *block_num)?;

            let mut details = vec![];
            if let Some(ref rpc_h) = rpc_hash {
                if our_block.hash != *rpc_h {
                    details.push(format!(
                        "hash mismatch: ours={}, rpc={}",
                        &our_block.hash[..18],
                        &rpc_h[..rpc_h.len().min(18)],
                    ));
                }
            }

            let rpc_tx_count = rpc_get_block_tx_count(ctx, rpc_url, *block_num)?;
            if let Some(rpc_tc) = rpc_tx_count {
                if our_block.transactions_count != rpc_tc {
                    details.push(format!(
                        "tx_count: ours={}, rpc={}",
                        our_block.transactions_count, rpc_tc,
                    ));
                }
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
        let assets: CursorPageWithTotal<AssetListApiRecord> = api_get(
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
            let asset = &assets.data[idx];

            // The ?type=object filter is a catch-all returning object + identity types.
            let detail_prefix = match asset.asset_type.as_str() {
                "object" => "assets/objects",
                "identity" => "assets/identities",
                other => {
                    findings.push(Finding {
                        entity: format!("asset={}", asset.id),
                        details: vec![format!(
                            "unexpected asset type '{}' in object asset list",
                            other
                        )],
                    });
                    checked += 1;
                    progress.inc(1);
                    continue;
                }
            };

            if asset.standard.eq_ignore_ascii_case("spore") {
                progress.inc(1);
                continue;
            }

            let collection_detail: NftCollectionDetailApiRecord =
                api_get(ctx, &format!("{}/{}", detail_prefix, asset.id))?;
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
            if collection_detail.total_count != asset.transfers_count {
                findings.push(Finding {
                    entity: format!("asset={}", asset.id),
                    details: vec![format!(
                        "detail totalCount={} != list transfersCount={}",
                        collection_detail.total_count, asset.transfers_count
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

            let items: CursorPageWithTotal<serde_json::Value> = api_get(
                ctx,
                &format!("{}/{}/items?limit=1", detail_prefix, asset.id),
            )?;
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

/// S22: Top token/object asset holders must align with address-page counts.
pub struct AssetTopHoldersAddressConsistency;

impl Check for AssetTopHoldersAddressConsistency {
    fn name(&self) -> &'static str {
        "asset_top_holders_address_consistency"
    }
    fn description(&self) -> &'static str {
        "Top asset holders align with address activities/cells/transactions counts"
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
                // ?type=object returns object + identity types
                let detail_prefix = match asset.asset_type.as_str() {
                    "object" => "assets/objects",
                    "identity" => "assets/identities",
                    other => {
                        asset_details
                            .push(format!("unexpected asset type '{}' in object list", other));
                        findings.push(Finding {
                            entity: format!("object={} standard={}", asset.id, asset.standard),
                            details: asset_details,
                        });
                        checked += 1;
                        progress.inc(1);
                        continue;
                    }
                };
                let detail: NftCollectionDetailApiRecord =
                    api_get(ctx, &format!("{}/{}", detail_prefix, asset.id))?;
                let holders_page: CursorPageWithTotal<NftHolderApiRecord> = api_get(
                    ctx,
                    &format!(
                        "{}/{}/holders?limit={}",
                        detail_prefix, asset.id, TOP_HOLDER_LIMIT
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

        if checked == 0 {
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

fn rpc_get_block_hash(
    ctx: &CheckContext,
    rpc_url: &str,
    block_num: u64,
) -> anyhow::Result<Option<String>> {
    let hex_num = format!("0x{:x}", block_num);
    let result = rpc_call(
        ctx,
        rpc_url,
        "get_block_by_number",
        vec![serde_json::Value::String(hex_num)],
    )?;
    Ok(result.and_then(|v| {
        v.get("header")
            .and_then(|h| h.get("hash"))
            .and_then(|h| h.as_str())
            .map(|s| s.to_string())
    }))
}

fn rpc_get_block_tx_count(
    ctx: &CheckContext,
    rpc_url: &str,
    block_num: u64,
) -> anyhow::Result<Option<i32>> {
    let hex_num = format!("0x{:x}", block_num);
    let result = rpc_call(
        ctx,
        rpc_url,
        "get_block_by_number",
        vec![serde_json::Value::String(hex_num)],
    )?;
    Ok(result.and_then(|v| {
        v.get("transactions")
            .and_then(|t| t.as_array())
            .map(|arr| arr.len() as i32)
    }))
}

// ============================================
// Registration
// ============================================

/// Return all API-based checks (fast + sampling).
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
        Box::new(SecondaryIssuanceMatchesDaoStatistics),
        Box::new(BurntSupplyGenesisInvariant),
        Box::new(RpcBlockSpotCheck),
        Box::new(TokenActivityTransferBidirectional),
        Box::new(SporeOwnerRoundtrip),
        Box::new(ObjectAssetCollectionConsistency),
        Box::new(AssetTopHoldersAddressConsistency),
        Box::new(IdentityCollectionHolderConsistency),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
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
        }
    }

    #[test]
    fn test_api_checks_registered() {
        let checks = api_checks();
        assert_eq!(checks.len(), 30);
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
        assert!(names.contains(&"secondary_issuance_matches_dao_statistics"));
        assert!(names.contains(&"burnt_supply_genesis_invariant"));
        assert!(names.contains(&"token_activity_transfer_bidirectional"));
        assert!(names.contains(&"spore_owner_roundtrip"));
        assert!(names.contains(&"object_asset_collection_consistency"));
        assert!(names.contains(&"asset_top_holders_address_consistency"));
        assert!(names.contains(&"identity_collection_holder_consistency"));
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
        assert_eq!(sampling_count, 23);
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
        assert!(result.passed, "findings: {:?}", result.findings);
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
            !result.passed,
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
            result.passed,
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
        assert!(!result.passed, "8000s per block must fail the sanity bound");
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
        assert!(result.passed, "findings: {:?}", result.findings);
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
        assert!(!result.passed);
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
        }
    }

    fn mount_burnt_charts(
        runtime: &tokio::runtime::Runtime,
        server: &MockServer,
        total_supply_burnt_ckb: &str,
        secondary_burnt_ckb: &str,
    ) {
        let total = json!({
            "data": [{ "date": "2024-01-01", "values": { "burnt": total_supply_burnt_ckb } }]
        });
        let secondary = json!({
            "data": [{ "date": "2024-01-01", "values": { "burnt": secondary_burnt_ckb } }]
        });
        runtime.block_on(async {
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
            result.passed,
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
        assert!(!result.passed);
        assert_eq!(result.findings.len(), 1);
        assert!(result.findings[0].details[0].contains("8.4B network invariant"));
    }

    #[test]
    fn test_rpc_spot_check_requires_rpc() {
        let check = RpcBlockSpotCheck;
        assert!(check.requires_rpc());
        // Should be skipped when no rpc_url
        let ctx = test_ctx();
        let progress = ProgressReporter::new(None);
        let completed = execute_check(&check, &ctx, &progress);
        assert!(completed.skipped);
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

        assert!(result.passed, "unexpected findings: {:?}", result.findings);
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

        assert!(!result.passed);
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

        assert!(!result.passed);
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
    fn test_shannons_to_rounded_whole_ckb_matches_chart_rounding() {
        assert_eq!(shannons_to_rounded_whole_ckb(149_999_999), Some(1));
        assert_eq!(shannons_to_rounded_whole_ckb(150_000_000), Some(2));
        assert_eq!(shannons_to_rounded_whole_ckb(0), Some(0));
        assert_eq!(shannons_to_rounded_whole_ckb(-1), None);
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
        assert_eq!(sampling_tip_from_stats(&stats), 100);
    }

    #[test]
    fn test_exceeds_drift_limit() {
        assert!(!exceeds_drift_limit(1_000, 1_005, 10));
        assert!(exceeds_drift_limit(1_000, 1_020, 10));
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
    fn test_address_count_mismatch_details_empty_when_consistent() {
        let snapshot = AddressCountSnapshot {
            lock_script_hash: "0xabc".to_string(),
            live_cells_count: 3,
            transactions_count: 5,
            recent_activities_count: 5,
            tx_total: 5,
            activity_total: 5,
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
            activity_total: 2,
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
        assert!(details.iter().any(
            |line| line.contains("activities endpoint total=2 != address transactionsCount=6")
        ));
        assert!(details
            .iter()
            .any(|line| line
                .contains("transactions endpoint total=3 != activities endpoint total=2")));
        assert!(details
            .iter()
            .any(|line| line.contains("live cells endpoint total=9 != address liveCellsCount=7")));
    }

    #[test]
    fn test_normalize_hex_key_handles_uppercase_prefix() {
        assert_eq!(normalize_hex_key("0XABcd"), "abcd");
    }
}
