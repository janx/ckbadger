# Performance Testing Framework

> Comprehensive performance testing strategy for ckbadger to prevent regression and enable continuous optimization.

## Goals

1. **Prevent Regression**: Detect performance degradation in API and indexer before merge
2. **Enable Optimization**: Provide actionable metrics for continuous improvement
3. **Track History**: Build performance history for trend analysis

## Architecture Overview

```
┌─────────────────────────────────────────────────────────────────────┐
│                     Performance Testing Layers                       │
├─────────────────────────────────────────────────────────────────────┤
│  Layer 4: Continuous Monitoring (Prometheus + Grafana)              │
│           └── Runtime metrics, alerts, dashboards                   │
├─────────────────────────────────────────────────────────────────────┤
│  Layer 3: Load Testing (k6)                                         │
│           └── API endpoints under load, concurrency, latency P99    │
├─────────────────────────────────────────────────────────────────────┤
│  Layer 2: Integration Benchmarks (Criterion)                        │
│           └── Pipeline throughput, batch processing, reorg handling │
├─────────────────────────────────────────────────────────────────────┤
│  Layer 1: Micro-benchmarks (Criterion)                              │
│           └── Parsers, serialization, hashing, cache operations     │
└─────────────────────────────────────────────────────────────────────┘
```

---

## Layer 1: Micro-benchmarks (Rust Criterion)

### Purpose

Measure performance of individual functions that are on the critical path.

### Setup

```toml
# crates/indexer/Cargo.toml
[dev-dependencies]
criterion = { version = "0.5", features = ["html_reports"] }

[[bench]]
name = "parsers"
harness = false

[[bench]]
name = "cache"
harness = false
```

### Benchmark Targets

| Component              | File                    | Key Functions           | Target         |
| ---------------------- | ----------------------- | ----------------------- | -------------- |
| **Block Parser**       | `parser/block.rs`       | `parse_block()`         | < 100μs/block  |
| **Transaction Parser** | `parser/transaction.rs` | `parse_transaction()`   | < 10μs/tx      |
| **Cell Parser**        | `parser/cell.rs`        | `parse_cell_output()`   | < 5μs/cell     |
| **DAO Parser**         | `parser/dao.rs`         | `parse_dao_deposit()`   | < 10μs/deposit |
| **Activity Parser**    | `parser/activity.rs`    | `parse_activities()`    | < 50μs/tx      |
| **Spore Parser**       | `parser/spore.rs`       | `parse_spore_cell()`    | < 20μs/cell    |
| **Script Hashing**     | `parser/script.rs`      | `compute_script_hash()` | < 2μs/script   |
| **LRU Cache**          | `cache/cell_cache.rs`   | `get()`, `insert()`     | < 100ns/op     |

### Example Benchmark

```rust
// crates/indexer/benches/parsers.rs
use criterion::{black_box, criterion_group, criterion_main, Criterion, Throughput};
use ckbadger_indexer::parser::block::parse_block;

fn bench_block_parser(c: &mut Criterion) {
    let raw_block = include_str!("../fixtures/block_sample.json");
    let block: ckb_jsonrpc_types::BlockView = serde_json::from_str(raw_block).unwrap();

    let mut group = c.benchmark_group("block_parser");
    group.throughput(Throughput::Elements(1));

    group.bench_function("parse_block", |b| {
        b.iter(|| parse_block(black_box(&block)))
    });

    group.finish();
}

fn bench_transaction_parser(c: &mut Criterion) {
    let raw_tx = include_str!("../fixtures/transaction_sample.json");
    let tx: ckb_jsonrpc_types::TransactionView = serde_json::from_str(raw_tx).unwrap();

    let mut group = c.benchmark_group("transaction_parser");
    group.throughput(Throughput::Elements(1));

    group.bench_function("parse_transaction", |b| {
        b.iter(|| parse_transaction(black_box(&tx), 12345, 0))
    });

    group.finish();
}

criterion_group!(benches, bench_block_parser, bench_transaction_parser);
criterion_main!(benches);
```

### Running Benchmarks

