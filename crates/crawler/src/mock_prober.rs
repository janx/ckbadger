use std::collections::HashMap;

use async_trait::async_trait;

use crate::prober::{ProbeOutcome, Prober};

/// Deterministic in-memory prober for tests. `graph` maps addr -> outcome for
/// reachable addresses; any addr not present probes as unreachable (`Ok(None)`).
pub struct MockProber {
    bootnodes: Vec<String>,
    graph: HashMap<String, ProbeOutcome>,
}

impl MockProber {
    pub fn new(bootnodes: Vec<String>, graph: HashMap<String, ProbeOutcome>) -> Self {
        Self { bootnodes, graph }
    }
}

#[async_trait]
impl Prober for MockProber {
    async fn probe(&self, addr: &str) -> anyhow::Result<Option<ProbeOutcome>> {
        Ok(self.graph.get(addr).cloned())
    }
    fn bootnodes(&self) -> Vec<String> {
        self.bootnodes.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prober::Prober;

    #[tokio::test]
    async fn reachable_addr_returns_outcome_absent_returns_none() {
        let mut g = std::collections::HashMap::new();
        g.insert("a".to_string(), out(b"A", &["b"]));
        let p = MockProber::new(vec!["a".into()], g);
        assert_eq!(p.bootnodes(), vec!["a".to_string()]);
        assert!(p.probe("a").await.unwrap().is_some());
        assert!(p.probe("missing").await.unwrap().is_none());
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
        }
    }
}
