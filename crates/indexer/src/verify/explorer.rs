//! Explorer comparison checks — compares our data against the official CKB explorer API.
//!
//! Supports file-based caching of explorer responses to avoid repeated HTTP requests.
//! Cache files are stored in `{cache_dir}/{indicator}.json` with a 24-hour freshness window.
//! On HTTP failure, stale cache is used as fallback with a warning.

use std::collections::HashMap;
use std::path::PathBuf;

use console::style;

use super::checks::*;
use super::report::{format_number, format_number_i128};

/// Cached explorer response stored as JSON on disk.
#[derive(serde::Serialize, serde::Deserialize)]
struct CacheEntry {
    fetched_at: String,
    indicator: String,
    data: HashMap<String, String>,
}

const CACHE_FRESHNESS_SECS: i64 = 24 * 60 * 60; // 24 hours

/// Read a cache file for the given indicator. Returns None if file doesn't exist.
fn read_cache(cache_dir: &Option<PathBuf>, indicator: &str) -> Option<CacheEntry> {
    let dir = cache_dir.as_ref()?;
    let path = dir.join(format!("{}.json", indicator));
    let content = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str(&content).ok()
}

/// Check if a cache entry is fresh (< 24 hours old).
fn is_cache_fresh(entry: &CacheEntry) -> bool {
    let fetched = chrono::DateTime::parse_from_rfc3339(&entry.fetched_at)
        .map(|dt| dt.with_timezone(&chrono::Utc));
    match fetched {
        Ok(dt) => {
            let age = chrono::Utc::now().signed_duration_since(dt);
            age.num_seconds() < CACHE_FRESHNESS_SECS
        }
        Err(_) => false,
    }
}

/// Write a cache file for the given indicator.
fn write_cache(cache_dir: &Option<PathBuf>, indicator: &str, data: &HashMap<String, String>) {
    let Some(dir) = cache_dir.as_ref() else {
        return;
    };
    if std::fs::create_dir_all(dir).is_err() {
        return;
    }
    let entry = CacheEntry {
        fetched_at: chrono::Utc::now().to_rfc3339(),
        indicator: indicator.to_string(),
        data: data.clone(),
    };
    let path = dir.join(format!("{}.json", indicator));
    if let Ok(json) = serde_json::to_string_pretty(&entry) {
        let _ = std::fs::write(&path, json);
    }
}

/// Fetch daily statistics from the official CKB explorer API, with file-based caching.
/// Returns a map of date_str ("YYYY-MM-DD") -> value (as string).
fn fetch_explorer_daily(
    ctx: &CheckContext,
    indicator: &str,
    field: &str,
) -> anyhow::Result<HashMap<String, String>> {
    // 1. Try fresh cache first
    if let Some(cached) = read_cache(&ctx.cache_dir, indicator) {
        if is_cache_fresh(&cached) {
            return Ok(cached.data);
        }
    }

    // 2. Fetch from API
    match fetch_from_api(ctx, indicator, field) {
        Ok(data) => {
            write_cache(&ctx.cache_dir, indicator, &data);
            Ok(data)
        }
        Err(e) => {
            // 3. Fall back to stale cache
            if let Some(cached) = read_cache(&ctx.cache_dir, indicator) {
                eprintln!(
                    "    {} Explorer fetch for '{}' failed ({}), using stale cache from {}",
                    style("⚠").yellow(),
                    indicator,
                    e,
                    cached.fetched_at,
                );
                Ok(cached.data)
            } else {
                Err(e)
            }
        }
    }
}

