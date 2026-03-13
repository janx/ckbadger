use std::collections::HashSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

fn main() {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let out_dir = env::var("OUT_DIR").unwrap();

    let repo_root = PathBuf::from(&manifest_dir).join("../.."); // crates/indexer -> repo root
    let labels_dir = repo_root.join("docs/token-labels");
    let overrides_file = repo_root.join("docs/script-name-overrides.json");

    // Rerun triggers
    println!("cargo:rerun-if-changed={}", labels_dir.display());
    println!("cargo:rerun-if-changed={}", overrides_file.display());

    // --- UDT labels ---
    let mut udt_entries = Vec::new();
    for network in &["mainnet", "testnet"] {
        let network_dir = labels_dir.join("information/udt").join(network);
        if !network_dir.exists() {
            continue;
        }
        collect_index_jsons(&network_dir, &mut udt_entries);
    }
    // Filter to published only
    let udt_entries: Vec<serde_json::Value> = udt_entries
        .into_iter()
        .filter(|v| {
            v.get("published")
                .and_then(|p| p.as_bool())
                .unwrap_or(false)
        })
        .collect();
    let udt_json = serde_json::to_string(&udt_entries).expect("failed to serialize UDT labels");
    fs::write(
        Path::new(&out_dir).join("bundled_udt_labels.json"),
        udt_json,
    )
    .expect("failed to write bundled_udt_labels.json");

    // --- Script labels ---
    let mut script_entries = Vec::new();
    let script_dir = labels_dir.join("information/script");
    if script_dir.exists() {
        collect_index_jsons(&script_dir, &mut script_entries);
    }
    let script_json =
        serde_json::to_string(&script_entries).expect("failed to serialize script labels");
    fs::write(
        Path::new(&out_dir).join("bundled_script_labels.json"),
        script_json,
    )
    .expect("failed to write bundled_script_labels.json");

    // --- UDT-compatible script code_hashes ---
    // Extract code_hashes from scripts with decoderType "udt", excluding the 3
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
        // Only scripts with decoderType "udt"
        let is_udt = entry.get("decoderType").and_then(|v| v.as_str()) == Some("udt");
        if !is_udt {
            continue;
        }

        // Iterate deployments -> each network -> array of deployments
        let deployments = match entry.get("deployments").and_then(|d| d.as_object()) {
            Some(d) => d,
            None => continue,
        };
        for (_network, network_deployments) in deployments {
            let deployment_list = match network_deployments.as_array() {
                Some(a) => a,
                None => continue,
            };
            for deployment in deployment_list {
                // Skip deprecated deployments
                let is_deprecated = deployment
                    .get("deprecated")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                if is_deprecated {
                    continue;
                }

                if let Some(code_hash) = deployment.get("codeHash").and_then(|v| v.as_str()) {
                    if !code_hash.is_empty() && !excluded_udt_code_hashes.contains(code_hash) {
                        udt_script_code_hashes.push(code_hash.to_string());
                    }
                }
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

    // --- Script name overrides ---
    let overrides_content = if overrides_file.exists() {
        fs::read_to_string(&overrides_file).expect("failed to read script-name-overrides.json")
    } else {
        r#"{"overrides":{},"deprecated":[]}"#.to_string()
    };
    fs::write(
        Path::new(&out_dir).join("bundled_script_overrides.json"),
        overrides_content,
    )
    .expect("failed to write bundled_script_overrides.json");
}

fn collect_index_jsons(dir: &Path, out: &mut Vec<serde_json::Value>) {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
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
        if let Ok(content) = fs::read_to_string(&index_path) {
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(&content) {
                out.push(value);
            }
        }
    }
}
