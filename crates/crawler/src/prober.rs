use async_trait::async_trait;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeCandidate {
    pub peer_id: Vec<u8>,
    pub addr: String,
}

/// Everything learned from a single successful probe of one address.
#[derive(Debug, Clone)]
pub struct ProbeOutcome {
    pub peer_id: Vec<u8>,
    pub client_version: String,
    pub flags: u64,
    pub protocols: Vec<String>,
    pub own_addrs: Vec<String>,
    pub rtt_ms: Option<u32>,
    /// Addresses returned by the Discovery `Nodes` response.
    pub discovered_addrs: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbeObservation {
    Reachable,
    DialFailed,
    TimedOut,
    MalformedAddress,
    ForeignNetwork,
    HandshakeRejected,
}

#[derive(Debug, Clone)]
pub struct ProbeResult {
    pub observation: ProbeObservation,
    pub outcome: Option<ProbeOutcome>,
}

impl ProbeResult {
    pub fn reachable(outcome: ProbeOutcome) -> Self {
        Self {
            observation: ProbeObservation::Reachable,
            outcome: Some(outcome),
        }
    }

    pub fn failed(observation: ProbeObservation) -> Self {
        debug_assert_ne!(observation, ProbeObservation::Reachable);
        Self {
            observation,
            outcome: None,
        }
    }
}

#[async_trait]
pub trait Prober: Send + Sync {
    /// Normalize one address into a peer-keyed dial candidate. A peer hint is
    /// available for authenticated Identify `own_addrs`, which may omit `/p2p`.
    /// `Ok(None)` is malformed/unkeyed remote input; `Err` is a local invariant
    /// violation (for example a corrupt persisted peer id).
    fn candidate_from_addr(
        &self,
        addr: &str,
        peer_hint: Option<&[u8]>,
    ) -> anyhow::Result<Option<ProbeCandidate>>;

    /// Dial `addr`, run Identify+Discovery, then disconnect (feeler style).
    /// Expected network outcomes are represented by [`ProbeObservation`]; only
    /// local/prober invariant failures return `Err`.
    async fn probe(&self, peer_id: &[u8], addr: &str) -> anyhow::Result<ProbeResult>;

    /// Seed (bootnode) addresses for a cold start.
    fn bootnodes(&self) -> Vec<String>;
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn probe_outcome_is_constructible() {
        let o = ProbeOutcome {
            peer_id: vec![1],
            client_version: "0.119.0".into(),
            flags: 0,
            protocols: vec![],
            own_addrs: vec![],
            rtt_ms: None,
            discovered_addrs: vec![],
        };
        assert_eq!(o.peer_id, vec![1]);
    }

    #[test]
    fn reachable_result_always_contains_an_outcome() {
        let outcome = ProbeOutcome {
            peer_id: vec![1],
            client_version: String::new(),
            flags: 0,
            protocols: vec![],
            own_addrs: vec![],
            rtt_ms: None,
            discovered_addrs: vec![],
        };
        let result = ProbeResult::reachable(outcome);
        assert_eq!(result.observation, ProbeObservation::Reachable);
        assert!(result.outcome.is_some());
    }
}
