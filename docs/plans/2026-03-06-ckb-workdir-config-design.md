# CKB Workdir Config Design

## Goal

- Replace `[ckb].data_path` in `ckbadger.toml` with `[ckb].workdir`.
- Make `ckbadger` derive the final CKB RocksDB path from the CKB node config directory using CKB's own path rules.
- Keep a single fail-fast path with no RocksDB-to-RPC fallback.

## Principle Alignment

- CKB Native: ckbadger follows the CKB node's own config model (`ckb.toml`, `data_dir`, optional `[db].path`) instead of inventing a parallel path contract.
- Local First: the user points ckbadger at the local CKB node config directory, and ckbadger resolves the local RocksDB path directly.
- Agent Friendly: one shared resolver computes the final path once; API, indexer, and label import consume the resolved result without duplicating path logic.

## Problem Summary

- Current ckbadger config exposes `[ckb].data_path`, which forces users to configure the final RocksDB directory directly.
- That duplicates knowledge already present in the CKB node config and creates drift when the CKB node changes `data_dir` or `[db].path`.
- Service configs still carry optional raw path fields deep into API and indexer startup, which weakens the single-calculation-path contract.
- The previous RPC fallback problem was fixed at the read-path layer, but the config contract is still wrong: users should configure the CKB node workdir, not the derived RocksDB path.

## Constraints

- `docs/prompts/WORLD_VIEW.md` and `docs/prompts/BULK_SYNC.md` remain authoritative.
- Missing or invalid CKB path configuration must fail fast with actionable errors.
- No path-resolution fallback chain is allowed.
- No RocksDB read path may fall back to JSON-RPC when direct-read path resolution fails.
- This is an explicit breaking config change; backward compatibility is not required.

## External CKB Semantics

From the local CKB source tree:

- `ckb -C <path> run` treats `<path>` as the CKB config directory.
- `ckb.toml` uses top-level `data_dir`.
- CKB also supports optional `[db].path`.
- CKB computes the final RocksDB path as:
  - canonicalize `data_dir` relative to the config directory if needed
  - use `[db].path` when set, relative to the config directory if needed
  - otherwise default to `data_dir/db`

ckbadger should mirror this logic exactly.

## Approaches Considered

### Approach 1: Fixed `<workdir>/data/db`

- Replace `data_path` with `workdir` and always derive `<ckb workdir>/data/db`.

Trade-offs:

- Very simple.
- Wrong when the CKB node sets absolute `data_dir` or custom `[db].path`.
- Diverges from CKB's actual configuration semantics.

### Approach 2: Parse `ckb.toml` and mirror CKB path rules

- Replace `data_path` with `workdir`.
- Read `<ckb.workdir>/ckb.toml`.
- Resolve `data_dir` and optional `[db].path` using CKB's own rules.

Trade-offs:

- Matches the source of truth.
- Keeps one exact calculation path.
- Slightly more code because ckbadger needs a minimal CKB config parser.

### Approach 3: Keep both `workdir` and `data_path`

- Support both the old direct DB path and the new workdir-based contract.

Trade-offs:

- Easier migration.
- Reintroduces dual contracts and ambiguity.
- Violates the explicit requirement to fully replace `data_path`.

## Recommendation

- Use Approach 2.
- The user config should point to the CKB node config directory, and ckbadger should derive the final RocksDB path once from the CKB node's own config.
- The resolved RocksDB path should be passed downward as a required runtime value, not recomputed or optionally omitted in service layers.

## Proposed Design

### 1. Config contract

- `ckbadger.toml` `[ckb]` removes `data_path`.
- `ckbadger.toml` `[ckb]` adds `workdir`.
- `workdir` means the CKB node config directory used by `ckb -C <path> run`.
- `ckbadger init` generates:

```toml
[ckb]
rpc_url = "http://127.0.0.1:8114"
network = "mainnet"
workdir = ""                      # REQUIRED: CKB node config directory
```

- Old `[ckb].data_path` is rejected explicitly during config loading with a migration error.

### 2. Shared path resolver

- Add a shared resolver in `ckbadger-config`, for example:

```rust
pub struct ResolvedCkbPaths {
    pub ckb_workdir: PathBuf,
    pub ckb_config_path: PathBuf,
    pub ckb_data_dir: PathBuf,
    pub ckb_db_path: PathBuf,
}
```

