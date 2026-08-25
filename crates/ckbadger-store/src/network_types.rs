//! Network-store value types. All use `bincode` serialization (see types.rs convention).
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Geo {
    pub country: String, // ISO code; never stored empty — use None on the record instead
    pub city: String,
    pub lat: f64,
    pub lon: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct Asn {
    pub number: u32,
    pub org: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NodeRecord {
    pub own_addrs: Vec<String>,
    pub client_version: String,
    pub flags: u64,
    pub protocols: Vec<String>,
    pub first_seen: u64,
    pub last_seen: u64,
    pub last_reachable_at: u64,
    pub reachable: bool,
    pub geo: Option<Geo>,
    pub asn: Option<Asn>,
    pub last_rtt_ms: Option<u32>,
    /// Exact Discovery protocol evidence captured by the same successful probe
    /// that refreshed this node record.
    pub discovery: DiscoveryEvidence,
    /// Peer ids resolved from addresses in this node's Discovery response for
    /// this round. This is address-book gossip, not reachability proof.
    pub known_peers: Vec<Vec<u8>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct DiscoveryEvidence {
    pub valid_nodes_messages: u64,
    pub valid_response_messages: u64,
    pub valid_announce_messages: u64,
    pub malformed_messages: u64,
    pub unexpected_messages: u64,
    /// Distinct normalized peer-keyed addresses accepted by the crawler.
    pub normalized_advertised_addresses: u64,
    pub rejected_advertised_addresses: u64,
}

impl DiscoveryEvidence {
    pub fn checked_add_assign(&mut self, rhs: &Self, round_id: u64) -> anyhow::Result<()> {
        self.checked_validate_message_kinds(round_id, "accumulator")?;
        rhs.checked_validate_message_kinds(round_id, "addend")?;
        macro_rules! add {
            ($field:ident) => {
                self.$field = self.$field.checked_add(rhs.$field).ok_or_else(|| {
                    anyhow::anyhow!(
                        "crawler Discovery counter overflow: round_id={}, field={}",
                        round_id,
                        stringify!($field)
                    )
                })?;
            };
        }
        add!(valid_nodes_messages);
        add!(valid_response_messages);
        add!(valid_announce_messages);
        add!(malformed_messages);
        add!(unexpected_messages);
        add!(normalized_advertised_addresses);
        add!(rejected_advertised_addresses);
        self.checked_validate_message_kinds(round_id, "sum")?;
        Ok(())
    }

    pub fn checked_validate_message_kinds(
        &self,
        round_id: u64,
        context: &str,
    ) -> anyhow::Result<()> {
        let classified = self
            .valid_response_messages
            .checked_add(self.valid_announce_messages)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "crawler Discovery classified-message counter overflow: round_id={}, context={}",
                    round_id,
                    context
                )
            })?;
        if classified != self.valid_nodes_messages {
            anyhow::bail!(
                "crawler Discovery message-kind invariant failed: round_id={}, context={}, valid_nodes_messages={}, valid_response_messages={}, valid_announce_messages={}",
                round_id,
                context,
                self.valid_nodes_messages,
                self.valid_response_messages,
                self.valid_announce_messages
            );
        }
        Ok(())
    }
}

/// Session initiation direction from the configured local CKB RPC observer's
/// vantage point. This is deliberately not a reachability classification.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SessionInitiator {
    /// The configured local CKB node established the connection.
    Observer,
    /// The remote peer established the connection to the configured local node.
    Peer,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct LocalObserverProtocol {
    pub id: u64,
    pub name: String,
    pub support_versions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct DirectSessionProtocol {
    pub id: u64,
    pub version: String,
}

/// One active-round `local_node_info` observation. It remains staged in the
/// active crawl singleton until the whole logical round publishes.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LocalObserverObservation {
    pub round_id: u64,
    pub observed_at: u64,
    pub peer_id: Vec<u8>,
    pub client_version: String,
    pub active: bool,
    pub addresses: Vec<String>,
    pub protocols: Vec<LocalObserverProtocol>,
    pub connections: u64,
}

/// Completed longitudinal evidence for the configured local CKB observer.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LocalObserverEvidence {
    pub peer_id: Vec<u8>,
    pub first_observed_at: u64,
    pub last_observed_at: u64,
    pub first_observed_round: u64,
    pub last_observed_round: u64,
    pub observation_count: u64,
    pub client_version: String,
    pub active: bool,
    pub addresses: Vec<String>,
    pub protocols: Vec<LocalObserverProtocol>,
    pub connections: u64,
}

/// One current-round `get_peers` row staged on the remote target candidate.
/// `session_addresses` are evidence about that session only; they are never
/// crawler dial aliases because an inbound row can contain an ephemeral source
/// port and the RPC contract permits an empty list.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DirectSessionObservation {
    pub round_id: u64,
    pub observed_at: u64,
    pub observer_peer_id: Vec<u8>,
    pub initiator: SessionInitiator,
    pub client_version: String,
    pub session_addresses: Vec<String>,
    pub connected_duration_ms: u64,
    pub last_ping_duration_ms: Option<u64>,
    pub protocols: Vec<DirectSessionProtocol>,
}

/// Completed target-centric direct-session evidence keyed by
/// `(observer_peer_id, initiator)`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DirectSessionEvidence {
    pub observer_peer_id: Vec<u8>,
    pub initiator: SessionInitiator,
    pub first_observed_at: u64,
    pub last_observed_at: u64,
    pub first_observed_round: u64,
    pub last_observed_round: u64,
    pub observation_count: u64,
    pub client_version: String,
    pub session_addresses: Vec<String>,
    pub connected_duration_ms: u64,
    pub last_ping_duration_ms: Option<u64>,
    pub protocols: Vec<DirectSessionProtocol>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct DirectSessionObservationSummary {
    pub observer_initiated: u64,
    pub peer_initiated: u64,
}

impl DirectSessionObservationSummary {
    pub fn checked_record(
        &mut self,
        initiator: SessionInitiator,
        round_id: u64,
        target_peer_id: &[u8],
    ) -> anyhow::Result<()> {
        let (field, name) = match initiator {
            SessionInitiator::Observer => (&mut self.observer_initiated, "observer_initiated"),
            SessionInitiator::Peer => (&mut self.peer_initiated, "peer_initiated"),
        };
        *field = field.checked_add(1).ok_or_else(|| {
            anyhow::anyhow!(
                "direct-session observation counter overflow: round_id={}, target_peer_id=0x{}, field={}",
                round_id,
                crate::bytes_to_hex(target_peer_id),
                name
            )
        })?;
        Ok(())
    }

    pub fn total(&self, round_id: u64) -> anyhow::Result<u64> {
        self.observer_initiated
            .checked_add(self.peer_initiated)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "direct-session observation total overflow: round_id={}",
                    round_id
                )
            })
    }
}

fn strings_are_canonical(values: &[String]) -> bool {
    values.windows(2).all(|window| window[0] < window[1])
}

