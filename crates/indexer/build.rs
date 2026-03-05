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
