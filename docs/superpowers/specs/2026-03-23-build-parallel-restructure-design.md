# Build Phase Parallel Restructure + Chrono Cache

## Problem

The bulk-build `apply_blocks` method has a serial critical path that leaves CPU cores idle:

```
facts(295ms) → resolve(142ms) → rayon::join(history‖reducers)(744ms) → activity_stats(245ms) → chain_stats(~150ms) → misc(~110ms)
Total build_ms = 1888ms avg (399 batches, 18.9M blocks, run-20260323T120050)
```

Three serial segments waste time after the parallel `rayon::join` completes:
1. `activity_stats.apply_bundles` (245ms) — only depends on `history.activity_bundles`, not reducers
2. `chain_stats.apply_blocks` (unmeasured, ~150ms) — reads only immutable `arena` + `resolved`
3. `address_reduce` (276ms) — runs serially before 5 independent parallel reducers that don't read address state

Additionally, `activity_stats.apply_bundles` formats chrono date/hour strings per-tx (~123K calls), despite all txs in the same block sharing one timestamp.

## Design

### Change A: activity_stats into LEFT branch

Move `activity_stats.apply_bundles(&history.activity_bundles)` from the serial post-join code (line 1569-1571) into the LEFT branch of the outer `rayon::join`, running immediately after `build_history_rows` returns.

