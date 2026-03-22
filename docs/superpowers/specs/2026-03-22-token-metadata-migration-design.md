# Token Metadata Migration: Remove token-labels Submodule

**Date**: 2026-03-22
**Status**: Draft

## Goal

Replace the external `docs/token-labels` git submodule (appfi5/ckb-labels) with a self-owned `docs/metadata/` directory using a custom TOML format. All token and script metadata becomes first-party data that can be added, updated, or deleted by editing files in the repository.

### Principle Alignment

- **CKB Native**: Metadata describes CKB scripts and UDT tokens; owning the format lets us model CKB concepts directly.
- **Local First**: Bundled at compile time, overridable from workdir at runtime; no network fetch required.
- **Agent Friendly**: TOML files with consistent schema are easy for both humans and automation to read/write.

## Current State

### Data Sources

| Source | Format | Content |
|--------|--------|---------|
| `docs/token-labels/` (submodule) | JSON, `{hash}/index.json` | ~675 UDT labels, 62 script labels |
| `docs/script-name-overrides.json` | JSON | 6 name rewrites, 5 known_scripts, 5 deprecated hashes, 4 NFT tier overrides, protocol groups (unused) |
| `labels.toml` (workdir, optional) | TOML | `LabelsConfig`: script_name_overrides, nft_storage_tier_overrides, deprecated |

### Consumers

| Consumer | File | What It Reads |
|----------|------|---------------|
| Build-time bundler | `crates/indexer/build.rs` | token-labels dir + script-name-overrides.json → bundles JSON blobs |
| Runtime importer | `crates/indexer/src/label_import.rs` | token-labels dir or bundled JSON blobs + overrides |
| Activity UDT classifier | `crates/indexer/src/db/writer/activities.rs` | `bundled_udt_script_code_hashes.json` (compile-time include from build.rs) |
| NFT tier resolver | `crates/api/src/utils/assets.rs` | script-name-overrides.json (compile-time static via `CARGO_MANIFEST_DIR`) |
| Indexer internal config | `crates/indexer/src/config.rs` | `token_labels_path` field in indexer `Config` |
| Indexer service configs | `crates/indexer/src/entry.rs` | `IndexerServiceConfig.token_labels_path`, `LabelImportServiceConfig` (carries `token_labels_path`, `import_udt`, `import_scripts`) |
| Labels config loader | `crates/config/src/lib.rs` | `labels.toml` from workdir; `resolve_token_labels_path()` for workdir/share dir resolution |
| CLI label-import cmd | `crates/cli/src/main.rs` | `resolve_token_labels_path()`, `resolve_share_dir()` for `{exe}/../share/token-labels/` |

### Storage Targets (Unchanged)

Label import writes to these domain-store column families. These CFs and their schemas are not changing:

- `CF_TOKENS` — `TokenInfo` keyed by type_script_hash
- `CF_SCRIPT_INFO` — `ScriptInfo` keyed by code_hash
- `CF_SCRIPT_VERSIONS` — `ScriptVersionInfo` keyed by version_hash (data_hash or code_hash)
- `CF_SCRIPT_VERSIONS_BY_LABEL` — index for named script family lookups

Import preserves indexer-maintained fields (holders_count, total_supply, transfers_count, cells_count, etc.) and only writes metadata fields (name, symbol, decimals, description, etc.).

## New Format

### Directory Layout

```
docs/metadata/
  tokens/
    seal.toml
    ckb-fi.toml
    r-ordi.toml
    ...                    # ~675 files
  scripts/
    default-lock.toml
    dot-bit-lock.toml
    always-success.toml
    ...                    # ~67 files
  nft-tiers.toml           # NFT storage tier overrides
```

### Token File Format

```toml
# docs/metadata/tokens/seal.toml
name = "Seal"
symbol = "SEAL"
decimals = 8
standard = "xudt"
# icon = "https://..."       # optional
# description = "..."        # optional

[mainnet]
code_hash = "0x50bd8d6680b8b9cf98b73f3c08faf8b2a21914311954118ad6609be6e78a1b95"
hash_type = "data1"
args = "0xf283825c247337e4c6048a24daa688570e0055ed18fa026ab169fe42e4b59e4c"

# [testnet]
# code_hash = "0x..."
# hash_type = "data1"
# args = "0x..."
```

**Fields**:

| Field | Required | Description |
|-------|----------|-------------|
| `name` | yes | Display name |
| `symbol` | yes | Ticker symbol |
| `decimals` | yes | Decimal places |
| `standard` | yes | UDT standard: `sudt`, `xudt`, etc. |
| `icon` | no | Icon URL |
| `description` | no | Human-readable description |
| `mainnet` | no* | Single table: the token's type script on mainnet |
| `testnet` | no* | Single table: the token's type script on testnet |