fn validate_local_observer_observation(
    observation: &LocalObserverObservation,
    round_id: u64,
) -> anyhow::Result<()> {
    if observation.round_id != round_id {
        anyhow::bail!(
            "local observer round mismatch: round_id={}, observation_round={}",
            round_id,
            observation.round_id
        );
    }
    if observation.peer_id.is_empty() {
        anyhow::bail!("local observer has empty peer id: round_id={round_id}");
    }
    if !strings_are_canonical(&observation.addresses) {
        anyhow::bail!(
            "local observer addresses are duplicate or not canonically sorted: round_id={}, peer_id=0x{}",
            round_id,
            crate::bytes_to_hex(&observation.peer_id)
        );
    }
    if observation
        .protocols
        .windows(2)
        .any(|window| window[0] >= window[1])
        || observation
            .protocols
            .iter()
            .any(|protocol| !strings_are_canonical(&protocol.support_versions))
    {
        anyhow::bail!(
            "local observer protocols are duplicate or not canonically sorted: round_id={}, peer_id=0x{}",
            round_id,
            crate::bytes_to_hex(&observation.peer_id)
        );
    }
    Ok(())
}

pub fn checked_merge_local_observer_evidence(
    prior: Option<&LocalObserverEvidence>,
    observation: &LocalObserverObservation,
    round_id: u64,
) -> anyhow::Result<LocalObserverEvidence> {
    validate_local_observer_observation(observation, round_id)?;
    let (first_observed_at, first_observed_round, observation_count) = match prior
        .filter(|prior| prior.peer_id == observation.peer_id)
    {
        Some(prior) => {
            if prior.last_observed_round >= round_id
                || prior.last_observed_at > observation.observed_at
            {
                anyhow::bail!(
                        "local observer evidence regressed: round_id={}, peer_id=0x{}, prior_round={}, prior_time={}, observed_at={}",
                        round_id,
                        crate::bytes_to_hex(&observation.peer_id),
                        prior.last_observed_round,
                        prior.last_observed_at,
                        observation.observed_at
                    );
            }
            (
                prior.first_observed_at,
                prior.first_observed_round,
                prior.observation_count.checked_add(1).ok_or_else(|| {
                    anyhow::anyhow!(
                        "local observer observation count overflow: round_id={}, peer_id=0x{}",
                        round_id,
                        crate::bytes_to_hex(&observation.peer_id)
                    )
                })?,
            )
        }
        None => (observation.observed_at, round_id, 1),
    };
    Ok(LocalObserverEvidence {
        peer_id: observation.peer_id.clone(),
        first_observed_at,
        last_observed_at: observation.observed_at,
        first_observed_round,
        last_observed_round: round_id,
        observation_count,
        client_version: observation.client_version.clone(),
        active: observation.active,
        addresses: observation.addresses.clone(),
        protocols: observation.protocols.clone(),
        connections: observation.connections,
    })
}

fn direct_session_key(
    observer_peer_id: &[u8],
    initiator: SessionInitiator,
) -> (&[u8], SessionInitiator) {
    (observer_peer_id, initiator)
}

fn validate_direct_session_metadata(
    observation: &DirectSessionObservation,
    round_id: u64,
    target_peer_id: &[u8],
) -> anyhow::Result<()> {
    if observation.round_id != round_id {
        anyhow::bail!(
            "direct-session round mismatch: round_id={}, target_peer_id=0x{}, observation_round={}",
            round_id,
            crate::bytes_to_hex(target_peer_id),
            observation.round_id
        );
    }
    if observation.observer_peer_id.is_empty() || observation.observer_peer_id == target_peer_id {
        anyhow::bail!(
            "direct-session observer identity is invalid: round_id={}, target_peer_id=0x{}, observer_peer_id=0x{}",
            round_id,
            crate::bytes_to_hex(target_peer_id),
            crate::bytes_to_hex(&observation.observer_peer_id)
        );
    }
    if !strings_are_canonical(&observation.session_addresses)
        || observation
            .protocols
            .windows(2)
            .any(|window| window[0] >= window[1])
    {
        anyhow::bail!(
            "direct-session metadata is duplicate or not canonically sorted: round_id={}, target_peer_id=0x{}, observer_peer_id=0x{}",
            round_id,
            crate::bytes_to_hex(target_peer_id),
            crate::bytes_to_hex(&observation.observer_peer_id)
        );
    }
    Ok(())
}

/// Merge current-round staged direct sessions into completed target-centric
/// evidence. This is the single calculation path used by engine and validator.
pub fn checked_merge_direct_session_evidence(
    durable: &mut Vec<DirectSessionEvidence>,
    staged: &[DirectSessionObservation],
    round_id: u64,
    target_peer_id: &[u8],
) -> anyhow::Result<()> {
    if durable.windows(2).any(|window| {
        direct_session_key(&window[0].observer_peer_id, window[0].initiator)
            >= direct_session_key(&window[1].observer_peer_id, window[1].initiator)
    }) {
        anyhow::bail!(
            "durable direct-session evidence is duplicate or not canonically sorted: round_id={}, target_peer_id=0x{}",
            round_id,
            crate::bytes_to_hex(target_peer_id)
        );
    }
    for evidence in durable.iter() {
        if evidence.observer_peer_id.is_empty()
            || evidence.observer_peer_id == target_peer_id
            || evidence.observation_count == 0
            || evidence.first_observed_at > evidence.last_observed_at
            || evidence.first_observed_round > evidence.last_observed_round
            || evidence.last_observed_round >= round_id
            || !strings_are_canonical(&evidence.session_addresses)
            || evidence
                .protocols
                .windows(2)
                .any(|window| window[0] >= window[1])
        {
            anyhow::bail!(
                "durable direct-session evidence invariant failed: round_id={}, target_peer_id=0x{}, observer_peer_id=0x{}",
                round_id,
                crate::bytes_to_hex(target_peer_id),
                crate::bytes_to_hex(&evidence.observer_peer_id)
            );
        }
    }
    let mut prior_key = None;
    for observation in staged {
        validate_direct_session_metadata(observation, round_id, target_peer_id)?;
        let key = direct_session_key(&observation.observer_peer_id, observation.initiator);
        if prior_key.is_some_and(|prior| prior >= key) {
            anyhow::bail!(
                "staged direct-session evidence is duplicate or not canonically sorted: round_id={}, target_peer_id=0x{}",
                round_id,
                crate::bytes_to_hex(target_peer_id)
            );
        }
        prior_key = Some(key);
    }
    for observation in staged {
        let key = direct_session_key(&observation.observer_peer_id, observation.initiator);
        match durable.binary_search_by(|prior| {
            direct_session_key(&prior.observer_peer_id, prior.initiator).cmp(&key)
        }) {
            Ok(index) => {
                let prior = &mut durable[index];
                if observation.observed_at < prior.last_observed_at {
                    anyhow::bail!(
                        "direct-session time regressed: round_id={}, target_peer_id=0x{}, observer_peer_id=0x{}",
                        round_id,
                        crate::bytes_to_hex(target_peer_id),
                        crate::bytes_to_hex(&observation.observer_peer_id)
                    );
                }
                prior.last_observed_at = observation.observed_at;
                prior.last_observed_round = round_id;
                prior.observation_count = prior.observation_count.checked_add(1).ok_or_else(|| {
                    anyhow::anyhow!(
                        "direct-session observation count overflow: round_id={}, target_peer_id=0x{}, observer_peer_id=0x{}",
                        round_id,
                        crate::bytes_to_hex(target_peer_id),
                        crate::bytes_to_hex(&observation.observer_peer_id)
                    )
                })?;
                prior.client_version = observation.client_version.clone();
                prior.session_addresses = observation.session_addresses.clone();
                prior.connected_duration_ms = observation.connected_duration_ms;
                prior.last_ping_duration_ms = observation.last_ping_duration_ms;
                prior.protocols = observation.protocols.clone();
            }
            Err(index) => durable.insert(
                index,
                DirectSessionEvidence {
                    observer_peer_id: observation.observer_peer_id.clone(),
                    initiator: observation.initiator,
                    first_observed_at: observation.observed_at,
                    last_observed_at: observation.observed_at,
                    first_observed_round: round_id,
                    last_observed_round: round_id,
                    observation_count: 1,
                    client_version: observation.client_version.clone(),
                    session_addresses: observation.session_addresses.clone(),
                    connected_duration_ms: observation.connected_duration_ms,
                    last_ping_duration_ms: observation.last_ping_duration_ms,
                    protocols: observation.protocols.clone(),
                },
            ),
        }
    }
    Ok(())
}