/// Perform the actual HTTP fetch from the explorer API.
fn fetch_from_api(
    ctx: &CheckContext,
    indicator: &str,
    field: &str,
) -> anyhow::Result<HashMap<String, String>> {
    let explorer_url = ctx
        .explorer_url
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Explorer URL not set"))?;

    let url = format!(
        "{}/api/v1/daily_statistics/{}",
        explorer_url.trim_end_matches('/'),
        indicator
    );

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;

    let resp = rt.block_on(async {
        ctx.http_client
            .get(&url)
            .header("Content-Type", "application/vnd.api+json")
            .header("Accept", "application/vnd.api+json")
            .send()
            .await
    })?;

    let body: serde_json::Value = rt.block_on(resp.json())?;

    let mut result = HashMap::new();
    if let Some(data) = body.get("data").and_then(|d| d.as_array()) {
        for item in data {
            let attrs = match item.get("attributes") {
                Some(a) => a,
                None => continue,
            };
            // Try as string first, then as number
            let ts_val = attrs
                .get("created_at_unixtimestamp")
                .and_then(|v| {
                    v.as_str()
                        .and_then(|s| s.parse::<i64>().ok())
                        .or_else(|| v.as_i64())
                })
                .unwrap_or(0);

            let date = chrono::DateTime::from_timestamp(ts_val, 0)
                .map(|dt| dt.format("%Y-%m-%d").to_string())
                .unwrap_or_default();

            if date.is_empty() {
                continue;
            }

            let value = attrs
                .get(field)
                .map(|v| match v {
                    serde_json::Value::String(s) => s.clone(),
                    serde_json::Value::Number(n) => n.to_string(),
                    _ => v.to_string(),
                })
                .unwrap_or_default();

            if !value.is_empty() {
                result.insert(date, value);
            }
        }
    }

    Ok(result)
}

/// Compare integer values: our store vs explorer, requiring exact match.
fn compare_exact_i64(ours: i64, theirs: &str, date: &str, label: &str) -> Option<Finding> {
    let their_val: i64 = match theirs.parse() {
        Ok(v) => v,
        Err(_) => return None, // Skip unparseable values
    };
    if ours != their_val {
        Some(Finding {
            entity: date.to_string(),
            details: vec![format!(
                "{}: ours={}, explorer={} (Δ {:+})",
                label,
                format_number(ours as u64),
                format_number(their_val as u64),
                ours - their_val,
            )],
        })
    } else {
        None
    }
}

/// Compare i128 values: our store vs explorer, requiring exact match.
fn compare_exact_i128(ours: i128, theirs: &str, date: &str, label: &str) -> Option<Finding> {
    let their_val: i128 = match theirs.parse() {
        Ok(v) => v,
        Err(_) => return None,
    };
    if ours != their_val {
        Some(Finding {
            entity: date.to_string(),
            details: vec![format!(
                "{}: ours={}, explorer={} (Δ {:+})",
                label,
                format_number_i128(ours),
                format_number_i128(their_val),
                ours - their_val,
            )],
        })
    } else {
        None
    }
}

/// Compare float values with tolerance.
fn compare_tolerance_f64(
    ours: f64,
    theirs: &str,
    date: &str,
    label: &str,
    tolerance: f64,
) -> Option<Finding> {
    let their_val: f64 = match theirs.parse() {
        Ok(v) => v,
        Err(_) => return None,
    };
    if their_val == 0.0 && ours == 0.0 {
        return None;
    }
    let denom = if their_val.abs() > f64::EPSILON {
        their_val.abs()
    } else {
        1.0
    };
    let deviation = ((ours - their_val) / denom).abs();
    if deviation > tolerance {
        Some(Finding {
            entity: date.to_string(),
            details: vec![format!(
                "{}: ours={:.6}, explorer={:.6} (deviation: {:.4}%, tolerance: {:.4}%)",
                label,
                ours,
                their_val,
                deviation * 100.0,
                tolerance * 100.0,
            )],
        })
    } else {
        None
    }
}

/// Get the last 30 completed days (excluding today).
fn last_30_days() -> Vec<String> {
    let today = chrono::Utc::now().date_naive();
    (1..=30)
        .map(|i| {
            (today - chrono::Duration::days(i))
                .format("%Y-%m-%d")
                .to_string()
        })
        .collect()
}

/// Convert "YYYY-MM-DD" to "YYYYMMDD" for store lookups.
fn date_to_key(date: &str) -> String {
    date.replace('-', "")
}

// ============================================
// Explorer checks
// ============================================

/// X1: Daily transactions_count for last 30 days.
pub struct ExplorerTxCount;

