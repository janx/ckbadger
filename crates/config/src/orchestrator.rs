use std::collections::HashSet;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

use crate::{FrontendConfig, LogConfig};
use ckbadger_common::hardfork::normalize_network;

/// One launched network stack: a subdir workdir under the orchestrator root.
// `PartialEq`-only (no `Eq`) to match the crate's config-struct convention:
// this embeds the reused `FrontendConfig`/`LogConfig`, which are `PartialEq`-only.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NetworkEntry {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dir: Option<String>,
}

impl NetworkEntry {
    /// Subdirectory name (defaults to the network name).
    pub fn dir(&self) -> &str {
        self.dir.as_deref().unwrap_or(&self.name)
    }
}

/// Top-level `ckbadger.toml`: which network subdirs to run + shared frontend/log.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OrchestratorConfig {
    #[serde(rename = "network", default)]
    pub networks: Vec<NetworkEntry>,
    #[serde(default)]
    pub frontend: FrontendConfig,
    #[serde(default)]
    pub log: LogConfig,
}

pub fn parse_orchestrator_config(s: &str) -> Result<OrchestratorConfig> {
    let cfg: OrchestratorConfig =
        toml::from_str(s).context("failed to parse orchestrator ckbadger.toml")?;
    if cfg.networks.is_empty() {
        bail!("orchestrator ckbadger.toml must define at least one [[network]]");
    }
    let mut seen = HashSet::new();
    for entry in &cfg.networks {
        // Dedup on the CANONICAL chain, not the raw name, so aliases of the same
        // network (e.g. "mainnet"/"ckb", "testnet"/"pudge") cannot spawn two stacks.
        let canonical = match normalize_network(&entry.name) {
            Some(canonical) => canonical,
            None => bail!(
                "unknown network '{}' in [[network]] (expected mainnet or testnet)",
                entry.name
            ),
        };
        if !seen.insert(canonical) {
            bail!("duplicate network name '{}' in [[network]]", entry.name);
        }
    }
    Ok(cfg)
}

pub fn load_orchestrator_config(root: &Path) -> Result<OrchestratorConfig> {
    let path = root.join("ckbadger.toml");
    let content = std::fs::read_to_string(&path)
        .with_context(|| format!("failed to read orchestrator config: {}", path.display()))?;
    parse_orchestrator_config(&content)
}

/// True iff `<root>/ckbadger.toml` exists (orchestrator mode) vs a plain
/// single-network workdir (which has `config.toml`).
pub fn is_orchestrator(root: &Path) -> bool {
    root.join("ckbadger.toml").is_file()
}

/// The per-network workdir path for an entry.
pub fn network_workdir(root: &Path, entry: &NetworkEntry) -> PathBuf {
    root.join(entry.dir())
}

