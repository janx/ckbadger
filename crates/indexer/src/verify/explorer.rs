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
struct ChartDataPoint {
    date: String,
    value: String,
    #[serde(default)]
    value2: Option<String>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ChartResponse {
    data: Vec<ChartDataPoint>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct StackedAreaDataPoint {
    date: String,
    values: HashMap<String, String>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct StackedAreaChartResponse {
    data: Vec<StackedAreaDataPoint>,
}

// ---------------------------------------------------------------------------
// Explorer NervosDAO API types
// ---------------------------------------------------------------------------

#[derive(serde::Deserialize)]
struct ExplorerNervosDaoResponse {
    data: ExplorerNervosDaoData,
}

#[derive(serde::Deserialize)]
struct ExplorerNervosDaoData {
    attributes: ExplorerNervosDaoAttributes,
}

#[derive(serde::Deserialize)]
struct ExplorerNervosDaoAttributes {
    total_deposit: String,
    depositors_count: String,
    unclaimed_compensation: String,
    claimed_compensation: String,
    average_deposit_time: String,
    mining_reward: String,
    deposit_compensation: String,
    treasury_amount: String,
    estimated_apc: String,
}

/// Our `/dao/statistics` response (subset of fields needed for verification).
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct OurDaoStatisticsResponse {
    total_deposited: String,
    total_depositors: i32,
    total_compensation_paid: String,
    unclaimed_compensation: String,
    average_deposit_days: String,
    estimated_apc: String,
    mining_reward: String,
    deposit_compensation: String,
    burnt: String,
    #[allow(dead_code)]
    #[serde(default)]
    pending_withdrawal_capacity: String,
}

// ---------------------------------------------------------------------------
// Explorer API caching
// ---------------------------------------------------------------------------

/// Cached explorer response stored as JSON on disk.
#[derive(serde::Serialize, serde::Deserialize)]
struct CacheEntry {
    #[serde(default = "default_cache_version")]
    version: u8,
    fetched_at: String,
    indicator: String,
    data: HashMap<String, String>,
}

const CACHE_FRESHNESS_SECS: i64 = 24 * 60 * 60; // 24 hours
const CACHE_VERSION: u8 = 3;

fn default_cache_version() -> u8 {
    1
}

/// Cache v2 incorrectly shifted explorer dates back by one day.
/// Normalize v2 cache keys in memory by shifting them forward one day.
fn remap_v2_cache_dates(data: &HashMap<String, String>) -> HashMap<String, String> {
    let mut remapped = HashMap::with_capacity(data.len());
    for (date, value) in data {
        if let Ok(d) = chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d") {
            let fixed = (d + chrono::Duration::days(1))
                .format("%Y-%m-%d")
                .to_string();
            remapped.insert(fixed, value.clone());
        } else {
            remapped.insert(date.clone(), value.clone());
        }
    }
    remapped
}

/// Read a cache file for the given indicator. Returns None if file doesn't exist.
fn read_cache(cache_dir: &Option<PathBuf>, indicator: &str) -> Option<CacheEntry> {
    let dir = cache_dir.as_ref()?;
    let path = dir.join(format!("{}.json", indicator));
    let content = std::fs::read_to_string(&path).ok()?;
    let mut entry: CacheEntry = serde_json::from_str(&content).ok()?;
    if entry.version < CACHE_VERSION {
        if entry.version == 2 {
            entry.data = remap_v2_cache_dates(&entry.data);
        }
        entry.version = CACHE_VERSION;
    }
    Some(entry)
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
        version: CACHE_VERSION,
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

/// Fetch a single explorer statistic value from `/api/v1/statistics/{name}`.
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
            let ts_val = match attrs.get("created_at_unixtimestamp").and_then(|v| {
                v.as_str()
                    .and_then(|s| s.parse::<i64>().ok())
                    .or_else(|| v.as_i64())
            }) {
                Some(ts) if ts > 0 => ts,
                _ => continue,
            };

            let date = explorer_timestamp_to_date(ts_val).unwrap_or_default();

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

/// Explorer's `created_at_unixtimestamp` maps directly to the stats date in UTC+8.
fn explorer_timestamp_to_date(ts_val: i64) -> Option<String> {
    let dt = chrono::DateTime::from_timestamp(ts_val, 0)?;
    let utc8 = chrono::FixedOffset::east_opt(ckbadger_common::CKB_UTC8_OFFSET)?;
    let day = dt.with_timezone(&utc8).date_naive();
    Some(day.format("%Y-%m-%d").to_string())
}

// ---------------------------------------------------------------------------
// Helpers: fetch our data from the ckbadger API charts
// ---------------------------------------------------------------------------

// api_get is imported via `use super::checks::*` above.

/// Normalize date string to "YYYY-MM-DD" format.
/// Handles "YYYYMMDD", "YYYY/MM/DD", and "YYYY-MM-DD" inputs.
fn normalize_date(date: &str) -> String {
    let d = date.replace('/', "-");
    if d.len() == 8 && !d.contains('-') {
        format!("{}-{}-{}", &d[0..4], &d[4..6], &d[6..8])
    } else {
        d
    }
}

/// Fetch our chart data as a date→value map. Works for simple ChartResponse endpoints.
fn fetch_our_chart(
    ctx: &CheckContext,
    chart_path: &str,
) -> anyhow::Result<HashMap<String, String>> {
    let wrapper: ChartResponse = api_get(ctx, chart_path)?;
    let mut map = HashMap::new();
    for point in wrapper.data {
        map.insert(normalize_date(&point.date), point.value);
    }
    Ok(map)
}

/// Fetch our stacked area chart data and extract a specific series key as a date→value map.
fn fetch_our_stacked_chart(
    ctx: &CheckContext,
    chart_path: &str,
    series_key: &str,
) -> anyhow::Result<HashMap<String, String>> {
    let wrapper: StackedAreaChartResponse = api_get(ctx, chart_path)?;
    let mut map = HashMap::new();
    for point in wrapper.data {
        if let Some(val) = point.values.get(series_key) {
            map.insert(normalize_date(&point.date), val.clone());
        }
    }
    Ok(map)
}

/// Fetch our stacked area chart data and sum multiple series keys as a date→value map.
fn fetch_our_stacked_chart_sum(
    ctx: &CheckContext,
    chart_path: &str,
    series_keys: &[&str],
) -> anyhow::Result<HashMap<String, String>> {
    let wrapper: StackedAreaChartResponse = api_get(ctx, chart_path)?;
    let mut map = HashMap::new();
    for point in wrapper.data {
        let sum: f64 = series_keys
            .iter()
            .filter_map(|k| point.values.get(*k))
            .filter_map(|v| v.parse::<f64>().ok())
            .sum();
        map.insert(normalize_date(&point.date), format!("{sum:.0}"));
    }
    Ok(map)
}

/// Fetch the NervosDAO contract summary from the official CKB explorer API, with file-based caching.
fn fetch_explorer_nervos_dao(ctx: &CheckContext) -> anyhow::Result<ExplorerNervosDaoAttributes> {
    let cache_key = "nervos_dao";

    // 1. Try fresh cache first
    if let Some(cached) = read_cache(&ctx.cache_dir, cache_key) {
        if is_cache_fresh(&cached) {
            // Reconstruct attributes from cached data map
            return nervos_dao_from_cache(&cached.data);
        }
    }

    // 2. Fetch from API
    let explorer_url = ctx
        .explorer_url
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Explorer URL not set"))?;
    let url = format!(
        "{}/api/v1/contracts/nervos_dao",
        explorer_url.trim_end_matches('/')
    );

    let fetch_result = (|| -> anyhow::Result<ExplorerNervosDaoAttributes> {
        let resp = ctx
            .http
            .get(&url)
            .header("Content-Type", "application/vnd.api+json")
            .header("Accept", "application/vnd.api+json")
            .send()?;
        let status = resp.status();
        if !status.is_success() {
            anyhow::bail!("GET {} returned {}", url, status);
        }
        let parsed: ExplorerNervosDaoResponse = resp.json()?;
        Ok(parsed.data.attributes)
    })();

    match fetch_result {
        Ok(attrs) => {
            // Cache as flat key-value map
            let mut data = HashMap::new();
            data.insert("total_deposit".to_string(), attrs.total_deposit.clone());
            data.insert(
                "depositors_count".to_string(),
                attrs.depositors_count.clone(),
            );
            data.insert(
                "unclaimed_compensation".to_string(),
                attrs.unclaimed_compensation.clone(),
            );
            data.insert(
                "claimed_compensation".to_string(),
                attrs.claimed_compensation.clone(),
            );
            data.insert(
                "average_deposit_time".to_string(),
                attrs.average_deposit_time.clone(),
            );
            data.insert("mining_reward".to_string(), attrs.mining_reward.clone());
            data.insert(
                "deposit_compensation".to_string(),
                attrs.deposit_compensation.clone(),
            );
            data.insert("treasury_amount".to_string(), attrs.treasury_amount.clone());
            data.insert("estimated_apc".to_string(), attrs.estimated_apc.clone());
            write_cache(&ctx.cache_dir, cache_key, &data);
            Ok(attrs)
        }
        Err(e) => {
            // 3. Fall back to stale cache
            if let Some(cached) = read_cache(&ctx.cache_dir, cache_key) {
                eprintln!(
                    "    {} Explorer fetch for '{}' failed ({}), using stale cache from {}",
                    style("⚠").yellow(),
                    cache_key,
                    e,
                    cached.fetched_at,
                );
                nervos_dao_from_cache(&cached.data)
            } else {
                Err(e)
            }
        }
    }
}

fn nervos_dao_from_cache(
    data: &HashMap<String, String>,
) -> anyhow::Result<ExplorerNervosDaoAttributes> {
    let get = |key: &str| -> anyhow::Result<String> {
        data.get(key)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("missing cached field '{}'", key))
    };
    Ok(ExplorerNervosDaoAttributes {
        total_deposit: get("total_deposit")?,
        depositors_count: get("depositors_count")?,
        unclaimed_compensation: get("unclaimed_compensation")?,
        claimed_compensation: get("claimed_compensation")?,
        average_deposit_time: get("average_deposit_time")?,
        mining_reward: get("mining_reward")?,
        deposit_compensation: get("deposit_compensation")?,
        treasury_amount: get("treasury_amount")?,
        estimated_apc: get("estimated_apc")?,
    })
}

/// Parse our `averageDepositDays` formatted string back to a float.
///
/// Handles formats like `"1198 days"`, `"1.2K days+"`, `"0.3 days"`.
fn parse_average_deposit_days(s: &str) -> Option<f64> {
    let s = s
        .trim()
        .trim_end_matches('+')
        .trim_end_matches(" days")
        .trim_end_matches("days");
    if s.ends_with('K') {
        s.trim_end_matches('K')
            .trim()
            .parse::<f64>()
            .ok()
            .map(|v| v * 1000.0)
    } else {
        s.trim().parse::<f64>().ok()
    }
}

/// Fetch our chart data and return `value2` as a date→value map (for depositors_count series).
fn fetch_our_chart_value2(
    ctx: &CheckContext,
    chart_path: &str,
) -> anyhow::Result<HashMap<String, String>> {
    let wrapper: ChartResponse = api_get(ctx, chart_path)?;
    let mut map = HashMap::new();
    for point in wrapper.data {
        if let Some(v2) = point.value2 {
            map.insert(normalize_date(&point.date), v2);
        }
    }
    Ok(map)
}

/// Derive weighted average block time (milliseconds) from distribution buckets.
fn weighted_avg_block_time_ms_from_distribution(points: &[ChartDataPoint]) -> Option<f64> {
    let mut weighted_sum_seconds = 0.0f64;
    let mut ratio_sum = 0.0f64;

    for point in points {
        let bucket_seconds = point.date.parse::<f64>().ok()?;
        let ratio = point.value.parse::<f64>().ok()?;
        if bucket_seconds < 0.0 || ratio < 0.0 {
            return None;
        }
        weighted_sum_seconds += bucket_seconds * ratio;
        ratio_sum += ratio;
    }

    if ratio_sum <= 0.0 {
        return None;
    }
    Some((weighted_sum_seconds / ratio_sum) * 1000.0)
}

// ---------------------------------------------------------------------------
// Comparison helpers
// ---------------------------------------------------------------------------

/// Compare integer values: our API vs explorer, requiring exact match.
fn compare_exact_i64(ours: i64, theirs: &str, date: &str, label: &str) -> Option<Finding> {
    let their_val: i64 = match theirs.parse() {
        Ok(v) => v,
        Err(_) => {
            return Some(Finding {
                entity: date.to_string(),
                details: vec![format!(
                    "{}: explorer value '{}' could not be parsed as i64",
                    label, theirs,
                )],
            })
        }
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

/// Parse a CKB decimal string (e.g. "270263243.54537001") back to shannons (i128).
/// The API's `shannon_to_ckb` formats as `{integer}.{remainder:08}` with trailing zeros trimmed.
fn parse_ckb_to_shannon(ckb: &str) -> Option<i128> {
    const SHANNON_PER_CKB: i128 = 100_000_000;

    if let Some(dot_pos) = ckb.find('.') {
        let integer_part: i128 = ckb[..dot_pos].parse().ok()?;
        let decimal_str = &ckb[dot_pos + 1..];
        // Pad right to 8 digits (shannon_to_ckb uses {:08} format, then trims trailing zeros)
        let padded = format!("{:0<8}", decimal_str);
        let decimal_part: i128 = padded[..8].parse().ok()?;
        Some(integer_part * SHANNON_PER_CKB + decimal_part)
    } else {
        // No decimal point — whole CKB value
        let integer_part: i128 = ckb.parse().ok()?;
        Some(integer_part * SHANNON_PER_CKB)
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
        Err(_) => {
            return Some(Finding {
                entity: date.to_string(),
                details: vec![format!(
                    "{}: explorer value '{}' could not be parsed as f64",
                    label, theirs,
                )],
            })
        }
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

/// Get the last 30 completed days (excluding today) using UTC+8 boundaries.
fn last_30_days() -> Vec<String> {
    let utc8 = chrono::FixedOffset::east_opt(ckbadger_common::CKB_UTC8_OFFSET).unwrap();
    let today = chrono::Utc::now().with_timezone(&utc8).date_naive();
    (1..=30)
        .map(|i| {
            (today - chrono::Duration::days(i))
                .format("%Y-%m-%d")
                .to_string()
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Common explorer check runner to reduce boilerplate
// ---------------------------------------------------------------------------

fn compute_baseline_offset_f64(
    our_data: &HashMap<String, String>,
    explorer_data: &HashMap<String, String>,
    dates: &[String],
    value_transform: &impl Fn(&str, &str) -> Option<(f64, f64)>,
) -> Option<f64> {
    for date in dates {
        if let (Some(our_val), Some(explorer_val)) = (our_data.get(date), explorer_data.get(date)) {
            if let Some((ours, theirs)) = value_transform(our_val, explorer_val) {
                return Some(ours - theirs);
            }
        }
    }
    None
}

fn compute_baseline_offset_i128(
    our_data: &HashMap<String, String>,
    explorer_data: &HashMap<String, String>,
    dates: &[String],
    parse_our: impl Fn(&str) -> Option<i128>,
    parse_theirs: impl Fn(&str) -> Option<i128>,
) -> Option<i128> {
    for date in dates {
        if let (Some(our_val), Some(explorer_val)) = (our_data.get(date), explorer_data.get(date)) {
            let ours = parse_our(our_val)?;
            let theirs = parse_theirs(explorer_val)?;
            return Some(ours - theirs);
        }
    }
    None
}

/// Run an exact i128 explorer comparison after baseline alignment.
///
/// Cumulative metrics can carry a fixed historical offset between data sources
/// (for example, legacy snapshot baselines). We align to the first overlapping
/// date, then enforce exact match on the remaining relative series.
fn run_exact_i128_explorer_check_with_offset(
    our_data: &HashMap<String, String>,
    explorer_data: &HashMap<String, String>,
    progress: &ProgressReporter,
    label: &str,
    parse_our: impl Fn(&str) -> Option<i128>,
) -> CheckResult {
    let dates = last_30_days();
    let baseline_offset =
        compute_baseline_offset_i128(our_data, explorer_data, &dates, &parse_our, |v| {
            v.parse::<i128>().ok()
        })
        .unwrap_or(0);

    let mut findings = vec![];
    let mut checked = 0u64;
    for date in &dates {
        if let (Some(our_val), Some(explorer_val)) = (our_data.get(date), explorer_data.get(date)) {
            if let (Some(ours_raw), Ok(theirs)) = (parse_our(our_val), explorer_val.parse::<i128>())
            {
                let ours = ours_raw - baseline_offset;
                if ours != theirs {
                    findings.push(Finding {
                        entity: date.to_string(),
                        details: vec![format!(
                            "{}: ours={}, explorer={} (Δ {:+}, baseline_offset={:+})",
                            label,
                            format_number_i128(ours),
                            format_number_i128(theirs),
                            ours - theirs,
                            baseline_offset,
                        )],
                    });
                }
                checked += 1;
            }
        }
        progress.inc(1);
    }

    if checked == 0 {
        return CheckResult {
            passed: false,
            items_checked: 0,
            items_failed: 0,
            detail: Some("no overlapping dates found between local and explorer data".to_string()),
            findings: vec![Finding {
                entity: "overlap".to_string(),
                details: vec![
                    "no overlapping dates found between local and explorer data".to_string()
                ],
            }],
        };
    }

    if findings.is_empty() {
        CheckResult::pass(checked)
    } else {
        CheckResult::fail(checked, findings)
    }
}

/// Run a tolerance-based explorer comparison over the last 30 days.
/// `value_transform` converts (our_value_str, explorer_value_str) → (our_f64, explorer_f64).
fn run_tolerance_explorer_check(
    our_data: &HashMap<String, String>,
    explorer_data: &HashMap<String, String>,
    progress: &ProgressReporter,
    label: &str,
    tolerance: f64,
    value_transform: impl Fn(&str, &str) -> Option<(f64, f64)>,
) -> CheckResult {
    run_tolerance_explorer_check_internal(
        our_data,
        explorer_data,
        progress,
        label,
        tolerance,
        false,
        value_transform,
    )
}

/// Run a tolerance-based explorer comparison with baseline alignment.
fn run_tolerance_explorer_check_with_offset(
    our_data: &HashMap<String, String>,
    explorer_data: &HashMap<String, String>,
    progress: &ProgressReporter,
    label: &str,
    tolerance: f64,
    value_transform: impl Fn(&str, &str) -> Option<(f64, f64)>,
) -> CheckResult {
    run_tolerance_explorer_check_internal(
        our_data,
        explorer_data,
        progress,
        label,
        tolerance,
        true,
        value_transform,
    )
}

fn run_tolerance_explorer_check_internal(
    our_data: &HashMap<String, String>,
    explorer_data: &HashMap<String, String>,
    progress: &ProgressReporter,
    label: &str,
    tolerance: f64,
    align_baseline: bool,
    value_transform: impl Fn(&str, &str) -> Option<(f64, f64)>,
) -> CheckResult {
    let dates = last_30_days();
    let baseline_offset = if align_baseline {
        compute_baseline_offset_f64(our_data, explorer_data, &dates, &value_transform)
            .unwrap_or(0.0)
    } else {
        0.0
    };
    let mut findings = vec![];
    let mut checked = 0u64;

    for date in &dates {
        if let (Some(our_val), Some(explorer_val)) = (our_data.get(date), explorer_data.get(date)) {
            if let Some((ours, theirs)) = value_transform(our_val, explorer_val) {
                let aligned_ours = ours - baseline_offset;
                if let Some(f) =
                    compare_tolerance_f64_values(aligned_ours, theirs, date, label, tolerance)
                {
                    findings.push(f);
                }
                checked += 1;
            }
        }
        progress.inc(1);
    }

    if checked == 0 {
        return CheckResult {
            passed: false,
            items_checked: 0,
            items_failed: 0,
            detail: Some("no overlapping dates found between local and explorer data".to_string()),
            findings: vec![Finding {
                entity: "overlap".to_string(),
                details: vec![
                    "no overlapping dates found between local and explorer data".to_string()
                ],
            }],
        };
    }

    if findings.is_empty() {
        CheckResult::pass(checked)
    } else {
        CheckResult::fail(checked, findings)
    }
}

/// Compare two f64 values with tolerance (like `compare_tolerance_f64` but takes f64 directly).
fn compare_tolerance_f64_values(
    ours: f64,
    theirs: f64,
    date: &str,
    label: &str,
    tolerance: f64,
) -> Option<Finding> {
    if theirs == 0.0 && ours == 0.0 {
        return None;
    }
    let denom = if theirs.abs() > f64::EPSILON {
        theirs.abs()
    } else {
        1.0
    };
    let deviation = ((ours - theirs) / denom).abs();
    if deviation > tolerance {
        Some(Finding {
            entity: date.to_string(),
            details: vec![format!(
                "{}: ours={:.6}, explorer={:.6} (deviation: {:.4}%, tolerance: {:.4}%)",
                label,
                ours,
                theirs,
                deviation * 100.0,
                tolerance * 100.0,
            )],
        })
    } else {
        None
    }
}

// ============================================
// Explorer checks (X1-X16)
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

        // Our snapshot total_deposited subtracts at phase-1 (withdraw
        // request), matching the CKB explorer convention.  Convert our
        // CKB-denominated chart values to shannons for direct comparison.
        let our_shannons: HashMap<String, String> = our_data
            .into_iter()
            .filter_map(|(date, val)| {
                let shannons = parse_ckb_to_shannon(&val)?;
                Some((date, shannons.to_string()))
            })
            .collect();

        Ok(run_exact_i128_explorer_check_with_offset(
            &our_shannons,
            &explorer_data,
            progress,
            "total_dao_deposit",
            |v| v.parse::<i128>().ok(),
        ))
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
                let ours: f64 = our_val.parse::<f64>().unwrap_or(0.0);
                if let Some(f) =
                    compare_tolerance_f64(ours, explorer_val, date, "avg_hash_rate", 0.02)
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
                    compare_tolerance_f64(ours, explorer_val, date, "avg_difficulty", 0.02)
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

        // Tolerance comparison: CKB Explorer uses UTC+8 (Beijing time) for daily
        // boundaries while we use UTC, so the "last block of the day" differs by
        // ~8 hours. For point-in-time values like occupied_capacity this means
        // the DAO U field is read at a different block height. Observed deviation
        // is typically <0.2%.
        for date in &dates {
            if let (Some(our_val), Some(explorer_val)) =
                (our_data.get(date), explorer_data.get(date))
            {
                // Our API returns CKB units (via shannon_to_ckb_string), but the
                // explorer returns raw shannons.  Convert our value back to shannons
                // so the comparison is apples-to-apples.
                let ours: f64 = parse_ckb_to_shannon(our_val)
                    .map(|s| s as f64)
                    .unwrap_or_else(|| our_val.parse().unwrap_or(0.0));
                if let Some(f) =
                    compare_tolerance_f64(ours, explorer_val, date, "knowledge_size", 0.002)
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

/// X6: Compare /charts/uncle-rate vs explorer uncle_rate (tolerance-based).
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
        let our_data = fetch_our_chart(ctx, "charts/uncle-rate")?;
        Ok(run_tolerance_explorer_check(
            &our_data,
            &explorer_data,
            progress,
            "uncle_rate",
            0.002,
            |ours, theirs| {
                let o: f64 = ours.parse().ok()?;
                let t: f64 = theirs.parse().ok()?;
                Some((o, t))
            },
        ))
    }
}

/// X7: Compare /charts/cell-count (liveCells) vs explorer live_cells_count (tolerance-based).
pub struct ExplorerLiveCellCount;

impl Check for ExplorerLiveCellCount {
    fn name(&self) -> &'static str {
        "explorer_live_cell_count"
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
        let our_data = fetch_our_stacked_chart(ctx, "charts/cell-count", "liveCells")?;
        Ok(run_tolerance_explorer_check(
            &our_data,
            &explorer_data,
            progress,
            "live_cells_count",
            0.002,
            |ours, theirs| {
                let o: f64 = ours.parse().ok()?;
                let t: f64 = theirs.parse().ok()?;
                Some((o, t))
            },
        ))
    }
}

/// X8: Compare /charts/cell-count (deadCells) vs explorer dead_cells_count (tolerance-based).
pub struct ExplorerDeadCellCount;

impl Check for ExplorerDeadCellCount {
    fn name(&self) -> &'static str {
        "explorer_dead_cell_count"
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
        let our_data = fetch_our_stacked_chart(ctx, "charts/cell-count", "deadCells")?;
        Ok(run_tolerance_explorer_check(
            &our_data,
            &explorer_data,
            progress,
            "dead_cells_count",
            0.002,
            |ours, theirs| {
                let o: f64 = ours.parse().ok()?;
                let t: f64 = theirs.parse().ok()?;
                Some((o, t))
            },
        ))
    }
}

/// X9: Compare /dao/charts/daily-deposit vs explorer daily_dao_deposit (tolerance-based).
/// Our API returns CKB, explorer returns shannons.
pub struct ExplorerDailyDeposit;

impl Check for ExplorerDailyDeposit {
    fn name(&self) -> &'static str {
        "explorer_daily_deposit"
    }
    fn description(&self) -> &'static str {
        "Daily daily_dao_deposit vs explorer"
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
        let explorer_data = fetch_explorer_daily(ctx, "daily_dao_deposit", "daily_dao_deposit")?;
        let our_data = fetch_our_chart(ctx, "dao/charts/daily-deposit")?;
        Ok(run_tolerance_explorer_check(
            &our_data,
            &explorer_data,
            progress,
            "daily_dao_deposit",
            0.002,
            |ours, theirs| {
                let o = parse_ckb_to_shannon(ours)? as f64;
                let t: f64 = theirs.parse().ok()?;
                Some((o, t))
            },
        ))
    }
}

/// X10: Compare /dao/charts/circulation-ratio vs explorer circulation_ratio (tolerance-based).
/// Our API returns percentage (e.g., "15.5600"), explorer returns decimal (e.g., "0.1556").
pub struct ExplorerCirculationRatio;

impl Check for ExplorerCirculationRatio {
    fn name(&self) -> &'static str {
        "explorer_circulation_ratio"
    }
    fn description(&self) -> &'static str {
        "Daily circulation_ratio vs explorer"
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
        let explorer_data = fetch_explorer_daily(ctx, "circulation_ratio", "circulation_ratio")?;
        let our_data = fetch_our_chart(ctx, "dao/charts/circulation-ratio")?;
        Ok(run_tolerance_explorer_check(
            &our_data,
            &explorer_data,
            progress,
            "circulation_ratio",
            0.02,
            |ours, theirs| {
                let o: f64 = ours.parse().ok()?;
                let t: f64 = theirs.parse().ok()?;
                // Explorer returns decimal fraction, our API returns percentage
                Some((o, t * 100.0))
            },
        ))
    }
}

/// X11: Compare /charts/total-supply (circulating + nervosdao) vs explorer circulating_supply (tolerance-based).
/// Explorer's circulating_supply includes DAO-locked CKB. Our API returns CKB, explorer returns shannons.
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
        // Explorer's circulating_supply includes DAO-locked CKB, so sum both series.
        let our_data =
            fetch_our_stacked_chart_sum(ctx, "charts/total-supply", &["circulating", "nervosdao"])?;
        Ok(run_tolerance_explorer_check_with_offset(
            &our_data,
            &explorer_data,
            progress,
            "circulating_supply",
            0.002,
            |ours, theirs| {
                let o = parse_ckb_to_shannon(ours)? as f64;
                let t: f64 = theirs.parse().ok()?;
                Some((o, t))
            },
        ))
    }
}

/// X12: Compare /charts/total-supply (burnt) vs explorer burnt (tolerance-based).
/// Our API returns CKB, explorer returns shannons.
pub struct ExplorerBurnt;

impl Check for ExplorerBurnt {
    fn name(&self) -> &'static str {
        "explorer_burnt"
    }
    fn description(&self) -> &'static str {
        "Daily burnt vs explorer"
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
        let explorer_data = fetch_explorer_daily(ctx, "burnt", "burnt")?;
        let our_data = fetch_our_stacked_chart(ctx, "charts/total-supply", "burnt")?;
        Ok(run_tolerance_explorer_check(
            &our_data,
            &explorer_data,
            progress,
            "burnt",
            0.002,
            |ours, theirs| {
                let o = parse_ckb_to_shannon(ours)? as f64;
                let t: f64 = theirs.parse().ok()?;
                Some((o, t))
            },
        ))
    }
}

