# API Performance Benchmark Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build `ckbadger-bench`, a Rust binary that benchmarks all ckbadger API endpoints against a live local instance, measuring latency percentiles and detecting regressions.

**Architecture:** Standalone binary crate (`crates/bench/`) with no store dependency. Discovers real parameters by querying the running API. Declarative endpoint registry with custom resolver hooks. Dual output: terminal table + JSON.

**Tech Stack:** Rust, reqwest, tokio, clap, serde_json, chrono

**Spec:** `docs/superpowers/specs/2026-03-28-api-benchmark-design.md`

---

## File Structure

```
crates/bench/
  Cargo.toml                    # Binary crate, workspace member
  src/
    main.rs                     # CLI parsing (clap), orchestration
    discovery.rs                # Phase 1-3: capabilities, probes, param discovery
    registry.rs                 # EndpointEntry, RiskTier, ReadPattern, registry builder
    runner.rs                   # Execution engine: warmup, measure, concurrency
    metrics.rs                  # Sample, ComputedMetrics, percentile math
    report.rs                   # Terminal table + JSON output + regression diff
    endpoints/
      mod.rs                    # register_all() aggregating all modules
      activities.rs             # 3 endpoints
      assets.rs                 # 8 endpoints
      blocks.rs                 # 4 endpoints
      cells.rs                  # 8 endpoints (incl addresses)
      dao.rs                    # 9 endpoints
      fiber.rs                  # 4 endpoints
      forks.rs                  # 3 endpoints
      graph.rs                  # 3 endpoints
      hardforks.rs              # 1 endpoint
      identities.rs             # 8 endpoints
      mempool.rs                # 4 endpoints
      scripts.rs                # 8 endpoints
      search.rs                 # 1 endpoint
      spore.rs                  # 13 endpoints
      statistics.rs             # 27 endpoints
      tokens.rs                 # 5 endpoints
      transactions.rs           # 5 endpoints
```

---

### Task 1: Crate Scaffold and CLI

**Files:**
- Create: `crates/bench/Cargo.toml`
- Create: `crates/bench/src/main.rs`
- Modify: `Cargo.toml` (workspace members)

- [ ] **Step 1: Create Cargo.toml**

```toml
[package]
name = "ckbadger-bench"
version.workspace = true
edition.workspace = true
license.workspace = true

[[bin]]
name = "ckbadger-bench"
path = "src/main.rs"

[dependencies]
reqwest = { workspace = true }
tokio = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
clap = { workspace = true }
chrono = { workspace = true }
anyhow = { workspace = true }
```

- [ ] **Step 2: Add crate to workspace**

In `Cargo.toml` root, add `"crates/bench"` to the `members` array:

```toml
members = [
    "crates/common",
    "crates/config",
    "crates/ckb-store-reader",
    "crates/ckbadger-store",
    "crates/indexer",
    "crates/api",
    "crates/tui",
    "crates/ipc",
    "crates/cli",
    "crates/dob-decoder",
    "crates/bench",
]
```

- [ ] **Step 3: Create main.rs with CLI**

```rust
use anyhow::Result;
use clap::Parser;

#[derive(Parser)]
#[command(name = "ckbadger-bench", about = "API performance benchmark")]
struct Cli {
    /// API base URL
    #[arg(long, default_value = "http://localhost:8101/api/v1")]
    api_url: String,

    /// Frontend URL (for /capabilities)
    #[arg(long, default_value = "http://localhost:8100")]
    frontend_url: String,

    /// Requests per endpoint
    #[arg(long, default_value = "10")]
    iterations: u32,

    /// Concurrent requests
    #[arg(long, default_value = "1")]
    concurrency: u32,

    /// Warmup requests (not measured)
    #[arg(long, default_value = "2")]
    warmup: u32,

    /// Per-request timeout in milliseconds
    #[arg(long, default_value = "10000")]
    timeout_ms: u64,

    /// Filter by module name
    #[arg(long)]
    module: Option<String>,

    /// Filter by endpoint path template
    #[arg(long)]
    endpoint: Option<String>,

    /// Filter by risk tier (high, medium, low)
    #[arg(long)]
    risk: Option<String>,

    /// Output JSON instead of table
    #[arg(long)]
    json: bool,

    /// Save JSON output to file
    #[arg(long)]
    output: Option<String>,

    /// Compare against baseline JSON file
    #[arg(long)]
    compare: Option<String>,

    /// Print discovered params and exit
    #[arg(long)]
    discovery_only: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    println!("ckbadger-bench: api_url={}", cli.api_url);
    Ok(())
}
```

- [ ] **Step 4: Verify it compiles**

Run: `cargo check -p ckbadger-bench`
Expected: compiles with no errors

- [ ] **Step 5: Commit**

```bash
git add crates/bench/ Cargo.toml
git commit -m "feat(bench): scaffold ckbadger-bench crate with CLI"
```

---

### Task 2: Core Types — Metrics and Registry

**Files:**
- Create: `crates/bench/src/metrics.rs`
- Create: `crates/bench/src/registry.rs`

- [ ] **Step 1: Create metrics.rs**

```rust
use std::time::Duration;

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Sample {
    pub latency_ms: f64,
    pub status: u16,
    pub body_size: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ComputedMetrics {
    pub p50_ms: f64,
    pub p95_ms: f64,
    pub p99_ms: f64,
    pub min_ms: f64,
    pub max_ms: f64,
    pub mean_ms: f64,
    pub std_dev_ms: f64,
    pub error_rate: f64,
    pub avg_body_size: usize,
    pub throughput_rps: f64,
}

impl ComputedMetrics {
    pub fn from_samples(samples: &[Sample], wall_clock: Duration) -> Self {
        if samples.is_empty() {
            return Self {
                p50_ms: 0.0,
                p95_ms: 0.0,
                p99_ms: 0.0,
                min_ms: 0.0,
                max_ms: 0.0,
                mean_ms: 0.0,
                std_dev_ms: 0.0,
                error_rate: 0.0,
                avg_body_size: 0,
                throughput_rps: 0.0,
            };
        }

        let mut latencies: Vec<f64> = samples.iter().map(|s| s.latency_ms).collect();
        latencies.sort_by(|a, b| a.partial_cmp(b).unwrap());

        let n = latencies.len();
        let sum: f64 = latencies.iter().sum();
        let mean = sum / n as f64;

        let variance = latencies.iter().map(|l| (l - mean).powi(2)).sum::<f64>() / n as f64;
        let std_dev = variance.sqrt();

        let errors = samples.iter().filter(|s| s.error.is_some()).count();
        let total_body: usize = samples.iter().map(|s| s.body_size).sum();

        let wall_secs = wall_clock.as_secs_f64();
        let throughput = if wall_secs > 0.0 {
            n as f64 / wall_secs
        } else {
            0.0
        };

        Self {
            p50_ms: percentile(&latencies, 50.0),
            p95_ms: percentile(&latencies, 95.0),
            p99_ms: percentile(&latencies, 99.0),
            min_ms: latencies[0],
            max_ms: latencies[n - 1],
            mean_ms: mean,
            std_dev_ms: std_dev,
            error_rate: errors as f64 / n as f64,
            avg_body_size: total_body / n,
            throughput_rps: throughput,
        }
    }
}

fn percentile(sorted: &[f64], pct: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let rank = (pct / 100.0) * (sorted.len() - 1) as f64;
    let lower = rank.floor() as usize;
    let upper = rank.ceil() as usize;
    if lower == upper {
        sorted[lower]
    } else {
        let frac = rank - lower as f64;
        sorted[lower] * (1.0 - frac) + sorted[upper] * frac
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_percentile_basic() {
        let data = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        assert!((percentile(&data, 50.0) - 3.0).abs() < 0.01);
        assert!((percentile(&data, 0.0) - 1.0).abs() < 0.01);
        assert!((percentile(&data, 100.0) - 5.0).abs() < 0.01);
    }

    #[test]
    fn test_percentile_single() {
        let data = vec![42.0];
        assert!((percentile(&data, 50.0) - 42.0).abs() < 0.01);
        assert!((percentile(&data, 95.0) - 42.0).abs() < 0.01);
    }

    #[test]
    fn test_computed_metrics_from_samples() {
        let samples = vec![
            Sample { latency_ms: 10.0, status: 200, body_size: 100, error: None },
            Sample { latency_ms: 20.0, status: 200, body_size: 200, error: None },
            Sample { latency_ms: 30.0, status: 200, body_size: 300, error: None },
        ];
        let metrics = ComputedMetrics::from_samples(&samples, Duration::from_secs(1));
        assert!((metrics.mean_ms - 20.0).abs() < 0.01);
        assert!((metrics.min_ms - 10.0).abs() < 0.01);
        assert!((metrics.max_ms - 30.0).abs() < 0.01);
        assert!((metrics.error_rate - 0.0).abs() < 0.01);
        assert_eq!(metrics.avg_body_size, 200);
    }

    #[test]
    fn test_computed_metrics_with_errors() {
        let samples = vec![
            Sample { latency_ms: 10.0, status: 200, body_size: 100, error: None },
            Sample { latency_ms: 50.0, status: 500, body_size: 0, error: Some("timeout".into()) },
        ];
        let metrics = ComputedMetrics::from_samples(&samples, Duration::from_secs(1));
        assert!((metrics.error_rate - 0.5).abs() < 0.01);
    }

    #[test]
    fn test_computed_metrics_empty() {
        let metrics = ComputedMetrics::from_samples(&[], Duration::from_secs(1));
        assert!((metrics.mean_ms - 0.0).abs() < 0.01);
        assert!((metrics.throughput_rps - 0.0).abs() < 0.01);
    }
}
```

- [ ] **Step 2: Create registry.rs**

