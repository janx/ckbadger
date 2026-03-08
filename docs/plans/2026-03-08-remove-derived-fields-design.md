# Remove `derived_*` Fields from Sync Status

## Problem

`derived_tip_block_number`, `derived_sync_in_progress`, and `derived_last_synced_at` were
designed for a scenario where the append-only store lags behind the domain store. This never
happens: both stores are written synchronously in the same `write_batch_inner()` call, with
append-only commits completing before domain commits. The fields are dead weight that:

- Confuse TUI display ("Derived" always ahead of "Current" due to 10s publish interval)
- Add 12 dead `ensure_derived_ready()` gates in API routes
- Pollute `SyncStatus`, `SyncStatusData`, and `SyncStatusRow` with unused fields

## Changes by Crate

### ckbadger-store (types + ops)

- Remove from `SyncStatus`: `derived_tip_block_number`, `derived_last_synced_at`,
  `derived_sync_in_progress`
- Remove `derived_sync_in_progress` logic from `init_sync_start()`

### common (SyncStatusData)

- Remove: `derived_tip_block_number`, `derived_last_synced_at`, `derived_sync_in_progress`

### indexer (writer + cache)

- `db/writer/sync.rs`: Remove `derived_*` writes from `update_sync_status()`
- `sync/batch.rs`: Remove `derived_sync_in_progress = false` in `check_bulk_sync_completion()`
- `cache.rs`: Remove `derived_*` fields from `get_sync_status()` builder
- `db/repository.rs`: Remove `derived_*` from cache update in `update_sync_tip()`

### api

- Delete `utils/derived.rs`
- Remove `pub mod derived` from `utils/mod.rs`
- Remove all `ensure_derived_ready()` calls from 12 route files

### tui

- Remove `derived_tip_block`, `derived_lag_blocks`, `derived_sync_in_progress` from
  `SyncStatusRow`
- Remove `derive_sync_status_fields()`, `derived_status_line()`,
  `response_indicates_derived_syncing()` functions
- `build_from_progress()`: Use `max(SyncStatus.tip_block_number,
SyncProgressData.current_block)` as `tip_block` to fix 10s staleness
- `is_bulk_sync`: Use `blocks_behind > threshold` instead of `derived_sync_in_progress`
- Remove `derived_syncing` error detection from API health check

### Tests

- Update all tests referencing `derived_*` fields

## Not in Scope

- `SyncProgressData` struct and its 10s publish loop (still needed for rates, ETA, pipeline
  metrics)
- `update_sync_tip()` in repository (separate cleanup)
- Schema migration (rebuild DB is the standard path)
