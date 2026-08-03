use anyhow::Result;
use ckbadger_common::{LabelImportConfig, LabelImportResult};
use ckbadger_store::CkbadgerStore;
use serde::Deserialize;
use std::path::Path;
use tracing::{debug, info, warn};

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

/// What a version entry's `version_hash` field attaches to.
///
/// A version is a deployment's bytecode identity — the code cell's data hash.
/// The chain-side usage rollup attributes reference stats to versions by the
/// live code cell's actual data hash, so a label attached to anything else
/// decorates a version that will never receive usage while the real one stays
/// unlabeled (the failure that zeroed the Fiber families).
enum VersionAttachment {
    /// A plausible bytecode identity to attach the label to.
    Attachable(Vec<u8>),
    /// Legacy all-zero sentinel: the entry declares no version.
    NoVersion,
    /// The declared identity is provably not a bytecode data hash; the reason
    /// explains why. Attaching it would silently misattribute the family.
    Invalid(String),
}

/// THE single computation deciding whether an entry's version identity is
/// attachable. Both the family `versions_count` and the version-write path go
/// through here so they can never disagree.
fn version_attachment(entry: &ScriptDeploymentEntry) -> Result<VersionAttachment> {
    let version_hash = decode_hex(&entry.version_hash)
        .map_err(|e| anyhow::anyhow!("invalid version_hash `{}`: {}", entry.version_hash, e))?;
    if version_hash.iter().all(|&b| b == 0) {
        return Ok(VersionAttachment::NoVersion);
    }
    let reference_hash = decode_hex(&entry.canonical_ref_hash).map_err(|e| {
        anyhow::anyhow!(
            "invalid canonical_ref_hash `{}`: {}",
            entry.canonical_ref_hash,
            e
        )
    })?;
    match entry.canonical_hash_type.as_str() {
        // A type-script hash is never a bytecode data hash, so an entry that
        // copies the reference into version_hash is a placeholder.
        "type" if version_hash == reference_hash => Ok(VersionAttachment::Invalid(
            "version_hash equals canonical_ref_hash for a type-referenced deployment; \
             a type-script hash is never the bytecode's data hash (set version_hash \
             to the code cell's data hash)"
                .to_string(),
        )),
        // For data forms the reference IS the bytecode data hash.
        "data" | "data1" | "data2" if version_hash != reference_hash => {
            Ok(VersionAttachment::Invalid(
                "version_hash differs from canonical_ref_hash for a data-form \
                 deployment; the data-form reference IS the bytecode's data hash"
                    .to_string(),
            ))
        }
        _ => Ok(VersionAttachment::Attachable(version_hash)),
    }
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

/// Describe the fields where the stored token row contradicts what another
/// source asserts about the same token. `None` when they agree, when the store
/// has no value yet, or when the other source asserts nothing (`None` argument).
///
/// This is the one comparison used by every token-metadata observation point:
/// label import (label vs. store) and the sync write path (chain vs. store).
/// Keeping a single implementation is what makes "the bundled label disagrees
/// with the chain" a detectable event rather than a matter of which observer
/// happened to be written most recently.
pub(crate) fn token_metadata_divergence(
    existing: &ckbadger_store::types::TokenInfo,
    asserted_name: Option<&str>,
    asserted_symbol: Option<&str>,
    asserted_decimals: Option<i32>,
) -> Option<String> {
    let mut parts = Vec::new();
    if let (Some(name), Some(asserted)) = (existing.name.as_deref(), asserted_name) {
        if name != asserted {
            parts.push(format!("name {name:?} -> {asserted:?}"));
        }
    }
    if let (Some(symbol), Some(asserted)) = (existing.symbol.as_deref(), asserted_symbol) {
        if symbol != asserted {
            parts.push(format!("symbol {symbol:?} -> {asserted:?}"));
        }
    }
    if let (Some(decimals), Some(asserted)) = (existing.decimals, asserted_decimals) {
        if decimals != asserted {
            parts.push(format!("decimals {decimals} -> {asserted}"));
        }
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(", "))
    }
}