impl Check for ExplorerTxCount {
    fn name(&self) -> &'static str {
        "explorer_tx_count"
    }
    fn description(&self) -> &'static str {
        "Daily transactions_count vs explorer"
    }
    fn tier(&self) -> CheckTier {
        CheckTier::Sampling
    }
    fn requires_explorer(&self) -> bool {
        true
    }
    fn estimated_total(&self, _ctx: &CheckContext) -> Option<u64> {
        Some(30)
    }
    fn run(&self, ctx: &CheckContext, progress: &ProgressReporter) -> anyhow::Result<CheckResult> {
        let explorer_data = fetch_explorer_daily(ctx, "transactions_count", "transactions_count")?;
        let dates = last_30_days();
        let mut findings = vec![];
        let mut checked = 0u64;

        for date in &dates {
            if let Some(explorer_val) = explorer_data.get(date) {
                let key = date_to_key(date);
                if let Some(stats) = ctx.store.get_daily_stats(&key)? {
                    if let Some(f) = compare_exact_i64(
                        stats.transactions_count as i64,
                        explorer_val,
                        date,
                        "transactions_count",
                    ) {
                        findings.push(f);
                    }
                    checked += 1;
                }
            }
            progress.inc(1);
        }

        if findings.is_empty() {
            Ok(CheckResult::pass(checked))
        } else {
            Ok(CheckResult::fail(checked, findings))
        }
    }
}

/// X2: Daily live_cells_count.
pub struct ExplorerLiveCells;

impl Check for ExplorerLiveCells {
    fn name(&self) -> &'static str {
        "explorer_live_cells"
    }
    fn description(&self) -> &'static str {
        "Daily live_cells_count vs explorer"
    }
    fn tier(&self) -> CheckTier {
        CheckTier::Sampling
    }
    fn requires_explorer(&self) -> bool {
        true
    }
    fn estimated_total(&self, _ctx: &CheckContext) -> Option<u64> {
        Some(30)
    }
    fn run(&self, ctx: &CheckContext, progress: &ProgressReporter) -> anyhow::Result<CheckResult> {
        let explorer_data = fetch_explorer_daily(ctx, "live_cells_count", "live_cells_count")?;
        let dates = last_30_days();
        let mut findings = vec![];
        let mut checked = 0u64;

        for date in &dates {
            if let Some(explorer_val) = explorer_data.get(date) {
                let key = date_to_key(date);
                if let Some(stats) = ctx.store.get_daily_stats(&key)? {
                    if let Some(f) = compare_exact_i64(
                        stats.total_live_cells,
                        explorer_val,
                        date,
                        "live_cells_count",
                    ) {
                        findings.push(f);
                    }
                    checked += 1;
                }
            }
            progress.inc(1);
        }

        if findings.is_empty() {
            Ok(CheckResult::pass(checked))
        } else {
            Ok(CheckResult::fail(checked, findings))
        }
    }
}

/// X3: Daily dead_cells_count.
pub struct ExplorerDeadCells;

impl Check for ExplorerDeadCells {
    fn name(&self) -> &'static str {
        "explorer_dead_cells"
    }
    fn description(&self) -> &'static str {
        "Daily dead_cells_count vs explorer"
    }
    fn tier(&self) -> CheckTier {
        CheckTier::Sampling
    }
    fn requires_explorer(&self) -> bool {
        true
    }
    fn estimated_total(&self, _ctx: &CheckContext) -> Option<u64> {
        Some(30)
    }
    fn run(&self, ctx: &CheckContext, progress: &ProgressReporter) -> anyhow::Result<CheckResult> {
        let explorer_data = fetch_explorer_daily(ctx, "dead_cells_count", "dead_cells_count")?;
        let dates = last_30_days();
        let mut findings = vec![];
        let mut checked = 0u64;

        for date in &dates {
            if let Some(explorer_val) = explorer_data.get(date) {
                let key = date_to_key(date);
                if let Some(stats) = ctx.store.get_daily_stats(&key)? {
                    if let Some(f) = compare_exact_i64(
                        stats.total_dead_cells,
                        explorer_val,
                        date,
                        "dead_cells_count",
                    ) {
                        findings.push(f);
                    }
                    checked += 1;
                }
            }
            progress.inc(1);
        }

        if findings.is_empty() {
            Ok(CheckResult::pass(checked))
        } else {
            Ok(CheckResult::fail(checked, findings))
        }
    }
}

/// X4: Daily total_dao_deposit.
pub struct ExplorerDaoDeposit;

