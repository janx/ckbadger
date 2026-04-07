# Testing Systems

ckbadger has three testing systems, each answering a different question:

| System     | Question                                     | Runs against    |
| ---------- | -------------------------------------------- | --------------- |
| **verify** | Is the indexed data correct?                 | API (read-only) |
| **bench**  | How fast is each endpoint?                   | API + frontend  |
| **stress** | How much load can it handle before breaking? | API + frontend  |

All three require a running ckbadger instance (indexer + API + frontend). None modify the database.

## Verify

Data integrity verification. Calls the API and optionally the official CKB explorer to confirm indexed data matches the chain.

### Quick Start

```bash
ckbadger verify --depth fast              # 6 checks, seconds
ckbadger verify --depth sampling          # 55 checks, minutes
ckbadger verify --list-checks             # List all checks
ckbadger verify --checks genesis_block,dao_statistics_sane  # Specific checks
```

### Check Tiers

| Tier                  | Checks | Runtime | What it validates                                                                                                                                                                                                                               |
| --------------------- | ------ | ------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Fast** (F1-F6)      | 6      | seconds | API reachable, sync complete, genesis block, tip block, deep fork clear, DAO statistics sane                                                                                                                                                    |
| **Sampling** (S1-S23) | 23     | minutes | Block hash roundtrip, parent chain, address balance, chart validations (tx count, cells, supply, block time, epoch, HODL wave, knowledge composition, APC, inflation), supply invariants, RPC compare, tokens, spores, NFTs, holder consistency |
| **Explorer** (X1-X26) | 26     | minutes | Compare last 30 days against official CKB explorer API (tx count, DAO deposit, hash rate, difficulty, knowledge size, uncle rate, cell counts, supply, circulation, mining reward, treasury)                                                    |

`--depth fast` runs Fast tier only. `--depth sampling` runs all three tiers (Fast + Sampling + Explorer). Explorer checks can be skipped with `--no-explorer`.

### CLI Reference

```
ckbadger verify [OPTIONS]

OPTIONS:
  --depth <DEPTH>          fast or sampling [default: fast]
  --list-checks            List all checks and exit
  --checks <NAME,...>      Run specific checks by name
  --api-url <URL>          API base URL [default: http://localhost:3001/api/v1]
  --rpc-url <URL>          CKB RPC URL (enables RPC spot-checks)
  --explorer-url <URL>     Official explorer API [default: https://mainnet-api.explorer.nervos.org]
  --no-explorer            Skip explorer comparison checks
  --sample-count <N>       Samples for sampling tier [default: 1000]
  --seed <N>               Deterministic seed for reproducibility [default: 42]
  --tolerance <F>          Max deviation from explorer [default: 0.001]
  --format <FORMAT>        Output format: text or json [default: text]
  --cache-dir <DIR>        Explorer response cache [default: .verify-cache]
```

### Explorer Response Cache

Explorer checks cache HTTP responses to `.verify-cache/`. Fresh cache (< 24h) is reused. Stale cache is re-fetched; on HTTP failure, stale data is used with a warning.

### Adding a Check

1. Choose tier: `api_checks.rs` (fast/sampling) or `explorer.rs` (explorer comparison)
2. Create struct implementing `Check` trait (name, description, tier, run)
3. Register in the module's `*_checks()` function
4. Convention: `F{N}` / `S{N}` / `X{N}` prefix in doc comment

### File Locations

| What                | Where                                     |
| ------------------- | ----------------------------------------- |
| CLI args & runner   | `crates/indexer/src/verify/mod.rs`        |
| Check trait & types | `crates/indexer/src/verify/checks.rs`     |
| API checks (F+S)    | `crates/indexer/src/verify/api_checks.rs` |
| Explorer checks (X) | `crates/indexer/src/verify/explorer.rs`   |
| Report rendering    | `crates/indexer/src/verify/report.rs`     |
| LCG sampler         | `crates/indexer/src/verify/sampling.rs`   |

---

## Bench

Per-endpoint baseline benchmarking. Runs each API endpoint N times and reports latency percentiles, error rates, and throughput. Used for regression detection.

### Quick Start

```bash
make bench                                         # All endpoints, report saved to test_outputs/bench/
make bench BENCH_ARGS="--module blocks"             # Only blocks endpoints
make bench BENCH_ARGS="--risk high"                 # Only high-risk endpoints
make bench-json                                     # JSON output to stdout
make bench-baseline                                 # Save baseline for regression comparison
```

