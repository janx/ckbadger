# Stress Testing Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a `stress` subcommand to `ckbadger-bench` that performs staged concurrent load testing against the API and frontend, detecting degradation and breaking points.

**Architecture:** Extend the existing `crates/bench/` crate with a `stress/` module tree. The stress runner spawns N tokio tasks (VUs) that loop sending weighted random requests. A dedicated collector task aggregates samples per stage via an mpsc channel. Stages ramp up VU count, and auto-ramp mode stops when hard failure is detected.

**Tech Stack:** Rust, tokio, reqwest, clap (subcommands), serde_json, chrono

**Spec:** `docs/superpowers/specs/2026-04-07-stress-testing-design.md`

---

## File Structure

```
crates/bench/src/
  main.rs              # MODIFY: add clap subcommand dispatch (bench vs stress)
  runner.rs            # MODIFY: make execute_request pub
  metrics.rs           # MODIFY: make percentile pub
  stress/
    mod.rs             # CREATE: stress subcommand entry, stage scheduler, run_stress()
    scenario.rs        # CREATE: Scenario enum, endpoint group weights, build functions
    collector.rs       # CREATE: StressSample, StageResult, collector task, degradation detection
    vu.rs              # CREATE: VU loop task, endpoint selection, think time
    report.rs          # CREATE: stage summary, endpoint breakdown, read pattern summary, JSON
```

Existing files `registry.rs`, `discovery.rs`, `endpoints/` are unchanged.

---

### Task 1: Refactor main.rs for subcommand dispatch and expose internals

Make `execute_request` and `percentile` public so the stress module can reuse them. Restructure `main.rs` with clap subcommands while preserving backward compatibility (no subcommand = bench).

**Files:**
- Modify: `crates/bench/src/runner.rs:38` (change `async fn` to `pub async fn`)
- Modify: `crates/bench/src/metrics.rs:79` (change `fn` to `pub fn`)
- Modify: `crates/bench/src/main.rs` (restructure to subcommands)

- [ ] **Step 1: Make execute_request public**

In `crates/bench/src/runner.rs`, change line 38 from:

```rust
async fn execute_request(
```

to:

```rust
pub async fn execute_request(
```

- [ ] **Step 2: Make percentile public**

In `crates/bench/src/metrics.rs`, change line 79 from:

```rust
fn percentile(sorted: &[f64], pct: f64) -> f64 {
```

to:

```rust
pub fn percentile(sorted: &[f64], pct: f64) -> f64 {
```

- [ ] **Step 3: Restructure main.rs with subcommands**

Replace `crates/bench/src/main.rs` with subcommand dispatch. The key change: current flat `Cli` struct becomes `BenchArgs` under a `Commands::Bench` variant. A new `Commands::Stress` variant is added (placeholder struct for now). When no subcommand is given, default to bench behavior.

```rust
mod discovery;
mod endpoints;
mod metrics;
mod registry;
mod report;
mod runner;
mod stress;

use std::path::PathBuf;
use std::time::Instant;

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};

use crate::discovery::{check_connectivity, print_discovery, run_discovery};
use crate::registry::RiskTier;

#[derive(Parser)]
#[command(name = "ckbadger-bench", about = "API performance benchmark & stress testing")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Per-endpoint baseline benchmarking (default)
    Bench(BenchArgs),
    /// Concurrent load stress testing with staged ramp-up
    Stress(stress::StressArgs),
}

#[derive(Parser)]
struct BenchArgs {
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

    /// Directory for auto-saved timestamped reports
    #[arg(long)]
    output_dir: Option<PathBuf>,

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

    match cli.command {
        Some(Commands::Stress(args)) => stress::run_stress(args).await,
        Some(Commands::Bench(args)) => run_bench(args).await,
        None => {
            // Re-parse as BenchArgs for backward compatibility.
            // When no subcommand is given, treat all args as bench args.
            let args = BenchArgs::parse();
            run_bench(args).await
        }
    }
}

async fn run_bench(cli: BenchArgs) -> Result<()> {
    println!("ckbadger-bench: api_url={}", cli.api_url);

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_millis(cli.timeout_ms))
        .build()
        .context("failed to build HTTP client")?;

    println!("Checking API connectivity...");
    check_connectivity(&cli.api_url, &client).await?;
    println!("API is reachable.");

    println!("Running discovery...");
    let discovery = run_discovery(&cli.api_url, &cli.frontend_url, &client).await?;

    if cli.discovery_only {
        print_discovery(&discovery);
        return Ok(());
    }

    let mut registry = endpoints::register_all();
    let total_registered = registry.entries.len();

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

    registry.sort_by_risk();

    eprintln!(
        "Running {} of {} endpoints ({} iterations, concurrency {})...\n",
        registry.entries.len(),
        total_registered,
        cli.iterations,
        cli.concurrency,
    );

    let run_config = runner::RunConfig {
        iterations: cli.iterations,
        concurrency: cli.concurrency,
        warmup: cli.warmup,
    };

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
            runner::bench_endpoint(&client, entry, &cli.api_url, &discovery.params, &run_config)
                .await?;

        if result.skipped {
            eprintln!(" SKIPPED");
        } else if result.metrics.error_rate > 0.0 {
            let mut statuses = std::collections::BTreeMap::new();
            for s in &result.samples {
                if s.error.is_some() {
                    *statuses.entry(s.status).or_insert(0u32) += 1;
                }
            }
            let codes: Vec<String> = statuses
                .iter()
                .map(|(code, count)| format!("{code}x{count}"))
                .collect();
            eprintln!(
                " p95={:.0}ms ERRORS({})",
                result.metrics.p95_ms,
                codes.join(",")
            );
        } else {
            eprintln!(" p95={:.0}ms", result.metrics.p95_ms);
        }

        results.push(result);
    }

    let total_time = bench_start.elapsed();
    eprintln!("\nBenchmark complete in {:.1}s", total_time.as_secs_f64());

    let bench_report = report::build_report(
        &results,
        &cli.api_url,
        cli.iterations,
        cli.concurrency,
        cli.warmup,
    );

    if cli.json {
        report::print_json(&bench_report)?;
    } else {
        report::print_table(&bench_report);
    }

    if let Some(ref dir) = cli.output_dir {
        let timestamp = bench_report.timestamp.replace(':', "-").replace('+', "p");
        let auto_path = dir.join(format!("{timestamp}.json"));
        report::save_json(&bench_report, &auto_path)?;
    }

    if let Some(ref path) = cli.output {
        report::save_json(&bench_report, path.as_ref())?;
    }

    if let Some(ref baseline_path) = cli.compare {
        report::compare_reports(&bench_report, baseline_path)?;
    }

    Ok(())
}
```

- [ ] **Step 4: Create stub stress module**

Create `crates/bench/src/stress/mod.rs` with a placeholder so the code compiles:

```rust
use anyhow::Result;
use clap::Parser;

mod collector;
mod report;
mod scenario;
mod vu;

#[derive(Parser, Debug)]
pub struct StressArgs {
    /// Placeholder — will be filled in Task 8
    #[arg(long, default_value = "http://localhost:8101/api/v1")]
    api_url: String,
}

pub async fn run_stress(_args: StressArgs) -> Result<()> {
    eprintln!("stress subcommand not yet implemented");
    Ok(())
}
```

Create stub files for submodules:

`crates/bench/src/stress/scenario.rs`:
```rust
// Scenario definitions — implemented in Task 3
```

`crates/bench/src/stress/collector.rs`:
```rust
// Sample collection — implemented in Task 4
```

`crates/bench/src/stress/vu.rs`:
```rust
// Virtual user loop — implemented in Task 5
```

`crates/bench/src/stress/report.rs`:
```rust
// Stress report output — implemented in Task 7
```

- [ ] **Step 5: Verify it compiles**

Run: `cargo check -p ckbadger-bench`
Expected: compiles with no errors

- [ ] **Step 6: Run existing tests to verify no regression**

Run: `cargo test -p ckbadger-bench`
Expected: all existing tests pass

- [ ] **Step 7: Commit**

```bash
git add crates/bench/src/main.rs crates/bench/src/runner.rs crates/bench/src/metrics.rs crates/bench/src/stress/
git commit -m "refactor(bench): add subcommand structure, expose execute_request and percentile"
```

---

### Task 2: Stress types and scenario definitions

Define the core types: `Scenario` enum, endpoint group structures, weight-based random selection. The scenario module builds endpoint groups from the existing registry.

**Files:**
- Create: `crates/bench/src/stress/scenario.rs`

