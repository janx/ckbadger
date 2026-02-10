use anyhow::Result;
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

            match upsert_script_label(store, script).await {
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

async fn upsert_token_label(_store: &CkbadgerStore, label: &UdtLabelInfo) -> Result<bool> {
    // TODO: Implement store.update_token_label() API
    tracing::debug!("Token label upsert pending store API: {}", label.type_hash);
    Ok(false)
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

async fn upsert_script_label(_store: &CkbadgerStore, script: &ScriptLabelInfo) -> Result<()> {
    // TODO: Implement store.upsert_script_info() API
    tracing::debug!("Script label upsert pending store API: {}", script.name);
    Ok(())
}