### How It Works

1. **Discovery** -- probes the API to find real parameters (block numbers, tx hashes, lock hashes, token type hashes, etc.)
2. **Resolution** -- resolves each endpoint template into a concrete URL using discovered params
3. **Benchmark** -- runs each endpoint sequentially: warmup requests (discarded), then measured iterations
4. **Report** -- per-endpoint latency percentiles (p50/p95/p99), error rates, throughput

### Endpoint Classification

131 endpoints across 18 modules, each classified by:

- **Risk Tier**: Low, Medium, High -- reflects expected query cost
- **Read Pattern**: KeyLookup, BatchLookup, PrefixScan, RangeScan, FullCfScan, CrossStore, RpcDependent, Cached, Aggregation

### CLI Reference

```
ckbadger-bench [bench] [OPTIONS]

OPTIONS:
  --api-url <URL>          API base URL [default: http://localhost:8101/api/v1]
  --frontend-url <URL>     Frontend URL [default: http://localhost:8100]
  --iterations <N>         Requests per endpoint [default: 10]
  --concurrency <N>        Concurrent requests [default: 1]
  --warmup <N>             Warmup requests, not measured [default: 2]
  --timeout-ms <MS>        Per-request timeout [default: 10000]
  --module <NAME>          Filter by module name
  --endpoint <PATH>        Filter by endpoint path template
  --risk <TIER>            Filter by risk tier: high, medium, low
  --json                   Output JSON instead of table
  --output <FILE>          Save JSON report to file
  --output-dir <DIR>       Auto-save timestamped JSON reports
  --compare <FILE>         Compare against baseline JSON file
  --discovery-only         Print discovered params and exit
```

### Regression Detection

```bash
# Save a baseline
make bench-baseline

# Later, compare against it
make bench BENCH_ARGS="--compare bench-baseline.json"
```

Thresholds: regression > 20% slower (p95), improvement > 10% faster.

### File Locations

| What                  | Where                                          |
| --------------------- | ---------------------------------------------- |
| CLI & main            | `crates/bench/src/main.rs`                     |
| Endpoint registry     | `crates/bench/src/registry.rs`                 |
| Execution engine      | `crates/bench/src/runner.rs`                   |
| Metrics (percentiles) | `crates/bench/src/metrics.rs`                  |
| Report & comparison   | `crates/bench/src/report.rs`                   |
| Discovery             | `crates/bench/src/discovery.rs`                |
| Endpoint definitions  | `crates/bench/src/endpoints/*.rs` (18 modules) |

---

## Stress

Concurrent load testing with staged ramp-up. Finds how much traffic the system can handle, what breaks first, and at what concurrency level.

### Quick Start

```bash
make stress                                                    # Mixed scenario, auto-saved to test_outputs/stress/
make stress STRESS_ARGS="--scenario api --auto-ramp"           # Pure API throughput, auto-ramp until breakage
make stress STRESS_ARGS="--scenario heavy --stages 10,50,100"  # Heavy queries, fixed stages
make stress STRESS_ARGS="--remote-host 192.168.1.100"          # Test a remote instance
```

### How It Works

1. **Discovery** -- same as bench, probes API for real parameters
2. **Resolution** -- resolves endpoints into concrete URLs
3. **Staged ramp-up** -- for each stage, spawns N virtual users (VUs) as tokio tasks
4. **VU loop** -- each VU loops: pick weighted random endpoint, execute request, send sample to collector, optional think time
5. **Collection** -- dedicated collector task aggregates samples per stage, prints real-time status line
6. **Degradation detection** -- compares each stage against baseline (stage 1), detects soft degradation and hard failure
7. **Report** -- stage summary table, endpoint breakdown, read pattern summary

### Scenarios

| Scenario  | Endpoints                   | Weight strategy                                                                                              | Think time | Frontend |
| --------- | --------------------------- | ------------------------------------------------------------------------------------------------------------ | ---------- | -------- |
| **mixed** | All API + 7 frontend routes | By page type (homepage 25%, blocks 20%, transactions 20%, addresses 15%, assets 10%, other 10%, frontend 5%) | 50-200ms   | Yes      |
| **heavy** | Only High-risk              | By ReadPattern danger (FullCfScan 4, CrossStore 4, RangeScan 3, PrefixScan 2, other 1)                       | None       | No       |
| **api**   | All API                     | Uniform                                                                                                      | None       | No       |

