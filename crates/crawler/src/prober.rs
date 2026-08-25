use async_trait::async_trait;
use ckbadger_store::{AddressProbeResult, DiscoveryEvidence};

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
    pub discovery: DiscoveryEvidence,
}

#[derive(Debug, Clone)]
pub struct ProbeResult {
    pub(crate) observation: AddressProbeResult,
    pub(crate) elapsed_ms: u64,
    pub(crate) outcome: Option<ProbeOutcome>,
}

impl ProbeResult {
    pub fn reachable(outcome: ProbeOutcome, elapsed_ms: u64) -> Self {
        Self {
            observation: AddressProbeResult::SameNetworkIdentified,
            elapsed_ms,
            outcome: Some(outcome),
        }
    }

    pub fn failed(observation: AddressProbeResult, elapsed_ms: u64) -> anyhow::Result<Self> {
        if observation == AddressProbeResult::SameNetworkIdentified {
            anyhow::bail!("same-network observation requires a reachable outcome");
        }
        Ok(Self {
            observation,
            elapsed_ms,
            outcome: None,
        })
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
    /// Expected network outcomes are represented by [`AddressProbeResult`]; only
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
            discovery: DiscoveryEvidence::default(),
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
            discovery: DiscoveryEvidence::default(),
        };
        let result = ProbeResult::reachable(outcome, 42);
        assert_eq!(
            result.observation,
            AddressProbeResult::SameNetworkIdentified
        );
        assert!(result.outcome.is_some());
    }

    #[test]
    fn failure_result_rejects_same_network_success() {
        let error = ProbeResult::failed(AddressProbeResult::SameNetworkIdentified, 42)
            .expect_err("same-network success requires an outcome");
        assert!(error
            .to_string()
            .contains("same-network observation requires a reachable outcome"));
    }
}