*At least one network section is required.

Each network section contains `code_hash`, `hash_type`, and `args` — the token's type script on that network. A single `[mainnet]` table (not `[[mainnet]]` array) because each token has exactly one type script identity per network. The `type_hash` (used as the store key) is derived at import time by computing `ckb_hash(serialize(code_hash, hash_type, args))`.

**Why no `type_hash` field**: The type_hash is a deterministic function of the type script. Storing it would create a redundancy that can go stale. Deriving it at import time ensures correctness.

**Why no `published` field**: All files in `docs/metadata/tokens/` are considered published. Removing a token = deleting or renaming its file.

### Script File Format

```toml
# docs/metadata/scripts/default-lock.toml
name = "Default Lock"
description = "The default SECP256K1/blake160 lock script."
website = "https://github.com/nervosnetwork/ckb-system-scripts"
# category = "lock"          # optional (was decoderType)

[[mainnet]]
code_hash = "0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8"
data_hash = "0x709f3fda12f561cfacf92273c57a98fede188a3f1a59b1f888d113f9cce08649"
hash_type = "type"
# deprecated = false         # optional, default false

[[testnet]]
code_hash = "0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8"
data_hash = "0x709f3fda12f561cfacf92273c57a98fede188a3f1a59b1f888d113f9cce08649"
hash_type = "type"
```

**Fields**:

| Field | Required | Description |
|-------|----------|-------------|
| `name` | yes | Display name (overrides already applied, e.g., "Default Lock" not "SECP256K1/blake160") |
| `description` | yes | Human-readable description |
| `website` | no | Project/source URL |
| `category` | no | Script category (was `decoderType`): `"lock"`, `"udt"`, etc. |
| `mainnet` | no* | Array of mainnet deployments |
| `testnet` | no* | Array of testnet deployments |

*At least one network deployment is required.

Each deployment entry:

| Field | Required | Description |
|-------|----------|-------------|
| `code_hash` | yes | Script code hash |
| `data_hash` | no | Script data hash (version identifier). Omit for pseudo-scripts (Type ID, Zero Lock) that have no deployed code cell. When omitted, only code_hash-level metadata is written; no `ScriptVersionInfo` entry is created. |
| `hash_type` | yes | `"type"`, `"data"`, `"data1"`, `"data2"` |
| `deprecated` | no | Default false. Marks this deployment as deprecated. |

### NFT Tiers File

```toml
# docs/metadata/nft-tiers.toml
[overrides]
".bit" = "fully_on_ckb"
"dotbit" = "fully_on_ckb"
"did:ckb" = "fully_on_ckb"
"did_ckb" = "fully_on_ckb"
```

Valid tier values: `fully_on_ckb_and_btc`, `fully_on_ckb`, `decentralized_dependent`, `centralized_dependent`, `unknown`.

## Workdir Override Mechanism

Bundled data from `docs/metadata/` is compiled into the binary via `build.rs`. At runtime, the workdir can override or extend the bundled set using the same directory structure and file format:

```
{workdir}/
  metadata/
    tokens/
      my-local-token.toml     # addition: new token not in bundled set
      seal.toml                # override: replaces bundled seal.toml entirely
    scripts/
      my-local-script.toml    # addition
    nft-tiers.toml             # override: replaces bundled nft-tiers.toml
```

**Merge rules**:

1. Load all bundled entries (keyed by filename/slug).
2. Scan `{workdir}/metadata/` if it exists.
3. For each workdir file, if a bundled entry with the same slug exists, replace it entirely. Otherwise, add it.
4. A workdir file with `disabled = true` at the top level suppresses the corresponding bundled entry without adding a replacement.
5. NFT tiers: workdir `nft-tiers.toml` replaces the bundled file entirely (no per-key merge).

This replaces both `labels.toml` and `script-name-overrides.json`.

## Slug Convention

Token slugs are derived from the symbol: lowercase, non-alphanumeric characters replaced with hyphens, consecutive hyphens collapsed, leading/trailing hyphens stripped.

Examples: `"SEAL"` -> `seal`, `"CKB-FI"` -> `ckb-fi`, `"R-ordi"` -> `r-ordi`.

If two tokens share a slug (same symbol, different type scripts), append a truncated type_hash suffix: `seal.toml` vs `seal-00286029.toml`.