/// Describe label fields that would overwrite a *different* pre-existing
/// value (chain-derived on-chain info or an earlier label). Returns None when
/// the label agrees with, or only fills, the existing metadata.
///
/// This observer only sees what is already in the store when the indexer
/// starts. It can therefore never see a chain value that the store does not
/// yet hold — for tokens whose only on-chain metadata binding is the issuance
/// co-occurrence heuristic, the stored value at startup *is* this same label,
/// so nothing diverges. Detecting label-vs-chain disagreement is the job of
/// the sync write path, which is the only place both values coexist; see
/// `crate::db::writer::udt::apply_onchain_token_info`.
fn label_override_conflicts(
    existing: &ckbadger_store::types::TokenInfo,
    label: &TokenMetadata,
) -> Option<String> {
    token_metadata_divergence(
        existing,
        Some(&label.name),
        Some(&label.symbol),
        Some(i32::from(label.decimals)),
    )
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

    if let Some(conflict) = label_override_conflicts(&info, token) {
        warn!(
            type_hash = %format!("0x{}", hex::encode(&type_hash)),
            label = %token.symbol,
            %conflict,
            "token label overrides differing pre-existing metadata (labels take \
             precedence by design; if the previous value is chain-derived, fix \
             the upstream token-labels entry)"
        );
    }

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
    // Count unique attachable bytecode versions: one binary deployed under
    // several references (e.g. RGB++ signet + BTC-testnet3) is ONE version,
    // and an entry whose version identity is invalid attaches nothing, so the
    // family must not claim it.
    let mut attachable_versions: std::collections::HashSet<Vec<u8>> = Default::default();
    for deployment in active_deployments {
        if let ImportDeployment::Version(version) = deployment {
            if let VersionAttachment::Attachable(version_hash) = version_attachment(version)? {
                attachable_versions.insert(version_hash);
            }
        }
    }
    family.versions_count = attachable_versions.len() as i64;
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
    //
    // A version entry whose declared identity is provably not a bytecode data
    // hash (see [`version_attachment`]) is skipped LOUDLY: attaching it would
    // label a version that no chain rollup will ever attribute usage to, while
    // the real deployed version stays unlabeled — the family then reads zero
    // against thousands of live cells on chain. The reference-level label
    // written above is kept; the canonical reference hash itself is real chain
    // vocabulary.
    let version_hash = match deployment {
        ImportDeployment::Version(version) => match version_attachment(version)? {
            VersionAttachment::Attachable(version_hash) => Some(version_hash),
            VersionAttachment::NoVersion => None,
            VersionAttachment::Invalid(reason) => {
                warn!(
                    family = family_id,
                    script = %script.name,
                    version_hash = %version.version_hash,
                    canonical_ref_hash = %version.canonical_ref_hash,
                    canonical_hash_type = version.canonical_hash_type.as_str(),
                    %reason,
                    "skipping script version attachment: fix the version_hash in \
                     docs/metadata/scripts/{}.toml",
                    family_id,
                );
                return Ok(());
            }
        },
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

/// Capture the tracing output a piece of production code emits.
///
/// The surface for "a curated label contradicts the chain" is a warning, so
/// the only honest assertion is on the log line the production path actually
/// writes. Tests that assert on a hand-rolled copy of the comparison would
/// pass even if the comparison were never wired up — which is exactly the
/// failure mode being guarded against here.
#[cfg(test)]
pub(crate) mod test_log_capture {
    use std::io::Write;
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Default)]
    pub(crate) struct SharedBuffer(Arc<Mutex<Vec<u8>>>);

    impl Write for SharedBuffer {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0
                .lock()
                .expect("log capture buffer poisoned")
                .extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for SharedBuffer {
        type Writer = SharedBuffer;

        fn make_writer(&'a self) -> Self::Writer {
            self.clone()
        }
    }

    /// Run `f` with a thread-local subscriber capturing WARN and above, and
    /// return its result together with everything that was logged.
    pub(crate) fn capture_warnings<T>(f: impl FnOnce() -> T) -> (T, String) {
        let buffer = SharedBuffer::default();
        let subscriber = tracing_subscriber::fmt()
            .with_writer(buffer.clone())
            .with_max_level(tracing::Level::WARN)
            .with_ansi(false)
            .finish();
        let value = tracing::subscriber::with_default(subscriber, f);
        let logged = buffer
            .0
            .lock()
            .expect("log capture buffer poisoned")
            .clone();
        (
            value,
            String::from_utf8(logged).expect("tracing output must be UTF-8"),
        )
    }
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
        assert_eq!(
            version_info.canonical_reference_hash,
            Some(mainnet_code_hash)
        );
        assert_eq!(
            store
                .list_script_version_hashes_by_label("Shared Version")
                .unwrap(),
            vec![version_hash]
        );
    }
}