**Dependency satisfied:** `apply_bundles` reads only `history.activity_bundles` (produced by LEFT) and writes only `&mut activity_stats` (disjoint from RIGHT's `&mut owners`).

**Before:**
```
rayon::join(
  LEFT:  history(392ms)
  RIGHT: reducers(744ms)
) → activity_stats(245ms)    ← serial wait
```

**After:**
```
rayon::join(
  LEFT:  history(392ms) → activity_stats(245ms) = 637ms
  RIGHT: reducers(744ms)
) = max(637, 744) = 744ms    ← activity_stats runs free
```

**Saves: ~245ms** (activity_stats fully hidden under RIGHT).

### Change B: chain_stats as independent parallel branch

Move `chain_stats.apply_blocks(&arena, &resolved)` from serial post-join code (line 1574) into a new parallel branch via nested `rayon::join`.

**Dependency satisfied:** `apply_blocks` reads only immutable `&arena` + `&resolved`. Writes only `&mut chain_stats` (disjoint from all other branches).

**Structure:** Outer `rayon::join(LEFT, rayon::join(MIDDLE, RIGHT))` — a 3-way split using nested join.

- LEFT: history → activity_stats (637ms)
- MIDDLE: chain_stats (~150ms)
- RIGHT: reducers

MIDDLE finishes early; its rayon worker returns to the pool and can assist LEFT's `par_iter` or RIGHT's nested joins.

**Saves: ~150ms** (chain_stats fully hidden).

### Change C: address reducer parallel with 5 independent reducers

Currently inside the RIGHT branch, address+cell_dist runs serially **before** the 5 parallel reducers:

```
RIGHT = hodl(~10ms) → address+cell_dist(276ms, serial) → rayon::join(5 reducers)(~438ms)
Total RIGHT ≈ 724ms
```

The 5 reducers (script, token, dao, fiber, object) **do not read** address state or cell_dist_tracker:

| Reducer | Reads | Writes | Depends on address? |
|---------|-------|--------|:---:|
| script | resolved, frozen | script_state | No |
| token | resolved, frozen | token_state | No |
| dao | resolved, arena.blocks | dao_state | No |
| fiber | resolved, frozen | fiber_state | No |
| object | resolved, frozen | object_state | No |

Restructure to run address+cell_dist in parallel with the 5 reducers:

```
RIGHT = hodl(~10ms) → rayon::join(
  address+cell_dist(276ms),
  rayon::join(script+token, dao+rayon::join(fiber, object))(~438ms)
) = max(276, 438) = ~448ms
```

**Saves: ~276ms** in RIGHT branch. Since LEFT (637ms) > RIGHT (448ms), LEFT becomes the new critical path.

### Change D: chrono format cache in apply_bundles

In `ActivityStatsAccumulator::apply_bundles` (line 894-928), each tx bundle triggers two `chrono::format` + `String` allocations:

```rust
let date = block_date_from_ms(bundle.timestamp).format("%Y%m%d").to_string();
let hour = block_datetime_from_ms(bundle.timestamp).format("%Y%m%d%H").to_string();
```

All txs in the same block share the same `timestamp_ms`. Cache the formatted strings:

```rust
let mut cached_ts = i64::MIN;
let mut cached_date = String::new();
let mut cached_hour = String::new();

for bundle in bundles {
    if bundle.timestamp != cached_ts {
        cached_ts = bundle.timestamp;
        cached_date = block_date_from_ms(bundle.timestamp).format("%Y%m%d").to_string();
        cached_hour = block_datetime_from_ms(bundle.timestamp).format("%Y%m%d%H").to_string();
    }
    // use cached_date, cached_hour for entry lookups
}
```

~123K format calls → ~47K blocks (2.6x fewer). Also reduces `String::clone` calls by hoisting `entry()` lookups outside the per-owner loop where possible.

**Saves: ~50-75ms** in activity_stats (20-30% reduction).

### Combined execution model

```
facts(295ms) → resolve(142ms) →
  rayon::join(
    LEFT:  history(392) → activity_stats(~195 with chrono cache) = ~587ms   ← critical path
    rayon::join(
      MIDDLE: chain_stats(~150ms)                                           ← finishes early, worker helps others
      RIGHT:  hodl(~10) → rayon::join(
                address+cell_dist(276),
                5_parallel_reducers(~438)
              ) = max(276,438)+10 ≈ 448ms
    ) = max(150, 448) = 448ms
  ) = max(587, 448) = 587ms
→ post_overlap(~20ms) → misc(~110ms)

Estimated build_ms ≈ 295 + 142 + 587 + 20 + 110 = 1154ms
Current build_ms = 1888ms
Estimated savings ≈ 734ms (39%)
```

### Borrow checker strategy

The existing destructure at line 1404 already splits `self` into individual fields. Each closure captures disjoint `&mut` fields:

| Branch | Mutable captures | Shared immutable |
|--------|-----------------|------------------|
| LEFT | `activity_stats` | `arena`, `resolved`, `frozen`, `is_mainnet`, `token_info_cache` |
| MIDDLE | `chain_stats` | `arena`, `resolved` |
| RIGHT | `owners`, `cell_dist_tracker`, `hodl_tracker`, `hodl_live_cells_by_lock` | `arena`, `resolved`, `frozen` |

No overlap in mutable captures. Shared references are `&` (immutable).

### Return type

```rust
let (left_result, (mid_result, right_result)) = rayon::join(
    || -> Result<(BatchHistoryRows, Duration, Duration)> {
        // history + activity_stats; returns (history, history_elapsed, act_stats_elapsed)
    },
    || rayon::join(
        || -> Result<()> {
            // chain_stats.apply_blocks
        },
        || -> Result<(Vec<MaterializedRow>, Vec<MaterializedRow>, Duration)> {
            // hodl + rayon::join(address+cell_dist, 5 reducers); returns (hodl_rows, dist_rows, addr_elapsed)
        },
    ),
);
```

### Post-overlap (unchanged)

After the 3-way join completes:

```rust
owners.object.apply_object_activity_count_deltas(&history.object_activity_count_deltas)?;
owners.object.apply_identity_activity_count_deltas(&history.identity_activity_count_deltas)?;
```

This requires both `history` (from LEFT) and `object` (from RIGHT) to be done. Both are guaranteed done after `rayon::join` returns.

### Timing measurement changes

| Metric | Current | After |
|--------|---------|-------|
| `facts_ms` | Unchanged | Unchanged |
| `resolve_ms` | Unchanged | Unchanged |
| `reduce_ms` | Outer join + post-overlap + activity_stats + chain_stats | Outer 3-way join + post-overlap only |
| `history_ms` | Measured inside LEFT | Unchanged |
| `address_reduce_ms` | Measured inside RIGHT | Unchanged |
| `activity_stats_ms` | Measured after join (serial) | Measured inside LEFT (parallel) |

`reduce_ms` semantics change: it now measures only the 3-way parallel tree + post-overlap, excluding activity_stats and chain_stats. This gives a cleaner picture of the parallel phase. No new timing fields needed; the existing `BatchBuildTimings` struct is sufficient.

### Scope

Files changed:
- `crates/indexer/src/sync/bulk_build/mod.rs`: restructure `apply_blocks` and `apply_blocks_hex` parallel tree

No changes to:
- DB schema, column families, or key encoding
- Store ops or API routes
- Any types or traits
- Test assertions (timing values are not asserted)
- `BatchBuildTimings` struct fields

### Testing

- Existing `cargo test -p ckbadger-indexer` must pass — unit tests exercise `apply_blocks` via `build_history_rows`, reducer `apply_tx`, and `activity_stats.apply_bundles` end-to-end.
- Full sync re-run to verify identical DB output (activity stats, chain stats, address balances all match prior run).
- Compare perf run metrics: `build_ms` should decrease ~30-39%, `reduce_ms` should decrease (narrower scope), `activity_stats_ms` should decrease ~20-30% (chrono cache).