Script slugs are derived from the name using the same rules: `"Default Lock"` -> `default-lock`, `".bit Lock"` -> `dot-bit-lock`, `"AlwaysSuccess"` -> `alwayssuccess` (or manually chosen: `always-success`).

The slug is purely a filename convention for human convenience. The authoritative identity is the data inside the file (type_script for tokens, code_hash for scripts).

## What Gets Dropped

| Removed | Reason |
|---------|--------|
| `docs/token-labels/` submodule | Replaced by `docs/metadata/` |
| `.gitmodules` entry for token-labels | Submodule removed |
| `docs/script-name-overrides.json` | Name overrides baked into script files; NFT tiers in `nft-tiers.toml`; deprecated flags inline; known_scripts become individual files; protocols map unused |
| `LabelsConfig` / `labels.toml` support | Replaced by workdir `metadata/` override |
| `protocols` map | Unused (`_protocols` prefix in current code). Protocol detectors are hardcoded in indexer. |
| Unused UDT fields | `tags`, `manager`, `email`, `famous`, `operatorWebsite`, `$schema` |
| Unused script fields | `rfc`, `sourceUrl`, `tag`, `typeHash`, `cellDeps`, `$schema` |
| `published` flag on UDT labels | Presence in `docs/metadata/tokens/` implies published |
| `import_udt` / `import_scripts` config toggles | Always import both |
| `run_label_import_staged()` | No longer needed without per-kind toggles |

### Field Renames

| Old Field (JSON) | New Field (TOML) | Context |
|------------------|------------------|---------|
| `udtType` | `standard` | Token files |
| `decoderType` | `category` | Script files |

## Code Changes

### `crates/indexer/build.rs`

- Scan `docs/metadata/tokens/*.toml` and `docs/metadata/scripts/*.toml`.
- Parse TOML into intermediate structs.
- Serialize to bundled JSON blobs (`bundled_udt_labels.json`, `bundled_script_labels.json`).
- Bundle `docs/metadata/nft-tiers.toml` content.
- Extract UDT-compatible script code_hashes from scripts with `category = "udt"` (for `bundled_udt_script_code_hashes.json`).
- Remove: all references to `docs/token-labels/`, `script-name-overrides.json`.

### `crates/indexer/src/label_import.rs`

- New TOML-based deserialization structs (`TokenMetadata`, `ScriptMetadata`, `TokenDeployment`, `ScriptDeployment`).
- Remove: `UdtLabelInfo`, `UdtTypeScript`, `ScriptLabelInfo`, `ScriptDeployments`, `ScriptDeployment` (old JSON structs), `ScriptNameOverrides`, `apply_script_overrides`, `apply_deprecated_flags`, `scripts_overlap`, `load_script_overrides`.
- Remove: `run_label_import_staged()` (existed to run UDT/script passes separately; no longer needed since `import_udt`/`import_scripts` toggles are removed).
- `run_label_import()`: load from `docs/metadata/` directory (TOML files).
- `run_label_import_bundled()`: load from compiled-in bundled data, then overlay workdir metadata if present.
- `upsert_token_label()`: derive type_hash from type_script fields at import time using `ckb_hash`.
- `upsert_script_label()`: simplified, no override indirection.
- Update all tests to use new TOML format fixtures.

### `crates/config/src/lib.rs`

- Remove `LabelsConfig`, `parse_labels_config`, `load_labels_config`, and all associated tests (`test_parse_labels_*`, `test_load_labels_config_*`).
- Remove `resolve_token_labels_path()` and share directory resolution for `share/token-labels/`.
- `WorkDir`: replace `token_labels: Option<PathBuf>` + `labels_toml: Option<PathBuf>` with `metadata: Option<PathBuf>`.
- `WorkDir::resolve()`: check for `{root}/metadata/` directory.
- Update `resolve_share_dir()` callers: if the share dir concept is retained for metadata, look for `{exe}/../share/metadata/` instead of `share/token-labels/`. Otherwise, remove share dir support entirely (workdir override is the only override path).

### `crates/api/src/utils/assets.rs`

- Read NFT tiers from `docs/metadata/nft-tiers.toml` instead of `docs/script-name-overrides.json`.
- Continue using `CARGO_MANIFEST_DIR`-relative path at compile time (the API crate reads `../../docs/metadata/nft-tiers.toml`). This works because `assets.rs` lives in the `api` crate which cannot access the indexer's `build.rs` output.
- NFT tier keys in the TOML file are stored raw (as-is from the current JSON). The existing `normalize_standard_alias_key()` normalization logic in `assets.rs` is preserved unchanged.

