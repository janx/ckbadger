use async_trait::async_trait;

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

#[async_trait]
pub trait Prober: Send + Sync {
    /// Dial `addr`, run Identify+Ping+Discovery, then disconnect (feeler style).
    /// `Ok(Some)` = reachable (handshake completed).
    /// `Ok(None)` = unreachable/refused/timeout — a normal recorded observation, NOT an error.
    /// `Err` = prober-internal invariant failure only.
    async fn probe(&self, addr: &str) -> anyhow::Result<Option<ProbeOutcome>>;

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
}
