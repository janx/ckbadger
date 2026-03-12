# Graceful Shutdown & Startup Cleanup Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Eliminate 4+ minute startup cleanup by fixing supervisor to send SIGTERM (not SIGKILL) and skipping rollback when no partial data exists.

**Architecture:** Two independent layers. Layer 1 changes supervisor to gracefully stop children with SIGTERM + 10s timeout + SIGKILL fallback. Layer 2 changes startup cleanup logic to skip full-CF-scan rollback when `has_partial_data == false`, regardless of `force_cleanup` flag.

**Tech Stack:** Rust, tokio, Unix signals (libc), RocksDB

---

### Task 1: Layer 2 — Skip cleanup when no partial data (test first)

**Files:**

- Modify: `crates/indexer/src/db/writer/sync.rs:192-252` (init_sync_start_with_options)
- Test: `crates/indexer/src/db/writer/sync.rs:536` (existing test to update + new test)

**Step 1: Update existing test to expect new behavior**

The existing test `test_init_sync_start_forces_cleanup_without_partial_data` (line 536) currently asserts that force_cleanup runs rollback even without partial data. Update it to assert the new behavior: force_cleanup WITHOUT partial data should SKIP rollback.

In `crates/indexer/src/db/writer/sync.rs`, replace the test at line 536:

```rust
#[test]
fn test_init_sync_start_forces_cleanup_without_partial_data() {
    let (_dir, store, append_store, writer) = setup();
    let lock_hash = vec![0xCC; 32];

    // Write some addr_balance data that would be wiped by a full rollback to -1
    store
        .put_addr_balance_direct(
            &lock_hash,
            &AddressBalance {
                balance: 789,
                ..Default::default()
            },
        )
        .unwrap();

    // force_cleanup=true but no partial data → should skip rollback, data preserved
    writer
        .init_sync_start_with_options(append_store.as_ref(), 0, false, true)
        .unwrap();

    // addr_balance should survive because no rollback was executed
    assert!(store.get_addr_balance(&lock_hash).unwrap().is_some());
    let status = store.get_sync_status().unwrap();
    assert!(status.sync_started_at.is_some());
}
```

**Step 2: Add test for force_cleanup WITH partial data still triggers cleanup**

Add a new test after the updated one:

```rust
#[test]
fn test_init_sync_start_forces_cleanup_with_partial_data() {
    let (_dir, store, append_store, writer) = setup();

    // Write block headers at 0 and 1, then init from block 0 with force_cleanup
    let mut batch = StoreBatch::new(&store);
    batch.put_block_header(0, &make_header(0x60, 1_700_000_000_000));
    batch.put_block_header(1, &make_header(0x61, 1_700_000_010_000));
    batch.commit().unwrap();

    // force_cleanup=true AND partial data (block 1 beyond start_block 0) → cleanup runs
    writer
        .init_sync_start_with_options(append_store.as_ref(), 0, false, true)
        .unwrap();

    // Block 1 should be cleaned up
    assert!(store.get_block_header(1).unwrap().is_none());
}
```

**Step 3: Run tests to verify the new test fails (old behavior)**

Run: `cargo test -p ckbadger-indexer test_init_sync_start_forces_cleanup -- --nocapture`

Expected: `test_init_sync_start_forces_cleanup_without_partial_data` FAILS (addr_balance wiped by unnecessary rollback). The new `_with_partial_data` test should PASS.

**Step 4: Implement the change**

In `crates/indexer/src/db/writer/sync.rs`, replace lines 220-251 in `init_sync_start_with_options()`:

Current:

```rust
        if force_cleanup || has_partial_data {
            info!(
                start_block,
                next_block,
                force_cleanup,
                has_partial_data,
                cleanup_reason,
                "Cleaning up partial data before sync start"
            );

            // Use the store's rollback mechanism to clean up everything
            let rollback_target =
                if start_block >= 0 && self.store.get_block_header(start_block)?.is_none() {
                    warn!(
                        start_block,
                        "Startup cleanup tip header missing; rolling back to -1 for full cleanup"
                    );
                    -1
                } else {
                    start_block
                };
            self.store
                .rollback_to_block_with_append_only_store(rollback_target, Some(append_store))?;
            info!(
                start_block,
                rollback_target, next_block, cleanup_reason, "Startup cleanup complete"
            );
        } else {
            info!(
                start_block,
                next_block, cleanup_reason, "Skipping startup rollback cleanup"
            );
        }
```