/// X13: Compare /charts/secondary-issuance (compensation) vs explorer deposit_compensation (tolerance-based).
/// Our API returns CKB, explorer returns shannons.
pub struct ExplorerDepositCompensation;

impl Check for ExplorerDepositCompensation {
    fn name(&self) -> &'static str {
        "explorer_deposit_compensation"
    }
    fn description(&self) -> &'static str {
        "Daily deposit_compensation vs explorer"
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
        let explorer_data =
            fetch_explorer_daily(ctx, "deposit_compensation", "deposit_compensation")?;
        let our_data = fetch_our_stacked_chart(ctx, "charts/secondary-issuance", "compensation")?;
        Ok(run_tolerance_explorer_check_with_offset(
            &our_data,
            &explorer_data,
            progress,
            "deposit_compensation",
            0.002,
            |ours, theirs| {
                let o = parse_ckb_to_shannon(ours)? as f64;
                let t: f64 = theirs.parse().ok()?;
                Some((o, t))
            },
        ))
    }
}

/// X14: Compare /charts/secondary-issuance (mining) vs explorer mining_reward (tolerance-based).
/// Our API returns CKB, explorer returns shannons.
pub struct ExplorerMiningReward;

impl Check for ExplorerMiningReward {
    fn name(&self) -> &'static str {
        "explorer_mining_reward"
    }
    fn description(&self) -> &'static str {
        "Daily mining_reward vs explorer"
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
        let explorer_data = fetch_explorer_daily(ctx, "mining_reward", "mining_reward")?;
        let our_data = fetch_our_stacked_chart(ctx, "charts/secondary-issuance", "mining")?;
        Ok(run_tolerance_explorer_check(
            &our_data,
            &explorer_data,
            progress,
            "mining_reward",
            0.002,
            |ours, theirs| {
                let o = parse_ckb_to_shannon(ours)? as f64;
                let t: f64 = theirs.parse().ok()?;
                Some((o, t))
            },
        ))
    }
}