- Resolver inputs:
  - ckbadger workdir
  - parsed `[ckb]` config
- Resolver steps:
  - resolve `ckb.workdir` relative to the ckbadger workdir when needed
  - load `<ckb.workdir>/ckb.toml`
  - parse only the minimal needed fields:
    - top-level `data_dir`
    - optional `[db].path`
  - resolve relative `data_dir` and `[db].path` against `ckb.workdir`
  - compute final `ckb_db_path`
  - verify the final path exists

- This becomes the only calculation path for CKB direct-read path resolution.

### 3. Runtime wiring

- CLI resolves `ResolvedCkbPaths` before starting services.
- CLI passes only the final `ckb_db_path` into:
  - API service config
  - indexer service config
  - label import service config

- Service-layer configs stop accepting optional raw path configuration:
  - `ckb_data_path: Option<String>` becomes `ckb_db_path: String`

- Service-layer startup no longer contains ad-hoc `require_ckb_data_path` helpers.
- API and indexer just open RocksDB using the already-resolved path.

### 4. API and indexer contract cleanup

- `crates/api/src/lib.rs` `AppConfig` should use a required `ckb_db_path: String`.
- `AppState.ckb_store` can be made required as well, because runtime startup will already fail before router creation if the direct-read database is unavailable.
- `crates/indexer/src/config.rs` validates required `ckb_db_path` instead of optional `ckb_data_path`.
- All error messages should refer to:
  - `[ckb].workdir`
  - `ckb.toml`
  - resolved CKB RocksDB path

### 5. Failure handling

- Fail fast on:
  - missing or blank `[ckb].workdir`
  - old `[ckb].data_path` present
  - missing `<ckb.workdir>/ckb.toml`
  - invalid `ckb.toml`
  - blank `data_dir`
  - resolved RocksDB path missing

- No fallback behavior:
  - no direct-read fallback to RPC
  - no guessed default outside CKB's own path rules
  - no directory scanning
  - no automatic migration from `data_path`

### 6. User-visible behavior

- On success, startup logs should show:
  - loaded CKB config path
  - resolved final RocksDB path

- On failure, startup errors should be specific, for example:
  - `ckb.workdir is required`
  - `[ckb].data_path has been removed; use [ckb].workdir`
  - `ckb.toml not found under <workdir>`
  - `failed to parse CKB config at <path>`
  - `CKB config data_dir is blank`
  - `resolved CKB RocksDB path does not exist: <path>`

## Affected Files

- `crates/config/src/lib.rs`
  - config struct change
  - default config generation
  - shared CKB path resolver
  - config parsing regression tests
- `crates/cli/src/main.rs`
  - resolve CKB paths once before service startup
  - pass final `ckb_db_path` into runtime configs
- `crates/indexer/src/config.rs`
  - required runtime `ckb_db_path` validation
- `crates/indexer/src/entry.rs`
  - required service config field
  - remove local path-required helpers
- `crates/api/src/entry.rs`
  - required service config field
  - remove local path-required helpers
- `crates/api/src/lib.rs`
  - required router/app config field
- `crates/ckb-store-reader/src/lib.rs`
  - docs and error wording only
- `docs/INDEXER_PIPELINE.md`
  - replace old `ckb_data_path` / `data_path` references

## Testing Strategy

### Config resolver tests

- Default config round-trip includes `ckb.workdir`.
- Reject old `[ckb].data_path`.
- Resolve relative `ckb.workdir`.
- Resolve relative `data_dir`.
- Resolve absolute `data_dir`.
- Resolve relative `[db].path`.
- Resolve absolute `[db].path`.
- Reject missing `ckb.toml`.
- Reject invalid `ckb.toml`.
- Reject blank `data_dir`.
- Reject missing resolved RocksDB path.

### Runtime config tests

- API service config requires `ckb_db_path`.
- Indexer service config requires `ckb_db_path`.
- Indexer `Config::validate()` rejects blank `ckb_db_path`.
- App/router config tests no longer depend on `Option<String>` path semantics.

## Non-Goals

- Supporting legacy `[ckb].data_path`.
- Scanning the filesystem to infer a CKB node directory.
- Reintroducing any JSON-RPC fallback for direct RocksDB paths.
- Changing bulk-sync logic beyond the configuration and startup path resolution contract.
