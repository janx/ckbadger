use std::collections::BTreeMap;
use std::sync::Arc;

use axum::{
    extract::{Path as AxumPath, Query, State},
    routing::get,
    Router,
};
use serde::{Deserialize, Serialize};

use ckbadger_store::network_keys::{bucket_of, Granularity, Metric};
use ckbadger_store::{
    ActiveCandidateState, AddressObservationHistogram, AddressProbeEvidence, AddressProbeResult,
    CrawlCandidate, CrawlProgress, DiscoveryEvidence, LatestStatus, NodeRecord,
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
        .route("/network/history", get(history))
        .route("/network/peers", get(peers))
        .route("/network/peers/{peer_id}", get(peer_by_id))
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
    let points = match state.network_store.load_full() {
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
            let mut rows = store
                .scan_history(metric, granularity, from, to)
                .map_err(|error| ApiError::internal(error.to_string()))?;
            if granularity == Granularity::Day {
                let reference = match query.to {
                    Some(timestamp) => timestamp,
                    None => std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map_err(|error| {
                            ApiError::internal(format!("system clock before UNIX epoch: {error}"))
                        })?
                        .as_secs(),
                };
                let current_day = bucket_of(reference, Granularity::Day);
                rows.retain(|(bucket, _)| *bucket < current_day);
            }
            rows.into_iter()
                .map(|(bucket, point)| {
                    let ts = bucket.checked_mul(granularity.seconds()).ok_or_else(|| {
                        ApiError::internal(format!(
                            "network history timestamp overflow: metric={:?}, granularity={:?}, bucket={}, seconds={}",
                            metric,
                            granularity,
                            bucket,
                            granularity.seconds()
                        ))
                    })?;
                    Ok(HistoryPointResponse {
                        ts,
                        scalar: point.scalar,
                        buckets: point
                            .buckets
                            .into_iter()
                            .map(|(label, count)| LabelCount { label, count })
                            .collect(),
                    })
                })
                .collect::<Result<Vec<_>, _>>()?
        }
    };
    ok(NetworkHistory {
        metric: query.metric,
        granularity: query.granularity,
        points,
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
    pub display_state: PeerDisplayState,
    pub primary_addr: String,
    pub version: Option<String>,
    pub country: Option<String>,
    pub asn: Option<String>,
    pub last_advertised_at: u64,
    pub last_observed_at: Option<u64>,
    pub last_reachable_at: Option<u64>,
    pub rtt_ms: Option<u32>,
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

fn require_primary_alias<'a>(
    peer_id: &[u8],
    candidate: &'a CrawlCandidate,
) -> Result<&'a ckbadger_store::CrawlAddress, ApiRouteError> {
    candidate.addresses.first().ok_or_else(|| {
        ApiError::internal(format!(
            "network candidate has no retained alias: peerId={}",
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
    let primary_addr = require_primary_alias(peer_id, candidate)?.addr.clone();
    Ok(PeerSummary {
        peer_id: hex(peer_id),
        display_state: display_state(candidate, node),
        primary_addr,
        version: node.map(|record| record.client_version.clone()),
        country: node.map(country_label),
        asn: node.map(asn_label),
        last_advertised_at: candidate.last_advertised_at,
        last_observed_at,
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
        let summary = peer_summary(peer_id, candidate, node)?;
        let matches = state_filter.is_none_or(|state| state == summary.display_state)
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
            .last_advertised_at
            .cmp(&left.last_advertised_at)
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
    pub malformed_messages: u64,
    pub unexpected_messages: u64,
    pub normalized_advertised_addresses: u64,
    pub rejected_advertised_addresses: u64,
}

impl From<DiscoveryEvidence> for DiscoveryEvidenceResponse {
    fn from(evidence: DiscoveryEvidence) -> Self {
        Self {
            valid_nodes_messages: evidence.valid_nodes_messages,
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
    pub observed_at: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PeerDetailResponse {
    pub peer_id: String,
    pub observation_vantage: String,
    pub display_state: PeerDisplayState,
    pub first_discovered_at: u64,
    pub last_advertised_at: u64,
    pub aliases: Vec<AliasEvidenceResponse>,
    pub last_completed: Option<CandidateEvidenceResponse>,
    pub active: Option<ActiveCandidateEvidenceResponse>,
    pub verified: Option<VerifiedPeerResponse>,
    pub advertisers: Vec<AdvertiserResponse>,
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
    require_primary_alias(&peer_id, &candidate)?;
    let node = store
        .get_node(&peer_id)
        .map_err(|error| ApiError::internal(error.to_string()))?;
    let advertisers = store
        .scan_nodes()
        .map_err(|error| ApiError::internal(error.to_string()))?
        .into_iter()
        .filter(|(_, record)| record.known_peers.iter().any(|known| known == &peer_id))
        .map(|(advertiser_peer_id, record)| AdvertiserResponse {
            advertiser_peer_id: hex(&advertiser_peer_id),
            observed_at: record.last_seen,
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
    ok(PeerDetailResponse {
        peer_id: peer_hex.to_ascii_lowercase(),
        observation_vantage: "thisCkbadgerInstance".to_string(),
        display_state: display_state(&candidate, node.as_ref()),
        first_discovered_at: candidate.first_discovered_at,
        last_advertised_at: candidate.last_advertised_at,
        aliases: candidate
            .addresses
            .into_iter()
            .map(|address| AliasEvidenceResponse {
                address: address.addr,
                first_advertised_at: address.first_advertised_at,
                last_advertised_at: address.last_advertised_at,
            })
            .collect(),
        last_completed,
        active,
        verified,
        advertisers,
    })
}
