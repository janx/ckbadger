# Background Task Observability Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make post-sync background tasks (DOB decode, cache warmup, chart warmup, assets refresh) observable via RocksDB status, API response, and TUI display.

**Architecture:** Indexer tasks write progress to RocksDB domain store (`CF_SYNC_META`) via bincode-serialized `BackgroundTasksData`. API tasks write to in-memory `Arc<RwLock<BackgroundTasksData>>` on `AppState`. TUI merges both sources into a unified "Background Tasks" section.

**Tech Stack:** Rust (bincode, serde, rocksdb, tokio, ratatui), Axum REST API

**Spec:** `docs/superpowers/specs/2026-03-22-background-task-observability-design.md`

---

## File Structure

| File | Responsibility |
|------|---------------|
| `crates/common/src/sync.rs` | Data model types: `BackgroundTasksData`, `BackgroundTaskEntry`, `BackgroundTaskState` |
| `crates/ckbadger-store/src/keys.rs` | RocksDB key constant `BACKGROUND_TASKS` in `sync_meta_keys` |
| `crates/ckbadger-store/src/background_task_ops.rs` (new) | Store get/set/update ops for background tasks |
| `crates/ckbadger-store/src/lib.rs` | Register new `background_task_ops` module |

**Note:** The spec lists `crates/ckbadger-store/src/batch.rs` (`StoreBatch::put_background_tasks`), but no task in this plan requires batch-level writes — all writers use `update_background_task` on the store directly. The batch method is dropped from the plan. The spec should be updated to reflect this.
| `crates/ckbadger-store/src/spore_ops.rs` | New `count_undecoded_dob_spores()` method |
| `crates/indexer/src/sync/dob_decode_worker.rs` | Instrument DOB worker with timing + progress reporting |
| `crates/indexer/src/sync/indexer.rs` | Initialize "dob_decode" Waiting entry before spawn |
| `crates/api/src/lib.rs` | Add `background_tasks` field to `AppState` + helper |
| `crates/api/src/warmup.rs` | Instrument 3 warmup tasks to report progress |
| `crates/api/src/routes/statistics.rs` | Add `api_background_tasks` to `/statistics/network` response |
| `crates/tui/src/db.rs` | Read indexer tasks from RocksDB, parse API tasks from HTTP |
| `crates/tui/src/ui.rs` | Render "Background Tasks" table section |

---

### Task 1: Data Model Types

**Files:**
- Modify: `crates/common/src/sync.rs` (append after `format_duration_smart` at line ~339, before `MemoryStatsData`)

- [ ] **Step 1: Add `BackgroundTaskState` enum, `BackgroundTaskEntry` struct, and `BackgroundTasksData` struct**

```rust
// --- Background task observability ---

pub const BACKGROUND_TASKS_CACHE_KEY: &str = "bg:tasks";

/// State of a single background task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BackgroundTaskState {
    /// Waiting for precondition (e.g. sync catching up).
    Waiting,
    /// Actively processing.
    Running,
    /// Finished successfully.
    Completed,
    /// Terminated with error.
    Failed,
}

/// Status of a single background task.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackgroundTaskEntry {
    /// Stable identifier: "dob_decode", "cache_warmup", "chart_warmup", "assets_refresh".
    pub name: String,
    pub state: BackgroundTaskState,
    /// Human-readable status line.
    pub message: Option<String>,
    /// Progress numerator (items processed so far).
    pub progress_current: Option<u64>,
    /// Progress denominator (total items, if known).
    pub progress_total: Option<u64>,
    /// Processing rate (items/sec), computed at batch boundaries.
    pub rate: Option<f64>,
    /// ETA in seconds, if computable.
    pub eta_seconds: Option<f64>,
    /// Unix timestamp when task entered Running state.
    pub started_at: Option<i64>,
    /// Elapsed wall-clock time in ms since started_at.
    pub elapsed_ms: Option<f64>,
    /// Error message if state is Failed.
    pub error: Option<String>,
}

/// Status of all background tasks, stored in a single RocksDB domain key.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct BackgroundTasksData {
    pub tasks: Vec<BackgroundTaskEntry>,
    pub updated_at: i64,
}
```

- [ ] **Step 2: Add the types to the public re-exports**

Check the top of `crates/common/src/sync.rs` — the types are already public by virtue of being `pub` in the file. Also check `crates/common/src/lib.rs` to ensure they're re-exported if the pattern requires it. The existing types (`SyncStatusData`, `SyncProgressData`, `BulkBuildProgressData`, `MemoryStatsData`) are re-exported in the `pub use sync::*` line — the new types will be included automatically.

- [ ] **Step 3: Write unit tests for serde roundtrip**

