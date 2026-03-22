# Token Metadata Migration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the `docs/token-labels` git submodule with self-owned `docs/metadata/` TOML files and update all code that reads/writes label data.

**Architecture:** A one-time Python migration script converts existing JSON label data to TOML files. Then all Rust code is updated atomically in a single commit: common types, build.rs bundling, label_import runtime, indexer config/entry, config crate, CLI, and API. The store schema and column families are unchanged.

**Tech Stack:** Rust (serde, toml), Python 3 (migration script), TOML file format

**Spec:** `docs/superpowers/specs/2026-03-22-token-metadata-migration-design.md`

---

## Task 1: Write and Run Migration Script

Generate `docs/metadata/` TOML files from current JSON sources. This runs before any Rust code changes.

**Files:**
- Create: `scripts/migrate-labels.py`
- Create: `docs/metadata/tokens/*.toml` (~675 files)
- Create: `docs/metadata/scripts/*.toml` (~67 files)
- Create: `docs/metadata/nft-tiers.toml`
- Read: `docs/token-labels/information/udt/{mainnet,testnet}/*/index.json`
- Read: `docs/token-labels/information/script/*/index.json`
- Read: `docs/script-name-overrides.json`

- [ ] **Step 1: Write the migration script**

Create `scripts/migrate-labels.py`. The script must:

1. Read all UDT JSON files from `docs/token-labels/information/udt/{mainnet,testnet}/*/index.json`.
2. Filter to `published: true` only.
3. Group by `(name, symbol)` to merge mainnet+testnet entries for the same token into one file.
4. Generate slug from symbol: lowercase, non-alphanumeric→hyphen, collapse consecutive hyphens, strip leading/trailing hyphens. On collision, append truncated type_hash (first 8 hex chars).
5. Write `docs/metadata/tokens/{slug}.toml` per token. Use `[mainnet]` / `[testnet]` single-table format. Fields: `name`, `symbol`, `decimals`, `standard` (was `udtType`). Optional: `icon`, `description`. Each network section: `code_hash`, `hash_type`, `args`.
6. Read all script JSON files from `docs/token-labels/information/script/*/index.json`.
7. Read `docs/script-name-overrides.json`: apply name overrides map, merge `known_scripts` as additional entries, apply deprecated flags by code_hash match.
8. Generate slug from the final (overridden) script name using the same rules.
9. Write `docs/metadata/scripts/{slug}.toml` per script. Use `[[mainnet]]` / `[[testnet]]` array-of-tables format. Fields: `name`, `description`. Optional: `website`, `category` (was `decoderType`). Each deployment: `code_hash`, `hash_type`. Optional: `data_hash` (omit entirely for pseudo-scripts with all-zero data_hash), `deprecated` (omit if false).
10. Write `docs/metadata/nft-tiers.toml` from `nftStorageTierOverrides` in overrides JSON. Format: `[overrides]` table with key=value pairs.
11. Print summary: token count per network, script count, any slug collisions.

- [ ] **Step 2: Run the migration script**

```bash
python3 scripts/migrate-labels.py
```

Expected: `docs/metadata/tokens/` ~675 files, `docs/metadata/scripts/` ~67 files, `docs/metadata/nft-tiers.toml` exists.

- [ ] **Step 3: Spot-check generated files**

Verify:
- `docs/metadata/tokens/seal.toml` has correct fields and `[mainnet]` section
- `docs/metadata/scripts/default-lock.toml` has `name = "Default Lock"` (not "SECP256K1/blake160")
- `docs/metadata/scripts/type-id.toml` has NO `data_hash` in its deployment
- `docs/metadata/scripts/dot-bit-time-index-state.toml` exists (from `known_scripts`)
- `docs/metadata/scripts/godwoken-custodian-lock.toml` exists (from `known_scripts`)

- [ ] **Step 4: Commit**

```bash
git add docs/metadata/
git commit -m "feat: generate docs/metadata/ TOML files from token-labels submodule"
```

---

## Task 2: Update All Rust Code (Atomic)

All Rust source changes in a single commit. These files are tightly coupled — changing them separately creates broken intermediate states.