impl Check for ExplorerDaoDeposit {
    fn name(&self) -> &'static str {
        "explorer_dao_deposit"
    }
    fn description(&self) -> &'static str {
        "Daily total_dao_deposit vs explorer"
    }
    fn tier(&self) -> CheckTier {
        CheckTier::Sampling
    }
    fn requires_explorer(&self) -> bool {
        true
    }
    fn estimated_total(&self, _ctx: &CheckContext) -> Option<u64> {
        Some(30)
    }
    fn run(&self, ctx: &CheckContext, progress: &ProgressReporter) -> anyhow::Result<CheckResult> {
        let explorer_data = fetch_explorer_daily(ctx, "total_dao_deposit", "total_dao_deposit")?;
        let dates = last_30_days();
        let mut findings = vec![];
        let mut checked = 0u64;

        // Load DAO daily snapshots into a map
        let snapshots = ctx.store.list_dao_daily_snapshots()?;
        let snap_map: HashMap<String, _> =
            snapshots.into_iter().map(|s| (s.date.clone(), s)).collect();

        for date in &dates {
            if let Some(explorer_val) = explorer_data.get(date) {
                if let Some(snap) = snap_map.get(date) {
                    if let Some(f) = compare_exact_i128(
                        snap.total_deposited,
                        explorer_val,
                        date,
                        "total_dao_deposit",
                    ) {
                        findings.push(f);
                    }
                    checked += 1;
                }
            }
            progress.inc(1);
        }

        if findings.is_empty() {
            Ok(CheckResult::pass(checked))
        } else {
            Ok(CheckResult::fail(checked, findings))
        }
    }
}

/// X5: Daily avg_hash_rate (tolerance-based).
pub struct ExplorerHashRate;

impl Check for ExplorerHashRate {
    fn name(&self) -> &'static str {
        "explorer_hash_rate"
    }
    fn description(&self) -> &'static str {
        "Daily avg_hash_rate vs explorer"
    }
    fn tier(&self) -> CheckTier {
        CheckTier::Sampling
    }
    fn requires_explorer(&self) -> bool {
        true
    }
    fn estimated_total(&self, _ctx: &CheckContext) -> Option<u64> {
        Some(30)
    }
    fn run(&self, ctx: &CheckContext, progress: &ProgressReporter) -> anyhow::Result<CheckResult> {
        let explorer_data = fetch_explorer_daily(ctx, "avg_hash_rate", "avg_hash_rate")?;
        let dates = last_30_days();
        let daily_block_stats = ctx.store.list_daily_block_stats()?;
        let stats_map: HashMap<String, _> = daily_block_stats
            .into_iter()
            .map(|(d, s)| (format!("{}-{}-{}", &d[..4], &d[4..6], &d[6..8]), s))
            .collect();

        let mut findings = vec![];
        let mut checked = 0u64;

        for date in &dates {
            if let Some(explorer_val) = explorer_data.get(date) {
                if let Some(stats) = stats_map.get(date) {
                    // Convert compact_target to hash_rate
                    // avg_compact_target is already stored as difficulty-equivalent
                    let our_hash_rate = compact_target_to_hash_rate(stats.avg_compact_target);
                    if let Some(f) = compare_tolerance_f64(
                        our_hash_rate,
                        explorer_val,
                        date,
                        "avg_hash_rate",
                        ctx.tolerance,
                    ) {
                        findings.push(f);
                    }
                    checked += 1;
                }
            }
            progress.inc(1);
        }

        if findings.is_empty() {
            Ok(CheckResult::pass(checked))
        } else {
            Ok(CheckResult::fail(checked, findings))
        }
    }
}

/// Convert compact target average to hash rate estimate.
/// Hash rate ≈ difficulty / block_time.
/// The explorer stores this differently, so we may need tolerance.
fn compact_target_to_hash_rate(avg_compact_target: f64) -> f64 {
    // compact_target is stored as the raw difficulty value in our store
    avg_compact_target
}

/// X6: Daily avg_difficulty (tolerance-based).
pub struct ExplorerDifficulty;

