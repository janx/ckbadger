use anyhow::Result;
use ckb_store_reader::CkbChainReader;
use ckbadger_common::{LabelImportConfig, LabelImportResult};
use ckbadger_store::CkbadgerStore;
use serde::Deserialize;
use std::path::Path;
use tracing::{debug, info};

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
struct UdtLabelInfo {
    pub name: Option<String>,
    pub symbol: String,
    pub icon: Option<String>,
    pub decimal: i16,
    pub tags: Option<Vec<String>>,
    pub manager: Option<String>,
    #[serde(rename = "type")]
    pub type_script: UdtTypeScript,
    pub type_hash: String,
    pub description: Option<String>,
    pub udt_type: String,
    pub published: bool,
    pub email: Option<String>,
    pub famous: bool,
    pub operator_website: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
struct UdtTypeScript {
    pub code_hash: String,
    pub hash_type: String,
    pub args: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
struct ScriptLabelInfo {
    pub name: String,
    pub description: String,
    pub rfc: String,
    pub website: String,
    pub source_url: String,
    pub decoder_type: Option<String>,
    pub deployments: ScriptDeployments,
}

#[derive(Debug, Clone, Deserialize)]
struct ScriptDeployments {
    pub mainnet: Vec<ScriptDeployment>,
    pub testnet: Vec<ScriptDeployment>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
struct ScriptDeployment {
    pub tag: Option<String>,
    pub deprecated: bool,
    pub hash_type: String,
    pub data_hash: String,
    pub type_hash: String,
    pub code_hash: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct ScriptNameOverrides {
    #[serde(default)]
    pub overrides: std::collections::HashMap<String, String>,
    // Parsed for shared docs compatibility; currently consumed by API-side NFT metadata logic.
    #[allow(dead_code)]
    #[serde(default)]
    pub nft_storage_tier_overrides: std::collections::HashMap<String, String>,
    #[serde(default)]
    pub deprecated: Vec<String>,
}

pub fn run_label_import(
    store: &CkbadgerStore,
    ckb_store: Option<&CkbChainReader>,
    config: &LabelImportConfig,
) -> Result<LabelImportResult> {
    info!(
        "Starting label import: path={}, network={}, udt={}, scripts={}",
        config.token_labels_path, config.network, config.import_udt, config.import_scripts
    );

    let base_path = Path::new(&config.token_labels_path);
    if !base_path.exists() {
        info!(
            "Token labels path not found: {}. Skipping label import.",
            config.token_labels_path
        );
        return Ok(LabelImportResult::default());
    }

    let mut result = LabelImportResult::default();

    if config.import_udt {
        let labels = load_token_labels(&config.token_labels_path)?;
        info!("Found {} UDT labels to import", labels.len());

        for label in &labels {
            match upsert_token_label(store, label) {
                Ok(updated) => {
                    if updated {
                        result.udt_labels_imported += 1;
                    }
                }
                Err(e) => {
                    result
                        .errors
                        .push(format!("UDT {}: {}", label.type_hash, e));
                }
            }
        }
    }

    if config.import_scripts {
        let scripts = load_script_labels(&config.token_labels_path)?;
        info!("Found {} script labels to import", scripts.len());

        for script in &scripts {
            match upsert_script_label(store, ckb_store, script, &config.network) {
                Ok(()) => {
                    result.script_labels_imported += 1;
                }
                Err(e) => {
                    result.errors.push(format!("Script {}: {}", script.name, e));
                }
            }
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

fn load_token_labels(base_path: &str) -> Result<Vec<UdtLabelInfo>> {
    let mut labels = Vec::new();

    for network in ["mainnet", "testnet"] {
        let network_path = Path::new(base_path)
            .join("information")
            .join("udt")
            .join(network);

        if !network_path.exists() {
            continue;
        }

        let entries = std::fs::read_dir(&network_path)?;

        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }

            let index_path = path.join("index.json");
            if !index_path.exists() {
                continue;
            }

            match std::fs::read_to_string(&index_path) {
                Ok(content) => match serde_json::from_str::<UdtLabelInfo>(&content) {
                    Ok(label) => {
                        if label.published {
                            labels.push(label);
                        }
                    }
                    Err(e) => {
                        debug!("Failed to parse {:?}: {}", index_path, e);
                    }
                },
                Err(e) => {
                    debug!("Failed to read {:?}: {}", index_path, e);
                }
            }
        }
    }

    Ok(labels)
}

fn parse_hash_type(hash_type: &str) -> Result<u8> {
    match hash_type {
        "data" => Ok(0),
        "type" => Ok(1),
        "data1" => Ok(2),
        "data2" => Ok(4),
        _ => Err(anyhow::anyhow!("unknown hash_type: '{}'", hash_type)),
    }
}

fn decode_hex(s: &str) -> Result<Vec<u8>> {
    let s = s.strip_prefix("0x").unwrap_or(s);
    hex::decode(s).map_err(|e| anyhow::anyhow!("invalid hex: {}", e))
}

fn upsert_token_label(store: &CkbadgerStore, label: &UdtLabelInfo) -> Result<bool> {
    let type_hash = decode_hex(&label.type_hash)?;
    let label_hash_type = parse_hash_type(&label.type_script.hash_type)?;

    let mut info =
        store
            .get_token(&type_hash)?
            .unwrap_or_else(|| ckbadger_store::types::TokenInfo {
                type_code_hash: decode_hex(&label.type_script.code_hash).unwrap_or_default(),
                hash_type: label_hash_type,
                type_args: decode_hex(&label.type_script.args).unwrap_or_default(),
                standard: label.udt_type.clone(),
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
    info.name = label.name.clone().or(Some(label.symbol.clone()));
    info.symbol = Some(label.symbol.clone());
    info.decimals = Some(label.decimal as i32);
    info.icon_url = label.icon.clone();
    info.description = label.description.clone();
    info.standard = label.udt_type.clone();

    store.put_token_direct(&type_hash, &info)?;
    Ok(true)
}

fn load_script_labels(base_path: &str) -> Result<Vec<ScriptLabelInfo>> {
    let mut scripts = Vec::new();
    let overrides = load_script_overrides(base_path);

    let script_path = Path::new(base_path).join("information").join("script");

    if !script_path.exists() {
        return Ok(scripts);
    }

    let entries = std::fs::read_dir(&script_path)?;

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        let index_path = path.join("index.json");
        if !index_path.exists() {
            continue;
        }

        match std::fs::read_to_string(&index_path) {
            Ok(content) => match serde_json::from_str::<ScriptLabelInfo>(&content) {
                Ok(mut script) => {
                    if let Some(new_name) = overrides.overrides.get(&script.name) {
                        script.name = new_name.clone();
                    }
                    for deployment in &mut script.deployments.mainnet {
                        let code_hash_lower = deployment.code_hash.to_lowercase();
                        if overrides
                            .deprecated
                            .iter()
                            .any(|d| d.to_lowercase() == code_hash_lower)
                        {
                            deployment.deprecated = true;
                        }
                    }
                    for deployment in &mut script.deployments.testnet {
                        let code_hash_lower = deployment.code_hash.to_lowercase();
                        if overrides
                            .deprecated
                            .iter()
                            .any(|d| d.to_lowercase() == code_hash_lower)
                        {
                            deployment.deprecated = true;
                        }
                    }
                    scripts.push(script);
                }
                Err(e) => {
                    debug!("Failed to parse {:?}: {}", index_path, e);
                }
            },
            Err(e) => {
                debug!("Failed to read {:?}: {}", index_path, e);
            }
        }
    }

    Ok(scripts)
}

fn load_script_overrides(base_path: &str) -> ScriptNameOverrides {
    let overrides_path = Path::new(base_path)
        .parent()
        .unwrap_or(Path::new("."))
        .join("script-name-overrides.json");

    if !overrides_path.exists() {
        return ScriptNameOverrides::default();
    }

    match std::fs::read_to_string(&overrides_path) {
        Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
        Err(_) => ScriptNameOverrides::default(),
    }
}

fn upsert_script_label(
    store: &CkbadgerStore,
    ckb_store: Option<&CkbChainReader>,
    script: &ScriptLabelInfo,
    network: &str,
) -> Result<()> {
    // Only import deployments for the configured network.
    let (active, excluded): (&[ScriptDeployment], &[ScriptDeployment]) = match network {
        "mainnet" => (&script.deployments.mainnet, &script.deployments.testnet),
        "testnet" => (&script.deployments.testnet, &script.deployments.mainnet),
        _ => {
            let all_deployments: Vec<&ScriptDeployment> = script
                .deployments
                .mainnet
                .iter()
                .chain(script.deployments.testnet.iter())
                .collect();
            for deployment in all_deployments {
                if !deployment.deprecated {
                    import_single_deployment(store, ckb_store, script, deployment)?;
                }
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
                    info.description = None;
                    info.website = None;
                    info.dep_type_hash = None;
                    info.dep_data_hash = None;
                    info.code_cell_tx_hash = None;
                    info.code_cell_output_index = None;
                    store.put_script_info_direct(&code_hash, &info)?;
                }
            }
        }
    }

    for deployment in active {
        if !deployment.deprecated {
            import_single_deployment(store, ckb_store, script, deployment)?;
        }
    }
    Ok(())
}

fn import_single_deployment(
    store: &CkbadgerStore,
    ckb_store: Option<&CkbChainReader>,
    script: &ScriptLabelInfo,
    deployment: &ScriptDeployment,
) -> Result<()> {
    let code_hash = decode_hex(&deployment.code_hash)?;
    let deployment_hash_type = parse_hash_type(&deployment.hash_type)?;

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

    // Update label fields (preserve indexer-maintained stats).
    info.name = Some(script.name.clone());
    info.description = Some(script.description.clone());
    info.website = Some(script.website.clone());

    // Store deployment cell's type_hash and data_hash for code cell lookup.
    let type_hash = decode_hex(&deployment.type_hash).ok();
    let is_zero_type = type_hash
        .as_ref()
        .map(|h| h.iter().all(|&b| b == 0))
        .unwrap_or(true);
    info.dep_type_hash = if is_zero_type { None } else { type_hash };

    let data_hash = decode_hex(&deployment.data_hash).ok();
    let is_zero_data = data_hash
        .as_ref()
        .map(|h| h.iter().all(|&b| b == 0))
        .unwrap_or(true);
    info.dep_data_hash = if is_zero_data { None } else { data_hash };

    // Resolve code cell outpoint for scripts that can't use type index at runtime.
    if info.hash_type != 1 && info.dep_type_hash.is_none() {
        if let Some(ref dh) = info.dep_data_hash {
            if dh.len() == 32 {
                let mut hash = [0u8; 32];
                hash.copy_from_slice(dh);
                if let Some(ckb) = ckb_store {
                    if let Some((tx_hash, idx)) = ckb.find_cell_by_data_hash(&hash) {
                        info.code_cell_tx_hash = Some(tx_hash.to_vec());
                        info.code_cell_output_index = Some(idx);
                        debug!(
                            "Resolved code cell for {}: {}:{}",
                            script.name,
                            hex::encode(tx_hash),
                            idx
                        );
                    }
                }
            }
        }
    }

    store.put_script_info_direct(&code_hash, &info)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_parse_hash_type() {
        assert_eq!(parse_hash_type("data").unwrap(), 0);
        assert_eq!(parse_hash_type("type").unwrap(), 1);
        assert_eq!(parse_hash_type("data1").unwrap(), 2);
        assert_eq!(parse_hash_type("data2").unwrap(), 4);
        assert!(parse_hash_type("unknown").is_err());
    }

    #[test]
    fn test_decode_hex_invalid() {
        assert!(decode_hex("0xgg").is_err());
    }

    #[test]
    fn test_run_label_import_missing_path_returns_default() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open(dir.path().to_str().unwrap()).unwrap();

        let config = LabelImportConfig {
            token_labels_path: dir.path().join("not-found").to_string_lossy().to_string(),
            ..Default::default()
        };

        let result = run_label_import(&store, None, &config).unwrap();
        assert_eq!(result.udt_labels_imported, 0);
        assert_eq!(result.script_labels_imported, 0);
        assert!(result.errors.is_empty());
    }

    #[test]
    fn test_upsert_token_label_preserves_existing_max_supply() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open(dir.path().to_str().unwrap()).unwrap();

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

        let label = UdtLabelInfo {
            name: Some("Cap Token".to_string()),
            symbol: "CAP".to_string(),
            icon: None,
            decimal: 8,
            tags: None,
            manager: None,
            type_script: UdtTypeScript {
                code_hash: "0x5e7a36a77e68eecc013dfa2fe6a23f3b6c344b04005808694ae6dd45eea4cfd5"
                    .to_string(),
                hash_type: "type".to_string(),
                args: "0x01".to_string(),
            },
            type_hash: "0x1111111111111111111111111111111111111111111111111111111111111111"
                .to_string(),
            description: None,
            udt_type: "sudt".to_string(),
            published: true,
            email: None,
            famous: false,
            operator_website: None,
        };

        upsert_token_label(&store, &label).unwrap();
        let token = store.get_token(&type_hash).unwrap().unwrap();

        assert_eq!(token.max_supply, Some(1_000_000));
        assert_eq!(token.total_supply, Some(0));
    }
}