Add to the `#[cfg(test)]` module at the bottom of `crates/common/src/sync.rs` (or create one if absent):

```rust
#[cfg(test)]
mod background_task_tests {
    use super::*;

    #[test]
    fn test_background_task_entry_bincode_roundtrip() {
        let entry = BackgroundTaskEntry {
            name: "dob_decode".to_string(),
            state: BackgroundTaskState::Running,
            message: Some("Processing batch 3".to_string()),
            progress_current: Some(142),
            progress_total: Some(1283),
            rate: Some(12.3),
            eta_seconds: Some(92.7),
            started_at: Some(1711100000),
            elapsed_ms: Some(83000.0),
            error: None,
        };
        let bytes = bincode::serialize(&entry).unwrap();
        let decoded: BackgroundTaskEntry = bincode::deserialize(&bytes).unwrap();
        assert_eq!(decoded.name, "dob_decode");
        assert_eq!(decoded.state, BackgroundTaskState::Running);
        assert_eq!(decoded.progress_current, Some(142));
        assert_eq!(decoded.progress_total, Some(1283));
    }

    #[test]
    fn test_background_tasks_data_json_roundtrip() {
        let data = BackgroundTasksData {
            tasks: vec![BackgroundTaskEntry {
                name: "cache_warmup".to_string(),
                state: BackgroundTaskState::Completed,
                message: None,
                progress_current: None,
                progress_total: None,
                rate: None,
                eta_seconds: None,
                started_at: Some(1711100000),
                elapsed_ms: Some(820.0),
                error: None,
            }],
            updated_at: 1711100001,
        };
        let json = serde_json::to_string(&data).unwrap();
        assert!(json.contains("\"progressCurrent\""));  // camelCase
        let decoded: BackgroundTasksData = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.tasks.len(), 1);
        assert_eq!(decoded.tasks[0].name, "cache_warmup");
    }

    #[test]
    fn test_background_task_state_all_variants_serialize() {
        for state in [
            BackgroundTaskState::Waiting,
            BackgroundTaskState::Running,
            BackgroundTaskState::Completed,
            BackgroundTaskState::Failed,
        ] {
            let bytes = bincode::serialize(&state).unwrap();
            let decoded: BackgroundTaskState = bincode::deserialize(&bytes).unwrap();
            assert_eq!(decoded, state);
        }
    }
}
```

- [ ] **Step 4: Run tests to verify**

Run: `cargo test -p ckbadger-common background_task`
Expected: All 3 tests PASS

- [ ] **Step 5: Commit**

```bash
git add crates/common/src/sync.rs
git commit -m "feat(common): add BackgroundTasksData types for task observability"
```

---

### Task 2: RocksDB Key + Store Operations

**Files:**
- Modify: `crates/ckbadger-store/src/keys.rs:1199-1213` (add key constant)
- Create: `crates/ckbadger-store/src/background_task_ops.rs`
- Modify: `crates/ckbadger-store/src/lib.rs` (register module)

- [ ] **Step 1: Add RocksDB key constant**

In `crates/ckbadger-store/src/keys.rs`, inside the `sync_meta_keys` module (~line 1199), add:

```rust
pub const BACKGROUND_TASKS: &[u8] = b"background_tasks";
```

- [ ] **Step 2: Create `background_task_ops.rs` with get/set/update operations**

Create `crates/ckbadger-store/src/background_task_ops.rs`:

```rust
//! Background task status operations.

use ckbadger_common::{BackgroundTaskEntry, BackgroundTaskState, BackgroundTasksData};

use crate::keys::sync_meta_keys;
use crate::store::CkbadgerStore;

impl CkbadgerStore {
    /// Read current background tasks state from domain store.
    pub fn get_background_tasks(&self) -> anyhow::Result<BackgroundTasksData> {
        match self.get_cf(self.cf_sync_meta(), sync_meta_keys::BACKGROUND_TASKS)? {
            Some(value) => Ok(bincode::deserialize(&value)?),
            None => Ok(BackgroundTasksData::default()),
        }
    }

    /// Write background tasks state (full replace).
    pub fn set_background_tasks(&self, data: &BackgroundTasksData) -> anyhow::Result<()> {
        let value = bincode::serialize(data)?;
        self.put_cf(
            self.cf_sync_meta(),
            sync_meta_keys::BACKGROUND_TASKS,
            &value,
        )
    }

    /// Update a single task entry by name, inserting if absent.
    /// Each task name has a single writer — no concurrent updates on the same name.
    pub fn update_background_task<F>(&self, task_name: &str, update_fn: F) -> anyhow::Result<()>
    where
        F: FnOnce(&mut BackgroundTaskEntry),
    {
        let mut data = self.get_background_tasks()?;
        let entry = match data.tasks.iter_mut().find(|t| t.name == task_name) {
            Some(existing) => existing,
            None => {
                data.tasks.push(BackgroundTaskEntry {
                    name: task_name.to_string(),
                    state: BackgroundTaskState::Waiting,
                    message: None,
                    progress_current: None,
                    progress_total: None,
                    rate: None,
                    eta_seconds: None,
                    started_at: None,
                    elapsed_ms: None,
                    error: None,
                });
                data.tasks.last_mut().unwrap()
            }
        };
        update_fn(entry);
        data.updated_at = chrono::Utc::now().timestamp();
        self.set_background_tasks(&data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_background_tasks_empty_store_returns_default() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        let data = store.get_background_tasks().unwrap();
        assert!(data.tasks.is_empty());
        assert_eq!(data.updated_at, 0);
    }

    #[test]
    fn test_background_tasks_set_and_get_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();

        let data = BackgroundTasksData {
            tasks: vec![BackgroundTaskEntry {
                name: "test_task".to_string(),
                state: BackgroundTaskState::Running,
                message: Some("hello".to_string()),
                progress_current: Some(10),
                progress_total: Some(100),
                rate: Some(5.0),
                eta_seconds: Some(18.0),
                started_at: Some(1711100000),
                elapsed_ms: Some(2000.0),
                error: None,
            }],
            updated_at: 1711100000,
        };
        store.set_background_tasks(&data).unwrap();

        let restored = store.get_background_tasks().unwrap();
        assert_eq!(restored.tasks.len(), 1);
        assert_eq!(restored.tasks[0].name, "test_task");
        assert_eq!(restored.tasks[0].state, BackgroundTaskState::Running);
        assert_eq!(restored.tasks[0].progress_current, Some(10));
    }

    #[test]
    fn test_update_background_task_inserts_new() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();

        store
            .update_background_task("dob_decode", |entry| {
                entry.state = BackgroundTaskState::Waiting;
                entry.message = Some("Waiting for sync".to_string());
            })
            .unwrap();

        let data = store.get_background_tasks().unwrap();
        assert_eq!(data.tasks.len(), 1);
        assert_eq!(data.tasks[0].name, "dob_decode");
        assert_eq!(data.tasks[0].state, BackgroundTaskState::Waiting);
        assert!(data.updated_at > 0);
    }

    #[test]
    fn test_update_background_task_modifies_existing() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();

        store
            .update_background_task("dob_decode", |entry| {
                entry.state = BackgroundTaskState::Waiting;
            })
            .unwrap();

        store
            .update_background_task("dob_decode", |entry| {
                entry.state = BackgroundTaskState::Running;
                entry.progress_current = Some(42);
                entry.progress_total = Some(500);
            })
            .unwrap();

        let data = store.get_background_tasks().unwrap();
        assert_eq!(data.tasks.len(), 1);
        assert_eq!(data.tasks[0].state, BackgroundTaskState::Running);
        assert_eq!(data.tasks[0].progress_current, Some(42));
    }

    #[test]
    fn test_update_background_task_isolates_different_tasks() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();

        store
            .update_background_task("dob_decode", |entry| {
                entry.state = BackgroundTaskState::Running;
                entry.progress_current = Some(10);
            })
            .unwrap();

        store
            .update_background_task("cache_warmup", |entry| {
                entry.state = BackgroundTaskState::Completed;
                entry.elapsed_ms = Some(820.0);
            })
            .unwrap();

        let data = store.get_background_tasks().unwrap();
        assert_eq!(data.tasks.len(), 2);

        let dob = data.tasks.iter().find(|t| t.name == "dob_decode").unwrap();
        assert_eq!(dob.state, BackgroundTaskState::Running);
        assert_eq!(dob.progress_current, Some(10));

        let warmup = data.tasks.iter().find(|t| t.name == "cache_warmup").unwrap();
        assert_eq!(warmup.state, BackgroundTaskState::Completed);
        assert_eq!(warmup.elapsed_ms, Some(820.0));
    }
}
```

- [ ] **Step 3: Register module in `lib.rs`**

In `crates/ckbadger-store/src/lib.rs`, add `mod background_task_ops;` in alphabetical order (before `mod block_ops;`, around line 30):

```rust
mod background_task_ops;
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p ckbadger-store background_task`
Expected: All 5 tests PASS

- [ ] **Step 5: Commit**

```bash
git add crates/ckbadger-store/src/keys.rs crates/ckbadger-store/src/background_task_ops.rs crates/ckbadger-store/src/lib.rs
git commit -m "feat(store): add background task status operations"
```

---

### Task 3: Count Undecoded DOB Spores

**Files:**
- Modify: `crates/ckbadger-store/src/spore_ops.rs` (add method after `list_undecoded_dob_spores`)