**Files:**
- Modify: `crates/common/src/label_import.rs`
- Modify: `crates/indexer/Cargo.toml`
- Modify: `crates/indexer/build.rs`
- Modify: `crates/indexer/src/label_import.rs`
- Modify: `crates/indexer/src/config.rs`
- Modify: `crates/indexer/src/entry.rs`
- Modify: `crates/config/src/lib.rs`
- Modify: `crates/cli/src/main.rs`
- Modify: `crates/api/Cargo.toml`
- Modify: `crates/api/src/utils/assets.rs`

### Part A: Common Types

- [ ] **Step 1: Rewrite `crates/common/src/label_import.rs`**

Replace the entire file. New `LabelImportConfig` has only `metadata_path: Option<String>` and `network: String`. Remove `token_labels_path`, `import_udt`, `import_scripts`, `default_token_labels_path()`, `default_true()`. Keep `LabelImportResult` unchanged.

```rust
use serde::{Deserialize, Serialize};

/// Configuration for label import.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LabelImportConfig {
    /// Path to workdir metadata override directory. None = use bundled data only.
    #[serde(default)]
    pub metadata_path: Option<String>,
    #[serde(default = "default_network")]
    pub network: String,
}

impl Default for LabelImportConfig {
    fn default() -> Self {
        Self {
            metadata_path: None,
            network: default_network(),
        }
    }
}

fn default_network() -> String {
    "mainnet".to_string()
}

/// Label import summary result.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct LabelImportResult {
    pub udt_labels_imported: i64,
    pub script_labels_imported: i64,
    pub errors: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_label_import_config() {
        let cfg = LabelImportConfig::default();
        assert!(cfg.metadata_path.is_none());
        assert_eq!(cfg.network, "mainnet");
    }
}
```

### Part B: Build-Time Bundling

- [ ] **Step 2: Add dependencies to `crates/indexer/Cargo.toml`**

Add `toml` to both sections:

Under `[dependencies]` add:
```toml
toml = { workspace = true }
```

If `toml` is not in workspace deps, use `toml = "0.8"`.

Under `[build-dependencies]` add:
```toml
serde = { version = "1", features = ["derive"] }
toml = "0.8"
```

The existing `serde_json = "1"` in `[build-dependencies]` stays.

- [ ] **Step 3: Rewrite `crates/indexer/build.rs`**

Replace entirely. The new version:
1. Scans `docs/metadata/tokens/*.toml`, parses TOML, serializes to `bundled_udt_labels.json`.
2. Scans `docs/metadata/scripts/*.toml`, parses TOML, serializes to `bundled_script_labels.json`.
3. Extracts UDT-compatible script code_hashes from scripts with `category = "udt"` (excluding 3 hardcoded UDT code_hashes), writes to `bundled_udt_script_code_hashes.json`.
4. Sets `cargo:rerun-if-changed=docs/metadata`.

Does NOT bundle nft-tiers.toml (the API crate reads that directly via `CARGO_MANIFEST_DIR`).

Deserialization structs for build.rs:
```rust
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize)]
struct TokenMetadata {
    name: String,
    symbol: String,
    decimals: i16,
    standard: String,
    #[serde(default)]
    icon: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    mainnet: Option<TokenDeployment>,
    #[serde(default)]
    testnet: Option<TokenDeployment>,
}

#[derive(Deserialize, Serialize)]
struct TokenDeployment {
    code_hash: String,
    hash_type: String,
    args: String,
}

#[derive(Deserialize, Serialize)]
struct ScriptMetadata {
    name: String,
    description: String,
    #[serde(default)]
    website: Option<String>,
    #[serde(default)]
    category: Option<String>,
    #[serde(default)]
    mainnet: Vec<ScriptDeployment>,
    #[serde(default)]
    testnet: Vec<ScriptDeployment>,
}

#[derive(Deserialize, Serialize)]
struct ScriptDeployment {
    code_hash: String,
    #[serde(default)]
    data_hash: Option<String>,
    hash_type: String,
    #[serde(default)]
    deprecated: bool,
}
```

### Part C: Label Import

