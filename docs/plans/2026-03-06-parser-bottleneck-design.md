# Parser Bottleneck Optimization Design

## Goal

- Reduce parser-stage stalls around height `~14,280,000` where `precompute_ms` dominates total parser time and drains writer/parse queues.

## Problem Summary

- The current run is not direct-read bulk sync. It is RPC catch-up because `CKB_DATA_PATH` is unset.
- Around height `14,270,001-14,290,000`, parser batch average total time jumps from sub-second to multi-second, with `precompute_ms` dominating.
- Idle warnings show `fetch_queue_depth` remains non-zero while `parse_queue_depth=0` and `writer_queue_depth=0`, which means the parser is busy on an in-flight batch rather than the fetcher starving the pipeline.
- The current parser stage performs a large serial precompute section after `parse_blocks_parallel` and before handing the batch to the writer.

## Evidence

- `temp/run/logs/indexer.log` around `14277134-14280544` shows `precompute_ms="9647.8"` and `total_ms="11649.2"`.
- Idle warnings around the same window show `fetch_queue_depth=Some(6)` and `parse_queue_depth=Some(0)`.
- `crates/indexer/src/sync/pipeline.rs` keeps the heavy precompute path inline in the parser task, from the `t_precompute_parser` start through the handoff to `parse_tx`.
- Adaptive backoff reduces target tx volume, but the actual sub-batch can still exceed the nominal target because sub-batch caps are based on `target_batch_txs * 2` and only account for tx/input counts, not cell-heavy workloads.

## Constraints

- Preserve fail-fast behavior. Do not introduce fallback computation paths.
- Keep a single canonical computation path for derived data.
- Any optimization must preserve DB ownership boundaries: indexer writes only, API remains read-only.
- Changes must be test-backed.

## Approaches

### Approach 1: Instrument First, Then Tighten Batch Split, Then Parallelize Precompute

- Add phase-level parser precompute timing so hot sub-phases are visible in logs.
- Tighten sub-batch planning so parser-facing batches do not significantly overshoot adaptive intent.
- Refactor parser precompute into a dedicated blocking/parallelizable helper and move the heavy CPU work off the async parser task.

Trade-offs:

- Best balance of safety and throughput.
- Produces evidence before changing behavior further.
- Requires moderate refactor but keeps the current computation model intact.

### Approach 2: Only Shrink Adaptive Batch Sizes

- Reduce tx/input caps and inflight limits more aggressively near problematic heights.

Trade-offs:

- Easy to implement.
- Likely lowers throughput globally.
- Treats symptoms rather than root cause.
- Does not address the large serial precompute section.

### Approach 3: Move More Work Back to Writer

- Remove parser precompute responsibilities and let writer recompute more derived data.

Trade-offs:

- Simplifies parser.
- Recreates writer bottlenecks and loses pipeline overlap.
- Conflicts with the current direction of moving expensive precompute ahead of DB I/O.

## Recommendation

- Use Approach 1.
- First expose internal timings so we can see which parser sub-phases dominate.
- Second prevent pathological parser batch sizes from overshooting adaptive control.
- Third move the heavy precompute path into blocking/parallel execution once the phase boundaries are explicit and test-covered.

## Proposed Design

### 1. Add parser precompute phase metrics

- Introduce a lightweight parser precompute timing struct in `crates/indexer/src/sync/pipeline.rs`.
- Record at least:
  - `build_batch_cell_infos_ms`
  - `compute_fee_ms`
  - `cache_and_balance_ms`
  - `spore_precompute_ms`
  - `nft_precompute_ms`
- Log these alongside existing parser batch metrics.

Why:

- Current `precompute_ms` is too coarse to justify targeted optimization.
- We need to distinguish whether the hotspot is generic balance/script aggregation or chain-standard-specific parsing such as Spore / mNFT / DotBit.

### 2. Tighten parser-facing sub-batch planning

- Extend sub-batch planning inputs to account for per-block cell counts in addition to tx/input counts.
- Reduce the parser-facing tx cap so backoff actually constrains parser work.
- Keep the implementation deterministic and fail-fast.

Why:

- The current `adaptive_sub_batch_tx_cap()` can still produce parser batches substantially larger than the adaptive target.
- The hotspot batches are both tx-heavy and cell-heavy; tx/input-only splitting is too weak.

### 3. Offload parser precompute from the async task

- Extract the parser precompute section into a helper that accepts parsed blocks, tx data, cached input cell info, and raw blocks.
- Run that helper via `spawn_blocking` first.
- Structure the helper so the heaviest loops can later be parallelized with rayon if needed.

Why:

- The parser task currently does CPU-heavy work synchronously after `parse_blocks_parallel`.
- Even before full rayonization, isolating this work in a blocking task prevents a long CPU section from monopolizing the async parser loop.

### 4. Remove obvious repeated work inside parser precompute

- Reuse already computed occupied capacity where possible instead of recalculating it in multiple passes.
- Batch metadata/index lookups where the current code performs repeated store calls for cache misses.
- Keep exact semantics unchanged.

Why:

- This should provide incremental wins after instrumentation identifies which repeated work is material.

## Testing Strategy

- Add unit tests for the new sub-batch planner inputs and caps in `crates/indexer/src/sync/adaptive.rs`.
- Add unit tests for any new parser precompute timing helpers in `crates/indexer/src/sync/pipeline.rs` or `crates/indexer/src/sync/diagnostics.rs`.
- Run targeted indexer tests after each task, then a focused crate test pass at the end.

## Rollout Order

1. Add instrumentation.
2. Add and verify tighter sub-batch constraints.
3. Refactor precompute into blocking execution.
4. Re-check logs and compare parser phase timings.

## Success Criteria

- The `14,270,001-14,290,000` style window no longer shows parser `precompute_ms` an order of magnitude above the surrounding range.
- Idle warnings caused by empty parse/write queues while fetch queue remains non-empty are materially reduced.
- No correctness regressions in parser/write tests.