/// X15: Compare /charts/secondary-issuance (burnt) vs explorer treasury_amount (tolerance-based).
/// Our API returns CKB, explorer returns shannons.
pub struct ExplorerTreasuryAmount;

impl Check for ExplorerTreasuryAmount {
    fn name(&self) -> &'static str {
        "explorer_treasury_amount"
    }
    fn description(&self) -> &'static str {
        "Daily treasury_amount vs explorer"
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
        let explorer_data = fetch_explorer_daily(ctx, "treasury_amount", "treasury_amount")?;
        let our_data = fetch_our_stacked_chart(ctx, "charts/secondary-issuance", "burnt")?;
        Ok(run_tolerance_explorer_check_with_offset(
            &our_data,
            &explorer_data,
            progress,
            "treasury_amount",
            0.002,
            |ours, theirs| {
                let o = parse_ckb_to_shannon(ours)? as f64;
                let t: f64 = theirs.parse().ok()?;
                Some((o, t))
            },
        ))
    }
}

/// X16: Compare weighted average from /charts/block-time-distribution vs
/// recent daily averages from /charts/average-block-time (internal consistency).
pub struct BlockTimeDistributionConsistency;

/// Number of recent daily averages to compare against the distribution.
/// 42 epochs ≈ 7 days at ~4h/epoch, so 7 days covers the window.
const BLOCK_TIME_CONSISTENCY_RECENT_DAYS: usize = 7;