/// Apply the direct-session TTL after current-round observations have been
/// merged. Missing a peer from a later `get_peers` snapshot is not negative
/// evidence; only the independent time cutoff expires a completed fact.
pub fn checked_prune_direct_session_evidence(
    durable: &mut Vec<DirectSessionEvidence>,
    cutoff: Option<u64>,
    round_id: u64,
    target_peer_id: &[u8],
) -> anyhow::Result<()> {
    let mut prior_key = None;
    for evidence in durable.iter() {
        let key = direct_session_key(&evidence.observer_peer_id, evidence.initiator);
        if prior_key.is_some_and(|prior| prior >= key)
            || evidence.observer_peer_id.is_empty()
            || evidence.observer_peer_id == target_peer_id
            || evidence.observation_count == 0
            || evidence.first_observed_at > evidence.last_observed_at
            || evidence.first_observed_round > evidence.last_observed_round
            || evidence.last_observed_round > round_id
            || !strings_are_canonical(&evidence.session_addresses)
            || evidence
                .protocols
                .windows(2)
                .any(|window| window[0] >= window[1])
        {
            anyhow::bail!(
                "completed direct-session evidence invariant failed while pruning: round_id={}, target_peer_id=0x{}, observer_peer_id=0x{}",
                round_id,
                crate::bytes_to_hex(target_peer_id),
                crate::bytes_to_hex(&evidence.observer_peer_id)
            );
        }
        prior_key = Some(key);
    }
    if let Some(cutoff) = cutoff {
        durable.retain(|evidence| evidence.last_observed_at >= cutoff);
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum AddressProbeResult {
    DialRequestFailed,
    NoAuthenticatedSessionBeforeDeadline,
    AuthenticatedSessionWithoutIdentifyBeforeDeadline,
    MalformedIdentify,
    ForeignNetwork,
    SameNetworkIdentified,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AddressProbeEvidence {
    pub address: String,
    pub round_id: u64,
    pub observed_at: u64,
    pub elapsed_ms: u64,
    pub result: AddressProbeResult,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct AddressObservationHistogram {
    pub dial_request_failed: u64,
    pub no_authenticated_session_before_deadline: u64,
    pub authenticated_session_without_identify_before_deadline: u64,
    pub malformed_identify: u64,
    pub foreign_network: u64,
    pub same_network_identified: u64,
}

impl AddressObservationHistogram {
    pub fn checked_record(
        &mut self,
        result: AddressProbeResult,
        round_id: u64,
    ) -> anyhow::Result<()> {
        let (field, name) = match result {
            AddressProbeResult::DialRequestFailed => {
                (&mut self.dial_request_failed, "dial_request_failed")
            }
            AddressProbeResult::NoAuthenticatedSessionBeforeDeadline => (
                &mut self.no_authenticated_session_before_deadline,
                "no_authenticated_session_before_deadline",
            ),
            AddressProbeResult::AuthenticatedSessionWithoutIdentifyBeforeDeadline => (
                &mut self.authenticated_session_without_identify_before_deadline,
                "authenticated_session_without_identify_before_deadline",
            ),
            AddressProbeResult::MalformedIdentify => {
                (&mut self.malformed_identify, "malformed_identify")
            }
            AddressProbeResult::ForeignNetwork => (&mut self.foreign_network, "foreign_network"),
            AddressProbeResult::SameNetworkIdentified => {
                (&mut self.same_network_identified, "same_network_identified")
            }
        };
        *field = field.checked_add(1).ok_or_else(|| {
            anyhow::anyhow!(
                "crawler address observation counter overflow: round_id={}, field={}",
                round_id,
                name
            )
        })?;
        Ok(())
    }

    pub fn address_attempts(&self, round_id: u64) -> anyhow::Result<u64> {
        checked_sum(
            [
                self.dial_request_failed,
                self.no_authenticated_session_before_deadline,
                self.authenticated_session_without_identify_before_deadline,
                self.malformed_identify,
                self.foreign_network,
                self.same_network_identified,
            ],
            "address_attempts",
            round_id,
        )
    }

    pub fn non_successful_address_attempts(&self, round_id: u64) -> anyhow::Result<u64> {
        checked_sum(
            [
                self.dial_request_failed,
                self.no_authenticated_session_before_deadline,
                self.authenticated_session_without_identify_before_deadline,
                self.malformed_identify,
                self.foreign_network,
            ],
            "non_successful_address_attempts",
            round_id,
        )
    }

    /// Validate and aggregate one candidate's address evidence. This is the
    /// single calculation path used by both crawler publication and the
    /// network-store atomic commit validator.
    pub fn checked_record_candidate(
        &mut self,
        observations: &[AddressProbeEvidence],
        addresses: &[CrawlAddress],
        outcome: CompletedCandidateOutcome,
        round_id: u64,
        peer_id: &[u8],
    ) -> anyhow::Result<()> {
        if addresses.is_empty() {
            anyhow::bail!(
                "completed candidate has no retained aliases: round_id={}, peer_id=0x{}",
                round_id,
                crate::bytes_to_hex(peer_id)
            );
        }
        let mut aliases = std::collections::HashSet::new();
        for address in addresses {
            if address.addr.is_empty() {
                anyhow::bail!(
                    "completed candidate has empty retained alias: round_id={}, peer_id=0x{}",
                    round_id,
                    crate::bytes_to_hex(peer_id)
                );
            }
            if address.first_advertised_at > address.last_advertised_at {
                anyhow::bail!(
                    "candidate alias advertisement time regressed: round_id={}, peer_id=0x{}, addr={}, first_advertised_at={}, last_advertised_at={}",
                    round_id,
                    crate::bytes_to_hex(peer_id),
                    address.addr,
                    address.first_advertised_at,
                    address.last_advertised_at
                );
            }
            if !aliases.insert(address.addr.as_str()) {
                anyhow::bail!(
                    "completed candidate has duplicate retained alias: round_id={}, peer_id=0x{}, addr={}",
                    round_id,
                    crate::bytes_to_hex(peer_id),
                    address.addr
                );
            }
        }
        if observations.is_empty() {
            anyhow::bail!(
                "completed candidate has no address observations: round_id={}, peer_id=0x{}",
                round_id,
                crate::bytes_to_hex(peer_id)
            );
        }
        let mut observed_addresses = std::collections::HashSet::new();
        let mut same_network_results = 0u64;
        let mut foreign_network_results = 0u64;
        for evidence in observations {
            if evidence.round_id != round_id {
                anyhow::bail!(
                    "address evidence round mismatch: round_id={}, peer_id=0x{}, evidence_round={}, addr={}",
                    round_id,
                    crate::bytes_to_hex(peer_id),
                    evidence.round_id,
                    evidence.address
                );
            }
            if !aliases.contains(evidence.address.as_str()) {
                anyhow::bail!(
                    "candidate observation references unknown alias: round_id={}, peer_id=0x{}, addr={}",
                    round_id,
                    crate::bytes_to_hex(peer_id),
                    evidence.address
                );
            }
            if !observed_addresses.insert(evidence.address.as_str()) {
                anyhow::bail!(
                    "candidate has duplicate completed address observation: round_id={}, peer_id=0x{}, addr={}",
                    round_id,
                    crate::bytes_to_hex(peer_id),
                    evidence.address
                );
            }
            self.checked_record(evidence.result, round_id)?;
            match evidence.result {
                AddressProbeResult::SameNetworkIdentified => {
                    same_network_results =
                        same_network_results.checked_add(1).ok_or_else(|| {
                            anyhow::anyhow!(
                                "same-network result count overflow: round_id={}, peer_id=0x{}",
                                round_id,
                                crate::bytes_to_hex(peer_id)
                            )
                        })?;
                }
                AddressProbeResult::ForeignNetwork => {
                    foreign_network_results =
                        foreign_network_results.checked_add(1).ok_or_else(|| {
                            anyhow::anyhow!(
                                "foreign-network result count overflow: round_id={}, peer_id=0x{}",
                                round_id,
                                crate::bytes_to_hex(peer_id)
                            )
                        })?;
                }
                _ => {}
            }
        }
        let result_matches_outcome = match outcome {
            CompletedCandidateOutcome::SameNetworkIdentified => same_network_results == 1,
            CompletedCandidateOutcome::Exhausted => {
                same_network_results == 0 && foreign_network_results == 0
            }
            CompletedCandidateOutcome::ForeignNetwork => {
                same_network_results == 0 && foreign_network_results > 0
            }
        };
        if !result_matches_outcome {
            anyhow::bail!(
                "candidate outcome does not match address evidence: round_id={}, peer_id=0x{}, outcome={:?}, same_network_results={}, foreign_network_results={}",
                round_id,
                crate::bytes_to_hex(peer_id),
                outcome,
                same_network_results,
                foreign_network_results
            );
        }
        if outcome != CompletedCandidateOutcome::SameNetworkIdentified
            && observed_addresses.len() != aliases.len()
        {
            anyhow::bail!(
                "terminal candidate did not attempt every retained alias: round_id={}, peer_id=0x{}, outcome={:?}, aliases={}, observations={}",
                round_id,
                crate::bytes_to_hex(peer_id),
                outcome,
                aliases.len(),
                observed_addresses.len()
            );
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct CompletedPeerOutcomes {
    pub same_network_identified: u64,
    pub exhausted_with_retained_verification: u64,
    pub exhausted_without_retained_verification: u64,
    pub foreign_with_retained_verification: u64,
    pub foreign_without_retained_verification: u64,
}

fn checked_sum<const N: usize>(
    values: [u64; N],
    field: &str,
    round_id: u64,
) -> anyhow::Result<u64> {
    values.into_iter().try_fold(0u64, |sum, value| {
        sum.checked_add(value).ok_or_else(|| {
            anyhow::anyhow!(
                "crawler derived counter overflow: round_id={}, field={}",
                round_id,
                field
            )
        })
    })
}

impl CompletedPeerOutcomes {
    /// Classify one completed candidate against the final retained-node
    /// snapshot. Both the crawler and store validator use this exact mapping.
    pub fn checked_record(
        &mut self,
        outcome: CompletedCandidateOutcome,
        retained: bool,
        round_id: u64,
        peer_id: &[u8],
    ) -> anyhow::Result<()> {
        let (field, name) = match (outcome, retained) {
            (CompletedCandidateOutcome::SameNetworkIdentified, true) => {
                (&mut self.same_network_identified, "same_network_identified")
            }
            (CompletedCandidateOutcome::SameNetworkIdentified, false) => {
                anyhow::bail!(
                    "same-network candidate has no retained node: round_id={}, peer_id=0x{}",
                    round_id,
                    crate::bytes_to_hex(peer_id)
                )
            }
            (CompletedCandidateOutcome::Exhausted, true) => (
                &mut self.exhausted_with_retained_verification,
                "exhausted_with_retained_verification",
            ),
            (CompletedCandidateOutcome::Exhausted, false) => (
                &mut self.exhausted_without_retained_verification,
                "exhausted_without_retained_verification",
            ),
            (CompletedCandidateOutcome::ForeignNetwork, true) => (
                &mut self.foreign_with_retained_verification,
                "foreign_with_retained_verification",
            ),
            (CompletedCandidateOutcome::ForeignNetwork, false) => (
                &mut self.foreign_without_retained_verification,
                "foreign_without_retained_verification",
            ),
        };
        *field = field.checked_add(1).ok_or_else(|| {
            anyhow::anyhow!(
                "completed peer outcome overflow: round_id={}, peer_id=0x{}, field={}",
                round_id,
                crate::bytes_to_hex(peer_id),
                name
            )
        })?;
        Ok(())
    }

    pub fn candidate_peers(&self, round_id: u64) -> anyhow::Result<u64> {
        checked_sum(
            [
                self.same_network_identified,
                self.exhausted_with_retained_verification,
                self.exhausted_without_retained_verification,
                self.foreign_with_retained_verification,
                self.foreign_without_retained_verification,
            ],
            "candidate_peers",
            round_id,
        )
    }

    pub fn reachable_peers(&self) -> u64 {
        self.same_network_identified
    }

    pub fn exhausted_candidates(&self, round_id: u64) -> anyhow::Result<u64> {
        checked_sum(
            [
                self.exhausted_with_retained_verification,
                self.exhausted_without_retained_verification,
            ],
            "exhausted_candidates",
            round_id,
        )
    }

    pub fn foreign_peers(&self, round_id: u64) -> anyhow::Result<u64> {
        checked_sum(
            [
                self.foreign_with_retained_verification,
                self.foreign_without_retained_verification,
            ],
            "foreign_peers",
            round_id,
        )
    }

    pub fn verified_unavailable_peers(&self, round_id: u64) -> anyhow::Result<u64> {
        checked_sum(
            [
                self.exhausted_with_retained_verification,
                self.foreign_with_retained_verification,
            ],
            "verified_unavailable_peers",
            round_id,
        )
    }

    pub fn verified_retained_peers(&self, round_id: u64) -> anyhow::Result<u64> {
        checked_sum(
            [
                self.same_network_identified,
                self.exhausted_with_retained_verification,
                self.foreign_with_retained_verification,
            ],
            "verified_retained_peers",
            round_id,
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct LatestStatus {
    pub round_id: u64,
    pub started: u64,
    pub finished: u64,
    pub peer_outcomes: CompletedPeerOutcomes,
    pub address_observations: AddressObservationHistogram,
    pub discovery: DiscoveryEvidence,
    pub malformed_addresses: u64,
    pub new_verified_peers: u64,
    /// Latest completed observation of the configured local CKB RPC node.
    pub local_observer: Option<LocalObserverEvidence>,
    /// Exact `get_peers` rows captured for this completed logical round.
    pub direct_session_observations: DirectSessionObservationSummary,
}

/// Durable metadata for one logical crawl round. A logical round may span
/// several execution slices; this singleton survives those boundaries and
/// process restarts until the round is atomically published.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ActiveCrawl {
    pub round_id: u64,
    pub started_at: u64,
    pub last_checkpoint_at: u64,
    pub next_schedule_sequence: u64,
    pub address_observations: AddressObservationHistogram,
    pub malformed_addresses: u64,
    /// Per-alias positive-observation cutoff fixed when this logical round
    /// starts. `None` means the UNIX clock is still below the configured TTL.
    /// Durable alias/evidence pruning is deferred to completed publication.
    pub alias_freshness_cutoff: Option<u64>,
    /// Direct-session evidence uses its own retention transition. It may share
    /// a configured duration with aliases, but never derives freshness from an
    /// advertised or successfully dialed address.
    pub direct_session_freshness_cutoff: Option<u64>,
    /// Staged `local_node_info` observation. Presence is also the durable
    /// marker that this logical round has sampled the local RPC exactly once.
    pub local_observer_observation: Option<LocalObserverObservation>,
    /// Sorted target peer ids whose staged direct-session row lives on the
    /// corresponding `CrawlCandidate` in the same checkpoint batch.
    pub direct_session_targets: Vec<Vec<u8>>,
    /// Set only when the scheduler cannot preserve its coverage invariant.
    /// The crawler exits with the same actionable reason instead of publishing
    /// a truncated snapshot.
    pub blocked_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct CrawlAddress {
    pub addr: String,
    pub first_advertised_at: u64,
    pub last_advertised_at: u64,
    /// Most recent successful same-network Identify reached through this exact
    /// dial alias. A peer-wide success must never refresh its other aliases.
    pub last_verified_at: Option<u64>,
}

pub fn crawl_address_is_fresh(address: &CrawlAddress, cutoff: Option<u64>) -> bool {
    cutoff.is_none_or(|cutoff| {
        address.last_advertised_at >= cutoff
            || address
                .last_verified_at
                .is_some_and(|verified_at| verified_at >= cutoff)
    })
}

/// Apply exact per-address same-network observations at completed publication.
/// The active checkpoint keeps the observation itself authoritative, so a
/// partial logical round never mutates durable verification freshness.
pub fn checked_apply_alias_verifications(
    addresses: &mut [CrawlAddress],
    observations: &[AddressProbeEvidence],
    round_id: u64,
    peer_id: &[u8],
) -> anyhow::Result<()> {
    for observation in observations
        .iter()
        .filter(|observation| observation.result == AddressProbeResult::SameNetworkIdentified)
    {
        let address = addresses
            .iter_mut()
            .find(|address| address.addr == observation.address)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "same-network verification references an unknown alias: round_id={}, peer_id=0x{}, alias={}",
                    round_id,
                    crate::bytes_to_hex(peer_id),
                    observation.address
                )
            })?;
        if address
            .last_verified_at
            .is_some_and(|prior| observation.observed_at < prior)
        {
            anyhow::bail!(
                "alias verification time regressed: round_id={}, peer_id=0x{}, alias={}, prior_last_verified_at={:?}, observed_at={}",
                round_id,
                crate::bytes_to_hex(peer_id),
                observation.address,
                address.last_verified_at,
                observation.observed_at
            );
        }
        address.last_verified_at = Some(observation.observed_at);
    }
    Ok(())
}

/// Apply the completed round's exact TTL transition. This must only be called
/// while constructing the atomic completed-round commit, never at a partial
/// checkpoint, because candidates are read directly by the API secondary.
pub fn checked_prune_candidate_aliases(
    candidate: &mut CrawlCandidate,
    cutoff: Option<u64>,
    round_id: u64,
    peer_id: &[u8],
) -> anyhow::Result<()> {
    let mut aliases = std::collections::HashSet::new();
    for address in &candidate.addresses {
        if address.addr.is_empty() {
            anyhow::bail!(
                "cannot prune an empty candidate alias: round_id={}, peer_id=0x{}",
                round_id,
                crate::bytes_to_hex(peer_id)
            );
        }
        if address.first_advertised_at > address.last_advertised_at {
            anyhow::bail!(
                "candidate alias time regressed while pruning: round_id={}, peer_id=0x{}, alias={}, first_advertised_at={}, last_advertised_at={}",
                round_id,
                crate::bytes_to_hex(peer_id),
                address.addr,
                address.first_advertised_at,
                address.last_advertised_at
            );
        }
        if !aliases.insert(address.addr.as_str()) {
            anyhow::bail!(
                "candidate contains duplicate alias while pruning: round_id={}, peer_id=0x{}, alias={}",
                round_id,
                crate::bytes_to_hex(peer_id),
                address.addr
            );
        }
    }
    candidate
        .addresses
        .retain(|address| crawl_address_is_fresh(address, cutoff));
    let retained_aliases: std::collections::HashSet<&str> = candidate
        .addresses
        .iter()
        .map(|address| address.addr.as_str())
        .collect();
    candidate
        .advertisements
        .retain(|evidence| retained_aliases.contains(evidence.alias.as_str()));
    Ok(())
}

/// One positive Discovery observation, stored on the advertised target peer.
///
/// Counts use one observation per successful source-peer probe in which the
/// exact normalized alias appeared at least once. Repeated wire messages from
/// that same short-lived probe are unioned before this evidence is staged;
/// [`DiscoveryEvidence`] retains the exact message counters separately.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct AdvertisementEvidence {
    pub advertiser_peer_id: Vec<u8>,
    pub alias: String,
    pub first_observed_at: u64,
    pub last_observed_at: u64,
    pub first_observed_round: u64,
    pub last_observed_round: u64,
    pub observation_count: u64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum ActiveCandidateState {
    #[default]
    Pending,
    RetryAlias,
    Succeeded,
    Exhausted,
    ForeignNetwork,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum CompletedCandidateOutcome {
    SameNetworkIdentified,
    Exhausted,
    ForeignNetwork,
}

/// A successful probe staged until the complete logical round is atomically
/// published. Keeping it outside `CF_NET_NODES` prevents partial reachability
/// changes from leaking through the API secondary.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct StagedProbeOutcome {
    pub observed_at: u64,
    pub client_version: String,
    pub flags: u64,
    pub protocols: Vec<String>,
    pub own_addrs: Vec<String>,
    pub rtt_ms: Option<u32>,
    pub discovered_addrs: Vec<String>,
    pub discovery: DiscoveryEvidence,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ActiveCandidateProbe {
    pub round_id: u64,
    pub state: ActiveCandidateState,
    pub observations: Vec<AddressProbeEvidence>,
    pub staged_success: Option<StagedProbeOutcome>,
    /// Positive target-centric advertisements observed during this logical
    /// round. These remain active-round state until completed publication.
    pub staged_advertisements: Vec<AdvertisementEvidence>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CompletedCandidateEvidence {
    pub round_id: u64,
    pub outcome: CompletedCandidateOutcome,
    pub observations: Vec<AddressProbeEvidence>,
    pub consecutive_exhausted_rounds: u64,
}

/// Peer-keyed durable scheduler state in `CF_NET_CRAWL`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct CrawlCandidate {
    pub addresses: Vec<CrawlAddress>,
    pub first_discovered_at: u64,
    pub last_advertised_at: u64,
    pub last_scheduled_sequence: u64,
    pub active: Option<ActiveCandidateProbe>,
    pub last_completed: Option<CompletedCandidateEvidence>,
    /// Durable positive Discovery evidence keyed by `(advertiser, alias)`.
    /// A later random response that omits an alias never erases this history.
    pub advertisements: Vec<AdvertisementEvidence>,
    /// Current-round direct-session facts. Addressless candidates use this
    /// without acquiring scheduler `active` state.
    pub staged_direct_sessions: Vec<DirectSessionObservation>,
    /// Completed positive session evidence retained independently from dial
    /// aliases and from current crawler reachability.
    pub direct_sessions: Vec<DirectSessionEvidence>,
}

fn advertisement_key(evidence: &AdvertisementEvidence) -> (&[u8], &str) {
    (&evidence.advertiser_peer_id, evidence.alias.as_str())
}

fn checked_validate_advertisements(
    evidence: &[AdvertisementEvidence],
    addresses: &[CrawlAddress],
    round_id: u64,
    target_peer_id: &[u8],
    staged: bool,
) -> anyhow::Result<()> {
    let aliases: std::collections::HashSet<&str> = addresses
        .iter()
        .map(|address| address.addr.as_str())
        .collect();
    let mut prior_key: Option<(&[u8], &str)> = None;
    for item in evidence {
        if item.advertiser_peer_id.is_empty() {
            anyhow::bail!(
                "advertisement evidence has empty advertiser peer id: round_id={}, target_peer_id=0x{}, alias={}",
                round_id,
                crate::bytes_to_hex(target_peer_id),
                item.alias
            );
        }
        if item.alias.is_empty() {
            anyhow::bail!(
                "advertisement evidence has empty alias: round_id={}, target_peer_id=0x{}, advertiser_peer_id=0x{}",
                round_id,
                crate::bytes_to_hex(target_peer_id),
                crate::bytes_to_hex(&item.advertiser_peer_id)
            );
        }
        if !aliases.contains(item.alias.as_str()) {
            anyhow::bail!(
                "advertisement evidence references an unretained alias: round_id={}, target_peer_id=0x{}, advertiser_peer_id=0x{}, alias={}",
                round_id,
                crate::bytes_to_hex(target_peer_id),
                crate::bytes_to_hex(&item.advertiser_peer_id),
                item.alias
            );
        }
        if item.first_observed_at > item.last_observed_at {
            anyhow::bail!(
                "advertisement evidence time regressed: round_id={}, target_peer_id=0x{}, advertiser_peer_id=0x{}, alias={}, first_observed_at={}, last_observed_at={}",
                round_id,
                crate::bytes_to_hex(target_peer_id),
                crate::bytes_to_hex(&item.advertiser_peer_id),
                item.alias,
                item.first_observed_at,
                item.last_observed_at
            );
        }
        if item.first_observed_round > item.last_observed_round {
            anyhow::bail!(
                "advertisement evidence round regressed: round_id={}, target_peer_id=0x{}, advertiser_peer_id=0x{}, alias={}, first_observed_round={}, last_observed_round={}",
                round_id,
                crate::bytes_to_hex(target_peer_id),
                crate::bytes_to_hex(&item.advertiser_peer_id),
                item.alias,
                item.first_observed_round,
                item.last_observed_round
            );
        }
        if item.observation_count == 0 {
            anyhow::bail!(
                "advertisement evidence has zero observations: round_id={}, target_peer_id=0x{}, advertiser_peer_id=0x{}, alias={}",
                round_id,
                crate::bytes_to_hex(target_peer_id),
                crate::bytes_to_hex(&item.advertiser_peer_id),
                item.alias
            );
        }
        if staged {
            if item.first_observed_round != round_id || item.last_observed_round != round_id {
                anyhow::bail!(
                    "staged advertisement evidence round mismatch: round_id={}, target_peer_id=0x{}, advertiser_peer_id=0x{}, alias={}, first_observed_round={}, last_observed_round={}",
                    round_id,
                    crate::bytes_to_hex(target_peer_id),
                    crate::bytes_to_hex(&item.advertiser_peer_id),
                    item.alias,
                    item.first_observed_round,
                    item.last_observed_round
                );
            }
        } else if item.last_observed_round >= round_id {
            anyhow::bail!(
                "durable advertisement evidence is not from a prior round: round_id={}, target_peer_id=0x{}, advertiser_peer_id=0x{}, alias={}, last_observed_round={}",
                round_id,
                crate::bytes_to_hex(target_peer_id),
                crate::bytes_to_hex(&item.advertiser_peer_id),
                item.alias,
                item.last_observed_round
            );
        }
        let key = advertisement_key(item);
        if prior_key.is_some_and(|prior| prior >= key) {
            anyhow::bail!(
                "advertisement evidence is duplicate or not canonically sorted: round_id={}, target_peer_id=0x{}, advertiser_peer_id=0x{}, alias={}",
                round_id,
                crate::bytes_to_hex(target_peer_id),
                crate::bytes_to_hex(&item.advertiser_peer_id),
                item.alias
            );
        }
        prior_key = Some(key);
    }
    Ok(())
}

/// Merge one completed logical round's staged positive advertisements into
/// durable target-centric history. This is the single calculation path used by
/// the crawler publisher and the network-store commit validator.
pub fn checked_merge_advertisement_evidence(
    durable: &mut Vec<AdvertisementEvidence>,
    staged: &[AdvertisementEvidence],
    addresses: &[CrawlAddress],
    round_id: u64,
    target_peer_id: &[u8],
) -> anyhow::Result<()> {
    checked_validate_advertisements(durable, addresses, round_id, target_peer_id, false)?;
    checked_validate_advertisements(staged, addresses, round_id, target_peer_id, true)?;

    for observed in staged {
        match durable
            .binary_search_by(|prior| advertisement_key(prior).cmp(&advertisement_key(observed)))
        {
            Ok(index) => {
                let prior = &mut durable[index];
                if observed.first_observed_at < prior.last_observed_at {
                    anyhow::bail!(
                        "advertisement evidence time moved backwards across rounds: round_id={}, target_peer_id=0x{}, advertiser_peer_id=0x{}, alias={}, prior_last_observed_at={}, staged_first_observed_at={}",
                        round_id,
                        crate::bytes_to_hex(target_peer_id),
                        crate::bytes_to_hex(&observed.advertiser_peer_id),
                        observed.alias,
                        prior.last_observed_at,
                        observed.first_observed_at
                    );
                }
                prior.last_observed_at = observed.last_observed_at;
                prior.last_observed_round = observed.last_observed_round;
                prior.observation_count = prior
                    .observation_count
                    .checked_add(observed.observation_count)
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                            "advertisement observation count overflow: round_id={}, target_peer_id=0x{}, advertiser_peer_id=0x{}, alias={}",
                            round_id,
                            crate::bytes_to_hex(target_peer_id),
                            crate::bytes_to_hex(&observed.advertiser_peer_id),
                            observed.alias
                        )
                    })?;
            }
            Err(index) => durable.insert(index, observed.clone()),
        }
    }
    Ok(())
}

/// Build the single canonical retained-alias index used to resolve Discovery
/// advertisements into peer edges. One alias must identify exactly one peer.
pub fn checked_candidate_alias_map(
    candidates: &BTreeMap<Vec<u8>, CrawlCandidate>,
    round_id: u64,
) -> anyhow::Result<BTreeMap<String, Vec<u8>>> {
    let mut aliases = BTreeMap::new();
    for (peer_id, candidate) in candidates {
        for address in &candidate.addresses {
            if address.addr.is_empty() {
                anyhow::bail!(
                    "candidate alias is empty: round_id={}, peer_id=0x{}",
                    round_id,
                    crate::bytes_to_hex(peer_id)
                );
            }
            if let Some(prior_peer_id) = aliases.insert(address.addr.clone(), peer_id.clone()) {
                anyhow::bail!(
                    "candidate alias is not unique: round_id={}, addr={}, first_peer_id=0x{}, second_peer_id=0x{}",
                    round_id,
                    address.addr,
                    crate::bytes_to_hex(&prior_peer_id),
                    crate::bytes_to_hex(peer_id)
                );
            }
        }
    }
    Ok(aliases)
}

/// Resolve a successful peer's normalized Discovery addresses through the
/// canonical candidate alias map. Normalized addresses must already have been
/// admitted as candidates; an absent alias is a checkpoint invariant failure.
pub fn checked_resolve_known_peers(
    discovered_addrs: &[String],
    aliases: &BTreeMap<String, Vec<u8>>,
    round_id: u64,
    source_peer_id: &[u8],
) -> anyhow::Result<Vec<Vec<u8>>> {
    let mut peers = Vec::with_capacity(discovered_addrs.len());
    for address in discovered_addrs {
        let peer_id = aliases.get(address).ok_or_else(|| {
            anyhow::anyhow!(
                "normalized Discovery address is missing from candidate aliases: round_id={}, source_peer_id=0x{}, addr={}",
                round_id,
                crate::bytes_to_hex(source_peer_id),
                address
            )
        })?;
        peers.push(peer_id.clone());
    }
    peers.sort();
    peers.dedup();
    Ok(peers)
}

/// Read-only active-round progress exposed separately from the last completed
/// status. Counts are exact scans of peer-keyed candidate state.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct CrawlProgress {
    pub round_id: u64,
    pub started_at: u64,
    pub last_checkpoint_at: u64,
    pub candidate_peers: u64,
    pub completed_peers: u64,
    pub address_attempts: u64,
    pub blocked_reason: Option<String>,
}

/// One aggregated time-bucket value. Scalar metrics use `scalar`;
/// share metrics use `buckets` (top-N (label, count), descending).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct HistoryPoint {
    pub scalar: u64,
    pub buckets: Vec<(String, u64)>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn peer_outcome_matrix_preserves_the_observed_144_57_87_58_split() {
        let outcomes = CompletedPeerOutcomes {
            same_network_identified: 57,
            exhausted_with_retained_verification: 1,
            exhausted_without_retained_verification: 86,
            foreign_with_retained_verification: 0,
            foreign_without_retained_verification: 0,
        };

        assert_eq!(outcomes.candidate_peers(53).unwrap(), 144);
        assert_eq!(outcomes.reachable_peers(), 57);
        assert_eq!(outcomes.exhausted_candidates(53).unwrap(), 87);
        assert_eq!(outcomes.verified_unavailable_peers(53).unwrap(), 1);
        assert_eq!(outcomes.verified_retained_peers(53).unwrap(), 58);
    }

    #[test]
    fn address_histogram_records_every_typed_probe_milestone() {
        let mut histogram = AddressObservationHistogram::default();
        for result in [
            AddressProbeResult::DialRequestFailed,
            AddressProbeResult::NoAuthenticatedSessionBeforeDeadline,
            AddressProbeResult::AuthenticatedSessionWithoutIdentifyBeforeDeadline,
            AddressProbeResult::MalformedIdentify,
            AddressProbeResult::ForeignNetwork,
            AddressProbeResult::SameNetworkIdentified,
        ] {
            histogram.checked_record(result, 53).unwrap();
        }

        assert_eq!(histogram.address_attempts(53).unwrap(), 6);
        assert_eq!(histogram.non_successful_address_attempts(53).unwrap(), 5);
        assert_eq!(histogram.dial_request_failed, 1);
        assert_eq!(histogram.no_authenticated_session_before_deadline, 1);
        assert_eq!(
            histogram.authenticated_session_without_identify_before_deadline,
            1
        );
        assert_eq!(histogram.malformed_identify, 1);
        assert_eq!(histogram.foreign_network, 1);
        assert_eq!(histogram.same_network_identified, 1);
    }

    #[test]
    fn completed_outcome_must_match_its_address_evidence() {
        let addresses = [CrawlAddress {
            addr: "addrA".into(),
            first_advertised_at: 1,
            last_advertised_at: 2,
            last_verified_at: None,
        }];
        let observations = [AddressProbeEvidence {
            address: "addrA".into(),
            round_id: 53,
            observed_at: 2,
            elapsed_ms: 1,
            result: AddressProbeResult::DialRequestFailed,
        }];
        let error = AddressObservationHistogram::default()
            .checked_record_candidate(
                &observations,
                &addresses,
                CompletedCandidateOutcome::SameNetworkIdentified,
                53,
                b"peerA",
            )
            .unwrap_err();

        assert!(error
            .to_string()
            .contains("outcome does not match address evidence"));
    }

    #[test]
    fn starting_a_new_round_does_not_erase_completed_candidate_evidence() {
        let completed = CompletedCandidateEvidence {
            round_id: 52,
            outcome: CompletedCandidateOutcome::Exhausted,
            observations: vec![AddressProbeEvidence {
                address: "addrA".into(),
                round_id: 52,
                observed_at: 1_000,
                elapsed_ms: 15_000,
                result: AddressProbeResult::NoAuthenticatedSessionBeforeDeadline,
            }],
            consecutive_exhausted_rounds: 3,
        };
        let candidate = CrawlCandidate {
            active: Some(ActiveCandidateProbe {
                round_id: 53,
                ..Default::default()
            }),
            last_completed: Some(completed.clone()),
            ..Default::default()
        };

        assert_eq!(candidate.last_completed, Some(completed));
        assert_eq!(candidate.active.unwrap().round_id, 53);
    }

    #[test]
    fn advertisement_merge_preserves_first_positive_observation_and_exact_count() {
        let mut durable = vec![AdvertisementEvidence {
            advertiser_peer_id: b"source".to_vec(),
            alias: "addr-target".into(),
            first_observed_at: 100,
            last_observed_at: 100,
            first_observed_round: 4,
            last_observed_round: 4,
            observation_count: 1,
        }];
        let staged = vec![AdvertisementEvidence {
            advertiser_peer_id: b"source".to_vec(),
            alias: "addr-target".into(),
            first_observed_at: 200,
            last_observed_at: 200,
            first_observed_round: 6,
            last_observed_round: 6,
            observation_count: 1,
        }];

        checked_merge_advertisement_evidence(
            &mut durable,
            &staged,
            &[CrawlAddress {
                addr: "addr-target".into(),
                first_advertised_at: 100,
                last_advertised_at: 200,
                last_verified_at: None,
            }],
            6,
            b"target",
        )
        .unwrap();

        assert_eq!(durable.len(), 1);
        assert_eq!(durable[0].first_observed_at, 100);
        assert_eq!(durable[0].last_observed_at, 200);
        assert_eq!(durable[0].first_observed_round, 4);
        assert_eq!(durable[0].last_observed_round, 6);
        assert_eq!(durable[0].observation_count, 2);
    }

    fn direct_observation(round_id: u64, observed_at: u64) -> DirectSessionObservation {
        DirectSessionObservation {
            round_id,
            observed_at,
            observer_peer_id: b"observer".to_vec(),
            initiator: SessionInitiator::Peer,
            client_version: "ckb/0.119.0".into(),
            session_addresses: vec![],
            connected_duration_ms: 42,
            last_ping_duration_ms: None,
            protocols: vec![],
        }
    }

    #[test]
    fn addressless_direct_session_merge_preserves_direction_and_exact_round_count() {
        let mut durable = Vec::new();
        checked_merge_direct_session_evidence(
            &mut durable,
            &[direct_observation(5, 100)],
            5,
            b"target",
        )
        .unwrap();
        checked_merge_direct_session_evidence(
            &mut durable,
            &[direct_observation(6, 200)],
            6,
            b"target",
        )
        .unwrap();

        assert_eq!(durable.len(), 1);
        assert_eq!(durable[0].initiator, SessionInitiator::Peer);
        assert!(durable[0].session_addresses.is_empty());
        assert_eq!(durable[0].first_observed_round, 5);
        assert_eq!(durable[0].last_observed_round, 6);
        assert_eq!(durable[0].observation_count, 2);
    }

    #[test]
    fn local_observer_merge_counts_one_completed_sample_per_round() {
        let observation = LocalObserverObservation {
            round_id: 5,
            observed_at: 100,
            peer_id: b"observer".to_vec(),
            client_version: "ckb/0.119.0".into(),
            active: true,
            addresses: vec![],
            protocols: vec![],
            connections: 1,
        };
        let first = checked_merge_local_observer_evidence(None, &observation, 5).unwrap();
        let next_observation = LocalObserverObservation {
            round_id: 6,
            observed_at: 200,
            connections: 2,
            ..observation
        };
        let second =
            checked_merge_local_observer_evidence(Some(&first), &next_observation, 6).unwrap();

        assert_eq!(second.first_observed_at, 100);
        assert_eq!(second.last_observed_at, 200);
        assert_eq!(second.observation_count, 2);
        assert_eq!(second.connections, 2);
    }

    #[test]
    fn canonical_alias_map_resolves_sorted_deduplicated_peer_edges() {
        let candidates = BTreeMap::from([
            (
                b"B".to_vec(),
                CrawlCandidate {
                    addresses: vec![CrawlAddress {
                        addr: "addrB".into(),
                        ..Default::default()
                    }],
                    ..Default::default()
                },
            ),
            (
                b"C".to_vec(),
                CrawlCandidate {
                    addresses: vec![CrawlAddress {
                        addr: "addrC".into(),
                        ..Default::default()
                    }],
                    ..Default::default()
                },
            ),
        ]);
        let aliases = checked_candidate_alias_map(&candidates, 53).unwrap();

        assert_eq!(
            checked_resolve_known_peers(
                &["addrC".into(), "addrB".into(), "addrC".into()],
                &aliases,
                53,
                b"A"
            )
            .unwrap(),
            vec![b"B".to_vec(), b"C".to_vec()]
        );
    }

    #[test]
    fn canonical_alias_resolution_rejects_duplicates_and_missing_normalized_aliases() {
        let candidates = BTreeMap::from([
            (
                b"A".to_vec(),
                CrawlCandidate {
                    addresses: vec![CrawlAddress {
                        addr: "shared".into(),
                        ..Default::default()
                    }],
                    ..Default::default()
                },
            ),
            (
                b"B".to_vec(),
                CrawlCandidate {
                    addresses: vec![CrawlAddress {
                        addr: "shared".into(),
                        ..Default::default()
                    }],
                    ..Default::default()
                },
            ),
        ]);
        assert!(checked_candidate_alias_map(&candidates, 53)
            .unwrap_err()
            .to_string()
            .contains("alias is not unique"));

        let aliases = BTreeMap::from([("addrB".into(), b"B".to_vec())]);
        let error = checked_resolve_known_peers(&["addrX".into()], &aliases, 53, b"A").unwrap_err();
        assert!(error.to_string().contains("missing from candidate aliases"));
        assert!(error.to_string().contains("source_peer_id=0x41"));
    }

    #[test]
    fn node_record_bincode_roundtrip() {
        let rec = NodeRecord {
            own_addrs: vec!["/ip4/1.2.3.4/tcp/8115".into()],
            client_version: "0.119.0".into(),
            flags: 1,
            protocols: vec!["/ckb/sync".into()],
            first_seen: 100,
            last_seen: 200,
            last_reachable_at: 200,
            reachable: true,
            geo: Some(Geo {
                country: "US".into(),
                city: "NYC".into(),
                lat: 40.0,
                lon: -74.0,
            }),
            asn: Some(Asn {
                number: 65000,
                org: "Example".into(),
            }),
            last_rtt_ms: Some(42),
            discovery: DiscoveryEvidence {
                valid_nodes_messages: 1,
                valid_response_messages: 1,
                normalized_advertised_addresses: 1,
                ..Default::default()
            },
            known_peers: vec![vec![1, 2, 3]],
        };
        let bytes = bincode::serialize(&rec).unwrap();
        let back: NodeRecord = bincode::deserialize(&bytes).unwrap();
        assert_eq!(rec, back);
    }

    #[test]
    fn status_and_history_roundtrip() {
        let s = LatestStatus {
            round_id: 7,
            peer_outcomes: CompletedPeerOutcomes {
                same_network_identified: 3,
                ..Default::default()
            },
            ..Default::default()
        };
        assert_eq!(
            s,
            bincode::deserialize(&bincode::serialize(&s).unwrap()).unwrap()
        );
        let p = HistoryPoint {
            scalar: 9,
            buckets: vec![("0.119.0".into(), 5)],
        };
        assert_eq!(
            p,
            bincode::deserialize(&bincode::serialize(&p).unwrap()).unwrap()
        );
    }

    #[test]
    fn crawl_state_roundtrips() {
        let candidate = CrawlCandidate {
            addresses: vec![CrawlAddress {
                addr: "/ip4/1.2.3.4/tcp/8114/p2p/peer".into(),
                first_advertised_at: 1,
                last_advertised_at: 10,
                last_verified_at: None,
            }],
            first_discovered_at: 1,
            last_advertised_at: 10,
            last_scheduled_sequence: 9,
            active: Some(ActiveCandidateProbe {
                round_id: 4,
                state: ActiveCandidateState::Succeeded,
                observations: vec![AddressProbeEvidence {
                    address: "/ip4/1.2.3.4/tcp/8114/p2p/peer".into(),
                    round_id: 4,
                    observed_at: 10,
                    elapsed_ms: 42,
                    result: AddressProbeResult::SameNetworkIdentified,
                }],
                staged_success: Some(StagedProbeOutcome {
                    observed_at: 10,
                    client_version: "ckb/0.119.0".into(),
                    ..Default::default()
                }),
                staged_advertisements: vec![],
            }),
            last_completed: None,
            advertisements: vec![],
            staged_direct_sessions: vec![],
            direct_sessions: vec![],
        };
        assert_eq!(
            candidate,
            bincode::deserialize(&bincode::serialize(&candidate).unwrap()).unwrap()
        );
    }
}
