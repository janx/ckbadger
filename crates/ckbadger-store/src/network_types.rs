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
    pub malformed_messages: u64,
    pub unexpected_messages: u64,
    /// Distinct normalized peer-keyed addresses accepted by the crawler.
    pub normalized_advertised_addresses: u64,
    pub rejected_advertised_addresses: u64,
}

impl DiscoveryEvidence {
    pub fn checked_add_assign(&mut self, rhs: &Self, round_id: u64) -> anyhow::Result<()> {
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
        add!(malformed_messages);
        add!(unexpected_messages);
        add!(normalized_advertised_addresses);
        add!(rejected_advertised_addresses);
        Ok(())
    }
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
            }),
            last_completed: None,
        };
        assert_eq!(
            candidate,
            bincode::deserialize(&bincode::serialize(&candidate).unwrap()).unwrap()
        );
    }
}