impl Check for BlockTimeDistributionConsistency {
    fn name(&self) -> &'static str {
        "block_time_distribution_consistency"
    }
    fn description(&self) -> &'static str {
        "Block-time distribution weighted avg vs recent daily avg_block_time"
    }
    fn tier(&self) -> CheckTier {
        CheckTier::Sampling
    }
    fn requires_explorer(&self) -> bool {
        false
    }
    fn estimated_total(&self, _ctx: &CheckContext) -> Option<u64> {
        Some(1)
    }
    fn run(&self, ctx: &CheckContext, _progress: &ProgressReporter) -> anyhow::Result<CheckResult> {
        // 1. Weighted average from distribution (last ~50k blocks)
        let distribution: ChartResponse = api_get(ctx, "charts/block-time-distribution")?;

        let has_nonzero = distribution
            .data
            .iter()
            .any(|p| p.value.parse::<f64>().unwrap_or(0.0) > 0.0);
        if distribution.data.is_empty() || !has_nonzero {
            return Ok(CheckResult::fail(
                1,
                vec![Finding {
                    entity: "distribution".to_string(),
                    details: vec![format!(
                        "block-time-distribution chart has no data ({} points, all ratios zero)",
                        distribution.data.len()
                    )],
                }],
            ));
        }

        let dist_ms = weighted_avg_block_time_ms_from_distribution(&distribution.data)
            .ok_or_else(|| anyhow::anyhow!(
                "failed to derive avg block time from distribution: {} points, unexpected parse failure",
                distribution.data.len()
            ))?;

        // 2. Average from recent daily stats
        let daily_chart: ChartResponse = api_get(ctx, "charts/average-block-time")?;
        let recent_days: Vec<&ChartDataPoint> = daily_chart
            .data
            .iter()
            .rev()
            .take(BLOCK_TIME_CONSISTENCY_RECENT_DAYS)
            .collect();

        if recent_days.is_empty() {
            return Ok(CheckResult::fail(
                1,
                vec![Finding {
                    entity: "daily".to_string(),
                    details: vec!["average-block-time chart has no data".to_string()],
                }],
            ));
        }

        let mut sum_ms = 0.0f64;
        let mut count = 0usize;
        for point in &recent_days {
            if let Ok(seconds) = point.value.parse::<f64>() {
                if seconds > 0.0 {
                    sum_ms += seconds * 1000.0;
                    count += 1;
                }
            }
        }

        if count == 0 {
            return Ok(CheckResult::fail(
                1,
                vec![Finding {
                    entity: "daily".to_string(),
                    details: vec!["no valid daily avg_block_time values in recent days".to_string()],
                }],
            ));
        }

        let daily_ms = sum_ms / count as f64;

        // 3. Compare — both from our own data, 20% tolerance for window mismatch
        let mut findings = vec![];
        if let Some(f) =
            compare_tolerance_f64_values(dist_ms, daily_ms, "latest", "avg_block_time_ms", 0.20)
        {
            findings.push(f);
        }

        if findings.is_empty() {
            Ok(CheckResult::pass_with_detail(
                1,
                format!("distribution={dist_ms:.2}ms, daily_avg={daily_ms:.2}ms ({count} days)",),
            ))
        } else {
            Ok(CheckResult::fail(1, findings))
        }
    }
}