```bash
# Run all benchmarks
cargo bench -p ckbadger-indexer

# Run specific benchmark group
cargo bench -p ckbadger-indexer -- block_parser
cargo bench -p ckbadger-indexer -- cache
cargo bench -p ckbadger-indexer -- pipeline
cargo bench -p ckbadger-indexer -- batch_data
cargo bench -p ckbadger-indexer -- row_construction

# Generate HTML report
cargo bench -p ckbadger-indexer -- --save-baseline main
# After changes:
cargo bench -p ckbadger-indexer -- --baseline main
```

---

## Layer 2: Integration Benchmarks

### Purpose

Measure end-to-end performance of major subsystems.

### Benchmark Targets

| Subsystem    | Scenario                 | Metric              | Target |
| ------------ | ------------------------ | ------------------- | ------ |
| **Pipeline** | Bulk sync (10k blocks)   | blocks/sec          | > 3000 |
| **Pipeline** | Near-tip sync            | blocks/sec          | > 500  |
| **Pipeline** | With reorg               | reorg handling time | < 5s   |
| **Writer**   | Batch write (10k blocks) | write time          | < 3s   |
| **Cache**    | Cold start (1M lookups)  | miss rate           | < 5%   |
| **Cache**    | Warm (1M lookups)        | hit rate            | > 95%  |

### Example Integration Benchmark

```rust
// crates/indexer/benches/pipeline.rs
use criterion::{criterion_group, criterion_main, Criterion, BenchmarkId};
use std::time::Duration;

fn bench_batch_write(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();

    // Setup: Create mock blocks
    let blocks = generate_mock_blocks(1000);

    let mut group = c.benchmark_group("batch_write");
    group.measurement_time(Duration::from_secs(30));
    group.sample_size(10);

    for batch_size in [100, 500, 1000, 5000, 10000] {
        group.bench_with_input(
            BenchmarkId::new("blocks", batch_size),
            &batch_size,
            |b, &size| {
                b.to_async(&rt).iter(|| async {
                    write_batch(&blocks[..size]).await
                })
            },
        );
    }

    group.finish();
}

criterion_group!(benches, bench_batch_write);
criterion_main!(benches);
```

---

## Layer 3: API Load Testing (k6)

### Purpose

Test API performance under realistic load conditions.

### Setup

```bash
# Install k6
brew install k6  # macOS
# or: https://k6.io/docs/get-started/installation/
```

### Test Scenarios

| Scenario   | Endpoints | VUs     | Duration | SLO                  |
| ---------- | --------- | ------- | -------- | -------------------- |
| **Smoke**  | All       | 1       | 30s      | P99 < 200ms          |
| **Load**   | Critical  | 50      | 5m       | P99 < 500ms          |
| **Stress** | Critical  | 200     | 10m      | P99 < 1s, error < 1% |
| **Spike**  | Homepage  | 0→500→0 | 2m       | Recovery < 30s       |

### Critical Endpoints (Priority Order)

1. `GET /api/v1/blocks` - Block list (homepage)
2. `GET /api/v1/blocks/{id}` - Block detail
3. `GET /api/v1/transactions/{hash}` - Transaction detail
4. `GET /api/v1/statistics/network` - Network stats
5. `GET /api/v1/mempool/summary` - Mempool (real-time)
6. `GET /api/v1/charts/transaction-count` - Chart (heavy)

### k6 Test Script

