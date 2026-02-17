//! Explorer comparison checks — compares our API data against the official CKB explorer API.
//!
//! Supports file-based caching of explorer responses to avoid repeated HTTP requests.
//! Cache files are stored in `{cache_dir}/{indicator}.json` with a 24-hour freshness window.
//! On HTTP failure, stale cache is used as fallback with a warning.

use std::collections::HashMap;
use std::path::PathBuf;

use console::style;

use super::checks::*;
use super::report::{format_number, format_number_i128};

// ---------------------------------------------------------------------------
// Lightweight types for our API chart responses
// ---------------------------------------------------------------------------

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApiResponse<T> {
    data: T,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ChartDataPoint {
    date: String,
    value: String,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ChartResponse {
    data: Vec<ChartDataPoint>,
}

// ---------------------------------------------------------------------------
// Explorer API caching
// ---------------------------------------------------------------------------

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
    match fetch_from_explorer_api(ctx, indicator, field) {
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
fn fetch_from_explorer_api(
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

    let resp = ctx
        .http
        .get(&url)
        .header("Content-Type", "application/vnd.api+json")
        .header("Accept", "application/vnd.api+json")
        .send()?;

    let body: serde_json::Value = resp.json()?;

    let mut result = HashMap::new();
    if let Some(data) = body.get("data").and_then(|d| d.as_array()) {
        for item in data {
            let attrs = match item.get("attributes") {
                Some(a) => a,
                None => continue,
            };
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

// ---------------------------------------------------------------------------
// Helpers: fetch our data from the ckbadger API charts
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

/// Fetch our chart data as a date→value map. Works for simple ChartResponse endpoints.
fn fetch_our_chart(
    ctx: &CheckContext,
    chart_path: &str,
) -> anyhow::Result<HashMap<String, String>> {
    let wrapper: ApiResponse<ChartResponse> = api_get(ctx, chart_path)?;
    let mut map = HashMap::new();
    for point in wrapper.data.data {
        map.insert(point.date, point.value);
    }
    Ok(map)
}

// ---------------------------------------------------------------------------
// Comparison helpers
// ---------------------------------------------------------------------------

/// Compare integer values: our API vs explorer, requiring exact match.
fn compare_exact_i64(ours: i64, theirs: &str, date: &str, label: &str) -> Option<Finding> {
    let their_val: i64 = match theirs.parse() {
        Ok(v) => v,
        Err(_) => return None,
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

/// Compare i128 values: our API vs explorer, requiring exact match.
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

// ============================================
// Explorer checks (X1-X5)
// ============================================

/// X1: Compare /charts/transaction-count last 30 days vs explorer transactions_count.
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
        let our_data = fetch_our_chart(ctx, "charts/transaction-count")?;
        let dates = last_30_days();
        let mut findings = vec![];
        let mut checked = 0u64;

        for date in &dates {
            if let (Some(our_val), Some(explorer_val)) =
                (our_data.get(date), explorer_data.get(date))
            {
                let ours: i64 = our_val.parse().unwrap_or(0);
                if let Some(f) = compare_exact_i64(ours, explorer_val, date, "transactions_count") {
                    findings.push(f);
                }
                checked += 1;
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

/// X2: Compare /dao/charts/total-deposit vs explorer total_dao_deposit.
pub struct ExplorerTotalDeposit;

impl Check for ExplorerTotalDeposit {
    fn name(&self) -> &'static str {
        "explorer_total_deposit"
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
        let our_data = fetch_our_chart(ctx, "dao/charts/total-deposit")?;
        let dates = last_30_days();
        let mut findings = vec![];
        let mut checked = 0u64;

        for date in &dates {
            if let (Some(our_val), Some(explorer_val)) =
                (our_data.get(date), explorer_data.get(date))
            {
                let ours: i128 = our_val.parse().unwrap_or(0);
                if let Some(f) = compare_exact_i128(ours, explorer_val, date, "total_dao_deposit") {
                    findings.push(f);
                }
                checked += 1;
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

/// X3: Compare /charts/hash-rate vs explorer avg_hash_rate (tolerance-based).
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
        let our_data = fetch_our_chart(ctx, "charts/hash-rate")?;
        let dates = last_30_days();
        let mut findings = vec![];
        let mut checked = 0u64;

        for date in &dates {
            if let (Some(our_val), Some(explorer_val)) =
                (our_data.get(date), explorer_data.get(date))
            {
                let ours: f64 = our_val.parse().unwrap_or(0.0);
                if let Some(f) =
                    compare_tolerance_f64(ours, explorer_val, date, "avg_hash_rate", ctx.tolerance)
                {
                    findings.push(f);
                }
                checked += 1;
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

/// X4: Compare /charts/difficulty vs explorer avg_difficulty (tolerance-based).
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
        let our_data = fetch_our_chart(ctx, "charts/difficulty")?;
        let dates = last_30_days();
        let mut findings = vec![];
        let mut checked = 0u64;

        for date in &dates {
            if let (Some(our_val), Some(explorer_val)) =
                (our_data.get(date), explorer_data.get(date))
            {
                let ours: f64 = our_val.parse().unwrap_or(0.0);
                if let Some(f) =
                    compare_tolerance_f64(ours, explorer_val, date, "avg_difficulty", ctx.tolerance)
                {
                    findings.push(f);
                }
                checked += 1;
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

/// X5: Compare /charts/knowledge-size vs explorer occupied_capacity.
pub struct ExplorerKnowledgeSize;

impl Check for ExplorerKnowledgeSize {
    fn name(&self) -> &'static str {
        "explorer_knowledge_size"
    }
    fn description(&self) -> &'static str {
        "Daily knowledge_size vs explorer occupied_capacity"
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
        let our_data = fetch_our_chart(ctx, "charts/knowledge-size")?;
        let dates = last_30_days();
        let mut findings = vec![];
        let mut checked = 0u64;

        for date in &dates {
            if let (Some(our_val), Some(explorer_val)) =
                (our_data.get(date), explorer_data.get(date))
            {
                let ours: i128 = our_val.parse().unwrap_or(0);
                if let Some(f) = compare_exact_i128(ours, explorer_val, date, "knowledge_size") {
                    findings.push(f);
                }
                checked += 1;
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
        Box::new(ExplorerTotalDeposit),
        Box::new(ExplorerHashRate),
        Box::new(ExplorerDifficulty),
        Box::new(ExplorerKnowledgeSize),
    ]
}