Scenarios can be combined: `--scenario mixed,heavy,api` runs all three sequentially.

### Ramp-Up Modes

**Fixed stages** (default): `--stages 10,25,50,100,200,300` runs each VU count for `--stage-duration` seconds.

**Auto-ramp**: `--auto-ramp` starts at 10 VUs, doubles each stage (10, 20, 40, 80, 160, 320, ...), stops when hard failure is detected.

### Degradation Detection

Based on stage 1 (baseline) metrics:

| Signal           | Threshold             | Meaning                  |
| ---------------- | --------------------- | ------------------------ |
| Soft degradation | p95 > 2x baseline p95 | Latency deteriorating    |
| Errors emerging  | error rate > 1%       | Failures appearing       |
| Hard failure     | error rate > 10%      | Service effectively down |

Auto-ramp records soft degradation but continues. Stops at hard failure.

### Report Output

Three tables per scenario:

**Stage summary** -- per-stage RPS, latency percentiles, error rate, status:

```
  Stage   VUs    Dur(s)  RPS      p50      p95      p99      Err%     Status
  -----------------------------------------------------------------------
  1       10     30      120      12ms     25ms     40ms     0.0%     baseline
  2       25     30      285      14ms     30ms     55ms     0.0%     ok
  3       100    30      780      35ms     120ms    250ms    0.2%     ⚠ soft degradation
  4       300    18      620      200ms    2500ms   5000ms   15.2%    ✖ breaking point
```

**Endpoint breakdown** -- last stable stage vs breaking stage, per endpoint:

```
  Endpoint                        Pattern         Stable p95   Break p95    Verdict
  ----------------------------------------------------------------------------------
  /addresses/{addr}/activities    RangeScan       85ms         3200ms       ✖ first to break
  /tokens/{hash}/holders          FullCfScan      120ms        4500ms       ✖ critical
  /blocks/{id}                    KeyLookup       15ms         45ms         ok
```

**Read pattern summary** -- aggregated by ReadPattern:

```
  ReadPattern     Endpoints  Avg p95 @stable  Avg p95 @break  Degradation
  -----------------------------------------------------------------------
  FullCfScan            7          90ms             3000ms       33.3×  ✖
  CrossStore            5          80ms             2200ms       27.5×  ✖
  Cached               14           5ms               12ms        2.4×
```

Reports are auto-saved as JSON to `test_outputs/stress/` when run via `make stress`.

### CLI Reference

```
ckbadger-bench stress [OPTIONS]

OPTIONS:
  --scenario <SCENARIO>        mixed, heavy, api, or comma-separated [default: mixed]
  --stages <VU_LIST>           Comma-separated VU counts per stage [default: 10,25,50,100,200,300]
  --stage-duration <SECS>      Duration per stage in seconds [default: 30]
  --auto-ramp                  Auto-increase VUs until hard failure (ignores --stages)
  --think-time-ms <RANGE>      Think time range for mixed scenario [default: 50-200]
  --remote-host <HOST>         Remote ckbadger host (expands to api-url + frontend-url)
  --api-url <URL>              API base URL [default: http://localhost:8101/api/v1]
  --frontend-url <URL>         Frontend base URL [default: http://localhost:8100]
  --timeout-ms <MS>            Per-request timeout [default: 10000]
  --warmup-duration <SECS>     Warmup before stage 1 [default: 5]
  --json                       Output JSON to stdout
  --output <FILE>              Save JSON report to file
  --output-dir <DIR>           Auto-save timestamped JSON reports
```

`--remote-host` and `--api-url`/`--frontend-url` are mutually exclusive.

### File Locations

| What                 | Where                                  |
| -------------------- | -------------------------------------- |
| Stage scheduler      | `crates/bench/src/stress/mod.rs`       |
| Scenario definitions | `crates/bench/src/stress/scenario.rs`  |
| Sample collector     | `crates/bench/src/stress/collector.rs` |
| VU loop              | `crates/bench/src/stress/vu.rs`        |
| Report output        | `crates/bench/src/stress/report.rs`    |

---

## Makefile Targets

```bash
make verify                    # ckbadger verify --depth fast
make bench                     # All endpoints, auto-save to test_outputs/bench/
make bench-json                # JSON output to stdout
make bench-baseline            # Save baseline to bench-baseline.json
make stress                    # Mixed scenario, auto-save to test_outputs/stress/
```

Reports are saved to `test_outputs/` (gitignored).
