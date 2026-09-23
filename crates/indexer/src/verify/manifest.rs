//! What a chain-derived run actually covered, written next to its report.
//!
//! The report says whether the checks passed. The manifest says what they were
//! allowed to look at: which anchor, which source, which entities, what the
//! budgets were, how much of them was spent, and — the part that matters most —
//! what was *not* covered and why. Without it "sampling passed" is unauditable:
//! a run that verified one entity and a run that verified sixteen produce the
//! same green line.
//!
//! Like the report, it is written to a temp file and renamed, so an interrupted
//! run never leaves a partial manifest that could be read as complete evidence.

use std::path::{Path, PathBuf};

use anyhow::Context as _;

use super::checks::EntitySelector;
use super::report::{SourceProfileReport, REPORT_SCHEMA_VERSION};

/// How completely one selected entity was verified.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntityCoverage {
    pub selector: EntitySelector,
    pub complete: bool,
    /// Chain history records folded into the expectation.
    pub history_records: usize,
    pub differences: usize,
    /// Why this entity was not fully covered, when it was not.
    pub uncovered_reason: Option<String>,
}

impl EntityCoverage {
    pub fn complete(selector: &EntitySelector, history_records: usize, differences: usize) -> Self {
        Self {
            selector: selector.clone(),
            complete: true,
            history_records,
            differences,
            uncovered_reason: None,
        }
    }

    pub fn incomplete(selector: &EntitySelector, reason: String) -> Self {
        Self {
            selector: selector.clone(),
            complete: false,
            history_records: 0,
            differences: 0,
            uncovered_reason: Some(reason),
        }
    }
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifyManifest {
    pub schema_version: u32,
    pub network: String,
    pub anchor_block_number: u64,
    pub anchor_block_hash: String,
    pub source_profile: SourceProfileReport,
    /// First block the qualified source's index covers.
    pub index_start_block: u64,
    pub budget_records: usize,
    pub budget_rpc_requests: usize,
    pub budget_seconds: u64,
    pub history_records: usize,
    pub rpc_requests: usize,
    pub entities: Vec<EntityCoverage>,
}

impl VerifyManifest {
    pub fn new(
        network: &str,
        anchor_block_number: u64,
        anchor_block_hash: &str,
        source_profile: SourceProfileReport,
    ) -> Self {
        Self {
            schema_version: REPORT_SCHEMA_VERSION,
            network: network.to_string(),
            anchor_block_number,
            anchor_block_hash: anchor_block_hash.to_string(),
            source_profile,
            index_start_block: 0,
            budget_records: 0,
            budget_rpc_requests: 0,
            budget_seconds: 0,
            history_records: 0,
            rpc_requests: 0,
            entities: Vec::new(),
        }
    }

    /// Write `<dir>/manifest.json` atomically.
    pub fn write(&self, dir: &Path) -> anyhow::Result<PathBuf> {
        std::fs::create_dir_all(dir)
            .with_context(|| format!("creating manifest dir {}", dir.display()))?;
        let final_path = dir.join("manifest.json");
        let temp_path = dir.join(format!("manifest.json.{}.partial", std::process::id()));
        let body = serde_json::to_string_pretty(self).context("serializing the verify manifest")?;
        std::fs::write(&temp_path, body)
            .with_context(|| format!("writing {}", temp_path.display()))?;
        std::fs::rename(&temp_path, &final_path).with_context(|| {
            format!(
                "renaming {} to {}",
                temp_path.display(),
                final_path.display()
            )
        })?;
        Ok(final_path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn selector(id: &str) -> EntitySelector {
        EntitySelector {
            kind: "token".to_string(),
            id: id.to_string(),
        }
    }

    fn manifest() -> VerifyManifest {
        VerifyManifest::new(
            "mainnet",
            1_000,
            "0xabc",
            SourceProfileReport {
                status: "qualified".to_string(),
                reason: None,
                genesis_hash: Some("0xgen".to_string()),
                node_version: Some("0.119.0".to_string()),
                indexer_tip: Some(1_100),
                declaration_path: Some("/w/verify-source.toml".to_string()),
                provenance: Some("built from genesis".to_string()),
                block_filter: None,
                cell_filter: None,
            },
        )
    }

    #[test]
    fn an_uncovered_entity_records_why() {
        let mut manifest = manifest();
        manifest
            .entities
            .push(EntityCoverage::complete(&selector("0x01"), 42, 0));
        manifest.entities.push(EntityCoverage::incomplete(
            &selector("0x02"),
            "budget exhausted".to_string(),
        ));

        let json = serde_json::to_value(&manifest).unwrap();
        assert_eq!(json["entities"][0]["complete"], true);
        assert_eq!(json["entities"][0]["historyRecords"], 42);
        assert_eq!(json["entities"][1]["complete"], false);
        assert_eq!(json["entities"][1]["uncoveredReason"], "budget exhausted");
        assert_eq!(json["sourceProfile"]["status"], "qualified");
        assert_eq!(json["anchorBlockNumber"], 1_000);
    }

    #[test]
    fn a_manifest_lands_atomically_with_no_partial_left_behind() {
        let dir = tempfile::tempdir().unwrap();
        let path = manifest().write(dir.path()).unwrap();

        assert_eq!(path, dir.path().join("manifest.json"));
        let leftovers: Vec<String> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|name| name != "manifest.json")
            .collect();
        assert!(leftovers.is_empty(), "{leftovers:?}");

        let written: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(written["schemaVersion"], 1);
        assert_eq!(written["network"], "mainnet");
    }
}