// ============================================
// NervosDAO point-in-time checks (X17-X25)
// ============================================

/// Helper: compare a single shannon-valued field from our DAO stats vs explorer NervosDAO API.
fn run_nervos_dao_shannon_check(
    our_value: &str,
    explorer_value: &str,
    label: &str,
    tolerance: f64,
) -> CheckResult {
    let ours: f64 = match our_value.parse::<f64>() {
        Ok(v) => v,
        Err(_) => {
            return CheckResult::fail(
                1,
                vec![Finding {
                    entity: "dao/statistics".to_string(),
                    details: vec![format!(
                        "{}: failed to parse our value '{}'",
                        label, our_value
                    )],
                }],
            )
        }
    };
    let theirs: f64 = match explorer_value.parse::<f64>() {
        Ok(v) => v,
        Err(_) => {
            return CheckResult::fail(
                1,
                vec![Finding {
                    entity: "nervos_dao".to_string(),
                    details: vec![format!(
                        "{}: failed to parse explorer value '{}'",
                        label, explorer_value
                    )],
                }],
            )
        }
    };
    if let Some(f) = compare_tolerance_f64_values(ours, theirs, "latest", label, tolerance) {
        CheckResult::fail(1, vec![f])
    } else {
        CheckResult::pass(1)
    }
}

/// X17: Compare total_deposit from /dao/statistics vs explorer nervos_dao.
pub struct NervosDaoTotalDeposit;

impl Check for NervosDaoTotalDeposit {
    fn name(&self) -> &'static str {
        "nervos_dao_total_deposit"
    }
    fn description(&self) -> &'static str {
        "NervosDAO total_deposit vs explorer"
    }
    fn tier(&self) -> CheckTier {
        CheckTier::Sampling
    }
    fn requires_explorer(&self) -> bool {
        true
    }
    fn estimated_total(&self, _ctx: &CheckContext) -> Option<u64> {
        Some(1)
    }
    fn run(&self, ctx: &CheckContext, _progress: &ProgressReporter) -> anyhow::Result<CheckResult> {
        let explorer = fetch_explorer_nervos_dao(ctx)?;
        let ours: OurDaoStatisticsResponse = api_get(ctx, "dao/statistics")?;
        Ok(run_nervos_dao_shannon_check(
            &ours.total_deposited,
            &explorer.total_deposit,
            "total_deposit",
            0.005,
        ))
    }
}