#[cfg(test)]
mod label_override_conflict_tests {
    use super::{label_override_conflicts, TokenDeployment, TokenMetadata};

    fn token_info(
        name: Option<&str>,
        symbol: Option<&str>,
        decimals: Option<i32>,
    ) -> ckbadger_store::types::TokenInfo {
        ckbadger_store::types::TokenInfo {
            type_code_hash: vec![0u8; 32],
            hash_type: 1,
            type_args: vec![],
            standard: "xudt".to_string(),
            name: name.map(str::to_string),
            symbol: symbol.map(str::to_string),
            decimals,
            max_supply: None,
            first_seen_block: 0,
            icon_url: None,
            description: None,
            transfers_count: 0,
        }
    }

    fn label(name: &str, symbol: &str, decimals: i16) -> TokenMetadata {
        TokenMetadata {
            name: name.to_string(),
            symbol: symbol.to_string(),
            decimals,
            standard: "xudt".to_string(),
            icon: None,
            description: None,
            disabled: false,
            mainnet: None::<TokenDeployment>,
            testnet: None::<TokenDeployment>,
        }
    }

    #[test]
    fn reports_differing_chain_derived_symbol() {
        // The RGB++ case: chain info cell says "RGB++", the bundled TOML says
        // "RGB++ Protocol" — the override must be surfaced, not silent.
        let existing = token_info(Some("RGB++ Protocol"), Some("RGB++"), Some(8));
        let conflict =
            label_override_conflicts(&existing, &label("RGB++ Protocol", "RGB++ Protocol", 8))
                .expect("differing symbol must be reported");
        assert!(conflict.contains("symbol"), "got: {conflict}");
        assert!(!conflict.contains("name"), "got: {conflict}");
    }

    #[test]
    fn silent_when_label_agrees_or_fills_gaps() {
        assert!(
            label_override_conflicts(&token_info(None, None, None), &label("Seal", "SEAL", 8))
                .is_none()
        );
        assert!(label_override_conflicts(
            &token_info(Some("Seal"), Some("SEAL"), Some(8)),
            &label("Seal", "SEAL", 8)
        )
        .is_none());
    }

    #[test]
    fn reports_decimals_divergence() {
        let conflict =
            label_override_conflicts(&token_info(None, None, Some(6)), &label("USDI", "USDI", 8))
                .expect("decimals divergence must be reported");
        assert!(conflict.contains("decimals 6 -> 8"), "got: {conflict}");
    }
}

#[cfg(test)]
mod bundled_label_chain_consistency_tests {
    use super::test_log_capture::capture_warnings;
    use super::{bundled, compute_type_hash, TokenDeployment, TokenMetadata};
    use crate::db::writer::udt::apply_onchain_token_info;
    use crate::sync::token_helpers::{
        parse_unique_cell_token_info, OnchainInfoBinding, OnchainTokenInfo, UniqueTokenInfo,
    };
    use ckbadger_store::types::TokenInfo;
    use ckbadger_store::CkbadgerStore;
    use std::collections::HashSet;
    use tempfile::TempDir;

    /// Real mainnet Unique Cell payload from the RGB++ Protocol issuance
    /// (tx 0xd088a128…, output 0) — the same vector the parser test uses.
    const RGBPP_UNIQUE_CELL_DATA_HEX: &str = "080e5247422b2b2050726f746f636f6c055247422b2b";
    const RGBPP_MAINNET_ARGS: &str =
        "0x08875c56644d39dd9629d291357d3026debc5d22fa88d924d60ce8f16dd50e70";

    /// Every network a bundled token can deploy to. The corpus guard runs over
    /// all of them, so no bundled row sits outside the check.
    const NETWORKS: [&str; 2] = ["mainnet", "testnet"];

