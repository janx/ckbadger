# API Performance Benchmark Design

## Goal

Build `ckbadger-bench`, a Rust-native benchmark binary that tests all API endpoints against a live local ckbadger instance. The benchmark discovers real parameters from the running API, measures latency and throughput, identifies bottlenecks, and detects regressions between runs.

## Principle Alignment

- **Local First** — runs against localhost, self-configures from local data
- **Agent Friendly** — JSON output for automation, structured registry for maintenance
- **CKB Native** — tests real CKB data paths (cross-store cell reads, DAO scanning, activity filtering)

## Architecture

### Crate Structure

```
crates/bench/
  Cargo.toml          # ckbadger-bench binary crate
  src/
    main.rs           # CLI entry point (clap)
    discovery.rs      # Parameter discovery from live API
    registry.rs       # Endpoint registry (declarative table + hooks)
    runner.rs         # Execution engine (reqwest + tokio)
    metrics.rs        # Latency collection, percentile calculation
    report.rs         # Terminal table + JSON output
    endpoints/
      mod.rs
      activities.rs
      assets.rs
      blocks.rs
      cells.rs
      dao.rs
      fiber.rs
      forks.rs
      graph.rs
      hardforks.rs
      identities.rs
      mempool.rs
      scripts.rs
      search.rs
      spore.rs
      statistics.rs
      tokens.rs
      transactions.rs
```

The binary depends on `reqwest`, `tokio`, `clap`, `serde`, `serde_json`, and `chrono`. It does not depend on `ckbadger-store`. All parameter discovery goes through the HTTP API, keeping the benchmark independent of store internals.

### CLI Interface

```bash
ckbadger-bench                                    # All endpoints, 10 iterations, sequential
ckbadger-bench --module dao                       # DAO endpoints only
ckbadger-bench --endpoint "/blocks/{id}"          # Single endpoint
ckbadger-bench --iterations 50                    # 50 requests per endpoint
ckbadger-bench --concurrency 4                    # 4 parallel requests
ckbadger-bench --risk high                        # High-risk endpoints only
ckbadger-bench --json                             # JSON output
ckbadger-bench --json --output report.json        # Save JSON to file
ckbadger-bench --compare baseline.json            # Regression comparison
ckbadger-bench --discovery-only                   # Print discovered params, exit
ckbadger-bench --api-url http://localhost:8101     # Custom API base URL
```

Default API base: `http://localhost:8101/api/v1`.

## Discovery System

Discovery runs at startup to collect real parameters for endpoint testing. Three phases execute in sequence.

### Phase 1: Capabilities

Fetch `GET /capabilities` from the frontend server (port 8100 by default, configurable via `--frontend-url`) to obtain the route matrix. This is the authoritative list of all supported route patterns. The benchmark validates its registry against this list and warns about coverage gaps. If the frontend is unreachable, the benchmark skips coverage validation and proceeds with the registry as-is.

```rust
struct Capabilities {
    routes_markdown: Vec<String>,
    routes_raw: Vec<String>,
}
```

### Phase 2: Data Availability Probes

Lightweight GET requests determine which data modules have content:

| Probe | Endpoint | Enables |
|-------|----------|---------|
| `has_fiber` | `GET /fiber/channels?limit=1` | Fiber module |
| `has_spore` | `GET /spore/clusters?limit=1` | Spore module |
| `has_dao` | `GET /dao/deposits?limit=1` | DAO module |
| `has_identities` | `GET /identities/dotbit/items?limit=1` | Identities module |
| `has_tokens` | `GET /tokens?limit=1` | Tokens module |
| `has_graph` | `GET /graph/transaction/{hash}` (using a discovered tx hash) | Graph module |
| `has_mempool` | `GET /mempool/info` | Mempool module |
| `has_assets` | `GET /assets?limit=1` | Assets module |

A module with no data is skipped entirely — no wasted discovery requests, no false failures.

### Phase 3: Parameter Discovery

For each enabled module, hit specific endpoints to collect real values:

```rust
struct DiscoveredParams {
    // From GET /statistics/network
    sync_tip: u64,

    // From GET /blocks?limit=1
    latest_block_number: u64,
    latest_block_hash: String,

    // Computed: sync_tip / 2
    mid_block_number: u64,

    // From GET /transactions?limit=5
    tx_hashes: Vec<String>,

    // From GET /transactions/{hash}/detail (first tx with multiple inputs)
    complex_tx_hash: Option<String>,

    // From GET /addresses/top?limit=5
    top_addresses: Vec<String>,
    top_lock_hashes: Vec<String>,

    // From GET /dao/top-depositors?limit=3
    dao_lock_hashes: Vec<String>,

    // From GET /dao/deposits?limit=1
    dao_deposit_outpoint: Option<(String, u32)>,

    // From GET /tokens?limit=3
    token_type_hashes: Vec<String>,

    // From GET /spore/clusters?limit=3
    cluster_ids: Vec<String>,

    // From GET /spore/objects?limit=3
    spore_ids: Vec<String>,

    // From GET /scripts?limit=3
    script_names: Vec<String>,

    // From GET /cells/live?limit=1
    live_cell_outpoint: Option<(String, u32)>,

    // From GET /fiber/channels?limit=1
    fiber_channel_id: Option<String>,

    // From GET /identities/dotbit/items?limit=1
    dotbit_item_id: Option<String>,

    // From GET /assets?limit=1
    object_collection_id: Option<String>,
    object_item_id: Option<String>,
}
```

Discovery takes ~15-20 API calls and finishes in 1-2 seconds. Results are cached for the entire run. The `--discovery-only` flag prints discovered params and exits.

## Endpoint Registry

Each endpoint is a declarative entry with an optional custom resolver.

### Entry Structure

```rust
struct EndpointEntry {
    module: &'static str,
    method: Method,
    path_template: &'static str,
    description: &'static str,
    resolve: fn(&DiscoveredParams) -> Option<ResolvedRequest>,
    expect_status: u16,
    risk_tier: RiskTier,
    read_pattern: ReadPattern,
}
```

### Risk Tiers

| Tier | Criteria |
|------|----------|
| **High** | Cross-store reads, filtered range scans, full CF scans, visitor-pattern scans |
| **Medium** | Prefix scans with limit, batch multi-gets, chart aggregations |
| **Low** | Single key lookups, cached responses, RPC pass-throughs |

### Read Patterns

| Pattern | Description | Example Endpoints |
|---------|-------------|-------------------|
| `KeyLookup` | Single `get_cf` | `GET /blocks/{id}` |
| `BatchLookup` | `multi_get_cf` | `GET /transactions/{hash}` |
| `PrefixScan` | `prefix_iterator` with limit | `GET /blocks`, `GET /dao/deposits` |
| `RangeScan` | Range iterator, potentially filtered | `GET /addresses/{addr}/activities` |
| `FullCfScan` | Scans entire column family | `GET /tokens`, `GET /spore/objects` |
| `CrossStore` | Reads both domain + append-only stores | `GET /cells/{tx_hash}/{idx}`, `GET /cells/live` |
| `RpcDependent` | Depends on CKB node RPC | `GET /mempool/info`, `GET /graph/*` |
| `Cached` | Served from in-memory cache | `GET /statistics/network` |
| `Aggregation` | Date-range chart queries | `GET /charts/*`, `GET /dao/charts/*` |

### Resolver Pattern

Simple endpoints return a static URL:

```rust
resolve: |_| Some(req("/api/v1/tokens"))
```

Parameterized endpoints use discovered values:

```rust
resolve: |p| p.top_addresses.first().map(|a|
    req(&format!("/api/v1/addresses/{a}/activities?limit=50"))
)
```

Returning `None` skips the endpoint with a note.

### Coverage Validation

At startup, the registry compares itself against the `/capabilities` route matrix:

```
Registry:  92 endpoints defined
Capabilities: 88 routes in matrix
  ✓ 88 matched
  + 4 registry-only (not in capabilities matrix)
  - 0 uncovered capability routes
```

Uncovered capability routes produce warnings.

## Execution Engine

### Configuration

```rust
struct BenchConfig {
    api_base: String,               // default "http://localhost:8101"
    iterations: u32,                // default 10
    concurrency: u32,               // default 1
    warmup: u32,                    // default 2
    timeout_ms: u64,                // default 10_000
    module_filter: Option<String>,
    endpoint_filter: Option<String>,
    risk_filter: Option<RiskTier>,
}
```

### Execution Flow Per Endpoint

1. Call `resolve(params)`. If `None`, skip with note.
2. Run `warmup` requests (not measured). These prime cache and RocksDB block cache.
3. Run `iterations` measured requests (sequential or concurrent).
4. Record each sample: latency, status code, response body size.
5. Collect into `EndpointResult`.

### Concurrency Model

Sequential (`concurrency=1`): simple loop, measures baseline latency.

Concurrent (`concurrency>1`): semaphore-bounded `tokio::spawn`, measures contention behavior.

