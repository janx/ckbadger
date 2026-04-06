# Stress Testing Design

## Goal

Design a stress testing capability for ckbadger that answers: **how much concurrent load can the website handle during live sync, and what breaks first?**

The database is already synced. The indexer is in live sync mode (polling for new blocks). The target is the read path: API endpoints + frontend static resources serving real user traffic.

## Principle Alignment

- **Local First** — stress tool is part of `crates/bench/`, runs from localhost by default, no external service dependencies
- **Agent Friendly** — structured JSON output, `--auto-ramp` for unattended runs
- **Fail Fast** — detect and report degradation/failure points, don't mask them

## Architecture

### Subcommand Structure

Add `stress` subcommand to `ckbadger-bench`. Existing benchmark behavior becomes the `bench` subcommand (default when no subcommand is given, preserving backward compatibility).

```
ckbadger-bench bench  [existing flags]   # per-endpoint baseline benchmarking
ckbadger-bench stress [stress flags]     # concurrent load + ramp-up stress testing
```

### New Modules

```
crates/bench/src/
  stress/
    mod.rs           # subcommand entry, stage scheduler
    vu.rs            # Virtual User loop, endpoint selection, think time
    scenario.rs      # mixed / heavy scenario definitions, weight tables
    collector.rs     # mpsc sample collection, per-stage aggregation, degradation detection
    report.rs        # stage summary + endpoint breakdown + read pattern summary
```

### Reused Modules

- `discovery.rs` — parameter discovery (unchanged)
- `registry.rs` — endpoint registry, risk tiers, read patterns (unchanged)
- `metrics.rs` — percentile computation (extract as public functions if needed)
- `runner.rs` — extract `execute_request` as public function for VU reuse

### Changes to Existing Code

- `main.rs` — add clap subcommand dispatch (`bench` and `stress`)
- `runner.rs` — make `execute_request` public
- `metrics.rs` — make percentile functions public if currently private

## Ramp Stages

Core concept: **staged ramp-up with snapshot metrics per stage**.

### Default Stages

```
Stage 1:  10 VUs, 30s   ── baseline
Stage 2:  25 VUs, 30s   ── light load
Stage 3:  50 VUs, 30s   ── moderate
Stage 4: 100 VUs, 30s   ── heavy
Stage 5: 200 VUs, 30s   ── stress
Stage 6: 300 VUs, 30s   ── break point
```

Each VU is a tokio task that loops: select endpoint by weight, execute request, optional think time, repeat. At stage boundaries, the collector snapshots all metrics for that stage.

### Custom Stages

CLI flag `--stages 10,25,50,100,200,300` overrides the default sequence. `--stage-duration 30` sets per-stage duration in seconds.

### Auto-Ramp Mode

`--auto-ramp` ignores the fixed stage list. Instead:

1. Start at 10 VUs (baseline stage, always runs full duration)
2. Multiply VU count by 2 each stage: 10 → 20 → 40 → 80 → 160 → 320 → ...
3. After each stage, check degradation signals
4. Stop when hard failure is detected

### Exit Conditions

- All stages complete (fixed mode)
- Hard failure detected (`--auto-ramp`)
- User Ctrl+C — output report for completed stages

## Scenarios

### Mixed Traffic (--scenario mixed)

Simulates real user browsing. Each VU request is randomly assigned to an endpoint group by weight:

| Group | Weight | Endpoints |
|-------|--------|-----------|
| Homepage/Overview | 25% | `GET /statistics/network`, `GET /blocks?limit=10`, `GET /transactions?limit=10`, frontend `index.html` + static assets |
| Block Browsing | 20% | `GET /blocks/{id}`, `GET /blocks/{id}/fee-stats` |
| Transaction Detail | 20% | `GET /transactions/{hash}`, `GET /transactions/{hash}/detail` |
| Address Pages | 15% | `GET /addresses/{lock_hash}/activities`, `GET /addresses/{lock_hash}/transactions` |
| Assets/Tokens | 10% | `GET /tokens`, `GET /tokens/{type_hash}/holders`, `GET /spore/clusters` |
| Search/Other | 10% | `GET /search?q=...`, `GET /dao/deposits`, `GET /mempool/info` |

Think time: random 50–200ms between requests per VU.

