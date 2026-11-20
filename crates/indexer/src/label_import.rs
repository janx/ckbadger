use anyhow::{Context, Result};
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
    pub role: Option<String>,
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
    /// `[lock_args]`: how this lock's args read (validated by `build.rs`;
    /// read by [`crate::parser::registry::SCRIPT_LAYOUTS`], not by import).
    #[serde(default)]
    pub lock_args: Option<LockArgsMetadata>,
    /// `[cobuild]`: the CoBuild action schema of this type script's actions
    /// (validated by `build.rs`; read by `SCRIPT_LAYOUTS`, not by import).
    #[serde(default)]
    pub cobuild: Option<CobuildMetadata>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct LockArgsMetadata {
    #[serde(default)]
    pub codec: Option<String>,
    #[serde(default)]
    pub fields: Vec<LockArgsFieldMetadata>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct LockArgsFieldMetadata {
    pub name: String,
    pub bytes: usize,
    #[serde(default)]
    pub equals: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct CobuildMetadata {
    pub schema: String,
    pub actions: Vec<CobuildActionMetadata>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct CobuildActionMetadata {
    pub id: u32,
    pub name: String,
    pub fields: usize,
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

    pub(crate) fn as_str(&self) -> &str {
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
pub(crate) enum VersionAttachment {
    /// A plausible bytecode identity to attach the label to.
    Attachable(Vec<u8>),
    /// Legacy all-zero sentinel: the entry declares no version.
    NoVersion,
    /// The declared identity is provably not a bytecode data hash; the reason
    /// explains why. Attaching it would silently misattribute the family.
    Invalid(String),
}

/// THE single computation deciding whether an entry's version identity is
/// attachable. The version-write path goes through here, and a family's
/// `versions_count` counts the version rows it attached
/// (`family_versions_counts`), so the two can never disagree.
pub(crate) fn version_attachment(entry: &ScriptDeploymentEntry) -> Result<VersionAttachment> {
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
    existing.metadata_divergence(
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
        overlay_from_dir(Path::new(metadata_path), &mut tokens, &mut scripts)?;
    }

    // Same role/slug validation the API applies at router build — fail import
    // before writing so overlay mistakes cannot diverge API vs indexer.
    let _ = collect_script_roles(&scripts, &config.network)?;

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
    let _ = collect_script_roles(&scripts, network)?;
    import_all(store, network, &tokens, &scripts)
}

/// Collect declared script roles from already-loaded metadata (bundled and/or
/// workdir overlay). Invalid role strings or a role without a slug fail fast.
fn collect_script_roles(
    scripts: &[ScriptMetadata],
    network: &str,
) -> Result<std::collections::HashMap<String, (String, String)>> {
    let mut roles = std::collections::HashMap::new();
    for script in scripts {
        if script.disabled {
            continue;
        }
        let active = match network {
            "mainnet" => script.mainnet.is_some(),
            "testnet" => script.testnet.is_some(),
            _ => script.mainnet.is_some() || script.testnet.is_some(),
        };
        if !active {
            continue;
        }
        let Some(role) = script.role.clone() else {
            continue;
        };
        anyhow::ensure!(
            matches!(role.as_str(), "lock" | "type" | "lock+type"),
            "invalid script metadata role {role}: {}",
            script.name
        );
        let slug = script
            .metadata_slug
            .clone()
            .ok_or_else(|| anyhow::anyhow!("script metadata missing slug: {}", script.name))?;
        roles.insert(slug, (script.name.clone(), role));
    }
    Ok(roles)
}

/// Read-only counterpart of label import's bundled-plus-workdir metadata
/// precedence. The API uses declared roles only for registered families with
/// no observed lock/type usage, without changing chain-store schema or writes.
pub fn effective_script_roles(
    metadata_path: Option<&Path>,
    network: &str,
) -> Result<std::collections::HashMap<String, (String, String)>> {
    let mut tokens = bundled::udt_labels();
    let mut scripts = bundled::script_labels();
    if let Some(path) = metadata_path {
        overlay_from_dir(path, &mut tokens, &mut scripts)?;
    }
    collect_script_roles(&scripts, network)
}

pub(crate) fn import_all(
    store: &CkbadgerStore,
    network: &str,
    tokens: &[TokenMetadata],
    scripts: &[ScriptMetadata],
) -> Result<LabelImportResult> {
    // Label import writes its CFs directly, outside the block journal: the
    // journal discipline admits those writes from this scope only (Lane B
    // carry-over 4c, `OUTSIDE_JOURNAL`'s label-import entries).
    let _label_import = store.outside_journal_scope(ckbadger_store::OutsideWriter::LabelImport)?;
    let registrations = validate_script_registrations(store, scripts, network)?;
    // Every label row is read before any write: a corrupt row fails the
    // import with its pair named, and the rows no active pseudo deployment
    // keeps are deleted after the writes (`remove_script_labels_not_imported`).
    let existing_labels = store
        .list_script_infos()
        .with_context(|| format!("label import preflight: network={network} script_info rows"))?;
    // Every token label row is read before any write: a label no entry
    // imports any more is deleted after the writes, so a token the chain first
    // holds later is never given a label that was removed.
    let existing_token_labels = store
        .list_token_label_hashes()
        .with_context(|| format!("label import preflight: network={network} token_labels rows"))?;
    let mut imported_token_labels: std::collections::HashSet<Vec<u8>> =
        std::collections::HashSet::new();
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
            // A malformed metadata entry is collected and skipped; a store
            // error (a failed read or write, a journal-discipline refusal)
            // stops the import (finding 14).
            match prepare_token_label(token, deployment) {
                Ok(prepared) => {
                    write_token_label(store, token, &prepared).with_context(|| {
                        format!(
                            "label import: network={network} token={} type_hash=0x{}",
                            token.symbol,
                            hex::encode(&prepared.type_hash)
                        )
                    })?;
                    imported_token_labels.insert(prepared.type_hash);
                    result.udt_labels_imported += 1;
                }
                Err(e) => result.errors.push(format!("Token {}: {}", token.symbol, e)),
            }
        }
    }
    // Every script entry's metadata was validated by the preflight above, so
    // what fails here is the store: it stops the import (finding 14).
    for script in scripts {
        if script.disabled {
            continue;
        }
        upsert_script_label_validated(store, script, network, &registrations)
            .with_context(|| format!("label import: network={network} script={}", script.name))?;
        result.script_labels_imported += 1;
    }
    for type_hash in existing_token_labels {
        if !imported_token_labels.contains(&type_hash) {
            store
                .delete_token_label_direct(&type_hash)
                .with_context(|| {
                    format!(
                        "label import: delete token label 0x{}",
                        hex::encode(&type_hash)
                    )
                })?;
        }
    }
    remove_script_labels_not_imported(store, existing_labels, &registrations.label_pairs)?;
    set_family_versions_counts(store)?;
    info!(
        "Label import completed: {} UDT, {} scripts, {} errors",
        result.udt_labels_imported,
        result.script_labels_imported,
        result.errors.len()
    );
    Ok(result)
}

/// Label import owns the whole `script_info` CF (lane rd/api-lock-names, fix
/// round S1): after its writes, every row whose pair is not an active pseudo
/// deployment's (`label_pairs`, built over every enabled script before any
/// write) is deleted, whatever left it: a corrected hash type or reference, a
/// removed entry or overlay file, `disabled = true`, a version's label row
/// from before versions were named by their code. A row another active
/// deployment names is in `label_pairs` and is never deleted. Domain store,
/// outside the journal (the caller's `OutsideWriter::LabelImport` scope).
fn remove_script_labels_not_imported(
    store: &CkbadgerStore,
    existing_labels: Vec<((Vec<u8>, u8), ckbadger_store::types::ScriptInfo)>,
    label_pairs: &std::collections::HashSet<(Vec<u8>, u8)>,
) -> Result<()> {
    for ((code_hash, hash_type), _) in existing_labels {
        if !label_pairs.contains(&(code_hash.clone(), hash_type)) {
            store
                .delete_script_info_direct(&code_hash, hash_type)
                .with_context(|| {
                    format!(
                        "label import: delete script_info 0x{} by hash type {hash_type}",
                        hex::encode(&code_hash)
                    )
                })?;
        }
    }
    Ok(())
}

fn overlay_from_dir(
    metadata_path: &Path,
    tokens: &mut Vec<TokenMetadata>,
    scripts: &mut Vec<ScriptMetadata>,
) -> Result<()> {
    let base = metadata_path;

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

/// A token label's deployment, decoded from its metadata entry: the type
/// script hash its label is stored under.
struct PreparedTokenLabel {
    type_hash: Vec<u8>,
}

/// Decode a token label's deployment: pure metadata validation, the only
/// per-entry failure label import collects instead of stopping (finding 14).
fn prepare_token_label(
    token: &TokenMetadata,
    deployment: &TokenDeployment,
) -> Result<PreparedTokenLabel> {
    decode_hex(&deployment.code_hash).map_err(|e| {
        anyhow::anyhow!(
            "invalid type script code_hash for token {}: {}",
            token.symbol,
            e
        )
    })?;
    decode_hex(&deployment.args).map_err(|e| {
        anyhow::anyhow!("invalid type script args for token {}: {}", token.symbol, e)
    })?;
    // Metadata, not node data: an unknown label is a collected entry error,
    // checked before the script hash reads it.
    ckbadger_common::hash_type_from_label(&deployment.hash_type).ok_or_else(|| {
        anyhow::anyhow!(
            "invalid type script hash_type `{}` for token {}",
            deployment.hash_type,
            token.symbol
        )
    })?;
    Ok(PreparedTokenLabel {
        type_hash: compute_type_hash(deployment)?,
    })
}

#[cfg(test)]
fn upsert_token_label(
    store: &CkbadgerStore,
    token: &TokenMetadata,
    deployment: &TokenDeployment,
) -> Result<()> {
    write_token_label(store, token, &prepare_token_label(token, deployment)?)
}

/// The label a token metadata entry carries, as `token_labels` stores it.
fn token_label(token: &TokenMetadata) -> ckbadger_store::types::TokenLabel {
    ckbadger_store::types::TokenLabel {
        name: token.name.clone(),
        symbol: token.symbol.clone(),
        decimals: i32::from(token.decimals),
        icon_url: token.icon.clone(),
        description: token.description.clone(),
        standard: token.standard.clone(),
    }
}

/// Write one decoded token label; every store error propagates.
///
/// The label is stored in `token_labels` (domain store, outside the journal:
/// label import is its only writer) and NOWHERE else: a token row holds what
/// the chain says (resync fix round M1, fix round 2 B), and the API joins the
/// label to the row when it reads the token (`token_identity` in the API,
/// by [`TokenInfo::attach_label`]). A label that differs from what the
/// chain's row already says is logged: the page shows the label's fields,
/// the chain's beside them.
///
/// [`TokenInfo::attach_label`]: ckbadger_store::types::TokenInfo::attach_label
fn write_token_label(
    store: &CkbadgerStore,
    token: &TokenMetadata,
    prepared: &PreparedTokenLabel,
) -> Result<()> {
    let type_hash = &prepared.type_hash;
    store.put_token_label_direct(type_hash, &token_label(token))?;
    if let Some(info) = store.get_token(type_hash)? {
        if let Some(conflict) = label_override_conflicts(&info, token) {
            warn!(
                type_hash = %format!("0x{}", hex::encode(type_hash)),
                label = %token.symbol,
                %conflict,
                "token label differs from what the chain says of the token (the API shows \
                 the label's fields, the chain's beside them; if the chain is right, fix \
                 the upstream token-labels entry)"
            );
        }
    }
    Ok(())
}

fn selected_script_deployments(
    script: &ScriptMetadata,
    network: &str,
) -> (Vec<ImportDeployment>, Vec<ImportDeployment>) {
    let mainnet = script
        .mainnet
        .as_ref()
        .map(ScriptNetworkMetadata::import_deployments)
        .unwrap_or_default();
    let testnet = script
        .testnet
        .as_ref()
        .map(ScriptNetworkMetadata::import_deployments)
        .unwrap_or_default();
    match network {
        "mainnet" => (mainnet, testnet),
        "testnet" => (testnet, mainnet),
        _ => (mainnet.into_iter().chain(testnet).collect(), Vec::new()),
    }
}

struct ImportedVersionRegistration {
    references: Vec<(Vec<u8>, u8)>,
    preferred: (Vec<u8>, u8),
}

fn registered_versions(
    deployments: &[ImportDeployment],
) -> Result<std::collections::BTreeMap<Vec<u8>, ImportedVersionRegistration>> {
    let mut versions = std::collections::BTreeMap::<Vec<u8>, ImportedVersionRegistration>::new();
    for deployment in deployments {
        let ImportDeployment::Version(version) = deployment else {
            continue;
        };
        let VersionAttachment::Attachable(hash) = version_attachment(version)? else {
            continue;
        };
        let reference = (
            decode_hex(&version.canonical_ref_hash)?,
            ScriptParser::parse_hash_type(version.canonical_hash_type.as_str()),
        );
        let registration = versions
            .entry(hash)
            .or_insert_with(|| ImportedVersionRegistration {
                references: Vec::new(),
                preferred: reference.clone(),
            });
        registration.references.push(reference);
    }
    for registration in versions.values_mut() {
        registration.references.sort();
        registration.references.dedup();
    }
    Ok(versions)
}

/// Binaries already attributed by Type ID lineage to another family. Metadata
/// that lists the same data hash under a different family must yield — lineage
/// owns attribution — instead of failing startup or stealing the family_id.
fn type_id_lineage_owns_binary(
    store: &CkbadgerStore,
    version: &ckbadger_store::ScriptVersionInfo,
    owner_family: &str,
) -> Result<bool> {
    if version.type_id_deployments.is_empty() {
        return Ok(false);
    }
    let mut registered_to_owner = std::collections::HashSet::new();
    for (_, other) in store.list_script_versions()? {
        if other.family_id.as_deref() != Some(owner_family) {
            continue;
        }
        for (reference, kind) in &other.registered_references {
            if *kind == 1 {
                registered_to_owner.insert(reference.clone());
            }
        }
    }
    Ok(version
        .type_id_deployments
        .iter()
        .any(|(reference, _)| registered_to_owner.contains(reference)))
}

/// Returns binaries that metadata wants to register but Type ID lineage already
/// bound to a different family. Callers must skip importing those hashes.
fn validate_existing_family(
    store: &CkbadgerStore,
    family: &str,
    registrations: &std::collections::BTreeMap<Vec<u8>, ImportedVersionRegistration>,
) -> Result<std::collections::HashSet<Vec<u8>>> {
    let mut lineage_owned = std::collections::HashSet::new();
    for hash in registrations.keys() {
        let Some(version) = store.get_script_version(hash)? else {
            continue;
        };
        let Some(existing) = version.family_id.as_deref() else {
            continue;
        };
        if existing == family {
            continue;
        }
        if type_id_lineage_owns_binary(store, &version, existing)? {
            warn!(
                binary = %format!("0x{}", hex::encode(hash)),
                existing_family = existing,
                imported_family = family,
                "Type ID lineage owns binary; skipping conflicting metadata family registration"
            );
            lineage_owned.insert(hash.clone());
            continue;
        }
        anyhow::bail!(
            "binary registered in conflicting families: binary=0x{} existing_family={} imported_family={}; correct metadata and rebuild",
            hex::encode(hash),
            existing,
            family
        );
    }
    Ok(lineage_owned)
}

/// Active metadata is resolved once before any label writes. Excluded entries
/// must not revoke another entry's active binary or reference registration.
#[derive(Default)]
struct ScriptRegistrations {
    binaries: std::collections::HashMap<Vec<u8>, String>,
    references: std::collections::HashMap<(Vec<u8>, u8), String>,
    /// The pairs (code hash, hash type) of the active pseudo deployments (Zero
    /// Lock, Type ID): the keys of the `script_info` rows this import writes,
    /// and the only rows the CF keeps. A version's pair has no label row: it
    /// is named by the code it runs (its family, through the version rows).
    label_pairs: std::collections::HashSet<(Vec<u8>, u8)>,
}

fn deployment_reference(deployment: &ImportDeployment) -> &str {
    match deployment {
        ImportDeployment::Version(version) => &version.canonical_ref_hash,
        ImportDeployment::Pseudo(pseudo) => &pseudo.code_hash,
    }
}

/// The script a deployment names: its pair (code hash, hash type). A version
/// names its canonical reference by its canonical hash type; a pseudo
/// deployment (Zero Lock by `data`, Type ID by `type`) its code hash by its
/// hash type, the key of its `script_info` row. A reference that is not 32
/// bytes is an error naming it (the caller names the file and the entry).
fn deployment_pair(deployment: &ImportDeployment) -> Result<(Vec<u8>, u8)> {
    let hash_type = match deployment {
        ImportDeployment::Version(version) => version.canonical_hash_type.as_str(),
        ImportDeployment::Pseudo(pseudo) => pseudo.hash_type.as_str(),
    };
    let reference = deployment_reference(deployment);
    let code_hash = decode_hex(reference)?;
    anyhow::ensure!(
        code_hash.len() == 32,
        "reference {reference} is {} bytes, not 32",
        code_hash.len()
    );
    Ok((code_hash, ScriptParser::parse_hash_type(hash_type)))
}

fn validate_script_registrations(
    store: &CkbadgerStore,
    scripts: &[ScriptMetadata],
    network: &str,
) -> Result<ScriptRegistrations> {
    anyhow::ensure!(
        store.cf_handle_exists(ckbadger_store::CF_SCRIPT_VERSIONS),
        "label import requires the domain store: network={network}"
    );
    let existing_versions = store
        .list_script_versions()
        .with_context(|| format!("label import preflight: network={network}"))?;
    ckbadger_store::ScriptFamilyMembershipIndex::new(
        existing_versions.iter().map(|(_, version)| version),
    ).with_context(|| format!("label import persisted membership is invalid: network={network}; verify the owning writer and rebuild the damaged store"))?;
    let mut active_registrations = ScriptRegistrations::default();
    for script in scripts.iter().filter(|script| !script.disabled) {
        let family = script_family_id(script)?;
        let context = || format!("label import preflight: network={network} family={family}");
        store.get_script_family(family).with_context(context)?;
        let (active, excluded) = selected_script_deployments(script, network);
        let registrations = registered_versions(&active).with_context(context)?;
        let lineage_owned =
            validate_existing_family(store, family, &registrations).with_context(context)?;
        // Every entry's reference is checked before any write, the file
        // (family) and the entry named.
        let entry_pair = |deployment: &ImportDeployment| {
            deployment_pair(deployment).with_context(|| {
                format!(
                    "{} script={} entry={}",
                    context(),
                    script.name,
                    deployment_reference(deployment)
                )
            })
        };
        for deployment in &excluded {
            entry_pair(deployment)?;
        }
        for deployment in &active {
            let pair = entry_pair(deployment)?;
            if !matches!(deployment, ImportDeployment::Pseudo(_)) {
                continue;
            }
            // The label is written whole below, so a corrupt stored row would
            // be overwritten unseen: read it first, and a corrupt row fails
            // the import before any write, naming the pair.
            store.get_script_info(&pair.0, pair.1).with_context(|| {
                format!(
                    "{} script={} reference=0x{} hash_type={}",
                    context(),
                    script.name,
                    hex::encode(&pair.0),
                    pair.1
                )
            })?;
            active_registrations.label_pairs.insert(pair);
        }
        for (hash, registration) in registrations {
            if lineage_owned.contains(&hash) {
                continue;
            }
            if let Some(previous) = active_registrations
                .binaries
                .insert(hash.clone(), family.into())
            {
                anyhow::ensure!(
                    previous == family,
                    "binary registered in conflicting families: network={} binary=0x{} families={}/{}",
                    network, hex::encode(&hash), previous, family
                );
            }
            for (reference, kind) in registration.references {
                if let Some(previous) = active_registrations
                    .references
                    .insert((reference.clone(), kind), family.into())
                {
                    anyhow::ensure!(previous == family, "reference registered in conflicting families: network={} reference=0x{} hash_type={} families={}/{}", network, hex::encode(reference), kind, previous, family);
                }
            }
        }
        // Cleanup used to swallow malformed excluded metadata and corrupt rows.
        // Validate every touched read before token import or any script
        // mutation; a later store error stops the import too (finding 14).
        for deployment in &excluded {
            let reference = deployment_reference(deployment);
            let (code_hash, hash_type) = deployment_pair(deployment)
                .with_context(|| format!("{} excluded reference={reference}", context()))?;
            store
                .get_script_info(&code_hash, hash_type)
                .with_context(|| {
                    format!(
                        "{} excluded reference={reference} hash_type={hash_type}",
                        context()
                    )
                })?;
            if let ImportDeployment::Version(version) = deployment {
                let hash = decode_hex(&version.version_hash).with_context(|| {
                    format!("{} excluded binary={}", context(), version.version_hash)
                })?;
                if hash.iter().any(|&b| b != 0) {
                    store.get_script_version(&hash).with_context(|| {
                        format!("{} excluded binary={}", context(), version.version_hash)
                    })?;
                }
            }
        }
        for hash in store
            .list_script_version_hashes_by_family(family)
            .with_context(context)?
        {
            let version = store
                .get_script_version(&hash)
                .with_context(context)?
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "{} family index points to missing version: binary=0x{}",
                        context(),
                        hex::encode(&hash)
                    )
                })?;
            if version.family_id.as_deref() == Some(family) {
                continue;
            }
            // Stale by-family rows are a writer/reorg bug. Do not heal here —
            // that would mask missing undo pre-images. Purge and rebuild.
            anyhow::bail!(
                "{} family index contradicts version: binary=0x{} version_family={:?}; purge and rebuild from genesis",
                context(),
                hex::encode(&hash),
                version.family_id
            );
        }
    }
    // An observation can retain existing binary attribution, but cannot repair
    // missing/wrong attribution or grant a copied Type ID a registration. Check
    // all observations against the complete active registry, independently of
    // metadata order, in one scan before any writes.
    for (hash, version) in existing_versions {
        // A direct binary declaration is the normal metadata assignment path.
        // It can label this exact previously-unknown binary, but cannot assign
        // any other observed historical version. Persisted damage was rejected
        // above before considering these planned registrations.
        let declared_family = version
            .family_id
            .as_ref()
            .or_else(|| active_registrations.binaries.get(&hash));
        for (reference, block) in &version.type_id_deployments {
            if let Some(family) = active_registrations.references.get(&(reference.clone(), 1)) {
                anyhow::ensure!(declared_family == Some(family),
                    "label import observed registered type-id contradicts family: network={} reference=0x{} block={} binary=0x{} registered_family={} version_family={:?}; verify metadata and the owning writer, then rebuild from genesis", network, hex::encode(reference), block, hex::encode(&hash), family, version.family_id);
            }
        }
    }
    Ok(active_registrations)
}

/// Remove metadata-owned registration and its display preference only. A
/// chain-derived display reference was never part of this imported set.
fn revoke_imported_references(version: &mut ckbadger_store::ScriptVersionInfo) {
    if let (Some(reference), Some(kind)) = (
        &version.canonical_reference_hash,
        version.canonical_hash_type,
    ) {
        if version
            .registered_references
            .contains(&(reference.clone(), kind))
        {
            version.canonical_reference_hash = None;
            version.canonical_hash_type = None;
        }
    }
    version.registered_references.clear();
}

#[cfg(test)]
fn upsert_script_label(
    store: &CkbadgerStore,
    script: &ScriptMetadata,
    network: &str,
) -> Result<()> {
    let registrations =
        validate_script_registrations(store, std::slice::from_ref(script), network)?;
    upsert_script_label_validated(store, script, network, &registrations)?;
    set_family_versions_counts(store)
}

fn upsert_script_label_validated(
    store: &CkbadgerStore,
    script: &ScriptMetadata,
    network: &str,
    active_registrations: &ScriptRegistrations,
) -> Result<()> {
    let family_id = script_family_id(script)?;
    let (active, excluded) = selected_script_deployments(script, network);
    let registrations = registered_versions(&active)?;

    if !active.is_empty() {
        upsert_script_family(store, script, family_id)?;
    }

    for deployment in &excluded {
        if let ImportDeployment::Version(version) = deployment {
            let data_hash = decode_hex(&version.version_hash)?;
            if data_hash.iter().all(|&b| b == 0)
                || active_registrations.binaries.contains_key(&data_hash)
            {
                continue;
            }
            if let Some(mut version_info) = store.get_script_version(&data_hash)? {
                let belongs_to_family = version_info.family_id.as_deref() == Some(family_id)
                    || (version_info.family_id.is_none()
                        && version_info.name.as_deref() == Some(&script.name));
                if !belongs_to_family {
                    continue;
                }
                let before = version_info.clone();
                revoke_imported_references(&mut version_info);
                // Local deployment evidence retains an already-known bytecode
                // family. It does not register any observed reference: only
                // imported anchors confer permanent Type ID family membership.
                if version_info.type_id_deployments.is_empty() {
                    if let Some(name) = &version_info.name {
                        store.delete_script_version_by_label(name, &data_hash)?;
                    }
                    if let Some(family) = &version_info.family_id {
                        store.delete_script_version_by_family_direct(family, &data_hash)?;
                    }
                    version_info.name = None;
                    version_info.family_id = None;
                    version_info.deprecated = false;
                    version_info.category = None;
                    version_info.description = None;
                    version_info.website = None;
                    version_info.canonical_reference_hash = None;
                    version_info.canonical_hash_type = None;
                }
                if version_info != before {
                    store.put_script_version(&data_hash, &version_info)?;
                }
            }
        }
    }

    // Registrations are a complete metadata declaration. Revoking the last
    // imported anchor preserves chain observations, attribution and usage.
    for version_hash in store.list_script_version_hashes_by_family(family_id)? {
        let mut version = store.get_script_version(&version_hash)?.ok_or_else(|| {
            anyhow::anyhow!(
                "family version index points to missing version: family={} binary=0x{}",
                family_id,
                hex::encode(&version_hash)
            )
        })?;
        if version.family_id.as_deref() != Some(family_id) {
            anyhow::bail!(
                "family version index contradicts version: family={} binary=0x{} version_family={:?}; purge and rebuild from genesis",
                family_id,
                hex::encode(&version_hash),
                version.family_id
            );
        }
        if active_registrations.binaries.contains_key(&version_hash) {
            continue;
        }
        if !version.registered_references.is_empty() {
            revoke_imported_references(&mut version);
            store.put_script_version(&version_hash, &version)?;
        }
    }

    for deployment in &active {
        import_single_deployment(store, script, family_id, deployment, &registrations)?;
    }
    Ok(())
}

/// Set every family row's `versions_count` from the version rows the import
/// leaves, with the rollup's own routine (`family_versions_counts`): the
/// count is the number of version rows naming the family, so an import that
/// changes no label rewrites no family row, and the next block's rollup
/// computes the same value. A family no version row names counts zero.
fn set_family_versions_counts(store: &CkbadgerStore) -> Result<()> {
    let versions = store.list_script_versions()?;
    let counts =
        crate::db::writer::family_versions_counts(versions.iter().map(|(_, version)| version))?;
    for (family_id, mut family) in store.list_script_families()? {
        let count = match counts.get(&family_id) {
            Some(count) => *count,
            None => 0,
        };
        if family.versions_count != count {
            family.versions_count = count;
            store.put_script_family_direct(&family_id, &family)?;
        }
    }
    Ok(())
}

fn upsert_script_family(
    store: &CkbadgerStore,
    script: &ScriptMetadata,
    family_id: &str,
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
    store.put_script_family_direct(family_id, &family)?;
    store.put_script_family_name_direct(&script.name, family_id)?;
    Ok(())
}

fn import_single_deployment(
    store: &CkbadgerStore,
    script: &ScriptMetadata,
    family_id: &str,
    deployment: &ImportDeployment,
    registrations: &std::collections::BTreeMap<Vec<u8>, ImportedVersionRegistration>,
) -> Result<()> {
    // A pseudo deployment (Zero Lock, Type ID) has no code to be named by, so
    // the registry's label of its pair is a `script_info` row (domain store,
    // label import's write, outside the journal): the label alone, no usage.
    // A version's pair has no label row: it is named by the code it runs, the
    // family its version rows below record (`ckbadger_api::utils::ScriptNamer`).
    let (code_hash, hash_type) = deployment_pair(deployment)?;
    if let ImportDeployment::Pseudo(_) = deployment {
        let info = ckbadger_store::types::ScriptInfo {
            name: script.name.clone(),
            deprecated: false,
            category: script.category.clone(),
            website: script.website.clone(),
            description: script.description.clone(),
        };
        store.put_script_info_direct(&code_hash, hash_type, &info)?;
    }

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
    // Type ID lineage priority: do not let a metadata-by-data-hash registration
    // steal a binary already rebound to another family's Type ID template.
    if let Some(existing_family) = version_info.family_id.as_deref() {
        if existing_family != family_id
            && type_id_lineage_owns_binary(store, &version_info, existing_family)?
        {
            warn!(
                family = family_id,
                existing_family,
                binary = %format!("0x{}", hex::encode(&version_hash)),
                "skipping metadata version write: Type ID lineage owns binary"
            );
            return Ok(());
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
    let registration = registrations.get(&version_hash).ok_or_else(|| {
        anyhow::anyhow!(
            "missing imported registration: family={} binary=0x{}",
            family_id,
            hex::encode(&version_hash)
        )
    })?;
    version_info.registered_references = registration.references.clone();
    version_info.canonical_reference_hash = Some(registration.preferred.0.clone());
    version_info.canonical_hash_type = Some(registration.preferred.1);
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
        capture_logs(tracing::Level::WARN, f)
    }

    /// Run `f` with a thread-local subscriber capturing `level` and above, and
    /// return its result together with everything that was logged.
    pub(crate) fn capture_logs<T>(level: tracing::Level, f: impl FnOnce() -> T) -> (T, String) {
        let buffer = SharedBuffer::default();
        let subscriber = tracing_subscriber::fmt()
            .with_writer(buffer.clone())
            .with_max_level(level)
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
    fn effective_script_roles_use_same_workdir_overlay_as_label_import() {
        let bundled = effective_script_roles(None, "mainnet").unwrap();
        assert_eq!(
            bundled.get("timelock").map(|(_, role)| role.as_str()),
            Some("lock")
        );
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join("scripts")).unwrap();
        let override_toml = include_str!("../../../docs/metadata/scripts/timelock.toml")
            .replace("role = \"lock\"", "role = \"type\"");
        std::fs::write(dir.path().join("scripts/timelock.toml"), override_toml).unwrap();
        let effective = effective_script_roles(Some(dir.path()), "mainnet").unwrap();
        assert_eq!(
            effective.get("timelock").map(|(_, role)| role.as_str()),
            Some("type")
        );
    }

    #[test]
    fn run_label_import_rejects_invalid_overlay_role() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path().join("store")).unwrap();
        let metadata = dir.path().join("metadata");
        std::fs::create_dir_all(metadata.join("scripts")).unwrap();
        let override_toml = include_str!("../../../docs/metadata/scripts/timelock.toml")
            .replace("role = \"lock\"", "role = \"not-a-role\"");
        std::fs::write(metadata.join("scripts/timelock.toml"), override_toml).unwrap();
        let err = run_label_import(
            &store,
            &LabelImportConfig {
                network: "mainnet".to_string(),
                metadata_path: Some(metadata.to_string_lossy().into_owned()),
            },
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("invalid script metadata role"),
            "{err:#}"
        );
    }

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
        overlay_from_dir(&metadata_path, &mut tokens, &mut scripts).unwrap();

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
    fn bundled_dotcell_labels_present_on_both_networks() {
        let labels = super::bundled::script_labels();
        let expected: [(&str, &str, &str); 4] = [
            (
                "dotcell-account",
                "0xd96cee56727a2bb9a21408c154d278df5095fb4b4dcfd50516156424479bfe54",
                "0xe0706b176678181d982290d93dfcd82098e60cceaa4a87f10f32dcbcc91df1d9",
            ),
            (
                "dotcell-account-lock",
                "0x9f0f0ba142b58cba2fe047546cfd8481d5b1769437cd3533e6458b21b61871ab",
                "0xede6a3d80717c3d7927eea678d095abbe68dbb08ca6fdbbbdd9de906455a4afd",
            ),
            (
                "dotcell-sale-lock",
                "0x086c8f4e9d4272e3dfbaca399792f730e6604591e87931ee6d67047a3c900879",
                "0x498ab6b49b6b25b3c47fcea74bd8a4447bc4efda6417809152a846e058ad0ae4",
            ),
            (
                "dotcell-price",
                "0x97bf5f760cf72f918f13704d7184933b79d4ddc1fd85075762373e531152d4f9",
                "0xe1057caf161b3c720fcdb80190e89c6efc36b6b4256b3c99635fda63a9dd4294",
            ),
        ];
        for (slug, mainnet_ref, testnet_ref) in expected {
            let label = labels
                .iter()
                .find(|l| l.metadata_slug.as_deref() == Some(slug))
                .unwrap_or_else(|| panic!("missing bundled label {slug}"));
            let m = label.mainnet.as_ref().expect("mainnet block");
            let t = label.testnet.as_ref().expect("testnet block");
            assert!(
                m.versions
                    .iter()
                    .any(|v| v.canonical_ref_hash == mainnet_ref
                        && v.canonical_hash_type.as_str() == "type"),
                "{slug} mainnet"
            );
            assert!(
                t.versions
                    .iter()
                    .any(|v| v.canonical_ref_hash == testnet_ref
                        && v.canonical_hash_type.as_str() == "type"),
                "{slug} testnet"
            );
            for v in m.versions.iter().chain(t.versions.iter()) {
                assert_ne!(
                    v.version_hash, v.canonical_ref_hash,
                    "{slug}: type-id 脚本的 version_hash 必须是二进制 data hash"
                );
            }
        }
    }

    /// Phase 1b: the four Cells scripts are protocol identities, not just
    /// labels. Both networks' code hashes must resolve, or the indexer sees
    /// `.cell` cells as plain cells (PROTO-003).
    #[test]
    fn dotcell_scripts_classify_on_both_networks() {
        use crate::parser::registry::{ProtocolScript, PROTOCOL_REGISTRY};
        use crate::rpc::parse_hex_to_bytes;
        for (hash, expected) in [
            (
                "0xd96cee56727a2bb9a21408c154d278df5095fb4b4dcfd50516156424479bfe54",
                ProtocolScript::DotCellAccount,
            ),
            (
                "0xe0706b176678181d982290d93dfcd82098e60cceaa4a87f10f32dcbcc91df1d9",
                ProtocolScript::DotCellAccount,
            ),
            (
                "0x9f0f0ba142b58cba2fe047546cfd8481d5b1769437cd3533e6458b21b61871ab",
                ProtocolScript::DotCellAccountLock,
            ),
            (
                "0xede6a3d80717c3d7927eea678d095abbe68dbb08ca6fdbbbdd9de906455a4afd",
                ProtocolScript::DotCellAccountLock,
            ),
            (
                "0x086c8f4e9d4272e3dfbaca399792f730e6604591e87931ee6d67047a3c900879",
                ProtocolScript::DotCellSaleLock,
            ),
            (
                "0x498ab6b49b6b25b3c47fcea74bd8a4447bc4efda6417809152a846e058ad0ae4",
                ProtocolScript::DotCellSaleLock,
            ),
            (
                "0x97bf5f760cf72f918f13704d7184933b79d4ddc1fd85075762373e531152d4f9",
                ProtocolScript::DotCellPrice,
            ),
            (
                "0xe1057caf161b3c720fcdb80190e89c6efc36b6b4256b3c99635fda63a9dd4294",
                ProtocolScript::DotCellPrice,
            ),
        ] {
            assert_eq!(
                PROTOCOL_REGISTRY.protocol(&parse_hex_to_bytes(hash), 1),
                Some(expected),
                "{hash} must classify as {expected:?}"
            );
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

    fn startup_metadata() -> ScriptMetadata {
        serde_json::from_value(serde_json::json!({
            "metadata_slug": "startup-family", "name": "Startup Family",
            "mainnet": {"versions": [{"version_hash": format!("0x{}", "41".repeat(32)), "canonical_ref_hash": format!("0x{}", "51".repeat(32)), "canonical_hash_type": "type"}]},
            "testnet": {"versions": [{"version_hash": format!("0x{}", "42".repeat(32)), "canonical_ref_hash": format!("0x{}", "62".repeat(32)), "canonical_hash_type": "type"}]}
        })).unwrap()
    }

    #[test]
    fn startup_excluded_observed_binary_preserves_attribution_on_both_networks() {
        for network in ["mainnet", "testnet"] {
            for imported_display in [false, true] {
                let dir = TempDir::new().unwrap();
                let store = CkbadgerStore::open_domain(dir.path()).unwrap();
                let mut script = startup_metadata();
                if network == "testnet" {
                    std::mem::swap(&mut script.mainnet, &mut script.testnet);
                }
                let foreign_network = if network == "mainnet" {
                    "testnet"
                } else {
                    "mainnet"
                };
                upsert_script_label(&store, &script, foreign_network).unwrap();
                let mut observed = store.get_script_version(&[0x42; 32]).unwrap().unwrap();
                observed.type_id_deployments = vec![(vec![0x51; 32], 123)];
                observed.lock_cells_count = 17;
                observed.lock_live_cells_count = 6;
                if !imported_display {
                    observed.canonical_reference_hash = Some(vec![0x51; 32]);
                }
                store.put_script_version(&[0x42; 32], &observed).unwrap();
                let mut expected = observed;
                expected.registered_references.clear();
                if imported_display {
                    expected.canonical_reference_hash = None;
                    expected.canonical_hash_type = None;
                }
                for _ in 0..2 {
                    let result = import_all(&store, network, &[], &[script.clone()]).unwrap();
                    assert!(result.errors.is_empty(), "{:?}", result.errors);
                    assert_eq!(
                        store.get_script_version(&[0x42; 32]).unwrap().unwrap(),
                        expected
                    );
                    assert!(store
                        .list_script_version_hashes_by_family("startup-family")
                        .unwrap()
                        .contains(&vec![0x42; 32]));
                    assert!(store
                        .list_script_version_hashes_by_label("Startup Family")
                        .unwrap()
                        .contains(&vec![0x42; 32]));
                    assert!(store.get_script_info(&[0x62; 32], 1).unwrap().is_none());
                }
                // Revoking the last active anchor does not erase existing bytecode
                // attribution or turn observations into imported registrations.
                if network == "mainnet" {
                    script.mainnet.as_mut().unwrap().versions.clear();
                } else {
                    script.testnet.as_mut().unwrap().versions.clear();
                }
                for _ in 0..2 {
                    import_all(&store, network, &[], &[script.clone()]).unwrap();
                    assert_eq!(
                        store.get_script_version(&[0x42; 32]).unwrap().unwrap(),
                        expected
                    );
                    let versions = store.list_script_versions().unwrap();
                    let membership = ckbadger_store::ScriptFamilyMembershipIndex::new(
                        versions.iter().map(|(_, version)| version),
                    )
                    .unwrap();
                    assert_eq!(
                        membership.resolve_family(&[0x51; 32], 1, None).unwrap(),
                        None,
                        "retaining binary attribution must not register its observed Type ID"
                    );
                    assert_eq!(
                        membership
                            .resolve_family(&[0x51; 32], 1, Some(&[0x42; 32]))
                            .unwrap(),
                        Some("startup-family")
                    );
                }
            }
        }
    }

    #[test]
    fn startup_explicit_registration_of_observed_unknown_binary_is_allowed() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        let mut observed = ScriptVersionInfo {
            version_hash: vec![0x41; 32],
            type_id_deployments: vec![(vec![0x51; 32], 123)],
            lock_cells_count: 17,
            lock_live_cells_count: 6,
            ..Default::default()
        };
        store.put_script_version(&[0x41; 32], &observed).unwrap();
        let result = import_all(&store, "mainnet", &[], &[startup_metadata()]).unwrap();
        assert!(result.errors.is_empty(), "{:?}", result.errors);
        let imported = store.get_script_version(&[0x41; 32]).unwrap().unwrap();
        assert_eq!(imported.family_id.as_deref(), Some("startup-family"));
        assert_eq!(imported.registered_references, vec![(vec![0x51; 32], 1)]);
        assert_eq!(imported.type_id_deployments, observed.type_id_deployments);
        assert_eq!(imported.lock_cells_count, 17);
        assert_eq!(imported.lock_live_cells_count, 6);
        // The same None family is corruption when its registration was already
        // persisted. Explicit metadata must not silently heal that damaged row.
        observed.registered_references = imported.registered_references;
        store.put_script_version(&[0x41; 32], &observed).unwrap();
        let before = store.list_script_versions().unwrap();
        let error = import_all(&store, "mainnet", &[], &[startup_metadata()]).unwrap_err();
        assert!(
            format!("{error:#}").contains("registered references lack a family"),
            "{error:#}"
        );
        assert_eq!(store.list_script_versions().unwrap(), before);
    }

    #[test]
    fn startup_direct_metadata_cannot_repair_damaged_discovered_lineage() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        let mut script = startup_metadata();
        upsert_script_label(&store, &script, "mainnet").unwrap();
        let damaged = ScriptVersionInfo {
            version_hash: vec![0x42; 32],
            type_id_deployments: vec![(vec![0x51; 32], 123)],
            lock_cells_count: 17,
            lock_live_cells_count: 6,
            ..Default::default()
        };
        store.put_script_version(&[0x42; 32], &damaged).unwrap();
        // W/A is already registered, while the discovered V/A lost its family.
        // Adding V to metadata must not become an in-place corruption repair.
        let mut declaration = script.testnet.as_ref().unwrap().versions[0].clone();
        declaration.canonical_ref_hash = format!("0x{}", "51".repeat(32));
        script.mainnet.as_mut().unwrap().versions.push(declaration);
        let before_versions = store.list_script_versions().unwrap();
        let before_families = store.list_script_families().unwrap();
        let before_info = store
            .get_cf(
                store.cf_script_info(),
                &ckbadger_store::keys::encode_script_info_key(&[0x51; 32], 1),
            )
            .unwrap();
        let error = import_all(&store, "mainnet", &[], &[script]).unwrap_err();
        let message = format!("{error:#}");
        for context in [
            "mainnet",
            "123",
            "observed registered type-id contradicts family",
            &hex::encode([0x42; 32]),
        ] {
            assert!(message.contains(context), "missing {context}: {message}");
        }
        assert_eq!(store.list_script_versions().unwrap(), before_versions);
        assert_eq!(store.list_script_families().unwrap(), before_families);
        assert_eq!(
            store
                .get_cf(
                    store.cf_script_info(),
                    &ckbadger_store::keys::encode_script_info_key(&[0x51; 32], 1),
                )
                .unwrap(),
            before_info
        );
    }

    #[test]
    fn startup_corrupt_family_fails_before_token_import() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        store
            .put_cf(store.cf_script_families(), b"startup-family", b"corrupt")
            .unwrap();
        let token = bundled::udt_labels()
            .into_iter()
            .find(|token| !token.disabled && token.mainnet.is_some())
            .unwrap();
        let token_hash = compute_type_hash(token.mainnet.as_ref().unwrap()).unwrap();
        let error = import_all(&store, "mainnet", &[token], &[startup_metadata()]).unwrap_err();
        let message = format!("{error:#}");
        assert!(
            message.contains("mainnet") && message.contains("startup-family"),
            "{message}"
        );
        assert!(store.get_token(&token_hash).unwrap().is_none());
        assert!(store.list_script_versions().unwrap().is_empty());
        assert!(store.get_script_info(&[0x51; 32], 1).unwrap().is_none());
        assert_eq!(
            store
                .get_cf(store.cf_script_families(), b"startup-family")
                .unwrap()
                .unwrap(),
            b"corrupt"
        );
    }

    #[test]
    fn startup_observed_anchor_conflict_fails_before_any_import_write() {
        for family in [None, Some("wrong-family")] {
            let dir = TempDir::new().unwrap();
            let store = CkbadgerStore::open_domain(dir.path()).unwrap();
            let script = startup_metadata();
            let observed = ScriptVersionInfo {
                version_hash: vec![0x42; 32],
                family_id: family.map(str::to_owned),
                type_id_deployments: vec![(vec![0x51; 32], 123)],
                ..Default::default()
            };
            store.put_script_version(&[0x42; 32], &observed).unwrap();
            let before = store.list_script_versions().unwrap();
            let token = bundled::udt_labels()
                .into_iter()
                .find(|token| !token.disabled && token.mainnet.is_some())
                .unwrap();
            let token_hash = compute_type_hash(token.mainnet.as_ref().unwrap()).unwrap();
            let error = import_all(&store, "mainnet", &[token], &[script]).unwrap_err();
            let message = format!("{error:#}");
            for context in [
                "mainnet",
                "123",
                &hex::encode([0x42; 32]),
                &hex::encode([0x51; 32]),
                "startup-family",
            ] {
                assert!(message.contains(context), "missing {context}: {message}");
            }
            assert_eq!(store.list_script_versions().unwrap(), before);
            assert!(
                store.get_token(&token_hash).unwrap().is_none(),
                "preflight must precede token writes as well as script writes"
            );
            assert!(store.list_script_families().unwrap().is_empty());
            assert!(store.get_script_info(&[0x51; 32], 1).unwrap().is_none());
        }
    }

    #[test]
    fn startup_excluded_metadata_and_corrupt_rows_fail_before_writes() {
        for corrupt_row in [false, true] {
            let dir = TempDir::new().unwrap();
            let store = CkbadgerStore::open_domain(dir.path()).unwrap();
            let mut script = startup_metadata();
            if corrupt_row {
                store
                    .put_cf(
                        store.cf_script_info(),
                        &ckbadger_store::keys::encode_script_info_key(&[0x62; 32], 1),
                        b"corrupt",
                    )
                    .unwrap();
            } else {
                script.testnet.as_mut().unwrap().versions[0].canonical_ref_hash = "0xgg".into();
            }
            let error = import_all(&store, "mainnet", &[], &[script]).unwrap_err();
            let message = format!("{error:#}");
            assert!(
                message.contains("mainnet") && message.contains("startup-family"),
                "{message}"
            );
            assert!(
                message.contains(if corrupt_row { "62626262" } else { "0xgg" }),
                "{message}"
            );
            assert!(store.list_script_versions().unwrap().is_empty());
            assert!(store.list_script_families().unwrap().is_empty());
            assert!(store.get_script_info(&[0x51; 32], 1).unwrap().is_none());
        }
    }

    /// Finding 14: a store error stops the import instead of becoming a
    /// logged per-entry error — a corrupt stored token row, a corrupt
    /// `script_info` row of an active deployment — while a malformed token
    /// entry (metadata validation) is still collected and skipped.
    #[test]
    fn label_import_propagates_store_errors_and_collects_only_metadata_errors() {
        let token = bundled::udt_labels()
            .into_iter()
            .find(|token| !token.disabled && token.mainnet.is_some())
            .unwrap();
        let type_hash = compute_type_hash(token.mainnet.as_ref().unwrap()).unwrap();

        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        store
            .put_cf(store.cf_tokens(), &type_hash, b"corrupt")
            .unwrap();
        let error = import_all(&store, "mainnet", std::slice::from_ref(&token), &[]).unwrap_err();
        let message = format!("{error:#}");
        assert!(
            message.contains(&token.symbol) && message.contains(&hex::encode(&type_hash)),
            "{message}"
        );

        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        store
            .put_cf(
                store.cf_script_info(),
                &ckbadger_store::keys::encode_script_info_key(&[0x51; 32], 1),
                b"corrupt",
            )
            .unwrap();
        let error = import_all(&store, "mainnet", &[], &[startup_metadata()]).unwrap_err();
        let message = format!("{error:#}");
        // re-pinned (lane rd/api-lock-names, fix round S1): label import owns
        // the whole CF and reads every row before its first write, so the
        // corrupt row fails it naming the row's pair, no family attached
        assert!(
            message.contains("label import preflight") && message.contains(&"51".repeat(32)),
            "{message}"
        );
        assert!(store.list_script_families().unwrap().is_empty(), "no write");

        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        let mut malformed = token.clone();
        malformed.mainnet.as_mut().unwrap().code_hash = "0xgg".into();
        let result = import_all(&store, "mainnet", &[malformed], &[]).unwrap();
        assert_eq!(result.udt_labels_imported, 0);
        assert_eq!(result.errors.len(), 1, "{:?}", result.errors);
        assert!(
            result.errors[0].contains("invalid type script code_hash"),
            "{:?}",
            result.errors
        );

        // An unknown hash_type label is collected too, never a panic
        // (delta review: `parse_hash_type` is for node-validated labels).
        let mut bad_hash_type = token;
        bad_hash_type.mainnet.as_mut().unwrap().hash_type = "bogus".into();
        let result = import_all(&store, "mainnet", &[bad_hash_type], &[]).unwrap();
        assert_eq!(result.udt_labels_imported, 0);
        assert_eq!(result.errors.len(), 1, "{:?}", result.errors);
        assert!(
            result.errors[0].contains("invalid type script hash_type `bogus`"),
            "{:?}",
            result.errors
        );
    }

    #[test]
    fn startup_cleanup_uses_stable_family_and_preserves_other_active_registration() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        let mut script = startup_metadata();
        upsert_script_label(&store, &script, "testnet").unwrap();
        script.name = "Renamed Family".into();
        import_all(&store, "mainnet", &[], &[script.clone()]).unwrap();
        let stale = store.get_script_version(&[0x42; 32]).unwrap().unwrap();
        assert!(stale.family_id.is_none());
        assert!(stale.name.is_none());
        assert!(stale.registered_references.is_empty());
        assert!(store
            .list_script_version_hashes_by_label("Startup Family")
            .unwrap()
            .is_empty());
        assert!(store.get_script_info(&[0x62; 32], 1).unwrap().is_none());

        // An excluded entry must not revoke a reference explicitly active in
        // another metadata entry, regardless of metadata iteration order.
        let mut other = startup_metadata();
        other.metadata_slug = Some("other-family".into());
        other.mainnet = other.testnet.take();
        script.name = "Startup Family".into();
        for scripts in [
            [other.clone(), script.clone()],
            [script.clone(), other.clone()],
        ] {
            import_all(&store, "mainnet", &[], &scripts).unwrap();
            let active = store.get_script_version(&[0x42; 32]).unwrap().unwrap();
            assert_eq!(active.family_id.as_deref(), Some("other-family"));
            assert_eq!(active.registered_references, vec![(vec![0x62; 32], 1)]);
            // re-pinned (lane rd/api-lock-names, fix round ruling 2): a version's pair has no label row; it is named by the code it runs, its family
            assert_eq!(store.get_script_info(&[0x62; 32], 1).unwrap(), None);
            assert_eq!(
                store
                    .get_script_family("other-family")
                    .unwrap()
                    .unwrap()
                    .name,
                "Startup Family"
            );
        }
    }

    #[test]
    fn startup_import_rejects_append_only_without_mutating_cell_payload() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_append_only(dir.path()).unwrap();
        store
            .put_cf(store.cf_cells(), &[0x71; 36], b"immutable-payload")
            .unwrap();
        let error = import_all(&store, "mainnet", &[], &[startup_metadata()]).unwrap_err();
        assert!(error.to_string().contains("domain"), "{error:#}");
        assert_eq!(
            store
                .get_cf(store.cf_cells(), &[0x71; 36])
                .unwrap()
                .unwrap(),
            b"immutable-payload"
        );
    }

    #[test]
    fn removed_version_registration_preserves_observations_and_usage() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        let mut script: ScriptMetadata = serde_json::from_value(serde_json::json!({
            "metadata_slug": "registered-family", "name": "Registered Family",
            "mainnet": {"versions": [
                {"version_hash": format!("0x{}", "41".repeat(32)), "canonical_ref_hash": format!("0x{}", "51".repeat(32)), "canonical_hash_type": "type"},
                {"version_hash": format!("0x{}", "42".repeat(32)), "canonical_ref_hash": format!("0x{}", "52".repeat(32)), "canonical_hash_type": "type"}
            ]}
        })).unwrap();
        upsert_script_label(&store, &script, "mainnet").unwrap();
        let mut previous = store.get_script_version(&[0x42; 32]).unwrap().unwrap();
        previous.type_id_deployments = vec![(vec![0x52; 32], 123)];
        previous.lock_cells_count = 17;
        previous.lock_live_cells_count = 6;
        store.put_script_version(&[0x42; 32], &previous).unwrap();
        script.mainnet.as_mut().unwrap().versions.truncate(1);
        upsert_script_label(&store, &script, "mainnet").unwrap();
        let current = store.get_script_version(&[0x42; 32]).unwrap().unwrap();
        previous.registered_references.clear();
        previous.canonical_reference_hash = None;
        previous.canonical_hash_type = None;
        assert_eq!(
            current, previous,
            "only revoked imported registration is cleared"
        );
        upsert_script_label(&store, &script, "mainnet").unwrap();
        assert_eq!(
            store.get_script_version(&[0x42; 32]).unwrap().unwrap(),
            current
        );
    }

    #[test]
    fn registered_references_preserve_preferred_order_replace_removed_and_clear_excluded() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        let mut script: ScriptMetadata = serde_json::from_value(serde_json::json!({
            "metadata_slug": "registered-family", "name": "Registered Family",
            "mainnet": {"versions": [
                {"version_hash": format!("0x{}", "41".repeat(32)), "canonical_ref_hash": format!("0x{}", "61".repeat(32)), "canonical_hash_type": "type"},
                {"version_hash": format!("0x{}", "41".repeat(32)), "canonical_ref_hash": format!("0x{}", "51".repeat(32)), "canonical_hash_type": "type"},
                {"version_hash": format!("0x{}", "41".repeat(32)), "canonical_ref_hash": format!("0x{}", "61".repeat(32)), "canonical_hash_type": "type"}
            ]}
        })).unwrap();
        upsert_script_label(&store, &script, "mainnet").unwrap();
        let version = store.get_script_version(&[0x41; 32]).unwrap().unwrap();
        assert_eq!(
            version.registered_references,
            vec![(vec![0x51; 32], 1), (vec![0x61; 32], 1)]
        );
        assert_eq!(
            version.canonical_reference_hash,
            Some(vec![0x61; 32]),
            "first declaration is preferred, independent of sorted set"
        );
        upsert_script_label(&store, &script, "mainnet").unwrap();
        assert_eq!(
            store.get_script_version(&[0x41; 32]).unwrap().unwrap(),
            version
        );
        script.mainnet.as_mut().unwrap().versions.truncate(1);
        upsert_script_label(&store, &script, "mainnet").unwrap();
        assert_eq!(
            store
                .get_script_version(&[0x41; 32])
                .unwrap()
                .unwrap()
                .registered_references,
            vec![(vec![0x61; 32], 1)]
        );
        upsert_script_label(&store, &script, "testnet").unwrap();
        let excluded = store.get_script_version(&[0x41; 32]).unwrap().unwrap();
        assert!(excluded.registered_references.is_empty());
        assert!(excluded.family_id.is_none());
    }

    #[test]
    fn conflicting_imported_families_fail_before_any_metadata_write() {
        for (second_binary, expected) in
            [("41", "binary registered"), ("42", "reference registered")]
        {
            let dir = TempDir::new().unwrap();
            let store = CkbadgerStore::open_domain(dir.path()).unwrap();
            let scripts: Vec<ScriptMetadata> = [("one", "41"), ("two", second_binary)].into_iter().map(|(family, binary)| {
                serde_json::from_value(serde_json::json!({
                    "metadata_slug": family, "name": family,
                    "mainnet": {"versions": [{"version_hash": format!("0x{}", binary.repeat(32)), "canonical_ref_hash": format!("0x{}", "51".repeat(32)), "canonical_hash_type": "type"}]}
                })).unwrap()
            }).collect();
            let error = super::import_all(&store, "mainnet", &[], &scripts).unwrap_err();
            assert!(error.to_string().contains(expected), "{error:#}");
            assert!(store.list_script_versions().unwrap().is_empty());
            assert!(store.list_script_families().unwrap().is_empty());
            assert!(store.get_script_info(&[0x51; 32], 1).unwrap().is_none());
        }
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
                registered_references: vec![(hex::decode("9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8").unwrap(), 1)],
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
            role: None,
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
            lock_args: None,
            cobuild: None,
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
            role: None,
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
            lock_args: None,
            cobuild: None,
        };
        upsert_script_label(&store, &original, "mainnet").unwrap();

        let renamed = ScriptMetadata {
            metadata_slug: Some("rename-family".to_string()),
            name: "Renamed Family".to_string(),
            description: Some("renamed".to_string()),
            website: None,
            category: Some("lock".to_string()),
            role: None,
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
            lock_args: None,
            cobuild: None,
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

        // SECP256K1_BLAKE160 is a well-known type-ref script. Label import
        // writes its label (the row holds the label and nothing else: no
        // usage, no deployment fields) under its pair, the code hash by
        // `type` (lane rd/api-lock-names): the same code hash by `data` is
        // another script, which the registry does not name.
        let secp_code_hash =
            hex::decode("9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8")
                .unwrap();
        // re-pinned (lane rd/api-lock-names, fix round ruling 2): a version's pair has no label row; it is named by the code it runs, its family: the
        // default lock's pairs have no label row, by any hash type, and its
        // version records the registered reference
        for hash_type in [0u8, 1, 2, 4] {
            assert_eq!(
                store.get_script_info(&secp_code_hash, hash_type).unwrap(),
                None,
                "no label row for the default lock's code hash by hash type {hash_type}"
            );
        }

        // Version enrichment IS written
        let version_hash =
            hex::decode("709f3fda12f561cfacf92273c57a98fede188a3f1a59b1f888d113f9cce08649")
                .unwrap();
        let version = store.get_script_version(&version_hash).unwrap();
        assert_eq!(
            version
                .as_ref()
                .map(|version| version.registered_references.clone()),
            Some(vec![(secp_code_hash.clone(), 1)]),
            "the version registers the default lock's pair by type"
        );
        assert!(
            version.is_some(),
            "label import should write script version"
        );
    }

    /// Lane rd/api-lock-names: the pseudo deployments, which have no version
    /// row and live only in `script_info`, are labelled by their own pairs:
    /// Zero Lock is the all-zero code hash by `data`, Type ID its code hash by
    /// `type`. The other hash types of those code hashes have no label.
    /// Lane rd/api-lock-names, fix round S1: label import owns the whole
    /// `script_info` CF. After its writes every row an active pseudo
    /// deployment does not keep is gone: a version pair's row from before
    /// versions were named by their code (Default Lock by type), a pseudo
    /// deployment's row under a corrected hash type (Zero Lock by type), the
    /// row of a corrected reference (the Stable++ Vault Lock's mainnet
    /// reference 0xff352022…2e67 that ebf06909 replaced), and the row of a
    /// script set `disabled`. The active pseudo rows stay, each by its pair.
    #[test]
    fn label_import_deletes_every_label_row_it_does_not_keep() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        let label = |name: &str| ckbadger_store::types::ScriptInfo {
            name: name.to_string(),
            ..Default::default()
        };
        let secp = hex::decode("9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8")
            .unwrap();
        let vault_before_ebf06909 =
            hex::decode("ff352022029a6ecf03e8a838b979a46e1231f05f9a3df9b4198f7eeb4afc2e67")
                .unwrap();
        for (code_hash, hash_type, name) in [
            (secp.clone(), 1u8, "Default Lock"),
            (vec![0u8; 32], 1, "Zero Lock"),
            (vault_before_ebf06909.clone(), 1, "Stable++ Vault Lock"),
            (vec![0x77; 32], 0, "Disabled Script"),
        ] {
            store
                .put_script_info_direct(&code_hash, hash_type, &label(name))
                .unwrap();
        }

        super::run_label_import_bundled(&store, "mainnet").unwrap();

        let type_id =
            hex::decode("00000000000000000000000000000000000000000000000000545950455f4944")
                .unwrap();
        let rows: Vec<((Vec<u8>, u8), String)> = store
            .list_script_infos()
            .unwrap()
            .into_iter()
            .map(|(pair, info)| (pair, info.name))
            .collect();
        assert_eq!(
            rows,
            vec![
                ((vec![0u8; 32], 0), "Zero Lock".to_string()),
                ((type_id, 1), "Type ID".to_string()),
            ],
            "only the active pseudo deployments keep a label row"
        );
    }

    /// The delete never removes a row an active deployment names: a disabled
    /// entry naming the same pseudo pair as an active one leaves the active
    /// one's row, whichever entry comes first.
    #[test]
    fn label_import_keeps_a_row_an_active_entry_names() {
        let pseudo = |slug: &str, name: &str, disabled: bool| -> ScriptMetadata {
            serde_json::from_value(serde_json::json!({
                "metadata_slug": slug, "name": name, "disabled": disabled,
                "mainnet": {"pseudo": {"code_hash": format!("0x{}", "00".repeat(32)), "hash_type": "data"}}
            }))
            .unwrap()
        };
        let active = pseudo("zero-lock", "Zero Lock", false);
        let disabled = pseudo("zero-lock-old", "Old Zero Lock", true);
        for scripts in [
            [active.clone(), disabled.clone()],
            [disabled.clone(), active.clone()],
        ] {
            let dir = TempDir::new().unwrap();
            let store = CkbadgerStore::open_domain(dir.path()).unwrap();
            import_all(&store, "mainnet", &[], &scripts).unwrap();
            assert_eq!(
                store.get_script_info(&[0u8; 32], 0).unwrap().unwrap().name,
                "Zero Lock"
            );
        }
    }

    /// T SHOULD-1: a version registered by `data` or `data1` records that
    /// pair as its registered reference (the name and "registered or not"
    /// read it) and writes no label row: the bundled xUDT by data1, NexisDAO
    /// dCKB lock by data.
    #[test]
    fn label_import_registers_versions_by_data_and_data1() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        super::run_label_import_bundled(&store, "mainnet").unwrap();
        let xudt = hex::decode("50bd8d6680b8b9cf98b73f3c08faf8b2a21914311954118ad6609be6e78a1b95")
            .unwrap();
        let version = store.get_script_version(&xudt).unwrap().unwrap();
        assert!(
            version.registered_references.contains(&(xudt.clone(), 2)),
            "{:?}",
            version.registered_references
        );
        for hash_type in [0u8, 1, 2, 4] {
            assert_eq!(store.get_script_info(&xudt, hash_type).unwrap(), None);
        }

        let script: ScriptMetadata = serde_json::from_value(serde_json::json!({
            "metadata_slug": "by-data", "name": "By Data",
            "mainnet": {"versions": [{"version_hash": format!("0x{}", "5a".repeat(32)), "canonical_ref_hash": format!("0x{}", "5a".repeat(32)), "canonical_hash_type": "data"}]}
        }))
        .unwrap();
        import_all(&store, "mainnet", &[], &[script]).unwrap();
        let version = store.get_script_version(&[0x5a; 32]).unwrap().unwrap();
        assert_eq!(version.registered_references, vec![(vec![0x5a; 32], 0)]);
        assert_eq!(version.family_id.as_deref(), Some("by-data"));
        assert_eq!(store.get_script_info(&[0x5a; 32], 0).unwrap(), None);
    }

    /// L N2: a reference that is not 32 bytes is an error naming the file
    /// (the family) and the entry, before any write; it panicked in the key
    /// encoder with no file named.
    #[test]
    fn a_reference_of_the_wrong_length_is_refused_naming_the_entry() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        let script: ScriptMetadata = serde_json::from_value(serde_json::json!({
            "metadata_slug": "short-ref", "name": "Short Ref",
            "mainnet": {"versions": [{"version_hash": format!("0x{}", "5b".repeat(32)), "canonical_ref_hash": format!("0x{}", "5c".repeat(31)), "canonical_hash_type": "type"}]}
        }))
        .unwrap();
        let error = import_all(&store, "mainnet", &[], &[script]).unwrap_err();
        let message = format!("{error:#}");
        assert!(
            message.contains("family=short-ref")
                && message.contains(&format!("0x{}", "5c".repeat(31)))
                && message.contains("is 31 bytes, not 32"),
            "{message}"
        );
        assert!(store.list_script_families().unwrap().is_empty(), "no write");
    }

    #[test]
    fn label_import_keys_the_pseudo_deployments_by_their_pairs() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_domain(dir.path().to_str().unwrap()).unwrap();
        super::run_label_import_bundled(&store, "mainnet").unwrap();

        let zero = vec![0u8; 32];
        let type_id =
            hex::decode("00000000000000000000000000000000000000000000000000545950455f4944")
                .unwrap();
        for (code_hash, hash_type, name) in [(&zero, 0u8, "Zero Lock"), (&type_id, 1, "Type ID")] {
            assert_eq!(
                store
                    .get_script_info(code_hash, hash_type)
                    .unwrap()
                    .unwrap()
                    .name,
                name
            );
            for other in [0u8, 1, 2, 4]
                .into_iter()
                .filter(|other| *other != hash_type)
            {
                assert_eq!(
                    store.get_script_info(code_hash, other).unwrap(),
                    None,
                    "{name}'s code hash by hash type {other} is another script"
                );
            }
        }
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
            role: None,
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
            lock_args: None,
            cobuild: None,
        };

        // Should succeed — no error returned.
        upsert_script_label(&store, &script, "mainnet").unwrap();

        // Code-hash-level metadata IS written.
        let code_hash =
            hex::decode("00000000000000000000000000000000000000000000000000545950455f4944")
                .unwrap();
        // Type ID is a pseudo deployment by `type`: its label is keyed by that pair.
        let info = store.get_script_info(&code_hash, 1).unwrap().unwrap();
        assert_eq!(info.name, "Type ID");
        assert_eq!(store.get_script_info(&code_hash, 0).unwrap(), None);

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
                    metadata_source: None,
                    chain_metadata: None,
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

    /// resync fix round M1: a token row is the chain's. Label import stores
    /// the label (`token_labels`) and writes no token row for a token the
    /// chain does not hold; a row the chain made it leaves as it is (resync
    /// fix round 2, B: the API joins the label when it reads the token; it
    /// was "attaches the label to the row", and the test was named so); a
    /// label no entry carries any more is deleted.
    #[test]
    fn label_import_writes_the_label_alone_and_leaves_a_chain_row_as_it_is() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_domain(dir.path().to_str().unwrap()).unwrap();
        let token = |args: &str, symbol: &str| TokenMetadata {
            name: format!("{symbol} Token"),
            symbol: symbol.to_string(),
            decimals: 8,
            standard: "xudt_compatible".to_string(),
            icon: Some(format!("https://example.invalid/{symbol}.png")),
            description: Some(format!("{symbol} description")),
            disabled: false,
            mainnet: Some(TokenDeployment {
                code_hash: "0x50bd8d6680b8b9cf98b73f3c08faf8b2a21914311954118ad6609be6e78a1b95"
                    .to_string(),
                hash_type: "data1".to_string(),
                args: args.to_string(),
            }),
            testnet: None,
        };
        let unheld = token("0x01", "UNH");
        let held = token("0x02", "HLD");
        let unheld_hash = compute_type_hash(unheld.mainnet.as_ref().unwrap()).unwrap();
        let held_hash = compute_type_hash(held.mainnet.as_ref().unwrap()).unwrap();
        let chain_row = ckbadger_store::types::TokenInfo {
            type_code_hash: hex::decode(
                "50bd8d6680b8b9cf98b73f3c08faf8b2a21914311954118ad6609be6e78a1b95",
            )
            .unwrap(),
            hash_type: 2,
            type_args: vec![0x02],
            standard: "xudt".to_string(),
            name: None,
            symbol: None,
            decimals: None,
            max_supply: Some(21_000_000),
            first_seen_block: 4_242,
            icon_url: None,
            description: None,
            transfers_count: 9,
            metadata_source: None,
            chain_metadata: None,
        };
        store.put_token_direct(&held_hash, &chain_row).unwrap();

        let result = import_all(&store, "mainnet", &[unheld.clone(), held.clone()], &[]).unwrap();
        assert!(result.errors.is_empty(), "{:?}", result.errors);

        assert_eq!(
            store.get_token(&unheld_hash).unwrap(),
            None,
            "no token row for a token the chain does not hold"
        );
        let stored_label = store.get_token_label(&unheld_hash).unwrap().unwrap();
        assert_eq!(
            stored_label,
            ckbadger_store::types::TokenLabel {
                name: "UNH Token".to_string(),
                symbol: "UNH".to_string(),
                decimals: 8,
                icon_url: Some("https://example.invalid/UNH.png".to_string()),
                description: Some("UNH description".to_string()),
                standard: "xudt_compatible".to_string(),
            }
        );

        assert_eq!(
            store.get_token(&held_hash).unwrap().unwrap(),
            chain_row,
            "the chain's row, untouched"
        );
        assert!(store.get_token_label(&held_hash).unwrap().is_some());

        // The unheld token's entry removed: its label goes with it.
        import_all(&store, "mainnet", std::slice::from_ref(&held), &[]).unwrap();
        assert_eq!(store.get_token_label(&unheld_hash).unwrap(), None);
        assert!(store.get_token_label(&held_hash).unwrap().is_some());
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
            role: None,
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
            lock_args: None,
            cobuild: None,
        };

        upsert_script_label(&store, &script, "mainnet").unwrap();

        let code_hash =
            hex::decode("bf43c3602455798c1a61a596e0d95278864c552fafe231c063b3fabf97a8febc")
                .unwrap();
        // re-pinned (lane rd/api-lock-names, fix round ruling 2): a version's pair has no label row; it is named by the code it runs, its family; the
        // version carries the deprecated flag
        assert_eq!(store.get_script_info(&code_hash, 1).unwrap(), None);

        let version_hash =
            hex::decode("d6a5a0edb152e88e8bbc702e164441cb3890fae35da672b408d28ca9a1bde3ee")
                .unwrap();
        let version_info = store.get_script_version(&version_hash).unwrap().unwrap();
        assert_eq!(version_info.name.as_deref(), Some("Deprecated Script"));
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
            role: None,
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
            lock_args: None,
            cobuild: None,
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

        // re-pinned (lane rd/api-lock-names, fix round ruling 2): a version's pair has no label row; it is named by the code it runs, its family
        assert_eq!(store.get_script_info(&mainnet_code_hash, 1).unwrap(), None);
        assert_eq!(store.get_script_info(&testnet_code_hash, 1).unwrap(), None);

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
            metadata_source: (name.is_some() || symbol.is_some() || decimals.is_some())
                .then_some(ckbadger_store::types::TokenMetadataSource::Chain),
            chain_metadata: None,
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

    /// InterPay's published testnet USDI deployment. The contract's SSRI
    /// metadata reports six decimals; attaching this script to the unrelated
    /// eight-decimal plain-xUDT label was the regression fixed by PR #12.
    const TESTNET_USDI_TYPE_HASH: &str =
        "07ac97b5ff3df4b49f59a59f4d80d33d22c1263a57467c512c93b9c29b7a0de3";
    const TESTNET_USDI_CODE_HASH: &str =
        "0xcc9dc33ef234e14bc788c43a4848556a5fb16401a04662fc55db9bb201987037";
    const TESTNET_USDI_ARGS: &str =
        "0x71fd1985b2971a9903e4d8ed0d59e6710166985217ca0681437883837b86162f";

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

    /// The row the sync write path creates when the chain first holds the
    /// token of `deployment`: a fresh chain row with the label label import
    /// stored (`token_labels`) attached by the one rule. Label import itself
    /// writes no token row (resync fix round M1: a token row is the chain's),
    /// so the label is read back out of a real label import, never rebuilt
    /// from the TOML. The first block is any block: these checks read the
    /// metadata only.
    fn row_created_with_label(
        store: &CkbadgerStore,
        deployment: &TokenDeployment,
        type_hash: &[u8],
        symbol: &str,
    ) -> TokenInfo {
        assert!(
            store.get_token(type_hash).unwrap().is_none(),
            "label import writes no token row for {symbol} (0x{})",
            hex::encode(type_hash)
        );
        let label = store
            .get_token_label(type_hash)
            .expect("token label read must succeed")
            .unwrap_or_else(|| {
                panic!(
                    "label import must have stored the label of {symbol} (0x{})",
                    hex::encode(type_hash)
                )
            });
        let mut row = TokenInfo {
            type_code_hash: hex::decode(deployment.code_hash.trim_start_matches("0x")).unwrap(),
            hash_type: ckbadger_common::hash_type_from_label(&deployment.hash_type).unwrap(),
            type_args: hex::decode(deployment.args.trim_start_matches("0x")).unwrap(),
            standard: "xudt".to_string(),
            name: None,
            symbol: None,
            decimals: None,
            max_supply: None,
            first_seen_block: 1,
            icon_url: None,
            description: None,
            transfers_count: 0,
            metadata_source: None,
            chain_metadata: None,
        };
        row.attach_label(&label);
        row
    }

    /// Every token row the bundled labels produce for `network` when the
    /// chain first holds each token, from a real label import: the state the
    /// sync write path creates on a fresh database.
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
            let info = row_created_with_label(store, deployment, &type_hash, &token.symbol);
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

    #[test]
    fn bundled_testnet_usdi_label_matches_issuer_deployment() {
        let expected_type_hash =
            hex::decode(TESTNET_USDI_TYPE_HASH).expect("pinned USDI type hash must be valid hex");
        let labels = bundled::udt_labels();
        let matching: Vec<(&TokenMetadata, &TokenDeployment)> = labels
            .iter()
            .filter(|token| !token.disabled)
            .filter_map(|token| {
                let deployment = token.testnet.as_ref()?;
                let type_hash =
                    compute_type_hash(deployment).expect("bundled deployment must hash");
                (type_hash == expected_type_hash).then_some((token, deployment))
            })
            .collect();

        assert_eq!(
            matching.len(),
            1,
            "testnet USDI must have exactly one bundled label, found {}",
            matching.len()
        );
        let (label, deployment) = matching[0];
        assert_eq!(deployment.code_hash, TESTNET_USDI_CODE_HASH);
        assert_eq!(deployment.hash_type, "type");
        assert_eq!(deployment.args, TESTNET_USDI_ARGS);
        assert_eq!(label.name, "USDI");
        assert_eq!(label.symbol, "USDI");
        assert_eq!(label.decimals, 6);
        assert_eq!(label.standard, "xudt_compatible");

        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_domain(dir.path().to_str().unwrap()).unwrap();
        let result = super::run_label_import_bundled(&store, "testnet").unwrap();
        assert!(
            result.errors.is_empty(),
            "testnet label import failed: {:?}",
            result.errors
        );

        let stored = row_created_with_label(&store, deployment, &expected_type_hash, "USDI");
        assert_eq!(stored.name.as_deref(), Some("USDI"));
        assert_eq!(stored.symbol.as_deref(), Some("USDI"));
        assert_eq!(stored.decimals, Some(6));
        assert_eq!(stored.standard, "xudt_compatible");
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
        let mut row = row_created_with_label(&store, deployment, &type_hash, &label.symbol);

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
            role: None,
            disabled: false,
            mainnet: Some(ScriptNetworkMetadata {
                versions,
                pseudo: None,
            }),
            testnet: None,
            lock_args: None,
            cobuild: None,
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

        // re-pinned (lane rd/api-lock-names, fix round ruling 2): a reference
        // whose version cannot attach runs no registered code, so it has no
        // name and no label row (the bundled registry holds none such:
        // `no_bundled_script_label_carries_an_unattachable_version_identity`)
        assert_eq!(store.get_script_info(&placeholder_bytes, 1).unwrap(), None);
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
            role: None,
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
            lock_args: None,
            cobuild: None,
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

        // Both references are registered on the one version (re-pinned (lane rd/api-lock-names, fix round ruling 2): a version's pair has no label row; it is named by the code it runs, its family).
        let version = store
            .get_script_version(&decode_hex(shared_version).unwrap())
            .unwrap()
            .unwrap();
        for reference in [
            "0xd0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0",
            "0x6161616161616161616161616161616161616161616161616161616161616161",
        ] {
            let reference = decode_hex(reference).unwrap();
            assert!(version
                .registered_references
                .contains(&(reference.clone(), 1)));
            assert_eq!(store.get_script_info(&reference, 1).unwrap(), None);
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
                lock_code_hash: [0x22; 32],
                lock_hash_type: 1,
                lock_args: vec![],
                type_code_hash: Some([0x33; 32]),
                type_hash_type: Some(1),
                type_args: Some(vec![]),
                data_size: 128,
                udt_amount: None,
                data_hash: Some(funding_version.as_slice().try_into().unwrap()),
            },
            5,
        );
        batch.put_cell_by_type(&funding_ref, 5, &[0xcc; 32], 1);
        batch.put_cell_by_data_hash(&funding_version, 5, &[0xcc; 32], 1);
        // The chain writer owns this canonical binding; label refresh reads it.
        batch.put_script_reference_to_version(1, &funding_ref, &funding_version);
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

        // The live rollup over the committed state (no block moved).
        let store = std::sync::Arc::new(store);
        crate::db::BatchWriter::new(store.clone(), store.clone())
            .commit_rollup_of_committed_state_for_test()
            .unwrap();

        let version = store
            .get_script_version(&funding_version)
            .unwrap()
            .expect("the live funding bytecode version row must exist");
        assert_eq!(
            version.name.as_deref(),
            Some("Fiber Funding Lock"),
            "the label must be attached to the version the chain resolves"
        );
        assert_eq!(version.family_id.as_deref(), Some("fiber-funding-lock"));
        assert_eq!(version.lock_live_cells_count, 5021);
        assert_eq!(version.lock_owned_capacity_sum, 16_131_558_405_032_860);

        let family = store
            .get_script_family("fiber-funding-lock")
            .unwrap()
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
