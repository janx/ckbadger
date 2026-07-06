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
}