```javascript
// perf/k6/api-load-test.js
import http from 'k6/http';
import { check, sleep } from 'k6';
import { Rate, Trend } from 'k6/metrics';

const errorRate = new Rate('errors');
const blockListLatency = new Trend('block_list_latency');
const blockDetailLatency = new Trend('block_detail_latency');
const txDetailLatency = new Trend('tx_detail_latency');
const chartLatency = new Trend('chart_latency');

const BASE_URL = __ENV.API_URL || 'http://localhost:3001/api/v1';

// Known test data (from synced database)
const TEST_BLOCK = 1000000;
const TEST_TX_HASH = '0x...'; // Real tx hash from DB

export const options = {
  scenarios: {
    // Smoke test: 1 user, 30 seconds
    smoke: {
      executor: 'constant-vus',
      vus: 1,
      duration: '30s',
      exec: 'smokeTest',
    },
    // Load test: ramp up to 50 users
    load: {
      executor: 'ramping-vus',
      startVUs: 0,
      stages: [
        { duration: '1m', target: 25 },
        { duration: '3m', target: 50 },
        { duration: '1m', target: 0 },
      ],
      exec: 'loadTest',
    },
  },
  thresholds: {
    errors: ['rate<0.01'], // Error rate < 1%
    http_req_duration: ['p(99)<500'], // P99 < 500ms
    block_list_latency: ['p(95)<100', 'p(99)<200'],
    block_detail_latency: ['p(95)<50', 'p(99)<100'],
    chart_latency: ['p(95)<500', 'p(99)<1000'],
  },
};

export function smokeTest() {
  // Test all endpoints once
  const endpoints = [
    { name: 'blocks', path: '/blocks?limit=10' },
    { name: 'block_detail', path: `/blocks/${TEST_BLOCK}` },
    { name: 'network_stats', path: '/statistics/network' },
    { name: 'tx_stats', path: '/statistics/tx-stats' },
    { name: 'chart', path: '/charts/transaction-count' },
    { name: 'mempool', path: '/mempool/summary' },
  ];

  for (const ep of endpoints) {
    const res = http.get(`${BASE_URL}${ep.path}`);
    check(res, {
      [`${ep.name} status 200`]: (r) => r.status === 200,
      [`${ep.name} response time < 500ms`]: (r) => r.timings.duration < 500,
    });
    errorRate.add(res.status !== 200);
    sleep(0.5);
  }
}

export function loadTest() {
  // Weighted distribution of requests (realistic traffic pattern)
  const rand = Math.random();

  if (rand < 0.4) {
    // 40%: Block list (homepage)
    const res = http.get(`${BASE_URL}/blocks?limit=10`);
    blockListLatency.add(res.timings.duration);
    errorRate.add(res.status !== 200);
  } else if (rand < 0.6) {
    // 20%: Block detail
    const blockNum = Math.floor(Math.random() * 1000000) + 1;
    const res = http.get(`${BASE_URL}/blocks/${blockNum}`);
    blockDetailLatency.add(res.timings.duration);
    errorRate.add(res.status !== 200);
  } else if (rand < 0.75) {
    // 15%: Network stats
    const res = http.get(`${BASE_URL}/statistics/network`);
    errorRate.add(res.status !== 200);
  } else if (rand < 0.85) {
    // 10%: Mempool
    const res = http.get(`${BASE_URL}/mempool/summary`);
    errorRate.add(res.status !== 200);
  } else {
    // 15%: Charts (heavy)
    const charts = ['transaction-count', 'cell-count', 'hash-rate', 'average-block-time'];
    const chart = charts[Math.floor(Math.random() * charts.length)];
    const res = http.get(`${BASE_URL}/charts/${chart}`);
    chartLatency.add(res.timings.duration);
    errorRate.add(res.status !== 200);
  }

  sleep(Math.random() * 2); // Random sleep 0-2s
}

export function handleSummary(data) {
  return {
    'perf/results/api-load-test.json': JSON.stringify(data, null, 2),
  };
}
```

### Running Load Tests

```bash
# Run smoke test only
k6 run --env API_URL=http://localhost:3001/api/v1 perf/k6/api-load-test.js --scenario smoke

# Run full load test
k6 run --env API_URL=http://localhost:3001/api/v1 perf/k6/api-load-test.js

# Run with Prometheus output (for Grafana)
k6 run --out experimental-prometheus-rw perf/k6/api-load-test.js
```

---

## Layer 4: Continuous Monitoring (Prometheus + Grafana)

### Purpose

Real-time visibility into production performance.

### Metrics to Export

#### API Metrics

