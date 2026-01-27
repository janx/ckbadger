# Benchmark Evidence Directory

This directory stores benchmark results and performance test reports for the ClickHouse migration evaluation (Phase 0).

## Structure

- `task-0.2-write-performance.md` - Bulk insert benchmark results (1M rows)
- `task-0.3-query-performance.md` - Query performance comparison (Postgres vs ClickHouse)
- `task-0.4-decision.md` - Final go/no-go decision with evidence summary

## Benchmark Criteria

### Write Performance (Task 0.2)

- Target: 100K+ rows/second sustained throughput
- Test: Insert 1M cell records in batches
- Metrics: rows/sec, total time, memory usage

### Query Performance (Task 0.3)

- Target: <100ms for common queries at 18M block scale
- Test queries:
  1. OutPoint lookup (tx_hash + output_index)
  2. Address balance (lock_script_hash aggregation)
  3. Block range scan (created_at_block BETWEEN)
  4. Type script filter (sUDT token queries)

### Decision Gate (Task 0.4)

- GO: Both write AND query benchmarks meet targets
- NO-GO: Either benchmark fails → consider hybrid architecture
