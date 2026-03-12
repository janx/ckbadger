# Graceful Shutdown & Startup Cleanup Optimization

**Date**: 2026-03-12
**Status**: Approved

## Goal

Eliminate the 4+ minute full-table-scan startup cleanup that occurs after every restart, caused by two bugs:

1. Supervisor sends SIGKILL instead of SIGTERM, so indexer never writes clean shutdown marker
2. Startup cleanup runs full CF scans even when no partial data exists

## Principle Alignment

- **Local First**: Faster restart = cheaper local experiments
- **CKB Native**: N/A
- **Agent Friendly**: Predictable, fast restart behavior

## Layer 1: Supervisor Graceful Shutdown

**File**: `crates/cli/src/supervisor.rs`

Replace `child.kill().await` (SIGKILL) with SIGTERM + 10s timeout + SIGKILL fallback:

1. Send SIGTERM via `nix::sys::signal::kill(Pid::from_raw(pid), Signal::SIGTERM)`
2. `tokio::time::timeout(Duration::from_secs(10), child.wait()).await`
3. On timeout: `child.kill().await` (SIGKILL fallback)
4. Log "stopped gracefully" vs "force-killed after timeout"

Indexer already handles SIGTERM (`entry.rs:822-840`): sets shutdown flag, pipeline exits, `mark_runtime_shutdown("sigterm_shutdown", 0)` writes clean marker. No indexer changes needed.

## Layer 2: Skip Cleanup When No Partial Data

**File**: `crates/indexer/src/db/writer/sync.rs` function `init_sync_start_with_options()`

Current (line 220):

```rust
if force_cleanup || has_partial_data { → rollback cleanup }
```

Changed to: only execute rollback cleanup when `has_partial_data == true`. When `force_cleanup == true` but `has_partial_data == false`, skip the rollback and log a warning.

Rationale: RocksDB WriteBatch is atomic. If `has_partial_data` check (block_headers vs tx_index consistency) passes, the last batch either fully committed or fully didn't. There is nothing to clean up.

## Risk Analysis

| Risk                           | Mitigation                                                          |
| ------------------------------ | ------------------------------------------------------------------- |
| SIGTERM during batch write     | RocksDB WriteBatch atomic; `has_partial_data` detects inconsistency |
| Indexer hangs on SIGTERM       | 10s timeout + SIGKILL fallback                                      |
| Skip cleanup misses corruption | `has_partial_data` checks block_headers/tx_index alignment          |
| Stale undo log entries         | Cleaned up during next normal reorg; no correctness impact          |

## Expected Impact

- Restart after clean shutdown: ~0s cleanup (was 263s)
- Restart after SIGKILL/OOM: `has_partial_data` check runs; cleanup only if inconsistency found
- Normal reorg: unchanged (rollback_to < db_tip, blocks exist to remove)

## Scope

| File                                   | Change                                |
| -------------------------------------- | ------------------------------------- |
| `crates/cli/src/supervisor.rs`         | SIGTERM + timeout + SIGKILL           |
| `crates/indexer/src/db/writer/sync.rs` | Skip cleanup when `!has_partial_data` |
| `Cargo.toml` (cli crate)               | Add `nix` dependency for SIGTERM      |

## Validation

- Test: supervisor sends SIGTERM, indexer writes clean shutdown marker
- Test: unclean shutdown with no partial data skips cleanup
- Test: unclean shutdown with partial data still triggers cleanup
- Manual: restart ckbadger, verify <1s startup (no rollback cleanup logs)