```rust
// crates/api/src/metrics.rs
use prometheus::{
    register_histogram_vec, register_counter_vec, register_gauge,
    HistogramVec, CounterVec, Gauge,
};
use lazy_static::lazy_static;

lazy_static! {
    // Request latency by endpoint
    pub static ref HTTP_REQUEST_DURATION: HistogramVec = register_histogram_vec!(
        "http_request_duration_seconds",
        "HTTP request duration in seconds",
        &["method", "endpoint", "status"],
        vec![0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0]
    ).unwrap();

    // Request count by endpoint
    pub static ref HTTP_REQUEST_TOTAL: CounterVec = register_counter_vec!(
        "http_requests_total",
        "Total HTTP requests",
        &["method", "endpoint", "status"]
    ).unwrap();

    // Cache hit rate
    pub static ref CACHE_HIT_TOTAL: CounterVec = register_counter_vec!(
        "cache_hits_total",
        "Cache hits",
        &["cache_name"]
    ).unwrap();

    pub static ref CACHE_MISS_TOTAL: CounterVec = register_counter_vec!(
        "cache_misses_total",
        "Cache misses",
        &["cache_name"]
    ).unwrap();

    // Active WebSocket connections
    pub static ref WS_CONNECTIONS: Gauge = register_gauge!(
        "websocket_connections_active",
        "Number of active WebSocket connections"
    ).unwrap();
}
```

#### Indexer Metrics

```rust
// crates/indexer/src/metrics.rs
lazy_static! {
    // Sync progress
    pub static ref SYNC_CURRENT_BLOCK: Gauge = register_gauge!(
        "indexer_sync_current_block",
        "Current synced block number"
    ).unwrap();

    pub static ref SYNC_TARGET_BLOCK: Gauge = register_gauge!(
        "indexer_sync_target_block",
        "Target block number (chain tip)"
    ).unwrap();

    // Throughput
    pub static ref BLOCKS_PROCESSED_TOTAL: Counter = register_counter!(
        "indexer_blocks_processed_total",
        "Total blocks processed"
    ).unwrap();

    pub static ref SYNC_RATE_EMA: Gauge = register_gauge!(
        "indexer_sync_rate_ema",
        "Exponential moving average of blocks/second"
    ).unwrap();

    // Pipeline stage timings
    pub static ref PIPELINE_STAGE_DURATION: HistogramVec = register_histogram_vec!(
        "indexer_pipeline_stage_duration_seconds",
        "Duration of each pipeline stage",
        &["stage"],  // fetcher, parser, writer
        vec![0.01, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0]
    ).unwrap();

    // Cell cache
    pub static ref CELL_CACHE_SIZE: Gauge = register_gauge!(
        "indexer_cell_cache_size",
        "Number of cells in LRU cache"
    ).unwrap();

    pub static ref CELL_CACHE_HIT_RATE: Gauge = register_gauge!(
        "indexer_cell_cache_hit_rate",
        "Cell cache hit rate (0.0-1.0)"
    ).unwrap();

    // Batch write timing
    pub static ref BATCH_WRITE_DURATION: Histogram = register_histogram!(
        "indexer_batch_write_duration_seconds",
        "Time to write a batch to ClickHouse",
        vec![0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0, 30.0]
    ).unwrap();
}
```

### Grafana Dashboard

```json
// perf/grafana/dashboards/ckbadger.json
{
  "title": "ckbadger Performance",
  "panels": [
    {
      "title": "API Request Latency P99",
      "type": "timeseries",
      "targets": [
        {
          "expr": "histogram_quantile(0.99, rate(http_request_duration_seconds_bucket[5m]))"
        }
      ]
    },
    {
      "title": "Sync Rate (blocks/sec)",
      "type": "stat",
      "targets": [
        {
          "expr": "indexer_sync_rate_ema"
        }
      ]
    },
    {
      "title": "Sync Progress",
      "type": "gauge",
      "targets": [
        {
          "expr": "indexer_sync_current_block / indexer_sync_target_block * 100"
        }
      ]
    },
    {
      "title": "Cache Hit Rate",
      "type": "gauge",
      "targets": [
        {
          "expr": "rate(cache_hits_total[5m]) / (rate(cache_hits_total[5m]) + rate(cache_misses_total[5m]))"
        }
      ]
    },
    {
      "title": "Pipeline Stage Duration",
      "type": "heatmap",
      "targets": [
        {
          "expr": "histogram_quantile(0.95, rate(indexer_pipeline_stage_duration_seconds_bucket[5m]))"
        }
      ]
    }
  ]
}
```