Frontend requests: predefined set of key routes (`/`, `/blocks`, `/transactions/{hash}`, etc.) requested against `--frontend-url`. No HTML parsing — just HTTP GET the page URLs and static assets.

### Heavy Query Assault (--scenario heavy)

No think time. Concentrated fire on high-risk endpoints:

- Filter to `RiskTier::High` endpoints (~30+)
- Weight by `ReadPattern` danger:
  - `FullCfScan`: weight 4
  - `CrossStore`: weight 4
  - `RangeScan`: weight 3
  - `PrefixScan`: weight 2
  - All others: weight 1
- Uses discovery's "busiest" parameters (busiest lock hashes, top token type hashes, etc.) to maximize query cost
- Goal: identify which read patterns and endpoints collapse first under load

### CLI Usage

```bash
# Mixed traffic, auto-ramp until breakage
ckbadger-bench stress --scenario mixed --auto-ramp

# Heavy queries, fixed stages
ckbadger-bench stress --scenario heavy --stages 10,25,50,100

# Both scenarios sequentially
ckbadger-bench stress --scenario mixed,heavy --auto-ramp

# Remote target
ckbadger-bench stress --scenario mixed --auto-ramp --remote-host 192.168.1.100
```

## Target Configuration

### Local (default)

```bash
ckbadger-bench stress --scenario mixed
# api-url:      http://localhost:8101/api/v1
# frontend-url: http://localhost:8100
```

### Remote (--remote-host)

```bash
ckbadger-bench stress --remote-host 192.168.1.100
# auto-expands to:
#   api-url:      http://192.168.1.100:8101/api/v1
#   frontend-url: http://192.168.1.100:8100
```

Default ports: API 8101, frontend 8100 (ckbadger defaults).

`--remote-host` and `--api-url` are mutually exclusive. Passing both is an error.

Report config section auto-labels `target: local` or `target: remote (<host>)`.

## Metrics & Collection

### Sample Structure

```rust
struct StressSample {
    timestamp: Instant,
    stage_id: u16,
    endpoint_group: &'static str,  // "homepage", "block_detail", etc.
    endpoint_path: String,         // actual requested path
    read_pattern: ReadPattern,
    latency_ms: f64,
    status: u16,
    body_size: usize,
    error: Option<String>,         // connect refused, timeout, 5xx body
}
```

### Collection Architecture

All VUs send samples via `mpsc::unbounded_channel` to a dedicated collector task. The collector:

1. **Per-second rolling window** — computes 1-second RPS, p95, error rate for the real-time status line
2. **Per-stage aggregation** — at stage end, computes full metrics (percentiles, error breakdown, per-endpoint, per-read-pattern)

No disk I/O during collection. All in-memory.

### Real-Time Status Line (stderr)

```
[stage 3/6 · 50 VUs · 32s] rps=485  p95=45ms  err=0.0%  ▓▓▓▓▓▓░░░░
```

Refreshed every second. Does not interfere with JSON output on stdout.

### Degradation Detection (auto-ramp)

Based on stage 1 baseline metrics:

| Signal | Threshold | Meaning |
|--------|-----------|---------|
| Soft degradation | p95 > 2× baseline p95 | Latency deteriorating |
| Errors emerging | error rate > 1% | Failures appearing |
| Hard failure | error rate > 10% OR 5 consecutive seconds of all timeouts/connection refused | Service effectively down |

`--auto-ramp` records soft degradation point but continues. Stops at hard failure.

## Report Output

### Stage Summary Table

```
Stage  VUs  Duration  RPS    p50     p95      p99     Err%    Status
─────────────────────────────────────────────────────────────────────
  1     10    30s     120    12ms    25ms     40ms    0.0%    baseline
  2     25    30s     285    14ms    30ms     55ms    0.0%    ok
  3     50    30s     510    18ms    48ms     85ms    0.0%    ok
  4    100    30s     780    35ms   120ms    250ms    0.2%    ⚠ soft degradation
  5    200    30s     850    85ms   450ms   1200ms    3.5%    ⚠ errors rising
  6    300    18s     620   200ms  2500ms   5000ms   15.2%   ✖ breaking point

Soft degradation at: 100 VUs (p95 jumped from 25ms to 120ms, 4.8×)
Breaking point at: 300 VUs (error rate 15.2%, 12 connection refused)
```