Replace with:

```rust
        if has_partial_data {
            info!(
                start_block,
                next_block,
                force_cleanup,
                has_partial_data,
                cleanup_reason,
                "Cleaning up partial data before sync start"
            );

            // Use the store's rollback mechanism to clean up everything
            let rollback_target =
                if start_block >= 0 && self.store.get_block_header(start_block)?.is_none() {
                    warn!(
                        start_block,
                        "Startup cleanup tip header missing; rolling back to -1 for full cleanup"
                    );
                    -1
                } else {
                    start_block
                };
            self.store
                .rollback_to_block_with_append_only_store(rollback_target, Some(append_store))?;
            info!(
                start_block,
                rollback_target, next_block, cleanup_reason, "Startup cleanup complete"
            );
        } else if force_cleanup {
            info!(
                start_block,
                next_block,
                force_cleanup,
                has_partial_data,
                cleanup_reason,
                "Skipping rollback cleanup: force_cleanup requested but no partial data detected (atomic WriteBatch guarantees consistency)"
            );
        } else {
            info!(
                start_block,
                next_block, cleanup_reason, "Skipping startup rollback cleanup"
            );
        }
```

**Step 5: Update `needs_startup_cleanup_with_force` to match new semantics**

Check if `needs_startup_cleanup_with_force` is used elsewhere. It currently returns `true` when `force && !has_partial`. If the function is only used in tests, update the test at line 530 to match:

```rust
#[test]
fn test_needs_startup_cleanup_with_force_reports_true_without_partial_data() {
    let (_dir, _store, _append_store, writer) = setup();
    // force=true still reports needs_cleanup=true (the decision to skip rollback
    // happens inside init_sync_start_with_options, not in needs_startup_cleanup)
    assert!(writer.needs_startup_cleanup_with_force(0, true).unwrap());
}
```

This test should still pass — `needs_startup_cleanup_with_force` is a diagnostic function, the skip logic lives in `init_sync_start_with_options`.

**Step 6: Run all sync tests**

Run: `cargo test -p ckbadger-indexer -- sync --nocapture`
Expected: ALL PASS

**Step 7: Commit**

```bash
git add crates/indexer/src/db/writer/sync.rs
git commit -m "fix: skip startup rollback cleanup when no partial data detected

force_cleanup=true without has_partial_data no longer triggers full CF scans.
RocksDB atomic WriteBatch guarantees: if block_headers and tx_index are
consistent, no partial state exists to clean up.

Reduces restart-after-unclean-shutdown from ~264s to ~0s for the common
case where the process was killed cleanly between batch writes."
```

---

### Task 2: Layer 1 — Supervisor sends SIGTERM with timeout

**Files:**

- Modify: `crates/cli/Cargo.toml` (add libc dep)
- Modify: `crates/cli/src/supervisor.rs:17-20,30-33,178-186` (graceful stop)
- Test: `crates/cli/src/supervisor.rs` (existing test update)

**Step 1: Add libc to cli crate dependencies**

No need for `nix` crate — use `libc::kill()` directly (already available transitively, but add explicit dep for clarity). Actually, `libc` is lighter than `nix`. Check if libc is already a transitive dep:

In `crates/cli/Cargo.toml`, add under `[dependencies]`:

```toml
libc = "0.2"
```

And in workspace `Cargo.toml` under `[workspace.dependencies]`, add:

```toml
libc = "0.2"
```

Then update `crates/cli/Cargo.toml` to use workspace:

```toml
libc = { workspace = true }
```

**Step 2: Add graceful shutdown constant and helper function**

In `crates/cli/src/supervisor.rs`, add after the `HEALTH_CHECK_INTERVAL` constant (line 41):

```rust
/// Time to wait for a child to exit after SIGTERM before sending SIGKILL.
const GRACEFUL_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(10);
```

