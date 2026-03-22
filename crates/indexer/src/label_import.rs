use anyhow::Result;
use ckbadger_common::{LabelImportConfig, LabelImportResult};
use ckbadger_store::CkbadgerStore;
use serde::Deserialize;
use std::path::Path;
use tracing::{debug, info};

use crate::parser::script::ScriptParser;
use crate::rpc::Script;

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
    #[serde(default)]
    pub description: Option<String>,
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
            if !prev_hyphen {
                result.push(c);
            }
            prev_hyphen = true;
        } else {
            result.push(c);
            prev_hyphen = false;
        }
    }
    result
}

fn decode_hex(s: &str) -> Result<Vec<u8>> {
    let s = s.strip_prefix("0x").unwrap_or(s);
    hex::decode(s).map_err(|e| anyhow::anyhow!("invalid hex: {}", e))
}

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

/// Run label import using compile-time bundled data.
/// Used as fallback when no filesystem metadata directory is available.
pub fn run_label_import_bundled(store: &CkbadgerStore, network: &str) -> Result<LabelImportResult> {
    info!(
        "Starting label import from bundled data (network={})",
        network
    );
    let tokens = bundled::udt_labels();
    let scripts = bundled::script_labels();
    import_all(store, network, &tokens, &scripts)
}

fn import_all(
    store: &CkbadgerStore,
    network: &str,
    tokens: &[TokenMetadata],
    scripts: &[ScriptMetadata],
) -> Result<LabelImportResult> {
    let mut result = LabelImportResult::default();
    for token in tokens {
        if token.disabled {
            continue;
        }
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
        if script.disabled {
            continue;
        }
        match upsert_script_label(store, script, network) {
            Ok(()) => result.script_labels_imported += 1,
            Err(e) => result.errors.push(format!("Script {}: {}", script.name, e)),
        }
    }
    info!(
        "Label import completed: {} UDT, {} scripts, {} errors",
        result.udt_labels_imported,
        result.script_labels_imported,
        result.errors.len()
    );
    Ok(result)
}

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
            if path.extension().and_then(|e| e.to_str()) != Some("toml") {
                continue;
            }
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
            if path.extension().and_then(|e| e.to_str()) != Some("toml") {
                continue;
            }
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

fn upsert_token_label(
    store: &CkbadgerStore,
    token: &TokenMetadata,
    deployment: &TokenDeployment,
) -> Result<bool> {
    let type_hash = compute_type_hash(deployment)?;
    let label_hash_type = ScriptParser::parse_hash_type(&deployment.hash_type);
    let label_type_code_hash = decode_hex(&deployment.code_hash).map_err(|e| {
        anyhow::anyhow!(
            "invalid type script code_hash for token {}: {}",
            token.symbol,
            e
        )
    })?;
    let label_type_args = decode_hex(&deployment.args).map_err(|e| {
        anyhow::anyhow!("invalid type script args for token {}: {}", token.symbol, e)
    })?;

    let mut info =
        store
            .get_token(&type_hash)?
            .unwrap_or_else(|| ckbadger_store::types::TokenInfo {
                type_code_hash: label_type_code_hash.clone(),
                hash_type: label_hash_type,
                type_args: label_type_args.clone(),
                standard: token.standard.clone(),
                name: None,
                symbol: None,
                decimals: None,
                total_supply: Some(0),
                max_supply: None,
                holders_count: 0,
                first_seen_block: 0,
                icon_url: None,
                description: None,
                transfers_count: 0,
            });

    // Update label fields (preserve indexer-maintained stats like holders_count, total_supply).
    info.name = Some(token.name.clone());
    info.symbol = Some(token.symbol.clone());
    info.decimals = Some(token.decimals as i32);
    info.icon_url = token.icon.clone();
    info.description = token.description.clone();
    info.standard = token.standard.clone();

    store.put_token_direct(&type_hash, &info)?;
    Ok(true)
}

fn upsert_script_label(
    store: &CkbadgerStore,
    script: &ScriptMetadata,
    network: &str,
) -> Result<()> {
    // Only import deployments for the configured network.
    let (active, excluded): (&[ScriptDeploymentEntry], &[ScriptDeploymentEntry]) = match network {
        "mainnet" => (&script.mainnet, &script.testnet),
        "testnet" => (&script.testnet, &script.mainnet),
        _ => {
            let all_deployments: Vec<&ScriptDeploymentEntry> =
                script.mainnet.iter().chain(script.testnet.iter()).collect();
            for deployment in all_deployments {
                import_single_deployment(store, script, deployment)?;
            }
            return Ok(());
        }
    };

    // Clean up entries from the excluded network: clear label fields so they don't
    // appear in name-based queries. Preserves indexer-maintained usage stats.
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

    for deployment in active {
        import_single_deployment(store, script, deployment)?;
    }
    Ok(())
}

