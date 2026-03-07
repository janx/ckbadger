# Bulk Sync Workload Samples Design

## Goal

Make bulk-sync perf artifacts comparable across runs when batch tx/cell/input density differs, without changing the canonical sync path or introducing a second calculation path.

## Problem

Current `samples.jsonl` batch samples only record:

- `blocks`
- `batch_seconds`
- `commit_ms`
- RocksDB pressure counters

That is enough for coarse wall-clock tracking, but not enough to answer:

- whether a run is slower because batches got smaller
- whether a run is slower because parser work per tx got worse
- whether a run is slower because writer work per cell/input got worse

Because batch workload varies materially across runs, comparing only `avg_batch_seconds` or `avg_commit_ms` is not valid.

## Constraints

- Bulk sync stays single-shot and hot-path oriented per [docs/prompts/BULK_SYNC.md](/home/f0rk/projects/ckbadger/docs/prompts/BULK_SYNC.md)
- No new fallback calculation chains
- Reuse existing parser and writer timing values; do not recalculate the same metric in two places
- Keep `metrics.env` and `report.md` unchanged in this step

## Recommended Approach

Extend the existing batch sample schema in `samples.jsonl` with workload and hot-path timing fields:

- workload:
  - `txs`
  - `cells`
  - `inputs`
- parser:
  - `parse_ms`
  - `precompute_ms`
  - `nft_precompute_ms`
- writer:
  - `write_ms`
  - `commit_ms` (already present)
  - `t1_ms`
  - `t_act_ms`

This preserves a single perf sample per batch and keeps all later analysis aligned on one batch identity.

## Data Ownership

- Parser metrics come from the parser stage in `pipeline.rs`
- Writer metrics come from `write_parsed_batch()` / `BatchWriteMetrics`
- Workload counts come from the batch currently being written; they are already computed for logging and should be reused

## Why Not a Second Sample File

Creating a separate `batch_workload_samples.jsonl` would duplicate batch identity and make later analysis depend on joining two files. That adds complexity without any benefit for the current problem.

## Compatibility

- Old runs remain readable; they just lack the new fields
- This step does not change aggregate perf reports
- Later work can add normalized rollups to `metrics.env` / `report.md` once the raw sample shape is proven useful

## Success Criteria

After the next fresh-db run, we can compute:

- `write_ms / tx`
- `write_ms / cell`
- `commit_ms / block`
- `t1_ms / input`
- `parse_ms / tx`
- `nft_precompute_ms / tx`

without scraping runtime logs.