/// Number of network stacks co-resident on this host that share RAM with `workdir`.
///
/// Locates the governing orchestrator root — the nearest ancestor whose
/// `ckbadger.toml` lists `workdir` as one of its `[[network]]` entries — and
/// returns that orchestrator's network count. Returns 1 when no orchestrator
/// governs `workdir` (a single-network workdir, or a standalone invocation), so
/// the single-network case is the degenerate N=1 of one rule rather than a
/// special case.
///
/// Ancestors are walked rather than only checking the immediate parent because a
/// `[[network]].dir` may be nested (e.g. `dir = "nets/mainnet"`).
///
/// Callers divide the detected host RAM by this; see `StoreRuntimeConfig::network_count`.
pub fn co_resident_network_count(workdir: &Path) -> usize {
    // Canonicalize both sides so a relative `-C work/testnet` still matches the
    // orchestrator's resolved `root/<dir>`. A path that cannot be canonicalized
    // does not exist, so no orchestrator can be governing it.
    let Ok(target) = workdir.canonicalize() else {
        return 1;
    };

    let mut cursor = target.as_path();
    while let Some(root) = cursor.parent() {
        if is_orchestrator(root) {
            if let Ok(orch) = load_orchestrator_config(root) {
                let governs = orch.networks.iter().any(|entry| {
                    network_workdir(root, entry)
                        .canonicalize()
                        .map(|resolved| resolved == target)
                        .unwrap_or(false)
                });
                if governs {
                    return orch.networks.len();
                }
            }
        }
        cursor = root;
    }

    1
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn parses_network_array_and_defaults_dir_to_name() {
        let toml = r#"
[[network]]
name = "mainnet"

[[network]]
name = "testnet"
dir = "test-chain"

[frontend]
port = 3000

[log]
level = "debug"
"#;
        let cfg = parse_orchestrator_config(toml).unwrap();
        assert_eq!(cfg.networks.len(), 2);
        assert_eq!(cfg.networks[0].name, "mainnet");
        assert_eq!(cfg.networks[0].dir(), "mainnet"); // defaults to name
        assert_eq!(cfg.networks[1].dir(), "test-chain"); // explicit override
        assert_eq!(cfg.frontend.port, 3000);
        assert_eq!(cfg.log.level, "debug");
    }

    #[test]
    fn rejects_empty_networks() {
        let err = parse_orchestrator_config("[frontend]\nport = 3000\n").unwrap_err();
        assert!(err.to_string().contains("at least one [[network]]"));
    }

    #[test]
    fn rejects_duplicate_and_unknown_network_names() {
        let dup = "[[network]]\nname=\"mainnet\"\n[[network]]\nname=\"mainnet\"\n";
        assert!(parse_orchestrator_config(dup)
            .unwrap_err()
            .to_string()
            .contains("duplicate network name"));
        let unknown = "[[network]]\nname=\"devnet\"\n";
        assert!(parse_orchestrator_config(unknown)
            .unwrap_err()
            .to_string()
            .contains("unknown network"));
    }

    #[test]
    fn rejects_duplicate_canonical_network_aliases() {
        // "mainnet" and "ckb" are aliases of the same canonical chain: reject.
        let dup_alias = "[[network]]\nname=\"mainnet\"\n[[network]]\nname=\"ckb\"\n";
        assert!(parse_orchestrator_config(dup_alias)
            .unwrap_err()
            .to_string()
            .contains("duplicate network name"));
    }

    #[test]
    fn network_workdir_joins_dir() {
        let entry = NetworkEntry {
            name: "testnet".into(),
            dir: None,
        };
        assert_eq!(
            network_workdir(Path::new("/srv/ckb"), &entry),
            Path::new("/srv/ckb/testnet")
        );
    }

    #[test]
    fn is_orchestrator_detects_ckbadger_toml() {
        let dir = TempDir::new().unwrap();
        // Empty dir (no ckbadger.toml) => not an orchestrator root.
        assert!(!is_orchestrator(dir.path()));

        std::fs::write(
            dir.path().join("ckbadger.toml"),
            "[[network]]\nname=\"mainnet\"\n",
        )
        .unwrap();
        assert!(is_orchestrator(dir.path()));
    }

    #[test]
    fn load_orchestrator_config_reads_and_parses_file() {
        let dir = TempDir::new().unwrap();
        std::fs::write(
            dir.path().join("ckbadger.toml"),
            "[[network]]\nname=\"mainnet\"\n[[network]]\nname=\"testnet\"\ndir=\"test-chain\"\n",
        )
        .unwrap();

        let cfg = load_orchestrator_config(dir.path()).unwrap();
        assert_eq!(cfg.networks.len(), 2);
        assert_eq!(cfg.networks[0].name, "mainnet");
        assert_eq!(cfg.networks[0].dir(), "mainnet");
        assert_eq!(cfg.networks[1].dir(), "test-chain");
    }

    #[test]
    fn load_orchestrator_config_missing_file_errors() {
        let dir = TempDir::new().unwrap();
        let err = load_orchestrator_config(dir.path()).unwrap_err();
        assert!(err
            .to_string()
            .contains("failed to read orchestrator config"));
    }

    #[test]
    fn co_resident_count_returns_network_count_for_an_orchestrator_subdir() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        std::fs::write(
            root.join("ckbadger.toml"),
            r#"
[[network]]
name = "mainnet"

[[network]]
name = "testnet"
"#,
        )
        .unwrap();
        std::fs::create_dir_all(root.join("mainnet")).unwrap();
        std::fs::create_dir_all(root.join("testnet")).unwrap();

        assert_eq!(co_resident_network_count(&root.join("mainnet")), 2);
        assert_eq!(co_resident_network_count(&root.join("testnet")), 2);
    }

    #[test]
    fn co_resident_count_is_one_for_a_single_network_workdir() {
        let tmp = TempDir::new().unwrap();
        let workdir = tmp.path().join("work");
        std::fs::create_dir_all(&workdir).unwrap();
        std::fs::write(workdir.join("config.toml"), "").unwrap();

        // No orchestrator governs this workdir: the degenerate N=1 case.
        assert_eq!(co_resident_network_count(&workdir), 1);
    }

    #[test]
    fn co_resident_count_is_one_when_the_orchestrator_does_not_list_the_workdir() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        std::fs::write(
            root.join("ckbadger.toml"),
            "[[network]]\nname = \"mainnet\"\n",
        )
        .unwrap();
        std::fs::create_dir_all(root.join("mainnet")).unwrap();
        let stray = root.join("not-a-network");
        std::fs::create_dir_all(&stray).unwrap();

        assert_eq!(co_resident_network_count(&stray), 1);
    }

    #[test]
    fn co_resident_count_handles_a_nested_network_dir() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        std::fs::write(
            root.join("ckbadger.toml"),
            r#"
[[network]]
name = "mainnet"
dir = "nets/mainnet"

[[network]]
name = "testnet"
dir = "nets/testnet"
"#,
        )
        .unwrap();
        std::fs::create_dir_all(root.join("nets/mainnet")).unwrap();
        std::fs::create_dir_all(root.join("nets/testnet")).unwrap();

        // The root is two levels up, so an immediate-parent-only check would miss it.
        assert_eq!(co_resident_network_count(&root.join("nets/mainnet")), 2);
    }

    #[test]
    fn co_resident_count_is_one_for_a_nonexistent_path() {
        let tmp = TempDir::new().unwrap();
        assert_eq!(co_resident_network_count(&tmp.path().join("missing")), 1);
    }
}
