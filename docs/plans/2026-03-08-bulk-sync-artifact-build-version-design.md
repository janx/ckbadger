# Bulk Sync Artifact Build Version Design

## Goal

- Add the existing compile-time `buildVersion` string to bulk-sync perf artifacts.
- Keep one exact version source and avoid introducing any second version calculation path.

## Principle Alignment

- CKB Native: no chain data semantics or derived-chain write paths change; this only annotates local perf evidence generated from the true indexer runtime path.
- Local First: local perf artifacts under `workdir/perf/bulk-sync/` identify the exact local build that produced them.
- Agent Friendly: artifact files remain deterministic, grep-friendly outputs with machine-readable version metadata.

## Problem Summary

- Bulk-sync perf artifacts currently record run identity and metrics, but not the build identity that produced them.
- `run_id` is only a runtime timestamp + pid marker, so it cannot distinguish one binary build from another.
- The project already has a single compile-time `buildVersion` path used by CLI and frontend-facing outputs; bulk-sync artifacts should reuse that same value rather than invent a new Git lookup path.

## Constraints

- `buildVersion` must remain a single compile-time value computed in CLI build metadata.
- `ckbadger-indexer` must not execute Git commands or derive its own version string.
- Bulk-sync artifacts must remain file outputs under `workdir/perf/bulk-sync/`; no RocksDB changes.
- Fail fast on missing or blank version metadata; do not silently write empty placeholders.
- Do not change `run_id` format or overload it with build metadata.

## Existing Relevant Architecture

- `crates/cli/build.rs` computes `CKBADGER_BUILD_VERSION` exactly once at compile time.
- `crates/cli/src/main.rs` already consumes that string as `BUILD_VERSION`.
- `crates/api/src/entry.rs` already demonstrates the intended pattern: the CLI passes `BUILD_VERSION` into a service config and the service treats it as an opaque string.
- `crates/indexer/src/entry.rs` defines `IndexerServiceConfig`, which is the CLI-to-indexer boundary.
- `crates/indexer/src/config.rs` defines the internal validated indexer config.
- `crates/indexer/src/sync/indexer.rs` gates bulk-sync perf run startup.
- `crates/indexer/src/bulk_sync_perf.rs` owns `metadata.env`, `metrics.env`, `report.md`, and `latest/` updates.

## Approaches Considered

### Approach 1: Reuse CLI buildVersion and pass it through indexer config

- Extend the existing CLI-to-indexer config path with `build_version: String`.
- Keep the value opaque and let the perf writer persist it into artifacts.

Trade-offs:

- Preserves a single version calculation path.
- Matches the existing frontend service pattern.
- Requires a small amount of config plumbing and tests.

### Approach 2: Compute version again inside the indexer

- Add Git/branch/hash logic to `ckbadger-indexer` and let it derive its own build version.

Trade-offs:

- Avoids one config field.
- Directly violates the single-calculation-path rule and duplicates build metadata logic.
- Risks drift between CLI/frontend version output and artifact version output.

### Approach 3: Add version only to CI upload naming

- Leave local artifact contents unchanged and annotate only CI upload package names or wrapper scripts.

Trade-offs:

- Smallest implementation.
- Fails the local-first goal because local `workdir/perf/bulk-sync/*` still lacks build identity.
- Keeps the useful metadata outside the artifact body that humans and agents actually inspect.

## Recommendation

- Use Approach 1.
- The CLI already owns the only valid `buildVersion` string. The indexer should consume that exact string via config and persist it into bulk-sync artifact files without recomputation.

## Proposed Design

### 1. Version plumbing

- Extend `IndexerServiceConfig` with `build_version: String`.
- Extend internal indexer `Config` with `build_version: String`.
- In `crates/cli/src/main.rs`, pass `BUILD_VERSION.to_string()` into the indexer service config when launching the internal indexer service.
- Add config validation that rejects blank `build_version`.

### 2. Bulk-sync perf writer contract

- Extend `BulkSyncPerfRun` to hold `build_version: String`.
- Change `BulkSyncPerfRun::start(...)` to require `build_version`.
- Treat `build_version` as opaque metadata:
  - no parsing
  - no branch/hash extraction
  - no fallback default

### 3. Artifact format changes

- `metadata.env` gains:
  - `build_version=<opaque-build-version>`
- `report.md` header gains:
  - `Build Version: <opaque-build-version>`
- `latest/metadata.env` and `latest/report.md` continue to be copied from the most recent completed run, so version metadata stays aligned with the latest completed baseline.

`metrics.env` remains metric-focused and does not need a duplicate version field because `metadata.env` is already the run metadata file.

### 4. Run identity separation

- Keep `run_id` unchanged as a runtime-specific identifier.
- Keep build identity and run identity separate:
  - `run_id` answers "which execution?"
  - `build_version` answers "which build?"

This avoids conflating lifecycle state with artifact provenance.

## Failure Handling

- `Config::validate()` must reject blank `build_version`.
- `BulkSyncPerfRun::start(...)` must also reject blank `build_version` so the writer boundary enforces the invariant independently.
- No `"unknown"`, `"dev"`, or empty-string fallback is allowed in indexer/runtime artifact generation.
- If the CLI-to-indexer handoff breaks, startup or run initialization should fail immediately with actionable context.

## Affected Files

- `crates/cli/src/main.rs`
- `crates/indexer/src/entry.rs`
- `crates/indexer/src/config.rs`
- `crates/indexer/src/sync/indexer.rs`
- `crates/indexer/src/bulk_sync_perf.rs`

## Testing Strategy

- `crates/indexer/src/config.rs`
  - add a validation test proving blank `build_version` is rejected
- `crates/indexer/src/bulk_sync_perf.rs`
  - add a test proving `metadata.env` contains `build_version`
  - add a test proving `report.md` contains the build version
  - extend latest-update coverage so completed runs preserve build version in copied metadata/report artifacts
- `crates/cli/src/main.rs`
  - extract a small helper for building `IndexerServiceConfig`
  - test that the helper passes the exact `BUILD_VERSION` into the indexer service config without rewriting it

## Validation Notes

- No RocksDB write path changes.
- No domain store changes.
- No append-only store changes.
- No re-sync required.