```rust
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RiskTier {
    High,
    Medium,
    Low,
}

impl RiskTier {
    pub fn from_str_opt(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "high" => Some(Self::High),
            "medium" => Some(Self::Medium),
            "low" => Some(Self::Low),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize)]
pub enum ReadPattern {
    KeyLookup,
    BatchLookup,
    PrefixScan,
    RangeScan,
    FullCfScan,
    CrossStore,
    RpcDependent,
    Cached,
    Aggregation,
}

impl std::fmt::Display for ReadPattern {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::KeyLookup => write!(f, "KeyLookup"),
            Self::BatchLookup => write!(f, "BatchGet"),
            Self::PrefixScan => write!(f, "PrefixScan"),
            Self::RangeScan => write!(f, "RangeScan"),
            Self::FullCfScan => write!(f, "FullScan"),
            Self::CrossStore => write!(f, "CrossStore"),
            Self::RpcDependent => write!(f, "RpcDep"),
            Self::Cached => write!(f, "Cached"),
            Self::Aggregation => write!(f, "Aggregation"),
        }
    }
}

/// A resolved HTTP request ready to execute.
#[derive(Debug, Clone)]
pub struct ResolvedRequest {
    pub url: String,
    pub method: Method,
    pub body: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Method {
    Get,
    Post,
}

impl std::fmt::Display for Method {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Get => write!(f, "GET"),
            Self::Post => write!(f, "POST"),
        }
    }
}

pub fn get(url: &str) -> ResolvedRequest {
    ResolvedRequest {
        url: url.to_string(),
        method: Method::Get,
        body: None,
    }
}

pub fn post(url: &str, body: &str) -> ResolvedRequest {
    ResolvedRequest {
        url: url.to_string(),
        method: Method::Post,
        body: Some(body.to_string()),
    }
}

/// One endpoint to benchmark.
pub struct EndpointEntry {
    pub module: &'static str,
    pub method: Method,
    pub path_template: &'static str,
    pub description: &'static str,
    pub resolve: Box<dyn Fn(&str, &DiscoveredParams) -> Option<ResolvedRequest> + Send + Sync>,
    pub expect_status: u16,
    pub risk_tier: RiskTier,
    pub read_pattern: ReadPattern,
}

/// Placeholder — filled in by discovery.rs (Task 3).
#[derive(Debug, Clone, Default)]
pub struct DiscoveredParams {
    pub sync_tip: u64,
    pub latest_block_number: u64,
    pub latest_block_hash: String,
    pub mid_block_number: u64,
    pub tx_hashes: Vec<String>,
    pub complex_tx_hash: Option<String>,
    pub top_addresses: Vec<String>,
    pub top_lock_hashes: Vec<String>,
    pub dao_lock_hashes: Vec<String>,
    pub dao_deposit_outpoint: Option<(String, u32)>,
    pub token_type_hashes: Vec<String>,
    pub cluster_ids: Vec<String>,
    pub spore_ids: Vec<String>,
    pub script_names: Vec<String>,
    pub live_cell_outpoint: Option<(String, u32)>,
    pub fiber_channel_id: Option<String>,
    pub dotbit_item_id: Option<String>,
    pub object_collection_id: Option<String>,
    pub object_item_id: Option<String>,
    pub identity_collection_id: Option<String>,
    pub fork_id: Option<String>,
}

/// Registry of all endpoints.
pub struct Registry {
    pub entries: Vec<EndpointEntry>,
}

impl Registry {
    pub fn new() -> Self {
        Self { entries: Vec::new() }
    }

    pub fn add(&mut self, entry: EndpointEntry) {
        self.entries.push(entry);
    }

    pub fn filter_module(&mut self, module: &str) {
        self.entries.retain(|e| e.module == module);
    }

    pub fn filter_endpoint(&mut self, template: &str) {
        self.entries.retain(|e| e.path_template == template);
    }

    pub fn filter_risk(&mut self, tier: RiskTier) {
        self.entries.retain(|e| e.risk_tier == tier);
    }

    pub fn sort_by_risk(&mut self) {
        self.entries.sort_by_key(|e| match e.risk_tier {
            RiskTier::Low => 0,
            RiskTier::Medium => 1,
            RiskTier::High => 2,
        });
    }
}
```

- [ ] **Step 3: Verify it compiles**

Add `mod metrics; mod registry;` to `main.rs` and run:

```rust
// main.rs — add at top
mod metrics;
mod registry;
```

Run: `cargo check -p ckbadger-bench`
Expected: compiles with no errors

- [ ] **Step 4: Run tests**

Run: `cargo test -p ckbadger-bench`
Expected: all 5 tests pass (4 percentile/metrics tests + 1 empty)

- [ ] **Step 5: Commit**

```bash
git add crates/bench/src/metrics.rs crates/bench/src/registry.rs crates/bench/src/main.rs
git commit -m "feat(bench): add core types — metrics, registry, endpoint entry"
```

---

### Task 3: Discovery System

**Files:**
- Create: `crates/bench/src/discovery.rs`

- [ ] **Step 1: Create discovery.rs**

