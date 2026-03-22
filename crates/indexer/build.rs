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
            if let Ok(content) = fs::read_to_string(&path) {
                match toml::from_str::<TokenMetadata>(&content) {
                    Ok(token) => token_entries.push(token),
                    Err(e) => {
                        eprintln!("cargo:warning=failed to parse {}: {}", path.display(), e);
                    }
                }
            }
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
            if let Ok(content) = fs::read_to_string(&path) {
                match toml::from_str::<ScriptMetadata>(&content) {
                    Ok(script) => script_entries.push(script),
                    Err(e) => {
                        eprintln!("cargo:warning=failed to parse {}: {}", path.display(), e);
                    }
                }
            }
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

        for deployment in entry.mainnet.iter().chain(entry.testnet.iter()) {
            if deployment.deprecated {
                continue;
            }
            if !deployment.code_hash.is_empty()
                && !excluded_udt_code_hashes.contains(deployment.code_hash.as_str())
            {
                udt_script_code_hashes.push(deployment.code_hash.clone());
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