/// X18: Compare depositors_count from /dao/statistics vs explorer nervos_dao.
pub struct NervosDaoDepositorsCount;

impl Check for NervosDaoDepositorsCount {
    fn name(&self) -> &'static str {
        "nervos_dao_depositors_count"
    }
    fn description(&self) -> &'static str {
        "NervosDAO depositors_count vs explorer"
    }
    fn tier(&self) -> CheckTier {
        CheckTier::Sampling
    }
    fn requires_explorer(&self) -> bool {
        true
    }
    fn estimated_total(&self, _ctx: &CheckContext) -> Option<u64> {
        Some(1)
    }
    fn run(&self, ctx: &CheckContext, _progress: &ProgressReporter) -> anyhow::Result<CheckResult> {
        let explorer = fetch_explorer_nervos_dao(ctx)?;
        let ours: OurDaoStatisticsResponse = api_get(ctx, "dao/statistics")?;
        let our_val = ours.total_depositors as f64;
        let their_val: f64 = explorer.depositors_count.parse().map_err(|_| {
            anyhow::anyhow!(
                "failed to parse explorer depositors_count '{}'",
                explorer.depositors_count
            )
        })?;
        if let Some(f) =
            compare_tolerance_f64_values(our_val, their_val, "latest", "depositors_count", 0.01)
        {
            Ok(CheckResult::fail(1, vec![f]))
        } else {
            Ok(CheckResult::pass(1))
        }
    }
}

/// X19: Compare unclaimed_compensation from /dao/statistics vs explorer nervos_dao.
pub struct NervosDaoUnclaimedCompensation;

impl Check for NervosDaoUnclaimedCompensation {
    fn name(&self) -> &'static str {
        "nervos_dao_unclaimed_compensation"
    }
    fn description(&self) -> &'static str {
        "NervosDAO unclaimed_compensation vs explorer"
    }
    fn tier(&self) -> CheckTier {
        CheckTier::Sampling
    }
    fn requires_explorer(&self) -> bool {
        true
    }
    fn estimated_total(&self, _ctx: &CheckContext) -> Option<u64> {
        Some(1)
    }
    fn run(&self, ctx: &CheckContext, _progress: &ProgressReporter) -> anyhow::Result<CheckResult> {
        let explorer = fetch_explorer_nervos_dao(ctx)?;
        let ours: OurDaoStatisticsResponse = api_get(ctx, "dao/statistics")?;
        Ok(run_nervos_dao_shannon_check(
            &ours.unclaimed_compensation,
            &explorer.unclaimed_compensation,
            "unclaimed_compensation",
            0.02,
        ))
    }
}

/// X20: Compare claimed_compensation from /dao/statistics vs explorer nervos_dao.
pub struct NervosDaoClaimedCompensation;

impl Check for NervosDaoClaimedCompensation {
    fn name(&self) -> &'static str {
        "nervos_dao_claimed_compensation"
    }
    fn description(&self) -> &'static str {
        "NervosDAO claimed_compensation vs explorer"
    }
    fn tier(&self) -> CheckTier {
        CheckTier::Sampling
    }
    fn requires_explorer(&self) -> bool {
        true
    }
    fn estimated_total(&self, _ctx: &CheckContext) -> Option<u64> {
        Some(1)
    }
    fn run(&self, ctx: &CheckContext, _progress: &ProgressReporter) -> anyhow::Result<CheckResult> {
        let explorer = fetch_explorer_nervos_dao(ctx)?;
        let ours: OurDaoStatisticsResponse = api_get(ctx, "dao/statistics")?;
        Ok(run_nervos_dao_shannon_check(
            &ours.total_compensation_paid,
            &explorer.claimed_compensation,
            "claimed_compensation",
            0.02,
        ))
    }
}

/// X21: Compare average_deposit_time from /dao/statistics vs explorer nervos_dao.
pub struct NervosDaoAverageDepositTime;

impl Check for NervosDaoAverageDepositTime {
    fn name(&self) -> &'static str {
        "nervos_dao_average_deposit_time"
    }
    fn description(&self) -> &'static str {
        "NervosDAO average_deposit_time vs explorer"
    }
    fn tier(&self) -> CheckTier {
        CheckTier::Sampling
    }
    fn requires_explorer(&self) -> bool {
        true
    }
    fn estimated_total(&self, _ctx: &CheckContext) -> Option<u64> {
        Some(1)
    }
    fn run(&self, ctx: &CheckContext, _progress: &ProgressReporter) -> anyhow::Result<CheckResult> {
        let explorer = fetch_explorer_nervos_dao(ctx)?;
        let ours: OurDaoStatisticsResponse = api_get(ctx, "dao/statistics")?;
        let our_days = parse_average_deposit_days(&ours.average_deposit_days).ok_or_else(|| {
            anyhow::anyhow!(
                "failed to parse our averageDepositDays '{}'",
                ours.average_deposit_days
            )
        })?;
        let their_days: f64 = explorer.average_deposit_time.parse().map_err(|_| {
            anyhow::anyhow!(
                "failed to parse explorer average_deposit_time '{}'",
                explorer.average_deposit_time
            )
        })?;
        if let Some(f) = compare_tolerance_f64_values(
            our_days,
            their_days,
            "latest",
            "average_deposit_time",
            0.05,
        ) {
            Ok(CheckResult::fail(1, vec![f]))
        } else {
            Ok(CheckResult::pass(1))
        }
    }
}

/// X22: Compare mining_reward from /dao/statistics vs explorer nervos_dao.
pub struct NervosDaoMiningReward;

impl Check for NervosDaoMiningReward {
    fn name(&self) -> &'static str {
        "nervos_dao_mining_reward"
    }
    fn description(&self) -> &'static str {
        "NervosDAO mining_reward vs explorer"
    }
    fn tier(&self) -> CheckTier {
        CheckTier::Sampling
    }
    fn requires_explorer(&self) -> bool {
        true
    }
    fn estimated_total(&self, _ctx: &CheckContext) -> Option<u64> {
        Some(1)
    }
    fn run(&self, ctx: &CheckContext, _progress: &ProgressReporter) -> anyhow::Result<CheckResult> {
        let explorer = fetch_explorer_nervos_dao(ctx)?;
        let ours: OurDaoStatisticsResponse = api_get(ctx, "dao/statistics")?;
        Ok(run_nervos_dao_shannon_check(
            &ours.mining_reward,
            &explorer.mining_reward,
            "mining_reward",
            0.005,
        ))
    }
}

/// X23: Compare deposit_compensation from /dao/statistics vs explorer nervos_dao.
pub struct NervosDaoDepositCompensation;

impl Check for NervosDaoDepositCompensation {
    fn name(&self) -> &'static str {
        "nervos_dao_deposit_compensation"
    }
    fn description(&self) -> &'static str {
        "NervosDAO deposit_compensation vs explorer"
    }
    fn tier(&self) -> CheckTier {
        CheckTier::Sampling
    }
    fn requires_explorer(&self) -> bool {
        true
    }
    fn estimated_total(&self, _ctx: &CheckContext) -> Option<u64> {
        Some(1)
    }
    fn run(&self, ctx: &CheckContext, _progress: &ProgressReporter) -> anyhow::Result<CheckResult> {
        let explorer = fetch_explorer_nervos_dao(ctx)?;
        let ours: OurDaoStatisticsResponse = api_get(ctx, "dao/statistics")?;
        Ok(run_nervos_dao_shannon_check(
            &ours.deposit_compensation,
            &explorer.deposit_compensation,
            "deposit_compensation",
            0.005,
        ))
    }
}