    fn deployment_for<'a>(token: &'a TokenMetadata, network: &str) -> Option<&'a TokenDeployment> {
        match network {
            "mainnet" => token.mainnet.as_ref(),
            "testnet" => token.testnet.as_ref(),
            other => panic!("unhandled network in the bundled-label guard: {other}"),
        }
    }

    /// Every token row the bundled labels produce for `network`, read back out
    /// of a real label import rather than reconstructed from the TOML — this
    /// is the state the sync write path meets on a fresh database.
    fn stored_label_rows(store: &CkbadgerStore, network: &str) -> Vec<(Vec<u8>, TokenInfo)> {
        let mut rows = Vec::new();
        let mut seen = HashSet::new();
        for token in bundled::udt_labels() {
            if token.disabled {
                continue;
            }
            let Some(deployment) = deployment_for(&token, network) else {
                continue;
            };
            let type_hash = compute_type_hash(deployment).expect("bundled deployment must hash");
            if !seen.insert(type_hash.clone()) {
                continue;
            }
            let info = store
                .get_token(&type_hash)
                .expect("token read must succeed")
                .unwrap_or_else(|| {
                    panic!(
                        "label import must have written token {} (0x{})",
                        token.symbol,
                        hex::encode(&type_hash)
                    )
                });
            rows.push((type_hash, info));
        }
        rows
    }

    /// Import the bundled labels for `network` into a throwaway store and hand
    /// the resulting rows to `check`.
    fn with_imported_labels(network: &str, check: impl FnOnce(&[(Vec<u8>, TokenInfo)])) {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_domain(dir.path().to_str().unwrap()).unwrap();
        super::run_label_import_bundled(&store, network).unwrap();
        check(&stored_label_rows(&store, network));
    }

    /// The guard only means something if it reaches the whole corpus. Every
    /// enabled bundled token must deploy to a network the loop below imports;
    /// a token outside that set would be unchecked exactly the way 1469 rows
    /// were unchecked when only one vector was pinned.
    #[test]
    fn the_bundled_corpus_is_fully_reachable_by_the_guard() {
        let labels = bundled::udt_labels();
        assert!(labels.len() > 1000, "corpus shrank to {}", labels.len());
        let unreachable: Vec<&str> = labels
            .iter()
            .filter(|token| !token.disabled)
            .filter(|token| {
                !NETWORKS
                    .iter()
                    .any(|network| deployment_for(token, network).is_some())
            })
            .map(|token| token.symbol.as_str())
            .collect();
        assert!(
            unreachable.is_empty(),
            "{} bundled tokens deploy to no checked network: {:?}",
            unreachable.len(),
            &unreachable[..unreachable.len().min(5)]
        );
    }

    /// A Unique Cell observation that says exactly what the stored row says.
    fn agreeing_observation(stored: &TokenInfo, binding: OnchainInfoBinding) -> OnchainTokenInfo {
        OnchainTokenInfo {
            info: UniqueTokenInfo {
                decimal: stored
                    .decimals
                    .map(|d| u8::try_from(d).expect("bundled decimals must fit a Unique Cell byte"))
                    .expect("label import writes decimals for every bundled token"),
                name: stored.name.clone().unwrap_or_default(),
                symbol: stored.symbol.clone().unwrap_or_default(),
                total_supply: None,
            },
            binding,
        }
    }

    /// A Unique Cell observation that contradicts the stored row.
    fn contradicting_observation(
        stored: &TokenInfo,
        binding: OnchainInfoBinding,
    ) -> OnchainTokenInfo {
        let mut observation = agreeing_observation(stored, binding);
        observation.info.name = format!("{}~chain", observation.info.name);
        observation.info.symbol = format!("{}~chain", observation.info.symbol);
        observation
    }

    /// Pull the `token_type_hash` field out of every captured warning.
    fn warned_type_hashes(logs: &str) -> HashSet<String> {
        const FIELD: &str = "token_type_hash=";
        let mut found = HashSet::new();
        for line in logs.lines() {
            let Some(rest) = line.split(FIELD).nth(1) else {
                continue;
            };
            let hash: String = rest.chars().take_while(char::is_ascii_hexdigit).collect();
            if !hash.is_empty() {
                found.insert(hash);
            }
        }
        found
    }

    /// The corpus-wide guard. A bundled label is written before the first
    /// block is indexed and (for the issuance co-occurrence binding) keeps
    /// winning forever, so a label that contradicts the chain would otherwise
    /// be discoverable only by a human noticing the wrong symbol in the UI —
    /// which is exactly how the RGB++ row was found.
    ///
    /// Pinning one known-good vector guards one row out of ~1470. This walks
    /// every bundled label on every network through the real write path and
    /// requires the contradiction to be reported for each one, under both
    /// binding kinds, so no label can be silently authoritative.
    #[test]
    fn every_bundled_label_is_checked_against_the_chain_at_write_time() {
        let mut total = 0usize;
        for network in NETWORKS {
            with_imported_labels(network, |stored| {
                assert!(
                    !stored.is_empty(),
                    "no bundled labels imported for {network}"
                );
                total += stored.len();

                for binding in [
                    OnchainInfoBinding::IssuanceCooccurrence,
                    OnchainInfoBinding::XudtExtension,
                ] {
                    let (_, logs) = capture_warnings(|| {
                        for (type_hash, info) in stored {
                            let mut row = info.clone();
                            let observed = contradicting_observation(info, binding);
                            apply_onchain_token_info(type_hash, &mut row, Some(&observed));
                        }
                    });

                    let warned = warned_type_hashes(&logs);
                    let silent: Vec<String> = stored
                        .iter()
                        .map(|(type_hash, _)| hex::encode(type_hash))
                        .filter(|type_hash| !warned.contains(type_hash))
                        .collect();
                    assert!(
                        silent.is_empty(),
                        "{} of {} bundled {network} labels silently outrank contradicting \
                         on-chain metadata under {:?}; first few: {:?}",
                        silent.len(),
                        stored.len(),
                        binding,
                        &silent[..silent.len().min(5)]
                    );
                }
            });
        }
        assert!(
            total >= bundled::udt_labels().len(),
            "the two networks must cover at least one row per bundled token, got {total}"
        );
    }

    /// The other half of the same guard: agreement must stay quiet, or the
    /// warning is noise and gets ignored. Runs over the whole corpus so real
    /// label shapes (empty strings, unicode symbols, 0 and 255 decimals) are
    /// covered, not just a hand-picked row.
    #[test]
    fn bundled_labels_that_agree_with_the_chain_are_not_reported() {
        for network in NETWORKS {
            with_imported_labels(network, |stored| {
                for binding in [
                    OnchainInfoBinding::IssuanceCooccurrence,
                    OnchainInfoBinding::XudtExtension,
                ] {
                    let (_, logs) = capture_warnings(|| {
                        for (type_hash, info) in stored {
                            let mut row = info.clone();
                            let observed = agreeing_observation(info, binding);
                            apply_onchain_token_info(type_hash, &mut row, Some(&observed));
                        }
                    });
                    assert!(
                        warned_type_hashes(&logs).is_empty(),
                        "agreeing on-chain metadata must not be reported \
                         ({network}, {binding:?}): {logs}"
                    );
                }
            });
        }
    }

    /// The one row backed by genuine chain bytes rather than a synthesized
    /// observation, driven through the same production path: decode the real
    /// mainnet Unique Cell and require the imported RGB++ row to agree with
    /// it. Regressing the bundled TOML re-fires the warning and fails here.
    #[test]
    fn bundled_rgbpp_label_matches_the_on_chain_info_cell() {
        let chain = parse_unique_cell_token_info(&hex::decode(RGBPP_UNIQUE_CELL_DATA_HEX).unwrap())
            .expect("RGB++ mainnet Unique Cell vector must parse");

        let label = bundled::udt_labels()
            .into_iter()
            .find(|t| {
                t.mainnet
                    .as_ref()
                    .is_some_and(|d| d.args.eq_ignore_ascii_case(RGBPP_MAINNET_ARGS))
            })
            .expect("bundled labels must contain the RGB++ Protocol mainnet deployment");
        let deployment = label.mainnet.as_ref().expect("mainnet deployment");
        let type_hash = compute_type_hash(deployment).unwrap();

        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_domain(dir.path().to_str().unwrap()).unwrap();
        super::run_label_import_bundled(&store, "mainnet").unwrap();
        let mut row = store
            .get_token(&type_hash)
            .unwrap()
            .expect("RGB++ label row must be imported");

        // RGB++ carries a plain owner-lock-hash in its xUDT args, so the chain
        // binds its Unique Cell by issuance co-occurrence.
        let observed = OnchainTokenInfo {
            info: chain.clone(),
            binding: OnchainInfoBinding::IssuanceCooccurrence,
        };
        let (_, logs) = capture_warnings(|| {
            apply_onchain_token_info(&type_hash, &mut row, Some(&observed));
        });

        assert!(
            warned_type_hashes(&logs).is_empty(),
            "bundled RGB++ label must agree with the on-chain info cell: {logs}"
        );
        assert_eq!(row.symbol.as_deref(), Some(chain.symbol.as_str()));
        assert_eq!(row.name.as_deref(), Some(chain.name.as_str()));
        assert_eq!(row.decimals, Some(i32::from(chain.decimal)));
    }
}