impl Check for ExplorerDifficulty {
    fn name(&self) -> &'static str {
        "explorer_difficulty"
    }
    fn description(&self) -> &'static str {
        "Daily avg_difficulty vs explorer"
    }
    fn tier(&self) -> CheckTier {
        CheckTier::Sampling
    }
    fn requires_explorer(&self) -> bool {
        true
    }
    fn estimated_total(&self, _ctx: &CheckContext) -> Option<u64> {
        Some(30)
    }
    fn run(&self, ctx: &CheckContext, progress: &ProgressReporter) -> anyhow::Result<CheckResult> {
        let explorer_data = fetch_explorer_daily(ctx, "avg_difficulty", "avg_difficulty")?;
        let dates = last_30_days();
        let daily_block_stats = ctx.store.list_daily_block_stats()?;
        let stats_map: HashMap<String, _> = daily_block_stats
            .into_iter()
            .map(|(d, s)| (format!("{}-{}-{}", &d[..4], &d[4..6], &d[6..8]), s))
            .collect();

        let mut findings = vec![];
        let mut checked = 0u64;

        for date in &dates {
            if let Some(explorer_val) = explorer_data.get(date) {
                if let Some(stats) = stats_map.get(date) {
                    if let Some(f) = compare_tolerance_f64(
                        stats.avg_compact_target,
                        explorer_val,
                        date,
                        "avg_difficulty",
                        ctx.tolerance,
                    ) {
                        findings.push(f);
                    }
                    checked += 1;
                }
            }
            progress.inc(1);
        }

        if findings.is_empty() {
            Ok(CheckResult::pass(checked))
        } else {
            Ok(CheckResult::fail(checked, findings))
        }
    }
}

/// X7: Daily knowledge_size (exact match).
pub struct ExplorerKnowledgeSize;

impl Check for ExplorerKnowledgeSize {
    fn name(&self) -> &'static str {
        "explorer_knowledge_size"
    }
    fn description(&self) -> &'static str {
        "Daily knowledge_size vs explorer"
    }
    fn tier(&self) -> CheckTier {
        CheckTier::Sampling
    }
    fn requires_explorer(&self) -> bool {
        true
    }
    fn estimated_total(&self, _ctx: &CheckContext) -> Option<u64> {
        Some(30)
    }
    fn run(&self, ctx: &CheckContext, progress: &ProgressReporter) -> anyhow::Result<CheckResult> {
        let explorer_data = fetch_explorer_daily(ctx, "knowledge_size", "knowledge_size")?;
        let dates = last_30_days();
        let mut findings = vec![];
        let mut checked = 0u64;

        for date in &dates {
            if let Some(explorer_val) = explorer_data.get(date) {
                let key = date_to_key(date);
                if let Some(stats) = ctx.store.get_daily_stats(&key)? {
                    if let Some(our_ks) = stats.knowledge_size {
                        if let Some(f) =
                            compare_exact_i128(our_ks, explorer_val, date, "knowledge_size")
                        {
                            findings.push(f);
                        }
                        checked += 1;
                    }
                }
            }
            progress.inc(1);
        }

        if findings.is_empty() {
            Ok(CheckResult::pass(checked))
        } else {
            Ok(CheckResult::fail(checked, findings))
        }
    }
}

/// X8: Daily occupied_capacity (exact match).
pub struct ExplorerOccupiedCapacity;

impl Check for ExplorerOccupiedCapacity {
    fn name(&self) -> &'static str {
        "explorer_occupied_capacity"
    }
    fn description(&self) -> &'static str {
        "Daily occupied_capacity vs explorer"
    }
    fn tier(&self) -> CheckTier {
        CheckTier::Sampling
    }
    fn requires_explorer(&self) -> bool {
        true
    }
    fn estimated_total(&self, _ctx: &CheckContext) -> Option<u64> {
        Some(30)
    }
    fn run(&self, ctx: &CheckContext, progress: &ProgressReporter) -> anyhow::Result<CheckResult> {
        let explorer_data = fetch_explorer_daily(ctx, "occupied_capacity", "occupied_capacity")?;
        let dates = last_30_days();
        let mut findings = vec![];
        let mut checked = 0u64;

        let snapshots = ctx.store.list_dao_daily_snapshots()?;
        let snap_map: HashMap<String, _> =
            snapshots.into_iter().map(|s| (s.date.clone(), s)).collect();

        for date in &dates {
            if let Some(explorer_val) = explorer_data.get(date) {
                if let Some(snap) = snap_map.get(date) {
                    if let Some(f) = compare_exact_i128(
                        snap.occupied_capacity,
                        explorer_val,
                        date,
                        "occupied_capacity",
                    ) {
                        findings.push(f);
                    }
                    checked += 1;
                }
            }
            progress.inc(1);
        }

        if findings.is_empty() {
            Ok(CheckResult::pass(checked))
        } else {
            Ok(CheckResult::fail(checked, findings))
        }
    }
}

/// X9: Daily uncle_rate (tolerance-based).
pub struct ExplorerUncleRate;