```rust
// concurrency=1
for _ in 0..iterations {
    let sample = execute_request(&client, &request).await;
    samples.push(sample);
}

// concurrency>1
let semaphore = Arc::new(Semaphore::new(concurrency));
for _ in 0..iterations {
    let permit = semaphore.acquire().await;
    handles.push(tokio::spawn(async move {
        let sample = execute_request(&client, &request).await;
        drop(permit);
        sample
    }));
}
```

### Execution Order

Endpoints run module-by-module in registry order. Within each module, low-risk endpoints run first to warm RocksDB block cache for that module's column families, then medium-risk, then high-risk.

### Error Handling

- Errors do not abort the run. Each failed endpoint records its error.
- Connection refused at startup aborts immediately: "Is ckbadger API running at {url}?"
- Per-request timeout is configurable (default 10s).

## Metrics and Reporting

### Per-Request Sample

```rust
struct Sample {
    latency: Duration,
    status: u16,
    body_size: usize,
    error: Option<String>,
}
```

### Computed Metrics Per Endpoint

```rust
struct ComputedMetrics {
    p50: Duration,
    p95: Duration,
    p99: Duration,
    min: Duration,
    max: Duration,
    mean: Duration,
    std_dev: Duration,
    error_rate: f64,
    avg_body_size: usize,
    throughput_rps: f64,
}
```

### Terminal Table (Default)

```
ckbadger API Benchmark — 2026-03-28T14:30:00Z
API: http://localhost:8101 | Iterations: 10 | Concurrency: 1

Module        Endpoint                              Pattern      p50     p95     p99    Errs  Size
────────────────────────────────────────────────────────────────────────────────────────────────────
activities    GET /addresses/{addr}/activities       RangeScan    45ms   120ms   180ms   0%   12KB
blocks        GET /blocks/{id}                      KeyLookup     3ms     5ms     6ms   0%    2KB
cells         GET /cells/{tx_hash}/{output_index}   CrossStore    8ms    14ms    19ms   0%    1KB
dao           GET /dao/deposits                     PrefixScan   55ms   210ms   340ms   0%   18KB  ⚠ SLOW
mempool       GET /mempool/info                     RpcDep        —       —       —      —     —   SKIPPED

────────────────────────────────────────────────────────────────────────────────────────────────────
Summary: 88 tested, 4 skipped | Slowest p95: dao/deposits (210ms), activities/addr (120ms)
         3 endpoints > 100ms p95 ⚠
```

### Flagging Rules

| Flag | Condition |
|------|-----------|
| `⚠ SLOW` | p95 > 100ms |
| `🔴 VERY SLOW` | p95 > 500ms |
| `⚠ ERRORS` | error_rate > 0% |
| `SKIPPED` | Data not available |

### JSON Output (`--json`)

```json
{
  "timestamp": "2026-03-28T14:30:00Z",
  "config": { "iterations": 10, "concurrency": 1, "api_base": "..." },
  "summary": {
    "tested": 88,
    "skipped": 4,
    "slow_count": 3,
    "error_count": 0
  },
  "results": [
    {
      "module": "activities",
      "method": "GET",
      "path_template": "/addresses/{addr}/activities",
      "resolved_url": "/api/v1/addresses/ckb1q.../activities?limit=50",
      "read_pattern": "RangeScan",
      "risk_tier": "High",
      "metrics": { "p50_ms": 45, "p95_ms": 120, "p99_ms": 180 },
      "samples": [ { "latency_ms": 42, "status": 200, "body_size": 12340 } ]
    }
  ]
}
```

### Regression Comparison (`--compare baseline.json`)

Compares the current run against a saved JSON report:

```
Regression Report (vs baseline 2026-03-25):
  dao/deposits        p95: 210ms → 340ms  (+62%)  ⚠ REGRESSION
  blocks/list         p95:  18ms →  16ms  (-11%)  ✓ improved
  activities/addr     p95: 120ms → 125ms   (+4%)  ~ stable
```

A regression is flagged when p95 increases by more than 20%.

## Endpoint Coverage

All ~90 endpoints across 18 route modules. Organized by risk tier.

### High Risk (Cross-Store, Range Scans, Full CF Scans)

