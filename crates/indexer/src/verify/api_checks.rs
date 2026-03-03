//! API-based checks — validates data via the ckbadger REST API.
//!
//! Fast tier (F1-F6): few API calls, seconds.
//! Sampling tier (S1-S21): N API calls or chart validation, minutes.

use super::checks::*;
use super::report::format_number;
use super::sampling::LcgSampler;
use ckbadger_common::dao::GENESIS_BURNT;

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
const NFT_ASSET_LIST_LIMIT: usize = 100;
const NFT_COLLECTION_SAMPLE_MAX: usize = 20;
const SECONDARY_ISSUANCE_MAX_DRIFT_CKB: i128 = 10_000;

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

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct TopAddressResponse {
    lock_script_hash: String,
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
    asset_changes: Vec<serde_json::Value>,
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
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct NftCollectionDetailApiRecord {
    collection_id: String,
    total_count: i64,
    live_count: i64,
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

// ---------------------------------------------------------------------------
// Helper: GET a JSON endpoint from our API.
// ---------------------------------------------------------------------------

fn api_get<T: serde::de::DeserializeOwned>(ctx: &CheckContext, path: &str) -> anyhow::Result<T> {
    let url = format!(
        "{}/{}",
        ctx.api_url.trim_end_matches('/'),
        path.trim_start_matches('/')
    );
    let resp = ctx.http.get(&url).send()?;
    let status = resp.status();
    if !status.is_success() {
        anyhow::bail!("GET {} returned {}", path, status);
    }
    Ok(resp.json()?)
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

fn parse_i128_strict(raw: &str, field_name: &str) -> anyhow::Result<i128> {
    raw.parse::<i128>()
        .map_err(|e| anyhow::anyhow!("invalid {} '{}': {}", field_name, raw, e))
}

fn parse_u128_to_i128_strict(raw: &str, field_name: &str) -> anyhow::Result<i128> {
    let value = raw
        .parse::<u128>()
        .map_err(|e| anyhow::anyhow!("invalid {} '{}': {}", field_name, raw, e))?;
    i128::try_from(value).map_err(|_| anyhow::anyhow!("{} overflows i128: {}", field_name, raw))
}

fn extract_activity_token_deltas(
    activity: &AddressActivityRecord,
) -> anyhow::Result<Vec<(String, i128)>> {
    let mut deltas = Vec::new();

    for change in &activity.asset_changes {
        if change.get("type").and_then(|v| v.as_str()) != Some("token") {
            continue;
        }

        let Some(type_hash) = change.get("typeScriptHash").and_then(|v| v.as_str()) else {
            continue;
        };
        let delta_raw = change
            .get("delta")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                anyhow::anyhow!("token activity missing delta: tx_hash={}", activity.tx_hash)
            })?;
        let delta = parse_i128_strict(delta_raw, "token activity delta")?;

        deltas.push((normalize_hex_key(type_hash), delta));
    }

    Ok(deltas)
}

fn apply_transfer_delta_to_lookup(
    lookup: &mut std::collections::HashMap<(String, String, String), i128>,
    token_type_hash: &str,
    transfer: &TokenTransferApiRecord,
) -> anyhow::Result<()> {
    let token_key = normalize_hex_key(token_type_hash);
    let tx_key = normalize_hex_key(&transfer.tx_hash);
    let amount = parse_u128_to_i128_strict(&transfer.amount, "token transfer amount")?;

    let to_lock_key = normalize_hex_key(&transfer.to_lock_hash);
    if !to_lock_key.is_empty() {
        let key = (token_key.clone(), tx_key.clone(), to_lock_key);
        let current = lookup.get(&key).copied().unwrap_or(0);
        let next = current.checked_add(amount).ok_or_else(|| {
            anyhow::anyhow!(
                "token transfer lookup overflow: tx_hash={}, token_type_hash={}",
                transfer.tx_hash,
                token_type_hash
            )
        })?;
        lookup.insert(key, next);
    }

    if let Some(from_lock_hash) = transfer.from_lock_hash.as_deref() {
        let from_lock_key = normalize_hex_key(from_lock_hash);
        if !from_lock_key.is_empty() {
            let key = (token_key, tx_key, from_lock_key);
            let current = lookup.get(&key).copied().unwrap_or(0);
            let next = current.checked_sub(amount).ok_or_else(|| {
                anyhow::anyhow!(
                    "token transfer lookup underflow: tx_hash={}, token_type_hash={}",
                    transfer.tx_hash,
                    token_type_hash
                )
            })?;
            lookup.insert(key, next);
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

        let total_deposited: f64 = dao.total_deposited.parse().unwrap_or(0.0);
        if total_deposited <= 0.0 {
            findings.push(Finding {
                entity: "dao_statistics".to_string(),
                details: vec![format!("totalDeposited = {}", dao.total_deposited)],
            });
        }

        let apc: f64 = dao.estimated_apc.parse().unwrap_or(0.0);
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

/// S3: GET /addresses/top → for N addresses, paginate live cells and sum capacities == balance.
pub struct AddressBalanceSpotCheck;

impl Check for AddressBalanceSpotCheck {
    fn name(&self) -> &'static str {
        "address_balance_spot_check"
    }
    fn description(&self) -> &'static str {
        "Top address balances match sum of live cells"
    }
    fn tier(&self) -> CheckTier {
        CheckTier::Sampling
    }
    fn estimated_total(&self, _ctx: &CheckContext) -> Option<u64> {
        // We check up to 10 addresses (API returns top N)
        Some(10)
    }
    fn run(&self, ctx: &CheckContext, progress: &ProgressReporter) -> anyhow::Result<CheckResult> {
        let top_addresses: Vec<TopAddressResponse> = api_get(ctx, "addresses/top")?;

        let n = top_addresses.len().min(10);
        let mut findings = vec![];
        let mut checked = 0u64;

        for addr in top_addresses.iter().take(n) {
            let address_balance: AddressBalanceApiRecord =
                api_get(ctx, &format!("addresses/{}", addr.lock_script_hash))?;
            let stored_balance: i128 = address_balance.balance.parse().unwrap_or(0);

            // Paginate through all live cells for this lock_script_hash
            let mut computed_balance: i128 = 0;
            let mut cursor: Option<String> = None;
            loop {
                let path = match &cursor {
                    Some(c) => format!(
                        "cells/live?lock_script_hash={}&limit=100&cursor={}",
                        addr.lock_script_hash, c
                    ),
                    None => format!(
                        "cells/live?lock_script_hash={}&limit=100",
                        addr.lock_script_hash
                    ),
                };
                let resp: CellListResponse = api_get(ctx, &path)?;
                for cell in &resp.data {
                    let cap: i128 = cell.capacity.parse().unwrap_or(0);
                    computed_balance += cap;
                }
                cursor = resp.next_cursor;
                if cursor.is_none() {
                    break;
                }
            }

            if stored_balance != computed_balance {
                findings.push(Finding {
                    entity: format!("lock_hash: {}", &addr.lock_script_hash[..18]),
                    details: vec![format!(
                        "balance: stored={}, computed from cells={} (Δ {})",
                        stored_balance,
                        computed_balance,
                        computed_balance - stored_balance,
                    )],
                });
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

/// S10: GET /charts/average-block-time → positive values in expected range.
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
                Ok(seconds) if seconds > 0.0 && seconds <= 120.0 => {}
                Ok(seconds) => findings.push(Finding {
                    entity: point.date.clone(),
                    details: vec![format!(
                        "average block time out of expected range (0,120]: {}s",
                        seconds
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
    let rounded = format!("{:.0}", shannons as f64 / SHANNONS_PER_CKB as f64);
    rounded.parse::<i128>().ok()
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

        let expected_gap_ckb = (GENESIS_BURNT as i128) / SHANNONS_PER_CKB;
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
    delta: i128,
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
        let top_addresses: Vec<TopAddressResponse> = api_get(ctx, "addresses/top")?;
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
            std::collections::HashMap::<(String, String, String), i128>::new();

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
            let actual = transfer_delta_lookup
                .get(&(
                    sample.token_type_hash.clone(),
                    sample.tx_hash.clone(),
                    sample.lock_hash.clone(),
                ))
                .copied()
                .unwrap_or(0);
            if actual != sample.delta {
                findings.push(Finding {
                    entity: format!(
                        "tx=0x{} lock=0x{} token=0x{}",
                        sample.tx_hash, sample.lock_hash, sample.token_type_hash
                    ),
                    details: vec![format!(
                        "activity delta={} but transfer delta={}",
                        sample.delta, actual
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

/// S21: NFT asset list/detail/items totals must stay internally consistent.
pub struct NftAssetCollectionConsistency;

impl Check for NftAssetCollectionConsistency {
    fn name(&self) -> &'static str {
        "nft_asset_collection_consistency"
    }
    fn description(&self) -> &'static str {
        "NFT list/detail/items totals are consistent"
    }
    fn tier(&self) -> CheckTier {
        CheckTier::Sampling
    }
    fn estimated_total(&self, ctx: &CheckContext) -> Option<u64> {
        Some(ctx.sample_count.min(NFT_COLLECTION_SAMPLE_MAX) as u64)
    }
    fn run(&self, ctx: &CheckContext, progress: &ProgressReporter) -> anyhow::Result<CheckResult> {
        let assets: CursorPageWithTotal<AssetListApiRecord> = api_get(
            ctx,
            &format!("assets?type=nft&limit={}", NFT_ASSET_LIST_LIMIT),
        )?;
        if assets.data.is_empty() || ctx.sample_count == 0 {
            return Ok(CheckResult::pass_with_detail(
                0,
                "no NFT collections available for sampling".to_string(),
            ));
        }

        let sample_indices = sample_indices_with_cap(
            ctx.seed.wrapping_add(30),
            assets.data.len(),
            ctx.sample_count,
            NFT_COLLECTION_SAMPLE_MAX,
        );

        let mut findings = vec![];
        let mut checked = 0u64;

        for idx in sample_indices {
            let asset = &assets.data[idx];
            if asset.asset_type != "nft" {
                findings.push(Finding {
                    entity: format!("asset={}", asset.id),
                    details: vec![format!(
                        "unexpected asset type '{}' in nft asset list",
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

            let collection_detail: NftCollectionDetailApiRecord =
                api_get(ctx, &format!("assets/nfts/{}", asset.id))?;
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
            if collection_detail.live_count != asset.holders_count {
                findings.push(Finding {
                    entity: format!("asset={}", asset.id),
                    details: vec![format!(
                        "detail liveCount={} != list holdersCount={}",
                        collection_detail.live_count, asset.holders_count
                    )],
                });
            }

            let items: CursorPageWithTotal<serde_json::Value> =
                api_get(ctx, &format!("assets/nfts/{}/items?limit=1", asset.id))?;
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
                "no non-spore NFT collections sampled".to_string(),
            ));
        }

        if findings.is_empty() {
            Ok(CheckResult::pass_with_detail(
                checked,
                format!("{} sampled NFT collections", checked),
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
        Box::new(NftAssetCollectionConsistency),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_ctx() -> CheckContext {
        CheckContext {
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
        assert_eq!(checks.len(), 27);
        // Verify names are unique
        let names: Vec<&str> = checks.iter().map(|c| c.name()).collect();
        let unique: std::collections::HashSet<&str> = names.iter().copied().collect();
        assert_eq!(names.len(), unique.len(), "Duplicate check names found");
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
        assert!(names.contains(&"nft_asset_collection_consistency"));
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
        assert_eq!(fast_count, 6);
        assert_eq!(sampling_count, 21);
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
            asset_changes: vec![
                serde_json::json!({
                    "type": "token",
                    "typeScriptHash": "0xAABB",
                    "delta": "-10"
                }),
                serde_json::json!({
                    "type": "daoDeposit",
                    "capacity": "1000"
                }),
                serde_json::json!({
                    "type": "token",
                    "typeScriptHash": "0xccdd",
                    "delta": "25"
                }),
            ],
        };

        let parsed = extract_activity_token_deltas(&activity).unwrap();
        assert_eq!(
            parsed,
            vec![("aabb".to_string(), -10), ("ccdd".to_string(), 25),]
        );
    }

    #[test]
    fn test_extract_activity_token_deltas_requires_delta_field() {
        let activity = AddressActivityRecord {
            tx_hash: "0xabc".to_string(),
            block_number: 123,
            asset_changes: vec![serde_json::json!({
                "type": "token",
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
            asset_changes: vec![serde_json::json!({
                "type": "token",
                "delta": "10"
            })],
        };

        let parsed = extract_activity_token_deltas(&activity).unwrap();
        assert!(parsed.is_empty());
    }

    #[test]
    fn test_apply_transfer_delta_to_lookup_handles_transfer_mint_and_burn() {
        let mut lookup = std::collections::HashMap::<(String, String, String), i128>::new();

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

        assert_eq!(
            lookup
                .get(&("tt".to_string(), "01".to_string(), "aaaa".to_string()))
                .copied(),
            Some(-100)
        );
        assert_eq!(
            lookup
                .get(&("tt".to_string(), "01".to_string(), "bbbb".to_string()))
                .copied(),
            Some(130)
        );
    }

    #[test]
    fn test_apply_transfer_delta_to_lookup_ignores_empty_lock_hashes() {
        let mut lookup = std::collections::HashMap::<(String, String, String), i128>::new();
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
    fn test_normalize_hex_key_handles_uppercase_prefix() {
        assert_eq!(normalize_hex_key("0XABcd"), "abcd");
    }
}