/// X24: Compare treasury_amount (burnt) from /dao/statistics vs explorer nervos_dao.
pub struct NervosDaoTreasuryAmount;

impl Check for NervosDaoTreasuryAmount {
    fn name(&self) -> &'static str {
        "nervos_dao_treasury_amount"
    }
    fn description(&self) -> &'static str {
        "NervosDAO treasury_amount vs explorer"
    }
    fn tier(&self) -> CheckTier {
        CheckTier::Sampling
    }
    fn requires_explorer(&self) -> bool {
        true
    }
    fn estimated_total(&self, _ctx: &CheckContext) -> Option<u64> {
        Some(1)
    }
    fn run(&self, ctx: &CheckContext, _progress: &ProgressReporter) -> anyhow::Result<CheckResult> {
        let explorer = fetch_explorer_nervos_dao(ctx)?;
        let ours: OurDaoStatisticsResponse = api_get(ctx, "dao/statistics")?;
        Ok(run_nervos_dao_shannon_check(
            &ours.burnt,
            &explorer.treasury_amount,
            "treasury_amount",
            0.005,
        ))
    }
}

/// X25: Compare estimated_apc from /dao/statistics vs explorer nervos_dao.
pub struct NervosDaoEstimatedApc;

impl Check for NervosDaoEstimatedApc {
    fn name(&self) -> &'static str {
        "nervos_dao_estimated_apc"
    }
    fn description(&self) -> &'static str {
        "NervosDAO estimated_apc vs explorer"
    }
    fn tier(&self) -> CheckTier {
        CheckTier::Sampling
    }
    fn requires_explorer(&self) -> bool {
        true
    }
    fn estimated_total(&self, _ctx: &CheckContext) -> Option<u64> {
        Some(1)
    }
    fn run(&self, ctx: &CheckContext, _progress: &ProgressReporter) -> anyhow::Result<CheckResult> {
        let explorer = fetch_explorer_nervos_dao(ctx)?;
        let ours: OurDaoStatisticsResponse = api_get(ctx, "dao/statistics")?;
        let our_apc: f64 = ours.estimated_apc.parse().map_err(|_| {
            anyhow::anyhow!("failed to parse our estimatedApc '{}'", ours.estimated_apc)
        })?;
        let their_apc: f64 = explorer.estimated_apc.parse().map_err(|_| {
            anyhow::anyhow!(
                "failed to parse explorer estimated_apc '{}'",
                explorer.estimated_apc
            )
        })?;
        if let Some(f) =
            compare_tolerance_f64_values(our_apc, their_apc, "latest", "estimated_apc", 0.05)
        {
            Ok(CheckResult::fail(1, vec![f]))
        } else {
            Ok(CheckResult::pass(1))
        }
    }
}

// ============================================
// NervosDAO daily time-series checks (X26-X27)
// ============================================

/// X26: Compare /dao/charts/total-deposit value2 vs explorer total_depositors_count.
pub struct ExplorerTotalDepositorsCount;

impl Check for ExplorerTotalDepositorsCount {
    fn name(&self) -> &'static str {
        "explorer_total_depositors_count"
    }
    fn description(&self) -> &'static str {
        "Daily total_depositors_count vs explorer"
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
        let explorer_data =
            fetch_explorer_daily(ctx, "total_depositors_count", "total_depositors_count")?;
        let our_data = fetch_our_chart_value2(ctx, "dao/charts/total-deposit")?;
        // Wide tolerance: explorer uses incremental daily_dao_depositors_count
        // which re-counts returning depositors (deposit→withdraw→deposit again),
        // inflating the cumulative total.  Our cumulative_depositors tracks
        // first-time depositors only via an ever_deposited set and is more
        // accurate.  Expect explorer > ours by ~15-20%.
        Ok(run_tolerance_explorer_check(
            &our_data,
            &explorer_data,
            progress,
            "total_depositors_count",
            0.20,
            |ours, theirs| {
                let o: f64 = ours.parse().ok()?;
                let t: f64 = theirs.parse().ok()?;
                Some((o, t))
            },
        ))
    }
}

// X27 (ExplorerDailyDaoDepositorsCount) removed: semantic mismatch with explorer
// is fundamental — explorer counts addresses transitioning from non-active to active,
// we count all addresses that deposited. A 200% tolerance check provides no signal.