- [ ] **Step 4: Rewrite `crates/indexer/src/label_import.rs` — structs and bundled module**

Replace the old JSON structs and bundled module. New structs:

```rust
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct TokenMetadata {
    pub name: String,
    pub symbol: String,
    pub decimals: i16,
    pub standard: String,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub disabled: bool,
    #[serde(default)]
    pub mainnet: Option<TokenDeployment>,
    #[serde(default)]
    pub testnet: Option<TokenDeployment>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct TokenDeployment {
    pub code_hash: String,
    pub hash_type: String,
    pub args: String,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct ScriptMetadata {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub website: Option<String>,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub disabled: bool,
    #[serde(default)]
    pub mainnet: Vec<ScriptDeploymentEntry>,
    #[serde(default)]
    pub testnet: Vec<ScriptDeploymentEntry>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct ScriptDeploymentEntry {
    pub code_hash: String,
    #[serde(default)]
    pub data_hash: Option<String>,
    pub hash_type: String,
    #[serde(default)]
    pub deprecated: bool,
}
```

Updated bundled module (remove `BUNDLED_SCRIPT_OVERRIDES` and `script_overrides()`):

```rust
mod bundled {
    use super::*;

    const BUNDLED_UDT_LABELS: &[u8] =
        include_bytes!(concat!(env!("OUT_DIR"), "/bundled_udt_labels.json"));
    const BUNDLED_SCRIPT_LABELS: &[u8] =
        include_bytes!(concat!(env!("OUT_DIR"), "/bundled_script_labels.json"));

    pub fn udt_labels() -> Vec<TokenMetadata> {
        serde_json::from_slice(BUNDLED_UDT_LABELS)
            .expect("bundled UDT labels JSON is invalid — build.rs bug")
    }

    pub fn script_labels() -> Vec<ScriptMetadata> {
        serde_json::from_slice(BUNDLED_SCRIPT_LABELS)
            .expect("bundled script labels JSON is invalid — build.rs bug")
    }
}
```

- [ ] **Step 5: Add `compute_type_hash` and `make_slug` helpers**

```rust
use crate::rpc::Script;

fn compute_type_hash(deployment: &TokenDeployment) -> Result<Vec<u8>> {
    let script = Script {
        code_hash: deployment.code_hash.clone(),
        hash_type: deployment.hash_type.clone(),
        args: deployment.args.clone(),
    };
    Ok(ScriptParser::compute_script_hash(&script))
}

fn make_slug(name: &str) -> String {
    let slug: String = name
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let slug = slug.trim_matches('-').to_string();
    let mut result = String::new();
    let mut prev_hyphen = false;
    for c in slug.chars() {
        if c == '-' {
            if !prev_hyphen { result.push(c); }
            prev_hyphen = true;
        } else {
            result.push(c);
            prev_hyphen = false;
        }
    }
    result
}
```

- [ ] **Step 6: Rewrite `upsert_token_label`**

New signature: `fn upsert_token_label(store, token: &TokenMetadata, deployment: &TokenDeployment) -> Result<bool>`. Derive `type_hash` via `compute_type_hash(deployment)`. Same store write logic as before, but fields come from `TokenMetadata` instead of `UdtLabelInfo`. See Step 4 of original Task 4 for full code.

- [ ] **Step 7: Rewrite `upsert_script_label` and `import_single_deployment`**

Adapt from existing code (`label_import.rs` lines 503-634). Key changes:
- `ScriptLabelInfo` → `ScriptMetadata`
- `ScriptDeployment` → `ScriptDeploymentEntry`
- `script.decoder_type` → `script.category`
- `deployment.data_hash` (was required String) → `deployment.data_hash` (now `Option<String>`)
- `script.website` (was String) → `script.website` (now `Option<String>`, use `.clone().unwrap_or_default()` in store writes)
- `script.description` (was String) → keep as String

