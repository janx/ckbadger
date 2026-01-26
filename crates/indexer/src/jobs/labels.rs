use std::collections::HashMap;
use std::path::Path;
use std::time::Instant;

use anyhow::Result;
use serde::Deserialize;
use sqlx::PgPool;
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::control_plane::ControlPlaneClient;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UdtLabelInfo {
    name: Option<String>,
    symbol: String,
    icon: Option<String>,
    decimal: i16,
    tags: Option<Vec<String>>,
    manager: Option<String>,
    #[serde(rename = "type")]
    #[allow(dead_code)]
    type_script: UdtTypeScript,
    type_hash: String,
    description: Option<String>,
    udt_type: String,
    published: bool,
    email: Option<String>,
    famous: bool,
    operator_website: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UdtTypeScript {
    #[allow(dead_code)]
    code_hash: String,
    #[allow(dead_code)]
    hash_type: String,
    #[allow(dead_code)]
    args: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ScriptLabelInfo {
    name: String,
    description: String,
    rfc: String,
    website: String,
    source_url: String,
    decoder_type: Option<String>,
    deployments: ScriptDeployments,
}

#[derive(Debug, Clone, Deserialize)]
struct ScriptDeployments {
    mainnet: Vec<ScriptDeployment>,
    testnet: Vec<ScriptDeployment>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ScriptDeployment {
    tag: Option<String>,
    deprecated: bool,
    hash_type: String,
    data_hash: String,
    type_hash: String,
    code_hash: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct ScriptNameOverrides {
    #[serde(default)]
    overrides: HashMap<String, String>,
    #[serde(default)]
    deprecated: Vec<String>,
}

pub struct UdtLabelsTask {
    pool: PgPool,
    token_labels_path: Option<String>,
}

impl UdtLabelsTask {
    pub fn new(pool: PgPool, token_labels_path: Option<String>) -> Self {
        Self {
            pool,
            token_labels_path,
        }
    }

    pub async fn run(
        &self,
        control_plane: &ControlPlaneClient,
        job_id: &Uuid,
    ) -> Result<()> {
        let labels_path = match &self.token_labels_path {
            Some(p) => p.clone(),
            None => {
                info!("Token labels path not configured, skipping UDT labels update");
                return Ok(());
            }
        };

        info!("Starting UDT labels update from {}", labels_path);

        let labels = self.load_token_labels(&labels_path)?;
        let total = labels.len() as i64;

        if total == 0 {
            info!("No UDT labels found");
            return Ok(());
        }

        info!("Found {} UDT labels to import", total);
        control_plane.update_job_progress(job_id, 0, Some(total), None).await;

        let start_time = Instant::now();
        let mut processed = 0i64;

        for label in labels {
            if control_plane.is_job_cancelled(job_id).await {
                info!("Job cancelled, stopping UDT labels update");
                return Ok(());
            }

            if let Err(e) = self.upsert_token_label(&label).await {
                warn!("Failed to upsert token label for {}: {}", label.type_hash, e);
            }

            processed += 1;
            let elapsed = start_time.elapsed().as_secs_f64();
            let speed = if elapsed > 0.0 {
                processed as f64 / elapsed
            } else {
                0.0
            };

            if processed % 100 == 0 {
                control_plane
                    .update_job_progress(job_id, processed, None, Some(speed))
                    .await;
            }
        }

        control_plane
            .update_job_progress(job_id, processed, None, None)
            .await;

        info!("UDT labels update completed: {} processed", processed);
        Ok(())
    }

    pub async fn run_standalone(&self) -> Result<()> {
        let labels_path = match &self.token_labels_path {
            Some(p) => p.clone(),
            None => {
                info!("Token labels path not configured, skipping UDT labels update");
                return Ok(());
            }
        };

        info!("Starting UDT labels update from {}", labels_path);

        let labels = self.load_token_labels(&labels_path)?;
        let total = labels.len();

        if total == 0 {
            info!("No UDT labels found");
            return Ok(());
        }

        info!("Found {} UDT labels to import", total);

        let mut processed = 0usize;
        for label in labels {
            if let Err(e) = self.upsert_token_label(&label).await {
                warn!("Failed to upsert token label for {}: {}", label.type_hash, e);
            }
            processed += 1;
            if processed % 100 == 0 {
                info!("UDT labels progress: {}/{}", processed, total);
            }
        }

        info!("UDT labels update completed: {} processed", processed);
        Ok(())
    }

    fn load_token_labels(&self, base_path: &str) -> Result<Vec<UdtLabelInfo>> {
        let mut labels = Vec::new();

        for network in &["mainnet", "testnet"] {
            let network_path = Path::new(base_path)
                .join("information")
                .join("udt")
                .join(network);

            if !network_path.exists() {
                continue;
            }

            let entries = match std::fs::read_dir(&network_path) {
                Ok(e) => e,
                Err(e) => {
                    warn!("Failed to read directory {:?}: {}", network_path, e);
                    continue;
                }
            };

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

    async fn upsert_token_label(&self, label: &UdtLabelInfo) -> Result<()> {
        let type_hash = hex::decode(
            label
                .type_hash
                .strip_prefix("0x")
                .unwrap_or(&label.type_hash),
        )?;

        let tags: Option<Vec<String>> = label.tags.clone();

        let result = sqlx::query(
            r#"
            UPDATE tokens SET
                name = COALESCE($2, name),
                symbol = COALESCE($3, symbol),
                decimals = $4,
                description = COALESCE($5, description),
                icon_url = COALESCE($6, icon_url),
                published = $7,
                famous = $8,
                tags = $9,
                udt_type = $10,
                manager = $11,
                email = $12,
                operator_website = $13,
                label_updated_at = NOW(),
                updated_at = NOW()
            WHERE type_script_hash = $1
            "#,
        )
        .bind(&type_hash)
        .bind(&label.name)
        .bind(&label.symbol)
        .bind(label.decimal)
        .bind(&label.description)
        .bind(&label.icon)
        .bind(label.published)
        .bind(label.famous)
        .bind(&tags)
        .bind(&label.udt_type)
        .bind(&label.manager)
        .bind(&label.email)
        .bind(&label.operator_website)
        .execute(&self.pool)
        .await?;

        if result.rows_affected() > 0 {
            debug!(
                "Updated token label for {} ({})",
                label.type_hash, label.symbol
            );
        }

        Ok(())
    }
}

pub struct ScriptLabelsTask {
    pool: PgPool,
    token_labels_path: Option<String>,
}

impl ScriptLabelsTask {
    pub fn new(pool: PgPool, token_labels_path: Option<String>) -> Self {
        Self {
            pool,
            token_labels_path,
        }
    }

    pub async fn run(
        &self,
        control_plane: &ControlPlaneClient,
        job_id: &Uuid,
    ) -> Result<()> {
        let labels_path = match &self.token_labels_path {
            Some(p) => p.clone(),
            None => {
                info!("Token labels path not configured, skipping script labels update");
                return Ok(());
            }
        };

        info!("Starting script labels update from {}", labels_path);

        let scripts = self.load_script_labels(&labels_path)?;
        let total = scripts.len() as i64;

        if total == 0 {
            info!("No script labels found");
            return Ok(());
        }

        info!("Found {} script labels to import", total);
        control_plane.update_job_progress(job_id, 0, Some(total), None).await;

        let start_time = Instant::now();
        let mut processed = 0i64;

        for script in scripts {
            if control_plane.is_job_cancelled(job_id).await {
                info!("Job cancelled, stopping script labels update");
                return Ok(());
            }

            if let Err(e) = self.upsert_script_label(&script).await {
                warn!("Failed to upsert script label for {}: {}", script.name, e);
            } else {
                processed += 1;
            }

            let elapsed = start_time.elapsed().as_secs_f64();
            let speed = if elapsed > 0.0 {
                processed as f64 / elapsed
            } else {
                0.0
            };

            control_plane
                .update_job_progress(job_id, processed, None, Some(speed))
                .await;
        }

        info!("Script labels update completed: {} imported", processed);
        Ok(())
    }

    pub async fn run_standalone(&self) -> Result<()> {
        let labels_path = match &self.token_labels_path {
            Some(p) => p.clone(),
            None => {
                info!("Token labels path not configured, skipping script labels update");
                return Ok(());
            }
        };

        info!("Starting script labels update from {}", labels_path);

        let scripts = self.load_script_labels(&labels_path)?;
        let total = scripts.len();

        if total == 0 {
            info!("No script labels found");
            return Ok(());
        }

        info!("Found {} script labels to import", total);

        let mut processed = 0usize;
        for script in scripts {
            if let Err(e) = self.upsert_script_label(&script).await {
                warn!("Failed to upsert script label for {}: {}", script.name, e);
            } else {
                processed += 1;
            }
        }

        info!("Script labels update completed: {} imported", processed);
        Ok(())
    }

    fn load_script_labels(&self, base_path: &str) -> Result<Vec<ScriptLabelInfo>> {
        let mut scripts = Vec::new();
        let overrides = load_script_overrides(base_path);

        let script_path = Path::new(base_path).join("information").join("script");

        if !script_path.exists() {
            return Err(anyhow::anyhow!(
                "Script path {:?} does not exist",
                script_path
            ));
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
                            debug!("Overriding script name: {} -> {}", script.name, new_name);
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

    async fn upsert_script_label(&self, script: &ScriptLabelInfo) -> Result<()> {
        for (network, deployments) in [
            ("mainnet", &script.deployments.mainnet),
            ("testnet", &script.deployments.testnet),
        ] {
            for deployment in deployments {
                let code_hash = hex::decode(
                    deployment
                        .code_hash
                        .strip_prefix("0x")
                        .unwrap_or(&deployment.code_hash),
                )?;

                let data_hash = if deployment.data_hash.is_empty() {
                    None
                } else {
                    Some(hex::decode(
                        deployment
                            .data_hash
                            .strip_prefix("0x")
                            .unwrap_or(&deployment.data_hash),
                    )?)
                };

                let type_hash = if deployment.type_hash.is_empty() {
                    None
                } else {
                    Some(hex::decode(
                        deployment
                            .type_hash
                            .strip_prefix("0x")
                            .unwrap_or(&deployment.type_hash),
                    )?)
                };

                let tag = deployment.tag.clone().unwrap_or_default();

                let rfc = if script.rfc.is_empty() {
                    None
                } else {
                    Some(&script.rfc)
                };

                let website = if script.website.is_empty() {
                    None
                } else {
                    Some(&script.website)
                };

                let source_url = if script.source_url.is_empty() {
                    None
                } else {
                    Some(&script.source_url)
                };

                let description = if script.description.is_empty() {
                    None
                } else {
                    Some(&script.description)
                };

                sqlx::query(
                    r#"
                    INSERT INTO known_scripts (
                        code_hash, name, description, rfc, website, source_url, decoder_type,
                        network, hash_type, data_hash, type_hash, tag, deprecated, 
                        is_system, label_source, label_updated_at
                    ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, 'token-labels', NOW())
                    ON CONFLICT (code_hash, network, tag) DO UPDATE SET
                        name = EXCLUDED.name,
                        description = COALESCE(EXCLUDED.description, known_scripts.description),
                        rfc = COALESCE(EXCLUDED.rfc, known_scripts.rfc),
                        website = COALESCE(EXCLUDED.website, known_scripts.website),
                        source_url = COALESCE(EXCLUDED.source_url, known_scripts.source_url),
                        decoder_type = COALESCE(EXCLUDED.decoder_type, known_scripts.decoder_type),
                        hash_type = EXCLUDED.hash_type,
                        data_hash = EXCLUDED.data_hash,
                        type_hash = EXCLUDED.type_hash,
                        deprecated = EXCLUDED.deprecated,
                        label_updated_at = NOW()
                    "#,
                )
                .bind(&code_hash)
                .bind(&script.name)
                .bind(description)
                .bind(rfc)
                .bind(website)
                .bind(source_url)
                .bind(&script.decoder_type)
                .bind(network)
                .bind(&deployment.hash_type)
                .bind(&data_hash)
                .bind(&type_hash)
                .bind(&tag)
                .bind(deployment.deprecated)
                .bind(false)
                .execute(&self.pool)
                .await?;

                debug!(
                    "Upserted script label: {} ({}) for {}",
                    script.name,
                    if tag.is_empty() { "default" } else { &tag },
                    network
                );
            }
        }

        Ok(())
    }
}

fn load_script_overrides(base_path: &str) -> ScriptNameOverrides {
    let overrides_path = Path::new(base_path)
        .parent()
        .unwrap_or(Path::new("."))
        .join("script-name-overrides.json");

    debug!("Looking for script overrides at: {:?}", overrides_path);

    if !overrides_path.exists() {
        info!("Script overrides file not found at {:?}", overrides_path);
        return ScriptNameOverrides::default();
    }

    match std::fs::read_to_string(&overrides_path) {
        Ok(content) => match serde_json::from_str::<ScriptNameOverrides>(&content) {
            Ok(data) => {
                info!(
                    "Loaded {} script name overrides and {} deprecated scripts",
                    data.overrides.len(),
                    data.deprecated.len()
                );
                data
            }
            Err(e) => {
                warn!("Failed to parse script overrides: {}", e);
                ScriptNameOverrides::default()
            }
        },
        Err(e) => {
            debug!("No script overrides file found: {}", e);
            ScriptNameOverrides::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    #[test]
    fn test_load_script_overrides_valid_file() {
        let temp_dir = TempDir::new().unwrap();
        let overrides_path = temp_dir.path().join("script-name-overrides.json");

        let content = r#"{
            "overrides": {
                "DAS Lock": ".bit Lock",
                "Web5 DID": "did:ckb"
            },
            "deprecated": ["0xabc123"]
        }"#;

        let mut file = std::fs::File::create(&overrides_path).unwrap();
        file.write_all(content.as_bytes()).unwrap();

        let base_path = temp_dir.path().join("token-labels");
        std::fs::create_dir_all(&base_path).unwrap();

        let data = load_script_overrides(base_path.to_str().unwrap());

        assert_eq!(data.overrides.len(), 2);
        assert_eq!(
            data.overrides.get("DAS Lock"),
            Some(&".bit Lock".to_string())
        );
        assert_eq!(data.overrides.get("Web5 DID"), Some(&"did:ckb".to_string()));
        assert_eq!(data.deprecated.len(), 1);
        assert_eq!(data.deprecated[0], "0xabc123");
    }

    #[test]
    fn test_load_script_overrides_missing_file() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path().join("token-labels");
        std::fs::create_dir_all(&base_path).unwrap();

        let data = load_script_overrides(base_path.to_str().unwrap());

        assert!(data.overrides.is_empty());
        assert!(data.deprecated.is_empty());
    }

    #[test]
    fn test_load_script_overrides_invalid_json() {
        let temp_dir = TempDir::new().unwrap();
        let overrides_path = temp_dir.path().join("script-name-overrides.json");

        let mut file = std::fs::File::create(&overrides_path).unwrap();
        file.write_all(b"invalid json").unwrap();

        let base_path = temp_dir.path().join("token-labels");
        std::fs::create_dir_all(&base_path).unwrap();

        let data = load_script_overrides(base_path.to_str().unwrap());

        assert!(data.overrides.is_empty());
        assert!(data.deprecated.is_empty());
    }

    #[test]
    fn test_load_script_overrides_empty() {
        let temp_dir = TempDir::new().unwrap();
        let overrides_path = temp_dir.path().join("script-name-overrides.json");

        let content = r#"{"overrides": {}}"#;

        let mut file = std::fs::File::create(&overrides_path).unwrap();
        file.write_all(content.as_bytes()).unwrap();

        let base_path = temp_dir.path().join("token-labels");
        std::fs::create_dir_all(&base_path).unwrap();

        let data = load_script_overrides(base_path.to_str().unwrap());

        assert!(data.overrides.is_empty());
        assert!(data.deprecated.is_empty());
    }

    #[test]
    fn test_script_overrides_from_actual_docs() {
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let project_root = Path::new(manifest_dir).parent().unwrap().parent().unwrap();
        let base_path = project_root.join("docs/token-labels");

        let data = load_script_overrides(base_path.to_str().unwrap());

        assert_eq!(
            data.overrides.get("DAS Lock"),
            Some(&".bit Lock".to_string())
        );
        assert_eq!(
            data.overrides.get("DID Account"),
            Some(&".bit Account".to_string())
        );
        assert_eq!(
            data.overrides.get("DID Cell"),
            Some(&".bit Cell".to_string())
        );
        assert_eq!(data.overrides.get("Web5 DID"), Some(&"did:ckb".to_string()));
        assert!(data.deprecated.len() >= 5);
    }

    #[test]
    fn test_deprecated_code_hash_applied_to_deployment() {
        let overrides = ScriptNameOverrides {
            overrides: std::collections::HashMap::new(),
            deprecated: vec!["0xABC123".to_string(), "0xdef456".to_string()],
        };

        let mut deployment = ScriptDeployment {
            code_hash: "0xabc123".to_string(),
            hash_type: "type".to_string(),
            data_hash: String::new(),
            type_hash: String::new(),
            deprecated: false,
            tag: None,
        };

        let code_hash_lower = deployment.code_hash.to_lowercase();
        if overrides
            .deprecated
            .iter()
            .any(|d| d.to_lowercase() == code_hash_lower)
        {
            deployment.deprecated = true;
        }

        assert!(deployment.deprecated);
    }

    #[test]
    fn test_deprecated_code_hash_case_insensitive() {
        let overrides = ScriptNameOverrides {
            overrides: std::collections::HashMap::new(),
            deprecated: vec!["0xABCDEF".to_string()],
        };

        for test_hash in &["0xabcdef", "0xABCDEF", "0xAbCdEf"] {
            let code_hash_lower = test_hash.to_lowercase();
            let matches = overrides
                .deprecated
                .iter()
                .any(|d| d.to_lowercase() == code_hash_lower);
            assert!(matches, "Should match {}", test_hash);
        }
    }

    #[test]
    fn test_non_deprecated_code_hash_unchanged() {
        let overrides = ScriptNameOverrides {
            overrides: std::collections::HashMap::new(),
            deprecated: vec!["0xabc123".to_string()],
        };

        let mut deployment = ScriptDeployment {
            code_hash: "0x999999".to_string(),
            hash_type: "type".to_string(),
            data_hash: String::new(),
            type_hash: String::new(),
            deprecated: false,
            tag: None,
        };

        let code_hash_lower = deployment.code_hash.to_lowercase();
        if overrides
            .deprecated
            .iter()
            .any(|d| d.to_lowercase() == code_hash_lower)
        {
            deployment.deprecated = true;
        }

        assert!(!deployment.deprecated);
    }
}