/// Return all explorer comparison checks.
pub fn explorer_checks() -> Vec<Box<dyn Check>> {
    vec![
        Box::new(ExplorerTxCount),                  // X1
        Box::new(ExplorerTotalDeposit),             // X2
        Box::new(ExplorerHashRate),                 // X3
        Box::new(ExplorerDifficulty),               // X4
        Box::new(ExplorerKnowledgeSize),            // X5
        Box::new(ExplorerUncleRate),                // X6
        Box::new(ExplorerLiveCellCount),            // X7
        Box::new(ExplorerDeadCellCount),            // X8
        Box::new(ExplorerDailyDeposit),             // X9
        Box::new(ExplorerCirculationRatio),         // X10
        Box::new(ExplorerCirculatingSupply),        // X11
        Box::new(ExplorerBurnt),                    // X12
        Box::new(ExplorerDepositCompensation),      // X13
        Box::new(ExplorerMiningReward),             // X14
        Box::new(ExplorerTreasuryAmount),           // X15
        Box::new(BlockTimeDistributionConsistency), // X16
        Box::new(NervosDaoTotalDeposit),            // X17
        Box::new(NervosDaoDepositorsCount),         // X18
        Box::new(NervosDaoUnclaimedCompensation),   // X19
        Box::new(NervosDaoClaimedCompensation),     // X20
        Box::new(NervosDaoAverageDepositTime),      // X21
        Box::new(NervosDaoMiningReward),            // X22
        Box::new(NervosDaoDepositCompensation),     // X23
        Box::new(NervosDaoTreasuryAmount),          // X24
        Box::new(NervosDaoEstimatedApc),            // X25
        Box::new(ExplorerTotalDepositorsCount),     // X26
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_ckb_to_shannon_with_decimal() {
        // "270263243.54537001" → 270263243 * 100_000_000 + 54537001
        assert_eq!(
            parse_ckb_to_shannon("270263243.54537001"),
            Some(27026324354537001)
        );
    }

    #[test]
    fn test_parse_ckb_to_shannon_trimmed_zeros() {
        // shannon_to_ckb trims trailing zeros: "100.5" means remainder=50000000
        assert_eq!(
            parse_ckb_to_shannon("100.5"),
            Some(100 * 100_000_000 + 50_000_000)
        );
    }

    #[test]
    fn test_parse_ckb_to_shannon_whole_number() {
        assert_eq!(parse_ckb_to_shannon("42"), Some(42 * 100_000_000));
    }

    #[test]
    fn test_parse_ckb_to_shannon_zero() {
        assert_eq!(parse_ckb_to_shannon("0"), Some(0));
    }

    #[test]
    fn test_parse_ckb_to_shannon_full_precision() {
        // 8 decimal places (no trimming)
        assert_eq!(parse_ckb_to_shannon("1.00000001"), Some(100_000_001));
    }

    #[test]
    fn test_parse_ckb_to_shannon_roundtrip() {
        // Simulate what shannon_to_ckb produces for a known value
        let shannons: u128 = 750_402_753_667_822_462;
        let ckb = shannons / 100_000_000;
        let remainder = shannons % 100_000_000;
        let ckb_str = format!("{}.{:08}", ckb, remainder)
            .trim_end_matches('0')
            .to_string();
        assert_eq!(parse_ckb_to_shannon(&ckb_str), Some(shannons as i128));
    }

    #[test]
    fn test_normalize_date_slash_format() {
        assert_eq!(normalize_date("2024/01/15"), "2024-01-15");
    }

    #[test]
    fn test_normalize_date_compact_format() {
        assert_eq!(normalize_date("20240115"), "2024-01-15");
    }

    #[test]
    fn test_normalize_date_already_normalized() {
        assert_eq!(normalize_date("2024-01-15"), "2024-01-15");
    }

    #[test]
    fn test_explorer_timestamp_to_date_uses_same_utc8_day() {
        // 1771171200 = 2026-02-15 16:00:00 UTC = 2026-02-16 00:00:00 UTC+8.
        assert_eq!(
            explorer_timestamp_to_date(1_771_171_200),
            Some("2026-02-16".to_string())
        );
    }

    #[test]
    fn test_remap_v2_cache_dates_shifts_forward_one_day() {
        let mut v2 = HashMap::new();
        v2.insert("2026-02-15".to_string(), "16773".to_string());
        v2.insert("2026-02-16".to_string(), "17030".to_string());

        let fixed = remap_v2_cache_dates(&v2);
        assert_eq!(fixed.get("2026-02-16"), Some(&"16773".to_string()));
        assert_eq!(fixed.get("2026-02-17"), Some(&"17030".to_string()));
    }

    #[test]
    fn test_compute_baseline_offset_i128_uses_first_matching_date() {
        let dates = last_30_days();
        let d1 = dates[0].clone();
        let d2 = dates[1].clone();

        let mut ours = HashMap::new();
        ours.insert(d1.clone(), "110".to_string());
        ours.insert(d2.clone(), "105".to_string());

        let mut explorer = HashMap::new();
        explorer.insert(d1, "100".to_string());
        explorer.insert(d2, "95".to_string());

        let offset = compute_baseline_offset_i128(
            &ours,
            &explorer,
            &dates,
            |v| v.parse::<i128>().ok(),
            |v| v.parse::<i128>().ok(),
        );
        assert_eq!(offset, Some(10));
    }

    #[test]
    fn test_run_exact_i128_with_offset_allows_constant_shift() {
        let dates = last_30_days();
        let d1 = dates[0].clone();
        let d2 = dates[1].clone();

        let mut ours = HashMap::new();
        ours.insert(d1.clone(), "110".to_string());
        ours.insert(d2.clone(), "105".to_string());

        let mut explorer = HashMap::new();
        explorer.insert(d1, "100".to_string());
        explorer.insert(d2, "95".to_string());

        let progress = ProgressReporter::new(None);
        let result = run_exact_i128_explorer_check_with_offset(
            &ours,
            &explorer,
            &progress,
            "test_metric",
            |v| v.parse::<i128>().ok(),
        );

        assert!(result.passed);
        assert_eq!(result.items_checked, 2);
    }

    #[test]
    fn test_run_tolerance_with_offset_allows_constant_shift() {
        let dates = last_30_days();
        let d1 = dates[0].clone();
        let d2 = dates[1].clone();

        let mut ours = HashMap::new();
        ours.insert(d1.clone(), "110".to_string());
        ours.insert(d2.clone(), "120".to_string());

        let mut explorer = HashMap::new();
        explorer.insert(d1, "100".to_string());
        explorer.insert(d2, "110".to_string());

        let progress = ProgressReporter::new(None);
        let result = run_tolerance_explorer_check_with_offset(
            &ours,
            &explorer,
            &progress,
            "test_metric",
            0.0001,
            |our, exp| Some((our.parse::<f64>().ok()?, exp.parse::<f64>().ok()?)),
        );

        assert!(result.passed);
        assert_eq!(result.items_checked, 2);
    }

    #[test]
    fn test_run_tolerance_without_offset_detects_constant_shift() {
        let dates = last_30_days();
        let d1 = dates[0].clone();
        let d2 = dates[1].clone();

        let mut ours = HashMap::new();
        ours.insert(d1.clone(), "110".to_string());
        ours.insert(d2.clone(), "120".to_string());

        let mut explorer = HashMap::new();
        explorer.insert(d1, "100".to_string());
        explorer.insert(d2, "110".to_string());

        let progress = ProgressReporter::new(None);
        let result = run_tolerance_explorer_check(
            &ours,
            &explorer,
            &progress,
            "test_metric",
            0.0001,
            |our, exp| Some((our.parse::<f64>().ok()?, exp.parse::<f64>().ok()?)),
        );

        assert!(!result.passed);
        assert_eq!(result.items_failed, 2);
    }

    #[test]
    fn test_weighted_avg_block_time_ms_from_distribution() {
        let points = vec![
            ChartDataPoint {
                date: "10".to_string(),
                value: "50".to_string(),
                value2: None,
            },
            ChartDataPoint {
                date: "14".to_string(),
                value: "50".to_string(),
                value2: None,
            },
        ];
        assert_eq!(
            weighted_avg_block_time_ms_from_distribution(&points),
            Some(12_000.0)
        );
    }

    #[test]
    fn test_weighted_avg_block_time_ms_from_distribution_rejects_zero_ratio_sum() {
        let points = vec![ChartDataPoint {
            date: "10".to_string(),
            value: "0".to_string(),
            value2: None,
        }];
        assert_eq!(weighted_avg_block_time_ms_from_distribution(&points), None);
    }

    #[test]
    fn test_weighted_avg_block_time_ms_from_distribution_empty_data() {
        let points: Vec<ChartDataPoint> = vec![];
        assert_eq!(weighted_avg_block_time_ms_from_distribution(&points), None);
    }

    #[test]
    fn test_explorer_checks_registered() {
        let checks = explorer_checks();
        assert_eq!(checks.len(), 26);
        let names: Vec<&str> = checks.iter().map(|c| c.name()).collect();
        let unique: std::collections::HashSet<&str> = names.iter().copied().collect();
        assert_eq!(names.len(), unique.len(), "Duplicate check names found");
        assert!(names.contains(&"block_time_distribution_consistency"));
        assert!(names.contains(&"nervos_dao_total_deposit"));
        assert!(names.contains(&"nervos_dao_depositors_count"));
        assert!(names.contains(&"nervos_dao_unclaimed_compensation"));
        assert!(names.contains(&"nervos_dao_claimed_compensation"));
        assert!(names.contains(&"nervos_dao_average_deposit_time"));
        assert!(names.contains(&"nervos_dao_mining_reward"));
        assert!(names.contains(&"nervos_dao_deposit_compensation"));
        assert!(names.contains(&"nervos_dao_treasury_amount"));
        assert!(names.contains(&"nervos_dao_estimated_apc"));
        assert!(names.contains(&"explorer_total_depositors_count"));
    }

    #[test]
    fn test_parse_average_deposit_days_normal() {
        assert_eq!(parse_average_deposit_days("1198 days"), Some(1198.0));
    }

    #[test]
    fn test_parse_average_deposit_days_k_format() {
        assert_eq!(parse_average_deposit_days("1.2K days+"), Some(1200.0));
    }

    #[test]
    fn test_parse_average_deposit_days_fractional() {
        assert_eq!(parse_average_deposit_days("0.3 days"), Some(0.3));
    }

    #[test]
    fn test_parse_average_deposit_days_invalid() {
        assert_eq!(parse_average_deposit_days("invalid"), None);
    }
}