For `import_single_deployment`, the version_hash resolution changes from:
```rust
let data_hash = decode_hex(&deployment.data_hash).ok();
let is_zero_data = data_hash.as_ref().map(|h| h.iter().all(|&b| b == 0)).unwrap_or(true);
let version_hash = if is_zero_data { None } else { data_hash };
```
to:
```rust
let version_hash = match &deployment.data_hash {
    Some(dh) => {
        let decoded = decode_hex(dh).ok();
        let is_zero = decoded.as_ref().map(|h| h.iter().all(|&b| b == 0)).unwrap_or(true);
        if is_zero { None } else { decoded }
    }
    None => None,
};
```

The excluded-network cleanup loop in `upsert_script_label` (current lines 528-556) must also adapt `deployment.data_hash` to `Option<String>`:
```rust
for deployment in excluded {
    if let Ok(code_hash) = decode_hex(&deployment.code_hash) {
        if let Ok(Some(mut info)) = store.get_script_info(&code_hash) {
            if info.name.as_deref() == Some(&script.name) {
                info.name = None;
                info.deprecated = false;
                info.description = None;
                info.website = None;
                store.put_script_info_direct(&code_hash, &info)?;
            }
        }
    }
    if let Some(dh) = &deployment.data_hash {
        if let Ok(data_hash) = decode_hex(dh) {
            let is_zero_data = data_hash.iter().all(|&b| b == 0);
            if !is_zero_data {
                if let Ok(Some(mut version_info)) = store.get_script_version(&data_hash) {
                    if version_info.name.as_deref() == Some(&script.name) {
                        store.delete_script_version_by_label(&script.name, &data_hash)?;
                        version_info.name = None;
                        version_info.deprecated = false;
                        version_info.category = None;
                        version_info.description = None;
                        version_info.website = None;
                        store.put_script_version(&data_hash, &version_info)?;
                    }
                }
            }
        }
    }
}
```

- [ ] **Step 8: Rewrite public import functions**

Remove: `run_label_import_staged`, `load_token_labels`, `load_script_labels`, `load_script_overrides`, `apply_script_overrides`, `apply_deprecated_flags`, `scripts_overlap`.

New `run_label_import`:
```rust
pub fn run_label_import(
    store: &CkbadgerStore,
    config: &LabelImportConfig,
) -> Result<LabelImportResult> {
    let mut tokens = bundled::udt_labels();
    let mut scripts = bundled::script_labels();

    if let Some(ref metadata_path) = config.metadata_path {
        info!("Overlaying workdir metadata from: {}", metadata_path);
        overlay_from_dir(metadata_path, &mut tokens, &mut scripts)?;
    }

    import_all(store, &config.network, &tokens, &scripts)
}
```

Keep `run_label_import_bundled` as a public convenience (used by `api_integration.rs` tests):
```rust
pub fn run_label_import_bundled(store: &CkbadgerStore, network: &str) -> Result<LabelImportResult> {
    info!("Starting label import from bundled data (network={})", network);
    let tokens = bundled::udt_labels();
    let scripts = bundled::script_labels();
    import_all(store, network, &tokens, &scripts)
}
```

Shared `import_all`:
```rust
fn import_all(store: &CkbadgerStore, network: &str, tokens: &[TokenMetadata], scripts: &[ScriptMetadata]) -> Result<LabelImportResult> {
    let mut result = LabelImportResult::default();
    for token in tokens {
        if token.disabled { continue; }
        let deployment = match network {
            "mainnet" => token.mainnet.as_ref(),
            "testnet" => token.testnet.as_ref(),
            _ => token.mainnet.as_ref().or(token.testnet.as_ref()),
        };
        if let Some(deployment) = deployment {
            match upsert_token_label(store, token, deployment) {
                Ok(true) => result.udt_labels_imported += 1,
                Ok(false) => {}
                Err(e) => result.errors.push(format!("Token {}: {}", token.symbol, e)),
            }
        }
    }
    for script in scripts {
        if script.disabled { continue; }
        match upsert_script_label(store, script, network) {
            Ok(()) => result.script_labels_imported += 1,
            Err(e) => result.errors.push(format!("Script {}: {}", script.name, e)),
        }
    }
    info!("Label import completed: {} UDT, {} scripts, {} errors",
        result.udt_labels_imported, result.script_labels_imported, result.errors.len());
    Ok(result)
}
```