### `crates/indexer/src/config.rs`

- Rename `token_labels_path` to `metadata_path: Option<String>` in the indexer `Config` struct.
- Remove `import_udt`, `import_scripts` if present.

### `crates/indexer/src/entry.rs`

- `IndexerServiceConfig`: rename `token_labels_path` to `metadata_path`.
- `LabelImportServiceConfig`: rename `token_labels_path` to `metadata_path`; remove `import_udt`, `import_scripts` fields.
- Update `run_startup_label_import()`: check for `{workdir}/metadata/` instead of `docs/token-labels/information/`.

### `crates/indexer/src/db/writer/activities.rs`

- No structural change. The `bundled_udt_script_code_hashes.json` compile-time include continues to work — `build.rs` still produces this artifact, just from `docs/metadata/scripts/*.toml` (category = "udt") instead of the old script JSON files.

### `crates/common/src/label_import.rs`

- Simplify `LabelImportConfig`: `metadata_path: Option<String>` (workdir override dir) + `network: String`.
- Remove `token_labels_path`, `import_udt`, `import_scripts`.

### `crates/cli/src/main.rs`

- Update `cmd_label_import()` to use new `metadata_path` config.
- Update indexer startup path resolution.

### `.gitmodules`

- Remove the `[submodule "docs/token-labels"]` entry. The file has 3 other submodule entries (`docs.nervos.org`, `rfcs`, `dob-cookbook`) so the file itself is kept.

### `README.md`

- Update label import documentation: replace references to `token-labels/`, `labels.toml`, `share/token-labels/` with the new `metadata/` directory structure.
- Remove `labels.toml` from the workdir file listing.

## Migration Strategy

### Step 1: Migration Script

Write a one-time script (Python preferred for quick JSON/TOML manipulation) that:

1. Reads all `docs/token-labels/information/udt/{mainnet,testnet}/*/index.json`.
2. Groups by symbol+name (to merge mainnet and testnet entries for the same token into one file).
3. Filters to `published: true` only.
4. Generates `docs/metadata/tokens/{slug}.toml` for each token.
5. Reads all `docs/token-labels/information/script/*/index.json`.
6. Reads `docs/script-name-overrides.json`: applies name overrides, merges known_scripts, marks deprecated.
7. Generates `docs/metadata/scripts/{slug}.toml` for each script (with overrides baked in).
8. Generates `docs/metadata/nft-tiers.toml` from the overrides file.
9. Reports any slug collisions for manual resolution.

### Step 2: Code Update

Update `build.rs`, `label_import.rs`, `config/lib.rs`, `api/utils/assets.rs`, `common/label_import.rs`, `cli/main.rs` as described above.

### Step 3: Cleanup

- `git rm docs/token-labels` (remove submodule)
- Edit `.gitmodules`: remove `[submodule "docs/token-labels"]` entry (keep 3 other entries)
- Delete `docs/script-name-overrides.json`
- Delete `labels.toml` support code and tests
- Clean up `.git/modules/docs/token-labels` if needed
- Update `README.md`: replace `token-labels/`, `labels.toml`, `share/token-labels/` references with `metadata/`

### Step 4: Verification

- `cargo test` passes.
- `cargo clippy` clean.
- Compare store state: run `run_label_import_bundled()` with old code and new code against the same empty store, verify identical `TokenInfo` / `ScriptInfo` / `ScriptVersionInfo` entries (modulo expected changes from baking in overrides).
- Bundled label count assertions still pass (`>100` UDT labels, `>10` script labels).

## Testing

- Unit tests for new TOML deserialization structs.
- Unit test for type_hash derivation from type_script fields.
- Unit test for slug generation (collision handling, special characters).
- Unit test for workdir override merge logic (addition, replacement, `disabled = true`).
- Integration test: `run_label_import_bundled()` produces the same store entries as the old code.
- Integration test: workdir override adds/replaces entries correctly.
- Existing test `test_bundled_label_import_has_no_errors` updated for new format.
- Existing test assertions (SECP256K1, .bit Time scripts, Godwoken Custodian, etc.) updated.

## Risks and Mitigations

| Risk | Mitigation |
|------|------------|
| Slug collisions during migration | Migration script detects and reports; manual resolution before commit |
| type_hash derivation differs from upstream | Use same `ckb-hash` + `ckb-types` serialization as existing code; verify against known type_hashes |
| Missing tokens/scripts after migration | Count-based assertions in tests; diff bundled JSON before/after |
| Build regression (bundled data empty) | build.rs panics if `docs/metadata/` is missing or empty |