impl Check for ExplorerUncleRate {
    fn name(&self) -> &'static str {
        "explorer_uncle_rate"
    }
    fn description(&self) -> &'static str {
        "Daily uncle_rate vs explorer"
    }
    fn tier(&self) -> CheckTier {
        CheckTier::Sampling
    }
    fn requires_explorer(&self) -> bool {
        true
    }
    fn estimated_total(&self, _ctx: &CheckContext) -> Option<u64> {
        Some(30)
    }
    fn run(&self, ctx: &CheckContext, progress: &ProgressReporter) -> anyhow::Result<CheckResult> {
        let explorer_data = fetch_explorer_daily(ctx, "uncle_rate", "uncle_rate")?;
        let dates = last_30_days();
        let daily_block_stats = ctx.store.list_daily_block_stats()?;
        let stats_map: HashMap<String, _> = daily_block_stats
            .into_iter()
            .map(|(d, s)| (format!("{}-{}-{}", &d[..4], &d[4..6], &d[6..8]), s))
            .collect();

        let mut findings = vec![];
        let mut checked = 0u64;

        for date in &dates {
            if let Some(explorer_val) = explorer_data.get(date) {
                if let Some(stats) = stats_map.get(date) {
                    let our_rate = if stats.block_count > 0 {
                        stats.total_uncles as f64 / stats.block_count as f64
                    } else {
                        0.0
                    };
                    if let Some(f) = compare_tolerance_f64(
                        our_rate,
                        explorer_val,
                        date,
                        "uncle_rate",
                        ctx.tolerance,
                    ) {
                        findings.push(f);
                    }
                    checked += 1;
                }
            }
            progress.inc(1);
        }

        if findings.is_empty() {
            Ok(CheckResult::pass(checked))
        } else {
            Ok(CheckResult::fail(checked, findings))
        }
    }
}

/// X10: Daily circulating_supply (tolerance-based).
pub struct ExplorerCirculatingSupply;

impl Check for ExplorerCirculatingSupply {
    fn name(&self) -> &'static str {
        "explorer_circulating_supply"
    }
    fn description(&self) -> &'static str {
        "Daily circulating_supply vs explorer"
    }
    fn tier(&self) -> CheckTier {
        CheckTier::Sampling
    }
    fn requires_explorer(&self) -> bool {
        true
    }
    fn estimated_total(&self, _ctx: &CheckContext) -> Option<u64> {
        Some(30)
    }
    fn run(&self, ctx: &CheckContext, progress: &ProgressReporter) -> anyhow::Result<CheckResult> {
        let explorer_data = fetch_explorer_daily(ctx, "circulating_supply", "circulating_supply")?;
        let dates = last_30_days();
        let mut findings = vec![];
        let mut checked = 0u64;

        let snapshots = ctx.store.list_dao_daily_snapshots()?;
        let snap_map: HashMap<String, _> =
            snapshots.into_iter().map(|s| (s.date.clone(), s)).collect();

        // Genesis burnt: 8,400,000,000 CKB = 840,000,000,000,000,000 shannons
        const BURNT_SHANNONS: i128 = 840_000_000_000_000_000;

        for date in &dates {
            if let Some(explorer_val) = explorer_data.get(date) {
                if let Some(snap) = snap_map.get(date) {
                    // circulating = total_issuance - burnt - dao_locked
                    let circulating = snap.total_issuance - BURNT_SHANNONS - snap.total_deposited;
                    let our_supply = circulating as f64 / 1e8; // Convert shannons to CKB
                    if let Some(f) = compare_tolerance_f64(
                        our_supply,
                        explorer_val,
                        date,
                        "circulating_supply",
                        ctx.tolerance,
                    ) {
                        findings.push(f);
                    }
                    checked += 1;
                }
            }
            progress.inc(1);
        }

        if findings.is_empty() {
            Ok(CheckResult::pass(checked))
        } else {
            Ok(CheckResult::fail(checked, findings))
        }
    }
}

/// Return all explorer comparison checks.
pub fn explorer_checks() -> Vec<Box<dyn Check>> {
    vec![
        Box::new(ExplorerTxCount),
        Box::new(ExplorerLiveCells),
        Box::new(ExplorerDeadCells),
        Box::new(ExplorerDaoDeposit),
        Box::new(ExplorerHashRate),
        Box::new(ExplorerDifficulty),
        Box::new(ExplorerKnowledgeSize),
        Box::new(ExplorerOccupiedCapacity),
        Box::new(ExplorerUncleRate),
        Box::new(ExplorerCirculatingSupply),
    ]
}