- [ ] **Step 9: Implement workdir overlay**

The overlay function matches by filename (slug), not by `make_slug(field)`. This correctly handles collision-suffixed filenames like `seal-00286029.toml`:

```rust
fn overlay_from_dir(
    metadata_path: &str,
    tokens: &mut Vec<TokenMetadata>,
    scripts: &mut Vec<ScriptMetadata>,
) -> Result<()> {
    let base = Path::new(metadata_path);

    let tokens_dir = base.join("tokens");
    if tokens_dir.exists() {
        for entry in std::fs::read_dir(&tokens_dir)?.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("toml") { continue; }
            let slug = path.file_stem().unwrap().to_string_lossy().to_string();
            let content = std::fs::read_to_string(&path)?;
            let token: TokenMetadata = toml::from_str(&content)
                .map_err(|e| anyhow::anyhow!("failed to parse {}: {}", path.display(), e))?;
            // Match bundled entries by slug derived from symbol
            if let Some(existing) = tokens.iter_mut().find(|t| make_slug(&t.symbol) == slug) {
                *existing = token;
            } else {
                tokens.push(token);
            }
        }
    }

    let scripts_dir = base.join("scripts");
    if scripts_dir.exists() {
        for entry in std::fs::read_dir(&scripts_dir)?.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("toml") { continue; }
            let slug = path.file_stem().unwrap().to_string_lossy().to_string();
            let content = std::fs::read_to_string(&path)?;
            let script: ScriptMetadata = toml::from_str(&content)
                .map_err(|e| anyhow::anyhow!("failed to parse {}: {}", path.display(), e))?;
            if let Some(existing) = scripts.iter_mut().find(|s| make_slug(&s.name) == slug) {
                *existing = script;
            } else {
                scripts.push(script);
            }
        }
    }

    Ok(())
}
```

Note: For collision-suffixed workdir files (e.g., `seal-00286029.toml`), `make_slug("SEAL")` = `"seal"` ≠ `"seal-00286029"`, so it won't match a bundled entry and will be added as a new entry. This is the correct behavior — the suffixed file is a distinct token.

### Part D: Indexer Config and Entry

- [ ] **Step 10: Update `crates/indexer/src/config.rs`**

Replace `token_labels_path: String` (line 38-39) with `metadata_path: Option<String>` using `#[serde(default)]`. Remove `default_token_labels_path()` (lines 82-84). Keep `network` and `default_network()`.

- [ ] **Step 11: Update `crates/indexer/src/entry.rs`**

Changes:
- **Imports (line 12)**: Replace `use crate::label_import::{run_label_import_bundled, run_label_import_staged}` with `use crate::label_import::run_label_import as label_import_run`.
- **`IndexerServiceConfig` (line 26)**: `token_labels_path: String` → `metadata_path: Option<String>`.
- **`From<IndexerServiceConfig> for Config` (line 53)**: `token_labels_path: svc.token_labels_path` → `metadata_path: svc.metadata_path`.
- **`LabelImportServiceConfig` (lines 63-72)**: Remove `token_labels_path`, `import_udt`, `import_scripts`, `use_bundled`. Add `metadata_path: Option<String>`. Keep `domain_data_path`, `append_only_data_path`, `network`, `store_runtime_config`.
- **`run_startup_label_import` (lines 81-129)**: Simplify — always call `label_import_run` with `LabelImportConfig { metadata_path: config.metadata_path.clone(), network: config.network.clone() }`.
- **`pub async fn run_label_import` (lines 634-676)**: Simplify — construct `LabelImportConfig { metadata_path: config.metadata_path, network: config.network }` and call `label_import_run`.

### Part E: Config Crate

- [ ] **Step 12: Update `crates/config/src/lib.rs`**

Removals:
- `LabelsConfig` struct (lines ~92-102)
- `load_labels_config()`, `parse_labels_config()` functions
- `resolve_token_labels_path()` function (line ~521)
- All tests: `test_parse_labels_*`, `test_load_labels_config_*`, `test_workdir_resolve_with_existing_labels_toml`, `test_workdir_resolve_both_token_labels_and_labels_toml`
- Remove `LabelsConfig` from `pub use` exports if re-exported

