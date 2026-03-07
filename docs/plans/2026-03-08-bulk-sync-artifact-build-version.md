# Bulk Sync Artifact Build Version Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add the existing compile-time `buildVersion` string to bulk-sync perf artifacts without introducing any second version calculation path.

**Architecture:** Reuse the existing `CKBADGER_BUILD_VERSION` string from the CLI crate, pass it through `IndexerServiceConfig` into the indexer runtime config, and persist it from the bulk-sync perf writer into `metadata.env` and `report.md`. Keep the value opaque, validate it as non-blank, and avoid any Git/version logic inside `ckbadger-indexer`.

**Tech Stack:** Rust, inline unit tests in `ckbadger` and `ckbadger-indexer`, serde-free env-file artifact output, `cargo test`

---

### Task 1: Add failing tests for build version propagation and artifact output

**Files:**

- Modify: `crates/indexer/src/config.rs`
- Modify: `crates/indexer/src/bulk_sync_perf.rs`
- Modify: `crates/cli/src/main.rs`

**Step 1: Write the failing config validation test**

Add this test in `crates/indexer/src/config.rs`:

```rust
#[test]
fn test_validate_rejects_blank_build_version() {
    let mut config = make_valid_config();
    config.build_version = "   ".to_string();
    let err = config.validate().unwrap_err();
    assert!(err.to_string().contains("build_version must not be blank"));
}
```

**Step 2: Write the failing bulk-sync artifact tests**

Add these tests in `crates/indexer/src/bulk_sync_perf.rs`:

```rust
#[test]
fn test_bulk_sync_perf_run_start_writes_build_version_to_metadata() {
    let dir = TempDir::new().unwrap();
    BulkSyncPerfRun::start_for_test(dir.path(), "run-1", "0.1.0+feature/foo@abcdef123456")
        .unwrap();

    let metadata = std::fs::read_to_string(dir.path().join("run-1/metadata.env")).unwrap();
    assert!(metadata.contains("build_version=0.1.0+feature/foo@abcdef123456"));
}

#[test]
fn test_bulk_sync_perf_completed_run_writes_build_version_to_report_and_latest() {
    let dir = TempDir::new().unwrap();
    let mut run = BulkSyncPerfRun::start_for_test(
        dir.path(),
        "run-1",
        "0.1.0+feature/foo@abcdef123456",
    )
    .unwrap();

    run.finish_completed().unwrap();

    let report = std::fs::read_to_string(dir.path().join("run-1/report.md")).unwrap();
    let latest_metadata = std::fs::read_to_string(dir.path().join("latest/metadata.env")).unwrap();

    assert!(report.contains("Build Version: 0.1.0+feature/foo@abcdef123456"));
    assert!(latest_metadata.contains("build_version=0.1.0+feature/foo@abcdef123456"));
}
```

**Step 3: Write the failing CLI propagation test**

Extract a helper in `crates/cli/src/main.rs` for constructing `IndexerServiceConfig`, then add a test like:

```rust
#[test]
fn test_build_indexer_service_config_uses_cli_build_version() {
    let work = WorkDir::resolve(std::path::Path::new("/tmp/example"));
    let config = CkbadgerConfig::default_for_test();
    let ckb_paths = resolve_ckb_paths(std::path::Path::new("/tmp/example"), &config.ckb).unwrap();

    let service = build_indexer_service_config(
        std::path::Path::new("/tmp/example"),
        &work,
        &config,
        &ckb_paths,
        BUILD_VERSION,
    )
    .unwrap();

    assert_eq!(service.build_version, BUILD_VERSION);
}
```

If `CkbadgerConfig::default_for_test()` does not exist, create the smallest local test fixture instead of adding a new generic helper.

**Step 4: Run tests to verify they fail**

Run:

```bash
cargo test -p ckbadger-indexer test_validate_rejects_blank_build_version -- --nocapture
cargo test -p ckbadger-indexer test_bulk_sync_perf_run_start_writes_build_version_to_metadata test_bulk_sync_perf_completed_run_writes_build_version_to_report_and_latest -- --nocapture
cargo test -p ckbadger test_build_indexer_service_config_uses_cli_build_version -- --nocapture
```

Expected:

- Config test fails because `build_version` does not exist or is not validated.
- Artifact tests fail because bulk-sync perf writer does not accept or write a build version yet.
- CLI test fails because the helper/config field does not exist yet.

**Step 5: Commit**

```bash
git add crates/indexer/src/config.rs crates/indexer/src/bulk_sync_perf.rs crates/cli/src/main.rs
git commit -m "test: cover bulk sync artifact build version"
```

### Task 2: Implement build version plumbing into the indexer config path

**Files:**

- Modify: `crates/cli/src/main.rs`
- Modify: `crates/indexer/src/entry.rs`
- Modify: `crates/indexer/src/config.rs`

**Step 1: Implement the minimal config field additions**

- Add `pub build_version: String` to `IndexerServiceConfig` in `crates/indexer/src/entry.rs`.
- Add `build_version: svc.build_version` in `impl From<IndexerServiceConfig> for Config`.
- Add `pub build_version: String` to `crates/indexer/src/config.rs::Config`.

**Step 2: Add fail-fast validation**

In `crates/indexer/src/config.rs`, extend `validate()`:

```rust
if self.build_version.trim().is_empty() {
    bail!("config: build_version must not be blank");
}
```

Also update `make_valid_config()` in tests to include a non-blank sample value:

```rust
build_version: "0.1.0+feature/foo@abcdef123456".to_string(),
```

**Step 3: Refactor CLI indexer config construction**

In `crates/cli/src/main.rs`, extract the current inline indexer service config construction into a helper:

```rust
fn build_indexer_service_config(
    workdir: &Path,
    work: &WorkDir,
    config: &CkbadgerConfig,
    ckb_paths: &ResolvedCkbPaths,
    build_version: &str,
) -> Result<IndexerServiceConfig> {
    let store_paths = resolve_store_paths(workdir, &config.store);
    let token_labels_path = resolve_token_labels_path(work, resolve_share_dir().as_deref())
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();

    Ok(IndexerServiceConfig {
        domain_data_path: store_paths.domain_data.to_string_lossy().to_string(),
        append_only_data_path: store_paths.append_only_data.to_string_lossy().to_string(),
        bulk_sync_perf_output_root: work.bulk_sync_perf_dir.to_string_lossy().to_string(),
        ckb_rpc_url: config.ckb.rpc_url.clone(),
        ckb_db_path: ckb_paths.ckb_db_path.to_string_lossy().to_string(),
        token_labels_path,
        batch_size: config.indexer.batch_size,
        poll_interval_ms: config.indexer.poll_interval_ms,
        parallel_fetch_size: config.indexer.parallel_fetch_size,
        pipeline_enabled: config.indexer.pipeline_enabled,
        pipeline_buffer: config.indexer.pipeline_buffer,
        bulk_sync_threshold: config.indexer.bulk_sync_threshold,
        build_version: build_version.to_string(),
        store_runtime_config: store_runtime_config(&config.store),
    })
}
```

Then replace the inline constructor in `cmd_internal()` with this helper.

**Step 4: Run tests to verify they pass**

Run:

```bash
cargo test -p ckbadger-indexer test_validate_rejects_blank_build_version -- --nocapture
cargo test -p ckbadger test_build_indexer_service_config_uses_cli_build_version -- --nocapture
```

Expected: PASS

**Step 5: Commit**

```bash
git add crates/cli/src/main.rs crates/indexer/src/entry.rs crates/indexer/src/config.rs
git commit -m "feat(indexer): pass build version through service config"
```

### Task 3: Persist build version in bulk-sync perf artifacts

**Files:**

- Modify: `crates/indexer/src/sync/indexer.rs`
- Modify: `crates/indexer/src/bulk_sync_perf.rs`

**Step 1: Extend perf run startup API**

Update `crates/indexer/src/bulk_sync_perf.rs`:

- Add `build_version: String` to `BulkSyncPerfRun`
- Change:

```rust
pub fn start(output_root: &Path, run_id: impl Into<String>) -> Result<Self>
```

to:

```rust
pub fn start(
    output_root: &Path,
    run_id: impl Into<String>,
    build_version: impl Into<String>,
) -> Result<Self>
```

- Add fail-fast validation:

```rust
let build_version = build_version.into();
if build_version.trim().is_empty() {
    bail!("bulk sync perf build_version must not be blank");
}
```