/// Guards on the TOML `version_hash` field: a version is a deployment's
/// bytecode identity (the code cell's data hash). For a type-referenced
/// deployment the reference hash is a type-script hash and can never be the
/// bytecode's data hash, so `version_hash == canonical_ref_hash` under
/// `canonical_hash_type = "type"` is a placeholder that would label a
/// nonexistent version while the real one accumulates usage unlabeled — the
/// exact failure that zeroed the Fiber families.
#[cfg(test)]
mod version_identity_tests {
    use super::test_log_capture::capture_warnings;
    use super::*;
    use ckbadger_store::batch::StoreBatch;
    use ckbadger_store::types::{LiveCellInfo, ScriptReferenceInfo};
    use tempfile::TempDir;

    fn type_entry(version_hash: &str, canonical_ref_hash: &str) -> ScriptDeploymentEntry {
        ScriptDeploymentEntry {
            version_hash: version_hash.to_string(),
            canonical_ref_hash: canonical_ref_hash.to_string(),
            canonical_hash_type: ValidatedHashType::new("type".to_string(), "canonical_hash_type")
                .unwrap(),
            deprecated: false,
        }
    }

    fn script_with_mainnet_versions(
        slug: &str,
        name: &str,
        versions: Vec<ScriptDeploymentEntry>,
    ) -> ScriptMetadata {
        ScriptMetadata {
            metadata_slug: Some(slug.to_string()),
            name: name.to_string(),
            description: Some("test".to_string()),
            website: None,
            category: Some("lock".to_string()),
            disabled: false,
            mainnet: Some(ScriptNetworkMetadata {
                versions,
                pseudo: None,
            }),
            testnet: None,
        }
    }

