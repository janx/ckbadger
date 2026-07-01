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
    /// peer_id bytes of reachable peers whose address appeared in this node's
    /// Discovery response THIS round (fresh sample, replaced each round).
    pub known_peers: Vec<Vec<u8>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct LatestStatus {
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
            reachable: 3,
            frontier_drained: false,
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
}
