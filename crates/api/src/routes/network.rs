use axum::{extract::State, routing::get, Router};
use serde::Serialize;
use std::sync::Arc;

use crate::response::{ok, ApiError, ApiResult};
use crate::AppState;
use ckbadger_store::{LatestStatus, NodeRecord};

/// Latest crawl-round status, camelCase-serialized for the API.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LatestStatusResponse {
    pub round_id: u64,
    pub started: u64,
    pub finished: u64,
    pub dialed: u64,
    pub reachable: u64,
    pub unreachable: u64,
    pub foreign_dropped: u64,
    pub new_nodes: u64,
    pub total_known: u64,
    pub frontier_drained: bool,
}

impl From<LatestStatus> for LatestStatusResponse {
    fn from(s: LatestStatus) -> Self {
        Self {
            round_id: s.round_id,
            started: s.started,
            finished: s.finished,
            dialed: s.dialed,
            reachable: s.reachable,
            unreachable: s.unreachable,
            foreign_dropped: s.foreign_dropped,
            new_nodes: s.new_nodes,
            total_known: s.total_known,
            frontier_drained: s.frontier_drained,
        }
    }
}

/// Top-level network crawler summary: whether the crawler is enabled, whether any
/// data has been persisted yet, and the most recent completed round (if any).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkSummary {
    pub enabled: bool,
    pub has_data: bool,
    pub last_round: Option<LatestStatusResponse>,
}

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/network/summary", get(summary))
        .route("/network/distributions", get(distributions))
}

async fn summary(State(state): State<Arc<AppState>>) -> ApiResult<NetworkSummary> {
    let last_round = match &state.network_store {
        Some(s) => s
            .get_network_status()
            .map_err(|e| ApiError::internal(e.to_string()))?
            .map(Into::into),
        None => None,
    };
    ok(NetworkSummary {
        enabled: state.crawler_enabled,
        has_data: last_round.is_some(),
        last_round,
    })
}

/// A single `(label, count)` bucket in a distribution histogram.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LabelCount {
    pub label: String,
    pub count: u64,
}

/// Aggregated distributions over all currently-known nodes (`CF_NET_NODES`).
///
/// `total_known` is the node count; `reachable`/`unreachable` partition it by the
/// per-node `reachable` flag. Each histogram is sorted by count desc, then label asc.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkDistributions {
    pub total_known: u64,
    pub reachable: u64,
    pub unreachable: u64,
    pub versions: Vec<LabelCount>,
    pub countries: Vec<LabelCount>,
    pub asns: Vec<LabelCount>,
    pub protocols: Vec<LabelCount>,
}

/// Count occurrences of each label, returning buckets sorted by count desc then
/// label asc (deterministic ordering). Shared with the nodes endpoint (Task 5).
fn histogram<I: IntoIterator<Item = String>>(labels: I) -> Vec<LabelCount> {
    use std::collections::HashMap;
    let mut m: HashMap<String, u64> = HashMap::new();
    for l in labels {
        *m.entry(l).or_insert(0) += 1;
    }
    let mut v: Vec<LabelCount> = m
        .into_iter()
        .map(|(label, count)| LabelCount { label, count })
        .collect();
    v.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.label.cmp(&b.label)));
    v
}

/// Resolve a node's country label: ISO code when geo is present and non-empty,
/// otherwise `"Unknown"`. Missing geo and empty country collapse to the same label.
fn country_label(rec: &NodeRecord) -> String {
    match rec.geo.as_ref().map(|g| g.country.as_str()) {
        Some(c) if !c.is_empty() => c.to_string(),
        _ => "Unknown".to_string(),
    }
}

/// Resolve a node's ASN label as `"AS<number> <org>"`, or `"Unknown"` when absent.
fn asn_label(rec: &NodeRecord) -> String {
    match &rec.asn {
        Some(a) => format!("AS{} {}", a.number, a.org),
        None => "Unknown".to_string(),
    }
}

async fn distributions(State(state): State<Arc<AppState>>) -> ApiResult<NetworkDistributions> {
    let nodes = match &state.network_store {
        Some(s) => s
            .scan_nodes()
            .map_err(|e| ApiError::internal(e.to_string()))?,
        None => Vec::new(),
    };
    let total_known = nodes.len() as u64;
    let reachable = nodes.iter().filter(|(_, r)| r.reachable).count() as u64;
    ok(NetworkDistributions {
        total_known,
        reachable,
        unreachable: total_known - reachable,
        versions: histogram(nodes.iter().map(|(_, r)| r.client_version.clone())),
        countries: histogram(nodes.iter().map(|(_, r)| country_label(r))),
        asns: histogram(nodes.iter().map(|(_, r)| asn_label(r))),
        protocols: histogram(nodes.iter().flat_map(|(_, r)| r.protocols.clone())),
    })
}