```rust
use anyhow::{Context, Result};
use serde::Deserialize;

use crate::registry::DiscoveredParams;

/// Data availability flags per module.
#[derive(Debug, Clone, Default)]
pub struct DataAvailability {
    pub has_tokens: bool,
    pub has_spore: bool,
    pub has_dao: bool,
    pub has_fiber: bool,
    pub has_identities: bool,
    pub has_assets: bool,
    pub has_mempool: bool,
    pub has_graph: bool,
    pub has_forks: bool,
}

/// Full discovery result.
#[derive(Debug, Clone)]
pub struct Discovery {
    pub capabilities_route_count: usize,
    pub availability: DataAvailability,
    pub params: DiscoveredParams,
}

/// Run all three discovery phases.
pub async fn run_discovery(
    api_base: &str,
    frontend_url: &str,
    client: &reqwest::Client,
) -> Result<Discovery> {
    // Phase 1: Capabilities
    let capabilities_route_count = fetch_capabilities(frontend_url, client).await;

    // Phase 2: Data availability probes
    let availability = probe_availability(api_base, client).await;

    // Phase 3: Parameter discovery
    let params = discover_params(api_base, client, &availability).await?;

    Ok(Discovery {
        capabilities_route_count,
        availability,
        params,
    })
}

async fn fetch_capabilities(frontend_url: &str, client: &reqwest::Client) -> usize {
    let url = format!("{}/capabilities", frontend_url);
    match client.get(&url).send().await {
        Ok(resp) if resp.status().is_success() => {
            if let Ok(body) = resp.json::<serde_json::Value>().await {
                let md_count = body
                    .get("routes")
                    .and_then(|r| r.get("markdown"))
                    .and_then(|a| a.as_array())
                    .map(|a| a.len())
                    .unwrap_or(0);
                let raw_count = body
                    .get("routes")
                    .and_then(|r| r.get("raw"))
                    .and_then(|a| a.as_array())
                    .map(|a| a.len())
                    .unwrap_or(0);
                md_count + raw_count
            } else {
                0
            }
        }
        _ => {
            eprintln!("  [warn] /capabilities not reachable at {url}, skipping coverage validation");
            0
        }
    }
}

async fn probe_availability(api_base: &str, client: &reqwest::Client) -> DataAvailability {
    let mut avail = DataAvailability::default();

    avail.has_tokens = probe_non_empty(client, &format!("{api_base}/tokens?limit=1")).await;
    avail.has_spore = probe_non_empty(client, &format!("{api_base}/spore/clusters?limit=1")).await;
    avail.has_dao = probe_non_empty(client, &format!("{api_base}/dao/deposits?limit=1")).await;
    avail.has_fiber = probe_non_empty(client, &format!("{api_base}/fiber/channels?limit=1")).await;
    avail.has_identities =
        probe_non_empty(client, &format!("{api_base}/assets/identities/dotbit/items?limit=1"))
            .await;
    avail.has_assets = probe_non_empty(client, &format!("{api_base}/assets?limit=1")).await;
    avail.has_mempool = probe_ok(client, &format!("{api_base}/mempool/info")).await;
    avail.has_forks = probe_non_empty(client, &format!("{api_base}/forks")).await;

    avail
}

async fn probe_non_empty(client: &reqwest::Client, url: &str) -> bool {
    match client.get(url).send().await {
        Ok(resp) if resp.status().is_success() => {
            if let Ok(body) = resp.json::<serde_json::Value>().await {
                // Check common patterns: {data: [...]}, [...], {items: [...]}
                if let Some(arr) = body.get("data").and_then(|d| d.as_array()) {
                    return !arr.is_empty();
                }
                if let Some(arr) = body.as_array() {
                    return !arr.is_empty();
                }
                // Non-empty object counts as having data
                body.is_object()
            } else {
                false
            }
        }
        _ => false,
    }
}

async fn probe_ok(client: &reqwest::Client, url: &str) -> bool {
    matches!(client.get(url).send().await, Ok(resp) if resp.status().is_success())
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct NetworkStats {
    latest_block: i64,
}

#[derive(Debug, Deserialize)]
struct PaginatedResponse<T> {
    data: Vec<T>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BlockItem {
    block_number: i64,
    block_hash: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TxItem {
    tx_hash: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AddressItem {
    address: Option<String>,
    lock_hash: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DaoDepositorItem {
    lock_hash: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DaoDepositItem {
    tx_hash: Option<String>,
    output_index: Option<u32>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TokenItem {
    type_hash: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClusterItem {
    cluster_id: Option<String>,
    id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SporeItem {
    spore_id: Option<String>,
    id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ScriptItem {
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CellItem {
    tx_hash: Option<String>,
    output_index: Option<u32>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FiberChannelItem {
    channel_id: Option<String>,
    id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct IdentityItem {
    identity_id: Option<String>,
    id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AssetItem {
    collection_id: Option<String>,
    id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AssetObjectItem {
    object_id: Option<String>,
    id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ForkItem {
    id: Option<String>,
    fork_id: Option<String>,
}

async fn discover_params(
    api_base: &str,
    client: &reqwest::Client,
    avail: &DataAvailability,
) -> Result<DiscoveredParams> {
    let mut params = DiscoveredParams::default();

    // Network stats → sync tip
    if let Ok(resp) = client
        .get(format!("{api_base}/statistics/network"))
        .send()
        .await
    {
        if let Ok(stats) = resp.json::<NetworkStats>().await {
            params.sync_tip = stats.latest_block as u64;
        }
    }

    // Blocks
    if let Ok(resp) = client
        .get(format!("{api_base}/blocks?limit=1"))
        .send()
        .await
    {
        if let Ok(body) = resp.json::<serde_json::Value>().await {
            if let Some(data) = body.get("data").and_then(|d| d.as_array()) {
                if let Some(first) = data.first() {
                    if let Ok(block) = serde_json::from_value::<BlockItem>(first.clone()) {
                        params.latest_block_number = block.block_number as u64;
                        params.latest_block_hash = block.block_hash;
                    }
                }
            }
        }
    }
    params.mid_block_number = params.latest_block_number / 2;

    // Transactions
    if let Ok(resp) = client
        .get(format!("{api_base}/transactions?limit=5"))
        .send()
        .await
    {
        if let Ok(body) = resp.json::<serde_json::Value>().await {
            if let Some(data) = body.get("data").and_then(|d| d.as_array()) {
                params.tx_hashes = data
                    .iter()
                    .filter_map(|v| {
                        serde_json::from_value::<TxItem>(v.clone())
                            .ok()
                            .map(|t| t.tx_hash)
                    })
                    .collect();
            }
        }
    }

    // Top addresses
    if let Ok(resp) = client
        .get(format!("{api_base}/addresses/top?limit=5"))
        .send()
        .await
    {
        if let Ok(body) = resp.json::<serde_json::Value>().await {
            if let Some(data) = body.get("data").and_then(|d| d.as_array()) {
                for item in data {
                    if let Ok(addr) = serde_json::from_value::<AddressItem>(item.clone()) {
                        if let Some(a) = addr.address {
                            params.top_addresses.push(a);
                        }
                        if let Some(lh) = addr.lock_hash {
                            params.top_lock_hashes.push(lh);
                        }
                    }
                }
            }
        }
    }

    // DAO
    if avail.has_dao {
        if let Ok(resp) = client
            .get(format!("{api_base}/dao/top-depositors?limit=3"))
            .send()
            .await
        {
            if let Ok(body) = resp.json::<serde_json::Value>().await {
                if let Some(data) = body.get("data").and_then(|d| d.as_array()) {
                    params.dao_lock_hashes = data
                        .iter()
                        .filter_map(|v| {
                            serde_json::from_value::<DaoDepositorItem>(v.clone())
                                .ok()
                                .and_then(|d| d.lock_hash)
                        })
                        .collect();
                }
            }
        }

        if let Ok(resp) = client
            .get(format!("{api_base}/dao/deposits?limit=1"))
            .send()
            .await
        {
            if let Ok(body) = resp.json::<serde_json::Value>().await {
                if let Some(data) = body.get("data").and_then(|d| d.as_array()) {
                    if let Some(first) = data.first() {
                        if let Ok(dep) = serde_json::from_value::<DaoDepositItem>(first.clone()) {
                            if let (Some(h), Some(i)) = (dep.tx_hash, dep.output_index) {
                                params.dao_deposit_outpoint = Some((h, i));
                            }
                        }
                    }
                }
            }
        }
    }

    // Tokens
    if avail.has_tokens {
        if let Ok(resp) = client
            .get(format!("{api_base}/tokens?limit=3"))
            .send()
            .await
        {
            if let Ok(body) = resp.json::<serde_json::Value>().await {
                if let Some(data) = body.get("data").and_then(|d| d.as_array()) {
                    params.token_type_hashes = data
                        .iter()
                        .filter_map(|v| {
                            serde_json::from_value::<TokenItem>(v.clone())
                                .ok()
                                .and_then(|t| t.type_hash)
                        })
                        .collect();
                }
            }
        }
    }

    // Spore
    if avail.has_spore {
        if let Ok(resp) = client
            .get(format!("{api_base}/spore/clusters?limit=3"))
            .send()
            .await
        {
            if let Ok(body) = resp.json::<serde_json::Value>().await {
                if let Some(data) = body.get("data").and_then(|d| d.as_array()) {
                    params.cluster_ids = data
                        .iter()
                        .filter_map(|v| {
                            serde_json::from_value::<ClusterItem>(v.clone())
                                .ok()
                                .and_then(|c| c.cluster_id.or(c.id))
                        })
                        .collect();
                }
            }
        }

        if let Ok(resp) = client
            .get(format!("{api_base}/spore/objects?limit=3"))
            .send()
            .await
        {
            if let Ok(body) = resp.json::<serde_json::Value>().await {
                if let Some(data) = body.get("data").and_then(|d| d.as_array()) {
                    params.spore_ids = data
                        .iter()
                        .filter_map(|v| {
                            serde_json::from_value::<SporeItem>(v.clone())
                                .ok()
                                .and_then(|s| s.spore_id.or(s.id))
                        })
                        .collect();
                }
            }
        }
    }

    // Scripts
    if let Ok(resp) = client
        .get(format!("{api_base}/scripts?limit=3"))
        .send()
        .await
    {
        if let Ok(body) = resp.json::<serde_json::Value>().await {
            if let Some(data) = body.get("data").and_then(|d| d.as_array()) {
                params.script_names = data
                    .iter()
                    .filter_map(|v| {
                        serde_json::from_value::<ScriptItem>(v.clone())
                            .ok()
                            .and_then(|s| s.name)
                    })
                    .collect();
            }
        }
    }

    // Live cell
    if let Ok(resp) = client
        .get(format!("{api_base}/cells/live?limit=1"))
        .send()
        .await
    {
        if let Ok(body) = resp.json::<serde_json::Value>().await {
            if let Some(data) = body.get("data").and_then(|d| d.as_array()) {
                if let Some(first) = data.first() {
                    if let Ok(cell) = serde_json::from_value::<CellItem>(first.clone()) {
                        if let (Some(h), Some(i)) = (cell.tx_hash, cell.output_index) {
                            params.live_cell_outpoint = Some((h, i));
                        }
                    }
                }
            }
        }
    }

    // Fiber
    if avail.has_fiber {
        if let Ok(resp) = client
            .get(format!("{api_base}/fiber/channels?limit=1"))
            .send()
            .await
        {
            if let Ok(body) = resp.json::<serde_json::Value>().await {
                if let Some(data) = body.get("data").and_then(|d| d.as_array()) {
                    if let Some(first) = data.first() {
                        if let Ok(ch) =
                            serde_json::from_value::<FiberChannelItem>(first.clone())
                        {
                            params.fiber_channel_id = ch.channel_id.or(ch.id);
                        }
                    }
                }
            }
        }
    }

    // Identities
    if avail.has_identities {
        if let Ok(resp) = client
            .get(format!(
                "{api_base}/assets/identities/dotbit/items?limit=1"
            ))
            .send()
            .await
        {
            if let Ok(body) = resp.json::<serde_json::Value>().await {
                if let Some(data) = body.get("data").and_then(|d| d.as_array()) {
                    if let Some(first) = data.first() {
                        if let Ok(id) = serde_json::from_value::<IdentityItem>(first.clone()) {
                            params.dotbit_item_id = id.identity_id.or(id.id);
                        }
                    }
                }
            }
        }
        params.identity_collection_id = Some("dotbit".to_string());
    }

    // Assets (objects/MNFT)
    if avail.has_assets {
        if let Ok(resp) = client
            .get(format!("{api_base}/assets?limit=1"))
            .send()
            .await
        {
            if let Ok(body) = resp.json::<serde_json::Value>().await {
                if let Some(data) = body.get("data").and_then(|d| d.as_array()) {
                    if let Some(first) = data.first() {
                        if let Ok(asset) = serde_json::from_value::<AssetItem>(first.clone()) {
                            params.object_collection_id = asset.collection_id.or(asset.id);
                        }
                    }
                }
            }
        }

        // Discover an object item if we have a collection
        if let Some(ref col_id) = params.object_collection_id {
            if let Ok(resp) = client
                .get(format!(
                    "{api_base}/assets/objects/{col_id}/items?limit=1"
                ))
                .send()
                .await
            {
                if let Ok(body) = resp.json::<serde_json::Value>().await {
                    if let Some(data) = body.get("data").and_then(|d| d.as_array()) {
                        if let Some(first) = data.first() {
                            if let Ok(obj) =
                                serde_json::from_value::<AssetObjectItem>(first.clone())
                            {
                                params.object_item_id = obj.object_id.or(obj.id);
                            }
                        }
                    }
                }
            }
        }
    }

    // Forks
    if avail.has_forks {
        if let Ok(resp) = client
            .get(format!("{api_base}/forks"))
            .send()
            .await
        {
            if let Ok(body) = resp.json::<serde_json::Value>().await {
                if let Some(data) = body.get("data").and_then(|d| d.as_array()) {
                    if let Some(first) = data.first() {
                        if let Ok(fork) = serde_json::from_value::<ForkItem>(first.clone()) {
                            params.fork_id = fork.id.or(fork.fork_id);
                        }
                    }
                }
            }
        }
    }

    // Graph availability — depends on having a tx hash and ckb_store
    // We'll try the graph endpoint; if it fails, has_graph stays false
    // (This is done in probe_availability if we had a tx hash, but we discover
    //  tx_hashes after probes, so we do a late probe here)

    Ok(params)
}

pub fn print_discovery(discovery: &Discovery) {
    println!("\n=== Discovery Results ===");
    println!("Capabilities routes: {}", discovery.capabilities_route_count);
    println!("\nData Availability:");
    let a = &discovery.availability;
    println!("  tokens:     {}", a.has_tokens);
    println!("  spore:      {}", a.has_spore);
    println!("  dao:        {}", a.has_dao);
    println!("  fiber:      {}", a.has_fiber);
    println!("  identities: {}", a.has_identities);
    println!("  assets:     {}", a.has_assets);
    println!("  mempool:    {}", a.has_mempool);
    println!("  forks:      {}", a.has_forks);

    let p = &discovery.params;
    println!("\nDiscovered Parameters:");
    println!("  sync_tip:             {}", p.sync_tip);
    println!("  latest_block_number:  {}", p.latest_block_number);
    println!("  latest_block_hash:    {}", p.latest_block_hash);
    println!("  mid_block_number:     {}", p.mid_block_number);
    println!("  tx_hashes:            {} found", p.tx_hashes.len());
    println!("  top_addresses:        {} found", p.top_addresses.len());
    println!("  dao_lock_hashes:      {} found", p.dao_lock_hashes.len());
    println!("  token_type_hashes:    {} found", p.token_type_hashes.len());
    println!("  cluster_ids:          {} found", p.cluster_ids.len());
    println!("  spore_ids:            {} found", p.spore_ids.len());
    println!("  script_names:         {} found", p.script_names.len());
    println!(
        "  live_cell_outpoint:   {}",
        if p.live_cell_outpoint.is_some() {
            "yes"
        } else {
            "no"
        }
    );
    println!(
        "  fiber_channel_id:     {}",
        p.fiber_channel_id.as_deref().unwrap_or("none")
    );
    println!(
        "  dotbit_item_id:       {}",
        p.dotbit_item_id.as_deref().unwrap_or("none")
    );
    println!(
        "  object_collection_id: {}",
        p.object_collection_id.as_deref().unwrap_or("none")
    );
    println!(
        "  object_item_id:       {}",
        p.object_item_id.as_deref().unwrap_or("none")
    );
}
```

- [ ] **Step 2: Wire into main.rs**

Add `mod discovery;` to main.rs and update the main function:

```rust
mod discovery;
mod metrics;
mod registry;

use anyhow::{bail, Result};
use clap::Parser;
use std::time::Duration;

// ... Cli struct stays the same ...

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    let client = reqwest::Client::builder()
        .timeout(Duration::from_millis(cli.timeout_ms))
        .build()?;

    // Connectivity check
    match client.get(format!("{}/statistics/network", &cli.api_url)).send().await {
        Ok(resp) if resp.status().is_success() => {}
        Ok(resp) => bail!(
            "API returned {} — is ckbadger API running at {}?",
            resp.status(),
            cli.api_url
        ),
        Err(e) => bail!(
            "Cannot connect to {} — is ckbadger API running? Error: {}",
            cli.api_url,
            e
        ),
    }

    println!("ckbadger-bench: discovering parameters...");
    let disc = discovery::run_discovery(&cli.api_url, &cli.frontend_url, &client).await?;

    if cli.discovery_only {
        discovery::print_discovery(&disc);
        return Ok(());
    }

    println!("Discovery complete. {} params ready.", disc.params.tx_hashes.len());
    Ok(())
}
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo check -p ckbadger-bench`
Expected: compiles with no errors

- [ ] **Step 4: Commit**

```bash
git add crates/bench/src/discovery.rs crates/bench/src/main.rs
git commit -m "feat(bench): add discovery system — capabilities, probes, param resolution"
```

---

### Task 4: Execution Engine (Runner)

**Files:**
- Create: `crates/bench/src/runner.rs`

- [ ] **Step 1: Create runner.rs**

