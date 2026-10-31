use std::collections::BTreeMap;
use std::sync::Arc;

use axum::{
    extract::{Path as AxumPath, Query, State},
    routing::get,
    Router,
};
use serde::{Deserialize, Serialize};

use chrono::NaiveDate;
use ckbadger_common::stats_bucket::{stats_day, utc8_day_end_ms, DAY_MS};
use ckbadger_store::network_keys::{bucket_of, Granularity, Metric};
use ckbadger_store::{
    ActiveCandidateState, AddressObservationHistogram, AddressProbeEvidence, AddressProbeResult,
    CrawlCandidate, CrawlProgress, DirectSessionEvidence, DirectSessionObservationSummary,
    DiscoveryEvidence, HistoryPoint, LatestStatus, LocalObserverEvidence, NodeRecord,
    SessionInitiator,
};

use crate::response::{ok, ApiError, ApiResult, ApiRouteError};
use crate::AppState;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RetentionSplitResponse {
    pub with_retained_verification: u64,
    pub without_retained_verification: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PeerOutcomesResponse {
    pub same_network_identified: u64,
    pub exhausted: RetentionSplitResponse,
    pub foreign_network: RetentionSplitResponse,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LatestStatusResponse {
    pub round_id: u64,
    pub started_at: u64,
    pub finished_at: u64,
    pub candidate_peers: u64,
    pub verified_retained_peers: u64,
    pub reachable_peers: u64,
    pub verified_unavailable_peers: u64,
    pub exhausted_candidates: u64,
    pub foreign_peers: u64,
    pub address_attempts: u64,
    pub non_successful_address_attempts: u64,
    pub malformed_addresses: u64,
    pub new_verified_peers: u64,
    pub peer_outcomes: PeerOutcomesResponse,
    pub address_observations: AddressObservationHistogramResponse,
    pub discovery: DiscoveryEvidenceResponse,
    pub local_observer: Option<LocalObserverEvidenceResponse>,
    pub direct_session_observations: DirectSessionObservationSummaryResponse,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalObserverProtocolResponse {
    pub id: u64,
    pub name: String,
    pub support_versions: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalObserverEvidenceResponse {
    pub peer_id: String,
    pub first_observed_at: u64,
    pub last_observed_at: u64,
    pub first_observed_round: u64,
    pub last_observed_round: u64,
    pub observation_count: u64,
    pub client_version: String,
    pub active: bool,
    pub addresses: Vec<String>,
    pub protocols: Vec<LocalObserverProtocolResponse>,
    pub connections: u64,
}

impl From<LocalObserverEvidence> for LocalObserverEvidenceResponse {
    fn from(evidence: LocalObserverEvidence) -> Self {
        Self {
            peer_id: hex(&evidence.peer_id),
            first_observed_at: evidence.first_observed_at,
            last_observed_at: evidence.last_observed_at,
            first_observed_round: evidence.first_observed_round,
            last_observed_round: evidence.last_observed_round,
            observation_count: evidence.observation_count,
            client_version: evidence.client_version,
            active: evidence.active,
            addresses: evidence.addresses,
            protocols: evidence
                .protocols
                .into_iter()
                .map(|protocol| LocalObserverProtocolResponse {
                    id: protocol.id,
                    name: protocol.name,
                    support_versions: protocol.support_versions,
                })
                .collect(),
            connections: evidence.connections,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectSessionObservationSummaryResponse {
    pub observer_initiated: u64,
    pub peer_initiated: u64,
}

impl From<DirectSessionObservationSummary> for DirectSessionObservationSummaryResponse {
    fn from(summary: DirectSessionObservationSummary) -> Self {
        Self {
            observer_initiated: summary.observer_initiated,
            peer_initiated: summary.peer_initiated,
        }
    }
}

impl TryFrom<LatestStatus> for LatestStatusResponse {
    type Error = anyhow::Error;

    fn try_from(status: LatestStatus) -> Result<Self, Self::Error> {
        let round_id = status.round_id;
        Ok(Self {
            round_id,
            started_at: status.started,
            finished_at: status.finished,
            candidate_peers: status.peer_outcomes.candidate_peers(round_id)?,
            verified_retained_peers: status.peer_outcomes.verified_retained_peers(round_id)?,
            reachable_peers: status.peer_outcomes.reachable_peers(),
            verified_unavailable_peers: status
                .peer_outcomes
                .verified_unavailable_peers(round_id)?,
            exhausted_candidates: status.peer_outcomes.exhausted_candidates(round_id)?,
            foreign_peers: status.peer_outcomes.foreign_peers(round_id)?,
            address_attempts: status.address_observations.address_attempts(round_id)?,
            non_successful_address_attempts: status
                .address_observations
                .non_successful_address_attempts(round_id)?,
            malformed_addresses: status.malformed_addresses,
            new_verified_peers: status.new_verified_peers,
            peer_outcomes: PeerOutcomesResponse {
                same_network_identified: status.peer_outcomes.same_network_identified,
                exhausted: RetentionSplitResponse {
                    with_retained_verification: status
                        .peer_outcomes
                        .exhausted_with_retained_verification,
                    without_retained_verification: status
                        .peer_outcomes
                        .exhausted_without_retained_verification,
                },
                foreign_network: RetentionSplitResponse {
                    with_retained_verification: status
                        .peer_outcomes
                        .foreign_with_retained_verification,
                    without_retained_verification: status
                        .peer_outcomes
                        .foreign_without_retained_verification,
                },
            },
            address_observations: status.address_observations.into(),
            discovery: status.discovery.into(),
            local_observer: status.local_observer.map(Into::into),
            direct_session_observations: status.direct_session_observations.into(),
        })
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActiveCrawlResponse {
    pub round_id: u64,
    pub started_at: u64,
    pub last_checkpoint_at: u64,
    pub candidate_peers: u64,
    pub completed_peers: u64,
    pub address_attempts: u64,
    pub blocked_reason: Option<String>,
}

impl From<CrawlProgress> for ActiveCrawlResponse {
    fn from(progress: CrawlProgress) -> Self {
        Self {
            round_id: progress.round_id,
            started_at: progress.started_at,
            last_checkpoint_at: progress.last_checkpoint_at,
            candidate_peers: progress.candidate_peers,
            completed_peers: progress.completed_peers,
            address_attempts: progress.address_attempts,
            blocked_reason: progress.blocked_reason,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkSummary {
    pub enabled: bool,
    pub has_data: bool,
    pub last_round: Option<LatestStatusResponse>,
    pub active_round: Option<ActiveCrawlResponse>,
}

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/network/summary", get(summary))
        .route("/network/distributions", get(distributions))
        .route("/network/map", get(map))
        .route("/network/history", get(history))
        .route("/network/advertisements", get(advertisements))
        .route("/network/peers", get(peers))
        .route("/network/peers/{peer_id}", get(peer_by_id))
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkMapPoint {
    pub peer_id: String,
    pub country: String,
    pub lat: f64,
    pub lon: f64,
    pub reachable: bool,
    pub client_version: String,
    pub asn: Option<String>,
    pub first_seen: u64,
    pub last_seen: u64,
    pub advertiser_count: u64,
    pub local_session_observed: bool,
    /// This node's session with the peer in the latest completed round, as
    /// the crawler read it from the node's `get_peers`; present exactly when
    /// `localSessionObserved` (R-13 / CH-57).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub local_session: Option<LocalSessionResponse>,
    /// The dial alias this crawler last identified the node through; absent
    /// when no retained alias was ever verified (R-13 / CH-57).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alias: Option<VerifiedAliasResponse>,
}

/// One session of the configured node with a peer, exactly as the node's
/// `get_peers` reported it when the crawler read it (network store only;
/// the API makes no RPC call for it).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalSessionResponse {
    pub connected_duration_ms: u64,
    /// Absent when the node reported no ping for the session.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_ping_duration_ms: Option<u64>,
    /// When the crawler read `get_peers` (the crawler clock, as `lastSeen`);
    /// `connectedDurationMs` is as of this time.
    pub observed_at: u64,
    pub initiator: &'static str,
}

/// A dial alias of a peer and when this crawler last identified the node
/// through it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifiedAliasResponse {
    pub address: String,
    pub verified_at: u64,
}

/// The configured node's session with `peer_id` in the latest completed
/// round. The crawler reads the node's `get_peers` once per round and a peer
/// appears in it at most once, so two such sessions are an invariant
/// violation.
fn latest_local_session<'a>(
    peer_id: &[u8],
    candidate: &'a CrawlCandidate,
    status: Option<&LatestStatus>,
) -> Result<Option<&'a DirectSessionEvidence>, ApiRouteError> {
    let Some((round_id, observer)) = status.and_then(|status| {
        status
            .local_observer
            .as_ref()
            .map(|observer| (status.round_id, observer))
    }) else {
        return Ok(None);
    };
    let mut sessions = candidate.direct_sessions.iter().filter(|session| {
        session.last_observed_round == round_id && session.observer_peer_id == observer.peer_id
    });
    let session = sessions.next();
    if sessions.next().is_some() {
        return Err(ApiError::internal(format!(
            "peer has more than one session with the local observer in round {round_id}: peer_id={}, observer_peer_id={}",
            hex(peer_id),
            hex(&observer.peer_id)
        )));
    }
    Ok(session)
}

/// The alias verified last; aliases verified at the same instant are equally
/// true, so among them the smallest address is shown.
fn latest_verified_alias(candidate: &CrawlCandidate) -> Option<VerifiedAliasResponse> {
    candidate
        .addresses
        .iter()
        .filter_map(|address| {
            address
                .last_verified_at
                .map(|verified_at| (verified_at, &address.addr))
        })
        .max_by(|(left_at, left), (right_at, right)| {
            left_at.cmp(right_at).then_with(|| right.cmp(left))
        })
        .map(|(verified_at, address)| VerifiedAliasResponse {
            address: address.clone(),
            verified_at,
        })
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkMap {
    pub round_id: Option<u64>,
    pub verified_retained: u64,
    pub located: u64,
    pub location_unavailable: u64,
    pub points: Vec<NetworkMapPoint>,
}

async fn map(State(state): State<Arc<AppState>>) -> ApiResult<NetworkMap> {
    let Some(store) = state.network_store.load_full() else {
        return ok(NetworkMap {
            round_id: None,
            verified_retained: 0,
            located: 0,
            location_unavailable: 0,
            points: Vec::new(),
        });
    };
    let nodes = store
        .scan_nodes()
        .map_err(|error| ApiError::internal(error.to_string()))?;
    let status = store
        .get_network_status()
        .map_err(|error| ApiError::internal(error.to_string()))?;
    if !nodes.is_empty() && status.is_none() {
        return Err(ApiError::internal(
            "network nodes exist without a completed crawl status",
        ));
    }
    let candidates: BTreeMap<Vec<u8>, CrawlCandidate> = store
        .scan_crawl_candidates()
        .map_err(|error| ApiError::internal(error.to_string()))?
        .into_iter()
        .collect();
    let verified_retained = u64::try_from(nodes.len())
        .map_err(|_| ApiError::internal("network node count does not fit u64"))?;
    let mut points = Vec::new();
    for (peer_id, record) in nodes {
        let candidate = candidates.get(&peer_id).ok_or_else(|| {
            ApiError::internal(format!(
                "verified map peer lacks candidate evidence: peer_id={}",
                hex(&peer_id)
            ))
        })?;
        let completed_round = status.as_ref().map(|status| status.round_id);
        let advertisers: std::collections::BTreeSet<_> = candidate
            .advertisements
            .iter()
            .filter(|evidence| Some(evidence.last_observed_round) == completed_round)
            .map(|evidence| &evidence.advertiser_peer_id)
            .collect();
        let advertiser_count = u64::try_from(advertisers.len()).map_err(|_| {
            ApiError::internal(format!(
                "advertiser count overflow: peer_id={}",
                hex(&peer_id)
            ))
        })?;
        let local_session = latest_local_session(&peer_id, candidate, status.as_ref())?;
        let local_session_observed = local_session.is_some();
        let local_session = local_session.map(|session| LocalSessionResponse {
            connected_duration_ms: session.connected_duration_ms,
            last_ping_duration_ms: session.last_ping_duration_ms,
            observed_at: session.last_observed_at,
            initiator: session_initiator_name(session.initiator),
        });
        let alias = latest_verified_alias(candidate);
        if let Some(geo) = record.geo {
            match (geo.lat, geo.lon) {
                (Some(lat), Some(lon))
                    if lat.is_finite()
                        && lon.is_finite()
                        && (-90.0..=90.0).contains(&lat)
                        && (-180.0..=180.0).contains(&lon) =>
                {
                    points.push(NetworkMapPoint {
                        peer_id: hex(&peer_id),
                        country: geo.country,
                        lat,
                        lon,
                        reachable: record.reachable,
                        client_version: record.client_version,
                        asn: record
                            .asn
                            .map(|asn| format!("AS{} {}", asn.number, asn.org)),
                        first_seen: record.first_seen,
                        last_seen: record.last_seen,
                        advertiser_count,
                        local_session_observed,
                        local_session,
                        alias,
                    });
                }
                (None, None) => {}
                _ => {
                    return Err(ApiError::internal(format!(
                        "network geo coordinates invalid or incomplete: peer_id={}",
                        hex(&peer_id)
                    )))
                }
            }
        }
    }
    let located = u64::try_from(points.len())
        .map_err(|_| ApiError::internal("located network node count does not fit u64"))?;
    let location_unavailable = verified_retained
        .checked_sub(located)
        .ok_or_else(|| ApiError::internal("located network nodes exceed verified nodes"))?;
    ok(NetworkMap {
        round_id: status.map(|status| status.round_id),
        verified_retained,
        located,
        location_unavailable,
        points,
    })
}

/// Retained positive Discovery observations whose last observation was in the
/// latest completed round. This is address-book gossip, never a connection map.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RoundAdvertisement {
    pub advertiser_peer_id: String,
    pub target_peer_id: String,
    pub alias: String,
    pub observed_at: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RoundAdvertisements {
    pub round_id: Option<u64>,
    pub finished_at: Option<u64>,
    pub coverage: &'static str,
    pub edges: Vec<RoundAdvertisement>,
    pub distinct_peer_pairs: u64,
}

async fn advertisements(State(state): State<Arc<AppState>>) -> ApiResult<RoundAdvertisements> {
    let Some(store) = state.network_store.load_full() else {
        return ok(RoundAdvertisements {
            round_id: None,
            finished_at: None,
            coverage: "noCompletedRound",
            edges: Vec::new(),
            distinct_peer_pairs: 0,
        });
    };
    let Some(status) = store
        .get_network_status()
        .map_err(|error| ApiError::internal(error.to_string()))?
    else {
        return ok(RoundAdvertisements {
            round_id: None,
            finished_at: None,
            coverage: "noCompletedRound",
            edges: Vec::new(),
            distinct_peer_pairs: 0,
        });
    };
    let mut edges = Vec::new();
    for (target_peer_id, candidate) in store
        .scan_crawl_candidates()
        .map_err(|error| ApiError::internal(error.to_string()))?
    {
        for evidence in candidate.advertisements {
            if evidence.last_observed_round > status.round_id {
                return Err(ApiError::internal(format!(
                    "advertisement exceeds completed round: target_peer_id={}, evidence_round={}, completed_round={}",
                    hex(&target_peer_id), evidence.last_observed_round, status.round_id
                )));
            }
            if evidence.last_observed_round == status.round_id {
                if evidence.last_observed_at < status.started
                    || evidence.last_observed_at > status.finished
                {
                    return Err(ApiError::internal(format!(
                        "advertisement outside completed round clock: target_peer_id={}, evidence_round={}, observed_at={}, started={}, finished={}",
                        hex(&target_peer_id), status.round_id, evidence.last_observed_at,
                        status.started, status.finished
                    )));
                }
                edges.push(RoundAdvertisement {
                    advertiser_peer_id: hex(&evidence.advertiser_peer_id),
                    target_peer_id: hex(&target_peer_id),
                    alias: evidence.alias,
                    observed_at: evidence.last_observed_at,
                });
            }
        }
    }
    edges.sort_by(|left, right| {
        (&left.advertiser_peer_id, &left.target_peer_id, &left.alias).cmp(&(
            &right.advertiser_peer_id,
            &right.target_peer_id,
            &right.alias,
        ))
    });
    let distinct_peer_pairs = u64::try_from(
        edges
            .iter()
            .map(|edge| (&edge.advertiser_peer_id, &edge.target_peer_id))
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
    )
    .map_err(|_| ApiError::internal("advertisement pair count exceeds u64"))?;
    ok(RoundAdvertisements {
        round_id: Some(status.round_id),
        finished_at: Some(status.finished),
        coverage: "retainedLatestObservationOnly",
        edges,
        distinct_peer_pairs,
    })
}

async fn summary(State(state): State<Arc<AppState>>) -> ApiResult<NetworkSummary> {
    let network_store = state.network_store.load_full();
    let (last_round, active_round) = match network_store {
        Some(store) => {
            let last_round = store
                .get_network_status()
                .map_err(|error| ApiError::internal(error.to_string()))?
                .map(LatestStatusResponse::try_from)
                .transpose()
                .map_err(|error| ApiError::internal(error.to_string()))?;
            let active_round = store
                .get_crawl_progress()
                .map_err(|error| ApiError::internal(error.to_string()))?
                .map(Into::into);
            (last_round, active_round)
        }
        None => (None, None),
    };
    ok(NetworkSummary {
        enabled: state.crawler_enabled,
        has_data: last_round.is_some(),
        last_round,
        active_round,
    })
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LabelCount {
    pub label: String,
    pub count: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkDistributions {
    pub verified_retained: u64,
    pub same_network_reachable: u64,
    pub verified_unavailable: u64,
    pub versions: Vec<LabelCount>,
    pub countries: Vec<LabelCount>,
    pub asns: Vec<LabelCount>,
    pub protocols: Vec<LabelCount>,
}

fn histogram<I: IntoIterator<Item = String>>(labels: I) -> Result<Vec<LabelCount>, ApiRouteError> {
    use std::collections::HashMap;
    let mut counts: HashMap<String, u64> = HashMap::new();
    for label in labels {
        let count = counts.entry(label.clone()).or_insert(0);
        *count = count.checked_add(1).ok_or_else(|| {
            ApiError::internal(format!("network histogram overflow: label={label}"))
        })?;
    }
    let mut values: Vec<LabelCount> = counts
        .into_iter()
        .map(|(label, count)| LabelCount { label, count })
        .collect();
    values.sort_by(|left, right| {
        right
            .count
            .cmp(&left.count)
            .then_with(|| left.label.cmp(&right.label))
    });
    Ok(values)
}

fn country_label(record: &NodeRecord) -> String {
    match record.geo.as_ref().map(|geo| geo.country.as_str()) {
        Some(country) if !country.is_empty() => country.to_string(),
        _ => "Unknown".to_string(),
    }
}

fn asn_label(record: &NodeRecord) -> String {
    match &record.asn {
        Some(asn) => format!("AS{} {}", asn.number, asn.org),
        None => "Unknown".to_string(),
    }
}

async fn distributions(State(state): State<Arc<AppState>>) -> ApiResult<NetworkDistributions> {
    let nodes = match state.network_store.load_full() {
        Some(store) => store
            .scan_nodes()
            .map_err(|error| ApiError::internal(error.to_string()))?,
        None => Vec::new(),
    };
    let verified_retained = u64::try_from(nodes.len())
        .map_err(|_| ApiError::internal("verified peer count exceeds u64"))?;
    let same_network_reachable =
        u64::try_from(nodes.iter().filter(|(_, record)| record.reachable).count())
            .map_err(|_| ApiError::internal("reachable peer count exceeds u64"))?;
    let verified_unavailable = verified_retained
        .checked_sub(same_network_reachable)
        .ok_or_else(|| ApiError::internal("reachable peers exceed retained verified peers"))?;
    ok(NetworkDistributions {
        verified_retained,
        same_network_reachable,
        verified_unavailable,
        versions: histogram(
            nodes
                .iter()
                .map(|(_, record)| record.client_version.clone()),
        )?,
        countries: histogram(nodes.iter().map(|(_, record)| country_label(record)))?,
        asns: histogram(nodes.iter().map(|(_, record)| asn_label(record)))?,
        protocols: histogram(
            nodes
                .iter()
                .flat_map(|(_, record)| record.protocols.clone()),
        )?,
    })
}

#[derive(Debug, Deserialize)]
pub struct HistoryQuery {
    pub metric: String,
    pub granularity: String,
    pub from: Option<u64>,
    pub to: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryPointResponse {
    pub ts: u64,
    pub scalar: u64,
    pub buckets: Vec<LabelCount>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkHistory {
    pub metric: String,
    pub granularity: String,
    pub points: Vec<HistoryPointResponse>,
    /// Day-by-day retained observation coverage. A missing point is a gap,
    /// never a measurement of zero peers or a proven zero-round day.
    pub daily_coverage: Vec<DayCoverageResponse>,
    pub coverage_from: Option<u64>,
    pub coverage_to: Option<u64>,
    pub round_count_availability: &'static str,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DayCoverageResponse {
    pub ts: u64,
    pub has_retained_observation: bool,
    pub rounds_per_day: Option<u64>,
}

/// The UTC+8 calendar day (D2) of a unix-seconds instant, through the API's
/// one UTC+8 day boundary (`stats_bucket::stats_day`).
fn utc8_day_of_secs(secs: u64) -> anyhow::Result<NaiveDate> {
    let ms = secs
        .checked_mul(1_000)
        .and_then(|ms| i64::try_from(ms).ok())
        .ok_or_else(|| anyhow::anyhow!("time {secs} s does not fit a millisecond instant"))?;
    stats_day(ms)
}

/// Unix seconds of the UTC+8 midnight that starts `day`.
fn utc8_day_start_secs(day: NaiveDate) -> Result<u64, ApiRouteError> {
    utc8_day_end_ms(day)
        .ok()
        .and_then(|end_ms| end_ms.checked_sub(DAY_MS))
        .and_then(|start_ms| u64::try_from(start_ms / 1_000).ok())
        .ok_or_else(|| {
            ApiError::internal(format!(
                "network daily history: UTC+8 day {day} has no unix-seconds start"
            ))
        })
}

/// The day after `day`, as an error rather than a wrap at the calendar's end.
fn next_day(day: NaiveDate) -> Result<NaiveDate, ApiRouteError> {
    day.succ_opt()
        .ok_or_else(|| ApiError::internal(format!("network daily history: no day after {day}")))
}

/// Day-by-day coverage of `days` (the UTC+8 days that have a point), from
/// `start_day` through `end_day`.
fn daily_coverage(
    days: &BTreeMap<NaiveDate, ()>,
    start_day: NaiveDate,
    end_day: NaiveDate,
) -> Result<Vec<DayCoverageResponse>, ApiRouteError> {
    if start_day > end_day {
        return Ok(Vec::new());
    }
    let day_count = (end_day - start_day).num_days() + 1;
    if day_count > 10_000 {
        return Err(ApiError::bad_request(format!(
            "network daily coverage window is too wide: from_day={start_day}, to_day={end_day}, max_days=10000"
        )));
    }
    let mut coverage = Vec::new();
    let mut day = start_day;
    loop {
        coverage.push(DayCoverageResponse {
            ts: utc8_day_start_secs(day)?,
            has_retained_observation: days.contains_key(&day),
            rounds_per_day: None,
        });
        if day == end_day {
            break;
        }
        day = next_day(day)?;
    }
    Ok(coverage)
}

/// One history row as served: `ts` is the bucket start in unix seconds.
fn history_point_response(ts: u64, point: HistoryPoint) -> HistoryPointResponse {
    HistoryPointResponse {
        ts,
        scalar: point.scalar,
        buckets: point
            .buckets
            .into_iter()
            .map(|(label, count)| LabelCount { label, count })
            .collect(),
    }
}

/// Unix seconds of an hourly bucket's start.
fn hour_start_secs(metric: Metric, bucket: u64) -> Result<u64, ApiRouteError> {
    bucket.checked_mul(Granularity::Hour.seconds()).ok_or_else(|| {
        ApiError::internal(format!(
            "network history timestamp overflow: metric={:?}, granularity={:?}, bucket={}, seconds={}",
            metric,
            Granularity::Hour,
            bucket,
            Granularity::Hour.seconds()
        ))
    })
}

/// D2: the daily series by UTC+8 calendar day. Every stored point is the
/// snapshot of the last crawl round that completed in its bucket (each round
/// overwrites its hour and day bucket), and a UTC+8 day starts on a UTC hour
/// boundary, so a day's point is exactly its latest retained hourly point.
/// The crawler's UTC-day rows cannot be re-cut into UTC+8 days (the round
/// time inside a day is not stored) and are not read. Days run through the
/// last complete UTC+8 day by the wall clock; a day whose hours are no longer
/// retained has no point (a gap, never zero).
fn utc8_daily_history(
    store: Option<&ckbadger_store::CkbadgerStore>,
    metric: Metric,
    from: Option<u64>,
    to: Option<u64>,
) -> Result<(Vec<HistoryPointResponse>, Vec<DayCoverageResponse>), ApiRouteError> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|error| ApiError::internal(format!("system clock before UNIX epoch: {error}")))?
        .as_secs();
    let today = utc8_day_of_secs(now)
        .map_err(|error| ApiError::internal(format!("network daily history clock: {error:#}")))?;
    let last_complete_day = today.pred_opt().ok_or_else(|| {
        ApiError::internal(format!("network daily history: no day before {today}"))
    })?;
    let query_day = |secs: u64, name: &str| {
        utc8_day_of_secs(secs).map_err(|error| {
            ApiError::bad_request(format!("invalid network history `{name}`: {error:#}"))
        })
    };
    let from_day = from.map(|secs| query_day(secs, "from")).transpose()?;
    let end_day = match to {
        Some(secs) => query_day(secs, "to")?.min(last_complete_day),
        None => last_complete_day,
    };
    let mut by_day: BTreeMap<NaiveDate, (u64, HistoryPoint)> = BTreeMap::new();
    if let Some(store) = store {
        if from_day.is_none_or(|from_day| from_day <= end_day) {
            let first_hour = match from_day {
                Some(day) => utc8_day_start_secs(day)? / Granularity::Hour.seconds(),
                None => 0,
            };
            let last_hour = (utc8_day_start_secs(next_day(end_day)?)?
                / Granularity::Hour.seconds())
            .checked_sub(1)
            .ok_or_else(|| {
                ApiError::internal(format!(
                    "network daily history: day {end_day} ends at the epoch"
                ))
            })?;
            let rows = store
                .scan_history(metric, Granularity::Hour, first_hour, last_hour)
                .map_err(|error| ApiError::internal(error.to_string()))?;
            for (bucket, point) in rows {
                let ts = hour_start_secs(metric, bucket)?;
                let day = utc8_day_of_secs(ts).map_err(|error| {
                    ApiError::internal(format!(
                        "network history hour bucket {bucket} has no UTC+8 day: {error:#}"
                    ))
                })?;
                // The latest hour of the day holds the day's last round.
                if by_day.get(&day).is_none_or(|(hour, _)| *hour < bucket) {
                    by_day.insert(day, (bucket, point));
                }
            }
        }
    }
    let days: BTreeMap<NaiveDate, ()> = by_day.keys().map(|day| (*day, ())).collect();
    let points = by_day
        .into_iter()
        .map(|(day, (_, point))| Ok(history_point_response(utc8_day_start_secs(day)?, point)))
        .collect::<Result<Vec<_>, ApiRouteError>>()?;
    let coverage = match from_day.or_else(|| days.keys().next().copied()) {
        Some(start_day) => daily_coverage(&days, start_day, end_day)?,
        None => Vec::new(),
    };
    Ok((points, coverage))
}

fn parse_metric(metric: &str) -> Option<Metric> {
    match metric {
        "verifiedPeers" => Some(Metric::VerifiedPeers),
        "reachablePeers" => Some(Metric::ReachablePeers),
        "versionShare" => Some(Metric::VersionShare),
        "countryShare" => Some(Metric::CountryShare),
        _ => None,
    }
}

fn parse_granularity(granularity: &str) -> Option<Granularity> {
    match granularity {
        "hour" => Some(Granularity::Hour),
        "day" => Some(Granularity::Day),
        _ => None,
    }
}

async fn history(
    State(state): State<Arc<AppState>>,
    Query(query): Query<HistoryQuery>,
) -> ApiResult<NetworkHistory> {
    let metric = parse_metric(&query.metric)
        .ok_or_else(|| ApiError::bad_request(format!("unknown metric '{}'", query.metric)))?;
    let granularity = parse_granularity(&query.granularity).ok_or_else(|| {
        ApiError::bad_request(format!("unknown granularity '{}'", query.granularity))
    })?;
    let store = state.network_store.load_full();
    let (points, daily_coverage) = match granularity {
        Granularity::Hour => {
            let points = match store.as_deref() {
                None => Vec::new(),
                Some(store) => {
                    let from = query
                        .from
                        .map(|timestamp| bucket_of(timestamp, granularity))
                        .unwrap_or(0);
                    let to = query
                        .to
                        .map(|timestamp| bucket_of(timestamp, granularity))
                        .unwrap_or(u64::MAX);
                    store
                        .scan_history(metric, granularity, from, to)
                        .map_err(|error| ApiError::internal(error.to_string()))?
                        .into_iter()
                        .map(|(bucket, point)| {
                            Ok(history_point_response(
                                hour_start_secs(metric, bucket)?,
                                point,
                            ))
                        })
                        .collect::<Result<Vec<_>, ApiRouteError>>()?
                }
            };
            (points, Vec::new())
        }
        Granularity::Day => utc8_daily_history(store.as_deref(), metric, query.from, query.to)?,
    };
    let coverage_from = daily_coverage.first().map(|day| day.ts);
    let coverage_to = daily_coverage.last().map(|day| day.ts);
    ok(NetworkHistory {
        metric: query.metric,
        granularity: query.granularity,
        points,
        daily_coverage,
        coverage_from,
        coverage_to,
        round_count_availability: "notRecorded",
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PeerDisplayState {
    Reachable,
    VerifiedUnavailable,
    AdvertisedUnverified,
    ForeignNetwork,
    NoCompletedObservation,
}

fn display_state(candidate: &CrawlCandidate, node: Option<&NodeRecord>) -> PeerDisplayState {
    match candidate.last_completed.as_ref() {
        None => PeerDisplayState::NoCompletedObservation,
        Some(evidence) => match evidence.outcome {
            ckbadger_store::CompletedCandidateOutcome::ForeignNetwork => {
                PeerDisplayState::ForeignNetwork
            }
            ckbadger_store::CompletedCandidateOutcome::SameNetworkIdentified => {
                PeerDisplayState::Reachable
            }
            ckbadger_store::CompletedCandidateOutcome::Exhausted if node.is_some() => {
                PeerDisplayState::VerifiedUnavailable
            }
            ckbadger_store::CompletedCandidateOutcome::Exhausted => {
                PeerDisplayState::AdvertisedUnverified
            }
        },
    }
}

fn address_result_name(result: AddressProbeResult) -> &'static str {
    match result {
        AddressProbeResult::DialRequestFailed => "dialRequestFailed",
        AddressProbeResult::NoAuthenticatedSessionBeforeDeadline => {
            "noAuthenticatedSessionBeforeDeadline"
        }
        AddressProbeResult::AuthenticatedSessionWithoutIdentifyBeforeDeadline => {
            "authenticatedSessionWithoutIdentifyBeforeDeadline"
        }
        AddressProbeResult::MalformedIdentify => "malformedIdentify",
        AddressProbeResult::ForeignNetwork => "foreignNetwork",
        AddressProbeResult::SameNetworkIdentified => "sameNetworkIdentified",
    }
}

fn parse_display_state_filter(
    state: Option<&str>,
) -> Result<Option<PeerDisplayState>, ApiRouteError> {
    state
        .map(|state| match state {
            "reachable" => Ok(PeerDisplayState::Reachable),
            "verifiedUnavailable" => Ok(PeerDisplayState::VerifiedUnavailable),
            "advertisedUnverified" => Ok(PeerDisplayState::AdvertisedUnverified),
            "foreignNetwork" => Ok(PeerDisplayState::ForeignNetwork),
            "noCompletedObservation" => Ok(PeerDisplayState::NoCompletedObservation),
            _ => Err(ApiError::bad_request(format!(
                "unknown peer state filter: state={state}"
            ))),
        })
        .transpose()
}

fn parse_observation_filter(
    observation: Option<&str>,
) -> Result<Option<AddressProbeResult>, ApiRouteError> {
    observation
        .map(|observation| match observation {
            "dialRequestFailed" => Ok(AddressProbeResult::DialRequestFailed),
            "noAuthenticatedSessionBeforeDeadline" => {
                Ok(AddressProbeResult::NoAuthenticatedSessionBeforeDeadline)
            }
            "authenticatedSessionWithoutIdentifyBeforeDeadline" => {
                Ok(AddressProbeResult::AuthenticatedSessionWithoutIdentifyBeforeDeadline)
            }
            "malformedIdentify" => Ok(AddressProbeResult::MalformedIdentify),
            "foreignNetwork" => Ok(AddressProbeResult::ForeignNetwork),
            "sameNetworkIdentified" => Ok(AddressProbeResult::SameNetworkIdentified),
            _ => Err(ApiError::bad_request(format!(
                "unknown address observation filter: observation={observation}"
            ))),
        })
        .transpose()
}

#[derive(Debug, Deserialize)]
pub struct PeersQuery {
    pub cursor: Option<String>,
    pub limit: Option<usize>,
    pub state: Option<String>,
    pub observation: Option<String>,
    pub country: Option<String>,
    pub version: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PeerSummary {
    pub peer_id: String,
    /// Result of this crawler's dial/Identify path only. Direct-session and
    /// advertisement facts are exposed independently below.
    pub crawler_dial_state: PeerDisplayState,
    pub participation: ParticipationEvidenceResponse,
    pub session_initiators: Vec<String>,
    pub primary_addr: Option<String>,
    pub version: Option<String>,
    pub country: Option<String>,
    pub asn: Option<String>,
    pub last_advertised_at: Option<u64>,
    pub last_dial_observed_at: Option<u64>,
    pub latest_positive_observed_at: u64,
    pub last_reachable_at: Option<u64>,
    pub rtt_ms: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParticipationEvidenceResponse {
    pub discovery_advertised: bool,
    pub direct_session_observed: bool,
    pub crawler_identified: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkPeersPage {
    pub items: Vec<PeerSummary>,
    pub next_cursor: Option<String>,
}

fn hex(bytes: &[u8]) -> String {
    hex::encode(bytes)
}

fn validate_limit(limit: Option<usize>) -> Result<usize, ApiRouteError> {
    let limit = limit.unwrap_or(50);
    if !(1..=500).contains(&limit) {
        return Err(ApiError::bad_request("limit must be between 1 and 500"));
    }
    Ok(limit)
}

fn session_initiator_name(initiator: SessionInitiator) -> &'static str {
    match initiator {
        SessionInitiator::Observer => "observerInitiated",
        SessionInitiator::Peer => "peerInitiated",
    }
}

fn has_published_positive_evidence(candidate: &CrawlCandidate, node: Option<&NodeRecord>) -> bool {
    !candidate.addresses.is_empty()
        || !candidate.advertisements.is_empty()
        || !candidate.direct_sessions.is_empty()
        || node.is_some()
}

fn latest_positive_observed_at(
    peer_id: &[u8],
    candidate: &CrawlCandidate,
    node: Option<&NodeRecord>,
) -> Result<u64, ApiRouteError> {
    candidate
        .addresses
        .iter()
        .map(|address| address.last_advertised_at)
        .chain(
            candidate
                .advertisements
                .iter()
                .map(|evidence| evidence.last_observed_at),
        )
        .chain(
            candidate
                .direct_sessions
                .iter()
                .map(|evidence| evidence.last_observed_at),
        )
        .chain(node.into_iter().map(|record| record.last_seen))
        .max()
        .ok_or_else(|| {
            ApiError::internal(format!(
                "network candidate has no positive retained evidence: peerId={}",
                hex(peer_id)
            ))
        })
}

fn peer_summary(
    peer_id: &[u8],
    candidate: &CrawlCandidate,
    node: Option<&NodeRecord>,
) -> Result<PeerSummary, ApiRouteError> {
    let last_observed_at = candidate.last_completed.as_ref().and_then(|completed| {
        completed
            .observations
            .iter()
            .map(|observation| observation.observed_at)
            .max()
    });
    let primary_addr = candidate
        .addresses
        .first()
        .map(|address| address.addr.clone());
    let last_advertised_at = candidate
        .addresses
        .iter()
        .map(|address| address.last_advertised_at)
        .max();
    let mut session_initiators: Vec<String> = candidate
        .direct_sessions
        .iter()
        .map(|evidence| session_initiator_name(evidence.initiator).to_string())
        .collect();
    session_initiators.sort();
    session_initiators.dedup();
    Ok(PeerSummary {
        peer_id: hex(peer_id),
        crawler_dial_state: display_state(candidate, node),
        participation: ParticipationEvidenceResponse {
            discovery_advertised: !candidate.advertisements.is_empty(),
            direct_session_observed: !candidate.direct_sessions.is_empty(),
            crawler_identified: node.is_some(),
        },
        session_initiators,
        primary_addr,
        version: node.map(|record| record.client_version.clone()),
        country: node.map(country_label),
        asn: node.map(asn_label),
        last_advertised_at,
        last_dial_observed_at: last_observed_at,
        latest_positive_observed_at: latest_positive_observed_at(peer_id, candidate, node)?,
        last_reachable_at: node.map(|record| record.last_reachable_at),
        rtt_ms: node.and_then(|record| record.last_rtt_ms),
    })
}

async fn peers(
    State(state): State<Arc<AppState>>,
    Query(query): Query<PeersQuery>,
) -> ApiResult<NetworkPeersPage> {
    let limit = validate_limit(query.limit)?;
    let state_filter = parse_display_state_filter(query.state.as_deref())?;
    let observation_filter = parse_observation_filter(query.observation.as_deref())?;
    let Some(store) = state.network_store.load_full() else {
        return ok(NetworkPeersPage {
            items: Vec::new(),
            next_cursor: None,
        });
    };
    let nodes: BTreeMap<Vec<u8>, NodeRecord> = store
        .scan_nodes()
        .map_err(|error| ApiError::internal(error.to_string()))?
        .into_iter()
        .collect();
    let candidates: BTreeMap<Vec<u8>, CrawlCandidate> = store
        .scan_crawl_candidates()
        .map_err(|error| ApiError::internal(error.to_string()))?
        .into_iter()
        .collect();
    for peer_id in nodes.keys() {
        if !candidates.contains_key(peer_id) {
            return Err(ApiError::internal(format!(
                "verified peer is missing candidate evidence: peerId={}",
                hex(peer_id)
            )));
        }
    }
    let mut rows = Vec::new();
    for (peer_id, candidate) in &candidates {
        let node = nodes.get(peer_id);
        if !has_published_positive_evidence(candidate, node)
            && !candidate.staged_direct_sessions.is_empty()
        {
            // Current-round RPC facts remain operational until the complete
            // round commits. A brand-new addressless target has no published
            // peer row yet and must not make the completed list inconsistent.
            continue;
        }
        let summary = peer_summary(peer_id, candidate, node)?;
        let matches = state_filter.is_none_or(|state| state == summary.crawler_dial_state)
            && observation_filter.is_none_or(|observation| {
                candidate.last_completed.as_ref().is_some_and(|completed| {
                    completed
                        .observations
                        .iter()
                        .any(|evidence| evidence.result == observation)
                })
            })
            && query.country.as_deref().is_none_or(|country| {
                summary
                    .country
                    .as_deref()
                    .is_some_and(|value| value == country)
            })
            && query.version.as_deref().is_none_or(|version| {
                summary
                    .version
                    .as_deref()
                    .is_some_and(|value| value == version)
            });
        if matches {
            rows.push(summary);
        }
    }
    rows.sort_by(|left, right| {
        right
            .latest_positive_observed_at
            .cmp(&left.latest_positive_observed_at)
            .then_with(|| left.peer_id.cmp(&right.peer_id))
    });
    let start = match query.cursor.as_deref() {
        Some(cursor) => rows
            .iter()
            .position(|row| row.peer_id == cursor)
            .map(|index| index + 1)
            .ok_or_else(|| ApiError::bad_request("cursor does not identify a filtered peer"))?,
        None => 0,
    };
    let remaining = &rows[start..];
    let items: Vec<PeerSummary> = remaining.iter().take(limit).cloned().collect();
    let next_cursor = if remaining.len() > limit {
        items.last().map(|peer| peer.peer_id.clone())
    } else {
        None
    };
    ok(NetworkPeersPage { items, next_cursor })
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveryEvidenceResponse {
    pub valid_nodes_messages: u64,
    pub valid_response_messages: u64,
    pub valid_announce_messages: u64,
    pub malformed_messages: u64,
    pub unexpected_messages: u64,
    pub normalized_advertised_addresses: u64,
    pub rejected_advertised_addresses: u64,
}

impl From<DiscoveryEvidence> for DiscoveryEvidenceResponse {
    fn from(evidence: DiscoveryEvidence) -> Self {
        Self {
            valid_nodes_messages: evidence.valid_nodes_messages,
            valid_response_messages: evidence.valid_response_messages,
            valid_announce_messages: evidence.valid_announce_messages,
            malformed_messages: evidence.malformed_messages,
            unexpected_messages: evidence.unexpected_messages,
            normalized_advertised_addresses: evidence.normalized_advertised_addresses,
            rejected_advertised_addresses: evidence.rejected_advertised_addresses,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AddressObservationHistogramResponse {
    pub dial_request_failed: u64,
    pub no_authenticated_session_before_deadline: u64,
    pub authenticated_session_without_identify_before_deadline: u64,
    pub malformed_identify: u64,
    pub foreign_network: u64,
    pub same_network_identified: u64,
}

impl From<AddressObservationHistogram> for AddressObservationHistogramResponse {
    fn from(histogram: AddressObservationHistogram) -> Self {
        Self {
            dial_request_failed: histogram.dial_request_failed,
            no_authenticated_session_before_deadline: histogram
                .no_authenticated_session_before_deadline,
            authenticated_session_without_identify_before_deadline: histogram
                .authenticated_session_without_identify_before_deadline,
            malformed_identify: histogram.malformed_identify,
            foreign_network: histogram.foreign_network,
            same_network_identified: histogram.same_network_identified,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AddressProbeEvidenceResponse {
    pub address: String,
    pub round_id: u64,
    pub observed_at: u64,
    pub elapsed_ms: u64,
    pub result: String,
}

impl From<AddressProbeEvidence> for AddressProbeEvidenceResponse {
    fn from(evidence: AddressProbeEvidence) -> Self {
        Self {
            address: evidence.address,
            round_id: evidence.round_id,
            observed_at: evidence.observed_at,
            elapsed_ms: evidence.elapsed_ms,
            result: address_result_name(evidence.result).to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CandidateEvidenceResponse {
    pub round_id: u64,
    pub outcome: String,
    pub observations: Vec<AddressProbeEvidenceResponse>,
    pub consecutive_exhausted_rounds: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActiveCandidateEvidenceResponse {
    pub round_id: u64,
    pub state: String,
    pub observations: Vec<AddressProbeEvidenceResponse>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AliasEvidenceResponse {
    pub address: String,
    pub first_advertised_at: u64,
    pub last_advertised_at: u64,
    pub last_verified_at: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifiedPeerResponse {
    pub own_addrs: Vec<String>,
    pub client_version: String,
    pub flags: u64,
    pub protocols: Vec<String>,
    pub first_seen: u64,
    pub last_seen: u64,
    pub last_reachable_at: u64,
    pub country: String,
    pub asn: String,
    pub rtt_ms: Option<u32>,
    pub discovery: DiscoveryEvidenceResponse,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AdvertiserResponse {
    pub advertiser_peer_id: String,
    pub alias: String,
    pub first_observed_at: u64,
    pub last_observed_at: u64,
    pub first_observed_round: u64,
    pub last_observed_round: u64,
    pub observation_count: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectSessionProtocolResponse {
    pub id: u64,
    pub version: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectSessionEvidenceResponse {
    pub observer_peer_id: String,
    pub initiator: String,
    pub first_observed_at: u64,
    pub last_observed_at: u64,
    pub first_observed_round: u64,
    pub last_observed_round: u64,
    pub observation_count: u64,
    pub client_version: String,
    /// Session metadata only; never a crawler dial/advertisement alias.
    pub session_addresses: Vec<String>,
    pub connected_duration_ms: u64,
    pub last_ping_duration_ms: Option<u64>,
    pub protocols: Vec<DirectSessionProtocolResponse>,
}

impl From<&DirectSessionEvidence> for DirectSessionEvidenceResponse {
    fn from(evidence: &DirectSessionEvidence) -> Self {
        Self {
            observer_peer_id: hex(&evidence.observer_peer_id),
            initiator: session_initiator_name(evidence.initiator).to_string(),
            first_observed_at: evidence.first_observed_at,
            last_observed_at: evidence.last_observed_at,
            first_observed_round: evidence.first_observed_round,
            last_observed_round: evidence.last_observed_round,
            observation_count: evidence.observation_count,
            client_version: evidence.client_version.clone(),
            session_addresses: evidence.session_addresses.clone(),
            connected_duration_ms: evidence.connected_duration_ms,
            last_ping_duration_ms: evidence.last_ping_duration_ms,
            protocols: evidence
                .protocols
                .iter()
                .map(|protocol| DirectSessionProtocolResponse {
                    id: protocol.id,
                    version: protocol.version.clone(),
                })
                .collect(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PeerDetailResponse {
    pub peer_id: String,
    pub observation_vantage: String,
    pub crawler_dial_state: PeerDisplayState,
    pub participation: ParticipationEvidenceResponse,
    pub session_initiators: Vec<String>,
    pub first_discovered_at: Option<u64>,
    pub last_advertised_at: Option<u64>,
    pub latest_positive_observed_at: u64,
    pub aliases: Vec<AliasEvidenceResponse>,
    pub last_completed: Option<CandidateEvidenceResponse>,
    pub active: Option<ActiveCandidateEvidenceResponse>,
    pub verified: Option<VerifiedPeerResponse>,
    pub advertisers: Vec<AdvertiserResponse>,
    pub direct_sessions: Vec<DirectSessionEvidenceResponse>,
}

fn completed_outcome_name(outcome: ckbadger_store::CompletedCandidateOutcome) -> &'static str {
    match outcome {
        ckbadger_store::CompletedCandidateOutcome::SameNetworkIdentified => "sameNetworkIdentified",
        ckbadger_store::CompletedCandidateOutcome::Exhausted => "exhausted",
        ckbadger_store::CompletedCandidateOutcome::ForeignNetwork => "foreignNetwork",
    }
}

fn active_state_name(state: ActiveCandidateState) -> &'static str {
    match state {
        ActiveCandidateState::Pending => "pending",
        ActiveCandidateState::RetryAlias => "retryAlias",
        ActiveCandidateState::Succeeded => "succeeded",
        ActiveCandidateState::Exhausted => "exhausted",
        ActiveCandidateState::ForeignNetwork => "foreignNetwork",
    }
}

async fn peer_by_id(
    State(state): State<Arc<AppState>>,
    AxumPath(peer_hex): AxumPath<String>,
) -> ApiResult<PeerDetailResponse> {
    let peer_id = hex::decode(&peer_hex)
        .map_err(|_| ApiError::bad_request("peerId must be lowercase or uppercase hex"))?;
    let Some(store) = state.network_store.load_full() else {
        return Err(ApiError::not_found("peer not found"));
    };
    let candidate = store
        .get_crawl_candidate(&peer_id)
        .map_err(|error| ApiError::internal(error.to_string()))?
        .ok_or_else(|| ApiError::not_found("peer not found"))?;
    let node = store
        .get_node(&peer_id)
        .map_err(|error| ApiError::internal(error.to_string()))?;
    if !has_published_positive_evidence(&candidate, node.as_ref())
        && !candidate.staged_direct_sessions.is_empty()
    {
        return Err(ApiError::not_found(
            "peer has no completed positive evidence",
        ));
    }
    // Target-centric completed evidence is authoritative. `NodeRecord.known_peers`
    // remains only the latest source-side Discovery snapshot and must not erase
    // prior positive observations when a later random response omits a target.
    let advertisers = candidate
        .advertisements
        .iter()
        .map(|evidence| AdvertiserResponse {
            advertiser_peer_id: hex(&evidence.advertiser_peer_id),
            alias: evidence.alias.clone(),
            first_observed_at: evidence.first_observed_at,
            last_observed_at: evidence.last_observed_at,
            first_observed_round: evidence.first_observed_round,
            last_observed_round: evidence.last_observed_round,
            observation_count: evidence.observation_count,
        })
        .collect();
    let verified = node.as_ref().map(|record| VerifiedPeerResponse {
        own_addrs: record.own_addrs.clone(),
        client_version: record.client_version.clone(),
        flags: record.flags,
        protocols: record.protocols.clone(),
        first_seen: record.first_seen,
        last_seen: record.last_seen,
        last_reachable_at: record.last_reachable_at,
        country: country_label(record),
        asn: asn_label(record),
        rtt_ms: record.last_rtt_ms,
        discovery: record.discovery.clone().into(),
    });
    let last_completed =
        candidate
            .last_completed
            .as_ref()
            .map(|completed| CandidateEvidenceResponse {
                round_id: completed.round_id,
                outcome: completed_outcome_name(completed.outcome).to_string(),
                observations: completed
                    .observations
                    .iter()
                    .cloned()
                    .map(Into::into)
                    .collect(),
                consecutive_exhausted_rounds: completed.consecutive_exhausted_rounds,
            });
    let active = candidate
        .active
        .as_ref()
        .map(|active| ActiveCandidateEvidenceResponse {
            round_id: active.round_id,
            state: active_state_name(active.state).to_string(),
            observations: active
                .observations
                .iter()
                .cloned()
                .map(Into::into)
                .collect(),
        });
    let first_discovered_at = candidate
        .addresses
        .iter()
        .map(|address| address.first_advertised_at)
        .min();
    let last_advertised_at = candidate
        .addresses
        .iter()
        .map(|address| address.last_advertised_at)
        .max();
    let latest_positive_observed_at =
        latest_positive_observed_at(&peer_id, &candidate, node.as_ref())?;
    let mut session_initiators: Vec<String> = candidate
        .direct_sessions
        .iter()
        .map(|evidence| session_initiator_name(evidence.initiator).to_string())
        .collect();
    session_initiators.sort();
    session_initiators.dedup();
    let participation = ParticipationEvidenceResponse {
        discovery_advertised: !candidate.advertisements.is_empty(),
        direct_session_observed: !candidate.direct_sessions.is_empty(),
        crawler_identified: node.is_some(),
    };
    let direct_sessions = candidate.direct_sessions.iter().map(Into::into).collect();
    ok(PeerDetailResponse {
        peer_id: peer_hex.to_ascii_lowercase(),
        observation_vantage: "configuredLocalCkbRpcObserverAndThisCrawler".to_string(),
        crawler_dial_state: display_state(&candidate, node.as_ref()),
        participation,
        session_initiators,
        first_discovered_at,
        last_advertised_at,
        latest_positive_observed_at,
        aliases: candidate
            .addresses
            .into_iter()
            .map(|address| AliasEvidenceResponse {
                address: address.addr,
                first_advertised_at: address.first_advertised_at,
                last_advertised_at: address.last_advertised_at,
                last_verified_at: address.last_verified_at,
            })
            .collect(),
        last_completed,
        active,
        verified,
        advertisers,
        direct_sessions,
    })
}