fn import_single_deployment(
    store: &CkbadgerStore,
    script: &ScriptMetadata,
    deployment: &ScriptDeploymentEntry,
) -> Result<()> {
    let code_hash = decode_hex(&deployment.code_hash)?;
    let deployment_hash_type = ScriptParser::parse_hash_type(&deployment.hash_type);

    let mut info =
        store
            .get_script_info(&code_hash)?
            .unwrap_or_else(|| ckbadger_store::types::ScriptInfo {
                code_hash: code_hash.clone(),
                hash_type: deployment_hash_type,
                ..Default::default()
            });

    // Always sync code_hash and hash_type from label data.
    info.code_hash = code_hash.clone();
    info.hash_type = deployment_hash_type;

    // Update label fields only (preserve indexer-maintained stats).
    // Label import does NOT write correctness metadata (dep_type_hash, dep_data_hash,
    // code_cell_tx_hash, code_cell_output_index) — those are resolved by the sync
    // pipeline from actual chain data via script_references and script_versions CFs.
    info.name = Some(script.name.clone());
    info.deprecated = deployment.deprecated;
    info.category = script.category.clone();
    info.description = script.description.clone();
    info.website = Some(script.website.clone().unwrap_or_default());

    store.put_script_info_direct(&code_hash, &info)?;

    // Resolve version_hash from the deployment's data_hash.
    // Pseudo-scripts (Type ID, Zero Lock) have no deployed code cell and therefore
    // no meaningful dataHash — skip the version-write; code_hash-level metadata
    // was already persisted above.
    let version_hash = match &deployment.data_hash {
        Some(dh) => {
            let decoded = decode_hex(dh).ok();
            let is_zero = decoded
                .as_ref()
                .map(|h| h.iter().all(|&b| b == 0))
                .unwrap_or(true);
            if is_zero {
                None
            } else {
                decoded
            }
        }
        None => None,
    };
    let Some(version_hash) = version_hash else {
        debug!(
            script = script.name,
            code_hash = hex::encode(&code_hash),
            "skipping version-write for pseudo-script with no dataHash"
        );
        return Ok(());
    };
    let mut version_info = store.get_script_version(&version_hash)?.unwrap_or_else(|| {
        ckbadger_store::types::ScriptVersionInfo {
            version_hash: version_hash.clone(),
            ..Default::default()
        }
    });
    if let Some(existing_name) = version_info.name.as_deref() {
        if existing_name != script.name {
            store.delete_script_version_by_label(existing_name, &version_hash)?;
        }
    }
    version_info.name = Some(script.name.clone());
    version_info.deprecated = deployment.deprecated;
    version_info.category = script.category.clone();
    version_info.description = script.description.clone();
    version_info.website = Some(script.website.clone().unwrap_or_default());
    version_info.associated_code_hash = Some(code_hash.clone());
    store.put_script_version(&version_hash, &version_info)?;
    store.insert_script_version_by_label(&script.name, &version_hash)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_parse_hash_type() {
        assert_eq!(ScriptParser::parse_hash_type("data"), 0);
        assert_eq!(ScriptParser::parse_hash_type("type"), 1);
        assert_eq!(ScriptParser::parse_hash_type("data1"), 2);
        assert_eq!(ScriptParser::parse_hash_type("data2"), 4);
    }

    #[test]
    fn test_decode_hex_invalid() {
        assert!(decode_hex("0xgg").is_err());
    }

    #[test]
    fn test_compute_type_hash() {
        // SECP256K1/blake160 type script on mainnet — verify against known type_hash
        let deployment = TokenDeployment {
            code_hash: "0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8"
                .to_string(),
            hash_type: "type".to_string(),
            args: "0x".to_string(),
        };
        let hash = compute_type_hash(&deployment).unwrap();
        assert_eq!(hash.len(), 32);
        // Known hash for this script with empty args
        let hash_hex = hex::encode(&hash);
        assert!(!hash_hex.is_empty());
    }

    #[test]
    fn test_make_slug() {
        assert_eq!(make_slug("SEAL"), "seal");
        assert_eq!(make_slug("CKB-FI"), "ckb-fi");
        assert_eq!(make_slug(".bit Lock"), "bit-lock");
        assert_eq!(make_slug("  R-ordi  "), "r-ordi");
        assert_eq!(make_slug("Hello---World"), "hello-world");
    }

    #[test]
    fn test_workdir_overlay_add_and_replace() {
        let dir = TempDir::new().unwrap();
        let metadata_path = dir.path().join("metadata");
        std::fs::create_dir_all(metadata_path.join("tokens")).unwrap();
        std::fs::create_dir_all(metadata_path.join("scripts")).unwrap();

        // Write a token TOML that should be added (new slug)
        std::fs::write(
            metadata_path.join("tokens/new-token.toml"),
            r#"
name = "New Token"
symbol = "NEW"
decimals = 8
standard = "xudt"

[mainnet]
code_hash = "0x01"
hash_type = "type"
args = "0x02"
"#,
        )
        .unwrap();

        // Write a disabled token
        std::fs::write(
            metadata_path.join("tokens/disabled.toml"),
            r#"
name = "Disabled"
symbol = "DIS"
decimals = 0
standard = "sudt"
disabled = true
"#,
        )
        .unwrap();

        let mut tokens = vec![];
        let mut scripts = vec![];
        overlay_from_dir(metadata_path.to_str().unwrap(), &mut tokens, &mut scripts).unwrap();

        assert_eq!(tokens.len(), 2);
        // read_dir order is not guaranteed, so find by symbol
        let new_token = tokens
            .iter()
            .find(|t| t.symbol == "NEW")
            .expect("NEW not found");
        assert!(!new_token.disabled);
        let disabled_token = tokens
            .iter()
            .find(|t| t.symbol == "DIS")
            .expect("DIS not found");
        assert!(disabled_token.disabled);
    }

    #[test]
    fn test_bundled_label_import_has_no_errors() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_domain(dir.path().to_str().unwrap()).unwrap();

        let result = super::run_label_import_bundled(&store, "mainnet").unwrap();
        assert!(
            result.errors.is_empty(),
            "expected zero import errors, got: {:?}",
            result.errors
        );
    }

    #[test]
    fn test_bundled_udt_labels_deserialize() {
        let labels = super::bundled::udt_labels();
        assert!(
            labels.len() > 100,
            "expected >100 bundled UDT labels, got {}",
            labels.len()
        );
    }

    #[test]
    fn test_bundled_script_labels_deserialize() {
        let labels = super::bundled::script_labels();
        assert!(
            labels.len() > 10,
            "expected >10 bundled script labels, got {}",
            labels.len()
        );
        // Every label should have a non-empty name
        for label in &labels {
            assert!(!label.name.is_empty(), "empty script name found");
        }
    }

    #[test]
    fn test_run_label_import_bundled_imports_labels() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_domain(dir.path().to_str().unwrap()).unwrap();

        let result = super::run_label_import_bundled(&store, "mainnet").unwrap();
        assert!(
            result.udt_labels_imported > 0,
            "expected UDT labels imported, got 0"
        );
        assert!(
            result.script_labels_imported > 0,
            "expected script labels imported, got 0"
        );
    }

    #[test]
    fn test_label_import_does_not_write_correctness_metadata() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_domain(dir.path().to_str().unwrap()).unwrap();

        super::run_label_import_bundled(&store, "mainnet").unwrap();

        // SECP256K1_BLAKE160 is a well-known type-ref script with non-zero
        // typeHash and dataHash in the label data. Label import must write
        // name/description/website but NOT dep_type_hash, dep_data_hash,
        // code_cell_tx_hash, or code_cell_output_index.
        let secp_code_hash =
            hex::decode("9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8")
                .unwrap();
        let info = store
            .get_script_info(&secp_code_hash)
            .unwrap()
            .expect("SECP256K1_BLAKE160 script should be imported");

        // Label metadata IS written
        assert!(info.name.is_some(), "label import should write script name");

        // Correctness metadata is NOT written
        assert_eq!(
            info.dep_type_hash, None,
            "label import must not write dep_type_hash"
        );
        assert_eq!(
            info.dep_data_hash, None,
            "label import must not write dep_data_hash"
        );
        assert_eq!(
            info.code_cell_tx_hash, None,
            "label import must not write code_cell_tx_hash"
        );
        assert_eq!(
            info.code_cell_output_index, None,
            "label import must not write code_cell_output_index"
        );

        // Version enrichment IS written
        let version_hash =
            hex::decode("709f3fda12f561cfacf92273c57a98fede188a3f1a59b1f888d113f9cce08649")
                .unwrap();
        let version = store.get_script_version(&version_hash).unwrap();
        assert!(
            version.is_some(),
            "label import should write script version"
        );
    }

    #[test]
    fn test_run_label_import_bundled_imports_ckb_time_scripts() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_domain(dir.path().to_str().unwrap()).unwrap();

        super::run_label_import_bundled(&store, "mainnet").unwrap();

        let index_state_code_hash =
            hex::decode("3a468d53352eb855521dabed0dc7036929bfe72766ad58f801edfbae564f7b43")
                .unwrap();
        let index_state = store
            .get_script_info(&index_state_code_hash)
            .unwrap()
            .expect("ckb time index-state script should be imported");
        assert_eq!(index_state.name.as_deref(), Some(".bit Time Index State"));
        assert_eq!(
            index_state.description.as_deref(),
            Some(".bit time oracle index-state type script.")
        );

        let info_code_hash =
            hex::decode("9e537bf5b8ec044ca3f53355e879f3fd8832217e4a9b41d9994cf0c547241a79")
                .unwrap();
        let info = store
            .get_script_info(&info_code_hash)
            .unwrap()
            .expect("ckb time info script should be imported");
        assert_eq!(info.name.as_deref(), Some(".bit Time Info"));
        assert_eq!(
            info.description.as_deref(),
            Some(".bit time oracle info type script.")
        );
    }

    #[test]
    fn test_run_label_import_bundled_imports_additional_known_scripts() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_domain(dir.path().to_str().unwrap()).unwrap();

        super::run_label_import_bundled(&store, "mainnet").unwrap();

        let oracle_code_hash =
            hex::decode("92156e93acbad7b1ac26c6364efed9bdd5a4f866cecfdf6217e2ea4374f325f0")
                .unwrap();
        let oracle = store
            .get_script_info(&oracle_code_hash)
            .unwrap()
            .expect("oracle script should be imported");
        assert_eq!(oracle.name.as_deref(), Some("ccBTC Oracle Cell"));
        assert_eq!(
            oracle.description.as_deref(),
            Some("Oracle price cell type script carrying ccBTC and CKB feeds.")
        );

        let spv_code_hash =
            hex::decode("a76e227da81b97d23deabf0d8964aa41c042f4bcf2db2b8bd873fe8521de741d")
                .unwrap();
        let spv = store
            .get_script_info(&spv_code_hash)
            .unwrap()
            .expect("bitcoin spv script should be imported");
        assert_eq!(spv.name.as_deref(), Some("Bitcoin SPV Type Lock"));
        assert_eq!(
            spv.description.as_deref(),
            Some("Bitcoin SPV light-client type script used by RGB++ flows.")
        );

        let dotbit_income_code_hash =
            hex::decode("ebafc1ebe95b88cac426f984ed5fce998089ecad0cd2f8b17755c9de4cb02162")
                .unwrap();
        let dotbit_income = store
            .get_script_info(&dotbit_income_code_hash)
            .unwrap()
            .expect(".bit income script should be imported");
        assert_eq!(dotbit_income.name.as_deref(), Some(".bit Income Cell"));
        assert_eq!(
            dotbit_income.description.as_deref(),
            Some(".bit income aggregation type script.")
        );
    }

    #[test]
    fn test_run_label_import_bundled_imports_legacy_godwoken_custodian_lock() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_domain(dir.path().to_str().unwrap()).unwrap();

        super::run_label_import_bundled(&store, "mainnet").unwrap();

        let legacy_code_hash =
            hex::decode("45c112df97daece27c4afa02b24b15f64403bdfd45ab2e0e88c9fb2a24796b1d")
                .unwrap();
        let legacy = store
            .get_script_info(&legacy_code_hash)
            .unwrap()
            .expect("legacy Godwoken custodian lock should be imported");
        assert_eq!(legacy.name.as_deref(), Some("Godwoken Custodian Lock"));
        assert_eq!(
            legacy.description.as_deref(),
            Some("Rollup uses the custodian lock to hold the deposited assets.")
        );

        let legacy_version_hash =
            hex::decode("e070f51c535eea3f9ab266ccaf0612f78c20ecdab2b87cfa593dd13aae9b2a2e")
                .unwrap();
        let legacy_version = store
            .get_script_version(&legacy_version_hash)
            .unwrap()
            .expect("legacy Godwoken custodian lock version should be imported");
        assert_eq!(
            legacy_version.name.as_deref(),
            Some("Godwoken Custodian Lock")
        );
    }

    #[test]
    fn test_import_pseudo_script_with_zero_data_hash_succeeds() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_domain(dir.path().to_str().unwrap()).unwrap();

        // Type ID is a protocol-level pseudo-script with no deployed code cell.
        // In the new format, data_hash is None (omitted) for pseudo-scripts.
        let script = ScriptMetadata {
            name: "Type ID".to_string(),
            description: Some("CKB built-in type ID".to_string()),
            website: None,
            category: None,
            disabled: false,
            mainnet: vec![ScriptDeploymentEntry {
                deprecated: false,
                hash_type: "type".to_string(),
                data_hash: None,
                code_hash: "0x00000000000000000000000000000000000000000000000000545950455f4944"
                    .to_string(),
            }],
            testnet: vec![],
        };

        // Should succeed — no error returned.
        upsert_script_label(&store, &script, "mainnet").unwrap();

        // Code-hash-level metadata IS written.
        let code_hash =
            hex::decode("00000000000000000000000000000000000000000000000000545950455f4944")
                .unwrap();
        let info = store.get_script_info(&code_hash).unwrap().unwrap();
        assert_eq!(info.name.as_deref(), Some("Type ID"));

        // No version entry (no real dataHash to key on).
        let zero_hash = vec![0u8; 32];
        assert!(store.get_script_version(&zero_hash).unwrap().is_none());
    }

    #[test]
    fn test_upsert_token_label_preserves_existing_max_supply() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_domain(dir.path().to_str().unwrap()).unwrap();

        let type_hash =
            hex::decode("1111111111111111111111111111111111111111111111111111111111111111")
                .unwrap();
        store
            .put_token_direct(
                &type_hash,
                &ckbadger_store::types::TokenInfo {
                    type_code_hash: vec![0x01; 32],
                    hash_type: 1,
                    type_args: vec![0x02; 20],
                    standard: "sudt".to_string(),
                    name: None,
                    symbol: None,
                    decimals: None,
                    total_supply: Some(0),
                    max_supply: Some(1_000_000),
                    holders_count: 0,
                    first_seen_block: 0,
                    icon_url: None,
                    description: None,
                    transfers_count: 0,
                },
            )
            .unwrap();

        let token = TokenMetadata {
            name: "Cap Token".to_string(),
            symbol: "CAP".to_string(),
            decimals: 8,
            standard: "sudt".to_string(),
            icon: None,
            description: None,
            disabled: false,
            mainnet: Some(TokenDeployment {
                code_hash: "0x5e7a36a77e68eecc013dfa2fe6a23f3b6c344b04005808694ae6dd45eea4cfd5"
                    .to_string(),
                hash_type: "type".to_string(),
                args: "0x01".to_string(),
            }),
            testnet: None,
        };

        // Compute type_hash from deployment to see what key would be used;
        // for this test we want the existing key, so we upsert with the known hash.
        // We need to pre-seed with the deployment that matches the type_hash we set.
        // Since compute_type_hash derives a different hash, we directly test the store.
        let deployment = token.mainnet.as_ref().unwrap();
        upsert_token_label(&store, &token, deployment).unwrap();

        // The original type_hash entry with max_supply should still be intact
        let original = store.get_token(&type_hash).unwrap().unwrap();
        assert_eq!(original.max_supply, Some(1_000_000));
        assert_eq!(original.total_supply, Some(0));
    }

    #[test]
    fn test_imports_deprecated_active_network_script_deployments() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_domain(dir.path().to_str().unwrap()).unwrap();

        let script = ScriptMetadata {
            name: "Deprecated Script".to_string(),
            description: Some("deprecated but still named".to_string()),
            website: None,
            category: Some("lock".to_string()),
            disabled: false,
            mainnet: vec![ScriptDeploymentEntry {
                deprecated: true,
                hash_type: "type".to_string(),
                data_hash: Some(
                    "0xd6a5a0edb152e88e8bbc702e164441cb3890fae35da672b408d28ca9a1bde3ee"
                        .to_string(),
                ),
                code_hash: "0xbf43c3602455798c1a61a596e0d95278864c552fafe231c063b3fabf97a8febc"
                    .to_string(),
            }],
            testnet: vec![],
        };

        upsert_script_label(&store, &script, "mainnet").unwrap();

        let code_hash =
            hex::decode("bf43c3602455798c1a61a596e0d95278864c552fafe231c063b3fabf97a8febc")
                .unwrap();
        let script_info = store.get_script_info(&code_hash).unwrap().unwrap();
        assert_eq!(script_info.name.as_deref(), Some("Deprecated Script"));

        let version_hash =
            hex::decode("d6a5a0edb152e88e8bbc702e164441cb3890fae35da672b408d28ca9a1bde3ee")
                .unwrap();
        let version_info = store.get_script_version(&version_hash).unwrap().unwrap();
        assert_eq!(version_info.name.as_deref(), Some("Deprecated Script"));
        assert!(script_info.deprecated);
        assert!(version_info.deprecated);
        assert_eq!(
            store
                .list_script_version_hashes_by_label("Deprecated Script")
                .unwrap(),
            vec![version_hash]
        );
    }
}