Add a helper function before `run_supervisor` (after the `SupervisorState` struct):

```rust
/// Stop a child process gracefully: SIGTERM → wait → SIGKILL fallback.
async fn stop_child_gracefully(name: &str, child: &mut Child) {
    let pid = child.id().unwrap_or(0);
    if pid == 0 {
        // Process already exited
        return;
    }

    // Send SIGTERM
    // SAFETY: pid is a valid child process ID obtained from tokio::process::Child
    let sigterm_result = unsafe { libc::kill(pid as i32, libc::SIGTERM) };
    if sigterm_result != 0 {
        warn!(service = %name, pid, "failed to send SIGTERM, falling back to SIGKILL");
        let _ = child.kill().await;
        let _ = child.wait().await;
        return;
    }

    // Wait for graceful exit with timeout
    match tokio::time::timeout(GRACEFUL_SHUTDOWN_TIMEOUT, child.wait()).await {
        Ok(Ok(status)) => {
            info!(service = %name, pid, ?status, "service stopped gracefully");
        }
        Ok(Err(e)) => {
            warn!(service = %name, pid, error = %e, "error waiting for service, sending SIGKILL");
            let _ = child.kill().await;
            let _ = child.wait().await;
        }
        Err(_) => {
            warn!(service = %name, pid, timeout_secs = GRACEFUL_SHUTDOWN_TIMEOUT.as_secs(),
                "service did not exit after SIGTERM, sending SIGKILL");
            let _ = child.kill().await;
            let _ = child.wait().await;
        }
    }
}
```

**Step 3: Replace kill() calls in shutdown path**

In `crates/cli/src/supervisor.rs`, replace lines 178-186:

Current:

```rust
    // Stop all children
    {
        let mut locked = state.lock().await;
        locked.shutdown_requested = true;
        for managed in &mut locked.children {
            info!(service = %managed.name, pid = managed.pid(), "stopping service");
            let _ = managed.child.kill().await;
        }
    }
```

Replace with:

```rust
    // Stop all children gracefully (SIGTERM + timeout + SIGKILL)
    {
        let mut locked = state.lock().await;
        locked.shutdown_requested = true;
        for managed in &mut locked.children {
            info!(service = %managed.name, pid = managed.pid(), "stopping service");
            stop_child_gracefully(&managed.name, &mut managed.child).await;
        }
    }
```

**Step 4: Add `use libc` import**

Add to imports at top of `crates/cli/src/supervisor.rs` (after line 8):

```rust
use libc;
```

**Step 5: Run cargo check**

Run: `cargo check -p ckbadger`
Expected: compiles cleanly

**Step 6: Run existing supervisor tests**

Run: `cargo test -p ckbadger -- supervisor --nocapture`
Expected: ALL PASS (existing tests don't test the shutdown-of-real-children path)

**Step 7: Run clippy**

Run: `cargo clippy -p ckbadger`
Expected: no new warnings

**Step 8: Commit**

```bash
git add Cargo.toml crates/cli/Cargo.toml crates/cli/src/supervisor.rs
git commit -m "fix: supervisor sends SIGTERM before SIGKILL for graceful shutdown

Replace immediate child.kill() (SIGKILL) with SIGTERM + 10s timeout +
SIGKILL fallback. This allows the indexer to write its clean shutdown
marker to RocksDB, preventing unnecessary full-CF-scan rollback cleanup
on next startup."
```

---

### Task 3: Full verification

**Step 1: Run all Rust tests**

Run: `cargo test`
Expected: ALL PASS

**Step 2: Run clippy on full project**

Run: `cargo clippy`
Expected: no new warnings

**Step 3: Manual verification (if running instance available)**

1. Start ckbadger: `ckbadger run`
2. Wait for tip sync
3. Stop with Ctrl+C
4. Check `indexer.log` for `"service stopped gracefully"` (not force-killed)
5. Start again: `ckbadger run`
6. Check `indexer.log` for `"no_force_cleanup_signal"` or `"active_run_has_clean_shutdown_marker"` (no rollback cleanup)
7. Verify sync resumes within seconds (no 4-minute delay)