---

## CI Integration: Performance Regression Detection

### GitHub Actions Workflow

````yaml
# .github/workflows/perf.yml
name: Performance Tests

on:
  push:
    branches: [main]
  pull_request:
    branches: [main]
    paths:
      - 'crates/**'
      - 'Cargo.toml'
      - 'Cargo.lock'

env:
  CARGO_TERM_COLOR: always

jobs:
  benchmarks:
    name: Run Benchmarks
    runs-on: ubuntu-latest

    steps:
      - uses: actions/checkout@v4

      - name: Install Rust toolchain
        uses: dtolnay/rust-toolchain@stable

      - name: Cache cargo
        uses: actions/cache@v4
        with:
          path: |
            ~/.cargo/registry
            ~/.cargo/git
            target
          key: ${{ runner.os }}-cargo-bench-${{ hashFiles('**/Cargo.lock') }}

      - name: Download baseline (if exists)
        uses: dawidd6/action-download-artifact@v3
        with:
          workflow: perf.yml
          branch: main
          name: criterion-baseline
          path: target/criterion
        continue-on-error: true

      - name: Run benchmarks
        run: cargo bench -p ckbadger-indexer -- --save-baseline current

      - name: Compare with baseline
        if: github.event_name == 'pull_request'
        run: |
          cargo bench -p ckbadger-indexer -- --baseline current --load-baseline main \
            | tee benchmark-comparison.txt

          # Check for significant regressions (>10%)
          if grep -q "regressed" benchmark-comparison.txt; then
            echo "::warning::Performance regression detected!"
            # Optionally fail the build:
            # exit 1
          fi

      - name: Upload benchmark results
        uses: actions/upload-artifact@v4
        with:
          name: criterion-baseline
          path: target/criterion
          retention-days: 30

      - name: Comment PR with results
        if: github.event_name == 'pull_request'
        uses: actions/github-script@v7
        with:
          script: |
            const fs = require('fs');
            const comparison = fs.readFileSync('benchmark-comparison.txt', 'utf8');
            github.rest.issues.createComment({
              owner: context.repo.owner,
              repo: context.repo.repo,
              issue_number: context.issue.number,
              body: '## Benchmark Results\n```\n' + comparison + '\n```'
            });

  load-test:
    name: API Load Test
    runs-on: ubuntu-latest
    needs: benchmarks
    if: github.ref == 'refs/heads/main' # Only on main

    services:
      clickhouse:
        image: clickhouse/clickhouse-server:24.1
        ports:
          - 8123:8123
      redis:
        image: redis:7-alpine
        ports:
          - 6379:6379

    steps:
      - uses: actions/checkout@v4

      - name: Install k6
        run: |
          curl -s https://dl.k6.io/key.gpg | sudo apt-key add -
          echo "deb https://dl.k6.io/deb stable main" | sudo tee /etc/apt/sources.list.d/k6.list
          sudo apt-get update
          sudo apt-get install k6

      - name: Build and start API
        run: |
          cargo build -p ckbadger-api --release
          ./target/release/ckbadger-api &
          sleep 5
        env:
          CLICKHOUSE_URL: http://localhost:8123
          REDIS_URL: redis://localhost:6379

      - name: Run load test (smoke)
        run: k6 run --env API_URL=http://localhost:3001/api/v1 perf/k6/api-load-test.js --scenario smoke

      - name: Upload results
        uses: actions/upload-artifact@v4
        with:
          name: load-test-results
          path: perf/results/
          retention-days: 30
````

---

## Performance Baselines

### Indexer Targets

| Metric                     | Minimum | Target | Excellent |
| -------------------------- | ------- | ------ | --------- |
| Bulk sync (blocks/sec)     | 2000    | 3500   | 5000+     |
| Near-tip sync (blocks/sec) | 100     | 300    | 500+      |
| Batch write (10k blocks)   | 5s      | 3s     | 1s        |
| Cell cache hit rate        | 90%     | 95%    | 98%+      |
| Memory usage (1M cache)    | 400MB   | 200MB  | 150MB     |

### API Targets

