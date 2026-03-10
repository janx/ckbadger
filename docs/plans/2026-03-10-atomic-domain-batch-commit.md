# Atomic Domain Batch Commit Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Eliminate partial domain-store state by merging all domain-store `StoreBatch` objects into a single atomic commit per write path.

**Architecture:** Each write path (`write_parsed_batch`) currently creates multiple `StoreBatch` objects targeting the same domain RocksDB instance and commits them sequentially. If any commit after the first succeeds-then-fails, partial state persists and rollback cleanup doesn't cover all CFs (e.g. `CF_IDENTITIES`). Fix: use `StoreBatch::merge_from()` to consolidate all domain batches before a single `commit()` call. Append-only batches (`cells_batch`) stay separate since they target a different RocksDB instance and are write-once-safe.

**Tech Stack:** Rust, RocksDB WriteBatch, existing `StoreBatch::merge_from()`

---

## Background

The indexer has three write paths in `crates/indexer/src/sync/batch.rs`:

1. **T1 bulk sync** (~line 1385-1974): `batch` + `consume_addr_batch` + `domain_analytics_batch` + `append_history_batch` — 4 sequential domain commits
2. **T2 bulk sync consume** (~line 2774-2984): `consume_batch` + `object_activity_batch` + `identity_activity_batch` + `core_batch` + `stats_batch` — 5 sequential domain commits
3. **Live sync** (~line 4929-6458): `data_batch` + `domain_analytics_batch` + `object_activity_batch` + `identity_activity_batch` + `append_history_batch` + `activity_batch` + `core_batch` + `stats_batch` — 8 sequential domain commits

All target the same domain store. `StoreBatch::merge_from()` (line 80 of `batch.rs`) already exists and handles WriteBatch wire-format merging correctly.

The fix: before committing, merge all secondary batches into the primary batch, then commit once.

---

### Task 1: Atomic commit for live sync path

**Files:**

- Modify: `crates/indexer/src/sync/batch.rs:6081-6110` (data commit section)
- Modify: `crates/indexer/src/sync/batch.rs:6407-6459` (finalization commit section)

**Step 1: Replace sequential commits with merge-then-commit in data section**

Replace lines 6081-6110:

```rust
            // Commit all data writes in a single batch
            let data_commit_started = Instant::now();
            data_batch.commit()?;
            write_commit_ms += data_commit_started.elapsed().as_secs_f64() * 1000.0;
            if !domain_analytics_batch.is_empty() {
                let script_commit_started = Instant::now();
                domain_analytics_batch.commit()?;
                write_commit_ms += script_commit_started.elapsed().as_secs_f64() * 1000.0;
            }
            if !object_activity_batch.is_empty() {
                let nft_activity_commit_started = Instant::now();
                object_activity_batch.commit()?;
                write_commit_ms += nft_activity_commit_started.elapsed().as_secs_f64() * 1000.0;
            }
            if !identity_activity_batch.is_empty() {
                let identity_activity_commit_started = Instant::now();
                identity_activity_batch.commit()?;
                write_commit_ms +=
                    identity_activity_commit_started.elapsed().as_secs_f64() * 1000.0;
            }
            if !append_history_batch.is_empty() {
                let append_commit_started = Instant::now();
                append_history_batch.commit()?;
                write_commit_ms += append_commit_started.elapsed().as_secs_f64() * 1000.0;
            }
            if !activity_batch.is_empty() {
                let activity_commit_started = Instant::now();
                activity_batch.commit()?;
                write_commit_ms += activity_commit_started.elapsed().as_secs_f64() * 1000.0;
            }
```

With:

```rust
            // Merge all domain-store batches into data_batch for atomic commit.
            // This prevents partial state if any individual commit would fail.
            data_batch.merge_from(domain_analytics_batch);
            data_batch.merge_from(object_activity_batch);
            data_batch.merge_from(identity_activity_batch);
            data_batch.merge_from(append_history_batch);
            data_batch.merge_from(activity_batch);
```

Do NOT commit `data_batch` yet — it will be committed in finalization (step 2).

**Step 2: Merge finalization batches and commit once**

Replace the finalization section at lines 6407-6459. Currently `core_batch` and `stats_batch` are created and committed separately. Instead, merge them into `data_batch` and commit once.

This requires `data_batch` to survive into the finalization scope. Currently `data_batch` is live through the entire `else` branch (live sync), so it is accessible.

Replace lines 6407-6459 (the finalization block):

```rust
        // Finalization: block headers + stats commit
        let t_finalize = Instant::now();
        {
            let mut core_batch = StoreBatch::new(self.writer.store());
            self.writer
                .insert_blocks_batch(&block_refs, &mut core_batch)?;
            let mut stats_batch = StoreBatch::new(self.writer.store());
            self.write_batch_stats_to_batch(&batch_stats, &mut stats_batch)?;
            // Write accumulated daily activity stats
            for (date, stats) in &daily_activity_accum {
                let unique_count = daily_activity_addrs.get(date).map_or(0, |s| s.len() as u32);
                self.writer.update_daily_activity_stats(
                    date,
                    stats,
                    unique_count,
                    &mut stats_batch,
                )?;
            }
            ...
            if bulk_sync_mode {
                core_batch.commit_no_wal()...?;
                stats_batch.commit_no_wal()...?;
            } else {
                core_batch.commit()...?;
                stats_batch.commit()...?;
            }
```