```rust
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use tokio::sync::Semaphore;

use crate::metrics::{ComputedMetrics, Sample};
use crate::registry::{DiscoveredParams, EndpointEntry, Method, ResolvedRequest};

/// Result of benchmarking one endpoint.
pub struct EndpointResult {
    pub module: String,
    pub method: String,
    pub path_template: String,
    pub description: String,
    pub resolved_url: String,
    pub read_pattern: String,
    pub risk_tier: String,
    pub samples: Vec<Sample>,
    pub metrics: ComputedMetrics,
    pub wall_clock: Duration,
    pub skipped: bool,
    pub skip_reason: Option<String>,
}

pub struct RunConfig {
    pub iterations: u32,
    pub concurrency: u32,
    pub warmup: u32,
}

/// Run benchmark for a single endpoint.
pub async fn bench_endpoint(
    client: &reqwest::Client,
    entry: &EndpointEntry,
    api_base: &str,
    params: &DiscoveredParams,
    config: &RunConfig,
) -> EndpointResult {
    let resolved = (entry.resolve)(api_base, params);

    let Some(req) = resolved else {
        return EndpointResult {
            module: entry.module.to_string(),
            method: entry.method.to_string(),
            path_template: entry.path_template.to_string(),
            description: entry.description.to_string(),
            resolved_url: String::new(),
            read_pattern: entry.read_pattern.to_string(),
            risk_tier: format!("{:?}", entry.risk_tier),
            samples: vec![],
            metrics: ComputedMetrics::from_samples(&[], Duration::ZERO),
            wall_clock: Duration::ZERO,
            skipped: true,
            skip_reason: Some("no data available for parameter resolution".to_string()),
        };
    };

    // Warmup
    for _ in 0..config.warmup {
        let _ = execute_request(client, &req).await;
    }

    // Measured requests
    let wall_start = Instant::now();
    let samples = if config.concurrency <= 1 {
        run_sequential(client, &req, config.iterations, entry.expect_status).await
    } else {
        run_concurrent(
            client,
            &req,
            config.iterations,
            config.concurrency,
            entry.expect_status,
        )
        .await
    };
    let wall_clock = wall_start.elapsed();

    let metrics = ComputedMetrics::from_samples(&samples, wall_clock);

    EndpointResult {
        module: entry.module.to_string(),
        method: entry.method.to_string(),
        path_template: entry.path_template.to_string(),
        description: entry.description.to_string(),
        resolved_url: req.url.clone(),
        read_pattern: entry.read_pattern.to_string(),
        risk_tier: format!("{:?}", entry.risk_tier),
        samples,
        metrics,
        wall_clock,
        skipped: false,
        skip_reason: None,
    }
}

async fn run_sequential(
    client: &reqwest::Client,
    req: &ResolvedRequest,
    iterations: u32,
    expect_status: u16,
) -> Vec<Sample> {
    let mut samples = Vec::with_capacity(iterations as usize);
    for _ in 0..iterations {
        samples.push(execute_one(client, req, expect_status).await);
    }
    samples
}

async fn run_concurrent(
    client: &reqwest::Client,
    req: &ResolvedRequest,
    iterations: u32,
    concurrency: u32,
    expect_status: u16,
) -> Vec<Sample> {
    let semaphore = Arc::new(Semaphore::new(concurrency as usize));
    let mut handles = Vec::with_capacity(iterations as usize);

    for _ in 0..iterations {
        let permit = semaphore.clone().acquire_owned().await.unwrap();
        let client = client.clone();
        let req = req.clone();
        handles.push(tokio::spawn(async move {
            let sample = execute_one(&client, &req, expect_status).await;
            drop(permit);
            sample
        }));
    }

    let mut samples = Vec::with_capacity(iterations as usize);
    for handle in handles {
        if let Ok(sample) = handle.await {
            samples.push(sample);
        }
    }
    samples
}

async fn execute_one(
    client: &reqwest::Client,
    req: &ResolvedRequest,
    expect_status: u16,
) -> Sample {
    let start = Instant::now();
    match execute_request(client, req).await {
        Ok((status, body_size)) => {
            let latency = start.elapsed();
            let error = if status != expect_status {
                Some(format!("expected {expect_status}, got {status}"))
            } else {
                None
            };
            Sample {
                latency_ms: latency.as_secs_f64() * 1000.0,
                status,
                body_size,
                error,
            }
        }
        Err(e) => {
            let latency = start.elapsed();
            Sample {
                latency_ms: latency.as_secs_f64() * 1000.0,
                status: 0,
                body_size: 0,
                error: Some(e.to_string()),
            }
        }
    }
}

async fn execute_request(
    client: &reqwest::Client,
    req: &ResolvedRequest,
) -> Result<(u16, usize)> {
    let resp = match req.method {
        Method::Get => client.get(&req.url).send().await?,
        Method::Post => {
            let mut builder = client.post(&req.url);
            if let Some(ref body) = req.body {
                builder = builder
                    .header("content-type", "application/json")
                    .body(body.clone());
            }
            builder.send().await?
        }
    };
    let status = resp.status().as_u16();
    let bytes = resp.bytes().await?;
    Ok((status, bytes.len()))
}
```

- [ ] **Step 2: Verify it compiles**

Add `mod runner;` to main.rs.

Run: `cargo check -p ckbadger-bench`
Expected: compiles with no errors

- [ ] **Step 3: Commit**

```bash
git add crates/bench/src/runner.rs crates/bench/src/main.rs
git commit -m "feat(bench): add execution engine — sequential and concurrent runners"
```

---

### Task 5: Reporting (Terminal Table + JSON)

**Files:**
- Create: `crates/bench/src/report.rs`

- [ ] **Step 1: Create report.rs**

```rust
use std::io::Write;

use anyhow::Result;
use chrono::Utc;
use serde::Serialize;

use crate::runner::EndpointResult;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BenchmarkReport {
    pub timestamp: String,
    pub config: ReportConfig,
    pub summary: ReportSummary,
    pub results: Vec<ReportEntry>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportConfig {
    pub api_base: String,
    pub iterations: u32,
    pub concurrency: u32,
    pub warmup: u32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportSummary {
    pub tested: usize,
    pub skipped: usize,
    pub slow_count: usize,
    pub very_slow_count: usize,
    pub error_count: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportEntry {
    pub module: String,
    pub method: String,
    pub path_template: String,
    pub resolved_url: String,
    pub read_pattern: String,
    pub risk_tier: String,
    pub p50_ms: f64,
    pub p95_ms: f64,
    pub p99_ms: f64,
    pub min_ms: f64,
    pub max_ms: f64,
    pub mean_ms: f64,
    pub error_rate: f64,
    pub avg_body_size: usize,
    pub throughput_rps: f64,
    pub skipped: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skip_reason: Option<String>,
}

const SLOW_THRESHOLD_MS: f64 = 100.0;
const VERY_SLOW_THRESHOLD_MS: f64 = 500.0;

pub fn build_report(
    results: &[EndpointResult],
    api_base: &str,
    iterations: u32,
    concurrency: u32,
    warmup: u32,
) -> BenchmarkReport {
    let tested = results.iter().filter(|r| !r.skipped).count();
    let skipped = results.iter().filter(|r| r.skipped).count();
    let slow_count = results
        .iter()
        .filter(|r| !r.skipped && r.metrics.p95_ms > SLOW_THRESHOLD_MS)
        .count();
    let very_slow_count = results
        .iter()
        .filter(|r| !r.skipped && r.metrics.p95_ms > VERY_SLOW_THRESHOLD_MS)
        .count();
    let error_count = results
        .iter()
        .filter(|r| !r.skipped && r.metrics.error_rate > 0.0)
        .count();

    let entries: Vec<ReportEntry> = results
        .iter()
        .map(|r| ReportEntry {
            module: r.module.clone(),
            method: r.method.clone(),
            path_template: r.path_template.clone(),
            resolved_url: r.resolved_url.clone(),
            read_pattern: r.read_pattern.clone(),
            risk_tier: r.risk_tier.clone(),
            p50_ms: r.metrics.p50_ms,
            p95_ms: r.metrics.p95_ms,
            p99_ms: r.metrics.p99_ms,
            min_ms: r.metrics.min_ms,
            max_ms: r.metrics.max_ms,
            mean_ms: r.metrics.mean_ms,
            error_rate: r.metrics.error_rate,
            avg_body_size: r.metrics.avg_body_size,
            throughput_rps: r.metrics.throughput_rps,
            skipped: r.skipped,
            skip_reason: r.skip_reason.clone(),
        })
        .collect();

    BenchmarkReport {
        timestamp: Utc::now().to_rfc3339(),
        config: ReportConfig {
            api_base: api_base.to_string(),
            iterations,
            concurrency,
            warmup,
        },
        summary: ReportSummary {
            tested,
            skipped,
            slow_count,
            very_slow_count,
            error_count,
        },
        results: entries,
    }
}

pub fn print_table(report: &BenchmarkReport) {
    println!(
        "\nckbadger API Benchmark — {}",
        &report.timestamp[..19]
    );
    println!(
        "API: {} | Iterations: {} | Concurrency: {}\n",
        report.config.api_base, report.config.iterations, report.config.concurrency
    );

    // Header
    println!(
        "{:<14} {:<4} {:<42} {:<12} {:>7} {:>7} {:>7} {:>5} {:>6}  {}",
        "Module", "M", "Endpoint", "Pattern", "p50", "p95", "p99", "Errs", "Size", "Flag"
    );
    println!("{}", "─".repeat(120));

    let mut current_module = "";

    for entry in &report.results {
        // Module separator
        if entry.module != current_module {
            if !current_module.is_empty() {
                println!();
            }
            current_module = &entry.module;
        }

        if entry.skipped {
            println!(
                "{:<14} {:<4} {:<42} {:<12} {:>7} {:>7} {:>7} {:>5} {:>6}  SKIPPED",
                entry.module,
                entry.method,
                truncate(&entry.path_template, 42),
                entry.read_pattern,
                "—",
                "—",
                "—",
                "—",
                "—",
            );
            continue;
        }

        let flag = if entry.p95_ms > VERY_SLOW_THRESHOLD_MS {
            "VERY SLOW"
        } else if entry.p95_ms > SLOW_THRESHOLD_MS {
            "SLOW"
        } else if entry.error_rate > 0.0 {
            "ERRORS"
        } else {
            ""
        };

        println!(
            "{:<14} {:<4} {:<42} {:<12} {:>6.0}ms {:>6.0}ms {:>6.0}ms {:>4.0}% {:>5}  {}",
            entry.module,
            entry.method,
            truncate(&entry.path_template, 42),
            entry.read_pattern,
            entry.p50_ms,
            entry.p95_ms,
            entry.p99_ms,
            entry.error_rate * 100.0,
            format_size(entry.avg_body_size),
            flag,
        );
    }

    println!("{}", "─".repeat(120));
    println!(
        "Summary: {} tested, {} skipped | {} slow (p95>100ms), {} very slow (p95>500ms), {} with errors",
        report.summary.tested,
        report.summary.skipped,
        report.summary.slow_count,
        report.summary.very_slow_count,
        report.summary.error_count,
    );

    // Top 5 slowest
    let mut by_p95: Vec<&ReportEntry> = report
        .results
        .iter()
        .filter(|r| !r.skipped)
        .collect();
    by_p95.sort_by(|a, b| b.p95_ms.partial_cmp(&a.p95_ms).unwrap());

    if !by_p95.is_empty() {
        println!("\nSlowest 5 by p95:");
        for entry in by_p95.iter().take(5) {
            println!(
                "  {:.0}ms  {} {}",
                entry.p95_ms, entry.method, entry.path_template
            );
        }
    }
}

pub fn print_json(report: &BenchmarkReport) -> Result<()> {
    let json = serde_json::to_string_pretty(report)?;
    println!("{json}");
    Ok(())
}

pub fn save_json(report: &BenchmarkReport, path: &str) -> Result<()> {
    let json = serde_json::to_string_pretty(report)?;
    let mut file = std::fs::File::create(path)?;
    file.write_all(json.as_bytes())?;
    println!("Report saved to {path}");
    Ok(())
}

/// Compare current report against a baseline JSON file.
pub fn compare_reports(current: &BenchmarkReport, baseline_path: &str) -> Result<()> {
    let baseline_json = std::fs::read_to_string(baseline_path)?;
    let baseline: BenchmarkReport = serde_json::from_str(&baseline_json)?;

    println!(
        "\nRegression Report (vs baseline {})\n",
        &baseline.timestamp[..19]
    );
    println!(
        "{:<4} {:<42} {:>10} {:>10} {:>8}  {}",
        "M", "Endpoint", "Base p95", "Now p95", "Change", "Status"
    );
    println!("{}", "─".repeat(90));

    let mut regressions = 0;

    for current_entry in &current.results {
        if current_entry.skipped {
            continue;
        }

        // Find matching baseline entry
        let baseline_entry = baseline.results.iter().find(|b| {
            b.path_template == current_entry.path_template && b.method == current_entry.method
        });

        let Some(base) = baseline_entry else {
            println!(
                "{:<4} {:<42} {:>10} {:>9.0}ms {:>8}  NEW",
                current_entry.method,
                truncate(&current_entry.path_template, 42),
                "—",
                current_entry.p95_ms,
                "—",
            );
            continue;
        };

        if base.skipped {
            continue;
        }

        let change_pct = if base.p95_ms > 0.0 {
            ((current_entry.p95_ms - base.p95_ms) / base.p95_ms) * 100.0
        } else {
            0.0
        };

        let status = if change_pct > 20.0 {
            regressions += 1;
            "REGRESSION"
        } else if change_pct < -10.0 {
            "improved"
        } else {
            "stable"
        };

        println!(
            "{:<4} {:<42} {:>9.0}ms {:>9.0}ms {:>+7.0}%  {}",
            current_entry.method,
            truncate(&current_entry.path_template, 42),
            base.p95_ms,
            current_entry.p95_ms,
            change_pct,
            status,
        );
    }

    println!("{}", "─".repeat(90));
    if regressions > 0 {
        println!("{regressions} regressions detected (>20% p95 increase)");
    } else {
        println!("No regressions detected.");
    }

    Ok(())
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}...", &s[..max - 3])
    }
}

fn format_size(bytes: usize) -> String {
    if bytes >= 1_048_576 {
        format!("{}MB", bytes / 1_048_576)
    } else if bytes >= 1024 {
        format!("{}KB", bytes / 1024)
    } else {
        format!("{}B", bytes)
    }
}
```