- [ ] **Step 1: Add `count_undecoded_dob_spores` method**

In `crates/ckbadger-store/src/spore_ops.rs`, add after the `list_undecoded_dob_spores` method (~line 396):

```rust
    /// Count total undecoded DOB spores. Full CF scan with deserialization.
    /// One-time startup cost — called once when DOB decode worker begins.
    pub fn count_undecoded_dob_spores(&self) -> anyhow::Result<u64> {
        use crate::types::{ObjectEntry, ObjectExtra};

        let iter = self.iterator_cf(self.cf_spore_data(), rocksdb::IteratorMode::Start);
        let mut count: u64 = 0;

        for item in iter {
            let (key, value) = item.map_err(|e| {
                anyhow::anyhow!(
                    "failed to iterate spore_data in count_undecoded_dob_spores: {}",
                    e
                )
            })?;
            let entry: ObjectEntry = bincode::deserialize(&value)?;
            if let ObjectExtra::Spore { content_type, .. } = &entry.extra {
                if content_type.to_ascii_lowercase().starts_with("dob/")
                    && self.get_cf(self.cf_dob_decoded(), &key)?.is_none()
                {
                    count += 1;
                }
            }
        }
        Ok(count)
    }
```

- [ ] **Step 2: Add tests**

Add to the existing `#[cfg(test)]` module in `spore_ops.rs`:

```rust
    #[test]
    fn test_count_undecoded_dob_spores_empty_store() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        let count = store.count_undecoded_dob_spores().unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn test_count_undecoded_dob_spores_with_mixed_data() {
        use crate::types::{ObjectEntry, ObjectExtra, SporeMediaProfile, StorageDependencyTier};
        use crate::batch::StoreBatch;

        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();

        // Helper to build a minimal spore ObjectEntry with given content_type.
        let make_spore = |content_type: &str| ObjectEntry {
            collection_id: None,
            extra: ObjectExtra::Spore {
                content_type: content_type.to_string(),
                content_size: 0,
                media_profile: SporeMediaProfile {
                    tier: StorageDependencyTier::FullyOnCkb,
                    sources: vec![],
                    has_renderable_image: false,
                    issues: vec![],
                },
            },
        };

        // Insert 3 DOB spores and 1 non-DOB spore into cf_spore_data.
        let spore_a = [0x01u8; 32]; // dob/0, undecoded
        let spore_b = [0x02u8; 32]; // dob/1, undecoded
        let spore_c = [0x03u8; 32]; // dob/0, already decoded
        let spore_d = [0x04u8; 32]; // text/plain, not DOB

        // Write spore entries via put_cf on cf_spore_data.
        for (id, ct) in [
            (&spore_a, "dob/0"),
            (&spore_b, "dob/1"),
            (&spore_c, "dob/0"),
            (&spore_d, "text/plain"),
        ] {
            let value = bincode::serialize(&make_spore(ct)).unwrap();
            store.put_cf(store.cf_spore_data(), id, &value).unwrap();
        }

        // Mark spore_c as decoded.
        let decoded_entry = crate::types::DobDecodedEntry {
            traits: vec![],
            svg_markup: None,
            media_sources: vec![],
            decoded_at: 1711100000,
        };
        let decoded_value = bincode::serialize(&decoded_entry).unwrap();
        store.put_cf(store.cf_dob_decoded(), &spore_c, &decoded_value).unwrap();

        // Count should be 2 (spore_a and spore_b; spore_c is decoded, spore_d is not DOB).
        let count = store.count_undecoded_dob_spores().unwrap();
        assert_eq!(count, 2);
    }
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p ckbadger-store count_undecoded`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add crates/ckbadger-store/src/spore_ops.rs
git commit -m "feat(store): add count_undecoded_dob_spores for progress tracking"
```

---

### Task 4: Instrument DOB Decode Worker

**Files:**
- Modify: `crates/indexer/src/sync/dob_decode_worker.rs`
- Modify: `crates/indexer/src/sync/indexer.rs:1270-1317`

- [ ] **Step 1: Add timing and progress reporting to `DobDecodeWorker::run`**

In `crates/indexer/src/sync/dob_decode_worker.rs`, modify the `run` method:

1. At the top of `run()`, before the main loop, query total count and record start:

```rust
    pub async fn run(&self) -> Result<()> {
        info!("DOB decode worker started");

        // Get total for progress tracking (one-time scan).
        let total = self.store.count_undecoded_dob_spores()?;
        let start = std::time::Instant::now();

        self.store.update_background_task("dob_decode", |entry| {
            entry.state = ckbadger_common::BackgroundTaskState::Running;
            entry.started_at = Some(chrono::Utc::now().timestamp());
            entry.progress_current = Some(0);
            entry.progress_total = Some(total);
            entry.message = Some(format!("{} undecoded spores", total));
        })?;

        let mut cursor: Option<Vec<u8>> = None;
        let mut total_decoded: u64 = 0;
        let mut total_skipped: u64 = 0;
```

2. At each batch boundary (after the `for` loop over `batch_entries`, before cursor advance), add progress update:

```rust
            // Update progress at batch boundary
            let elapsed = start.elapsed();
            let elapsed_ms = elapsed.as_secs_f64() * 1000.0;
            let processed = total_decoded + total_skipped;
            let rate = if elapsed.as_secs_f64() > 0.0 {
                total_decoded as f64 / elapsed.as_secs_f64()
            } else {
                0.0
            };
            let eta = if rate > 0.0 && total > processed {
                Some((total - processed) as f64 / rate)
            } else {
                None
            };
            let _ = self.store.update_background_task("dob_decode", |entry| {
                entry.progress_current = Some(processed);
                entry.elapsed_ms = Some(elapsed_ms);
                entry.rate = Some(rate);
                entry.eta_seconds = eta;
                entry.message = Some(format!(
                    "Decoded {}, skipped {}",
                    total_decoded, total_skipped
                ));
            });
```

3. At completion (both normal end and shutdown), update to Completed:

```rust
        // After the main loop ends:
        let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
        let _ = self.store.update_background_task("dob_decode", |entry| {
            entry.state = ckbadger_common::BackgroundTaskState::Completed;
            entry.progress_current = Some(total_decoded + total_skipped);
            entry.elapsed_ms = Some(elapsed_ms);
            entry.rate = None;
            entry.eta_seconds = None;
            entry.message = Some(format!(
                "Done: {} decoded, {} skipped",
                total_decoded, total_skipped
            ));
        });
```

4. In the error path (wrap the `run()` call in `indexer.rs`), update to Failed:

In `indexer.rs` at line 1313, change:
```rust
                if let Err(e) = worker.run().await {
                    warn!(error = %e, "DOB decode worker failed");
                }
```
to:
```rust
                if let Err(e) = worker.run().await {
                    warn!(error = %e, "DOB decode worker failed");
                    let _ = dob_store_for_err.update_background_task("dob_decode", |entry| {
                        entry.state = ckbadger_common::BackgroundTaskState::Failed;
                        entry.error = Some(e.to_string());
                    });
                }
```

(This requires cloning `dob_store` once more before the `tokio::spawn` block for the error-reporting path — store it as `let dob_store_for_err = Arc::clone(&dob_store);`.)

- [ ] **Step 2: Initialize Waiting entry before spawn in `indexer.rs`**

In `crates/indexer/src/sync/indexer.rs`, just before the `tokio::spawn` block (~line 1282), add:

```rust
        // Initialize DOB task as Waiting before spawning the worker.
        let _ = dob_store.update_background_task("dob_decode", |entry| {
            entry.state = ckbadger_common::BackgroundTaskState::Waiting;
            entry.message = Some("Waiting for sync to catch up".to_string());
        });
```

And inside the spawn's wait-loop, update the Waiting message with the first iteration so TUI shows it:

No change needed — the Waiting state is already set. The spawn loop just sleeps until the threshold is met, then `run()` transitions to Running.

- [ ] **Step 3: Run check**

Run: `cargo check -p ckbadger-indexer`
Expected: No errors

- [ ] **Step 4: Commit**

```bash
git add crates/indexer/src/sync/dob_decode_worker.rs crates/indexer/src/sync/indexer.rs
git commit -m "feat(indexer): instrument DOB decode worker with progress tracking"
```

---

### Task 5: Instrument API Background Tasks

**Files:**
- Modify: `crates/api/src/lib.rs`
- Modify: `crates/api/src/warmup.rs`

- [ ] **Step 1: Add `background_tasks` field and helper to `AppState`**

In `crates/api/src/lib.rs`, add to `AppState` struct (after `asset_cache_warmup_error` field):

```rust
    /// Background task status for observability (API-side tasks only).
    pub background_tasks: Arc<RwLock<BackgroundTasksData>>,
```

Add import at top:
```rust
use ckbadger_common::{BackgroundTaskEntry, BackgroundTaskState, BackgroundTasksData};
```

Add helper method in the `impl AppState` block:

```rust
    /// Update a single API-side background task by name, inserting if absent.
    pub fn update_background_task(&self, task_name: &str, f: impl FnOnce(&mut BackgroundTaskEntry)) {
        let mut data = self
            .background_tasks
            .write()
            .expect("background tasks lock poisoned");
        let entry = match data.tasks.iter_mut().find(|t| t.name == task_name) {
            Some(existing) => existing,
            None => {
                data.tasks.push(BackgroundTaskEntry {
                    name: task_name.to_string(),
                    state: BackgroundTaskState::Waiting,
                    message: None,
                    progress_current: None,
                    progress_total: None,
                    rate: None,
                    eta_seconds: None,
                    started_at: None,
                    elapsed_ms: None,
                    error: None,
                });
                data.tasks.last_mut().unwrap()
            }
        };
        f(entry);
        data.updated_at = chrono::Utc::now().timestamp();
    }
```

Initialize the field in `AppState` construction (in the `start_api` or wherever `AppState` is built):

```rust
        background_tasks: Arc::new(RwLock::new(BackgroundTasksData::default())),
```

- [ ] **Step 2: Instrument `warmup_assets_cache_once` (task name: "cache_warmup")**

In `crates/api/src/warmup.rs`, modify `warmup_assets_cache_once`:

```rust
pub async fn warmup_assets_cache_once(state: Arc<AppState>) -> anyhow::Result<()> {
    let start = std::time::Instant::now();
    state.update_background_task("cache_warmup", |entry| {
        entry.state = BackgroundTaskState::Running;
        entry.started_at = Some(chrono::Utc::now().timestamp());
        entry.message = Some("Warming up asset caches...".to_string());
    });

    let result =
        tokio::task::spawn_blocking(move || {
            let r = refresh_assets_cache_sync(&state);
            let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
            match &r {
                Ok(()) => {
                    state.update_background_task("cache_warmup", |entry| {
                        entry.state = BackgroundTaskState::Completed;
                        entry.elapsed_ms = Some(elapsed_ms);
                        entry.message = Some("Asset caches ready".to_string());
                    });
                }
                Err(e) => {
                    state.update_background_task("cache_warmup", |entry| {
                        entry.state = BackgroundTaskState::Failed;
                        entry.elapsed_ms = Some(elapsed_ms);
                        entry.error = Some(e.to_string());
                    });
                }
            }
            r
        })
        .await
        .map_err(|e| anyhow::anyhow!("assets cache warmup task panicked: {}", e))?;
    result
}
```

Note: The `state` Arc is moved into `spawn_blocking`. Adjust the existing code to ensure `state` is accessible in both the update and the refresh call. You may need to clone the Arc before the spawn.

- [ ] **Step 3: Instrument `warmup_chart_caches` (task name: "chart_warmup")**

In `warmup_chart_caches`, add progress tracking around each chart type warmup. The function warms up multiple chart types sequentially. Count them and report progress.

At the start:
```rust
    state.update_background_task("chart_warmup", |entry| {
        entry.state = BackgroundTaskState::Running;
        entry.started_at = Some(chrono::Utc::now().timestamp());
    });
```

After each chart type is warmed, increment progress. At completion:
```rust
    state.update_background_task("chart_warmup", |entry| {
        entry.state = BackgroundTaskState::Completed;
        entry.elapsed_ms = Some(elapsed_ms);
        entry.message = Some("Chart caches ready".to_string());
    });
```

- [ ] **Step 4: Instrument `refresh_assets_cache_loop` (task name: "assets_refresh")**

In `refresh_assets_cache_loop`, at loop start:
```rust
    state.update_background_task("assets_refresh", |entry| {
        entry.state = BackgroundTaskState::Running;
        entry.started_at = Some(chrono::Utc::now().timestamp());
        entry.message = Some("Refresh loop active".to_string());
    });
```

After each successful refresh cycle:
```rust
    state.update_background_task("assets_refresh", |entry| {
        entry.elapsed_ms = Some(cycle_elapsed_ms);
        entry.message = Some(format!("Last refresh: {:.1}s", cycle_elapsed_ms / 1000.0));
    });
```

Add `Instant::now()` at each cycle start to measure `cycle_elapsed_ms`.

- [ ] **Step 5: Run check**

Run: `cargo check -p ckbadger-api`
Expected: No errors

- [ ] **Step 6: Commit**

```bash
git add crates/api/src/lib.rs crates/api/src/warmup.rs
git commit -m "feat(api): instrument warmup tasks with background task reporting"
```

---

### Task 6: Extend `/statistics/network` Response

**Files:**
- Modify: `crates/api/src/routes/statistics.rs`

- [ ] **Step 1: Add `api_background_tasks` field to `NetworkStats`**

In `crates/api/src/routes/statistics.rs`, add to the `NetworkStats` struct (~line 140):

```rust
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_background_tasks: Option<Vec<BackgroundTaskEntry>>,
```

Add import:
```rust
use ckbadger_common::BackgroundTaskEntry;
```

- [ ] **Step 2: Populate the field in `fetch_network_stats_from_db`**

In the function that builds `NetworkStats`, read from `AppState.background_tasks`:

```rust
    let api_bg_tasks = {
        let data = state.background_tasks.read().expect("background tasks lock poisoned");
        if data.tasks.is_empty() {
            None
        } else {
            Some(data.tasks.clone())
        }
    };
```

Set `api_background_tasks: api_bg_tasks` in the `NetworkStats` construction.

- [ ] **Step 3: Run check**

Run: `cargo check -p ckbadger-api`
Expected: No errors

- [ ] **Step 4: Commit**

```bash
git add crates/api/src/routes/statistics.rs
git commit -m "feat(api): include background task status in /statistics/network"
```

---

### Task 7: TUI Data Layer

**Files:**
- Modify: `crates/tui/src/db.rs`

- [ ] **Step 1: Add `BackgroundTaskEntry` import and API deserialization**

In `crates/tui/src/db.rs`, add `api_background_tasks` field to `ApiNetworkStats`:

```rust
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApiNetworkStats {
    pub latest_block: i64,
    pub avg_block_time: String,
    pub hash_rate: String,
    pub difficulty: String,
    pub epoch: String,
    pub tps: String,
    pub transactions_per_day: String,
    #[serde(default)]
    pub api_background_tasks: Option<Vec<BackgroundTaskEntry>>,
}
```

Add import:
```rust
use ckbadger_common::BackgroundTaskEntry;
```

- [ ] **Step 2: Read indexer background tasks from RocksDB in `get_local_snapshot`**

Extend the return type of `get_local_snapshot` to include `Option<BackgroundTasksData>`:

```rust
    pub async fn get_local_snapshot(
        &self,
    ) -> (
        Result<SyncStatusRow>,
        Option<MemoryStatsData>,
        Option<RuntimeDiagData>,
        Option<BackgroundTasksData>,
    ) {
        self.refresh_store();
        let bg_tasks = self.store.as_ref().and_then(|s| s.get_background_tasks().ok());
        (
            self.get_sync_status_without_refresh(),
            self.get_memory_stats_without_refresh(),
            self.get_runtime_diag_without_refresh(),
            bg_tasks,
        )
    }
```

Add import:
```rust
use ckbadger_common::BackgroundTasksData;
```

- [ ] **Step 3: Pass API background tasks from `get_chain_info_and_api_service_info`**

Extend the return type to include `Option<Vec<BackgroundTaskEntry>>`:

In the function, after successfully deserializing `ApiNetworkStats`, extract the `api_background_tasks` field and return it alongside the existing `ChainInfoData` and `ApiServiceInfo`.

Update the function signature:
```rust
    pub async fn get_chain_info_and_api_service_info(
        &self,
    ) -> (Option<ChainInfoData>, ApiServiceInfo, Option<Vec<BackgroundTaskEntry>>) {
```

In the success path where `ApiNetworkStats` is deserialized, extract:
```rust
        let api_bg_tasks = stats.api_background_tasks.clone();
```

Return it as the third tuple element.

- [ ] **Step 4: Update all callers of `get_local_snapshot` and `get_chain_info_and_api_service_info`**

In `crates/tui/src/ui.rs`, the `tokio::join!` at ~line 334 destructures these return values. Update the destructuring to capture the new fields and pass them down to the rendering code.

- [ ] **Step 5: Run check**

Run: `cargo check -p ckbadger-tui`
Expected: No errors

- [ ] **Step 6: Commit**

```bash
git add crates/tui/src/db.rs crates/tui/src/ui.rs
git commit -m "feat(tui): read background task status from RocksDB and API"
```

---

### Task 8: TUI Rendering

**Files:**
- Modify: `crates/tui/src/ui.rs`

- [ ] **Step 1: Add a `build_background_tasks_section` rendering function**

Create a function that takes merged `Vec<BackgroundTaskEntry>` and renders a ratatui table:

```rust
fn build_background_tasks_section(
    tasks: &[BackgroundTaskEntry],
    area: Rect,
    buf: &mut Buffer,
) {
    // Filter: hide Completed tasks older than 5 minutes (check started_at + elapsed_ms)
    // If no visible tasks, return early (section hidden)

    let block = Block::default()
        .title(" Background Tasks ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray));

    let header = Row::new(vec!["Task", "State", "Progress", "Rate", "Elapsed"])
        .style(Style::default().fg(Color::DarkGray));

    let rows: Vec<Row> = tasks.iter().filter_map(|task| {
        // Build row based on task.state:
        // Waiting: dim gray, message in Progress column
        // Running: progress_current/progress_total or "—", rate, elapsed
        // Completed: final counts, total elapsed
        // Failed: red, truncated error
        // ...
        Some(build_task_row(task))
    }).collect();

    let table = Table::new(rows, [
        Constraint::Length(16),  // Task name
        Constraint::Length(11),  // State
        Constraint::Length(14),  // Progress
        Constraint::Length(9),   // Rate
        Constraint::Length(10),  // Elapsed
    ])
    .header(header)
    .block(block);

    Widget::render(table, area, buf);
}
```

Helper for individual rows:
```rust
fn build_task_row(task: &BackgroundTaskEntry) -> Row<'static> {
    let state_str = match task.state {
        BackgroundTaskState::Waiting => "Waiting",
        BackgroundTaskState::Running => "Running",
        BackgroundTaskState::Completed => "Completed",
        BackgroundTaskState::Failed => "Failed",
    };

    let state_style = match task.state {
        BackgroundTaskState::Waiting => Style::default().fg(Color::DarkGray),
        BackgroundTaskState::Running => Style::default().fg(Color::Green),
        BackgroundTaskState::Completed => Style::default().fg(Color::Cyan),
        BackgroundTaskState::Failed => Style::default().fg(Color::Red),
    };

    let progress = match (task.progress_current, task.progress_total) {
        (Some(c), Some(t)) => format!("{}/{}", c, t),
        (Some(c), None) => format!("{}", c),
        _ => task.message.clone().unwrap_or_else(|| "—".to_string()),
    };

    let rate = task.rate.map_or("—".to_string(), |r| format!("{:.1}/s", r));

    let elapsed = task.elapsed_ms.map_or("—".to_string(), |ms| {
        format_duration_smart(ms / 1000.0)
    });

    Row::new(vec![
        Cell::from(task.name.clone()),
        Cell::from(state_str).style(state_style),
        Cell::from(progress),
        Cell::from(rate),
        Cell::from(elapsed),
    ])
}
```

- [ ] **Step 2: Merge indexer + API task entries in the main render function**

In the main TUI render/update function, merge the two sources:

```rust
    let mut all_bg_tasks: Vec<BackgroundTaskEntry> = Vec::new();
    if let Some(indexer_bg) = &indexer_background_tasks {
        all_bg_tasks.extend(indexer_bg.tasks.iter().cloned());
    }
    if let Some(api_bg) = &api_background_tasks {
        all_bg_tasks.extend(api_bg.iter().cloned());
    }
```

- [ ] **Step 3: Allocate layout area for the new section**

Find the layout split in the main render function. Add a conditional chunk for the "Background Tasks" section after the sync progress area. The section should only take space if there are visible tasks.

Calculate height: header (1) + border (2) + rows (task count). E.g., for 4 tasks: 7 lines.

- [ ] **Step 4: Run check**

Run: `cargo check -p ckbadger-tui`
Expected: No errors

- [ ] **Step 5: Run full project check**

Run: `cargo check && cargo clippy`
Expected: No errors or warnings

- [ ] **Step 6: Commit**

```bash
git add crates/tui/src/ui.rs
git commit -m "feat(tui): render Background Tasks section with merged indexer+API data"
```

---

### Task 9: Integration Test

**Files:**
- Modify: `crates/api/tests/api_integration.rs`

- [ ] **Step 1: Add test for `api_background_tasks` in `/statistics/network`**

Add a test that verifies the field appears in the response. This depends on the existing test setup pattern — follow the existing `test_scripts_list_returns_warmup_pending_when_script_cache_missing` test as a reference:

```rust
#[tokio::test]
async fn test_network_stats_includes_api_background_tasks() {
    // Setup test app state with a background task entry
    // Call GET /statistics/network
    // Assert response contains apiBackgroundTasks field
    // Assert it deserializes correctly
}
```

The exact setup depends on how the existing integration tests create `AppState`. Follow the existing patterns in `api_integration.rs`.

- [ ] **Step 2: Run integration tests**

Run: `cargo test -p ckbadger-api test_network_stats_includes_api_background_tasks`
Expected: PASS

- [ ] **Step 3: Run full test suite**

Run: `cargo test --lib`
Expected: All tests PASS

- [ ] **Step 4: Commit**

```bash
git add crates/api/tests/api_integration.rs
git commit -m "test(api): verify background tasks appear in /statistics/network"
```

---

### Task 10: Final Verification

- [ ] **Step 1: Run pre-commit checks**

Run: `cargo check && cargo clippy && cd frontend && pnpm type-check && pnpm lint`
Expected: All pass

- [ ] **Step 2: Run full test suite**

Run: `cargo test && cd frontend && npx vitest run`
Expected: All pass

- [ ] **Step 3: Final commit if any fixups needed**

If clippy or tests revealed issues, fix and commit with appropriate message.