With:

```rust
        // Finalization: merge block headers + stats into data_batch, commit atomically
        let t_finalize = Instant::now();
        {
            let mut core_batch = StoreBatch::new(self.writer.store());
            self.writer
                .insert_blocks_batch(&block_refs, &mut core_batch)?;
            let mut stats_batch = StoreBatch::new(self.writer.store());
            self.write_batch_stats_to_batch(&batch_stats, &mut stats_batch)?;
            for (date, stats) in &daily_activity_accum {
                let unique_count = daily_activity_addrs.get(date).map_or(0, |s| s.len() as u32);
                self.writer.update_daily_activity_stats(
                    date,
                    stats,
                    unique_count,
                    &mut stats_batch,
                )?;
            }
            // ... keep existing debug! log ...
            let finalize_commit_started = Instant::now();
            data_batch.merge_from(core_batch);
            data_batch.merge_from(stats_batch);
            if bulk_sync_mode {
                data_batch.commit_no_wal().with_context(|| {
                    format!(
                        "atomic domain commit_no_wal failed for blocks {}-{}",
                        first_block, last_block
                    )
                })?;
            } else {
                data_batch.commit().with_context(|| {
                    format!(
                        "atomic domain commit failed for blocks {}-{}",
                        first_block, last_block
                    )
                })?;
            }
            // ... keep existing timing/warn code, just rename variable references ...
        }
```

**Important:** `data_batch` must be declared outside the bulk-sync `if/else` so it's accessible in finalization. Check that the current scoping allows this. The live sync path creates `data_batch` at line 4929 in the `else` branch, and finalization is also inside that `else` branch, so scoping is fine.

**Step 3: Verify compilation**

Run: `cargo check -p ckbadger-indexer`
Expected: compiles without error

**Step 4: Run existing tests**

Run: `cargo test -p ckbadger-indexer --lib`
Expected: all pass (no behavior change, just atomicity)

**Step 5: Commit**

```bash
git add crates/indexer/src/sync/batch.rs
git commit -m "fix(sync): atomic domain batch commit in live sync path

Merge all domain-store StoreBatch objects into a single WriteBatch
before commit. Previously 8 sequential commits meant a failure between
commits left partial state (e.g. dotbit identity consumed but block
headers not written), causing infinite retry loops after rollback.

Now all domain state commits atomically: either the entire batch
persists or nothing does."
```

---

### Task 2: Atomic commit for T2 bulk sync consume path

**Files:**

- Modify: `crates/indexer/src/sync/batch.rs:2954-2986`

**Step 1: Replace sequential commits with merge-then-commit**

Replace lines 2954-2986:

```rust
        let commit_started = Instant::now();
        consume_batch.commit()?;
        commit_ms += commit_started.elapsed().as_secs_f64() * 1000.0;
        if !object_activity_batch.is_empty() {
            let commit_started = Instant::now();
            object_activity_batch.commit()?;
            commit_ms += commit_started.elapsed().as_secs_f64() * 1000.0;
        }
        if !identity_activity_batch.is_empty() {
            let commit_started = Instant::now();
            identity_activity_batch.commit()?;
            commit_ms += commit_started.elapsed().as_secs_f64() * 1000.0;
        }

        // Finalization: persist block headers last as the durable sync marker,
        // together with stats derived from this batch.
        {
            let mut core_batch = StoreBatch::new(self.writer.store());
            self.writer
                .insert_blocks_batch(&block_refs, &mut core_batch)?;
            let mut stats_batch = StoreBatch::new(self.writer.store());
            self.write_batch_stats_to_batch(&batch_stats, &mut stats_batch)?;
            if bulk_sync_mode {
                let commit_started = Instant::now();
                core_batch.commit_no_wal()?;
                stats_batch.commit_no_wal()?;
                commit_ms += commit_started.elapsed().as_secs_f64() * 1000.0;
            } else {
                let commit_started = Instant::now();
                core_batch.commit()?;
                stats_batch.commit()?;
                commit_ms += commit_started.elapsed().as_secs_f64() * 1000.0;
            }
        }
```

With:

```rust
        // Merge all domain-store batches into consume_batch for atomic commit
        consume_batch.merge_from(object_activity_batch);
        consume_batch.merge_from(identity_activity_batch);

        // Finalization: merge block headers + stats, then single atomic commit
        {
            let mut core_batch = StoreBatch::new(self.writer.store());
            self.writer
                .insert_blocks_batch(&block_refs, &mut core_batch)?;
            let mut stats_batch = StoreBatch::new(self.writer.store());
            self.write_batch_stats_to_batch(&batch_stats, &mut stats_batch)?;
            consume_batch.merge_from(core_batch);
            consume_batch.merge_from(stats_batch);
            let commit_started = Instant::now();
            if bulk_sync_mode {
                consume_batch.commit_no_wal()?;
            } else {
                consume_batch.commit()?;
            }
            commit_ms += commit_started.elapsed().as_secs_f64() * 1000.0;
        }
```