- [ ] **Step 2: Verify it compiles**

Add `mod report;` to main.rs.

Run: `cargo check -p ckbadger-bench`
Expected: compiles with no errors

- [ ] **Step 3: Commit**

```bash
git add crates/bench/src/report.rs crates/bench/src/main.rs
git commit -m "feat(bench): add reporting — terminal table, JSON output, regression comparison"
```

---

### Task 6: Endpoint Registry — All 18 Modules

**Files:**
- Create: `crates/bench/src/endpoints/mod.rs`
- Create: `crates/bench/src/endpoints/activities.rs`
- Create: `crates/bench/src/endpoints/assets.rs`
- Create: `crates/bench/src/endpoints/blocks.rs`
- Create: `crates/bench/src/endpoints/cells.rs`
- Create: `crates/bench/src/endpoints/dao.rs`
- Create: `crates/bench/src/endpoints/fiber.rs`
- Create: `crates/bench/src/endpoints/forks.rs`
- Create: `crates/bench/src/endpoints/graph.rs`
- Create: `crates/bench/src/endpoints/hardforks.rs`
- Create: `crates/bench/src/endpoints/identities.rs`
- Create: `crates/bench/src/endpoints/mempool.rs`
- Create: `crates/bench/src/endpoints/scripts.rs`
- Create: `crates/bench/src/endpoints/search.rs`
- Create: `crates/bench/src/endpoints/spore.rs`
- Create: `crates/bench/src/endpoints/statistics.rs`
- Create: `crates/bench/src/endpoints/tokens.rs`
- Create: `crates/bench/src/endpoints/transactions.rs`

This is the largest task. Each module file defines its endpoint entries and returns them as a `Vec<EndpointEntry>`. The `mod.rs` aggregates all modules into one registry.

- [ ] **Step 1: Create endpoints/mod.rs**

```rust
mod activities;
mod assets;
mod blocks;
mod cells;
mod dao;
mod fiber;
mod forks;
mod graph;
mod hardforks;
mod identities;
mod mempool;
mod scripts;
mod search;
mod spore;
mod statistics;
mod tokens;
mod transactions;

use crate::registry::Registry;

pub fn register_all() -> Registry {
    let mut reg = Registry::new();

    for entry in activities::entries() {
        reg.add(entry);
    }
    for entry in assets::entries() {
        reg.add(entry);
    }
    for entry in blocks::entries() {
        reg.add(entry);
    }
    for entry in cells::entries() {
        reg.add(entry);
    }
    for entry in dao::entries() {
        reg.add(entry);
    }
    for entry in fiber::entries() {
        reg.add(entry);
    }
    for entry in forks::entries() {
        reg.add(entry);
    }
    for entry in graph::entries() {
        reg.add(entry);
    }
    for entry in hardforks::entries() {
        reg.add(entry);
    }
    for entry in identities::entries() {
        reg.add(entry);
    }
    for entry in mempool::entries() {
        reg.add(entry);
    }
    for entry in scripts::entries() {
        reg.add(entry);
    }
    for entry in search::entries() {
        reg.add(entry);
    }
    for entry in spore::entries() {
        reg.add(entry);
    }
    for entry in statistics::entries() {
        reg.add(entry);
    }
    for entry in tokens::entries() {
        reg.add(entry);
    }
    for entry in transactions::entries() {
        reg.add(entry);
    }

    reg
}
```

- [ ] **Step 2: Create endpoints/activities.rs**

```rust
use crate::registry::*;

pub fn entries() -> Vec<EndpointEntry> {
    vec![
        EndpointEntry {
            module: "activities",
            method: Method::Get,
            path_template: "/activities",
            description: "Global activities",
            resolve: Box::new(|base, _p| Some(get(&format!("{base}/activities?limit=20")))),
            expect_status: 200,
            risk_tier: RiskTier::High,
            read_pattern: ReadPattern::RangeScan,
        },
        EndpointEntry {
            module: "activities",
            method: Method::Get,
            path_template: "/activities/latest",
            description: "Latest activities (scans until 64 non-cellbase)",
            resolve: Box::new(|base, _p| Some(get(&format!("{base}/activities/latest")))),
            expect_status: 200,
            risk_tier: RiskTier::High,
            read_pattern: ReadPattern::RangeScan,
        },
        EndpointEntry {
            module: "activities",
            method: Method::Get,
            path_template: "/addresses/{addr}/activities",
            description: "Address activities (filtered range scan)",
            resolve: Box::new(|base, p| {
                p.top_addresses
                    .first()
                    .map(|a| get(&format!("{base}/addresses/{a}/activities?limit=50")))
            }),
            expect_status: 200,
            risk_tier: RiskTier::High,
            read_pattern: ReadPattern::RangeScan,
        },
    ]
}
```

- [ ] **Step 3: Create endpoints/blocks.rs**

```rust
use crate::registry::*;

pub fn entries() -> Vec<EndpointEntry> {
    vec![
        EndpointEntry {
            module: "blocks",
            method: Method::Get,
            path_template: "/blocks",
            description: "List blocks (desc, with epoch enrichment)",
            resolve: Box::new(|base, _p| Some(get(&format!("{base}/blocks?limit=20")))),
            expect_status: 200,
            risk_tier: RiskTier::Medium,
            read_pattern: ReadPattern::PrefixScan,
        },
        EndpointEntry {
            module: "blocks",
            method: Method::Get,
            path_template: "/blocks/{id} (by number)",
            description: "Get block by number",
            resolve: Box::new(|base, p| {
                Some(get(&format!("{base}/blocks/{}", p.latest_block_number)))
            }),
            expect_status: 200,
            risk_tier: RiskTier::Low,
            read_pattern: ReadPattern::KeyLookup,
        },
        EndpointEntry {
            module: "blocks",
            method: Method::Get,
            path_template: "/blocks/{id} (by hash)",
            description: "Get block by hash",
            resolve: Box::new(|base, p| {
                if p.latest_block_hash.is_empty() {
                    None
                } else {
                    Some(get(&format!("{base}/blocks/{}", p.latest_block_hash)))
                }
            }),
            expect_status: 200,
            risk_tier: RiskTier::Low,
            read_pattern: ReadPattern::KeyLookup,
        },
        EndpointEntry {
            module: "blocks",
            method: Method::Get,
            path_template: "/blocks/{id}/fee-stats",
            description: "Block fee stats (list block txs)",
            resolve: Box::new(|base, p| {
                Some(get(&format!(
                    "{base}/blocks/{}/fee-stats",
                    p.latest_block_number
                )))
            }),
            expect_status: 200,
            risk_tier: RiskTier::Medium,
            read_pattern: ReadPattern::BatchLookup,
        },
        EndpointEntry {
            module: "blocks",
            method: Method::Get,
            path_template: "/blocks/{id}/proposals",
            description: "Block proposals",
            resolve: Box::new(|base, p| {
                Some(get(&format!(
                    "{base}/blocks/{}/proposals",
                    p.latest_block_number
                )))
            }),
            expect_status: 200,
            risk_tier: RiskTier::Low,
            read_pattern: ReadPattern::KeyLookup,
        },
    ]
}
```

- [ ] **Step 4: Create endpoints/cells.rs**