| Endpoint           | P50   | P95   | P99   |
| ------------------ | ----- | ----- | ----- |
| Block list         | 10ms  | 50ms  | 100ms |
| Block detail       | 5ms   | 20ms  | 50ms  |
| Transaction detail | 10ms  | 50ms  | 100ms |
| Network stats      | 5ms   | 20ms  | 50ms  |
| Charts (30-day)    | 100ms | 300ms | 500ms |
| Mempool summary    | 20ms  | 100ms | 200ms |

### Throughput Targets (at 50 VUs)

| Metric       | Minimum | Target  |
| ------------ | ------- | ------- |
| Requests/sec | 500     | 1000+   |
| Error rate   | < 1%    | < 0.1%  |
| P99 latency  | < 1s    | < 500ms |

---

## Directory Structure

```
ckbadger/
├── crates/
│   ├── indexer/
│   │   ├── benches/
│   │   │   ├── parsers.rs        # Parser micro-benchmarks
│   │   │   ├── cache.rs          # LRU cache benchmarks
│   │   │   └── pipeline.rs       # Integration benchmarks
│   │   └── fixtures/
│   │       ├── block_sample.json
│   │       └── transaction_sample.json
│   └── api/
│       ├── src/metrics.rs        # Prometheus metrics
│       └── benches/
│           └── handlers.rs       # Handler benchmarks
├── perf/
│   ├── k6/
│   │   ├── api-load-test.js      # Main load test
│   │   ├── smoke-test.js         # Quick smoke test
│   │   └── stress-test.js        # Stress test
│   ├── results/                  # Test results (gitignored)
│   └── grafana/
│       └── dashboards/
│           └── ckbadger.json     # Grafana dashboard
├── .github/
│   └── workflows/
│       ├── ci.yml                # Existing CI
│       └── perf.yml              # Performance CI
└── docs/
    └── PERFORMANCE_TESTING.md    # This document
```

---

## Implementation Roadmap

### Phase 1: Foundation ✅ COMPLETED

1. [x] Add `criterion` to indexer Cargo.toml
2. [x] Create parser micro-benchmarks with fixture data (`benches/parsers.rs`)
3. [x] Create cache operation benchmarks (`benches/cache.rs`)
4. [x] Add benchmark targets to AGENTS.md
5. [x] Run initial baselines, document results

### Phase 2: Integration ✅ COMPLETED

1. [x] Create pipeline integration benchmarks (`benches/pipeline.rs`)
2. [x] Add batch write benchmarks (BatchData generation, serialization)
3. [x] Set up k6 load testing scripts (`perf/k6/api-load-test.js`)
4. [x] Create smoke test for CI (`perf/k6/smoke-test.js`)
5. [x] Add performance job to CI workflow (`.github/workflows/perf.yml`)

### Phase 3: Monitoring (Week 5-6)

1. [ ] Add Prometheus metrics to API
2. [ ] Add Prometheus metrics to indexer
3. [ ] Create Grafana dashboard
4. [ ] Set up alerting rules
5. [ ] Document SLOs and runbooks

### Phase 4: Continuous Improvement (Ongoing)

1. [ ] Weekly benchmark runs on main
2. [ ] PR comments with performance diff
3. [ ] Track historical trends
4. [ ] Optimize based on data
5. [ ] Update baselines as improvements land

---

## Quick Start Commands

```bash
# Run micro-benchmarks
cargo bench -p ckbadger-indexer

# Run specific benchmark
cargo bench -p ckbadger-indexer -- block_parser

# Compare with baseline
cargo bench -p ckbadger-indexer -- --baseline main

# Run k6 smoke test
k6 run perf/k6/api-load-test.js --scenario smoke

# Run full load test
k6 run perf/k6/api-load-test.js

# View Criterion HTML report
open target/criterion/report/index.html
```

---

## Related Documentation

- [AGENTS.md](../AGENTS.md) - Development guidelines and testing requirements
- [INDEXER_PIPELINE.md](./INDEXER_PIPELINE.md) - Pipeline architecture
- [DAO_CALCULATIONS.md](./DAO_CALCULATIONS.md) - DAO performance considerations