| Module | Endpoint | Pattern |
|--------|----------|---------|
| activities | `GET /addresses/{addr}/activities` | RangeScan (filtered, may loop 128-item batches) |
| activities | `GET /activities` | RangeScan |
| activities | `GET /activities/latest` | RangeScan (scans until 64 non-cellbase txs) |
| cells | `GET /cells/live` | CrossStore (list live_cell_owners + get_cell_payload) |
| cells | `GET /cells/by-script` | CrossStore (prefix scan + cell payload fetch) |
| cells | `GET /cells/{tx_hash}/{idx}` | CrossStore (domain marker + append-only payload) |
| cells | `GET /addresses/{addr}/transactions` | PrefixScan (addr_txs index) |
| cells | `GET /addresses/{addr}/tokens` | PrefixScan (addr token balances) |
| dao | `GET /dao/deposits` | PrefixScan (visitor pattern, not batched) |
| dao | `GET /dao/deposits/{lock_hash}` | PrefixScan (per-address DAO scan) |
| dao | `GET /dao/top-depositors` | PrefixScan (sorted aggregation) |
| tokens | `GET /tokens` | FullCfScan (warmup cached, but cold = full scan) |
| tokens | `GET /tokens/{type_hash}/holders` | PrefixScan (holder list) |
| tokens | `GET /tokens/{type_hash}/transfers` | PrefixScan (transfer history) |
| spore | `GET /spore/objects` | FullCfScan (warmup cached) |
| spore | `GET /spore/clusters/{id}/spores` | PrefixScan (cluster objects) |
| transactions | `GET /transactions/{hash}/detail` | CrossStore (full input/output cell resolution) |
| search | `GET /search?q=...` | Multi-source fan-out (caches + store) |

### Medium Risk (Prefix Scans, Aggregations, Batch Lookups)

| Module | Endpoint | Pattern |
|--------|----------|---------|
| blocks | `GET /blocks` | PrefixScan (list_blocks_desc + epoch enrichment) |
| blocks | `GET /blocks/{id}/fee-stats` | BatchLookup (list block txs) |
| dao | `GET /dao/summary/{lock_hash}` | BatchLookup (deposit cache) |
| dao | `GET /dao/charts/*` (3 endpoints) | Aggregation (daily stats date range) |
| statistics | `GET /charts/*` (20 endpoints) | Aggregation (daily aggregates) |
| tokens | `GET /tokens/{type_hash}/activities` | PrefixScan |
| spore | `GET /spore/clusters` | PrefixScan (warmup cached) |
| spore | `GET /spore/clusters/{id}/holders` | PrefixScan |
| assets | `GET /assets/objects/{id}/items` | PrefixScan |
| identities | `GET /identities/{id}/items` | PrefixScan |
| fiber | `GET /fiber/channels` | PrefixScan |
| transactions | `GET /transactions` | PrefixScan (block txs) |
| transactions | `GET /transactions/{hash}/cell-deps` | CrossStore (code cell lookup) |
| scripts | `GET /scripts/{name}/charts/*` | Aggregation |

### Low Risk (Key Lookups, Cached, RPC)

| Module | Endpoint | Pattern |
|--------|----------|---------|
| blocks | `GET /blocks/{id}` | KeyLookup |
| blocks | `GET /blocks/{id}/proposals` | KeyLookup |
| dao | `GET /dao/statistics` | Cached |
| dao | `GET /dao/calculator` | KeyLookup (cell payload) |
| statistics | `GET /statistics/network` | Cached |
| statistics | `GET /statistics/tx-stats` | Cached |
| statistics | `GET /statistics/recent-blocks` | Cached |
| tokens | `GET /tokens/{type_hash}` | KeyLookup |
| spore | `GET /spore/objects/{id}` | KeyLookup |
| spore | `GET /spore/objects/{id}/decode` | KeyLookup + DOB decoder |
| scripts | `GET /scripts` | Cached (warmup) |
| scripts | `GET /scripts/{name}` | KeyLookup |
| hardforks | `GET /hardforks` | Cached |
| forks | `GET /forks` | PrefixScan (rare data) |
| forks | `GET /forks/recent` | KeyLookup |
| mempool | `GET /mempool/*` (4 endpoints) | RpcDependent |
| graph | `GET /graph/*` (3 endpoints) | RpcDependent (CKB RocksDB) |
| transactions | `GET /transactions/{hash}` | BatchLookup |
| transactions | `GET /transactions/{hash}/cycles` | KeyLookup + async |
| fiber | `GET /fiber/stats` | Cached |
| addresses | `GET /addresses/top` | Cached |
| addresses | `GET /addresses/active` | Cached |
| addresses | `GET /addresses/{addr}` | KeyLookup |

## Out of Scope

- WebSocket benchmarking (different protocol)
- Write-path benchmarks (API is read-only)
- RocksDB-level profiling (perf/flamegraph investigation, separate task)
- CI integration (add after baseline is established)
- Markdown/raw format benchmarking (focus on JSON API first)
