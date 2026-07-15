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
/// returns that orchestrator's network count.
///
/// Returns `Ok(1)` when no orchestrator governs `workdir`: a single-network
/// workdir, a standalone invocation, or an orchestrator that exists but does not
/// list `workdir`. That is the legitimate degenerate N=1 of one rule rather than
/// a special case.
///
/// Returns `Err` when an ancestor's `ckbadger.toml` is present but does not
/// parse. `is_orchestrator` having returned true proves a governing config
/// exists, so an unparseable one is a real error rather than an absent
/// orchestrator — and whether it governs `workdir` is unknowable without
/// parsing it. Guessing N=1 there would size every network to the whole host's
/// RAM, the exact over-commit this count exists to prevent, so an unreadable
/// config found anywhere in the walk fails fast instead of being skipped.
///
/// Never returns `Ok(0)`: each success path is either the literal 1 above or a
/// network count that `parse_orchestrator_config` has already rejected as empty.
/// Callers divide the detected host RAM by this without a zero check; see
/// `StoreRuntimeConfig::network_count`.
///
/// Ancestors are walked rather than only checking the immediate parent because a
/// `[[network]].dir` may be nested (e.g. `dir = "nets/mainnet"`).
pub fn co_resident_network_count(workdir: &Path) -> Result<usize> {
    // Canonicalize both sides so a relative `-C work/testnet` still matches the
    // orchestrator's resolved `root/<dir>`. A path that cannot be canonicalized
    // does not exist, so no orchestrator can be governing it.
    let Ok(target) = workdir.canonicalize() else {
        return Ok(1);
    };

    let mut cursor = target.as_path();
    while let Some(root) = cursor.parent() {
        if is_orchestrator(root) {
            // A governing config is present; only its contents can say whether it
            // lists `workdir`. Propagate rather than fall through to 1, which
            // would silently budget this network against the entire host.
            let orch = load_orchestrator_config(root).with_context(|| {
                format!(
                    "unreadable orchestrator config at {}: cannot determine how many networks share this host's RAM",
                    root.join("ckbadger.toml").display()
                )
            })?;
            let governs = orch.networks.iter().any(|entry| {
                network_workdir(root, entry)
                    .canonicalize()
                    .map(|resolved| resolved == target)
                    .unwrap_or(false)
            });
            if governs {
                return Ok(orch.networks.len());
            }
        }
        cursor = root;
    }

    Ok(1)
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

        assert_eq!(co_resident_network_count(&root.join("mainnet")).unwrap(), 2);
        assert_eq!(co_resident_network_count(&root.join("testnet")).unwrap(), 2);
    }

    #[test]
    fn co_resident_count_is_one_for_a_single_network_workdir() {
        let tmp = TempDir::new().unwrap();
        let workdir = tmp.path().join("work");
        std::fs::create_dir_all(&workdir).unwrap();
        std::fs::write(workdir.join("config.toml"), "").unwrap();

        // No orchestrator governs this workdir: the degenerate N=1 case, not an error.
        assert_eq!(co_resident_network_count(&workdir).unwrap(), 1);
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

        // Orchestrator present and valid, but does not list this workdir: the walk
        // continues past it and still reaches the degenerate N=1.
        assert_eq!(co_resident_network_count(&stray).unwrap(), 1);
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
        assert_eq!(
            co_resident_network_count(&root.join("nets/mainnet")).unwrap(),
            2
        );
    }

    #[test]
    fn co_resident_count_is_one_for_a_nonexistent_path() {
        let tmp = TempDir::new().unwrap();
        assert_eq!(
            co_resident_network_count(&tmp.path().join("missing")).unwrap(),
            1
        );
    }

    #[test]
    fn co_resident_count_errors_on_a_present_but_unreadable_orchestrator_config() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        // Present (so `is_orchestrator` is true) but not parseable: a governing
        // config exists and we cannot read it. Guessing 1 here would size this
        // network to the whole host's RAM — the over-commit this count prevents.
        std::fs::write(
            root.join("ckbadger.toml"),
            "[[network]\nname = \"mainnet\"\n",
        )
        .unwrap();
        let workdir = root.join("mainnet");
        std::fs::create_dir_all(&workdir).unwrap();

        let err = co_resident_network_count(&workdir).unwrap_err();
        let msg = format!("{err:#}");
        let offending = root.canonicalize().unwrap().join("ckbadger.toml");
        assert!(
            msg.contains(&offending.display().to_string()),
            "error must name the offending config path, got: {msg}"
        );
        assert!(
            msg.contains("unreadable"),
            "error must say the config is unreadable, got: {msg}"
        );
    }
}