**Step 2: Verify compilation**

Run: `cargo check -p ckbadger-indexer`

**Step 3: Run tests**

Run: `cargo test -p ckbadger-indexer --lib`

**Step 4: Commit**

```bash
git add crates/indexer/src/sync/batch.rs
git commit -m "fix(sync): atomic domain batch commit in T2 bulk sync path"
```

---

### Task 3: Atomic commit for T1 bulk sync path

**Files:**

- Modify: `crates/indexer/src/sync/batch.rs:1384-1975`

**Step 1: Hoist `batch` out of its block scope**

Currently `batch` is created at line 1385 inside a `{ }` block that ends at line 1410. It needs to survive to line ~1974 where the last secondary batch commits.

Remove the block scope delimiters (the `{` at line 1384 and `}` at line 1410). Keep `batch` alive.

**Step 2: Replace sequential commits with merge-then-commit**

Replace lines 1962-1975 (the three separate commits):

```rust
        {
            let commit_started = Instant::now();
            consume_addr_batch.commit()?;
            commit_ms += commit_started.elapsed().as_secs_f64() * 1000.0;
        }
        if !domain_analytics_batch.is_empty() {
            let commit_started = Instant::now();
            domain_analytics_batch.commit()?;
            commit_ms += commit_started.elapsed().as_secs_f64() * 1000.0;
        }
        if !append_history_batch.is_empty() {
            let commit_started = Instant::now();
            append_history_batch.commit()?;
            commit_ms += commit_started.elapsed().as_secs_f64() * 1000.0;
        }
```

With:

```rust
        // Merge all domain-store batches for atomic commit
        batch.merge_from(consume_addr_batch);
        batch.merge_from(domain_analytics_batch);
        batch.merge_from(append_history_batch);
```

Also remove the standalone `batch.commit()?;` at the old line 1408. Instead, batch will be committed in finalization.

Then in the finalization section (at ~line 2970 equivalent), merge `core_batch` and `stats_batch` into `batch` and commit once:

```rust
        {
            let mut core_batch = StoreBatch::new(self.writer.store());
            self.writer
                .insert_blocks_batch(&block_refs, &mut core_batch)?;
            let mut stats_batch = StoreBatch::new(self.writer.store());
            self.write_batch_stats_to_batch(&batch_stats, &mut stats_batch)?;
            batch.merge_from(core_batch);
            batch.merge_from(stats_batch);
            let commit_started = Instant::now();
            if bulk_sync_mode {
                batch.commit_no_wal()?;
            } else {
                batch.commit()?;
            }
            commit_ms += commit_started.elapsed().as_secs_f64() * 1000.0;
        }
```

**Note:** The T1 path has a finalization section similar to T2. Search for the `core_batch`/`stats_batch` pattern after the secondary batch commits. If T1 shares the same finalization code as T2 (they're in the same method with a branch), verify which branch applies.

**Important caveat:** T1's `batch` at line 1385 is inside a `{ }` scope. Removing the scope means `cells_batch` (append-only, line 1395) also lives longer. Since `cells_batch` is committed before `batch` (line 1404) and is a different store, this is safe — just verify no name conflicts.

**Step 3: Verify compilation**

Run: `cargo check -p ckbadger-indexer`

**Step 4: Run tests**

Run: `cargo test -p ckbadger-indexer --lib`

**Step 5: Commit**

```bash
git add crates/indexer/src/sync/batch.rs
git commit -m "fix(sync): atomic domain batch commit in T1 bulk sync path"
```

---

### Task 4: Verify with full test suite

**Step 1: Full Rust test suite**

Run: `cargo test`
Expected: all pass

**Step 2: Clippy**

Run: `cargo clippy`
Expected: no new warnings

**Step 3: Frontend tests (sanity)**

Run: `cd frontend && npx vitest run`
Expected: all pass (no frontend changes, but sanity check)

**Step 4: Final commit if any fixups needed**

---

### Task 5: Update comment at old "Commit all data writes" site

The old comment at line 6081 says "Commit all data writes in a single batch" which was aspirational but false. After Task 1 it becomes true. Verify the comment accurately describes the new behavior after all merges. Remove any stale per-sub-batch timing comments.

---

## Validation Checklist

- [ ] Live sync: single `data_batch.commit()` covers all domain state including finalization
- [ ] T2 bulk: single `consume_batch.commit()` covers all domain state including finalization
- [ ] T1 bulk: single `batch.commit()` covers all domain state including finalization
- [ ] `cells_batch` (append-only store) still committed separately — unaffected
- [ ] `cargo test` passes
- [ ] `cargo clippy` clean
- [ ] No new `StoreBatch::commit()` calls for domain store between merge and final commit
