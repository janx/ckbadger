# CKB Workdir Config Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Replace `[ckb].data_path` with `[ckb].workdir`, resolve the final CKB RocksDB path from `ckb.toml` using CKB's own rules, and remove optional unresolved path handling from CLI/API/indexer startup.

**Architecture:** Put all CKB path resolution inside `ckbadger-config` so there is one exact calculation path from `ckbadger.toml` to the final `ckb_db_path`. The CLI resolves that path once before starting services, and API/indexer/label-import consume only the resolved RocksDB path as a required runtime value. The change stays fail-fast and does not introduce any RPC fallback or compatibility layer for legacy `data_path`.

**Tech Stack:** Rust, `serde`, `toml`, `tempfile`, RocksDB secondary opens, inline unit tests in `ckbadger-config`, `ckbadger-indexer`, and `ckbadger-api`

---

### Task 1: Replace the public config contract with `ckb.workdir`

**Files:**

- Modify: `crates/config/src/lib.rs`
- Test: `crates/config/src/lib.rs`

**Step 1: Write the failing tests**

Add or update tests near the existing config tests in `crates/config/src/lib.rs`:

```rust
#[test]
fn test_default_config_toml_declares_ckb_workdir() {
    let toml_str = default_config_toml();
    assert!(toml_str.contains("workdir = "));
    assert!(!toml_str.contains("data_path = "));
}

#[test]
fn test_parse_config_rejects_legacy_ckb_data_path() {
    let err = parse_config(
        r#"
[ckb]
data_path = "/var/lib/ckb/data/db"
"#,
    )
    .unwrap_err();

    assert!(err.to_string().contains("[ckb].data_path has been removed"));
}
```

**Step 2: Run tests to verify they fail**

Run:

```bash
cargo test -p ckbadger-config test_default_config_toml_declares_ckb_workdir --lib
cargo test -p ckbadger-config test_parse_config_rejects_legacy_ckb_data_path --lib
```

Expected:

- The default-config test fails because `default_config_toml()` still emits `data_path`.
- The legacy-config test fails because `parse_config()` still accepts or silently ignores the old key.

**Step 3: Write minimal implementation**

- In `crates/config/src/lib.rs`, change `CkbConfig` from `data_path: Option<String>` to `workdir: Option<String>`.
- Update `Default for CkbConfig` to initialize `workdir` with `Some(String::new())`.
- Update `default_config_toml()` comments and example output to emit `workdir = ""`.
- Make `parse_config()` reject legacy `[ckb].data_path` with an explicit migration error before or during normal TOML parsing.

**Step 4: Run tests to verify they pass**

Run:

```bash
cargo test -p ckbadger-config test_default_config_toml_declares_ckb_workdir --lib
cargo test -p ckbadger-config test_parse_config_rejects_legacy_ckb_data_path --lib
```

Expected: PASS

**Step 5: Commit**

```bash
git add crates/config/src/lib.rs
git commit -m "refactor: replace ckb data path config with workdir"
```

### Task 2: Add the shared CKB path resolver in `ckbadger-config`

**Files:**

- Modify: `crates/config/src/lib.rs`
- Test: `crates/config/src/lib.rs`

**Step 1: Write the failing tests**

Add resolver tests that cover both the happy path and fail-fast cases:

```rust
#[test]
fn test_resolve_ckb_paths_uses_relative_data_dir_default_db_path() {
    let ckbadger_root = TempDir::new().unwrap();
    let ckb_root = ckbadger_root.path().join("node");
    std::fs::create_dir_all(ckb_root.join("data/db")).unwrap();
    std::fs::write(ckb_root.join("ckb.toml"), "data_dir = \"data\"\n").unwrap();

    let config = parse_config("[ckb]\nworkdir = \"node\"\n").unwrap();
    let resolved = resolve_ckb_paths(ckbadger_root.path(), &config.ckb).unwrap();

    assert_eq!(resolved.ckb_db_path, ckb_root.join("data/db"));
}

#[test]
fn test_resolve_ckb_paths_uses_relative_db_path_override() {
    let ckbadger_root = TempDir::new().unwrap();
    let ckb_root = ckbadger_root.path().join("node");
    std::fs::create_dir_all(ckb_root.join("custom-db")).unwrap();
    std::fs::write(
        ckb_root.join("ckb.toml"),
        "data_dir = \"data\"\n[db]\npath = \"custom-db\"\n",
    )
    .unwrap();

    let config = parse_config("[ckb]\nworkdir = \"node\"\n").unwrap();
    let resolved = resolve_ckb_paths(ckbadger_root.path(), &config.ckb).unwrap();

    assert_eq!(resolved.ckb_db_path, ckb_root.join("custom-db"));
}
```

Also add at least one fail-fast test for a missing `ckb.toml` or blank `data_dir`.

