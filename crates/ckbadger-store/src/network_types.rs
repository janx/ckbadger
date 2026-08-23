//! Network-store value types. All use `bincode` serialization (see types.rs convention).
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
    /// Peer ids resolved from addresses in this node's Discovery response for
    /// this round. This is address-book gossip, not reachability proof.
    pub known_peers: Vec<Vec<u8>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct LatestStatus {
    pub round_id: u64,
    pub started: u64,
    pub finished: u64,
    pub candidate_peers: u64,
    pub attempted_peers: u64,
    pub reachable_peers: u64,
    pub unreachable_peers: u64,
    pub address_attempts: u64,
    pub failed_address_attempts: u64,
    pub foreign_peers: u64,
    pub malformed_addresses: u64,
    pub new_nodes: u64,
    pub total_known: u64,
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
    pub address_attempts: u64,
    pub failed_address_attempts: u64,
    pub foreign_peers: u64,
    pub malformed_addresses: u64,
    /// Set only when the scheduler cannot preserve its coverage invariant.
    /// The crawler exits with the same actionable reason instead of publishing
    /// a truncated snapshot.
    pub blocked_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct CrawlAddress {
    pub addr: String,
    pub last_advertised_at: u64,
    /// Logical round in which this exact address was most recently attempted.
    /// Round ids start at one, so zero means never attempted.
    pub attempted_round: u64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum CrawlCandidateResult {
    #[default]
    Pending,
    RetryAlias,
    Succeeded,
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
}

/// Peer-keyed durable scheduler state in `CF_NET_CRAWL`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct CrawlCandidate {
    pub addresses: Vec<CrawlAddress>,
    pub first_discovered_at: u64,
    pub last_advertised_at: u64,
    /// Round whose `result` and `staged_success` fields describe.
    pub round_id: u64,
    pub last_scheduled_sequence: u64,
    pub result: CrawlCandidateResult,
    /// At least one address authenticated this peer id but identified as a
    /// different CKB network in the current logical round.
    pub foreign_observed: bool,
    pub staged_success: Option<StagedProbeOutcome>,
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
            reachable_peers: 3,
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
                last_advertised_at: 10,
                attempted_round: 4,
            }],
            first_discovered_at: 1,
            last_advertised_at: 10,
            round_id: 4,
            last_scheduled_sequence: 9,
            result: CrawlCandidateResult::Succeeded,
            foreign_observed: true,
            staged_success: Some(StagedProbeOutcome {
                observed_at: 10,
                client_version: "ckb/0.119.0".into(),
                ..Default::default()
            }),
        };
        assert_eq!(
            candidate,
            bincode::deserialize(&bincode::serialize(&candidate).unwrap()).unwrap()
        );
    }
}