Updates:
- `WorkDir`: Replace `token_labels: Option<PathBuf>` + `labels_toml: Option<PathBuf>` with `metadata: Option<PathBuf>`.
- `WorkDir::resolve()`: Check for `{root}/metadata/` directory instead of `token-labels/` and `labels.toml`.
- Keep `resolve_share_dir()` — it is still used by CLI for frontend server resolution (line 806 of main.rs).
- Remove `resolve_token_labels_path()` from public exports.

Add test:
```rust
#[test]
fn test_workdir_resolve_with_metadata_dir() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    std::fs::create_dir(root.join("metadata")).unwrap();
    let wd = WorkDir::resolve(root);
    assert_eq!(wd.metadata, Some(root.join("metadata")));
}
```

### Part F: CLI

- [ ] **Step 13: Update `crates/cli/src/main.rs`**

Imports (line 13-16): Remove `resolve_token_labels_path` from the `ckbadger_config` import. Keep `resolve_share_dir` (used for frontend resolution on line 806).

`cmd_label_import` (lines 452-478): Simplify:
```rust
async fn cmd_label_import(workdir: &Path) -> Result<()> {
    let config = load_config(workdir)?;
    let work = WorkDir::resolve(workdir);
    let store_paths = resolve_store_paths(workdir, &config.store);

    let import_config = LabelImportServiceConfig {
        domain_data_path: store_paths.domain_data.to_string_lossy().to_string(),
        append_only_data_path: store_paths.append_only_data.to_string_lossy().to_string(),
        metadata_path: work.metadata.map(|p| p.to_string_lossy().to_string()),
        network: config.ckb.network.clone(),
        store_runtime_config: store_runtime_config(&config.store),
    };

    run_label_import(import_config).await
}
```

Indexer service config builder (~line 211): Replace `resolve_token_labels_path(work, resolve_share_dir().as_deref())` with `work.metadata.as_ref().map(|p| p.to_string_lossy().to_string())` for the `metadata_path` field.

Purge command (~lines 596-601): Replace `token_labels` and `labels_toml` preserve lines with:
```rust
if let Some(ref md) = work.metadata {
    println!("  {}/", md.display());
}
```

### Part G: API NFT Tiers

- [ ] **Step 14: Update `crates/api/src/utils/assets.rs` and `crates/api/Cargo.toml`**

Add `toml = "0.8"` (or workspace) to `crates/api/Cargo.toml` under `[dependencies]`.

Replace `ScriptNameOverridesDoc` with:
```rust
#[derive(Debug, Default, Deserialize)]
struct NftTiersDoc {
    #[serde(default)]
    overrides: HashMap<String, String>,
}
```

Update `load_and_validate_nft_storage_tier_overrides` (line 52-87):
- Change path: `join("../../docs/script-name-overrides.json")` → `join("../../docs/metadata/nft-tiers.toml")`
- Change parser: `serde_json::from_str` → `toml::from_str`
- Change error message: `"malformed docs/script-name-overrides.json"` → `"malformed docs/metadata/nft-tiers.toml"`

### Part H: Update Tests

- [ ] **Step 15: Update `label_import.rs` tests**

Rewrite all tests to use new struct types. Key tests to preserve (adapted):
- `test_bundled_label_import_has_no_errors`
- `test_bundled_udt_labels_deserialize` — check `>100` labels
- `test_bundled_script_labels_deserialize` — check `>10` labels, all non-empty names
- `test_run_label_import_bundled_imports_labels`
- `test_label_import_does_not_write_correctness_metadata` (SECP256K1)
- `test_run_label_import_bundled_imports_ckb_time_scripts`
- `test_run_label_import_bundled_imports_additional_known_scripts`
- `test_run_label_import_bundled_imports_legacy_godwoken_custodian_lock`
- `test_import_pseudo_script_with_zero_data_hash_succeeds` — test with `data_hash: None`
- `test_upsert_token_label_preserves_existing_max_supply`