**Step 2: Run tests to verify they fail**

Run:

```bash
cargo test -p ckbadger-config test_resolve_ckb_paths_uses_relative_data_dir_default_db_path --lib
cargo test -p ckbadger-config test_resolve_ckb_paths_uses_relative_db_path_override --lib
```

Expected: FAIL because `resolve_ckb_paths()` and its supporting structs do not exist yet.

**Step 3: Write minimal implementation**

- In `crates/config/src/lib.rs`, add:
  - `ResolvedCkbPaths`
  - minimal deserialization structs for the CKB node config file (`data_dir`, optional `[db].path`)
  - `resolve_ckb_paths(work_dir: &Path, ckb: &CkbConfig) -> Result<ResolvedCkbPaths>`
- Match CKB's semantics exactly:
  - resolve `ckb.workdir` relative to the ckbadger workdir
  - read `<ckb.workdir>/ckb.toml`
  - resolve `data_dir` relative to `ckb.workdir` when needed
  - use `[db].path` when present, also relative to `ckb.workdir` when needed
  - otherwise default to `data_dir/db`
  - fail if the final `ckb_db_path` does not exist

**Step 4: Run tests to verify they pass**

Run:

```bash
cargo test -p ckbadger-config test_resolve_ckb_paths_uses_relative_data_dir_default_db_path --lib
cargo test -p ckbadger-config test_resolve_ckb_paths_uses_relative_db_path_override --lib
```

Expected: PASS

**Step 5: Commit**

```bash
git add crates/config/src/lib.rs
git commit -m "feat: resolve CKB RocksDB path from ckb workdir"
```

### Task 3: Pre-resolve `ckb_db_path` in CLI and indexer startup

**Files:**

- Modify: `crates/cli/src/main.rs`
- Modify: `crates/indexer/src/config.rs`
- Modify: `crates/indexer/src/entry.rs`
- Test: `crates/indexer/src/config.rs`
- Test: `crates/indexer/src/entry.rs`

**Step 1: Write the failing tests**

Update the existing indexer tests to require the resolved DB path directly:

```rust
#[test]
fn test_indexer_service_config_converts_to_config() {
    let svc = IndexerServiceConfig {
        domain_data_path: "/data/domain".to_string(),
        append_only_data_path: "/data/append".to_string(),
        bulk_sync_perf_output_root: "/workdir/perf/bulk-sync".to_string(),
        ckb_rpc_url: "http://localhost:8114".to_string(),
        ckb_db_path: "/ckb/data/db".to_string(),
        token_labels_path: "docs/labels".to_string(),
        batch_size: 5000,
        poll_interval_ms: 500,
        parallel_fetch_size: 32,
        pipeline_enabled: false,
        pipeline_buffer: 4,
        bulk_sync_threshold: 100,
        store_runtime_config: StoreRuntimeConfig::default(),
    };

    let config: Config = svc.into();
    assert_eq!(config.ckb_db_path, "/ckb/data/db");
}

#[test]
fn test_validate_rejects_blank_ckb_db_path() {
    let mut config = make_valid_config();
    config.ckb_db_path = "   ".to_string();
    let err = config.validate().unwrap_err();
    assert!(err.to_string().contains("ckb_db_path is required"));
}
```

**Step 2: Run tests to verify they fail**

Run:

```bash
cargo test -p ckbadger-indexer test_indexer_service_config_converts_to_config --lib
cargo test -p ckbadger-indexer test_validate_rejects_blank_ckb_db_path --lib
```

Expected: FAIL because the runtime types still use `Option<String>` `ckb_data_path`.

**Step 3: Write minimal implementation**

- In `crates/indexer/src/config.rs`:
  - rename `ckb_data_path` to `ckb_db_path`
  - make it a required `String`
  - update `validate()` and its helper tests
- In `crates/indexer/src/entry.rs`:
  - change `IndexerServiceConfig` and `LabelImportServiceConfig` to accept `ckb_db_path: String`
  - remove `require_ckb_data_path()`
  - open `CkbChainReader` with `config.ckb_db_path`
- In `crates/cli/src/main.rs`:
  - call `resolve_ckb_paths(workdir, &config.ckb)?` once in each startup path that needs CKB direct reads
  - pass `resolved.ckb_db_path.to_string_lossy().to_string()` into indexer and label import configs

**Step 4: Run tests to verify they pass**

Run:

```bash
cargo test -p ckbadger-indexer test_indexer_service_config_converts_to_config --lib
cargo test -p ckbadger-indexer test_validate_rejects_blank_ckb_db_path --lib
```

Expected: PASS

**Step 5: Commit**

```bash
git add crates/cli/src/main.rs crates/indexer/src/config.rs crates/indexer/src/entry.rs
git commit -m "refactor: pre-resolve CKB db path for indexer startup"
```