```rust
use crate::registry::*;

pub fn entries() -> Vec<EndpointEntry> {
    vec![
        EndpointEntry {
            module: "cells",
            method: Method::Get,
            path_template: "/cells/live",
            description: "List live cells (cross-store)",
            resolve: Box::new(|base, _p| Some(get(&format!("{base}/cells/live?limit=20")))),
            expect_status: 200,
            risk_tier: RiskTier::High,
            read_pattern: ReadPattern::CrossStore,
        },
        EndpointEntry {
            module: "cells",
            method: Method::Get,
            path_template: "/cells/by-script",
            description: "List cells by script hash",
            resolve: Box::new(|base, p| {
                p.top_lock_hashes
                    .first()
                    .map(|lh| get(&format!("{base}/cells/by-script?lock_script_hash={lh}&limit=10")))
            }),
            expect_status: 200,
            risk_tier: RiskTier::High,
            read_pattern: ReadPattern::CrossStore,
        },
        EndpointEntry {
            module: "cells",
            method: Method::Get,
            path_template: "/cells/{tx_hash}/{output_index}",
            description: "Get single cell (cross-store lookup)",
            resolve: Box::new(|base, p| {
                p.live_cell_outpoint
                    .as_ref()
                    .map(|(h, i)| get(&format!("{base}/cells/{h}/{i}")))
            }),
            expect_status: 200,
            risk_tier: RiskTier::High,
            read_pattern: ReadPattern::CrossStore,
        },
        EndpointEntry {
            module: "cells",
            method: Method::Get,
            path_template: "/addresses/top",
            description: "Top addresses by balance",
            resolve: Box::new(|base, _p| Some(get(&format!("{base}/addresses/top?limit=20")))),
            expect_status: 200,
            risk_tier: RiskTier::Low,
            read_pattern: ReadPattern::Cached,
        },
        EndpointEntry {
            module: "cells",
            method: Method::Get,
            path_template: "/addresses/active",
            description: "Active addresses",
            resolve: Box::new(|base, _p| Some(get(&format!("{base}/addresses/active?limit=20")))),
            expect_status: 200,
            risk_tier: RiskTier::Low,
            read_pattern: ReadPattern::Cached,
        },
        EndpointEntry {
            module: "cells",
            method: Method::Get,
            path_template: "/addresses/{addr}",
            description: "Get address detail",
            resolve: Box::new(|base, p| {
                p.top_addresses
                    .first()
                    .map(|a| get(&format!("{base}/addresses/{a}")))
            }),
            expect_status: 200,
            risk_tier: RiskTier::Low,
            read_pattern: ReadPattern::KeyLookup,
        },
        EndpointEntry {
            module: "cells",
            method: Method::Get,
            path_template: "/addresses/{addr}/transactions",
            description: "Address transaction history",
            resolve: Box::new(|base, p| {
                p.top_addresses
                    .first()
                    .map(|a| get(&format!("{base}/addresses/{a}/transactions?limit=20")))
            }),
            expect_status: 200,
            risk_tier: RiskTier::High,
            read_pattern: ReadPattern::PrefixScan,
        },
        EndpointEntry {
            module: "cells",
            method: Method::Get,
            path_template: "/addresses/{addr}/tokens",
            description: "Address token balances",
            resolve: Box::new(|base, p| {
                p.top_addresses
                    .first()
                    .map(|a| get(&format!("{base}/addresses/{a}/tokens")))
            }),
            expect_status: 200,
            risk_tier: RiskTier::High,
            read_pattern: ReadPattern::PrefixScan,
        },
    ]
}
```

- [ ] **Step 5: Create endpoints/dao.rs**

```rust
use crate::registry::*;

pub fn entries() -> Vec<EndpointEntry> {
    vec![
        EndpointEntry {
            module: "dao",
            method: Method::Get,
            path_template: "/dao/deposits",
            description: "List DAO deposits (prefix scan, visitor pattern)",
            resolve: Box::new(|base, _p| Some(get(&format!("{base}/dao/deposits?limit=20")))),
            expect_status: 200,
            risk_tier: RiskTier::High,
            read_pattern: ReadPattern::PrefixScan,
        },
        EndpointEntry {
            module: "dao",
            method: Method::Get,
            path_template: "/dao/deposits/{lock_hash}",
            description: "DAO deposits by address",
            resolve: Box::new(|base, p| {
                p.dao_lock_hashes
                    .first()
                    .map(|lh| get(&format!("{base}/dao/deposits/{lh}")))
            }),
            expect_status: 200,
            risk_tier: RiskTier::High,
            read_pattern: ReadPattern::PrefixScan,
        },
        EndpointEntry {
            module: "dao",
            method: Method::Get,
            path_template: "/dao/summary/{lock_hash}",
            description: "Address DAO summary",
            resolve: Box::new(|base, p| {
                p.dao_lock_hashes
                    .first()
                    .map(|lh| get(&format!("{base}/dao/summary/{lh}")))
            }),
            expect_status: 200,
            risk_tier: RiskTier::Medium,
            read_pattern: ReadPattern::BatchLookup,
        },
        EndpointEntry {
            module: "dao",
            method: Method::Get,
            path_template: "/dao/statistics",
            description: "DAO statistics (cached)",
            resolve: Box::new(|base, _p| Some(get(&format!("{base}/dao/statistics")))),
            expect_status: 200,
            risk_tier: RiskTier::Low,
            read_pattern: ReadPattern::Cached,
        },
        EndpointEntry {
            module: "dao",
            method: Method::Get,
            path_template: "/dao/top-depositors",
            description: "Top DAO depositors",
            resolve: Box::new(|base, _p| {
                Some(get(&format!("{base}/dao/top-depositors?limit=20")))
            }),
            expect_status: 200,
            risk_tier: RiskTier::High,
            read_pattern: ReadPattern::PrefixScan,
        },
        EndpointEntry {
            module: "dao",
            method: Method::Get,
            path_template: "/dao/calculator",
            description: "DAO compensation calculator",
            resolve: Box::new(|base, p| {
                p.dao_deposit_outpoint.as_ref().map(|(h, i)| {
                    get(&format!(
                        "{base}/dao/calculator?tx_hash={h}&output_index={i}"
                    ))
                })
            }),
            expect_status: 200,
            risk_tier: RiskTier::Low,
            read_pattern: ReadPattern::KeyLookup,
        },
        EndpointEntry {
            module: "dao",
            method: Method::Get,
            path_template: "/dao/charts/total-deposit",
            description: "Total deposit chart",
            resolve: Box::new(|base, _p| {
                Some(get(&format!("{base}/dao/charts/total-deposit")))
            }),
            expect_status: 200,
            risk_tier: RiskTier::Medium,
            read_pattern: ReadPattern::Aggregation,
        },
        EndpointEntry {
            module: "dao",
            method: Method::Get,
            path_template: "/dao/charts/daily-deposit",
            description: "Daily deposit chart",
            resolve: Box::new(|base, _p| {
                Some(get(&format!("{base}/dao/charts/daily-deposit")))
            }),
            expect_status: 200,
            risk_tier: RiskTier::Medium,
            read_pattern: ReadPattern::Aggregation,
        },
        EndpointEntry {
            module: "dao",
            method: Method::Get,
            path_template: "/dao/charts/deposit-rate",
            description: "Deposit rate chart",
            resolve: Box::new(|base, _p| {
                Some(get(&format!("{base}/dao/charts/deposit-rate")))
            }),
            expect_status: 200,
            risk_tier: RiskTier::Medium,
            read_pattern: ReadPattern::Aggregation,
        },
    ]
}
```

- [ ] **Step 6: Create endpoints/tokens.rs**

```rust
use crate::registry::*;

pub fn entries() -> Vec<EndpointEntry> {
    vec![
        EndpointEntry {
            module: "tokens",
            method: Method::Get,
            path_template: "/tokens",
            description: "List all tokens (warmup cached / full CF scan)",
            resolve: Box::new(|base, _p| Some(get(&format!("{base}/tokens")))),
            expect_status: 200,
            risk_tier: RiskTier::High,
            read_pattern: ReadPattern::FullCfScan,
        },
        EndpointEntry {
            module: "tokens",
            method: Method::Get,
            path_template: "/tokens/{type_hash}",
            description: "Get token by type hash",
            resolve: Box::new(|base, p| {
                p.token_type_hashes
                    .first()
                    .map(|th| get(&format!("{base}/tokens/{th}")))
            }),
            expect_status: 200,
            risk_tier: RiskTier::Low,
            read_pattern: ReadPattern::KeyLookup,
        },
        EndpointEntry {
            module: "tokens",
            method: Method::Get,
            path_template: "/tokens/{type_hash}/holders",
            description: "Token holders list",
            resolve: Box::new(|base, p| {
                p.token_type_hashes
                    .first()
                    .map(|th| get(&format!("{base}/tokens/{th}/holders?limit=20")))
            }),
            expect_status: 200,
            risk_tier: RiskTier::High,
            read_pattern: ReadPattern::PrefixScan,
        },
        EndpointEntry {
            module: "tokens",
            method: Method::Get,
            path_template: "/tokens/{type_hash}/transfers",
            description: "Token transfers",
            resolve: Box::new(|base, p| {
                p.token_type_hashes
                    .first()
                    .map(|th| get(&format!("{base}/tokens/{th}/transfers?limit=20")))
            }),
            expect_status: 200,
            risk_tier: RiskTier::High,
            read_pattern: ReadPattern::PrefixScan,
        },
        EndpointEntry {
            module: "tokens",
            method: Method::Get,
            path_template: "/tokens/{type_hash}/activities",
            description: "Token activities",
            resolve: Box::new(|base, p| {
                p.token_type_hashes
                    .first()
                    .map(|th| get(&format!("{base}/tokens/{th}/activities?limit=20")))
            }),
            expect_status: 200,
            risk_tier: RiskTier::Medium,
            read_pattern: ReadPattern::PrefixScan,
        },
    ]
}
```

- [ ] **Step 7: Create endpoints/transactions.rs**

```rust
use crate::registry::*;

pub fn entries() -> Vec<EndpointEntry> {
    vec![
        EndpointEntry {
            module: "transactions",
            method: Method::Get,
            path_template: "/transactions",
            description: "List transactions",
            resolve: Box::new(|base, _p| {
                Some(get(&format!("{base}/transactions?limit=20")))
            }),
            expect_status: 200,
            risk_tier: RiskTier::Medium,
            read_pattern: ReadPattern::PrefixScan,
        },
        EndpointEntry {
            module: "transactions",
            method: Method::Get,
            path_template: "/transactions/{hash}",
            description: "Get transaction by hash",
            resolve: Box::new(|base, p| {
                p.tx_hashes
                    .first()
                    .map(|h| get(&format!("{base}/transactions/{h}")))
            }),
            expect_status: 200,
            risk_tier: RiskTier::Low,
            read_pattern: ReadPattern::BatchLookup,
        },
        EndpointEntry {
            module: "transactions",
            method: Method::Get,
            path_template: "/transactions/{hash}/detail",
            description: "Transaction detail (full cell resolution, cross-store)",
            resolve: Box::new(|base, p| {
                p.tx_hashes
                    .first()
                    .map(|h| get(&format!("{base}/transactions/{h}/detail")))
            }),
            expect_status: 200,
            risk_tier: RiskTier::High,
            read_pattern: ReadPattern::CrossStore,
        },
        EndpointEntry {
            module: "transactions",
            method: Method::Get,
            path_template: "/transactions/{hash}/cell-deps",
            description: "Transaction cell deps",
            resolve: Box::new(|base, p| {
                p.tx_hashes
                    .first()
                    .map(|h| get(&format!("{base}/transactions/{h}/cell-deps")))
            }),
            expect_status: 200,
            risk_tier: RiskTier::Medium,
            read_pattern: ReadPattern::CrossStore,
        },
        EndpointEntry {
            module: "transactions",
            method: Method::Get,
            path_template: "/transactions/{hash}/cycles",
            description: "Transaction cycles",
            resolve: Box::new(|base, p| {
                p.tx_hashes
                    .first()
                    .map(|h| get(&format!("{base}/transactions/{h}/cycles")))
            }),
            expect_status: 200,
            risk_tier: RiskTier::Low,
            read_pattern: ReadPattern::KeyLookup,
        },
    ]
}
```

