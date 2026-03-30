# Finalize Phase Parallel Overlap Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Reduce bulk sync finalization wall clock from ~102s to ~68s by overlapping materialize phases with flush drain and parallelizing owner row building.

**Architecture:** Extract finalize phases 1-11 into `materialize_finalize_phases()`. Run it in `tokio::task::spawn_blocking` concurrently with `flush_drain.wait()` via `tokio::join!`. Within the materialize function, parallelize owner row building with `std::thread::scope`.

**Tech Stack:** Rust, tokio, RocksDB (ckbadger-store)

**Spec:** `docs/superpowers/specs/2026-03-30-finalize-phase-parallel-overlap-design.md`

---

## File Structure

| File | Role |
|---|---|
| `crates/indexer/src/sync/bulk_build/materialize.rs` | Add `OwnerFinalRows` struct, `merge_report()` method |
| `crates/indexer/src/sync/bulk_build/owners/address.rs` | Add `build_final_rows()` |
| `crates/indexer/src/sync/bulk_build/owners/fiber.rs` | Add `build_final_rows()` |
| `crates/indexer/src/sync/bulk_build/owners/dao.rs` | Add `build_final_rows()` |
| `crates/indexer/src/sync/bulk_build/owners/object.rs` | Add `build_final_rows()` |
| `crates/indexer/src/sync/bulk_build/owners/script.rs` | Add `build_final_rows(&CkbadgerStore, &CkbadgerStore)` |
| `crates/indexer/src/sync/bulk_build/owners/token.rs` | Add `build_final_rows(&CkbadgerStore)` |
| `crates/indexer/src/sync/bulk_build/mod.rs` | Extract `materialize_finalize_phases()`, restructure finalize with `tokio::join!` |

---

### Task 1: Add `OwnerFinalRows` struct and `merge_report` to materialize.rs

**Files:**
- Modify: `crates/indexer/src/sync/bulk_build/materialize.rs`

- [ ] **Step 1: Add `OwnerFinalRows` struct**

In `crates/indexer/src/sync/bulk_build/materialize.rs`, after the `MaterializedRow` struct (line 27), add:

```rust
/// Rows produced by an owner's `build_final_rows()` method, split by write policy.
/// `sealed_rows` are daily/hourly aggregates (SealedAggregate policy).
/// `snapshot_rows` are current-state data (FinalSnapshot policy).
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct OwnerFinalRows {
    pub(crate) sealed_rows: Vec<MaterializedRow>,
    pub(crate) snapshot_rows: Vec<MaterializedRow>,
}
```

- [ ] **Step 2: Add `merge_report` method to `Materializer`**

In the `impl<'a> Materializer<'a>` block, after the `finish()` method (line 105), add:

```rust
    pub(crate) fn merge_report(&mut self, other: MaterializationReport) {
        self.report.streamed_history_rows += other.streamed_history_rows;
        self.report.sealed_aggregate_rows += other.sealed_aggregate_rows;
        self.report.final_snapshot_rows += other.final_snapshot_rows;
        self.report.history_flushes += other.history_flushes;
        self.report.sealed_aggregate_flushes += other.sealed_aggregate_flushes;
        self.report.final_snapshot_flushes += other.final_snapshot_flushes;
    }
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo check -p ckbadger-indexer`
Expected: compiles with no errors (new code is unused so far — may get warnings, that's fine)

- [ ] **Step 4: Commit**

```bash
git add crates/indexer/src/sync/bulk_build/materialize.rs
git commit -m "feat(bulk-build): add OwnerFinalRows struct and merge_report to Materializer"
```

---

### Task 2: Add `build_final_rows` to all owners

Each owner gets a `build_final_rows` method that extracts the row-building logic from `flush_sealed` + `materialize_final`, returning `OwnerFinalRows`. The existing `flush_sealed`/`materialize_final` methods stay unchanged (used by `CoreOwners::materialize_all` in test paths).

**Files:**
- Modify: `crates/indexer/src/sync/bulk_build/owners/address.rs`
- Modify: `crates/indexer/src/sync/bulk_build/owners/fiber.rs`
- Modify: `crates/indexer/src/sync/bulk_build/owners/dao.rs`
- Modify: `crates/indexer/src/sync/bulk_build/owners/object.rs`
- Modify: `crates/indexer/src/sync/bulk_build/owners/script.rs`
- Modify: `crates/indexer/src/sync/bulk_build/owners/token.rs`
- Modify: `crates/indexer/src/sync/bulk_build/mod.rs` (tests)

**Important context for each owner:**

| Owner | flush_sealed | materialize_final | Store needed |
|---|---|---|---|
| AddressOwner | no-op (trait default) | CF_ADDR_BALANCE (line 138) | none |
| FiberOwner | no-op (trait default) | CF_FIBER_CHANNELS + indexes (line 39) | none |
| DaoOwner | CF_STATS_DAO cumulative stats (line 463) | CF_DAO_DEPOSITS + indexes (line 621) | none |
| ObjectOwner | CF_STATS_SPORE/MNFT/IDENTITY (line 63) | CF_SPORE_DATA + many CFs (line 248) | none |
| ScriptOwner | CF_STATS_SCRIPT daily (line 324) | CF_SCRIPT_INFO + references + versions + families (line 353) | domain_store + append_only_store |
| TokenOwner | CF_STATS_TOKEN daily (line 217) | CF_TOKEN_INFO merged with store (line 264) | domain_store |

- [ ] **Step 1: AddressOwner — add `build_final_rows`**

In `crates/indexer/src/sync/bulk_build/owners/address.rs`, add a method on `impl AddressOwner` (or after the `BulkReducer` impl). AddressOwner has no `flush_sealed`, so sealed_rows is empty. The row-building logic is extracted from `materialize_final` (lines 138-158):

```rust
impl AddressOwner {
    pub(crate) fn build_final_rows(&self) -> Result<super::super::materialize::OwnerFinalRows> {
        let mut lock_hashes: Vec<&Vec<u8>> = self.balances.keys().collect();
        lock_hashes.sort();

        let snapshot_rows = lock_hashes
            .into_iter()
            .map(|lock_hash| {
                let balance = self
                    .balances
                    .get(lock_hash)
                    .expect("sorted lock hash must exist in address owner");
                Ok(MaterializedRow::new(
                    CF_ADDR_BALANCE,
                    lock_hash.clone(),
                    bincode::serialize(balance)?,
                ))
            })
            .collect::<Result<Vec<_>>>()?;

        Ok(super::super::materialize::OwnerFinalRows {
            sealed_rows: Vec::new(),
            snapshot_rows,
        })
    }
}
```

Note: Add the necessary import `use super::super::materialize::MaterializedRow;` if not already imported (check existing imports — `MaterializedRow` should already be in scope since `materialize_final` uses it).

- [ ] **Step 2: FiberOwner — add `build_final_rows`**

In `crates/indexer/src/sync/bulk_build/owners/fiber.rs`, add on `impl FiberOwner`. FiberOwner has no `flush_sealed`. Extract from `materialize_final` (lines 39-77):

```rust
impl FiberOwner {
    // ... (existing estimated_bytes etc.)

    pub(crate) fn build_final_rows(&self) -> Result<super::super::materialize::OwnerFinalRows> {
        let mut snapshot_rows = Vec::new();

        for (channel_id, channel) in &self.channels {
            snapshot_rows.push(MaterializedRow::new(
                CF_FIBER_CHANNELS,
                channel_id.clone(),
                bincode::serialize(channel)?,
            ));
        }

        for (funding_args, channel_id) in &self.channel_by_funding_args {
            snapshot_rows.push(MaterializedRow::new(
                CF_FIBER_CHANNEL_BY_FUNDING_ARGS,
                funding_args.clone(),
                channel_id.clone(),
            ));
        }

        for (commitment_hash, channel_id) in &self.channel_by_commitment {
            snapshot_rows.push(MaterializedRow::new(
                CF_FIBER_CHANNEL_BY_COMMITMENT,
                commitment_hash.clone(),
                channel_id.clone(),
            ));
        }

        for (channel_id, channel) in &self.channels {
            for participant in &channel.participants {
                snapshot_rows.push(MaterializedRow::new(
                    CF_ADDR_FIBER_CHANNELS,
                    keys::encode_addr_fiber_channel_key(participant, channel_id),
                    Vec::new(),
                ));
            }
        }

        Ok(super::super::materialize::OwnerFinalRows {
            sealed_rows: Vec::new(),
            snapshot_rows,
        })
    }
}
```

- [ ] **Step 3: DaoOwner — add `build_final_rows`**

In `crates/indexer/src/sync/bulk_build/owners/dao.rs`, add on `impl DaoOwner`. DaoOwner has both `flush_sealed` (lines 463-620) and `materialize_final` (lines 621-690). The `build_final_rows` must produce both sealed_rows and snapshot_rows.

For `sealed_rows`: extract the row-building logic from `flush_sealed` (lines 463-620). This is a large method — copy the row construction verbatim, collecting into a `Vec<MaterializedRow>` instead of calling `materializer.stream_sealed_aggregate_rows()`.

For `snapshot_rows`: extract from `materialize_final` (lines 621-690). Copy verbatim, collecting into a `Vec<MaterializedRow>` instead of calling `materializer.materialize_final_snapshot()`.

```rust
    pub(crate) fn build_final_rows(&self) -> Result<super::super::materialize::OwnerFinalRows> {
        // sealed_rows: same logic as flush_sealed (lines 463-620)
        let sealed_rows = self.build_sealed_rows()?;

        // snapshot_rows: same logic as materialize_final (lines 621-690)
        let snapshot_rows = self.build_snapshot_rows()?;

        Ok(super::super::materialize::OwnerFinalRows {
            sealed_rows,
            snapshot_rows,
        })
    }
```

**Important implementation detail**: Rather than duplicating the 157 lines of `flush_sealed` and 70 lines of `materialize_final`, refactor the existing methods to call shared private helpers:

1. Extract the row-building logic from `flush_sealed` into `fn build_sealed_rows(&self) -> Result<Vec<MaterializedRow>>`
2. Extract the row-building logic from `materialize_final` into `fn build_snapshot_rows(&self) -> Result<Vec<MaterializedRow>>`
3. Make `flush_sealed` call `build_sealed_rows()` then `materializer.stream_sealed_aggregate_rows()`
4. Make `materialize_final` call `build_snapshot_rows()` then `materializer.materialize_final_snapshot()`
5. Make `build_final_rows` call both helpers

This avoids code duplication and keeps the existing methods working.

Apply the same helper-extraction pattern for all owners below.

- [ ] **Step 4: ObjectOwner — add `build_final_rows`**

Same pattern as DaoOwner. In `crates/indexer/src/sync/bulk_build/owners/object.rs`:

1. Extract `flush_sealed` (lines 63-166) row building into `fn build_sealed_rows(&mut self) -> Vec<MaterializedRow>`. Note: `flush_sealed` takes `&mut self` and the existing method collects from `self.stats_spore_rows`, `self.mnft_class_outpoints`, etc. The extracted helper should return the rows without draining the collections (so it works with `&self`), OR the helper can take `&mut self` if `build_final_rows` also takes `&mut self`.

   **Decision**: Since `build_final_rows` is called once during finalization (after all batches), taking `&self` is correct. The existing `flush_sealed` iterates over `self.stats_spore_rows` etc. without draining — it clones via `.iter().map()`. So `&self` works.

2. Extract `materialize_final` (lines 248-342) into `fn build_snapshot_rows(&self) -> Result<Vec<MaterializedRow>>`
3. Wire up `build_final_rows`

- [ ] **Step 5: ScriptOwner — add `build_final_rows`**

In `crates/indexer/src/sync/bulk_build/owners/script.rs`. ScriptOwner needs **both** stores:

- `materialize_final` calls `materializer.domain_store()` for `get_script_info()` reads (line 368)
- `materialize_final` calls `materializer.append_only_store()` for `collect_script_reference_rollup_state()` (line 443)

```rust
    pub(crate) fn build_final_rows(
        &self,
        domain_store: &CkbadgerStore,
        append_only_store: &CkbadgerStore,
    ) -> Result<super::super::materialize::OwnerFinalRows> {
        let sealed_rows = self.build_sealed_rows();
        let snapshot_rows = self.build_snapshot_rows(domain_store, append_only_store)?;
        Ok(super::super::materialize::OwnerFinalRows {
            sealed_rows,
            snapshot_rows,
        })
    }
```

Extract helpers:
1. `fn build_sealed_rows(&self) -> Vec<MaterializedRow>` — from `flush_sealed` (lines 324-351)
2. `fn build_snapshot_rows(&self, domain_store: &CkbadgerStore, append_only_store: &CkbadgerStore) -> Result<Vec<MaterializedRow>>` — from `materialize_final` (lines 353-471). Replace `materializer.domain_store()` with `domain_store` and `materializer.append_only_store()` with `append_only_store`. Collect ALL snapshot rows (script_info + reference_info + reference_mappings + version_rows + family_rows) into one Vec.

- [ ] **Step 6: TokenOwner — add `build_final_rows`**

In `crates/indexer/src/sync/bulk_build/owners/token.rs`. TokenOwner needs `domain_store`:

- `materialize_final` calls `materializer.domain_store()` for `get_tokens_batch()` (line 267)

```rust
    pub(crate) fn build_final_rows(
        &self,
        domain_store: &CkbadgerStore,
    ) -> Result<super::super::materialize::OwnerFinalRows> {
        let sealed_rows = self.build_sealed_rows();
        let snapshot_rows = self.build_snapshot_rows(domain_store)?;
        Ok(super::super::materialize::OwnerFinalRows {
            sealed_rows,
            snapshot_rows,
        })
    }
```

Extract helpers following the same pattern as ScriptOwner.

- [ ] **Step 7: Verify it compiles**

Run: `cargo check -p ckbadger-indexer`
Expected: compiles (new methods unused — warnings are fine)

- [ ] **Step 8: Add tests for `build_final_rows` consistency**

In `crates/indexer/src/sync/bulk_build/mod.rs`, in the `#[cfg(test)]` module, add a test following the `prepare_finalize_artifacts_matches_direct_finalize_components` pattern (line 6258). The test creates a runtime, applies blocks, then verifies that `build_final_rows()` output matches the rows produced by `flush_sealed()` + `materialize_final()`:

```rust
    #[test]
    fn owner_build_final_rows_matches_materialize_via_trait() {
        let mut runtime = BulkBuildRuntimeState::default();
        let block = bulk_build_addr_tx_fixture();
        runtime
            .apply_blocks_hex(std::slice::from_ref(&block), true, &FxHashMap::default())
            .unwrap();

        // Test each owner's build_final_rows produces same rows as flush_sealed + materialize_final
        let (domain_store, _append_only) = ckbadger_store::open_test_unified();

        // AddressOwner
        {
            let rows = runtime.owners.address.build_final_rows().unwrap();
            let mut materializer = materialize::Materializer::new(&domain_store, &domain_store);
            runtime.owners.address.materialize_final(&mut materializer).unwrap();
            let report = materializer.finish();
            assert_eq!(rows.sealed_rows.len(), 0);
            assert_eq!(rows.snapshot_rows.len(), report.final_snapshot_rows);
        }

        // FiberOwner
        {
            let rows = runtime.owners.fiber.build_final_rows().unwrap();
            assert_eq!(rows.sealed_rows.len(), 0);
            // fiber has no data in fixture, so 0 rows is expected
        }

        // DaoOwner (if fixture produces DAO data — check fixture)
        // ScriptOwner, TokenOwner, ObjectOwner — similar pattern
    }
```

Adapt the test to the actual fixture data available. The key assertion: for each owner, `build_final_rows().sealed_rows.len() + build_final_rows().snapshot_rows.len()` equals the total rows that `flush_sealed` + `materialize_final` would write. Use `open_test_unified()` to provide stores for script/token.

- [ ] **Step 9: Run tests**

Run: `cargo test -p ckbadger-indexer owner_build_final_rows`
Expected: PASS

- [ ] **Step 10: Commit**

```bash
git add crates/indexer/src/sync/bulk_build/owners/ crates/indexer/src/sync/bulk_build/mod.rs
git commit -m "feat(bulk-build): add build_final_rows to all owners for parallel row building"
```

---

### Task 3: Extract `materialize_finalize_phases` function

Refactor the finalize section (phases 1-11) in `mod.rs` into a standalone function. This task preserves identical behavior — the function is called sequentially, same as before. No parallelism yet.

**Files:**
- Modify: `crates/indexer/src/sync/bulk_build/mod.rs`

- [ ] **Step 1: Define the `materialize_finalize_phases` function**

Below the `flush_bulk_build_materialized_state` function (line 861), add:

```rust
/// Executes finalize phases 1-11: writes sealed aggregates, final snapshot,
/// owner data, and metadata to RocksDB. Returns a MaterializationReport
/// for merging into the main accounting.
///
/// Designed to be `Send` so it can run in `tokio::task::spawn_blocking`
/// concurrently with flush drain.
fn materialize_finalize_phases(
    domain_store: &CkbadgerStore,
    append_only_store: &CkbadgerStore,
    prepared: PreparedFinalizeArtifacts,
    mut owners: CoreOwners,
    hodl_tracker: crate::db::writer::hodl_wave::HodlWaveTracker,
    cell_dist_tracker: crate::db::writer::cell_distribution::CellDistributionTracker,
    perf_stats: &crate::sync::diagnostics::BulkBuildPerfStats,
    finalize_started: Instant,
) -> Result<materialize::MaterializationReport> {
    let mut materializer = materialize::Materializer::new(domain_store, append_only_store);

    // Phase 1: activity stats
    {
        let _guard =
            tracing::info_span!("bulk_finalize", phase = 2, label = "activity_stats").entered();
        perf_stats.record_finalize_step(2, finalize_started.elapsed());
        materializer.stream_sealed_aggregate_rows(&prepared.activity_sealed_rows)?;
    }

    // Phase 2: chain stats
    {
        let _guard =
            tracing::info_span!("bulk_finalize", phase = 3, label = "chain_stats").entered();
        perf_stats.record_finalize_step(3, finalize_started.elapsed());
        materializer.stream_sealed_aggregate_rows(&prepared.chain_sealed_rows)?;
    }

    // Phase 3: final snapshot (live cell markers + index CFs)
    {
        let _guard =
            tracing::info_span!("bulk_finalize", phase = 4, label = "final_snapshot").entered();
        perf_stats.record_finalize_step(4, finalize_started.elapsed());
        materializer.materialize_final_snapshot(&prepared.final_snapshot_rows)?;
    }

    // Phases 4-9: owners (flush_sealed + materialize_final per owner)
    {
        let _guard =
            tracing::info_span!("bulk_finalize", phase = 5, label = "owner_address").entered();
        perf_stats.record_finalize_step(5, finalize_started.elapsed());
        owners.address.flush_sealed(&mut materializer)?;
        owners.address.materialize_final(&mut materializer)?;
    }
    {
        let _guard =
            tracing::info_span!("bulk_finalize", phase = 6, label = "owner_script").entered();
        perf_stats.record_finalize_step(6, finalize_started.elapsed());
        owners.script.flush_sealed(&mut materializer)?;
        owners.script.materialize_final(&mut materializer)?;
    }
    {
        let _guard =
            tracing::info_span!("bulk_finalize", phase = 7, label = "owner_token").entered();
        perf_stats.record_finalize_step(7, finalize_started.elapsed());
        owners.token.flush_sealed(&mut materializer)?;
        owners.token.materialize_final(&mut materializer)?;
    }
    {
        let _guard =
            tracing::info_span!("bulk_finalize", phase = 8, label = "owner_dao").entered();
        perf_stats.record_finalize_step(8, finalize_started.elapsed());
        owners.dao.flush_sealed(&mut materializer)?;
        owners.dao.materialize_final(&mut materializer)?;
    }
    {
        let _guard =
            tracing::info_span!("bulk_finalize", phase = 9, label = "owner_fiber").entered();
        perf_stats.record_finalize_step(9, finalize_started.elapsed());
        owners.fiber.flush_sealed(&mut materializer)?;
        owners.fiber.materialize_final(&mut materializer)?;
    }
    {
        let _guard =
            tracing::info_span!("bulk_finalize", phase = 10, label = "owner_object").entered();
        perf_stats.record_finalize_step(10, finalize_started.elapsed());
        owners.object.flush_sealed(&mut materializer)?;
        owners.object.materialize_final(&mut materializer)?;
    }

    // Phase 10: metadata (HODL + cell distribution tracker state)
    {
        let _guard =
            tracing::info_span!("bulk_finalize", phase = 11, label = "metadata").entered();
        perf_stats.record_finalize_step(11, finalize_started.elapsed());
        let mut meta_batch = ckbadger_store::batch::StoreBatch::new(domain_store);
        meta_batch.put_hodl_tracker_state(&hodl_tracker.to_state());
        meta_batch.put_cell_dist_tracker_state(&cell_dist_tracker.to_state());
        if !meta_batch.is_empty() {
            meta_batch.commit()?;
        }
    }

    Ok(materializer.finish())
}
```

- [ ] **Step 2: Replace inline phases 1-11 with function call**

In `run_bulk_stage_until_pipeline_handoff`, replace lines 544-651 (phases 1-11, everything between the flush_drain.wait() section and the memtable_flush section) with:

```rust
        let BulkBuildRuntimeState {
            owners,
            hodl_tracker,
            cell_dist_tracker,
            ..
        } = runtime;

        let materialize_report = materialize_finalize_phases(
            indexer.writer.store().as_ref(),
            indexer.writer.append_only_store(),
            prepared_finalize,
            owners,
            hodl_tracker,
            cell_dist_tracker,
            &indexer.bulk_build_perf,
            finalize_started,
        )?;

        // Merge materialization accounting
        materializer.merge_report(materialize_report);
```

Keep the `flush_drain.wait()` (phase 0), memtable_flush (phase 11/12), and sync_cleanup (phase 12/13) in the original function.

**Important**: The `let mut owners = owners;` line (line 575) and the `BulkBuildRuntimeState` destructure (lines 537-542) currently happen between phases. Move the destructure to before the `materialize_finalize_phases` call. The `materializer` variable in the outer scope is used only for `add_external_counts` and the final `finish()` — it's still needed for the flush_stats accounting.

- [ ] **Step 3: Verify it compiles and tests pass**

Run: `cargo check -p ckbadger-indexer && cargo test -p ckbadger-indexer -- --lib`
Expected: compiles and all tests pass (behavior is identical)

- [ ] **Step 4: Commit**

```bash
git add crates/indexer/src/sync/bulk_build/mod.rs
git commit -m "refactor(bulk-build): extract materialize_finalize_phases function"
```

---

### Task 4: Parallelize owner row building inside `materialize_finalize_phases`

Replace the sequential owner `flush_sealed` + `materialize_final` calls with parallel `build_final_rows` + sequential writes.

**Files:**
- Modify: `crates/indexer/src/sync/bulk_build/mod.rs`

- [ ] **Step 1: Replace sequential owner phases with parallel build + sequential write**

In the `materialize_finalize_phases` function, replace the 6 sequential owner blocks (phases 4-9) with:

```rust
    // Phases 4-9: build owner rows in parallel, write sequentially
    {
        let _guard =
            tracing::info_span!("bulk_finalize", phase = 5, label = "owners_build").entered();
        perf_stats.record_finalize_step(5, finalize_started.elapsed());

        // Build rows in parallel — each owner is independent
        let (addr_result, fiber_result, dao_result, object_result, script_result, token_result) =
            std::thread::scope(|s| {
                let h_addr = s.spawn(|| owners.address.build_final_rows());
                let h_script =
                    s.spawn(|| owners.script.build_final_rows(domain_store, append_only_store));
                let h_token = s.spawn(|| owners.token.build_final_rows(domain_store));
                let h_object = s.spawn(|| owners.object.build_final_rows());

                // dao + fiber are small, run inline
                let dao_result = owners.dao.build_final_rows();
                let fiber_result = owners.fiber.build_final_rows();

                (
                    h_addr.join().expect("address build_final_rows panicked"),
                    fiber_result,
                    dao_result,
                    h_object.join().expect("object build_final_rows panicked"),
                    h_script.join().expect("script build_final_rows panicked"),
                    h_token.join().expect("token build_final_rows panicked"),
                )
            });

        let addr_rows = addr_result?;
        let fiber_rows = fiber_result?;
        let dao_rows = dao_result?;
        let object_rows = object_result?;
        let script_rows = script_result?;
        let token_rows = token_result?;

        // Write all rows sequentially through Materializer
        perf_stats.record_finalize_step(6, finalize_started.elapsed());
        for rows in [
            &addr_rows,
            &script_rows,
            &token_rows,
            &dao_rows,
            &fiber_rows,
            &object_rows,
        ] {
            if !rows.sealed_rows.is_empty() {
                materializer.stream_sealed_aggregate_rows(&rows.sealed_rows)?;
            }
            if !rows.snapshot_rows.is_empty() {
                materializer.materialize_final_snapshot(&rows.snapshot_rows)?;
            }
        }
    }
```

**Note on progress steps**: The original code used steps 5-10 (one per owner). With parallel building, we can't report per-owner progress meaningfully. Use step 5 for "building rows" and step 6 for "writing rows". Steps 7-10 become unused — that's fine, the TUI just shows the current step label. Alternatively, keep step 10 after the write loop so the TUI shows "owner:object" → "metadata" transition correctly. Update `finalize_step_label` in `diagnostics.rs` if needed, or leave as-is (the TUI will skip intermediate steps).

- [ ] **Step 2: Verify it compiles and tests pass**

Run: `cargo check -p ckbadger-indexer && cargo test -p ckbadger-indexer -- --lib`
Expected: compiles and all tests pass

- [ ] **Step 3: Commit**

```bash
git add crates/indexer/src/sync/bulk_build/mod.rs
git commit -m "feat(bulk-build): parallelize owner row building in finalize phase"
```

---

### Task 5: Add `tokio::join!` overlap of drain and materialize

The core optimization: run `materialize_finalize_phases` in `spawn_blocking` concurrently with `flush_drain.wait()`.

**Files:**
- Modify: `crates/indexer/src/sync/bulk_build/mod.rs`

- [ ] **Step 1: Restructure finalize to overlap drain and materialize**

In `run_bulk_stage_until_pipeline_handoff`, the current flow after phase 0 setup is:

```
flush_drain.wait()    →  materialize_finalize_phases()  →  memtable_flush  →  cleanup
```

Change to:

```
tokio::join!(flush_drain.wait(), spawn_blocking(materialize_finalize_phases))  →  memtable_flush  →  cleanup
```

Replace the section from `let flush_stats = flush_drain.wait().await?;` through the `materialize_finalize_phases` call with:

```rust
        // Run flush drain and materialize phases concurrently.
        // They write to disjoint CF sets — safe for concurrent RocksDB writes.
        let domain_store_arc = indexer.writer.store().clone();
        let append_only_arc = indexer.writer.append_only_store_arc().clone();
        let perf_stats = indexer.bulk_build_perf.clone();
        let finalize_started_copy = finalize_started;

        let materialize_handle = tokio::task::spawn_blocking(move || {
            materialize_finalize_phases(
                domain_store_arc.as_ref(),
                append_only_arc.as_ref(),
                prepared_finalize,
                owners,
                hodl_tracker,
                cell_dist_tracker,
                &perf_stats,
                finalize_started_copy,
            )
        });

        let (drain_result, materialize_result) = tokio::join!(
            flush_drain.wait(),
            materialize_handle,
        );

        let flush_stats = drain_result?;
        let materialize_report = materialize_result
            .map_err(|e| anyhow!("materialize finalize task panicked: {e}"))??;

        materializer.add_external_counts(
            flush_stats.total_history_rows,
            flush_stats.total_sealed_rows,
            flush_stats.flush_count,
        );
        materializer.merge_report(materialize_report);

        info!(
            "flush pipeline: prepare={:.1}s commit={:.1}s flushes={} rows={}",
            flush_stats.total_prepare_ms / 1000.0,
            flush_stats.total_commit_ms / 1000.0,
            flush_stats.flush_count,
            flush_stats.total_history_rows + flush_stats.total_sealed_rows,
        );
```

**Critical check**: The `materialize_finalize_phases` function and all data it captures must be `Send`. Verify:
- `PreparedFinalizeArtifacts`: contains `Vec<MaterializedRow>` — `Send` ✓
- `CoreOwners`: contains `FxHashMap`, `BTreeMap`, `Vec`, primitives — `Send` ✓
- `HodlWaveTracker`, `CellDistributionTracker`: standard structs — `Send` ✓
- `Arc<BulkBuildPerfStats>`: atomics — `Send + Sync` ✓
- `Instant`: `Send` ✓

If the compiler rejects any of these, the error message will indicate which type is not `Send`. Fix by wrapping in `Arc` or restructuring.

**Also check**: `indexer.writer.append_only_store_arc()` — verify this method exists and returns `Arc<CkbadgerStore>`. If the method is named differently, search for it:

```bash
grep -n "append_only_store" crates/indexer/src/sync/indexer.rs | head -10
```

The append_only store might be accessed differently. Adjust the code to match the actual API. The key requirement: get an `Arc<CkbadgerStore>` or `&CkbadgerStore` that can be moved into the `spawn_blocking` closure.

- [ ] **Step 2: Verify it compiles**

Run: `cargo check -p ckbadger-indexer`
Expected: compiles. If `Send` bound errors occur, the compiler will point to the exact type — fix it.

- [ ] **Step 3: Run tests**

Run: `cargo test -p ckbadger-indexer -- --lib`
Expected: all pass

- [ ] **Step 4: Commit**

```bash
git add crates/indexer/src/sync/bulk_build/mod.rs
git commit -m "feat(bulk-build): overlap materialize with flush drain via tokio::join!"
```

---

### Task 6: Final verification

- [ ] **Step 1: Run full Rust test suite**

Run: `cargo test`
Expected: all pass

- [ ] **Step 2: Run clippy**

Run: `cargo clippy`
Expected: no new warnings

- [ ] **Step 3: Commit any fixes**

If clippy or tests revealed issues, fix and commit.

- [ ] **Step 4: Document the change**

The perf improvement will be validated by a full re-sync. Expected outcomes:
- `finalize_seconds` drops from ~102s to ~65-70s
- `wall_clock_seconds` drops by ~30s
- Materialization row counts (streamed_history_rows, sealed_aggregate_rows, final_snapshot_rows) match baseline exactly
- `ckbadger verify --depth fast` passes

---

## Summary

| Task | What | Est. time |
|---|---|---|
| 1 | OwnerFinalRows + merge_report foundation | 5 min |
| 2 | build_final_rows for all 6 owners + tests | 20 min |
| 3 | Extract materialize_finalize_phases (refactor) | 10 min |
| 4 | Parallel owner row building | 10 min |
| 5 | tokio::join! overlap | 10 min |
| 6 | Final verification | 5 min |