### Task 4: Remove unresolved-path handling from API startup and integration tests

**Files:**

- Modify: `crates/api/src/entry.rs`
- Modify: `crates/api/src/lib.rs`
- Modify: `crates/api/src/routes/spore.rs`
- Modify: `crates/api/tests/api_integration.rs`
- Test: `crates/api/src/entry.rs`
- Test: `crates/api/tests/api_integration.rs`

**Step 1: Write the failing tests**

Update the API entry test and add a helper-backed integration test setup:

```rust
#[test]
fn test_api_service_config_fields() {
    let config = ApiServiceConfig {
        domain_data_path: "/data/domain".to_string(),
        append_only_data_path: "/data/append".to_string(),
        ckb_rpc_url: "http://localhost:8114".to_string(),
        ckb_network: "mainnet".to_string(),
        host: "0.0.0.0".to_string(),
        port: 3001,
        rate_limit: 100,
        rate_limit_burst: 200,
        ckb_db_path: "/ckb/data/db".to_string(),
        store_runtime_config: StoreRuntimeConfig::default(),
    };

    assert_eq!(config.ckb_db_path, "/ckb/data/db");
}
```

In `crates/api/tests/api_integration.rs`, add a small helper that creates an empty RocksDB with CKB's expected column families so `create_router()` can open a valid secondary database during tests.

**Step 2: Run tests to verify they fail**

Run:

```bash
cargo test -p ckbadger-api test_api_service_config_fields --lib
cargo test -p ckbadger-api test_network_stats_returns_ok --test api_integration
```

Expected:

- The unit test fails because the field is still named `ckb_data_path`.
- The integration test fails after the API starts requiring a real resolved CKB DB path and the test helper has not been added yet.

**Step 3: Write minimal implementation**

- In `crates/api/src/entry.rs`:
  - rename `ckb_data_path` to `ckb_db_path`
  - remove `require_ckb_data_path()`
  - pass the resolved path straight into `AppConfig`
- In `crates/api/src/lib.rs`:
  - change `AppConfig` to require `ckb_db_path: String`
  - open `CkbChainReader` unconditionally from that path
- In `crates/api/tests/api_integration.rs`:
  - add a test helper that creates a minimal CKB-compatible RocksDB directory
  - pass the resulting path into every `AppConfig` builder
- In `crates/api/src/routes/spore.rs`:
  - update stale `[ckb].data_path` wording to `[ckb].workdir`

**Step 4: Run tests to verify they pass**

Run:

```bash
cargo test -p ckbadger-api test_api_service_config_fields --lib
cargo test -p ckbadger-api test_network_stats_returns_ok --test api_integration
```

Expected: PASS

**Step 5: Commit**

```bash
git add crates/api/src/entry.rs crates/api/src/lib.rs crates/api/src/routes/spore.rs crates/api/tests/api_integration.rs
git commit -m "refactor: require resolved CKB db path in API startup"
```

### Task 5: Update docs and run final verification

**Files:**

- Modify: `docs/INDEXER_PIPELINE.md`
- Modify: `crates/ckb-store-reader/src/lib.rs`
- Modify: `docs/plans/2026-03-06-ckb-workdir-config.md`

**Step 1: Update stale docs and comments**

- Replace `ckb_data_path` / `[ckb].data_path` references with `ckb.workdir` where they describe user configuration.
- Update `crates/ckb-store-reader/src/lib.rs` comments so they describe the argument as the final resolved RocksDB path, not the user-facing config field.
- Keep the docs consistent with the design doc committed in `docs/plans/2026-03-06-ckb-workdir-config-design.md`.

**Step 2: Run targeted verification**

Run:

```bash
cargo test -p ckbadger-config --lib
cargo test -p ckbadger-indexer --lib
cargo test -p ckbadger-api --lib
cargo test -p ckbadger-api --test api_integration
cargo check -p ckbadger-config -p ckbadger-indexer -p ckbadger-api -p ckbadger
rg -n "\[ckb\]\.data_path|ckb_data_path|data_path = " crates docs -g '!target'
```

Expected:

- All targeted Rust tests pass.
- `cargo check` passes for the CLI and affected crates.
- `rg` finds no remaining user-facing config references to the removed `data_path` contract.

**Step 3: Commit**

```bash
git add docs/INDEXER_PIPELINE.md crates/ckb-store-reader/src/lib.rs docs/plans/2026-03-06-ckb-workdir-config.md
git commit -m "docs: align CKB workdir config references"
```

Plan complete and saved to `docs/plans/2026-03-06-ckb-workdir-config.md`.

Two execution options:

1. Subagent-Driven (this session) - I dispatch fresh subagent per task, review between tasks, fast iteration
2. Parallel Session (separate) - Open new session with executing-plans, batch execution with checkpoints

If no preference is given, use option 1 in this session.