### Endpoint Breakdown Table

Compares the last stable stage vs the breaking point stage per endpoint:

```
Endpoint                              Pattern       Stable(50VU)  Break(300VU)  Verdict
────────────────────────────────────────────────────────────────────────────────────────
GET /blocks                           PrefixScan    p95=30ms      p95=180ms     3.0× slow
GET /blocks/{id}                      KeyLookup     p95=15ms      p95=45ms      ok
GET /addresses/{addr}/activities      RangeScan     p95=85ms      p95=3200ms    ✖ first to break
GET /tokens/{hash}/holders            FullCfScan    p95=120ms     p95=4500ms    ✖ critical
GET /transactions/{hash}/detail       CrossStore    p95=95ms      p95=2800ms    ✖ critical
GET /statistics/network               Cached        p95=5ms       p95=12ms      ok

First to break: GET /addresses/{addr}/activities (RangeScan, High risk)
Most resilient: GET /statistics/network (Cached, Low risk)
```

### Read Pattern Summary Table

Aggregated across all endpoints by `ReadPattern`:

```
ReadPattern     Endpoints  Avg p95 @50VU  Avg p95 @300VU  Degradation
─────────────────────────────────────────────────────────────────────
Cached              14       5ms            12ms           2.4×
KeyLookup           26      15ms            45ms           3.0×
BatchLookup          4      30ms           150ms           5.0×
PrefixScan          33      45ms           800ms          17.8× ⚠
RangeScan            7      60ms          1500ms          25.0× ✖
FullCfScan           7      90ms          3000ms          33.3× ✖
CrossStore           5      80ms          2200ms          27.5× ✖
Aggregation         18      25ms           200ms           8.0×
```

### Output Formats

- **Default**: human-readable tables to stderr (as shown above)
- **`--json`**: structured JSON to stdout containing all stages, endpoint breakdown, read pattern summary
- **`--output-dir`**: auto-save timestamped JSON report to directory

## CLI Reference

```
ckbadger-bench stress [OPTIONS]

OPTIONS:
  --scenario <SCENARIO>        mixed, heavy, or mixed,heavy [default: mixed]
  --stages <VU_LIST>           Comma-separated VU counts per stage [default: 10,25,50,100,200,300]
  --stage-duration <SECS>      Duration per stage in seconds [default: 30]
  --auto-ramp                  Auto-increase VUs until hard failure (ignores --stages)
  --think-time-ms <RANGE>      Think time range in ms for mixed scenario [default: 50-200]
  --remote-host <HOST>         Remote ckbadger host (auto-expands to api-url + frontend-url)
  --api-url <URL>              API base URL [default: http://localhost:8101/api/v1]
  --frontend-url <URL>         Frontend base URL [default: http://localhost:8100]
  --timeout-ms <MS>            Per-request timeout [default: 10000]
  --warmup-duration <SECS>     Warmup before stage 1 (discovery + cache priming) [default: 5]
  --json                       Output JSON to stdout
  --output-dir <DIR>           Auto-save timestamped reports
```

Mutual exclusion: `--remote-host` and `--api-url`/`--frontend-url` cannot be used together.

## Scope Boundaries

### In Scope

- `stress` subcommand with staged ramp-up
- Mixed and heavy scenarios with weighted endpoint selection
- Real-time status line during test
- Auto-ramp with degradation detection
- Per-stage, per-endpoint, per-read-pattern reporting
- Frontend static resource requests in mixed scenario
- Local and remote target support
- JSON and human-readable output

### Not In Scope

- Distributed load generation (multi-machine coordinated load)
- Browser rendering / headless browser testing
- Write path stress testing (API is read-only)
- Criterion micro-benchmarks for storage layer
- CI integration (manual tool first)
- WebSocket stress testing
- POST/PUT/DELETE method testing

## Result

- **Behavior change**: `ckbadger-bench` gains a `stress` subcommand for concurrent load testing with staged ramp-up, two scenarios (mixed traffic + heavy query assault), real-time monitoring, and detailed breakdown reports
- **Re-sync required**: no
- **What to do next**: implement according to plan