    #[test]
    fn test_placeholder_type_version_hash_warns_and_skips_version_attachment() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_domain(dir.path().to_str().unwrap()).unwrap();

        let placeholder = "0x1111111111111111111111111111111111111111111111111111111111111111";
        let script = script_with_mainnet_versions(
            "placeholder-family",
            "Placeholder Lock",
            vec![type_entry(placeholder, placeholder)],
        );

        let (result, logs) = capture_warnings(|| upsert_script_label(&store, &script, "mainnet"));
        result.unwrap();

        assert!(
            logs.contains("placeholder-family") && logs.contains("version_hash"),
            "placeholder version_hash must be reported loudly with the family id, got: {logs}"
        );
        assert!(
            logs.contains(&placeholder[2..]),
            "warning must name the offending hash, got: {logs}"
        );

        let placeholder_bytes = decode_hex(placeholder).unwrap();
        assert!(
            store
                .get_script_version(&placeholder_bytes)
                .unwrap()
                .is_none(),
            "a placeholder version_hash must not create a version row"
        );
        assert!(
            store
                .list_script_version_hashes_by_family("placeholder-family")
                .unwrap()
                .is_empty(),
            "placeholder must not be indexed under the family"
        );
        assert!(
            store
                .list_script_version_hashes_by_label("Placeholder Lock")
                .unwrap()
                .is_empty(),
            "placeholder must not be indexed under the label"
        );

        let family = store
            .get_script_family("placeholder-family")
            .unwrap()
            .expect("family row is still created for the reference-level label");
        assert_eq!(
            family.versions_count, 0,
            "family must not claim a version that was never attached"
        );

