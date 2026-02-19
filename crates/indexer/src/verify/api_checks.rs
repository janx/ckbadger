//! API-based checks — validates data via the ckbadger REST API.
//!
//! Fast tier (F1-F6): few API calls, seconds.
//! Sampling tier (S1-S7): N API calls or chart validation, minutes.

use super::checks::*;
use super::report::format_number;
use super::sampling::LcgSampler;

const SYNC_COMPLETE_MAX_LAG_BLOCKS: i64 = 100;

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
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct TopAddressResponse {
    lock_script_hash: String,
    balance: String,
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

/// Simple chart point with a single value (e.g. transaction-count).
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ChartDataPoint {
    date: String,
    value: String,
}

/// Stacked chart point with named series (e.g. cell-count, total-supply).
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct StackedChartDataPoint {
    date: String,
    values: std::collections::HashMap<String, String>,
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
        let lag = (ss.tip_block - ss.synced_block).max(0);
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
    let lag = (ss.tip_block - ss.synced_block).max(0);

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
// SAMPLING CHECKS (S1-S7)
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
        let tip = stats.latest_block as u64;
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
        let tip = stats.latest_block as u64;
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
            let stored_balance: i128 = addr.balance.parse().unwrap_or(0);

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

/// S7: N random blocks: compare API vs CKB RPC (hash, txCount). Skipped without --rpc-url.
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
        let tip = stats.latest_block as u64;
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

// ---------------------------------------------------------------------------
// CKB RPC helpers (blocking, used only by S7)
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
        Box::new(RpcBlockSpotCheck),
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
        assert_eq!(checks.len(), 13);
        // Verify names are unique
        let names: Vec<&str> = checks.iter().map(|c| c.name()).collect();
        let unique: std::collections::HashSet<&str> = names.iter().copied().collect();
        assert_eq!(names.len(), unique.len(), "Duplicate check names found");
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
        assert_eq!(sampling_count, 7);
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
}
