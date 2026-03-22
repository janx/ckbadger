# Background Task Observability

**Date**: 2026-03-22
**Status**: Approved
**Branch**: feat/prefetch-channel-background-sampler

## Goal

Add logging, measurement, and TUI coverage for post-sync background tasks (DOB decode, cache warmup, chart warmup, assets refresh) that currently run as invisible fire-and-forget spawns.

## Principle Alignment

- **CKB Native**: Background tasks process CKB-specific data (DOB spores, chain-derived caches); observability makes these CKB-native processes tangible.
- **Local First**: Operators running ckbadger locally get full visibility into what their node is doing after sync completes.
- **Agent Friendly**: Structured, machine-readable task status enables automated monitoring and scripting.

## Background

### Current State

| Task | Logging | Timing | TUI | IPC |
|------|---------|--------|-----|-----|
| DOB decode worker | tracing info/debug/warn | In-memory counters only, no duration | None | None |
| API cache warmup | tracing info/debug/warn | None | None | None |
| API chart warmup | tracing info | None | None | None |
| Assets refresh loop | tracing trace/debug/warn | None | None | None |
| Prefetch worker | tracing info on finish | PrefetchWorkerStats | None | None |
| Background sampler | None | Samples disk I/O every 200ms | None | None |

All tasks are spawned via `tokio::spawn` with no external progress reporting. Operators can only observe them through log output.

**Prefetch worker and background sampler** are also listed above but are intentionally **out of scope** for this spec. They are bulk-sync-only internal mechanisms whose metrics are already captured in `BulkBuildProgressData` and `PrefetchWorkerStats`. They do not run after sync completes and do not need operator-facing TUI coverage.

### Existing Patterns

The TUI already reads sync progress from RocksDB:
- `get_sync_status()` / `get_sync_progress()` / `get_memory_stats()` — written by indexer, read by TUI via secondary reader
- API-side data reaches TUI via the `/statistics/network` HTTP endpoint (`crates/tui/src/db.rs` `get_chain_info_and_api_service_info()`)

The design follows these existing patterns rather than introducing new communication channels.

## Design Decisions

1. **RocksDB-backed status** (not IPC extension) — follows existing TUI data consumption pattern, works cross-process, naturally persistent.
2. **Batch-level timing** (not per-spore) — sufficient for performance monitoring and ETA without noise.
3. **Single unified TUI section** — compact table listing all tasks, rather than per-task sections or a minimal footer.
4. **API tasks use in-memory state** (not RocksDB writes) — respects the "API is read-only for RocksDB" boundary.

## Data Model

### Types (`crates/common/src/sync.rs`)

```rust
/// Status of all background tasks, stored in a single RocksDB domain key.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct BackgroundTasksData {
    pub tasks: Vec<BackgroundTaskEntry>,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackgroundTaskEntry {
    /// Stable identifier: "dob_decode", "cache_warmup", "chart_warmup", "assets_refresh"
    pub name: String,
    pub state: BackgroundTaskState,
    /// Human-readable status line
    pub message: Option<String>,
    /// Progress numerator (items processed so far)
    pub progress_current: Option<u64>,
    /// Progress denominator (total items, if known)
    pub progress_total: Option<u64>,
    /// Processing rate (items/sec), computed at batch boundaries
    pub rate: Option<f64>,
    /// ETA in seconds, if computable
    pub eta_seconds: Option<f64>,
    /// Unix timestamp when task entered Running state
    pub started_at: Option<i64>,
    /// Elapsed wall-clock time in ms since started_at
    pub elapsed_ms: Option<f64>,
    /// Error message if state is Failed
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BackgroundTaskState {
    Waiting,
    Running,
    Completed,
    Failed,
}
```

Cache key constant in `crates/common/src/sync.rs` (alongside existing `SYNC_STATUS_CACHE_KEY`):
`pub const BACKGROUND_TASKS_CACHE_KEY: &str = "bg:tasks";`

**Note on `#[serde(rename_all = "camelCase")]`**: This annotation is relevant only for the JSON serialization path (API responses, TUI HTTP fetch). RocksDB storage uses **bincode** (matching `SyncStatus`/`RuntimeStatus` in `sync_ops.rs`). The `camelCase` rename has no effect on bincode encoding but is kept on the types for API response serialization.

## Storage Layer

### Store Operations (`crates/ckbadger-store/src/background_task_ops.rs`)

No new column family. Uses existing metadata CF (`CF_SYNC_META`) with a fixed key, same pattern as `get_sync_status()`. Serialization format: **bincode** (matching `SyncStatus`/`RuntimeStatus`).