- [ ] **Step 8: Create endpoints/statistics.rs**

```rust
use crate::registry::*;

pub fn entries() -> Vec<EndpointEntry> {
    vec![
        EndpointEntry {
            module: "statistics",
            method: Method::Get,
            path_template: "/statistics/network",
            description: "Network stats (cached)",
            resolve: Box::new(|base, _p| Some(get(&format!("{base}/statistics/network")))),
            expect_status: 200,
            risk_tier: RiskTier::Low,
            read_pattern: ReadPattern::Cached,
        },
        EndpointEntry {
            module: "statistics",
            method: Method::Get,
            path_template: "/statistics/tx-stats",
            description: "Transaction stats",
            resolve: Box::new(|base, _p| Some(get(&format!("{base}/statistics/tx-stats")))),
            expect_status: 200,
            risk_tier: RiskTier::Low,
            read_pattern: ReadPattern::Cached,
        },
        EndpointEntry {
            module: "statistics",
            method: Method::Get,
            path_template: "/statistics/recent-blocks",
            description: "Recent blocks",
            resolve: Box::new(|base, _p| Some(get(&format!("{base}/statistics/recent-blocks")))),
            expect_status: 200,
            risk_tier: RiskTier::Low,
            read_pattern: ReadPattern::Cached,
        },
        EndpointEntry {
            module: "statistics",
            method: Method::Get,
            path_template: "/statistics/asset-ecosystem",
            description: "Asset ecosystem stats",
            resolve: Box::new(|base, _p| Some(get(&format!("{base}/statistics/asset-ecosystem")))),
            expect_status: 200,
            risk_tier: RiskTier::Low,
            read_pattern: ReadPattern::Cached,
        },
        EndpointEntry {
            module: "statistics",
            method: Method::Get,
            path_template: "/charts/transaction-count",
            description: "Transaction count chart",
            resolve: Box::new(|base, _p| Some(get(&format!("{base}/charts/transaction-count")))),
            expect_status: 200,
            risk_tier: RiskTier::Medium,
            read_pattern: ReadPattern::Aggregation,
        },
        EndpointEntry {
            module: "statistics",
            method: Method::Get,
            path_template: "/charts/cell-count",
            description: "Cell count chart",
            resolve: Box::new(|base, _p| Some(get(&format!("{base}/charts/cell-count")))),
            expect_status: 200,
            risk_tier: RiskTier::Medium,
            read_pattern: ReadPattern::Aggregation,
        },
        EndpointEntry {
            module: "statistics",
            method: Method::Get,
            path_template: "/charts/knowledge-size",
            description: "Knowledge size chart",
            resolve: Box::new(|base, _p| Some(get(&format!("{base}/charts/knowledge-size")))),
            expect_status: 200,
            risk_tier: RiskTier::Medium,
            read_pattern: ReadPattern::Aggregation,
        },
        EndpointEntry {
            module: "statistics",
            method: Method::Get,
            path_template: "/charts/common-knowledge-composition",
            description: "Common knowledge composition chart",
            resolve: Box::new(|base, _p| Some(get(&format!("{base}/charts/common-knowledge-composition")))),
            expect_status: 200,
            risk_tier: RiskTier::Medium,
            read_pattern: ReadPattern::Aggregation,
        },
        EndpointEntry {
            module: "statistics",
            method: Method::Get,
            path_template: "/charts/capacity-turnover-ratio",
            description: "Capacity turnover ratio chart",
            resolve: Box::new(|base, _p| Some(get(&format!("{base}/charts/capacity-turnover-ratio")))),
            expect_status: 200,
            risk_tier: RiskTier::Medium,
            read_pattern: ReadPattern::Aggregation,
        },
        EndpointEntry {
            module: "statistics",
            method: Method::Get,
            path_template: "/charts/cell-size-distribution",
            description: "Cell size distribution chart",
            resolve: Box::new(|base, _p| Some(get(&format!("{base}/charts/cell-size-distribution")))),
            expect_status: 200,
            risk_tier: RiskTier::Medium,
            read_pattern: ReadPattern::Aggregation,
        },
        EndpointEntry {
            module: "statistics",
            method: Method::Get,
            path_template: "/charts/address-cohort-retention",
            description: "Address cohort retention chart",
            resolve: Box::new(|base, _p| Some(get(&format!("{base}/charts/address-cohort-retention")))),
            expect_status: 200,
            risk_tier: RiskTier::Medium,
            read_pattern: ReadPattern::Aggregation,
        },
        EndpointEntry {
            module: "statistics",
            method: Method::Get,
            path_template: "/charts/most-utilized-scripts",
            description: "Most utilized scripts chart",
            resolve: Box::new(|base, _p| Some(get(&format!("{base}/charts/most-utilized-scripts")))),
            expect_status: 200,
            risk_tier: RiskTier::Medium,
            read_pattern: ReadPattern::Aggregation,
        },
        EndpointEntry {
            module: "statistics",
            method: Method::Get,
            path_template: "/charts/most-utilized-assets",
            description: "Most utilized assets chart",
            resolve: Box::new(|base, _p| Some(get(&format!("{base}/charts/most-utilized-assets")))),
            expect_status: 200,
            risk_tier: RiskTier::Medium,
            read_pattern: ReadPattern::Cached,
        },
        EndpointEntry {
            module: "statistics",
            method: Method::Get,
            path_template: "/charts/block-time-distribution",
            description: "Block time distribution chart",
            resolve: Box::new(|base, _p| Some(get(&format!("{base}/charts/block-time-distribution")))),
            expect_status: 200,
            risk_tier: RiskTier::Medium,
            read_pattern: ReadPattern::Aggregation,
        },
        EndpointEntry {
            module: "statistics",
            method: Method::Get,
            path_template: "/charts/epoch-time-distribution",
            description: "Epoch time distribution chart",
            resolve: Box::new(|base, _p| Some(get(&format!("{base}/charts/epoch-time-distribution")))),
            expect_status: 200,
            risk_tier: RiskTier::Medium,
            read_pattern: ReadPattern::Aggregation,
        },
        EndpointEntry {
            module: "statistics",
            method: Method::Get,
            path_template: "/charts/epoch-time-length",
            description: "Epoch time length chart",
            resolve: Box::new(|base, _p| Some(get(&format!("{base}/charts/epoch-time-length")))),
            expect_status: 200,
            risk_tier: RiskTier::Medium,
            read_pattern: ReadPattern::Aggregation,
        },
        EndpointEntry {
            module: "statistics",
            method: Method::Get,
            path_template: "/charts/average-block-time",
            description: "Average block time chart",
            resolve: Box::new(|base, _p| Some(get(&format!("{base}/charts/average-block-time")))),
            expect_status: 200,
            risk_tier: RiskTier::Medium,
            read_pattern: ReadPattern::Aggregation,
        },
        EndpointEntry {
            module: "statistics",
            method: Method::Get,
            path_template: "/charts/hash-rate",
            description: "Hash rate chart",
            resolve: Box::new(|base, _p| Some(get(&format!("{base}/charts/hash-rate")))),
            expect_status: 200,
            risk_tier: RiskTier::Medium,
            read_pattern: ReadPattern::Aggregation,
        },
        EndpointEntry {
            module: "statistics",
            method: Method::Get,
            path_template: "/charts/difficulty",
            description: "Difficulty chart",
            resolve: Box::new(|base, _p| Some(get(&format!("{base}/charts/difficulty")))),
            expect_status: 200,
            risk_tier: RiskTier::Medium,
            read_pattern: ReadPattern::Aggregation,
        },
        EndpointEntry {
            module: "statistics",
            method: Method::Get,
            path_template: "/charts/uncle-rate",
            description: "Uncle rate chart",
            resolve: Box::new(|base, _p| Some(get(&format!("{base}/charts/uncle-rate")))),
            expect_status: 200,
            risk_tier: RiskTier::Medium,
            read_pattern: ReadPattern::Aggregation,
        },
        EndpointEntry {
            module: "statistics",
            method: Method::Get,
            path_template: "/charts/miner-address-distribution",
            description: "Miner address distribution chart",
            resolve: Box::new(|base, _p| Some(get(&format!("{base}/charts/miner-address-distribution")))),
            expect_status: 200,
            risk_tier: RiskTier::Medium,
            read_pattern: ReadPattern::Aggregation,
        },
        EndpointEntry {
            module: "statistics",
            method: Method::Get,
            path_template: "/charts/total-supply",
            description: "Total supply chart",
            resolve: Box::new(|base, _p| Some(get(&format!("{base}/charts/total-supply")))),
            expect_status: 200,
            risk_tier: RiskTier::Medium,
            read_pattern: ReadPattern::Aggregation,
        },
        EndpointEntry {
            module: "statistics",
            method: Method::Get,
            path_template: "/charts/nominal-apc",
            description: "Nominal APC chart",
            resolve: Box::new(|base, _p| Some(get(&format!("{base}/charts/nominal-apc")))),
            expect_status: 200,
            risk_tier: RiskTier::Medium,
            read_pattern: ReadPattern::Aggregation,
        },
        EndpointEntry {
            module: "statistics",
            method: Method::Get,
            path_template: "/charts/secondary-issuance",
            description: "Secondary issuance chart",
            resolve: Box::new(|base, _p| Some(get(&format!("{base}/charts/secondary-issuance")))),
            expect_status: 200,
            risk_tier: RiskTier::Medium,
            read_pattern: ReadPattern::Aggregation,
        },
        EndpointEntry {
            module: "statistics",
            method: Method::Get,
            path_template: "/charts/inflation-rate",
            description: "Inflation rate chart",
            resolve: Box::new(|base, _p| Some(get(&format!("{base}/charts/inflation-rate")))),
            expect_status: 200,
            risk_tier: RiskTier::Medium,
            read_pattern: ReadPattern::Aggregation,
        },
        EndpointEntry {
            module: "statistics",
            method: Method::Get,
            path_template: "/charts/hodl-wave",
            description: "HODL wave chart",
            resolve: Box::new(|base, _p| Some(get(&format!("{base}/charts/hodl-wave")))),
            expect_status: 200,
            risk_tier: RiskTier::Medium,
            read_pattern: ReadPattern::Aggregation,
        },
        EndpointEntry {
            module: "statistics",
            method: Method::Get,
            path_template: "/stats/daily-activities",
            description: "Daily activity stats",
            resolve: Box::new(|base, _p| Some(get(&format!("{base}/stats/daily-activities")))),
            expect_status: 200,
            risk_tier: RiskTier::Medium,
            read_pattern: ReadPattern::Aggregation,
        },
        EndpointEntry {
            module: "statistics",
            method: Method::Get,
            path_template: "/stats/activity-summary-24h",
            description: "Activity summary (24h)",
            resolve: Box::new(|base, _p| Some(get(&format!("{base}/stats/activity-summary-24h")))),
            expect_status: 200,
            risk_tier: RiskTier::Low,
            read_pattern: ReadPattern::Cached,
        },
    ]
}
```