- Update `start_for_test(...)` to accept the new argument.

**Step 2: Write build version into artifacts**

Update `write_metadata()`:

```rust
let content = format!(
    "run_id={}\nstarted_at_utc={}\nbuild_version={}\n",
    self.run_id, self.started_at_utc, self.build_version
);
```

Update `write_report()` header:

```rust
content.push_str(&format!("- Build Version: {}\n", self.build_version));
```

Keep `latest/` copying logic unchanged so it continues to copy the completed run’s `metadata.env` and `report.md`.

**Step 3: Pass build version from indexer runtime**

In `crates/indexer/src/sync/indexer.rs`, change `maybe_start_bulk_sync_perf_run(...)` to accept `build_version: &str` and pass `&self.config.build_version` from `start_bulk_sync_perf_run(...)` into `BulkSyncPerfRun::start(...)`.

**Step 4: Run tests to verify they pass**

Run:

```bash
cargo test -p ckbadger-indexer test_bulk_sync_perf_run_start_writes_build_version_to_metadata test_bulk_sync_perf_completed_run_writes_build_version_to_report_and_latest -- --nocapture
cargo test -p ckbadger-indexer test_maybe_start_bulk_sync_perf_run_returns_none_when_bulk_sync_disabled -- --nocapture
```

Expected: PASS

**Step 5: Commit**

```bash
git add crates/indexer/src/sync/indexer.rs crates/indexer/src/bulk_sync_perf.rs
git commit -m "feat: add build version to bulk sync perf artifacts"
```

### Task 4: Update docs for the artifact contract

**Files:**

- Modify: `docs/INDEXER_PIPELINE.md`

**Step 1: Write the documentation change**

Update the bulk-sync perf artifact description so `metadata.env` explicitly includes `build_version`.

Suggested wording:

```md
- Fresh-db bulk sync writes perf artifacts directly from the indexer runtime under `workdir/perf/bulk-sync/`.
- `metadata.env` identifies both `run_id` and `build_version`, so artifact comparisons can distinguish runtime executions from binary builds.
```

**Step 2: Run a targeted search check**

Run:

```bash
rg -n "build_version|Build Version" docs/INDEXER_PIPELINE.md crates/indexer/src/bulk_sync_perf.rs
```

Expected: both the docs and writer implementation reference the new artifact field.

**Step 3: Commit**

```bash
git add docs/INDEXER_PIPELINE.md
git commit -m "docs: describe build version in bulk sync artifacts"
```

### Task 5: Final verification

**Files:**

- Verify only

**Step 1: Run focused Rust tests**

Run:

```bash
cargo test -p ckbadger-indexer test_validate_rejects_blank_build_version test_bulk_sync_perf_run_start_writes_build_version_to_metadata test_bulk_sync_perf_completed_run_writes_build_version_to_report_and_latest test_maybe_start_bulk_sync_perf_run_returns_none_when_bulk_sync_disabled -- --nocapture
cargo test -p ckbadger test_build_indexer_service_config_uses_cli_build_version -- --nocapture
```

Expected: PASS

**Step 2: Run crate-level regression checks**

Run:

```bash
cargo test -p ckbadger-indexer --lib -- --nocapture
cargo test -p ckbadger --lib -- --nocapture
```

Expected: PASS

**Step 3: Inspect the diff**

Run:

```bash
git diff -- crates/cli/src/main.rs crates/indexer/src/entry.rs crates/indexer/src/config.rs crates/indexer/src/sync/indexer.rs crates/indexer/src/bulk_sync_perf.rs docs/INDEXER_PIPELINE.md
```

Expected:

- only the planned config plumbing, artifact writer updates, tests, and docs changes appear
- no RocksDB schema or store write path changes appear

**Step 4: Manual artifact sanity check**

After running a fresh-db bulk sync locally, verify:

- `workdir/perf/bulk-sync/<run_id>/metadata.env` contains `build_version=...`
- `workdir/perf/bulk-sync/<run_id>/report.md` contains `Build Version: ...`
- `workdir/perf/bulk-sync/latest/metadata.env` matches the latest completed run’s build version

**Step 5: Commit**

```bash
git commit --allow-empty -m "chore: verify bulk sync artifact build version"
```
