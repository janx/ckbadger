use axum::{
    extract::{Path as AxumPath, Query, State},
    routing::get,
    Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::response::{ok, ApiError, ApiResult};
use crate::AppState;
use ckbadger_store::network_keys::{bucket_of, Granularity, Metric};
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
        .route("/network/history", get(history))
        .route("/network/nodes", get(nodes))
        .route("/network/nodes/{peer_id}", get(node_by_id))
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

/// Query parameters for `/network/history`. `metric`/`granularity` are required
/// enum strings; `from`/`to` are optional inclusive unix-seconds bounds (omitted
/// bounds scan the whole series).
#[derive(Debug, Deserialize)]
pub struct HistoryQuery {
    pub metric: String,
    pub granularity: String,
    pub from: Option<u64>,
    pub to: Option<u64>,
}

/// One point in a metric trend series: `ts` is the bucket boundary in unix
/// seconds, `scalar` carries scalar metrics, and `buckets` carries the top-N
/// `(label, count)` slices for share metrics (empty for scalar metrics).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryPointResponse {
    pub ts: u64,
    pub scalar: u64,
    pub buckets: Vec<LabelCount>,
}

/// A single metric's trend series, echoing back the requested `metric` and
/// `granularity` alongside the resolved `points` (ascending by `ts`).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkHistory {
    pub metric: String,
    pub granularity: String,
    pub points: Vec<HistoryPointResponse>,
}

/// Map an API metric string to its store [`Metric`], or `None` if unknown.
fn parse_metric(s: &str) -> Option<Metric> {
    match s {
        "totalNodes" => Some(Metric::TotalNodes),
        "reachableNodes" => Some(Metric::ReachableNodes),
        "versionShare" => Some(Metric::VersionShare),
        "countryShare" => Some(Metric::CountryShare),
        _ => None,
    }
}

/// Map an API granularity string to its store [`Granularity`], or `None` if unknown.
fn parse_gran(s: &str) -> Option<Granularity> {
    match s {
        "hour" => Some(Granularity::Hour),
        "day" => Some(Granularity::Day),
        _ => None,
    }
}

/// Range-scan a `(metric, granularity)` history series from `CF_NET_STATS` into a
/// trend series. Unknown `metric`/`granularity` ⇒ `400`. When no network store is
/// configured (crawler opt-out), returns an empty `points` series rather than an error.
///
/// Daily series exclude the incomplete current day: when `granularity==day` and a
/// `to` bound is provided, the current-day bucket (`bucket_of(to, Day)`) is dropped.
async fn history(
    State(state): State<Arc<AppState>>,
    Query(q): Query<HistoryQuery>,
) -> ApiResult<NetworkHistory> {
    let metric = parse_metric(&q.metric)
        .ok_or_else(|| ApiError::bad_request(format!("unknown metric '{}'", q.metric)))?;
    let gran = parse_gran(&q.granularity)
        .ok_or_else(|| ApiError::bad_request(format!("unknown granularity '{}'", q.granularity)))?;
    let points = match &state.network_store {
        None => Vec::new(),
        Some(s) => {
            let from_b = q.from.map(|t| bucket_of(t, gran)).unwrap_or(0);
            let to_b = q.to.map(|t| bucket_of(t, gran)).unwrap_or(u64::MAX);
            let mut rows = s
                .scan_history(metric, gran, from_b, to_b)
                .map_err(|e| ApiError::internal(e.to_string()))?;
            // Daily series exclude the incomplete current day: drop the current-day
            // bucket (`bucket_of(to, Day)`, and anything at/after it) when `to` is given.
            if let (Granularity::Day, Some(to)) = (gran, q.to) {
                let cur = bucket_of(to, Granularity::Day);
                rows.retain(|(b, _)| *b < cur);
            }
            rows.into_iter()
                .map(|(b, p)| HistoryPointResponse {
                    ts: b * gran.seconds(),
                    scalar: p.scalar,
                    buckets: p
                        .buckets
                        .into_iter()
                        .map(|(label, count)| LabelCount { label, count })
                        .collect(),
                })
                .collect()
        }
    };
    ok(NetworkHistory {
        metric: q.metric,
        granularity: q.granularity,
        points,
    })
}

/// Query parameters for `/network/nodes`. All optional: `cursor` is the previous
/// page's last `peerId` (hex); `limit` is clamped to `1..=500` (default 50);
/// `reachable`/`country`/`version` narrow the result set. `country` matches the
/// resolved [`country_label`] (e.g. `"US"`, `"Unknown"`); `version` matches
/// `client_version` exactly.
#[derive(Debug, Deserialize)]
pub struct NodesQuery {
    pub cursor: Option<String>,
    pub limit: Option<usize>,
    pub reachable: Option<bool>,
    pub country: Option<String>,
    pub version: Option<String>,
}

/// One row in the paginated node table: the fields most useful for a list view.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeSummary {
    pub peer_id: String,
    pub addr: String,
    pub version: String,
    pub country: String,
    pub asn: String,
    pub reachable: bool,
    pub last_seen: u64,
    pub last_reachable_at: u64,
    pub rtt_ms: Option<u32>,
}

