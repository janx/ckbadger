# TUI System Workdirs Design

## Goal

- Add a `Workdir` section to the TUI `System` tab.
- Show both the `ckbadger` workdir root and the resolved `ckb.workdir`.
- Extend the existing `Store Paths` section to also show the resolved CKB RocksDB path.

## Principle Alignment

- CKB Native: the displayed CKB RocksDB path comes from the same `resolve_ckb_paths()` logic used elsewhere, so the TUI reflects the real CKB node layout instead of inventing its own path rule.
- Local First: the system tab makes the local filesystem layout explicit, which helps users inspect their local node and explorer deployment directly.
- Agent Friendly: path resolution stays in CLI/config layers, and the TUI only renders already-resolved runtime values.

## Problem Summary

- The current `System` tab shows system environment details and ckbadger store paths, but it does not show the top-level work directories that operators actually reason about first.
- It also omits the final CKB RocksDB path, which makes path-debugging incomplete when checking direct-read configuration.

## Constraints

- Keep one calculation path for resolved filesystem values.
- Do not duplicate CKB path resolution logic inside `ckbadger-tui`.
- Keep the change read-only; no RocksDB write-path changes are involved.
- Add test coverage for the new runtime fields and rendered system-tab text.

## Approaches

### Approach 1: Resolve paths in CLI, render in TUI

- CLI computes `ckbadger` workdir, resolved `ckb.workdir`, and resolved CKB RocksDB path.
- `TuiServiceConfig` carries those values into `TuiDb`.
- UI renders those values in dedicated sections.

Trade-offs:

- Preserves a single calculation path.
- Keeps TUI logic simple and read-only.
- Slightly expands the TUI runtime config surface.

### Approach 2: Recompute paths inside TUI

- Pass only the root workdir or raw config values into TUI and resolve there.

Trade-offs:

- Avoids a few config fields in `TuiServiceConfig`.
- Duplicates path resolution logic in the wrong layer.
- Risks drift from `ckbadger-config`.

## Recommendation

- Use Approach 1.
- The CLI already owns startup-time config resolution, so it should remain the only place that computes these paths.

## Proposed Design

### 1. Runtime wiring

- In `crates/cli/src/main.rs`, `cmd_tui()` resolves:
  - `ckbadger` workdir from `WorkDir::resolve(workdir)`
  - `ckb.workdir` and final CKB RocksDB path from `resolve_ckb_paths(workdir, &config.ckb)?`
- Pass the resolved strings into `TuiServiceConfig`.

### 2. TUI data model

- Extend `TuiServiceConfig` with:
  - `ckbadger_workdir`
  - `ckb_workdir`
  - `ckb_db_path`
- Extend `TuiDb` to store those paths and expose read-only accessors.

### 3. System tab layout

- Add a new `Workdir` section between `System Environment` and `Store Paths`.
- Keep `Store Paths` and add a `CKB RocksDB` line.
- Refactor section content into pure line-builder helpers so tests can assert text output directly.

### 4. Testing

- `crates/tui/src/entry.rs`: verify `TuiServiceConfig` carries the new fields.
- `crates/tui/src/db.rs`: verify `TuiDb` exposes the new resolved paths.
- `crates/tui/src/ui.rs`: verify `Workdir` lines and `Store Paths` lines include the new text.

## Scope

- `crates/cli/src/main.rs`
- `crates/tui/src/entry.rs`
- `crates/tui/src/db.rs`
- `crates/tui/src/ui.rs`

## Validation Plan

- Run focused TUI tests for config, DB accessors, and UI text helpers.
- Run a focused `ckbadger-tui` crate test pass after implementation.

## Store Boundary Check

- Store target: none, read-only UI/runtime wiring only.
- Domain vs append-only target confirmed: yes, no new write path.
- Append-only update/delete path check: pass, untouched.