- [ ] **Step 9: Create remaining endpoint modules**

Create these files following the same pattern. Each returns `Vec<EndpointEntry>`.

**endpoints/spore.rs** — 13 endpoints:
`/spore/clusters`, `/spore/clusters/{cluster_id}`, `/spore/clusters/{cluster_id}/charts/capacity-history`, `/spore/clusters/{cluster_id}/holders`, `/spore/clusters/{cluster_id}/activities`, `/spore/clusters/{cluster_id}/spores`, `/spore/objects`, `/spore/objects/{spore_id}`, `/spore/objects/{spore_id}/activities`, `/spore/objects/{spore_id}/decode`, `/spore/objects/{spore_id}/render`, `/spore/objects/{spore_id}/charts/capacity-history`, `/spore/owner/{lock_hash}`

Note: Skip `/spore/objects/{spore_id}/media/{hash}` (binary response, not JSON benchmark-able). Resolve cluster_id from `p.cluster_ids.first()`, spore_id from `p.spore_ids.first()`, lock_hash from `p.top_lock_hashes.first()`.

**endpoints/assets.rs** — 8 endpoints:
`/assets`, `/assets/objects/items/{object_id}`, `/assets/objects/items/{object_id}/activities`, `/assets/objects/{collection_id}`, `/assets/objects/{collection_id}/items`, `/assets/objects/{collection_id}/holders`, `/assets/objects/{collection_id}/activities`, `/assets/objects/{collection_id}/charts/capacity-history`

Resolve collection_id from `p.object_collection_id`, object_id from `p.object_item_id`.

**endpoints/identities.rs** — 8 endpoints:
`/assets/identities/dotbit/items/{identity_id}`, `/assets/identities/dotbit/items/{identity_id}/activities`, `/assets/identities/did/items/{identity_id}` (may skip if no DID data), `/assets/identities/did/items/{identity_id}/activities`, `/assets/identities/{collection_id}`, `/assets/identities/{collection_id}/holders`, `/assets/identities/{collection_id}/activities`, `/assets/identities/{collection_id}/items`

Resolve identity_id from `p.dotbit_item_id`, collection_id from `p.identity_collection_id` (default "dotbit").

**endpoints/fiber.rs** — 4 endpoints:
`/fiber/channels`, `/fiber/channels/{channel_id}`, `/fiber/channels/{channel_id}/nodes`, `/fiber/stats`

Resolve channel_id from `p.fiber_channel_id`.

**endpoints/forks.rs** — 3 endpoints:
`/forks`, `/forks/recent`, `/forks/{id}`

Resolve fork_id from `p.fork_id`.

**endpoints/graph.rs** — 3 endpoints:
`/graph/cell/{tx_hash}/{output_index}`, `/graph/transaction/{hash}`, `/graph/proposals/{block_number}`

Resolve from `p.live_cell_outpoint`, `p.tx_hashes.first()`, `p.latest_block_number`. All RpcDependent.

**endpoints/hardforks.rs** — 1 endpoint:
`/hardforks`

**endpoints/mempool.rs** — 4 endpoints:
`/mempool/info`, `/mempool/transactions`, `/mempool/blocks`, `/mempool/pending-proposals`

All RpcDependent.

**endpoints/scripts.rs** — 8 endpoints:
`/scripts`, `/scripts/lookup` (POST, body: `{"hashes":["<script_name>"]}`), `/scripts/code-cell`, `/scripts/code-cells`, `/scripts/charts/capacity-history`, `/scripts/{name}`, `/scripts/{name}/usage`, `/scripts/{name}/charts/capacity-history`

Resolve name from `p.script_names.first()`.

**endpoints/search.rs** — 1 endpoint:
`/search?q=<latest_block_number>`

- [ ] **Step 10: Add `mod endpoints;` to main.rs and verify**

Run: `cargo check -p ckbadger-bench`
Expected: compiles with no errors

- [ ] **Step 11: Commit**

```bash
git add crates/bench/src/endpoints/
git commit -m "feat(bench): add endpoint registry — all 18 modules, ~90 endpoints"
```

---

### Task 7: Wire Everything Together in main.rs

**Files:**
- Modify: `crates/bench/src/main.rs`

- [ ] **Step 1: Update main.rs with full orchestration**

```rust
mod discovery;
mod endpoints;
mod metrics;
mod registry;
mod report;
mod runner;

use anyhow::{bail, Result};
use clap::Parser;
use std::time::{Duration, Instant};

use registry::RiskTier;
use runner::RunConfig;

#[derive(Parser)]
#[command(name = "ckbadger-bench", about = "API performance benchmark")]
struct Cli {
    #[arg(long, default_value = "http://localhost:8101/api/v1")]
    api_url: String,
    #[arg(long, default_value = "http://localhost:8100")]
    frontend_url: String,
    #[arg(long, default_value = "10")]
    iterations: u32,
    #[arg(long, default_value = "1")]
    concurrency: u32,
    #[arg(long, default_value = "2")]
    warmup: u32,
    #[arg(long, default_value = "10000")]
    timeout_ms: u64,
    #[arg(long)]
    module: Option<String>,
    #[arg(long)]
    endpoint: Option<String>,
    #[arg(long)]
    risk: Option<String>,
    #[arg(long)]
    json: bool,
    #[arg(long)]
    output: Option<String>,
    #[arg(long)]
    compare: Option<String>,
    #[arg(long)]
    discovery_only: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    let client = reqwest::Client::builder()
        .timeout(Duration::from_millis(cli.timeout_ms))
        .build()?;

    // Connectivity check
    match client
        .get(format!("{}/statistics/network", &cli.api_url))
        .send()
        .await
    {
        Ok(resp) if resp.status().is_success() => {}
        Ok(resp) => bail!(
            "API returned {} — is ckbadger API running at {}?",
            resp.status(),
            cli.api_url
        ),
        Err(e) => bail!(
            "Cannot connect to {} — is ckbadger API running? Error: {}",
            cli.api_url,
            e
        ),
    }

    // Discovery
    eprintln!("Discovering parameters...");
    let disc = discovery::run_discovery(&cli.api_url, &cli.frontend_url, &client).await?;

    if cli.discovery_only {
        discovery::print_discovery(&disc);
        return Ok(());
    }

    // Build registry
    let mut registry = endpoints::register_all();
    let total_registered = registry.entries.len();

    // Apply filters
    if let Some(ref module) = cli.module {
        registry.filter_module(module);
    }
    if let Some(ref endpoint) = cli.endpoint {
        registry.filter_endpoint(endpoint);
    }
    if let Some(ref risk_str) = cli.risk {
        if let Some(tier) = RiskTier::from_str_opt(risk_str) {
            registry.filter_risk(tier);
        } else {
            bail!("Invalid risk tier: {risk_str}. Use: high, medium, low");
        }
    }

    // Sort: low risk first (warms cache), then medium, then high
    registry.sort_by_risk();

    eprintln!(
        "Running {} of {} endpoints ({} iterations, concurrency {})...\n",
        registry.entries.len(),
        total_registered,
        cli.iterations,
        cli.concurrency,
    );

    let run_config = RunConfig {
        iterations: cli.iterations,
        concurrency: cli.concurrency,
        warmup: cli.warmup,
    };

    // Execute benchmarks
    let mut results = Vec::new();
    let bench_start = Instant::now();

    for (i, entry) in registry.entries.iter().enumerate() {
        eprint!(
            "[{}/{}] {} {} {} ...",
            i + 1,
            registry.entries.len(),
            entry.module,
            entry.method,
            entry.path_template,
        );

        let result =
            runner::bench_endpoint(&client, entry, &cli.api_url, &disc.params, &run_config).await;

        if result.skipped {
            eprintln!(" SKIPPED");
        } else {
            eprintln!(" p95={:.0}ms", result.metrics.p95_ms);
        }

        results.push(result);
    }

    let total_time = bench_start.elapsed();
    eprintln!("\nBenchmark complete in {:.1}s", total_time.as_secs_f64());

    // Build report
    let bench_report = report::build_report(
        &results,
        &cli.api_url,
        cli.iterations,
        cli.concurrency,
        cli.warmup,
    );

    // Output
    if cli.json {
        report::print_json(&bench_report)?;
    } else {
        report::print_table(&bench_report);
    }

    if let Some(ref path) = cli.output {
        report::save_json(&bench_report, path)?;
    }

    if let Some(ref baseline_path) = cli.compare {
        report::compare_reports(&bench_report, baseline_path)?;
    }

    Ok(())
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo check -p ckbadger-bench`
Expected: compiles with no errors

- [ ] **Step 3: Build the binary**

Run: `cargo build -p ckbadger-bench`
Expected: binary built at `target/debug/ckbadger-bench`

- [ ] **Step 4: Test --help output**

Run: `cargo run -p ckbadger-bench -- --help`
Expected: shows CLI options

- [ ] **Step 5: Commit**

```bash
git add crates/bench/src/main.rs
git commit -m "feat(bench): wire orchestration — discovery, registry, runner, reporting"
```

---

### Task 8: Smoke Test Against Live API

This task validates the benchmark works end-to-end against a running ckbadger instance.

**Prerequisites:** ckbadger API must be running on localhost:8101.

- [ ] **Step 1: Run discovery-only mode**

Run: `cargo run -p ckbadger-bench -- --discovery-only`
Expected: prints discovered parameters (sync tip, tx hashes, addresses, etc.)

- [ ] **Step 2: Run single module with 3 iterations**

Run: `cargo run -p ckbadger-bench -- --module blocks --iterations 3`
Expected: table output showing 5 block endpoints with latencies

- [ ] **Step 3: Run full benchmark with JSON output**

Run: `cargo run -p ckbadger-bench -- --iterations 5 --json --output /tmp/claude-1000/bench-baseline.json`
Expected: JSON written to file, all endpoints measured or skipped

- [ ] **Step 4: Run regression comparison**

Run: `cargo run -p ckbadger-bench -- --iterations 5 --compare /tmp/claude-1000/bench-baseline.json`
Expected: regression report comparing against baseline

- [ ] **Step 5: Fix any issues found during smoke testing**

Adjust response parsing in discovery.rs if field names don't match. Fix any endpoint resolvers that return wrong URLs.

- [ ] **Step 6: Commit fixes**

```bash
git add -A crates/bench/
git commit -m "fix(bench): smoke test fixes from live API validation"
```

---

### Task 9: Add to Makefile

**Files:**
- Modify: `Makefile` (if exists) or document in CLAUDE.md

- [ ] **Step 1: Check if Makefile exists**

Run: `ls Makefile`

- [ ] **Step 2: Add bench target**

If Makefile exists, add:

```makefile
bench:
	cargo run -p ckbadger-bench -- $(BENCH_ARGS)

bench-json:
	cargo run -p ckbadger-bench -- --json $(BENCH_ARGS)

bench-baseline:
	cargo run -p ckbadger-bench -- --json --output bench-baseline.json $(BENCH_ARGS)
```

- [ ] **Step 3: Commit**

```bash
git add Makefile
git commit -m "feat(bench): add make targets for benchmark"
```
