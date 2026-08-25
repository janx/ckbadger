use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use ckbadger_store::AddressProbeResult;

use crate::prober::{ProbeCandidate, ProbeOutcome, ProbeResult, Prober};

/// Deterministic in-memory prober for tests. `graph` maps addr -> outcome for
/// reachable addresses; any addr not present returns a typed dial failure.
pub struct MockProber {
    bootnodes: Vec<String>,
    graph: HashMap<String, ProbeOutcome>,
    attempts: Arc<Mutex<Vec<String>>>,
}

impl MockProber {
    pub fn new(bootnodes: Vec<String>, graph: HashMap<String, ProbeOutcome>) -> Self {
        Self {
            bootnodes,
            graph,
            attempts: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn attempts(&self) -> Vec<String> {
        self.attempts
            .lock()
            .expect("mock prober attempts poisoned")
            .clone()
    }
}

#[async_trait]
impl Prober for MockProber {
    fn candidate_from_addr(
        &self,
        addr: &str,
        peer_hint: Option<&[u8]>,
    ) -> anyhow::Result<Option<ProbeCandidate>> {
        if addr.is_empty() {
            return Ok(None);
        }
        let peer_id = match peer_hint {
            Some([]) => {
                anyhow::bail!("mock candidate received an empty peer hint")
            }
            Some(peer_id) => peer_id.to_vec(),
            None => self
                .graph
                .get(addr)
                .map(|outcome| outcome.peer_id.clone())
                .unwrap_or_else(|| addr.as_bytes().to_vec()),
        };
        Ok(Some(ProbeCandidate {
            peer_id,
            addr: addr.to_string(),
        }))
    }

    async fn probe(&self, peer_id: &[u8], addr: &str) -> anyhow::Result<ProbeResult> {
        self.attempts
            .lock()
            .expect("mock prober attempts poisoned")
            .push(addr.to_string());
        match self.graph.get(addr).cloned() {
            Some(outcome) => {
                if outcome.peer_id != peer_id {
                    anyhow::bail!(
                        "mock graph peer mismatch: addr={}, expected={:?}, actual={:?}",
                        addr,
                        peer_id,
                        outcome.peer_id
                    );
                }
                Ok(ProbeResult::reachable(outcome, 1))
            }
            None => ProbeResult::failed(AddressProbeResult::DialRequestFailed, 1),
        }
    }
    fn bootnodes(&self) -> Vec<String> {
        self.bootnodes.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prober::Prober;
    use ckbadger_store::DiscoveryEvidence;

    #[tokio::test]
    async fn reachable_addr_returns_outcome_absent_returns_dial_failure() {
        let mut g = std::collections::HashMap::new();
        g.insert("a".to_string(), out(b"A", &["b"]));
        let p = MockProber::new(vec!["a".into()], g);
        assert_eq!(p.bootnodes(), vec!["a".to_string()]);
        assert_eq!(
            p.probe(b"A", "a").await.unwrap().observation,
            AddressProbeResult::SameNetworkIdentified
        );
        assert_eq!(
            p.probe(b"missing", "missing").await.unwrap().observation,
            AddressProbeResult::DialRequestFailed
        );
    }

    fn out(peer: &[u8], disc: &[&str]) -> ProbeOutcome {
        ProbeOutcome {
            peer_id: peer.to_vec(),
            client_version: "0.119.0".into(),
            flags: 0,
            protocols: vec![],
            own_addrs: vec![],
            rtt_ms: Some(1),
            discovered_addrs: disc.iter().map(|s| s.to_string()).collect(),
            discovery: DiscoveryEvidence::default(),
        }
    }
}