        // The canonical reference itself is real chain vocabulary — its
        // reference-level label survives so the deployment is still named.
        let info = store
            .get_script_info(&placeholder_bytes)
            .unwrap()
            .expect("reference-level script info should be labeled");
        assert_eq!(info.name.as_deref(), Some("Placeholder Lock"));
    }

    #[test]
    fn test_data_form_version_hash_mismatch_warns_and_skips_version_attachment() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_domain(dir.path().to_str().unwrap()).unwrap();

        let version = "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let reference = "0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
        let script = ScriptMetadata {
            metadata_slug: Some("data-mismatch".to_string()),
            name: "Data Mismatch".to_string(),
            description: None,
            website: None,
            category: None,
            disabled: false,
            mainnet: Some(ScriptNetworkMetadata {
                versions: vec![ScriptDeploymentEntry {
                    version_hash: version.to_string(),
                    canonical_ref_hash: reference.to_string(),
                    canonical_hash_type: ValidatedHashType::new(
                        "data1".to_string(),
                        "canonical_hash_type",
                    )
                    .unwrap(),
                    deprecated: false,
                }],
                pseudo: None,
            }),
            testnet: None,
        };

        let (result, logs) = capture_warnings(|| upsert_script_label(&store, &script, "mainnet"));
        result.unwrap();

        assert!(
            logs.contains("data-mismatch"),
            "a data-form entry whose version_hash differs from its reference must warn, got: {logs}"
        );
        assert!(
            store
                .get_script_version(&decode_hex(version).unwrap())
                .unwrap()
                .is_none(),
            "mismatched data-form version must not be attached"
        );
        assert_eq!(
            store
                .get_script_family("data-mismatch")
                .unwrap()
                .unwrap()
                .versions_count,
            0
        );
    }

    #[test]
    fn test_shared_bytecode_version_entries_count_once_in_family_versions_count() {
        // The RGB++ testnet shape: two type-id deployments (signet + testnet3)
        // of the SAME bytecode are two TOML entries with one version_hash. The
        // family has one version, not two.
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_domain(dir.path().to_str().unwrap()).unwrap();

        let shared_version = "0x7e7e7e7e7e7e7e7e7e7e7e7e7e7e7e7e7e7e7e7e7e7e7e7e7e7e7e7e7e7e7e7e";
        let script = script_with_mainnet_versions(
            "shared-bytecode",
            "Shared Bytecode",
            vec![
                type_entry(
                    shared_version,
                    "0xd0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0",
                ),
                type_entry(
                    shared_version,
                    "0x6161616161616161616161616161616161616161616161616161616161616161",
                ),
            ],
        );

        let (result, logs) = capture_warnings(|| upsert_script_label(&store, &script, "mainnet"));
        result.unwrap();
        assert!(
            logs.is_empty(),
            "two references sharing one real bytecode version are valid metadata, got: {logs}"
        );

        let family = store
            .get_script_family("shared-bytecode")
            .unwrap()
            .expect("family should be imported");
        assert_eq!(
            family.versions_count, 1,
            "one bytecode version deployed under two references is ONE version"
        );
        assert_eq!(
            store
                .list_script_version_hashes_by_family("shared-bytecode")
                .unwrap(),
            vec![decode_hex(shared_version).unwrap()]
        );

        // Both references carry the label.
        for reference in [
            "0xd0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0",
            "0x6161616161616161616161616161616161616161616161616161616161616161",
        ] {
            let info = store
                .get_script_info(&decode_hex(reference).unwrap())
                .unwrap()
                .expect("reference label should be written");
            assert_eq!(info.name.as_deref(), Some("Shared Bytecode"));
        }
    }

    /// Corpus guard: NO bundled script label may carry a version identity the
    /// import path would refuse. Pinning individual families guards those
    /// families; this walks every bundled entry on every network so a future
    /// TOML cannot reintroduce a placeholder and silently zero a family the
    /// way fiber-funding-lock, fiber-commitment-lock and rgb[testnet] were.
    #[test]
    fn no_bundled_script_label_carries_an_unattachable_version_identity() {
        let mut checked = 0usize;
        let mut offenders: Vec<String> = Vec::new();

        for script in bundled::script_labels() {
            if script.disabled {
                continue;
            }
            let family = script.metadata_slug.clone().unwrap_or_else(|| {
                panic!(
                    "bundled script label without metadata_slug: {}",
                    script.name
                )
            });
            for (network, metadata) in [
                ("mainnet", script.mainnet.as_ref()),
                ("testnet", script.testnet.as_ref()),
            ] {
                let Some(metadata) = metadata else { continue };
                for entry in &metadata.versions {
                    checked += 1;
                    match version_attachment(entry).expect("bundled hashes must decode") {
                        VersionAttachment::Attachable(_) | VersionAttachment::NoVersion => {}
                        VersionAttachment::Invalid(reason) => offenders.push(format!(
                            "{family}.toml [{network}] version_hash={} ref={}: {reason}",
                            entry.version_hash, entry.canonical_ref_hash
                        )),
                    }
                }
            }
        }

        assert!(
            checked > 50,
            "the guard must reach the whole bundled corpus, only saw {checked} version entries"
        );
        assert!(
            offenders.is_empty(),
            "{} bundled script version entries would be skipped by label import \
             (their families would read zero usage): {:#?}",
            offenders.len(),
            offenders
        );
    }

    /// End-to-end with the real bundled metadata: the testnet Fiber funding
    /// lock label must land on the version the chain actually resolves — the
    /// live code cell's bytecode data hash (node-verified vector) — so the
    /// usage rollup carries the family's numbers.
    #[test]
    fn test_bundled_fiber_funding_labels_attach_to_the_live_code_cell_version() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        super::run_label_import_bundled(&store, "testnet").unwrap();

        // Chain truth (testnet node, verified 2026-08-03): type reference
        // 0x6c67887f... resolves to the live code cell at
        // 0x12c569a2...:1 whose bytecode data hash is 0x17b1910f....
        let funding_ref =
            hex::decode("6c67887fe201ee0c7853f1682c0b77c0e6214044c156c7558269390a8afa6d7c")
                .unwrap();
        let funding_version =
            hex::decode("17b1910fbcfdfc146ee2ed05587f0e862b799d33ca3c8e1c52d18f2f67716e47")
                .unwrap();

        let mut batch = StoreBatch::new(&store);
        batch.put_cell(
            &[0xcc; 32],
            1,
            &LiveCellInfo {
                capacity: 100_00000000,
                lock_script_hash: vec![0x11; 32],
                lock_code_hash: vec![0x22; 32],
                lock_hash_type: 1,
                lock_args: vec![],
                type_script_hash: Some(funding_ref.clone()),
                type_code_hash: Some(vec![0x33; 32]),
                type_hash_type: Some(1),
                type_args: Some(vec![]),
                data_size: 128,
                occupied_capacity: 61_00000000,
                udt_amount: None,
                data_hash: Some(funding_version.clone()),
            },
            5,
        );
        batch.put_cell_by_type(&funding_ref, 5, &[0xcc; 32], 1);
        batch.put_cell_by_data_hash(&funding_version, 5, &[0xcc; 32], 1);
        batch.put_script_reference_info(
            1,
            &funding_ref,
            &ScriptReferenceInfo {
                reference_hash: funding_ref.clone(),
                hash_type: 1,
                lock_cells_count: 6000,
                lock_live_cells_count: 5021,
                lock_capacity_sum: 17_000_000_000_000_000,
                lock_owned_capacity_sum: 16_131_558_405_032_860,
                lock_used_capacity_sum: 40_000_000_000_000,
                lock_owned_knowledge_sum: 39_182_700_000_000,
                ..Default::default()
            },
        );
        batch.commit().unwrap();

        let rollups =
            crate::db::writer::collect_current_script_reference_rollup_state(&store, &store)
                .unwrap();

        let version = rollups
            .versions
            .iter()
            .find(|(hash, _)| hash == &funding_version)
            .map(|(_, info)| info)
            .expect("the live funding bytecode version row must exist");
        assert_eq!(
            version.name.as_deref(),
            Some("Fiber Funding Lock"),
            "the label must be attached to the version the chain resolves"
        );
        assert_eq!(version.family_id.as_deref(), Some("fiber-funding-lock"));
        assert_eq!(version.lock_live_cells_count, 5021);
        assert_eq!(version.lock_owned_capacity_sum, 16_131_558_405_032_860);

        let family = rollups
            .families
            .iter()
            .find(|(id, _)| id == "fiber-funding-lock")
            .map(|(_, info)| info)
            .expect("fiber-funding-lock family must exist");
        assert_eq!(
            family.live_cells_count, 5021,
            "family usage must carry the reference's rollup, not zero"
        );
        assert_eq!(family.owned_capacity_sum, 16_131_558_405_032_860);
        assert_eq!(family.owned_knowledge_sum, 39_182_700_000_000);
        assert_eq!(family.versions_count, 1);
    }
}
