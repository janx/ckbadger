use anyhow::Result;
use ckb_store_reader::CkbChainReader;
use ckbadger_common::{LabelImportConfig, LabelImportResult};
use ckbadger_store::CkbadgerStore;
use serde::Deserialize;
use std::path::Path;
use tracing::{debug, info};
use uuid::Uuid;

use crate::db::TaskDb;

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
    #[serde(default)]
    pub deprecated: Vec<String>,
}

pub async fn execute(
    db: &TaskDb,
    store: &CkbadgerStore,
    ckb_store: Option<&CkbChainReader>,
    task_id: Uuid,
    config: &LabelImportConfig,
) -> Result<()> {
    info!(
        "Starting label import: path={}, network={}, udt={}, scripts={}",
        config.token_labels_path, config.network, config.import_udt, config.import_scripts
    );

    let base_path = Path::new(&config.token_labels_path);
    if !base_path.exists() {
        let msg = format!(
            "Token labels path not found: {}. No labels imported.",
            config.token_labels_path
        );
        info!("{}", msg);
        db.update_progress(task_id, 100, 100, Some(&msg), None)
            .await?;
        db.complete_task(
            task_id,
            Some(serde_json::to_value(LabelImportResult::default())?),
        )
        .await?;
        return Ok(());
    }

    let mut result = LabelImportResult::default();

    if config.import_udt {
        db.update_progress(task_id, 0, 100, Some("Loading UDT labels..."), None)
            .await?;

        let labels = load_token_labels(&config.token_labels_path)?;
        let total = labels.len() as i64;
        info!("Found {} UDT labels to import", total);

        for (i, label) in labels.iter().enumerate() {
            if db.check_cancelled(task_id).await? {
                return Ok(());
            }

            match upsert_token_label(store, label).await {
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

            if i % 10 == 0 {
                let msg = format!("UDT labels: {}/{}", i + 1, total);
                db.update_progress(task_id, (i + 1) as i64, total, Some(&msg), None)
                    .await?;
            }
        }
    }

    if config.import_scripts {
        db.update_progress(task_id, 0, 100, Some("Loading script labels..."), None)
            .await?;

        let scripts = load_script_labels(&config.token_labels_path)?;
        let total = scripts.len() as i64;
        info!("Found {} script labels to import", total);

        for (i, script) in scripts.iter().enumerate() {
            if db.check_cancelled(task_id).await? {
                return Ok(());
            }

            match upsert_script_label(store, ckb_store, script).await {
                Ok(_) => {
                    result.script_labels_imported += 1;
                }
                Err(e) => {
                    result.errors.push(format!("Script {}: {}", script.name, e));
                }
            }

            if i % 10 == 0 {
                let msg = format!("Script labels: {}/{}", i + 1, total);
                db.update_progress(task_id, (i + 1) as i64, total, Some(&msg), None)
                    .await?;
            }
        }
    }

    info!(
        "Label import completed: {} UDT, {} scripts",
        result.udt_labels_imported, result.script_labels_imported
    );

    db.complete_task(task_id, Some(serde_json::to_value(&result)?))
        .await?;

    Ok(())
}

fn load_token_labels(base_path: &str) -> Result<Vec<UdtLabelInfo>> {
    let mut labels = Vec::new();

    for network in &["mainnet", "testnet"] {
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

fn parse_hash_type(hash_type: &str) -> u8 {
    match hash_type {
        "data" => 0,
        "type" => 1,
        "data1" => 2,
        "data2" => 4,
        _ => 0,
    }
}

fn decode_hex(s: &str) -> Result<Vec<u8>> {
    let s = s.strip_prefix("0x").unwrap_or(s);
    hex::decode(s).map_err(|e| anyhow::anyhow!("invalid hex: {}", e))
}

async fn upsert_token_label(store: &CkbadgerStore, label: &UdtLabelInfo) -> Result<bool> {
    let type_hash = decode_hex(&label.type_hash)?;

    let mut info =
        store
            .get_token(&type_hash)?
            .unwrap_or_else(|| ckbadger_store::types::TokenInfo {
                type_code_hash: decode_hex(&label.type_script.code_hash).unwrap_or_default(),
                hash_type: parse_hash_type(&label.type_script.hash_type),
                type_args: decode_hex(&label.type_script.args).unwrap_or_default(),
                standard: label.udt_type.clone(),
                name: None,
                symbol: None,
                decimals: None,
                total_supply: None,
                holders_count: 0,
                first_seen_block: 0,
                icon_url: None,
                description: None,
            });

    // Update label fields (preserve indexer-maintained stats like holders_count, total_supply)
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
                    for deployment in script.deployments.mainnet.iter_mut() {
                        let code_hash_lower = deployment.code_hash.to_lowercase();
                        if overrides
                            .deprecated
                            .iter()
                            .any(|d| d.to_lowercase() == code_hash_lower)
                        {
                            deployment.deprecated = true;
                        }
                    }
                    for deployment in script.deployments.testnet.iter_mut() {
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

async fn upsert_script_label(
    store: &CkbadgerStore,
    ckb_store: Option<&CkbChainReader>,
    script: &ScriptLabelInfo,
) -> Result<()> {
    // Import each non-deprecated deployment as a script_info entry keyed by code_hash
    let deployments = script
        .deployments
        .mainnet
        .iter()
        .chain(script.deployments.testnet.iter());

    for deployment in deployments {
        if deployment.deprecated {
            continue;
        }

        let code_hash = decode_hex(&deployment.code_hash)?;

        let mut info = store.get_script_info(&code_hash)?.unwrap_or_else(|| {
            ckbadger_store::types::ScriptInfo {
                code_hash: code_hash.clone(),
                hash_type: parse_hash_type(&deployment.hash_type),
                ..Default::default()
            }
        });

        // Update label fields (preserve indexer-maintained stats)
        info.name = Some(script.name.clone());
        info.description = Some(script.description.clone());
        info.website = Some(script.website.clone());

        // Store deployment cell's type_hash and data_hash for code cell lookup
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

        // Resolve code cell outpoint for scripts that can't use type index at runtime
        // (data/data1/data2 without dep_type_hash — e.g. genesis cells)
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
    }
    Ok(())
}