/// A page of the node table. `next_cursor` is the last item's `peerId` and is set
/// only when more rows remain after this page (otherwise `None`).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkNodesPage {
    pub items: Vec<NodeSummary>,
    pub next_cursor: Option<String>,
}

/// Lowercase hex-encode raw bytes (e.g. a `peer_id` for use as a cursor / id).
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// `GET /network/nodes` — filterable, cursor-paginated node table.
///
/// Filters (`reachable`/`country`/`version`) are ANDed; ordering is deterministic
/// (`last_seen` desc, then raw `peer_id` asc). The cursor is the hex `peer_id` of
/// the previous page's last row; paging resumes at the row after it. When no
/// network store is configured (crawler opt-out) this returns an empty page rather
/// than an error.
async fn nodes(
    State(state): State<Arc<AppState>>,
    Query(q): Query<NodesQuery>,
) -> ApiResult<NetworkNodesPage> {
    let limit = q.limit.unwrap_or(50).clamp(1, 500);
    let mut rows = match &state.network_store {
        Some(s) => s
            .scan_nodes()
            .map_err(|e| ApiError::internal(e.to_string()))?,
        None => Vec::new(),
    };
    // Apply the (ANDed) filters. Absent filter ⇒ matches everything.
    rows.retain(|(_, r)| {
        q.reachable.is_none_or(|want| r.reachable == want)
            && q.country.as_deref().is_none_or(|c| country_label(r) == c)
            && q.version.as_deref().is_none_or(|v| r.client_version == v)
    });
    // Deterministic order: last_seen desc, then peer_id asc (stable tiebreak).
    rows.sort_by(|(ka, a), (kb, b)| b.last_seen.cmp(&a.last_seen).then_with(|| ka.cmp(kb)));
    // Resume after the cursor's peer_id (unknown/stale cursor ⇒ start from the top).
    let start = match &q.cursor {
        Some(c) => rows
            .iter()
            .position(|(k, _)| hex(k) == *c)
            .map(|i| i + 1)
            .unwrap_or(0),
        None => 0,
    };
    let slice = &rows[start.min(rows.len())..];
    let items: Vec<NodeSummary> = slice
        .iter()
        .take(limit)
        .map(|(k, r)| NodeSummary {
            peer_id: hex(k),
            addr: r.own_addrs.first().cloned().unwrap_or_default(),
            version: r.client_version.clone(),
            country: country_label(r),
            asn: asn_label(r),
            reachable: r.reachable,
            last_seen: r.last_seen,
            last_reachable_at: r.last_reachable_at,
            rtt_ms: r.last_rtt_ms,
        })
        .collect();
    // More rows remain iff the post-cursor slice is longer than this page.
    let next_cursor = if slice.len() > limit {
        items.last().map(|n| n.peer_id.clone())
    } else {
        None
    };
    ok(NetworkNodesPage { items, next_cursor })
}

/// Full detail for a single node, aimed at API/agent consumers.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeDetailResponse {
    pub peer_id: String,
    pub own_addrs: Vec<String>,
    pub client_version: String,
    pub flags: u64,
    pub protocols: Vec<String>,
    pub first_seen: u64,
    pub last_seen: u64,
    pub last_reachable_at: u64,
    pub reachable: bool,
    pub country: String,
    pub asn: String,
    pub rtt_ms: Option<u32>,
    pub known_peers: usize,
}

/// `GET /network/nodes/{peer_id}` — single node detail. `peer_id` is hex; malformed
/// hex ⇒ `400`. Absent node (including crawler opt-out, where there is no store to
/// read) ⇒ `404`. Store read errors surface as `500` rather than masquerading as `404`.
async fn node_by_id(
    State(state): State<Arc<AppState>>,
    AxumPath(peer_hex): AxumPath<String>,
) -> ApiResult<NodeDetailResponse> {
    let peer = hex::decode(&peer_hex).map_err(|_| ApiError::bad_request("peerId must be hex"))?;
    let rec = match &state.network_store {
        Some(s) => s
            .get_node(&peer)
            .map_err(|e| ApiError::internal(e.to_string()))?,
        None => None,
    };
    let rec = rec.ok_or_else(|| ApiError::not_found("node not found"))?;
    // Resolve borrow-dependent labels before moving fields out of `rec`.
    let country = country_label(&rec);
    let asn = asn_label(&rec);
    let known_peers = rec.known_peers.len();
    ok(NodeDetailResponse {
        peer_id: peer_hex,
        own_addrs: rec.own_addrs,
        client_version: rec.client_version,
        flags: rec.flags,
        protocols: rec.protocols,
        first_seen: rec.first_seen,
        last_seen: rec.last_seen,
        last_reachable_at: rec.last_reachable_at,
        reachable: rec.reachable,
        country,
        asn,
        rtt_ms: rec.last_rtt_ms,
        known_peers,
    })
}
