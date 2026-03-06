# TUI System Workdirs Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add a `Workdir` section to the TUI `System` tab and extend `Store Paths` to show the resolved CKB RocksDB path.

**Architecture:** Keep all path resolution in the CLI/config layer. `cmd_tui()` resolves the runtime paths once, `TuiServiceConfig` carries them into `TuiDb`, and the UI renders those values through small pure helper functions that are easy to test.

**Tech Stack:** Rust, ratatui, inline crate tests, `ckbadger-config` path resolvers

---

### Task 1: Add failing tests for runtime path fields

**Files:**

- Modify: `crates/tui/src/entry.rs`
- Modify: `crates/tui/src/db.rs`

**Step 1: Write the failing tests**

- Extend the `TuiServiceConfig` field test to include:
  - `ckbadger_workdir`
  - `ckb_workdir`
  - `ckb_db_path`
- Add a `TuiDb` accessor test that expects those three resolved paths to be exposed.

**Step 2: Run test to verify it fails**

Run:

```bash
cargo test -p ckbadger-tui test_tui_service_config_fields --lib
cargo test -p ckbadger-tui tui_db_exposes_paths_and_profile_without_store --lib
```

Expected: FAIL because the new runtime fields/accessors do not exist yet.

**Step 3: Write minimal implementation**

- Extend `TuiServiceConfig`.
- Thread the new values through `run_tui()` into `TuiDb::new_with_monitoring()`.
- Store the new paths in `TuiDb` and add read-only accessors.

**Step 4: Run tests to verify it passes**

Run the same two commands and confirm PASS.

### Task 2: Add failing tests for system-tab path text

**Files:**

- Modify: `crates/tui/src/ui.rs`

**Step 1: Write the failing tests**

- Add a test for a new `system_workdir_lines()` helper that expects both:
  - `ckbadger workdir`
  - `CKB workdir`
- Add a test for a new `system_store_path_lines()` helper that expects:
  - `Domain store`
  - `Append-only store`
  - `CKB RocksDB`

**Step 2: Run test to verify it fails**

Run:

```bash
cargo test -p ckbadger-tui test_system_workdir_lines_include_both_roots --lib
cargo test -p ckbadger-tui test_system_store_path_lines_include_ckb_rocksdb --lib
```

Expected: FAIL because the helper functions do not exist yet.

**Step 3: Write minimal implementation**

- Extract pure helper functions that build the path lines.
- Update `draw_system_content()` to render the new `Workdir` section and updated `Store Paths` section.
- Adjust section heights to match the extra block and extra line.

**Step 4: Run tests to verify it passes**

Run the same two commands and confirm PASS.

### Task 3: Wire resolved paths from CLI into the TUI

**Files:**

- Modify: `crates/cli/src/main.rs`

**Step 1: Write the failing test or compile target**

- Use the focused TUI tests as the red signal for this runtime wiring task.

**Step 2: Run a focused verification command**

Run:

```bash
cargo test -p ckbadger-tui --lib
```

Expected: FAIL until CLI/TUI wiring compiles with the new config surface.

**Step 3: Write minimal implementation**

- In `cmd_tui()`, resolve `ckb_paths` with `resolve_ckb_paths(workdir, &config.ckb)?`.
- Fill `TuiServiceConfig` with:
  - `ckbadger_workdir`
  - `ckb_workdir`
  - `ckb_db_path`

**Step 4: Run tests to verify it passes**

Run:

```bash
cargo test -p ckbadger-tui --lib
```

Expected: PASS
