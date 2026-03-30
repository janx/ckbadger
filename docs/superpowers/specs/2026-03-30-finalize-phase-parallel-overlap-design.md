# Finalize Phase Parallel Overlap

Reduce bulk sync finalization wall clock from ~102s to ~68s by overlapping materialize phases with flush drain and parallelizing owner row building.

## Problem

The finalization phase runs 13 sub-phases sequentially after the last bulk-build batch. Timeline from the latest perf run (run-20260330T141849):

| Phase | Duration | What |
|---|---|---|
| Phase 0: Flush drain | 62s | Wait for 320 queued flush batches to commit |
| Phases 1-11: Materialize | 34.5s | Write ~16.8M rows (14M snapshot + 2.8M sealed + owners) |
| Phase 12: Memtable flush | 5.4s | flush_all_memtables (60 domain + 1 append-only CF) |
| Phase 13: Sync cleanup | 0.5s | finalize_success + clear markers |
| **Total** | **102.2s** | |

The flush drain and materialize phases write to disjoint CF sets. They block on each other unnecessarily.

## Design

### Optimization A: Overlap materialize with flush drain

Flush drain writes to: `CF_ACTIVITIES`, `CF_ADDR_ACTIVITIES`, `CF_ADDR_TXS`, `CF_STATS_*` (history/sealed rows from batch processing).

Materialize phases write to: `CF_LIVE_CELLS`, `CF_CELL_BY_*`, `CF_ADDR_BALANCE`, `CF_SCRIPT_INFO`, `CF_SPORE_DATA`, `CF_TOKEN_INFO`, etc.

No CF overlap — concurrent writes are safe.

**Change**: After `begin_shutdown()` and `prepare_finalize_artifacts()`, run phases 1-11 in `tokio::task::spawn_blocking` concurrently with `flush_drain.wait()` via `tokio::join!`.

```
BEFORE (sequential):
  flush_drain.wait()          ─── 62s ───
                              phases 1-11  ─── 34.5s ───
                                           phase 12-13 ─ 5.9s ─
  Total: 102s

AFTER (overlapped):
  flush_drain.wait()          ─── 62s ────────────────────
  phases 1-11 (spawn_blocking) ─── ~25s ─── (overlapped)
                                            phase 12-13 ─ 5.9s ─
  Total: ~68s
```

#### Implementation

Extract current phases 1-11 into a standalone `materialize_finalize_phases()` function:

```rust
fn materialize_finalize_phases(
    domain_store: &CkbadgerStore,
    append_only_store: &CkbadgerStore,
    prepared: PreparedFinalizeArtifacts,
    owners: CoreOwners,
    hodl_tracker: HodlWaveTracker,
    cell_dist_tracker: CellDistTracker,
    perf_stats: &BulkBuildPerfStats,
    finalize_started: Instant,
) -> Result<MaterializationReport> { ... }
```

In the main flow:

```rust
let flush_drain = flush_channel.begin_shutdown();
let prepared_finalize = runtime.prepare_finalize_artifacts()?;

let BulkBuildRuntimeState { owners, hodl_tracker, cell_dist_tracker, .. } = runtime;

let domain_store = indexer.writer.store().clone();
let append_store = indexer.writer.append_only_store().clone();
let perf_stats = indexer.bulk_build_perf.clone();
let finalize_started_clone = finalize_started;

let materialize_handle = tokio::task::spawn_blocking(move || {
    materialize_finalize_phases(
        &domain_store, &append_store,
        prepared_finalize, owners, hodl_tracker, cell_dist_tracker,
        &perf_stats, finalize_started_clone,
    )
});

let (drain_result, materialize_result) = tokio::join!(
    flush_drain.wait(),
    materialize_handle,
);

let flush_stats = drain_result?;
let materialize_report = materialize_result
    .map_err(|e| anyhow!("materialize task panicked: {e}"))??;
```

#### Join point

Both drain and materialize must complete before phase 12 (memtable flush). `tokio::join!` ensures this.

#### Progress reporting

Pass `Arc<BulkBuildPerfStats>` and `Instant` (finalize_started) into the spawned closure. Continue calling `record_finalize_step()` as before. TUI polling reads atomics — no change needed.

#### Materializer accounting

Create a fresh `Materializer` inside the spawned closure with its own `MaterializationReport`. After join, merge into main accounting:

```rust
materializer_main.merge_report(materialize_report);
materializer_main.add_external_counts(
    flush_stats.total_history_rows,
    flush_stats.total_sealed_rows,
    flush_stats.flush_count,
);
```

### Optimization B: Parallelize owner row building

Within `materialize_finalize_phases()`, the 6 owner `materialize_final` calls run sequentially. Each owner builds rows (CPU + optional DB reads) then writes through the materializer.

**Change**: Add `build_final_rows()` to each owner that returns `Result<OwnerFinalRows>` (a struct with `sealed_rows` and `snapshot_rows`), separating row construction from the write. This covers both the `flush_sealed` output (sealed aggregate daily deltas) and `materialize_final` output (current-state snapshots). Run row building in parallel via `std::thread::scope`:

```rust
let (addr_rows, script_rows, token_rows, object_rows) = std::thread::scope(|s| {
    let h_addr = s.spawn(|| owners.address.build_final_rows());
    let h_script = s.spawn(|| owners.script.build_final_rows(domain_store));
    let h_token = s.spawn(|| owners.token.build_final_rows());
    let h_obj = s.spawn(|| owners.object.build_final_rows());
    // join all
    Ok::<_, anyhow::Error>((
        h_addr.join().expect("addr")?,
        h_script.join().expect("script")?,
        h_token.join().expect("token")?,
        h_obj.join().expect("object")?,
    ))
})?;

let dao_rows = owners.dao.build_final_rows()?;
let fiber_rows = owners.fiber.build_final_rows()?;

// Write all rows sequentially through Materializer
for rows in [&addr_rows, &script_rows, &token_rows, &dao_rows, &fiber_rows, &object_rows] {
    materializer.stream_sealed_aggregate_rows(&rows.sealed_rows)?;
    materializer.materialize_final_snapshot(&rows.snapshot_rows)?;
}
```

`ScriptOwner::build_final_rows` takes `&CkbadgerStore` because it does per-key `get_script_info()` reads to merge label fields. Other owners need no store access.

`dao` and `fiber` owners are small — run on main thread to avoid spawn overhead.

### Trait change

Add `build_final_rows` to owners (not to `BulkReducer` trait, since the signature varies — `ScriptOwner` needs a store reference). Each owner implements it directly. The existing `materialize_final` method remains for test use.

### Error handling

If either `flush_drain.wait()` or the materialize task fails, `tokio::join!` ensures both complete. Errors are collected and the first is returned. This aligns with bulk sync rule 1 (single-shot rebuild — fail fast, then rebuild from genesis).

## Files changed

| File | Change |
|---|---|
| `crates/indexer/src/sync/bulk_build/mod.rs` | Extract `materialize_finalize_phases()`, restructure finalize with `tokio::join!` |
| `crates/indexer/src/sync/bulk_build/owners/mod.rs` | Document `build_final_rows` pattern |
| `crates/indexer/src/sync/bulk_build/owners/address.rs` | Add `build_final_rows()` |
| `crates/indexer/src/sync/bulk_build/owners/script.rs` | Add `build_final_rows(&CkbadgerStore)` |
| `crates/indexer/src/sync/bulk_build/owners/token.rs` | Add `build_final_rows()` |
| `crates/indexer/src/sync/bulk_build/owners/object.rs` | Add `build_final_rows()` |
| `crates/indexer/src/sync/bulk_build/owners/dao.rs` | Add `build_final_rows()` |
| `crates/indexer/src/sync/bulk_build/owners/fiber.rs` | Add `build_final_rows()` |
| `crates/indexer/src/sync/bulk_build/materialize.rs` | Add `merge_report()` method |

## Testing

1. **End-to-end**: Full re-sync from genesis, confirm finalize completes without error
2. **Integrity**: `ckbadger verify --depth fast` passes (6 checks)
3. **Perf comparison**: finalize_seconds drops from ~102s to ~65-70s; materialization row counts match baseline exactly
4. **Unit tests**: `build_final_rows()` for each owner produces identical rows to `materialize_final()`; follows existing `prepare_finalize_artifacts_matches_direct_finalize_components` test pattern

## Not in scope

- Chunked final_snapshot writes — can layer on later
- Drain-aware tail batches — independent controller change
- Refactoring `Materializer` for concurrent `&mut` — worked around with fresh instance
- Changes to flush channel depth, worker count, or bottleneck controller
