# Performance Benchmark Results

> Last updated: 2026-02-20
>
> This document is the source of truth for `Unrivaled Speed` measurement in the RocksDB-based
> architecture. Historical PostgreSQL numbers are retained only for context.

## Goal

Track performance using a reproducible localhost protocol and publish comparable benchmark artifacts
for every performance-affecting change.

## Current Architecture Baseline (RocksDB)

All current benchmarks must target:

- `ckbadger-store` (embedded RocksDB)
- `ckbadger-indexer` pipeline mode
- `ckbadger-api` `/api/v1` endpoints

## Measurement Protocol

### 1) Sync Throughput

Prerequisites:

- `CKBADGER_DATA_PATH` points to the benchmark database
- indexer logs are written to `/tmp/ckbadger-indexer.log`

Commands:

```bash
# Example: start indexer and tee logs
CKBADGER_DATA_PATH=./data/ckbadger-store \
  cargo run -p ckbadger-indexer 2>&1 | tee /tmp/ckbadger-indexer.log

# Run sync benchmark monitor (quick mode)
CKBADGER_DATA_PATH=./data/ckbadger-store \
  ./scripts/benchmark_sync.sh --quick --output-dir artifacts/perf
```

Artifacts:

- `artifacts/perf/benchmark_sync_<timestamp>.csv`
- `artifacts/perf/benchmark_sync_<timestamp>.md`

### 2) API Latency (p50/p95/p99)

Commands:

```bash
API_URL=http://localhost:3001 ./scripts/run-load-tests.sh quick --output-dir artifacts/perf
```

Artifacts:

- `artifacts/perf/load_test_<timestamp>.log`
- `artifacts/perf/load_test_<timestamp>_k6_summary.json` (when k6 tests run)
- `artifacts/perf/load_test_<timestamp>.md`

### 3) Correctness Guardrail

Commands:

```bash
cargo run -p ckbadger-indexer -- verify --depth fast
# For DAO/supply/aggregate changes:
cargo run -p ckbadger-indexer -- verify --depth sampling
```

Requirement:

- performance optimization is not accepted if verify checks regress.

## Reporting Template (Fill Per Run)

| Metric                | Result | Target                                   | Notes                            |
| --------------------- | ------ | ---------------------------------------- | -------------------------------- |
| Sync throughput (EMA) | TODO   | maximize without correctness regressions | include block range and hardware |
| API latency p50       | TODO   | <= 10ms                                  | warm cache                       |
| API latency p95       | TODO   | <= 50ms                                  | warm cache                       |
| API latency p99       | TODO   | <= 100ms                                 | warm cache                       |
| Verify failures       | TODO   | 0                                        | include `fast` / `sampling`      |

## CI / Nightly

Nightly performance smoke runs are defined in `.github/workflows/perf-nightly.yml`.

- Trigger: schedule + manual dispatch
- Output: `artifacts/perf/*` uploaded as workflow artifacts
- Recommended secret: `PERF_API_URL` (target API base URL)

## Legacy Benchmarks (PostgreSQL Era)

The previous benchmark set (2026-01-28, PostgreSQL/COPY bottleneck) is no longer representative of
current architecture and should not be used for decision making.

Legacy snapshot summary:

- Reported range: ~1500-2800 blocks/sec
- Primary bottleneck: PostgreSQL COPY/write path
- Status: historical reference only
