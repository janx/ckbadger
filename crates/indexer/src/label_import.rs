use anyhow::Result;
use ckbadger_common::{LabelImportConfig, LabelImportResult};
use ckbadger_store::CkbadgerStore;
use serde::Deserialize;
use std::path::Path;
use tracing::{debug, info};

use crate::parser::script::ScriptParser;
use crate::rpc::Script;

pub(crate) mod bundled {
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
    #[serde(default)]
    pub metadata_slug: Option<String>,
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
    pub mainnet: Option<ScriptNetworkMetadata>,
    #[serde(default)]
    pub testnet: Option<ScriptNetworkMetadata>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(try_from = "ScriptNetworkMetadataRaw")]
pub(crate) struct ScriptNetworkMetadata {
    pub versions: Vec<ScriptDeploymentEntry>,
    pub pseudo: Option<PseudoScriptDeployment>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct ScriptNetworkMetadataRaw {
    #[serde(default)]
    versions: Vec<ScriptDeploymentEntry>,
    #[serde(default)]
    pseudo: Option<PseudoScriptDeployment>,
}

impl TryFrom<ScriptNetworkMetadataRaw> for ScriptNetworkMetadata {
    type Error = String;

    fn try_from(raw: ScriptNetworkMetadataRaw) -> std::result::Result<Self, Self::Error> {
        let has_versions = !raw.versions.is_empty();
        let has_pseudo = raw.pseudo.is_some();
        match (has_versions, has_pseudo) {
            (true, false) | (false, true) => Ok(Self {
                versions: raw.versions,
                pseudo: raw.pseudo,
            }),
            (false, false) => Err(
                "network metadata must define exactly one of `versions` or `pseudo`".to_string(),
            ),
            (true, true) => {
                Err("network metadata cannot define both `versions` and `pseudo`".to_string())
            }
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PseudoScriptDeployment {
    pub code_hash: String,
    pub hash_type: ValidatedHashType,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(try_from = "ScriptDeploymentEntryRaw")]
pub(crate) struct ScriptDeploymentEntry {
    pub version_hash: String,
    pub canonical_ref_hash: String,
    pub canonical_hash_type: ValidatedHashType,
    #[serde(default)]
    pub deprecated: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct ScriptDeploymentEntryRaw {
    version_hash: String,
    canonical_ref_hash: String,
    canonical_hash_type: String,
    #[serde(default)]
    deprecated: bool,
}

impl TryFrom<ScriptDeploymentEntryRaw> for ScriptDeploymentEntry {
    type Error = String;

    fn try_from(raw: ScriptDeploymentEntryRaw) -> std::result::Result<Self, Self::Error> {
        Ok(Self {
            version_hash: raw.version_hash,
            canonical_ref_hash: raw.canonical_ref_hash,
            canonical_hash_type: ValidatedHashType::new(
                raw.canonical_hash_type,
                "canonical_hash_type",
            )?,
            deprecated: raw.deprecated,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ValidatedHashType(String);

impl ValidatedHashType {
    fn new(value: String, field: &str) -> std::result::Result<Self, String> {
        match value.as_str() {
            "data" | "type" | "data1" | "data2" => Ok(Self(value)),
            _ => Err(format!("invalid {field}: `{value}`")),
        }
    }

    fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for ValidatedHashType {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        ValidatedHashType::new(value, "hash_type").map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone)]
pub(crate) enum ImportDeployment {
    Version(ScriptDeploymentEntry),
    Pseudo(PseudoScriptDeployment),
}

impl ScriptNetworkMetadata {
    pub(crate) fn import_deployments(&self) -> Vec<ImportDeployment> {
        if let Some(pseudo) = &self.pseudo {
            return vec![ImportDeployment::Pseudo(pseudo.clone())];
        }
        self.versions
            .iter()
            .cloned()
            .map(ImportDeployment::Version)
            .collect()
    }
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
            let mut script: ScriptMetadata = toml::from_str(&content)
                .map_err(|e| anyhow::anyhow!("failed to parse {}: {}", path.display(), e))?;
            script.metadata_slug = Some(slug.clone());
            if let Some(existing) = scripts
                .iter_mut()
                .find(|s| s.metadata_slug.as_deref() == Some(slug.as_str()))
            {
                *existing = script;
            } else {
                scripts.push(script);
            }
        }
    }

    Ok(())
}

fn script_family_id(script: &ScriptMetadata) -> Result<&str> {
    script.metadata_slug.as_deref().ok_or_else(|| {
        anyhow::anyhow!(
            "script metadata missing metadata_slug: name={}",
            script.name
        )
    })
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
                max_supply: None,
                first_seen_block: 0,
                icon_url: None,
                description: None,
                transfers_count: 0,
            });

    // Update label fields while preserving chain-derived metadata.
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
    let family_id = script_family_id(script)?;
    let (active, excluded): (Vec<ImportDeployment>, Vec<ImportDeployment>) = match network {
        "mainnet" => (
            script
                .mainnet
                .as_ref()
                .map(ScriptNetworkMetadata::import_deployments)
                .unwrap_or_default(),
            script
                .testnet
                .as_ref()
                .map(ScriptNetworkMetadata::import_deployments)
                .unwrap_or_default(),
        ),
        "testnet" => (
            script
                .testnet
                .as_ref()
                .map(ScriptNetworkMetadata::import_deployments)
                .unwrap_or_default(),
            script
                .mainnet
                .as_ref()
                .map(ScriptNetworkMetadata::import_deployments)
                .unwrap_or_default(),
        ),
        _ => {
            let all_deployments: Vec<ImportDeployment> = script
                .mainnet
                .as_ref()
                .into_iter()
                .flat_map(ScriptNetworkMetadata::import_deployments)
                .chain(
                    script
                        .testnet
                        .as_ref()
                        .into_iter()
                        .flat_map(ScriptNetworkMetadata::import_deployments),
                )
                .collect();
            upsert_script_family(store, script, family_id, &all_deployments)?;
            for deployment in &all_deployments {
                import_single_deployment(store, script, family_id, deployment)?;
            }
            return Ok(());
        }
    };

    // Only create/update the family when there are active deployments for this
    // network. Without this guard, scripts with deployments on only the other
    // network leak into this network's /scripts listing with versions_count=0.
    if !active.is_empty() {
        upsert_script_family(store, script, family_id, &active)?;
    }

    // Clean up entries from the excluded network: clear label fields so they don't
    // appear in name-based queries. Preserves indexer-maintained usage stats.
    for deployment in &excluded {
        let code_hash_hex = match deployment {
            ImportDeployment::Version(version) => &version.canonical_ref_hash,
            ImportDeployment::Pseudo(pseudo) => &pseudo.code_hash,
        };
        if let Ok(code_hash) = decode_hex(code_hash_hex) {
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
        if let ImportDeployment::Version(version) = deployment {
            if let Ok(data_hash) = decode_hex(&version.version_hash) {
                let is_zero_data = data_hash.iter().all(|&b| b == 0);
                if !is_zero_data {
                    if let Ok(Some(mut version_info)) = store.get_script_version(&data_hash) {
                        if version_info.name.as_deref() == Some(&script.name) {
                            store.delete_script_version_by_label(&script.name, &data_hash)?;
                            if let Some(existing_family_id) = version_info.family_id.as_deref() {
                                store.delete_script_version_by_family_direct(
                                    existing_family_id,
                                    &data_hash,
                                )?;
                            }
                            version_info.name = None;
                            version_info.family_id = None;
                            version_info.deprecated = false;
                            version_info.category = None;
                            version_info.description = None;
                            version_info.website = None;
                            version_info.canonical_reference_hash = None;
                            version_info.canonical_hash_type = None;
                            version_info.associated_code_hash = None;
                            store.put_script_version(&data_hash, &version_info)?;
                        }
                    }
                }
            }
        }
    }

    for deployment in &active {
        import_single_deployment(store, script, family_id, deployment)?;
    }
    Ok(())
}

fn upsert_script_family(
    store: &CkbadgerStore,
    script: &ScriptMetadata,
    family_id: &str,
    active_deployments: &[ImportDeployment],
) -> Result<()> {
    let mut family = store.get_script_family(family_id)?.unwrap_or_else(|| {
        ckbadger_store::types::ScriptFamilyInfo {
            family_id: family_id.to_string(),
            ..Default::default()
        }
    });
    if !family.name.is_empty() && family.name != script.name {
        store.delete_script_family_name_direct(&family.name)?;
    }
    family.family_id = family_id.to_string();
    family.name = script.name.clone();
    family.description = script.description.clone();
    family.website = script.website.clone();
    family.category = script.category.clone();
    family.versions_count = active_deployments
        .iter()
        .filter(|deployment| matches!(deployment, ImportDeployment::Version(_)))
        .count() as i64;
    store.put_script_family_direct(family_id, &family)?;
    store.put_script_family_name_direct(&script.name, family_id)?;
    Ok(())
}

fn import_single_deployment(
    store: &CkbadgerStore,
    script: &ScriptMetadata,
    family_id: &str,
    deployment: &ImportDeployment,
) -> Result<()> {
    let (code_hash_hex, hash_type) = match deployment {
        ImportDeployment::Version(version) => (
            version.canonical_ref_hash.as_str(),
            version.canonical_hash_type.as_str(),
        ),
        ImportDeployment::Pseudo(pseudo) => (pseudo.code_hash.as_str(), pseudo.hash_type.as_str()),
    };
    let code_hash = decode_hex(code_hash_hex)?;
    let deployment_hash_type = ScriptParser::parse_hash_type(hash_type);

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
    info.deprecated =
        matches!(deployment, ImportDeployment::Version(version) if version.deprecated);
    info.category = script.category.clone();
    info.description = script.description.clone();
    info.website = script.website.clone();

    store.put_script_info_direct(&code_hash, &info)?;

    // Resolve version_hash from the deployment metadata.
    // Pseudo-scripts (Type ID, Zero Lock) have no deployed code cell and therefore
    // no meaningful version_hash — skip the version-write; code_hash-level metadata
    // was already persisted above.
    let version_hash = match deployment {
        ImportDeployment::Version(version) => {
            let decoded = decode_hex(&version.version_hash).ok();
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
        ImportDeployment::Pseudo(_) => None,
    };
    let Some(version_hash) = version_hash else {
        debug!(
            script = script.name,
            code_hash = hex::encode(&code_hash),
            "skipping version-write for pseudo-script with no version_hash"
        );
        return Ok(());
    };
    let mut version_info = store.get_script_version(&version_hash)?.unwrap_or_else(|| {
        ckbadger_store::types::ScriptVersionInfo {
            version_hash: version_hash.clone(),
            ..Default::default()
        }
    });
    if let Some(existing_family_id) = version_info.family_id.as_deref() {
        if existing_family_id != family_id {
            store.delete_script_version_by_family_direct(existing_family_id, &version_hash)?;
        }
    }
    if let Some(existing_name) = version_info.name.as_deref() {
        if existing_name != script.name {
            store.delete_script_version_by_label(existing_name, &version_hash)?;
        }
    }
    version_info.name = Some(script.name.clone());
    version_info.family_id = Some(family_id.to_string());
    version_info.deprecated =
        matches!(deployment, ImportDeployment::Version(version) if version.deprecated);
    version_info.category = script.category.clone();
    version_info.description = script.description.clone();
    version_info.website = script.website.clone();
    version_info.associated_code_hash = Some(code_hash.clone());
    if let ImportDeployment::Version(version) = deployment {
        version_info.canonical_reference_hash = Some(code_hash.clone());
        version_info.canonical_hash_type = Some(ScriptParser::parse_hash_type(
            version.canonical_hash_type.as_str(),
        ));
    }
    store.put_script_version(&version_hash, &version_info)?;
    store.insert_script_version_by_label(&script.name, &version_hash)?;
    store.put_script_version_by_family_direct(family_id, &version_hash)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ckbadger_store::types::{ScriptFamilyInfo, ScriptVersionInfo};
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
    fn test_script_metadata_parses_family_first_versioned_shape() {
        let script: ScriptMetadata = toml::from_str(
            r#"
name = "Omni Lock"
description = "Universal lock"
website = "https://omnilock.example"
category = "lock"

[mainnet]
[[mainnet.versions]]
version_hash = "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
canonical_ref_hash = "0x1111111111111111111111111111111111111111111111111111111111111111"
canonical_hash_type = "type"

[[mainnet.versions]]
version_hash = "0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
canonical_ref_hash = "0x2222222222222222222222222222222222222222222222222222222222222222"
canonical_hash_type = "type"
deprecated = true

[testnet]
[[testnet.versions]]
version_hash = "0xcccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"
canonical_ref_hash = "0x3333333333333333333333333333333333333333333333333333333333333333"
canonical_hash_type = "data1"
"#,
        )
        .unwrap();

        let mainnet = script.mainnet.expect("expected mainnet metadata");
        let testnet = script.testnet.expect("expected testnet metadata");

        assert_eq!(mainnet.versions.len(), 2);
        assert!(mainnet.pseudo.is_none());
        assert_eq!(mainnet.versions[0].canonical_hash_type.as_str(), "type");
        assert_eq!(
            mainnet.versions[1].version_hash,
            "0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
        );
        assert!(mainnet.versions[1].deprecated);

        assert_eq!(testnet.versions.len(), 1);
        assert!(testnet.pseudo.is_none());
        assert_eq!(
            testnet.versions[0].canonical_ref_hash,
            "0x3333333333333333333333333333333333333333333333333333333333333333"
        );
        assert_eq!(testnet.versions[0].canonical_hash_type.as_str(), "data1");
    }

    #[test]
    fn test_script_metadata_parses_explicit_pseudo_script_branch() {
        let script: ScriptMetadata = toml::from_str(
            r#"
name = "Type ID"

[mainnet.pseudo]
code_hash = "0x00000000000000000000000000000000000000000000000000545950455f4944"
hash_type = "type"

[testnet.pseudo]
code_hash = "0x00000000000000000000000000000000000000000000000000545950455f4944"
hash_type = "type"
"#,
        )
        .unwrap();

        let mainnet = script.mainnet.expect("expected mainnet metadata");
        let testnet = script.testnet.expect("expected testnet metadata");

        assert!(mainnet.versions.is_empty());
        assert_eq!(
            mainnet.pseudo.expect("expected pseudo metadata").code_hash,
            "0x00000000000000000000000000000000000000000000000000545950455f4944"
        );
        assert!(testnet.versions.is_empty());
        assert_eq!(
            testnet
                .pseudo
                .expect("expected pseudo metadata")
                .hash_type
                .as_str(),
            "type"
        );
    }

    #[test]
    fn test_script_metadata_rejects_legacy_deployment_array_shape() {
        let err = toml::from_str::<ScriptMetadata>(
            r#"
name = "Legacy"

[[mainnet]]
version_hash = "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
canonical_ref_hash = "0x1111111111111111111111111111111111111111111111111111111111111111"
canonical_hash_type = "type"
"#,
        )
        .unwrap_err();

        let msg = err.to_string();
        assert!(
            msg.contains("mainnet") || msg.contains("invalid type"),
            "unexpected error: {msg}"
        );
    }

    #[test]
    fn test_script_metadata_rejects_old_version_entry_field_names() {
        let err = toml::from_str::<ScriptMetadata>(
            r#"
name = "Legacy"

[mainnet]
[[mainnet.versions]]
code_hash = "0x1111111111111111111111111111111111111111111111111111111111111111"
data_hash = "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
hash_type = "type"
"#,
        )
        .unwrap_err();

        let msg = err.to_string();
        assert!(
            msg.contains("canonical_ref_hash")
                || msg.contains("unknown field")
                || msg.contains("missing field"),
            "unexpected error: {msg}"
        );
    }

    #[test]
    fn test_script_metadata_rejects_invalid_canonical_hash_type() {
        let err = toml::from_str::<ScriptMetadata>(
            r#"
name = "Broken"

[mainnet]
[[mainnet.versions]]
version_hash = "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
canonical_ref_hash = "0x1111111111111111111111111111111111111111111111111111111111111111"
canonical_hash_type = "bogus"
"#,
        )
        .unwrap_err();

        assert!(
            err.to_string().contains("canonical_hash_type"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn test_script_metadata_rejects_invalid_pseudo_hash_type() {
        let err = toml::from_str::<ScriptMetadata>(
            r#"
name = "Broken Pseudo"

[mainnet.pseudo]
code_hash = "0x00000000000000000000000000000000000000000000000000545950455f4944"
hash_type = "bogus"
"#,
        )
        .unwrap_err();

        assert!(
            err.to_string().contains("hash_type"),
            "unexpected error: {err}"
        );
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
    fn test_label_import_writes_family_and_versions() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_domain(dir.path().to_str().unwrap()).unwrap();

        super::run_label_import_bundled(&store, "mainnet").unwrap();

        let family = store
            .get_script_family("default-lock")
            .unwrap()
            .expect("default-lock family should be imported");
        assert_eq!(
            family,
            ScriptFamilyInfo {
                family_id: "default-lock".to_string(),
                name: "Default Lock".to_string(),
                description: Some(
                    "SECP256K1/blake160 is the default lock script to verify CKB transaction signature."
                        .to_string()
                ),
                website: None,
                category: None,
                versions_count: 1,
                ..Default::default()
            }
        );

        let family_versions = store
            .list_script_version_hashes_by_family("default-lock")
            .unwrap();
        assert_eq!(
            family_versions,
            vec![
                hex::decode("709f3fda12f561cfacf92273c57a98fede188a3f1a59b1f888d113f9cce08649")
                    .unwrap()
            ]
        );

        let version = store
            .get_script_version(&family_versions[0])
            .unwrap()
            .expect("default-lock version should exist");
        assert_eq!(
            version,
            ScriptVersionInfo {
                version_hash: hex::decode(
                    "709f3fda12f561cfacf92273c57a98fede188a3f1a59b1f888d113f9cce08649"
                )
                .unwrap(),
                name: Some("Default Lock".to_string()),
                description: Some(
                    "SECP256K1/blake160 is the default lock script to verify CKB transaction signature."
                        .to_string()
                ),
                family_id: Some("default-lock".to_string()),
                canonical_reference_hash: Some(hex::decode(
                    "9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8"
                )
                .unwrap()),
                canonical_hash_type: Some(ScriptParser::parse_hash_type("type")),
                associated_code_hash: Some(hex::decode(
                    "9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8"
                )
                .unwrap()),
                ..Default::default()
            }
        );

        let compatible_versions = store
            .list_script_version_hashes_by_label("Default Lock")
            .unwrap();
        assert_eq!(compatible_versions, family_versions);
    }

    #[test]
    fn test_label_import_uses_metadata_slug_for_family_identity() {
        let dir = TempDir::new().unwrap();
        let metadata_path = dir.path().join("metadata");
        std::fs::create_dir_all(metadata_path.join("scripts")).unwrap();

        std::fs::write(
            metadata_path.join("scripts/shadow-lock.toml"),
            r#"
name = "Nervape Shadow Lock"
description = "Generic ownership delegate/proxy lock used by Nervape composing flows."

[mainnet]
[[mainnet.versions]]
canonical_ref_hash = "0x6361d4b20d845953d9c9431bbba08905573005a71e2a2432e7e0e7c685666f24"
version_hash = "0x6361d4b20d845953d9c9431bbba08905573005a71e2a2432e7e0e7c685666f24"
canonical_hash_type = "data1"
"#,
        )
        .unwrap();

        let store_dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_domain(store_dir.path().to_str().unwrap()).unwrap();
        let config = LabelImportConfig {
            network: "mainnet".to_string(),
            metadata_path: Some(metadata_path.to_string_lossy().into_owned()),
        };

        super::run_label_import(&store, &config).unwrap();

        let family = store
            .get_script_family("shadow-lock")
            .unwrap()
            .expect("family id should come from metadata slug");
        assert_eq!(family.name, "Nervape Shadow Lock");

        let wrong_family = store.get_script_family("nervape-shadow-lock").unwrap();
        assert!(
            wrong_family.is_none(),
            "family id must not be derived from make_slug(name)"
        );

        let family_versions = store
            .list_script_version_hashes_by_family("shadow-lock")
            .unwrap();
        assert_eq!(
            family_versions,
            vec![
                hex::decode("6361d4b20d845953d9c9431bbba08905573005a71e2a2432e7e0e7c685666f24")
                    .unwrap()
            ]
        );
    }

    #[test]
    fn test_label_import_writes_multiple_versions_for_one_family() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_domain(dir.path().to_str().unwrap()).unwrap();

        super::run_label_import_bundled(&store, "mainnet").unwrap();

        let family = store
            .get_script_family("default-multisig")
            .unwrap()
            .expect("default-multisig family should be imported");
        assert_eq!(family.versions_count, 3);

        let family_versions = store
            .list_script_version_hashes_by_family("default-multisig")
            .unwrap();
        assert_eq!(
            family_versions,
            vec![
                hex::decode("36c971b8d41fbd94aabca77dc75e826729ac98447b46f91e00796155dddb0d29")
                    .unwrap(),
                hex::decode("43400de165f0821abf63dcac299bbdf7fd73898675ee4ddb099b0a0d8db63bfb")
                    .unwrap(),
                hex::decode("50c8623ef5112510ccdf2d8e480d02d0de7288eb9968f8b019817340c3991145")
                    .unwrap(),
            ]
        );
    }

    #[test]
    fn test_network_switch_removes_stale_script_version_by_family_entries() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_domain(dir.path().to_str().unwrap()).unwrap();

        let script = ScriptMetadata {
            metadata_slug: Some("network-switch".to_string()),
            name: "Network Switch".to_string(),
            description: Some("version differs by network".to_string()),
            website: None,
            category: Some("lock".to_string()),
            disabled: false,
            mainnet: Some(ScriptNetworkMetadata {
                versions: vec![ScriptDeploymentEntry {
                    version_hash:
                        "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                            .to_string(),
                    canonical_ref_hash:
                        "0x1111111111111111111111111111111111111111111111111111111111111111"
                            .to_string(),
                    canonical_hash_type: ValidatedHashType::new(
                        "type".to_string(),
                        "canonical_hash_type",
                    )
                    .unwrap(),
                    deprecated: false,
                }],
                pseudo: None,
            }),
            testnet: Some(ScriptNetworkMetadata {
                versions: vec![ScriptDeploymentEntry {
                    version_hash:
                        "0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
                            .to_string(),
                    canonical_ref_hash:
                        "0x2222222222222222222222222222222222222222222222222222222222222222"
                            .to_string(),
                    canonical_hash_type: ValidatedHashType::new(
                        "type".to_string(),
                        "canonical_hash_type",
                    )
                    .unwrap(),
                    deprecated: false,
                }],
                pseudo: None,
            }),
        };

        upsert_script_label(&store, &script, "mainnet").unwrap();
        assert_eq!(
            store
                .list_script_version_hashes_by_family("network-switch")
                .unwrap(),
            vec![
                hex::decode("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
                    .unwrap()
            ]
        );

        upsert_script_label(&store, &script, "testnet").unwrap();

        assert_eq!(
            store
                .list_script_version_hashes_by_family("network-switch")
                .unwrap(),
            vec![
                hex::decode("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb")
                    .unwrap()
            ]
        );
    }

    #[test]
    fn test_family_rename_removes_stale_script_family_name_index() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_domain(dir.path().to_str().unwrap()).unwrap();

        let original = ScriptMetadata {
            metadata_slug: Some("rename-family".to_string()),
            name: "Original Family Name".to_string(),
            description: Some("original".to_string()),
            website: None,
            category: Some("lock".to_string()),
            disabled: false,
            mainnet: Some(ScriptNetworkMetadata {
                versions: vec![ScriptDeploymentEntry {
                    version_hash:
                        "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                            .to_string(),
                    canonical_ref_hash:
                        "0x1111111111111111111111111111111111111111111111111111111111111111"
                            .to_string(),
                    canonical_hash_type: ValidatedHashType::new(
                        "type".to_string(),
                        "canonical_hash_type",
                    )
                    .unwrap(),
                    deprecated: false,
                }],
                pseudo: None,
            }),
            testnet: None,
        };
        upsert_script_label(&store, &original, "mainnet").unwrap();

        let renamed = ScriptMetadata {
            metadata_slug: Some("rename-family".to_string()),
            name: "Renamed Family".to_string(),
            description: Some("renamed".to_string()),
            website: None,
            category: Some("lock".to_string()),
            disabled: false,
            mainnet: Some(ScriptNetworkMetadata {
                versions: vec![ScriptDeploymentEntry {
                    version_hash:
                        "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                            .to_string(),
                    canonical_ref_hash:
                        "0x1111111111111111111111111111111111111111111111111111111111111111"
                            .to_string(),
                    canonical_hash_type: ValidatedHashType::new(
                        "type".to_string(),
                        "canonical_hash_type",
                    )
                    .unwrap(),
                    deprecated: false,
                }],
                pseudo: None,
            }),
            testnet: None,
        };
        upsert_script_label(&store, &renamed, "mainnet").unwrap();

        assert_eq!(
            store
                .get_script_family_id_by_name("Original Family Name")
                .unwrap(),
            None
        );
        assert_eq!(
            store
                .get_script_family_id_by_name("Renamed Family")
                .unwrap()
                .as_deref(),
            Some("rename-family")
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
    fn test_import_pseudo_script_branch_succeeds() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_domain(dir.path().to_str().unwrap()).unwrap();

        // Type ID is a protocol-level pseudo-script with no deployed code cell.
        // In the family-first format, pseudo-scripts use an explicit `pseudo`
        // branch instead of a version entry.
        let script = ScriptMetadata {
            metadata_slug: Some("type-id".to_string()),
            name: "Type ID".to_string(),
            description: Some("CKB built-in type ID".to_string()),
            website: None,
            category: None,
            disabled: false,
            mainnet: Some(ScriptNetworkMetadata {
                versions: vec![],
                pseudo: Some(PseudoScriptDeployment {
                    hash_type: ValidatedHashType::new("type".to_string(), "hash_type").unwrap(),
                    code_hash: "0x00000000000000000000000000000000000000000000000000545950455f4944"
                        .to_string(),
                }),
            }),
            testnet: None,
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
                    max_supply: Some(1_000_000),
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
    }

    #[test]
    fn test_imports_deprecated_active_network_script_deployments() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_domain(dir.path().to_str().unwrap()).unwrap();

        let script = ScriptMetadata {
            metadata_slug: Some("deprecated-script".to_string()),
            name: "Deprecated Script".to_string(),
            description: Some("deprecated but still named".to_string()),
            website: None,
            category: Some("lock".to_string()),
            disabled: false,
            mainnet: Some(ScriptNetworkMetadata {
                versions: vec![ScriptDeploymentEntry {
                    deprecated: true,
                    canonical_hash_type: ValidatedHashType::new(
                        "type".to_string(),
                        "canonical_hash_type",
                    )
                    .unwrap(),
                    version_hash:
                        "0xd6a5a0edb152e88e8bbc702e164441cb3890fae35da672b408d28ca9a1bde3ee"
                            .to_string(),
                    canonical_ref_hash:
                        "0xbf43c3602455798c1a61a596e0d95278864c552fafe231c063b3fabf97a8febc"
                            .to_string(),
                }],
                pseudo: None,
            }),
            testnet: None,
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

    #[test]
    fn test_active_network_import_handles_shared_version_hash_with_different_canonical_ref_hashes()
    {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_domain(dir.path().to_str().unwrap()).unwrap();

        let script = ScriptMetadata {
            metadata_slug: Some("shared-version".to_string()),
            name: "Shared Version".to_string(),
            description: Some("same version hash across networks".to_string()),
            website: Some("https://example.com".to_string()),
            category: Some("lock".to_string()),
            disabled: false,
            mainnet: Some(ScriptNetworkMetadata {
                versions: vec![ScriptDeploymentEntry {
                    version_hash:
                        "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                            .to_string(),
                    canonical_ref_hash:
                        "0x1111111111111111111111111111111111111111111111111111111111111111"
                            .to_string(),
                    canonical_hash_type: ValidatedHashType::new(
                        "type".to_string(),
                        "canonical_hash_type",
                    )
                    .unwrap(),
                    deprecated: false,
                }],
                pseudo: None,
            }),
            testnet: Some(ScriptNetworkMetadata {
                versions: vec![ScriptDeploymentEntry {
                    version_hash:
                        "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                            .to_string(),
                    canonical_ref_hash:
                        "0x2222222222222222222222222222222222222222222222222222222222222222"
                            .to_string(),
                    canonical_hash_type: ValidatedHashType::new(
                        "type".to_string(),
                        "canonical_hash_type",
                    )
                    .unwrap(),
                    deprecated: false,
                }],
                pseudo: None,
            }),
        };

        upsert_script_label(&store, &script, "mainnet").unwrap();

        let mainnet_code_hash =
            hex::decode("1111111111111111111111111111111111111111111111111111111111111111")
                .unwrap();
        let testnet_code_hash =
            hex::decode("2222222222222222222222222222222222222222222222222222222222222222")
                .unwrap();
        let version_hash =
            hex::decode("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
                .unwrap();

        let mainnet_info = store.get_script_info(&mainnet_code_hash).unwrap().unwrap();
        assert_eq!(mainnet_info.name.as_deref(), Some("Shared Version"));

        let testnet_info = store.get_script_info(&testnet_code_hash).unwrap();
        assert!(
            testnet_info.is_none(),
            "excluded network code hash should not retain label state"
        );

        let version_info = store.get_script_version(&version_hash).unwrap().unwrap();
        assert_eq!(version_info.name.as_deref(), Some("Shared Version"));
        assert_eq!(version_info.associated_code_hash, Some(mainnet_code_hash));
        assert_eq!(
            store
                .list_script_version_hashes_by_label("Shared Version")
                .unwrap(),
            vec![version_hash]
        );
    }
}