```rust
/// Read current background tasks state.
pub fn get_background_tasks(&self) -> Result<Option<BackgroundTasksData>>

/// Write background tasks state (full replace).
pub fn put_background_tasks(&self, data: &BackgroundTasksData) -> Result<()>

/// Update a single task entry by name, inserting if absent.
/// Reads current state, applies the update closure, writes back.
pub fn update_background_task(
    &self,
    task_name: &str,
    f: impl FnOnce(&mut BackgroundTaskEntry),
) -> Result<()>
```

The `update_background_task` helper handles the read-modify-write (get -> deserialize -> modify -> serialize -> put). This is non-atomic, same pattern as `update_sync_status` and `update_runtime_status` in `sync_ops.rs`.

**Concurrency safety**: The DOB decode worker runs on a separate tokio task from the main indexer pipeline. The "dob_decode" Waiting entry is initialized in `indexer.rs` *before* `tokio::spawn`, ensuring the entry exists before the worker starts. After that, only the DOB worker's spawned task updates the "dob_decode" entry — no other writer touches this task name. Each task name has a single writer, so no read-modify-write race can occur.

### StoreBatch (`crates/ckbadger-store/src/batch.rs`)

```rust
pub fn put_background_tasks(&mut self, data: &BackgroundTasksData)
```

For tasks that want to write atomically with other batch operations.

### New Query (`crates/ckbadger-store/src/spore_ops.rs`)

```rust
/// Count undecoded DOB spores (prefix scan count).
/// Called once at DOB decode worker start to get progress denominator.
pub fn count_undecoded_dob_spores(&self) -> Result<u64>
```

**Performance note**: This follows the same iteration path as `list_undecoded_dob_spores` (full CF scan with deserialization and filter). This is acceptable as a **one-time startup cost** before the worker begins processing. The count may drift slightly as the worker processes entries, but this is fine for progress display — the denominator is a snapshot, not a live guarantee.

## Instrumenting Background Tasks

### DOB Decode Worker (`crates/indexer/src/sync/dob_decode_worker.rs`)

The worker already holds `Arc<CkbadgerStore>`. Add `Instant`-based timing and `update_background_task` calls:

| Phase | State | Fields Updated |
|-------|-------|----------------|
| Spawn loop waiting for sync | `Waiting` | message: "Waiting for sync to catch up" |
| Worker start (after threshold met) | `Running` | started_at, progress_total (from `count_undecoded_dob_spores`) |
| Each batch boundary (every 500 spores) | `Running` | progress_current, elapsed_ms, rate, eta_seconds, message |
| Successful completion | `Completed` | progress_current = progress_total, elapsed_ms, message with final counts |
| Error | `Failed` | error message |
| Shutdown requested | `Completed` | message: "Shutdown requested", partial progress |

Rate calculation: `total_decoded / elapsed_seconds` at each batch boundary.
ETA calculation: `(progress_total - progress_current) / rate` when rate > 0.

### API Cache Warmup (`crates/api/src/warmup.rs`)

API tasks report to `Arc<RwLock<BackgroundTasksData>>` on `AppState` (not RocksDB — respects read-only boundary).

#### `warmup_assets_cache_once` — task name: "cache_warmup"

| Phase | State | Fields |
|-------|-------|--------|
| Start | `Running` | started_at |
| Success | `Completed` | elapsed_ms |
| Error | `Failed` | error |

No progress_current/total — single operation.

#### `warmup_chart_caches` — task name: "chart_warmup"

| Phase | State | Fields |
|-------|-------|--------|
| Start | `Running` | started_at, progress_total = number of chart types |
| Each chart warmed | `Running` | progress_current, elapsed_ms, message (chart name) |
| All done | `Completed` | elapsed_ms |

#### `refresh_assets_cache_loop` — task name: "assets_refresh"

| Phase | State | Fields |
|-------|-------|--------|
| Loop start | `Running` | started_at |
| Each cycle | `Running` | elapsed_ms of last cycle, message: "Last refresh: Xs ago" |
| Error in cycle | `Running` | message includes warning (loop continues) |

This task never completes (runs forever), so state stays `Running`.

### AppState Changes (`crates/api/src/lib.rs`)

Add field:
```rust
pub background_tasks: Arc<RwLock<BackgroundTasksData>>,
```

Add helper method matching the store's pattern:
```rust
pub fn update_background_task(
    &self,
    task_name: &str,
    f: impl FnOnce(&mut BackgroundTaskEntry),
)
```

## API Endpoint Change

Extend the `/statistics/network` response (`NetworkStats` or the enclosing response type) with:

```rust
#[serde(default)]
pub api_background_tasks: Option<Vec<BackgroundTaskEntry>>,
```

Handler reads from `AppState.background_tasks` and includes it in the response. Backward-compatible (optional field with `#[serde(default)]`, defaults to None for older clients). This reuses the endpoint TUI already calls (`crates/tui/src/db.rs` line 573), requiring no additional HTTP requests.

## TUI Display (`crates/tui/src/ui.rs`)

### Data Source

TUI merges two sources into a single `Vec<BackgroundTaskEntry>`:
1. **Indexer tasks**: `get_background_tasks()` from RocksDB secondary reader — fetched within `get_local_snapshot()` alongside existing sync status/progress reads
2. **API tasks**: `api_background_tasks` field from `/statistics/network` HTTP response — fetched within existing `get_chain_info_and_api_service_info()` call, requiring no additional HTTP requests

### Layout

New "Background Tasks" section, rendered as a compact table after sync progress:

```
┌ Background Tasks ─────────────────────────────────────────────┐
│ Task            State      Progress       Rate     Elapsed    │
│ dob_decode      Running    142/1283       12.3/s   1m 23s     │
│ cache_warmup    Completed  —              —        820ms      │
│ chart_warmup    Running    3/5            —        2.1s       │
│ assets_refresh  Running    —              —        last: 4s   │
└───────────────────────────────────────────────────────────────┘
```

### Display Rules

- **Waiting**: dim/gray text, message in Progress column (e.g. "Waiting for sync")
- **Running**: `current/total` when both known, rate and ETA when available
- **Completed**: final counts and total elapsed; auto-hide after 5 minutes
- **Failed**: red/yellow with truncated error message
- No progress_total: show `—` instead of fraction
- Section hidden entirely when zero active or recently-completed tasks

## Store Boundary Check

- All new writes target the **domain store** (metadata CF) — this is mutable canonical state.
- No changes to append-only store (`CF_CELLS`).
- No new column families — reuses existing metadata CF.
- API remains read-only for RocksDB; API-side tasks use in-memory `Arc<RwLock>`.

## Testing

| Test | Location | What |
|------|----------|------|
| `BackgroundTaskEntry` serde roundtrip | `common/src/sync.rs` | Serialize/deserialize all states via both bincode and JSON |
| `BackgroundTaskState` transitions | `common/src/sync.rs` | Verify all valid state values serialize correctly |
| `update_background_task` insert + update | `ckbadger-store/src/background_task_ops.rs` | Insert new task, update existing, verify other tasks untouched (isolation) |
| `update_background_task` multiple tasks | `ckbadger-store/src/background_task_ops.rs` | Insert "dob_decode" and "cache_warmup", update one, verify the other is unchanged |
| `count_undecoded_dob_spores` | `ckbadger-store/src/spore_ops.rs` | Returns 0 for empty store; correct count with mix of decoded and undecoded test data |
| `/statistics/network` includes `apiBackgroundTasks` | `api/tests/api_integration.rs` | Field present in response when tasks exist |

## Scope

### Files Changed

| File | Why |
|------|-----|
| `crates/common/src/sync.rs` | New types: `BackgroundTasksData`, `BackgroundTaskEntry`, `BackgroundTaskState` |
| `crates/ckbadger-store/src/background_task_ops.rs` (new) | Store read/write/update ops |
| `crates/ckbadger-store/src/batch.rs` | `put_background_tasks` on StoreBatch |
| `crates/ckbadger-store/src/spore_ops.rs` | `count_undecoded_dob_spores` |
| `crates/indexer/src/sync/dob_decode_worker.rs` | Timing + progress reporting |
| `crates/indexer/src/sync/indexer.rs` | Initialize dob_decode task entry as Waiting |
| `crates/api/src/lib.rs` | `background_tasks` field on AppState + helper |
| `crates/api/src/warmup.rs` | Instrument 3 warmup tasks |
| `crates/api/src/routes/statistics.rs` | Include api_background_tasks in `/statistics/network` response |
| `crates/tui/src/db.rs` | Read background tasks from RocksDB in `get_local_snapshot()`, parse api_background_tasks from `/statistics/network` |
| `crates/tui/src/ui.rs` | New Background Tasks section |

### Storage Impact

- One new key in metadata CF (domain store). Value size: ~500 bytes for 4 tasks.
- No schema migration needed. No re-sync required.

## Validation

- `cargo check && cargo clippy` pass
- `cargo test` passes (including new tests)
- `cd frontend && pnpm type-check && pnpm lint` pass (if API response types change)
- Store boundary: domain only, no append-only changes
- Append-only update/delete path check: not applicable (no append-only changes)