New tests:
- `test_compute_type_hash` — verify against known SECP256K1 type_hash
- `test_make_slug` — `"SEAL"` → `"seal"`, `"CKB-FI"` → `"ckb-fi"`, `".bit Lock"` → `"bit-lock"`
- `test_workdir_overlay_add_and_replace` — verify disabled/replace/add logic

Remove tests for: `ScriptNameOverrides`, `apply_script_overrides`, `run_label_import_staged`, old JSON compat field tests.

### Part I: Verify and Commit

- [ ] **Step 16: Verify full build**

```bash
cargo check && cargo clippy
```

Expected: All crates compile cleanly.

- [ ] **Step 17: Run all tests**

```bash
cargo test
```

Expected: All tests pass. Fix any failures before committing.

- [ ] **Step 18: Commit all Rust changes atomically**

```bash
git add crates/common/src/label_import.rs \
       crates/indexer/Cargo.toml crates/indexer/build.rs \
       crates/indexer/src/label_import.rs crates/indexer/src/config.rs crates/indexer/src/entry.rs \
       crates/config/src/lib.rs \
       crates/cli/src/main.rs \
       crates/api/Cargo.toml crates/api/src/utils/assets.rs
git commit -m "feat: migrate label import from token-labels submodule to docs/metadata/ TOML format

Replace JSON-based label import with TOML-based metadata files.
All label data is now first-party, bundled at compile time,
with optional workdir override support.

Changes:
- Rewrite build.rs to scan docs/metadata/*.toml
- Rewrite label_import.rs with new TOML structs
- Simplify LabelImportConfig (remove import_udt/import_scripts)
- Update indexer config, entry, CLI, and API crate
- Remove LabelsConfig/labels.toml support
- NFT tiers now read from docs/metadata/nft-tiers.toml"
```

---

## Task 3: Cleanup — Remove Submodule and Old Files

**Files:**
- Remove: `docs/token-labels/` (git submodule)
- Remove: `docs/script-name-overrides.json`
- Modify: `.gitmodules`
- Modify: `README.md`
- Modify: `CLAUDE.md`
- Remove: `scripts/migrate-labels.py`

- [ ] **Step 1: Remove the submodule**

```bash
git rm docs/token-labels
```

- [ ] **Step 2: Edit `.gitmodules`**

Remove the `[submodule "docs/token-labels"]` block (3 lines at lines 7-9). Keep the 3 other submodule entries (`docs.nervos.org`, `rfcs`, `dob-cookbook`).

- [ ] **Step 3: Delete old files**

```bash
git rm docs/script-name-overrides.json
rm scripts/migrate-labels.py
```

- [ ] **Step 4: Update README.md**

In `README.md`, make these specific replacements:
- Line ~145: `token-labels/` → `metadata/` in the share directory tree
- Line ~217: Replace `token-labels/` line with `metadata/              # Optional: local metadata overrides`
- Line ~218: Delete the `labels.toml` line
- Lines ~592: `token-labels/       # [submodule] Known token metadata` → remove this line
- Lines ~640-643: Replace the label import resolution section:
  - Remove `<work_dir>/token-labels/` and `<install_dir>/share/token-labels/` references
  - Replace with: `Labels are bundled at compile time from docs/metadata/. Optional workdir override: place TOML files in <work_dir>/metadata/.`
  - Remove the `labels.toml` mention

- [ ] **Step 5: Update CLAUDE.md**

Search for `token-labels`, `labels.toml`, `script-name-overrides` references and update:
- `label_import` description: mention `docs/metadata/` instead of `docs/token-labels`
- File locations table: update the `Label import` row
- Any references to `labels.toml` → remove

- [ ] **Step 6: Verify clean build**

```bash
cargo clean && cargo build -p ckbadger && cargo test
```

Expected: Full clean build and all tests pass.

- [ ] **Step 7: Commit cleanup**

```bash
git add -A
git commit -m "chore: remove token-labels submodule, script-name-overrides.json, and labels.toml support"
```

- [ ] **Step 8: Clean up git submodule cache**

```bash
rm -rf .git/modules/docs/token-labels
```
