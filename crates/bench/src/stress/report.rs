use std::path::Path;

use anyhow::Result;
use serde::Serialize;

use super::collector::StageResult;
use super::scenario::Scenario;

// ---------------------------------------------------------------------------
// ScenarioReport — collected results for one scenario
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScenarioReport {
    pub scenario: Scenario,
    pub stage_results: Vec<StageResult>,
    pub soft_degradation_vus: Option<u32>,
    pub breaking_point_vus: Option<u32>,
}

// ---------------------------------------------------------------------------
// StressConfig — serializable run configuration
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StressConfig {
    pub scenarios: String,
    pub stage_duration_secs: u64,
    pub auto_ramp: bool,
    pub think_time_ms: String,
    pub timeout_ms: u64,
}

// ---------------------------------------------------------------------------
// StressReport — top-level report containing all scenarios
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StressReport {
    pub timestamp: String,
    pub target: String,
    pub config: StressConfig,
    pub scenarios: Vec<ScenarioReport>,
}

// ---------------------------------------------------------------------------
// Output stubs (to be implemented in Task 6)
// ---------------------------------------------------------------------------

pub fn print_tables(_report: &StressReport) {
    eprintln!("(stress report tables not yet implemented)");
}

pub fn print_json(_report: &StressReport) -> Result<()> {
    let json = serde_json::to_string_pretty(_report)?;
    println!("{json}");
    Ok(())
}

pub fn save_json(_report: &StressReport, _path: &Path) -> Result<()> {
    eprintln!("(stress report save not yet implemented)");
    Ok(())
}