- [ ] **Step 1: Write tests for scenario building**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::*;

    fn make_entry(module: &'static str, path: &'static str, risk: RiskTier, pattern: ReadPattern) -> EndpointEntry {
        EndpointEntry {
            module,
            method: Method::Get,
            path_template: path,
            description: "test",
            resolve: Box::new(|base, _p| Some(get(&format!("{base}{path}")))),
            expect_status: 200,
            risk_tier: risk,
            read_pattern: pattern,
        }
    }

    #[test]
    fn test_parse_scenario() {
        assert_eq!(Scenario::parse("mixed").unwrap(), vec![Scenario::Mixed]);
        assert_eq!(Scenario::parse("heavy").unwrap(), vec![Scenario::Heavy]);
        assert_eq!(
            Scenario::parse("mixed,heavy").unwrap(),
            vec![Scenario::Mixed, Scenario::Heavy]
        );
        assert!(Scenario::parse("bogus").is_err());
    }

    #[test]
    fn test_mixed_scenario_has_groups() {
        let entries = vec![
            make_entry("statistics", "/statistics/network", RiskTier::Low, ReadPattern::Cached),
            make_entry("blocks", "/blocks", RiskTier::Medium, ReadPattern::PrefixScan),
            make_entry("blocks", "/blocks/{id}", RiskTier::Low, ReadPattern::KeyLookup),
            make_entry("transactions", "/transactions/{hash}", RiskTier::Low, ReadPattern::KeyLookup),
        ];
        let groups = build_mixed_groups(&entries);
        assert!(!groups.is_empty());
        // Every endpoint should appear in at least one group
        let all_indices: Vec<usize> = groups.iter().flat_map(|g| &g.endpoint_indices).copied().collect();
        // At least some endpoints should be assigned
        assert!(!all_indices.is_empty());
    }

    #[test]
    fn test_heavy_scenario_filters_high_risk() {
        let entries = vec![
            make_entry("blocks", "/blocks/{id}", RiskTier::Low, ReadPattern::KeyLookup),
            make_entry("cells", "/cells/live", RiskTier::High, ReadPattern::FullCfScan),
            make_entry("tokens", "/tokens/{hash}/holders", RiskTier::High, ReadPattern::PrefixScan),
        ];
        let groups = build_heavy_groups(&entries);
        // Should only include High-risk endpoints
        let all_indices: Vec<usize> = groups.iter().flat_map(|g| &g.endpoint_indices).copied().collect();
        assert!(all_indices.contains(&1)); // cells
        assert!(all_indices.contains(&2)); // tokens
        assert!(!all_indices.contains(&0)); // blocks is Low risk
    }

    #[test]
    fn test_pick_endpoint_respects_weights() {
        let groups = vec![
            EndpointGroup {
                name: "a",
                weight: 100,
                endpoint_indices: vec![0],
            },
            EndpointGroup {
                name: "b",
                weight: 0,
                endpoint_indices: vec![1],
            },
        ];
        // With weight 0 on group b, should always pick from group a
        for _ in 0..100 {
            let idx = pick_endpoint(&groups);
            assert_eq!(idx, 0);
        }
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p ckbadger-bench stress::scenario`
Expected: FAIL — types not defined

- [ ] **Step 3: Implement scenario.rs**

```rust
use anyhow::{bail, Result};
use rand::Rng;

use crate::registry::{EndpointEntry, ReadPattern, RiskTier};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scenario {
    Mixed,
    Heavy,
}

impl Scenario {
    pub fn parse(s: &str) -> Result<Vec<Scenario>> {
        let mut out = Vec::new();
        for part in s.split(',') {
            match part.trim() {
                "mixed" => out.push(Scenario::Mixed),
                "heavy" => out.push(Scenario::Heavy),
                other => bail!("Unknown scenario: {other}. Use: mixed, heavy"),
            }
        }
        if out.is_empty() {
            bail!("No scenarios specified");
        }
        Ok(out)
    }
}

#[derive(Debug, Clone)]
pub struct EndpointGroup {
    pub name: &'static str,
    pub weight: u32,
    pub endpoint_indices: Vec<usize>,
}

/// Module-to-group mapping for mixed scenario.
fn mixed_group_for_module(module: &str) -> Option<(&'static str, u32)> {
    match module {
        "statistics" | "mempool" | "hardforks" => Some(("homepage", 25)),
        "blocks" => Some(("blocks", 20)),
        "transactions" => Some(("transactions", 20)),
        "activities" => Some(("addresses", 15)),
        "tokens" | "spore" | "assets" => Some(("assets", 10)),
        "search" | "dao" | "scripts" | "identities" | "fiber" | "forks" | "graph" => {
            Some(("other", 10))
        }
        "cells" => Some(("addresses", 15)),
        _ => None,
    }
}

/// Build endpoint groups for the Mixed scenario.
pub fn build_mixed_groups(entries: &[EndpointEntry]) -> Vec<EndpointGroup> {
    let group_defs: &[(&str, u32)] = &[
        ("homepage", 25),
        ("blocks", 20),
        ("transactions", 20),
        ("addresses", 15),
        ("assets", 10),
        ("other", 10),
    ];

    let mut groups: Vec<EndpointGroup> = group_defs
        .iter()
        .map(|(name, weight)| EndpointGroup {
            name,
            weight: *weight,
            endpoint_indices: Vec::new(),
        })
        .collect();

    for (i, entry) in entries.iter().enumerate() {
        if let Some((group_name, _)) = mixed_group_for_module(entry.module) {
            if let Some(group) = groups.iter_mut().find(|g| g.name == group_name) {
                group.endpoint_indices.push(i);
            }
        }
    }

    // Remove empty groups
    groups.retain(|g| !g.endpoint_indices.is_empty());
    groups
}

/// Weight multiplier for read patterns in heavy scenario.
fn heavy_pattern_weight(pattern: ReadPattern) -> u32 {
    match pattern {
        ReadPattern::FullCfScan => 4,
        ReadPattern::CrossStore => 4,
        ReadPattern::RangeScan => 3,
        ReadPattern::PrefixScan => 2,
        _ => 1,
    }
}

/// Build endpoint groups for the Heavy scenario.
/// One group per read pattern, only High-risk endpoints.
pub fn build_heavy_groups(entries: &[EndpointEntry]) -> Vec<EndpointGroup> {
    let patterns = [
        ("FullCfScan", ReadPattern::FullCfScan),
        ("CrossStore", ReadPattern::CrossStore),
        ("RangeScan", ReadPattern::RangeScan),
        ("PrefixScan", ReadPattern::PrefixScan),
        ("Other", ReadPattern::KeyLookup), // catch-all
    ];

    let mut groups: Vec<EndpointGroup> = patterns
        .iter()
        .map(|(name, pattern)| EndpointGroup {
            name,
            weight: heavy_pattern_weight(*pattern),
            endpoint_indices: Vec::new(),
        })
        .collect();

    for (i, entry) in entries.iter().enumerate() {
        if entry.risk_tier != RiskTier::High {
            continue;
        }
        let group_name = match entry.read_pattern {
            ReadPattern::FullCfScan => "FullCfScan",
            ReadPattern::CrossStore => "CrossStore",
            ReadPattern::RangeScan => "RangeScan",
            ReadPattern::PrefixScan => "PrefixScan",
            _ => "Other",
        };
        if let Some(group) = groups.iter_mut().find(|g| g.name == group_name) {
            group.endpoint_indices.push(i);
        }
    }

    groups.retain(|g| !g.endpoint_indices.is_empty());
    groups
}

/// Pick a random endpoint index from weighted groups.
pub fn pick_endpoint(groups: &[EndpointGroup]) -> usize {
    let total_weight: u32 = groups.iter().map(|g| g.weight).sum();
    if total_weight == 0 {
        // Fallback: pick uniformly from first group
        let group = &groups[0];
        let idx = rand::rng().random_range(0..group.endpoint_indices.len());
        return group.endpoint_indices[idx];
    }

    let mut rng = rand::rng();
    let roll = rng.random_range(0..total_weight);
    let mut cumulative = 0;
    for group in groups {
        cumulative += group.weight;
        if roll < cumulative {
            let idx = rng.random_range(0..group.endpoint_indices.len());
            return group.endpoint_indices[idx];
        }
    }

    // Fallback (should not reach)
    groups.last().unwrap().endpoint_indices[0]
}

#[cfg(test)]
mod tests {
    // ... (tests from Step 1)
}
```

- [ ] **Step 4: Add rand dependency**

In `crates/bench/Cargo.toml`, add:

```toml
rand = "0.9"
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p ckbadger-bench stress::scenario`
Expected: all pass

- [ ] **Step 6: Commit**

```bash
git add crates/bench/Cargo.toml crates/bench/src/stress/scenario.rs
git commit -m "feat(bench): add stress scenario definitions with weighted endpoint groups"
```

---

### Task 3: Collector — sample aggregation and degradation detection

The collector receives samples from VUs via an mpsc channel, aggregates them per stage, and detects degradation signals. It also drives the real-time status line.

**Files:**
- Create: `crates/bench/src/stress/collector.rs`

- [ ] **Step 1: Write tests for StageResult aggregation and degradation detection**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn sample(latency_ms: f64, status: u16, error: Option<String>) -> StressSample {
        StressSample {
            endpoint_idx: 0,
            endpoint_path: "/test".to_string(),
            read_pattern: "KeyLookup".to_string(),
            latency_ms,
            status,
            body_size: 100,
            error,
        }
    }

    #[test]
    fn test_stage_result_from_samples() {
        let samples = vec![
            sample(10.0, 200, None),
            sample(20.0, 200, None),
            sample(30.0, 200, None),
            sample(40.0, 200, None),
            sample(50.0, 200, None),
        ];
        let result = StageResult::from_samples(1, 10, Duration::from_secs(30), &samples);
        assert_eq!(result.stage_id, 1);
        assert_eq!(result.vus, 10);
        assert_eq!(result.total_requests, 5);
        assert_eq!(result.error_rate, 0.0);
        assert!(result.p50_ms >= 10.0 && result.p50_ms <= 50.0);
        assert!(result.rps > 0.0);
    }

    #[test]
    fn test_stage_result_with_errors() {
        let samples = vec![
            sample(10.0, 200, None),
            sample(20.0, 500, Some("server error".into())),
            sample(30.0, 0, Some("connection refused".into())),
        ];
        let result = StageResult::from_samples(1, 10, Duration::from_secs(30), &samples);
        assert!((result.error_rate - 2.0 / 3.0).abs() < 0.01);
        assert_eq!(result.connection_refused, 1);
    }

    #[test]
    fn test_detect_degradation_baseline() {
        let baseline = StageResult::from_samples(
            0,
            10,
            Duration::from_secs(30),
            &vec![sample(10.0, 200, None); 100],
        );
        // Same performance — no degradation
        let current = StageResult::from_samples(
            1,
            20,
            Duration::from_secs(30),
            &vec![sample(12.0, 200, None); 100],
        );
        assert_eq!(detect_degradation(&baseline, &current), DegradationSignal::None);
    }

    #[test]
    fn test_detect_soft_degradation() {
        let baseline = StageResult::from_samples(
            0,
            10,
            Duration::from_secs(30),
            &vec![sample(10.0, 200, None); 100],
        );
        // p95 jumped way above 2x baseline
        let current = StageResult::from_samples(
            1,
            100,
            Duration::from_secs(30),
            &vec![sample(50.0, 200, None); 100],
        );
        assert_eq!(detect_degradation(&baseline, &current), DegradationSignal::SoftDegradation);
    }

    #[test]
    fn test_detect_hard_failure() {
        let baseline = StageResult::from_samples(
            0,
            10,
            Duration::from_secs(30),
            &vec![sample(10.0, 200, None); 100],
        );
        // >10% error rate
        let mut error_samples: Vec<StressSample> = vec![sample(10.0, 200, None); 85];
        error_samples.extend(vec![sample(100.0, 500, Some("error".into())); 15]);
        let current = StageResult::from_samples(1, 200, Duration::from_secs(30), &error_samples);
        assert_eq!(detect_degradation(&baseline, &current), DegradationSignal::HardFailure);
    }

    #[test]
    fn test_per_endpoint_metrics() {
        let samples = vec![
            StressSample {
                endpoint_idx: 0,
                endpoint_path: "/blocks".to_string(),
                read_pattern: "PrefixScan".to_string(),
                latency_ms: 10.0,
                status: 200,
                body_size: 100,
                error: None,
            },
            StressSample {
                endpoint_idx: 0,
                endpoint_path: "/blocks".to_string(),
                read_pattern: "PrefixScan".to_string(),
                latency_ms: 20.0,
                status: 200,
                body_size: 100,
                error: None,
            },
            StressSample {
                endpoint_idx: 1,
                endpoint_path: "/transactions/{hash}".to_string(),
                read_pattern: "KeyLookup".to_string(),
                latency_ms: 5.0,
                status: 200,
                body_size: 50,
                error: None,
            },
        ];
        let result = StageResult::from_samples(0, 10, Duration::from_secs(30), &samples);
        assert_eq!(result.per_endpoint.len(), 2);
        let blocks = result.per_endpoint.get(&0).unwrap();
        assert_eq!(blocks.count, 2);
        let txs = result.per_endpoint.get(&1).unwrap();
        assert_eq!(txs.count, 1);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p ckbadger-bench stress::collector`
Expected: FAIL — types not defined

- [ ] **Step 3: Implement collector.rs**

```rust
use std::collections::HashMap;
use std::time::Duration;

use tokio::sync::mpsc;

use crate::metrics::percentile;

// ---------------------------------------------------------------------------
// Sample (sent from VU to collector)
// ---------------------------------------------------------------------------

pub struct StressSample {
    pub endpoint_idx: usize,
    pub endpoint_path: String,
    pub read_pattern: String,
    pub latency_ms: f64,
    pub status: u16,
    pub body_size: usize,
    pub error: Option<String>,
}

// ---------------------------------------------------------------------------
// Per-endpoint metrics within a stage
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct EndpointStageMetrics {
    pub endpoint_path: String,
    pub read_pattern: String,
    pub count: u64,
    pub p50_ms: f64,
    pub p95_ms: f64,
    pub p99_ms: f64,
    pub error_rate: f64,
}

// ---------------------------------------------------------------------------
// Stage result (aggregated from all samples in one stage)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StageStatus {
    Baseline,
    Ok,
    SoftDegradation,
    ErrorsRising,
    HardFailure,
}

#[derive(Debug, Clone)]
pub struct StageResult {
    pub stage_id: u16,
    pub vus: u32,
    pub duration: Duration,
    pub total_requests: u64,
    pub rps: f64,
    pub p50_ms: f64,
    pub p95_ms: f64,
    pub p99_ms: f64,
    pub error_rate: f64,
    pub error_count: u64,
    pub connection_refused: u64,
    pub timeouts: u64,
    pub per_endpoint: HashMap<usize, EndpointStageMetrics>,
    pub status: StageStatus,
}

impl StageResult {
    pub fn from_samples(
        stage_id: u16,
        vus: u32,
        duration: Duration,
        samples: &[StressSample],
    ) -> Self {
        let total = samples.len() as u64;
        let wall_secs = duration.as_secs_f64();
        let rps = if wall_secs > 0.0 {
            total as f64 / wall_secs
        } else {
            0.0
        };

        let mut latencies: Vec<f64> = samples.iter().map(|s| s.latency_ms).collect();
        latencies.sort_by(|a, b| a.partial_cmp(b).unwrap());

        let errors = samples.iter().filter(|s| s.error.is_some()).count() as u64;
        let conn_refused = samples
            .iter()
            .filter(|s| {
                s.error
                    .as_ref()
                    .is_some_and(|e| e.contains("onnection refused") || s.status == 0)
            })
            .count() as u64;
        let timeouts = samples
            .iter()
            .filter(|s| {
                s.error
                    .as_ref()
                    .is_some_and(|e| e.contains("timed out") || e.contains("timeout"))
            })
            .count() as u64;

        // Per-endpoint aggregation
        let mut endpoint_samples: HashMap<usize, Vec<&StressSample>> = HashMap::new();
        for s in samples {
            endpoint_samples.entry(s.endpoint_idx).or_default().push(s);
        }
        let mut per_endpoint = HashMap::new();
        for (idx, ep_samples) in &endpoint_samples {
            let mut ep_latencies: Vec<f64> = ep_samples.iter().map(|s| s.latency_ms).collect();
            ep_latencies.sort_by(|a, b| a.partial_cmp(b).unwrap());
            let ep_errors = ep_samples.iter().filter(|s| s.error.is_some()).count();
            let first = ep_samples[0];
            per_endpoint.insert(
                *idx,
                EndpointStageMetrics {
                    endpoint_path: first.endpoint_path.clone(),
                    read_pattern: first.read_pattern.clone(),
                    count: ep_samples.len() as u64,
                    p50_ms: percentile(&ep_latencies, 50.0),
                    p95_ms: percentile(&ep_latencies, 95.0),
                    p99_ms: percentile(&ep_latencies, 99.0),
                    error_rate: ep_errors as f64 / ep_samples.len() as f64,
                },
            );
        }

        StageResult {
            stage_id,
            vus,
            duration,
            total_requests: total,
            rps,
            p50_ms: percentile(&latencies, 50.0),
            p95_ms: percentile(&latencies, 95.0),
            p99_ms: percentile(&latencies, 99.0),
            error_rate: if total > 0 {
                errors as f64 / total as f64
            } else {
                0.0
            },
            error_count: errors,
            connection_refused: conn_refused,
            timeouts,
            per_endpoint,
            status: StageStatus::Ok, // Set by caller after degradation detection
        }
    }
}

// ---------------------------------------------------------------------------
// Degradation detection
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DegradationSignal {
    None,
    SoftDegradation,
    ErrorsEmerging,
    HardFailure,
}

const SOFT_DEGRADATION_FACTOR: f64 = 2.0;
const ERROR_EMERGING_THRESHOLD: f64 = 0.01;
const HARD_FAILURE_ERROR_THRESHOLD: f64 = 0.10;

pub fn detect_degradation(baseline: &StageResult, current: &StageResult) -> DegradationSignal {
    // Hard failure: error rate > 10%
    if current.error_rate > HARD_FAILURE_ERROR_THRESHOLD {
        return DegradationSignal::HardFailure;
    }

    // Errors emerging: error rate > 1%
    if current.error_rate > ERROR_EMERGING_THRESHOLD {
        return DegradationSignal::ErrorsEmerging;
    }

    // Soft degradation: p95 > 2× baseline p95
    if baseline.p95_ms > 0.0 && current.p95_ms > baseline.p95_ms * SOFT_DEGRADATION_FACTOR {
        return DegradationSignal::SoftDegradation;
    }

    DegradationSignal::None
}

// ---------------------------------------------------------------------------
// Status line
// ---------------------------------------------------------------------------

pub struct StatusLine {
    pub stage_id: u16,
    pub total_stages: u16,
    pub vus: u32,
    pub elapsed_secs: u64,
    pub stage_duration_secs: u64,
    pub rps: f64,
    pub p95_ms: f64,
    pub error_rate: f64,
}

impl StatusLine {
    pub fn print(&self) {
        let filled = if self.stage_duration_secs > 0 {
            (self.elapsed_secs * 10 / self.stage_duration_secs).min(10)
        } else {
            0
        };
        let bar: String = (0..10)
            .map(|i| if i < filled { '▓' } else { '░' })
            .collect();
        eprint!(
            "\r[stage {}/{} · {} VUs · {}s] rps={:.0}  p95={:.0}ms  err={:.1}%  {}  ",
            self.stage_id,
            self.total_stages,
            self.vus,
            self.elapsed_secs,
            self.rps,
            self.p95_ms,
            self.error_rate * 100.0,
            bar,
        );
    }
}

// ---------------------------------------------------------------------------
// Collector channel types
// ---------------------------------------------------------------------------

pub type SampleSender = mpsc::UnboundedSender<StressSample>;
pub type SampleReceiver = mpsc::UnboundedReceiver<StressSample>;

pub fn sample_channel() -> (SampleSender, SampleReceiver) {
    mpsc::unbounded_channel()
}

/// Drain all currently buffered samples from the receiver (non-blocking).
pub fn drain_samples(rx: &mut SampleReceiver) -> Vec<StressSample> {
    let mut samples = Vec::new();
    while let Ok(s) = rx.try_recv() {
        samples.push(s);
    }
    samples
}

#[cfg(test)]
mod tests {
    // ... (tests from Step 1)
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p ckbadger-bench stress::collector`
Expected: all pass

- [ ] **Step 5: Commit**

```bash
git add crates/bench/src/stress/collector.rs
git commit -m "feat(bench): add stress collector with sample aggregation and degradation detection"
```

---

### Task 4: Virtual User loop

Each VU is a tokio task that loops: pick a random endpoint from weighted groups, resolve and execute the request, send the sample to the collector, optional think time.

**Files:**
- Create: `crates/bench/src/stress/vu.rs`

- [ ] **Step 1: Write test for VU request construction**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::*;

    #[test]
    fn test_resolve_endpoint_returns_none_for_missing_params() {
        let entry = EndpointEntry {
            module: "test",
            method: Method::Get,
            path_template: "/test/{id}",
            description: "test",
            resolve: Box::new(|_base, _p| None), // unresolvable
            expect_status: 200,
            risk_tier: RiskTier::Low,
            read_pattern: ReadPattern::KeyLookup,
        };
        let params = DiscoveredParams::default();
        assert!(resolve_for_stress(&entry, "http://localhost:8101/api/v1", &params).is_none());
    }

    #[test]
    fn test_resolve_endpoint_returns_request() {
        let entry = EndpointEntry {
            module: "blocks",
            method: Method::Get,
            path_template: "/blocks",
            description: "list blocks",
            resolve: Box::new(|base, _p| Some(get(&format!("{base}/blocks")))),
            expect_status: 200,
            risk_tier: RiskTier::Low,
            read_pattern: ReadPattern::PrefixScan,
        };
        let params = DiscoveredParams::default();
        let resolved = resolve_for_stress(&entry, "http://localhost:8101/api/v1", &params);
        assert!(resolved.is_some());
        assert!(resolved.unwrap().url.contains("/blocks"));
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p ckbadger-bench stress::vu`
Expected: FAIL

- [ ] **Step 3: Implement vu.rs**

```rust
use std::sync::Arc;
use std::time::Duration;

use rand::Rng;
use tokio_util::sync::CancellationToken;

use crate::registry::{DiscoveredParams, EndpointEntry, ResolvedRequest};
use crate::runner::execute_request;

use super::collector::{SampleSender, StressSample};
use super::scenario::{pick_endpoint, EndpointGroup};

/// Resolved endpoint ready for stress testing.
pub struct ResolvedEndpoint {
    pub idx: usize,
    pub path_template: String,
    pub read_pattern: String,
    pub resolved: ResolvedRequest,
    pub expect_status: u16,
}

/// Attempt to resolve an endpoint entry for stress testing.
pub fn resolve_for_stress(
    entry: &EndpointEntry,
    api_base: &str,
    params: &DiscoveredParams,
) -> Option<ResolvedRequest> {
    (entry.resolve)(api_base, params)
}

/// Pre-resolve all endpoints, returning only those that resolve successfully.
pub fn resolve_all(
    entries: &[EndpointEntry],
    api_base: &str,
    params: &DiscoveredParams,
) -> Vec<ResolvedEndpoint> {
    entries
        .iter()
        .enumerate()
        .filter_map(|(i, entry)| {
            let resolved = resolve_for_stress(entry, api_base, params)?;
            Some(ResolvedEndpoint {
                idx: i,
                path_template: entry.path_template.to_string(),
                read_pattern: entry.read_pattern.to_string(),
                resolved,
                expect_status: entry.expect_status,
            })
        })
        .collect()
}

/// Spawn a single VU task that loops sending requests until cancelled.
pub fn spawn_vu(
    client: Arc<reqwest::Client>,
    resolved_endpoints: Arc<Vec<ResolvedEndpoint>>,
    groups: Arc<Vec<EndpointGroup>>,
    tx: SampleSender,
    think_time: Option<(u64, u64)>,
    cancel: CancellationToken,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut rng = rand::rng();
        loop {
            if cancel.is_cancelled() {
                break;
            }

            // Pick a random endpoint via weighted groups
            let target_idx = pick_endpoint(&groups);

            // Find the resolved endpoint matching this index
            let ep = match resolved_endpoints.iter().find(|e| e.idx == target_idx) {
                Some(ep) => ep,
                None => continue, // endpoint couldn't be resolved, skip
            };

            let sample = execute_request(&client, &ep.resolved, ep.expect_status).await;

            let stress_sample = StressSample {
                endpoint_idx: ep.idx,
                endpoint_path: ep.path_template.clone(),
                read_pattern: ep.read_pattern.clone(),
                latency_ms: sample.latency_ms,
                status: sample.status,
                body_size: sample.body_size,
                error: sample.error,
            };

            // If send fails, the collector has been dropped — exit
            if tx.send(stress_sample).is_err() {
                break;
            }

            // Think time
            if let Some((min_ms, max_ms)) = think_time {
                let delay = rng.random_range(min_ms..=max_ms);
                tokio::select! {
                    _ = tokio::time::sleep(Duration::from_millis(delay)) => {}
                    _ = cancel.cancelled() => break,
                }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    // ... (tests from Step 1)
}
```

- [ ] **Step 4: Add tokio-util dependency for CancellationToken**

In `crates/bench/Cargo.toml`, add:

```toml
tokio-util = "0.7"
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p ckbadger-bench stress::vu`
Expected: all pass

- [ ] **Step 6: Commit**

```bash
git add crates/bench/Cargo.toml crates/bench/src/stress/vu.rs
git commit -m "feat(bench): add VU loop with weighted endpoint selection and think time"
```

---

### Task 5: Stage scheduler

The stage scheduler is the orchestrator: it runs discovery, builds scenarios, then for each stage spawns VUs, collects samples, computes metrics, detects degradation, and decides whether to continue. This is the main `run_stress()` function.

**Files:**
- Modify: `crates/bench/src/stress/mod.rs`

- [ ] **Step 1: Write test for stage list parsing**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_stages_default() {
        let stages = parse_stages("10,25,50,100,200,300");
        assert_eq!(stages, vec![10, 25, 50, 100, 200, 300]);
    }

    #[test]
    fn test_parse_stages_custom() {
        let stages = parse_stages("5,10,20");
        assert_eq!(stages, vec![5, 10, 20]);
    }

    #[test]
    fn test_auto_ramp_sequence() {
        let seq = auto_ramp_sequence(10);
        // 10, 20, 40, 80, 160, 320, ...
        assert_eq!(seq[0], 10);
        assert_eq!(seq[1], 20);
        assert_eq!(seq[2], 40);
        assert!(seq.len() >= 6);
    }

    #[test]
    fn test_resolve_target_local() {
        let (api, frontend) = resolve_target(None, "http://localhost:8101/api/v1", "http://localhost:8100");
        assert_eq!(api, "http://localhost:8101/api/v1");
        assert_eq!(frontend, "http://localhost:8100");
    }

    #[test]
    fn test_resolve_target_remote() {
        let (api, frontend) = resolve_target(Some("192.168.1.100"), "", "");
        assert_eq!(api, "http://192.168.1.100:8101/api/v1");
        assert_eq!(frontend, "http://192.168.1.100:8100");
    }

    #[test]
    fn test_target_label_local() {
        assert_eq!(target_label("http://localhost:8101/api/v1"), "local");
    }

    #[test]
    fn test_target_label_remote() {
        assert_eq!(
            target_label("http://192.168.1.100:8101/api/v1"),
            "remote (192.168.1.100:8101)"
        );
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p ckbadger-bench stress::tests`
Expected: FAIL

- [ ] **Step 3: Implement stress/mod.rs**

```rust
pub mod collector;
pub mod report;
pub mod scenario;
pub mod vu;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use clap::Parser;
use tokio_util::sync::CancellationToken;

use crate::discovery::{check_connectivity, run_discovery};
use crate::endpoints;

use self::collector::{
    detect_degradation, drain_samples, sample_channel, DegradationSignal, StageResult,
    StageStatus, StatusLine,
};
use self::scenario::Scenario;
use self::vu::{resolve_all, spawn_vu};

// ---------------------------------------------------------------------------
// CLI args
// ---------------------------------------------------------------------------

#[derive(Parser, Debug)]
pub struct StressArgs {
    /// Scenario: mixed, heavy, or mixed,heavy
    #[arg(long, default_value = "mixed")]
    pub scenario: String,

    /// Comma-separated VU counts per stage
    #[arg(long, default_value = "10,25,50,100,200,300")]
    pub stages: String,

    /// Duration per stage in seconds
    #[arg(long, default_value = "30")]
    pub stage_duration: u64,

    /// Auto-increase VUs until hard failure (ignores --stages)
    #[arg(long)]
    pub auto_ramp: bool,

    /// Think time range in ms for mixed scenario (e.g. "50-200")
    #[arg(long, default_value = "50-200")]
    pub think_time_ms: String,

    /// Remote ckbadger host (auto-expands to api-url + frontend-url)
    #[arg(long)]
    pub remote_host: Option<String>,

    /// API base URL
    #[arg(long, default_value = "http://localhost:8101/api/v1")]
    pub api_url: String,

    /// Frontend base URL
    #[arg(long, default_value = "http://localhost:8100")]
    pub frontend_url: String,

    /// Per-request timeout in milliseconds
    #[arg(long, default_value = "10000")]
    pub timeout_ms: u64,

    /// Warmup duration before stage 1 in seconds
    #[arg(long, default_value = "5")]
    pub warmup_duration: u64,

    /// Output JSON to stdout
    #[arg(long)]
    pub json: bool,

    /// Auto-save timestamped reports to directory
    #[arg(long)]
    pub output_dir: Option<PathBuf>,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

pub fn parse_stages(s: &str) -> Vec<u32> {
    s.split(',')
        .filter_map(|p| p.trim().parse().ok())
        .collect()
}

pub fn auto_ramp_sequence(start: u32) -> Vec<u32> {
    let mut seq = Vec::new();
    let mut n = start;
    // Generate up to 20 stages (way more than needed, scheduler stops at hard failure)
    for _ in 0..20 {
        seq.push(n);
        n *= 2;
        if n > 10_000 {
            break;
        }
    }
    seq
}

fn parse_think_time(s: &str) -> Result<(u64, u64)> {
    let parts: Vec<&str> = s.split('-').collect();
    if parts.len() != 2 {
        bail!("Invalid think-time-ms format: {s}. Expected: min-max (e.g. 50-200)");
    }
    let min: u64 = parts[0].parse().context("invalid think-time min")?;
    let max: u64 = parts[1].parse().context("invalid think-time max")?;
    Ok((min, max))
}

pub fn resolve_target(
    remote_host: Option<&str>,
    api_url: &str,
    frontend_url: &str,
) -> (String, String) {
    match remote_host {
        Some(host) => (
            format!("http://{host}:8101/api/v1"),
            format!("http://{host}:8100"),
        ),
        None => (api_url.to_string(), frontend_url.to_string()),
    }
}

pub fn target_label(api_url: &str) -> String {
    if api_url.contains("localhost") || api_url.contains("127.0.0.1") {
        "local".to_string()
    } else {
        // Extract host:port
        let host = api_url
            .trim_start_matches("http://")
            .trim_start_matches("https://")
            .split('/')
            .next()
            .unwrap_or(api_url);
        format!("remote ({host})")
    }
}

// ---------------------------------------------------------------------------
// Main stress runner
// ---------------------------------------------------------------------------

pub async fn run_stress(args: StressArgs) -> Result<()> {
    // Validate mutual exclusion
    if args.remote_host.is_some()
        && (args.api_url != "http://localhost:8101/api/v1"
            || args.frontend_url != "http://localhost:8100")
    {
        bail!("--remote-host cannot be used with --api-url or --frontend-url");
    }

    let (api_url, frontend_url) =
        resolve_target(args.remote_host.as_deref(), &args.api_url, &args.frontend_url);

    let scenarios = Scenario::parse(&args.scenario)?;
    let stage_vus = if args.auto_ramp {
        auto_ramp_sequence(10)
    } else {
        parse_stages(&args.stages)
    };
    let stage_duration = Duration::from_secs(args.stage_duration);
    let think_time = parse_think_time(&args.think_time_ms)?;
    let target = target_label(&api_url);

    eprintln!("ckbadger-bench stress");
    eprintln!("  target:    {target}");
    eprintln!("  scenarios: {}", args.scenario);
    eprintln!(
        "  stages:    {} ({})",
        if args.auto_ramp {
            "auto-ramp".to_string()
        } else {
            stage_vus
                .iter()
                .map(|v| v.to_string())
                .collect::<Vec<_>>()
                .join(",")
        },
        format!("{}s each", args.stage_duration)
    );
    eprintln!();

    // Build HTTP client
    let client = Arc::new(
        reqwest::Client::builder()
            .timeout(Duration::from_millis(args.timeout_ms))
            .pool_max_idle_per_host(500)
            .build()
            .context("failed to build HTTP client")?,
    );

    // Connectivity + discovery
    eprintln!("Checking API connectivity...");
    check_connectivity(&api_url, &client).await?;
    eprintln!("Running discovery...");
    let discovery = run_discovery(&api_url, &frontend_url, &client).await?;

    // Build registry and resolve all endpoints
    let registry = endpoints::register_all();
    let resolved_endpoints = Arc::new(resolve_all(
        &registry.entries,
        &api_url,
        &discovery.params,
    ));

    let resolvable = resolved_endpoints.len();
    let total = registry.entries.len();
    eprintln!("Resolved {resolvable}/{total} endpoints\n");

    if resolvable == 0 {
        bail!("No endpoints could be resolved — cannot run stress test");
    }

    // Run each scenario
    let mut all_scenario_results = Vec::new();

    for scenario in &scenarios {
        eprintln!(
            "=== Scenario: {} ===\n",
            match scenario {
                Scenario::Mixed => "mixed",
                Scenario::Heavy => "heavy",
            }
        );

        let groups = match scenario {
            Scenario::Mixed => {
                scenario::build_mixed_groups(&registry.entries)
            }
            Scenario::Heavy => {
                scenario::build_heavy_groups(&registry.entries)
            }
        };

        if groups.is_empty() {
            eprintln!("No endpoint groups for this scenario, skipping.\n");
            continue;
        }

        let groups = Arc::new(groups);
        let scenario_think_time = match scenario {
            Scenario::Mixed => Some(think_time),
            Scenario::Heavy => None,
        };

        // Warmup: run a few requests to prime caches
        if args.warmup_duration > 0 {
            eprint!("Warming up...");
            let warmup_cancel = CancellationToken::new();
            let (warmup_tx, mut warmup_rx) = sample_channel();

            let handle = spawn_vu(
                Arc::clone(&client),
                Arc::clone(&resolved_endpoints),
                Arc::clone(&groups),
                warmup_tx,
                None,
                warmup_cancel.clone(),
            );

            tokio::time::sleep(Duration::from_secs(args.warmup_duration)).await;
            warmup_cancel.cancel();
            let _ = handle.await;
            let warmup_count = drain_samples(&mut warmup_rx).len();
            eprintln!(" done ({warmup_count} requests)\n");
        }

        // Run stages
        let mut stage_results: Vec<StageResult> = Vec::new();
        let mut baseline: Option<StageResult> = None;
        let mut soft_degradation_vus: Option<u32> = None;
        let mut breaking_point_vus: Option<u32> = None;

        for (stage_idx, &vus) in stage_vus.iter().enumerate() {
            let stage_id = (stage_idx + 1) as u16;
            let total_stages = if args.auto_ramp { 0 } else { stage_vus.len() as u16 };

            let cancel = CancellationToken::new();
            let (tx, mut rx) = sample_channel();

            // Spawn VUs
            let mut handles = Vec::new();
            for _ in 0..vus {
                handles.push(spawn_vu(
                    Arc::clone(&client),
                    Arc::clone(&resolved_endpoints),
                    Arc::clone(&groups),
                    tx.clone(),
                    scenario_think_time,
                    cancel.clone(),
                ));
            }
            // Drop our copy of tx so receiver closes when all VUs finish
            drop(tx);

            // Collect samples for stage_duration, printing status line each second
            let stage_start = Instant::now();
            let mut all_samples = Vec::new();

            loop {
                let elapsed = stage_start.elapsed();
                if elapsed >= stage_duration {
                    break;
                }

                let remaining = stage_duration - elapsed;
                let sleep_dur = remaining.min(Duration::from_secs(1));
                tokio::time::sleep(sleep_dur).await;

                let batch = drain_samples(&mut rx);
                all_samples.extend(batch);

                // Print real-time status
                let elapsed_secs = stage_start.elapsed().as_secs();
                let window_rps = if elapsed_secs > 0 {
                    all_samples.len() as f64 / elapsed_secs as f64
                } else {
                    0.0
                };
                let mut window_latencies: Vec<f64> =
                    all_samples.iter().map(|s| s.latency_ms).collect();
                window_latencies.sort_by(|a, b| a.partial_cmp(b).unwrap());
                let window_p95 = crate::metrics::percentile(&window_latencies, 95.0);
                let window_errors = all_samples.iter().filter(|s| s.error.is_some()).count();
                let window_err_rate = if all_samples.is_empty() {
                    0.0
                } else {
                    window_errors as f64 / all_samples.len() as f64
                };

                StatusLine {
                    stage_id,
                    total_stages,
                    vus,
                    elapsed_secs,
                    stage_duration_secs: args.stage_duration,
                    rps: window_rps,
                    p95_ms: window_p95,
                    error_rate: window_err_rate,
                }
                .print();
            }

            // Stop VUs
            cancel.cancel();
            for handle in handles {
                let _ = handle.await;
            }

            // Drain remaining samples
            let remaining = drain_samples(&mut rx);
            all_samples.extend(remaining);

            eprintln!(); // clear status line

            // Compute stage metrics
            let mut result =
                StageResult::from_samples(stage_id, vus, stage_duration, &all_samples);

            // Determine stage status
            if baseline.is_none() {
                result.status = StageStatus::Baseline;
                baseline = Some(result.clone());
            } else if let Some(ref bl) = baseline {
                let signal = detect_degradation(bl, &result);
                result.status = match signal {
                    DegradationSignal::None => StageStatus::Ok,
                    DegradationSignal::SoftDegradation => {
                        if soft_degradation_vus.is_none() {
                            soft_degradation_vus = Some(vus);
                        }
                        StageStatus::SoftDegradation
                    }
                    DegradationSignal::ErrorsEmerging => {
                        if soft_degradation_vus.is_none() {
                            soft_degradation_vus = Some(vus);
                        }
                        StageStatus::ErrorsRising
                    }
                    DegradationSignal::HardFailure => {
                        breaking_point_vus = Some(vus);
                        StageStatus::HardFailure
                    }
                };
            }

            eprintln!(
                "  Stage {} complete: {} VUs, {:.0} rps, p95={:.0}ms, err={:.1}%, status={:?}",
                stage_id, vus, result.rps, result.p95_ms, result.error_rate * 100.0, result.status,
            );

            let is_hard_failure = result.status == StageStatus::HardFailure;
            stage_results.push(result);

            // Auto-ramp: stop on hard failure
            if args.auto_ramp && is_hard_failure {
                eprintln!("\nHard failure detected at {} VUs — stopping.", vus);
                break;
            }
        }

        all_scenario_results.push(report::ScenarioReport {
            scenario: *scenario,
            stage_results,
            soft_degradation_vus,
            breaking_point_vus,
        });
    }

    // Output report
    let stress_report = report::StressReport {
        timestamp: chrono::Utc::now().to_rfc3339(),
        target,
        config: report::StressConfig {
            scenarios: args.scenario.clone(),
            stage_duration_secs: args.stage_duration,
            auto_ramp: args.auto_ramp,
            think_time_ms: args.think_time_ms.clone(),
            timeout_ms: args.timeout_ms,
        },
        scenarios: all_scenario_results,
    };

    if args.json {
        report::print_json(&stress_report)?;
    } else {
        report::print_tables(&stress_report);
    }

    if let Some(ref dir) = args.output_dir {
        let timestamp = stress_report.timestamp.replace(':', "-").replace('+', "p");
        let auto_path = dir.join(format!("stress-{timestamp}.json"));
        report::save_json(&stress_report, &auto_path)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    // ... (tests from Step 1)
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p ckbadger-bench stress::tests`
Expected: all pass

- [ ] **Step 5: Verify it compiles**

Run: `cargo check -p ckbadger-bench`
Expected: compiles (report module is next — may need stub)

If needed, add a temporary stub to `crates/bench/src/stress/report.rs`:

```rust
use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use serde::Serialize;

use super::collector::StageResult;
use super::scenario::Scenario;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StressReport {
    pub timestamp: String,
    pub target: String,
    pub config: StressConfig,
    pub scenarios: Vec<ScenarioReport>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StressConfig {
    pub scenarios: String,
    pub stage_duration_secs: u64,
    pub auto_ramp: bool,
    pub think_time_ms: String,
    pub timeout_ms: u64,
}

#[derive(Debug, Clone)]
pub struct ScenarioReport {
    pub scenario: Scenario,
    pub stage_results: Vec<StageResult>,
    pub soft_degradation_vus: Option<u32>,
    pub breaking_point_vus: Option<u32>,
}

pub fn print_tables(_report: &StressReport) {
    eprintln!("(report tables not yet implemented)");
}

pub fn print_json(_report: &StressReport) -> Result<()> {
    eprintln!("(JSON report not yet implemented)");
    Ok(())
}

pub fn save_json(_report: &StressReport, _path: &Path) -> Result<()> {
    Ok(())
}
```

- [ ] **Step 6: Commit**

```bash
git add crates/bench/src/stress/mod.rs crates/bench/src/stress/report.rs
git commit -m "feat(bench): add stress stage scheduler with auto-ramp and VU orchestration"
```

---

### Task 6: Stress report output

Implement the three report tables: stage summary, endpoint breakdown, read pattern summary. Both human-readable (stderr) and JSON output.

**Files:**
- Modify: `crates/bench/src/stress/report.rs`

- [ ] **Step 1: Write tests for report formatting**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::stress::collector::{
        EndpointStageMetrics, StageResult, StageStatus, StressSample,
    };
    use std::collections::HashMap;
    use std::time::Duration;

    fn make_stage(stage_id: u16, vus: u32, p95: f64, err_rate: f64, status: StageStatus) -> StageResult {
        StageResult {
            stage_id,
            vus,
            duration: Duration::from_secs(30),
            total_requests: 1000,
            rps: 1000.0 / 30.0,
            p50_ms: p95 * 0.6,
            p95_ms: p95,
            p99_ms: p95 * 1.2,
            error_rate: err_rate,
            error_count: (1000.0 * err_rate) as u64,
            connection_refused: 0,
            timeouts: 0,
            per_endpoint: HashMap::new(),
            status,
        }
    }

    #[test]
    fn test_stage_status_label() {
        assert_eq!(stage_status_label(StageStatus::Baseline), "baseline");
        assert_eq!(stage_status_label(StageStatus::Ok), "ok");
        assert!(stage_status_label(StageStatus::SoftDegradation).contains("soft"));
        assert!(stage_status_label(StageStatus::HardFailure).contains("breaking"));
    }

    #[test]
    fn test_build_read_pattern_summary() {
        let baseline = StageResult {
            stage_id: 1,
            vus: 10,
            duration: Duration::from_secs(30),
            total_requests: 100,
            rps: 3.3,
            p50_ms: 5.0,
            p95_ms: 10.0,
            p99_ms: 15.0,
            error_rate: 0.0,
            error_count: 0,
            connection_refused: 0,
            timeouts: 0,
            per_endpoint: {
                let mut m = HashMap::new();
                m.insert(0, EndpointStageMetrics {
                    endpoint_path: "/blocks".to_string(),
                    read_pattern: "PrefixScan".to_string(),
                    count: 50,
                    p50_ms: 5.0,
                    p95_ms: 10.0,
                    p99_ms: 15.0,
                    error_rate: 0.0,
                });
                m.insert(1, EndpointStageMetrics {
                    endpoint_path: "/stats".to_string(),
                    read_pattern: "Cached".to_string(),
                    count: 50,
                    p50_ms: 2.0,
                    p95_ms: 5.0,
                    p99_ms: 8.0,
                    error_rate: 0.0,
                });
                m
            },
            status: StageStatus::Baseline,
        };

        let breaking = StageResult {
            per_endpoint: {
                let mut m = HashMap::new();
                m.insert(0, EndpointStageMetrics {
                    endpoint_path: "/blocks".to_string(),
                    read_pattern: "PrefixScan".to_string(),
                    count: 50,
                    p50_ms: 50.0,
                    p95_ms: 200.0,
                    p99_ms: 350.0,
                    error_rate: 0.1,
                });
                m.insert(1, EndpointStageMetrics {
                    endpoint_path: "/stats".to_string(),
                    read_pattern: "Cached".to_string(),
                    count: 50,
                    p50_ms: 5.0,
                    p95_ms: 12.0,
                    p99_ms: 18.0,
                    error_rate: 0.0,
                });
                m
            },
            ..baseline.clone()
        };

        let summary = build_read_pattern_summary(&baseline, &breaking);
        assert_eq!(summary.len(), 2);

        let prefix = summary.iter().find(|s| s.pattern == "PrefixScan").unwrap();
        assert_eq!(prefix.endpoint_count, 1);
        assert!(prefix.degradation > 1.0);

        let cached = summary.iter().find(|s| s.pattern == "Cached").unwrap();
        assert_eq!(cached.endpoint_count, 1);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p ckbadger-bench stress::report`
Expected: FAIL

- [ ] **Step 3: Implement report.rs**

```rust
use std::collections::HashMap;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use serde::Serialize;

use super::collector::{StageResult, StageStatus};
use super::scenario::Scenario;

// ---------------------------------------------------------------------------
// Report types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StressReport {
    pub timestamp: String,
    pub target: String,
    pub config: StressConfig,
    #[serde(skip)]
    pub scenarios: Vec<ScenarioReport>,
    #[serde(rename = "scenarios")]
    pub scenarios_json: Vec<ScenarioReportJson>,
}

// Separate JSON-friendly version since StageResult contains non-serializable fields
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScenarioReportJson {
    pub scenario: String,
    pub soft_degradation_vus: Option<u32>,
    pub breaking_point_vus: Option<u32>,
    pub stages: Vec<StageJson>,
    pub endpoint_breakdown: Vec<EndpointBreakdownEntry>,
    pub read_pattern_summary: Vec<ReadPatternSummaryEntry>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StageJson {
    pub stage_id: u16,
    pub vus: u32,
    pub duration_secs: u64,
    pub rps: f64,
    pub p50_ms: f64,
    pub p95_ms: f64,
    pub p99_ms: f64,
    pub error_rate: f64,
    pub error_count: u64,
    pub connection_refused: u64,
    pub status: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EndpointBreakdownEntry {
    pub endpoint_path: String,
    pub read_pattern: String,
    pub stable_p95_ms: f64,
    pub break_p95_ms: f64,
    pub degradation: f64,
    pub verdict: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadPatternSummaryEntry {
    pub pattern: String,
    pub endpoint_count: usize,
    pub stable_avg_p95_ms: f64,
    pub break_avg_p95_ms: f64,
    pub degradation: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StressConfig {
    pub scenarios: String,
    pub stage_duration_secs: u64,
    pub auto_ramp: bool,
    pub think_time_ms: String,
    pub timeout_ms: u64,
}

#[derive(Debug, Clone)]
pub struct ScenarioReport {
    pub scenario: Scenario,
    pub stage_results: Vec<StageResult>,
    pub soft_degradation_vus: Option<u32>,
    pub breaking_point_vus: Option<u32>,
}

// ---------------------------------------------------------------------------
// Status label
// ---------------------------------------------------------------------------

pub fn stage_status_label(status: StageStatus) -> &'static str {
    match status {
        StageStatus::Baseline => "baseline",
        StageStatus::Ok => "ok",
        StageStatus::SoftDegradation => "⚠ soft degradation",
        StageStatus::ErrorsRising => "⚠ errors rising",
        StageStatus::HardFailure => "✖ breaking point",
    }
}

// ---------------------------------------------------------------------------
// Endpoint breakdown
// ---------------------------------------------------------------------------

fn build_endpoint_breakdown(
    stable: &StageResult,
    breaking: &StageResult,
) -> Vec<EndpointBreakdownEntry> {
    let mut entries = Vec::new();

    for (idx, stable_ep) in &stable.per_endpoint {
        let break_ep = breaking.per_endpoint.get(idx);
        let break_p95 = break_ep.map(|e| e.p95_ms).unwrap_or(0.0);
        let degradation = if stable_ep.p95_ms > 0.0 {
            break_p95 / stable_ep.p95_ms
        } else {
            0.0
        };

        let verdict = if break_ep.is_some_and(|e| e.error_rate > 0.1) {
            "✖ critical".to_string()
        } else if degradation > 10.0 {
            "✖ first to break".to_string()
        } else if degradation > 3.0 {
            format!("{degradation:.1}× slow")
        } else {
            "ok".to_string()
        };

        entries.push(EndpointBreakdownEntry {
            endpoint_path: stable_ep.endpoint_path.clone(),
            read_pattern: stable_ep.read_pattern.clone(),
            stable_p95_ms: stable_ep.p95_ms,
            break_p95_ms: break_p95,
            degradation,
            verdict,
        });
    }

    entries.sort_by(|a, b| b.degradation.partial_cmp(&a.degradation).unwrap());
    entries
}

// ---------------------------------------------------------------------------
// Read pattern summary
// ---------------------------------------------------------------------------

pub fn build_read_pattern_summary(
    stable: &StageResult,
    breaking: &StageResult,
) -> Vec<ReadPatternSummaryEntry> {
    // Group by pattern
    let mut pattern_stable: HashMap<String, Vec<f64>> = HashMap::new();
    let mut pattern_break: HashMap<String, Vec<f64>> = HashMap::new();

    for ep in stable.per_endpoint.values() {
        pattern_stable
            .entry(ep.read_pattern.clone())
            .or_default()
            .push(ep.p95_ms);
    }
    for ep in breaking.per_endpoint.values() {
        pattern_break
            .entry(ep.read_pattern.clone())
            .or_default()
            .push(ep.p95_ms);
    }

    let mut entries = Vec::new();
    for (pattern, stable_vals) in &pattern_stable {
        let stable_avg = stable_vals.iter().sum::<f64>() / stable_vals.len() as f64;
        let break_vals = pattern_break.get(pattern);
        let break_avg = break_vals
            .map(|v| v.iter().sum::<f64>() / v.len() as f64)
            .unwrap_or(0.0);
        let degradation = if stable_avg > 0.0 {
            break_avg / stable_avg
        } else {
            0.0
        };

        entries.push(ReadPatternSummaryEntry {
            pattern: pattern.clone(),
            endpoint_count: stable_vals.len(),
            stable_avg_p95_ms: stable_avg,
            break_avg_p95_ms: break_avg,
            degradation,
        });
    }

    entries.sort_by(|a, b| b.degradation.partial_cmp(&a.degradation).unwrap());
    entries
}

// ---------------------------------------------------------------------------
// Terminal output
// ---------------------------------------------------------------------------

pub fn print_tables(report: &StressReport) {
    for scenario_report in &report.scenarios {
        let label = match scenario_report.scenario {
            Scenario::Mixed => "mixed",
            Scenario::Heavy => "heavy",
        };
        eprintln!("\n=== Stress Report: {label} ===\n");

        // Stage summary table
        eprintln!(
            "{:<6} {:>5} {:>9} {:>6} {:>8} {:>8} {:>8} {:>6}  Status",
            "Stage", "VUs", "Duration", "RPS", "p50", "p95", "p99", "Err%"
        );
        eprintln!("{}", "─".repeat(80));

        for stage in &scenario_report.stage_results {
            eprintln!(
                "{:>5}  {:>4}  {:>7}s  {:>5.0}  {:>6.0}ms {:>6.0}ms {:>6.0}ms {:>5.1}%  {}",
                stage.stage_id,
                stage.vus,
                stage.duration.as_secs(),
                stage.rps,
                stage.p50_ms,
                stage.p95_ms,
                stage.p99_ms,
                stage.error_rate * 100.0,
                stage_status_label(stage.status),
            );
        }

        eprintln!();
        if let Some(vus) = scenario_report.soft_degradation_vus {
            eprintln!("Soft degradation at: {vus} VUs");
        }
        if let Some(vus) = scenario_report.breaking_point_vus {
            eprintln!("Breaking point at: {vus} VUs");
        }

        // Endpoint breakdown: last stable vs last stage
        let last_stable = scenario_report
            .stage_results
            .iter()
            .filter(|s| matches!(s.status, StageStatus::Baseline | StageStatus::Ok))
            .last();
        let last_stage = scenario_report.stage_results.last();

        if let (Some(stable), Some(breaking)) = (last_stable, last_stage) {
            if stable.stage_id != breaking.stage_id {
                let breakdown = build_endpoint_breakdown(stable, breaking);
                if !breakdown.is_empty() {
                    eprintln!("\n--- Endpoint Breakdown ({}VU vs {}VU) ---\n",
                        stable.vus, breaking.vus);
                    eprintln!(
                        "{:<42} {:<14} {:>12} {:>12}  Verdict",
                        "Endpoint", "Pattern", "Stable p95", "Break p95"
                    );
                    eprintln!("{}", "─".repeat(95));
                    for entry in &breakdown {
                        eprintln!(
                            "{:<42} {:<14} {:>10.0}ms {:>10.0}ms  {}",
                            truncate(&entry.endpoint_path, 42),
                            entry.read_pattern,
                            entry.stable_p95_ms,
                            entry.break_p95_ms,
                            entry.verdict,
                        );
                    }

                    // First to break / most resilient
                    if let Some(worst) = breakdown.first() {
                        eprintln!(
                            "\nFirst to break: {} ({}, {:.1}× degradation)",
                            worst.endpoint_path, worst.read_pattern, worst.degradation
                        );
                    }
                    if let Some(best) = breakdown.last() {
                        eprintln!(
                            "Most resilient: {} ({}, {:.1}× degradation)",
                            best.endpoint_path, best.read_pattern, best.degradation
                        );
                    }
                }

                // Read pattern summary
                let pattern_summary = build_read_pattern_summary(stable, breaking);
                if !pattern_summary.is_empty() {
                    eprintln!("\n--- Read Pattern Summary ---\n");
                    eprintln!(
                        "{:<14} {:>10} {:>14} {:>14} {:>12}",
                        "ReadPattern", "Endpoints", "Avg p95 @stable", "Avg p95 @break", "Degradation"
                    );
                    eprintln!("{}", "─".repeat(70));
                    for entry in &pattern_summary {
                        let flag = if entry.degradation > 25.0 {
                            " ✖"
                        } else if entry.degradation > 15.0 {
                            " ⚠"
                        } else {
                            ""
                        };
                        eprintln!(
                            "{:<14} {:>10} {:>12.0}ms {:>12.0}ms {:>10.1}×{}",
                            entry.pattern,
                            entry.endpoint_count,
                            entry.stable_avg_p95_ms,
                            entry.break_avg_p95_ms,
                            entry.degradation,
                            flag,
                        );
                    }
                }
            }
        }

        eprintln!();
    }
}

fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len - 3])
    }
}

// ---------------------------------------------------------------------------
// JSON output
// ---------------------------------------------------------------------------

fn to_scenario_json(sr: &ScenarioReport) -> ScenarioReportJson {
    let stages: Vec<StageJson> = sr
        .stage_results
        .iter()
        .map(|s| StageJson {
            stage_id: s.stage_id,
            vus: s.vus,
            duration_secs: s.duration.as_secs(),
            rps: s.rps,
            p50_ms: s.p50_ms,
            p95_ms: s.p95_ms,
            p99_ms: s.p99_ms,
            error_rate: s.error_rate,
            error_count: s.error_count,
            connection_refused: s.connection_refused,
            status: stage_status_label(s.status).to_string(),
        })
        .collect();

    let last_stable = sr
        .stage_results
        .iter()
        .filter(|s| matches!(s.status, StageStatus::Baseline | StageStatus::Ok))
        .last();
    let last_stage = sr.stage_results.last();

    let (endpoint_breakdown, read_pattern_summary) =
        if let (Some(stable), Some(breaking)) = (last_stable, last_stage) {
            if stable.stage_id != breaking.stage_id {
                (
                    build_endpoint_breakdown(stable, breaking),
                    build_read_pattern_summary(stable, breaking),
                )
            } else {
                (Vec::new(), Vec::new())
            }
        } else {
            (Vec::new(), Vec::new())
        };

    ScenarioReportJson {
        scenario: match sr.scenario {
            Scenario::Mixed => "mixed".to_string(),
            Scenario::Heavy => "heavy".to_string(),
        },
        soft_degradation_vus: sr.soft_degradation_vus,
        breaking_point_vus: sr.breaking_point_vus,
        stages,
        endpoint_breakdown,
        read_pattern_summary,
    }
}

pub fn build_stress_report(report: &StressReport) -> StressReport {
    StressReport {
        timestamp: report.timestamp.clone(),
        target: report.target.clone(),
        config: report.config.clone(),
        scenarios_json: report.scenarios.iter().map(to_scenario_json).collect(),
        scenarios: Vec::new(), // not needed for JSON
    }
}

pub fn print_json(report: &StressReport) -> Result<()> {
    let json_report = build_stress_report(report);
    let json =
        serde_json::to_string_pretty(&json_report).context("failed to serialize stress report")?;
    println!("{json}");
    Ok(())
}

pub fn save_json(report: &StressReport, path: &Path) -> Result<()> {
    let json_report = build_stress_report(report);
    let json =
        serde_json::to_string_pretty(&json_report).context("failed to serialize stress report")?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    fs::write(path, &json)
        .with_context(|| format!("failed to write report to {}", path.display()))?;
    eprintln!("Report saved to {}", path.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    // ... (tests from Step 1)
}
```

- [ ] **Step 4: Update stress/mod.rs to use the real report**

Remove the `scenarios_json` field from `StressReport` construction in `run_stress()`. Instead, the JSON serialization handles the conversion in `print_json`/`save_json`. Update the `StressReport` creation:

```rust
let stress_report = report::StressReport {
    timestamp: chrono::Utc::now().to_rfc3339(),
    target,
    config: report::StressConfig {
        scenarios: args.scenario.clone(),
        stage_duration_secs: args.stage_duration,
        auto_ramp: args.auto_ramp,
        think_time_ms: args.think_time_ms.clone(),
        timeout_ms: args.timeout_ms,
    },
    scenarios: all_scenario_results,
    scenarios_json: Vec::new(), // populated lazily by print_json/save_json
};
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p ckbadger-bench stress::report`
Expected: all pass

- [ ] **Step 6: Verify full compilation**

Run: `cargo check -p ckbadger-bench`
Expected: compiles with no errors

- [ ] **Step 7: Commit**

```bash
git add crates/bench/src/stress/report.rs crates/bench/src/stress/mod.rs
git commit -m "feat(bench): add stress report with stage summary, endpoint breakdown, and pattern summary"
```

---

### Task 7: Frontend static requests in mixed scenario

Add predefined frontend route requests to the mixed scenario. These are simple GET requests to the frontend URL for key page routes.

**Files:**
- Modify: `crates/bench/src/stress/scenario.rs` (add frontend routes)
- Modify: `crates/bench/src/stress/vu.rs` (handle frontend requests)

- [ ] **Step 1: Add frontend routes to scenario.rs**

Add a constant list of frontend page routes and a function to create frontend endpoint groups:

```rust
/// Frontend page routes for mixed scenario.
pub const FRONTEND_ROUTES: &[&str] = &[
    "/",
    "/blocks",
    "/transactions",
    "/tokens",
    "/dao",
    "/scripts",
    "/charts",
];

/// Build a frontend endpoint group. Returns indices starting from `offset`.
pub fn build_frontend_group(offset: usize) -> EndpointGroup {
    EndpointGroup {
        name: "frontend",
        weight: 5, // small weight — most traffic is API
        endpoint_indices: (offset..offset + FRONTEND_ROUTES.len()).collect(),
    }
}
```

- [ ] **Step 2: Add FrontendEndpoint to vu.rs**

Add a type to represent resolved frontend requests alongside API endpoints:

```rust
/// A resolved endpoint — either an API endpoint or a frontend page.
pub enum ResolvedTarget {
    Api(ResolvedEndpoint),
    Frontend {
        idx: usize,
        route: String,
        url: String,
    },
}

/// Pre-resolve all endpoints including frontend routes.
pub fn resolve_all_with_frontend(
    entries: &[EndpointEntry],
    api_base: &str,
    frontend_url: &str,
    params: &DiscoveredParams,
) -> Vec<ResolvedTarget> {
    let mut targets: Vec<ResolvedTarget> = resolve_all(entries, api_base, params)
        .into_iter()
        .map(ResolvedTarget::Api)
        .collect();

    let offset = entries.len(); // frontend indices start after API entries
    for (i, route) in super::scenario::FRONTEND_ROUTES.iter().enumerate() {
        targets.push(ResolvedTarget::Frontend {
            idx: offset + i,
            route: route.to_string(),
            url: format!("{frontend_url}{route}"),
        });
    }

    targets
}
```

- [ ] **Step 3: Update spawn_vu to handle ResolvedTarget**

Update the VU loop to handle both API and frontend requests. For frontend requests, just do a simple GET:

```rust
pub fn spawn_vu(
    client: Arc<reqwest::Client>,
    resolved_targets: Arc<Vec<ResolvedTarget>>,
    groups: Arc<Vec<EndpointGroup>>,
    tx: SampleSender,
    think_time: Option<(u64, u64)>,
    cancel: CancellationToken,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut rng = rand::rng();
        loop {
            if cancel.is_cancelled() {
                break;
            }

            let target_idx = pick_endpoint(&groups);

            // Find matching target
            let target = resolved_targets.iter().find(|t| match t {
                ResolvedTarget::Api(ep) => ep.idx == target_idx,
                ResolvedTarget::Frontend { idx, .. } => *idx == target_idx,
            });

            let target = match target {
                Some(t) => t,
                None => continue,
            };

            let stress_sample = match target {
                ResolvedTarget::Api(ep) => {
                    let sample =
                        execute_request(&client, &ep.resolved, ep.expect_status).await;
                    StressSample {
                        endpoint_idx: ep.idx,
                        endpoint_path: ep.path_template.clone(),
                        read_pattern: ep.read_pattern.clone(),
                        latency_ms: sample.latency_ms,
                        status: sample.status,
                        body_size: sample.body_size,
                        error: sample.error,
                    }
                }
                ResolvedTarget::Frontend { idx, route, url } => {
                    let start = std::time::Instant::now();
                    let result = client.get(url).send().await;
                    let latency_ms = start.elapsed().as_secs_f64() * 1000.0;

                    match result {
                        Ok(resp) => {
                            let status = resp.status().as_u16();
                            let body = resp.bytes().await.unwrap_or_default();
                            StressSample {
                                endpoint_idx: *idx,
                                endpoint_path: route.clone(),
                                read_pattern: "Frontend".to_string(),
                                latency_ms,
                                status,
                                body_size: body.len(),
                                error: if status != 200 {
                                    Some(format!("expected 200, got {status}"))
                                } else {
                                    None
                                },
                            }
                        }
                        Err(e) => StressSample {
                            endpoint_idx: *idx,
                            endpoint_path: route.clone(),
                            read_pattern: "Frontend".to_string(),
                            latency_ms,
                            status: 0,
                            body_size: 0,
                            error: Some(e.to_string()),
                        },
                    }
                }
            };

            if tx.send(stress_sample).is_err() {
                break;
            }

            if let Some((min_ms, max_ms)) = think_time {
                let delay = rng.random_range(min_ms..=max_ms);
                tokio::select! {
                    _ = tokio::time::sleep(Duration::from_millis(delay)) => {}
                    _ = cancel.cancelled() => break,
                }
            }
        }
    })
}
```

- [ ] **Step 4: Update stress/mod.rs to use frontend-aware resolution**

In `run_stress()`, for `Scenario::Mixed`, use `resolve_all_with_frontend()` and include the frontend endpoint group. For `Scenario::Heavy`, use the original `resolve_all()` (no frontend).

```rust
let (resolved_targets, groups) = match scenario {
    Scenario::Mixed => {
        let targets = vu::resolve_all_with_frontend(
            &registry.entries,
            &api_url,
            &frontend_url,
            &discovery.params,
        );
        let mut groups = scenario::build_mixed_groups(&registry.entries);
        groups.push(scenario::build_frontend_group(registry.entries.len()));
        (Arc::new(targets), Arc::new(groups))
    }
    Scenario::Heavy => {
        let targets: Vec<vu::ResolvedTarget> = vu::resolve_all(&registry.entries, &api_url, &discovery.params)
            .into_iter()
            .map(vu::ResolvedTarget::Api)
            .collect();
        let groups = scenario::build_heavy_groups(&registry.entries);
        (Arc::new(targets), Arc::new(groups))
    }
};
```

Update `spawn_vu` calls to pass `resolved_targets` instead of `resolved_endpoints`.

- [ ] **Step 5: Verify compilation**

Run: `cargo check -p ckbadger-bench`
Expected: compiles

- [ ] **Step 6: Run all tests**

Run: `cargo test -p ckbadger-bench`
Expected: all pass

- [ ] **Step 7: Commit**

```bash
git add crates/bench/src/stress/
git commit -m "feat(bench): add frontend static route requests to mixed stress scenario"
```

---

### Task 8: Integration test and final verification

Run the full test suite, verify the CLI help output, and make sure everything works end-to-end.

**Files:**
- No new files — verification only

- [ ] **Step 1: Run all tests**

Run: `cargo test -p ckbadger-bench`
Expected: all tests pass

- [ ] **Step 2: Check clippy**

Run: `cargo clippy -p ckbadger-bench`
Expected: no warnings

- [ ] **Step 3: Verify CLI help for stress subcommand**

Run: `cargo run -p ckbadger-bench -- stress --help`
Expected: shows all stress flags (--scenario, --stages, --stage-duration, --auto-ramp, --think-time-ms, --remote-host, --api-url, --frontend-url, --timeout-ms, --warmup-duration, --json, --output-dir)

- [ ] **Step 4: Verify bench subcommand still works**

Run: `cargo run -p ckbadger-bench -- bench --help`
Expected: shows original bench flags

- [ ] **Step 5: Verify backward compatibility (no subcommand)**

Run: `cargo run -p ckbadger-bench -- --help`
Expected: shows help with both subcommands listed

- [ ] **Step 6: Fix any issues found, re-run tests**

- [ ] **Step 7: Commit any fixes**

```bash
git add crates/bench/
git commit -m "fix(bench): address clippy and integration issues in stress testing"
```
