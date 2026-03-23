use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

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
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    website: Option<String>,
    #[serde(default)]
    category: Option<String>,
    #[serde(default)]
    mainnet: Option<ScriptNetworkMetadata>,
    #[serde(default)]
    testnet: Option<ScriptNetworkMetadata>,
}

#[derive(Deserialize, Serialize)]
#[serde(try_from = "ScriptNetworkMetadataRaw")]
struct ScriptNetworkMetadata {
    versions: Vec<ScriptDeployment>,
    pseudo: Option<PseudoScriptDeployment>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ScriptNetworkMetadataRaw {
    #[serde(default)]
    versions: Vec<ScriptDeployment>,
    #[serde(default)]
    pseudo: Option<PseudoScriptDeployment>,
}

impl TryFrom<ScriptNetworkMetadataRaw> for ScriptNetworkMetadata {
    type Error = String;

    fn try_from(raw: ScriptNetworkMetadataRaw) -> Result<Self, Self::Error> {
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

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct PseudoScriptDeployment {
    code_hash: String,
    hash_type: ValidatedHashType,
}

#[derive(Deserialize, Serialize)]
#[serde(try_from = "ScriptDeploymentRaw")]
struct ScriptDeployment {
    version_hash: String,
    canonical_ref_hash: String,
    canonical_hash_type: ValidatedHashType,
    #[serde(default)]
    deprecated: bool,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ScriptDeploymentRaw {
    version_hash: String,
    canonical_ref_hash: String,
    canonical_hash_type: String,
    #[serde(default)]
    deprecated: bool,
}

impl TryFrom<ScriptDeploymentRaw> for ScriptDeployment {
    type Error = String;

    fn try_from(raw: ScriptDeploymentRaw) -> Result<Self, Self::Error> {
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

#[derive(Serialize)]
struct ValidatedHashType(String);

impl ValidatedHashType {
    fn new(value: String, field: &str) -> Result<Self, String> {
        match value.as_str() {
            "data" | "type" | "data1" | "data2" => Ok(Self(value)),
            _ => Err(format!("invalid {field}: `{value}`")),
        }
    }
}

impl<'de> Deserialize<'de> for ValidatedHashType {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        ValidatedHashType::new(value, "hash_type").map_err(serde::de::Error::custom)
    }
}

impl ScriptNetworkMetadata {
    fn versions(&self) -> &[ScriptDeployment] {
        &self.versions
    }
}

fn main() {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let out_dir = env::var("OUT_DIR").unwrap();

    let repo_root = PathBuf::from(&manifest_dir).join("../.."); // crates/indexer -> repo root
    let metadata_dir = repo_root.join("docs/metadata");

    // Rerun triggers
    println!("cargo:rerun-if-changed={}", metadata_dir.display());

    // --- UDT labels ---
    let tokens_dir = metadata_dir.join("tokens");
    let mut token_entries: Vec<TokenMetadata> = Vec::new();
    if tokens_dir.exists() {
        for entry in fs::read_dir(&tokens_dir).unwrap().flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("toml") {
                continue;
            }
            let content = fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("failed to read {}: {}", path.display(), e));
            let token = toml::from_str::<TokenMetadata>(&content)
                .unwrap_or_else(|e| panic!("failed to parse {}: {}", path.display(), e));
            token_entries.push(token);
        }
    }
    let udt_json = serde_json::to_string(&token_entries).expect("failed to serialize UDT labels");
    fs::write(
        Path::new(&out_dir).join("bundled_udt_labels.json"),
        udt_json,
    )
    .expect("failed to write bundled_udt_labels.json");

    // --- Script labels ---
    let scripts_dir = metadata_dir.join("scripts");
    let mut script_entries: Vec<ScriptMetadata> = Vec::new();
    if scripts_dir.exists() {
        for entry in fs::read_dir(&scripts_dir).unwrap().flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("toml") {
                continue;
            }
            let content = fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("failed to read {}: {}", path.display(), e));
            let script = toml::from_str::<ScriptMetadata>(&content)
                .unwrap_or_else(|e| panic!("failed to parse {}: {}", path.display(), e));
            script_entries.push(script);
        }
    }
    let script_json =
        serde_json::to_string(&script_entries).expect("failed to serialize script labels");
    fs::write(
        Path::new(&out_dir).join("bundled_script_labels.json"),
        script_json,
    )
    .expect("failed to write bundled_script_labels.json");

    // --- UDT-compatible script code_hashes ---
    // Extract code_hashes from scripts with category "udt", excluding the 3
    // well-known UDT code_hashes (SUDT, xUDT data1, xUDT type) that are already
    // hardcoded in the activity builder.
    let excluded_udt_code_hashes: HashSet<&str> = [
        "0x5e7a36a77e68eecc013dfa2fe6a23f3b6c344b04005808694ae6dd45eea4cfd5", // SUDT
        "0x50bd8d6680b8b9cf98b73f3c08faf8b2a21914311954118ad6609be6e78a1b95", // xUDT data1
        "0x25c29dc317811a6f6f3985a7a9ebc4838bd388d19d0feeecf0bcd60f6c0975bb", // xUDT type
    ]
    .into_iter()
    .collect();

    let mut udt_script_code_hashes: Vec<String> = Vec::new();
    for entry in &script_entries {
        let is_udt = entry.category.as_deref() == Some("udt");
        if !is_udt {
            continue;
        }

        let mainnet_versions = entry
            .mainnet
            .as_ref()
            .map(ScriptNetworkMetadata::versions)
            .into_iter()
            .flatten();
        let testnet_versions = entry
            .testnet
            .as_ref()
            .map(ScriptNetworkMetadata::versions)
            .into_iter()
            .flatten();

        for deployment in mainnet_versions.chain(testnet_versions) {
            if deployment.deprecated {
                continue;
            }
            if !deployment.canonical_ref_hash.is_empty()
                && !excluded_udt_code_hashes.contains(deployment.canonical_ref_hash.as_str())
            {
                udt_script_code_hashes.push(deployment.canonical_ref_hash.clone());
            }
        }
    }
    udt_script_code_hashes.sort();
    udt_script_code_hashes.dedup();

    let udt_code_hashes_json = serde_json::to_string(&udt_script_code_hashes)
        .expect("failed to serialize UDT script code_hashes");
    fs::write(
        Path::new(&out_dir).join("bundled_udt_script_code_hashes.json"),
        udt_code_hashes_json,
    )
    .expect("failed to write bundled_udt_script_code_hashes.json");
}
