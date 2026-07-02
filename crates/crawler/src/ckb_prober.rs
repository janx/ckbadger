//! Real `ckb-network` 0.119 prober: a discovery-only, feeler-style probe of CKB L1 peers.
//!
//! # How this maps onto the `ckb-network` API
//!
//! `ckb-network` ships the Identify / Ping / Discovery / Feeler / DisconnectMessage protocols as
//! *built-in* handlers registered inside [`NetworkService::new`], driven by
//! `config.support_protocols`. They cannot be replaced by user handlers (Identify is even
//! registered unconditionally). So the "capture per connected peer" the crawler needs is split:
//!
//! * **Identify + Ping** are captured by the built-in handlers into [`NetworkState`]'s peer
//!   registry. We read them back per peer via [`NetworkController::connected_peers`] →
//!   [`ckb_network::Peer`] (`identify_info` = client version + flags, `listened_addrs` = the
//!   node's own listen multiaddrs, `ping_rtt`, negotiated `protocols`).
//! * **Discovery `Nodes`** land only in the `pub(crate)` peer store, which is *not* readable from
//!   outside the crate. So we register our own discovery-only [`CKBProtocolHandler`] on the
//!   Discovery protocol id: on `connected` it sends a `GetNodes` request and on `received` it
//!   decodes the `Nodes` reply and records the advertised addresses, keyed by session. This is the
//!   feeler interrogation; we then disconnect. (`GetNodes` is honored by peers only for *outbound*
//!   dials — RFC 0012 — which every crawler dial is.)
//!
//! The network identifier is derived from the selected network's spec id + genesis hash exactly as
//! CKB does (`/{spec_id}/{genesis_hash[..8]}`). It is announced in Identify, so `ckb-network`
//! auto-rejects (and bans) foreign-network peers for us.
//!
//! A single long-lived [`NetworkController`] backs every probe; the engine drives probes
//! sequentially, so [`probe`](CkbProber::probe) dials one address, waits (bounded by
//! `dial_timeout_secs`) for the handshake, assembles a [`ProbeOutcome`], disconnects, and returns
//! `Ok(None)` on timeout/refusal — never an error (the trait reserves `Err` for prober-internal
//! invariant failures only).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;

use ckb_app_config::{NetworkConfig, SupportProtocol};
use ckb_network::{
    bytes::Bytes, extract_peer_id, multiaddr::Multiaddr, CKBProtocol, CKBProtocolContext,
    CKBProtocolHandler, Flags, NetworkController, NetworkService, NetworkState, Peer, PeerId,
    PeerIndex, ProtocolId, SupportProtocols,
};
use ckb_types::{packed, prelude::*};
use ckbadger_config::CrawlerConfig;

use crate::prober::{ProbeOutcome, Prober};

// ---------------------------------------------------------------------------
// Network identifiers + built-in bootnodes (from the CKB v0.119.0 chain specs
// and `resource/ckb.toml`). identify_name = `/{spec_id}/{genesis_hash[..8]}`.
// ---------------------------------------------------------------------------

const MAINNET_SPEC_ID: &str = "ckb";
const MAINNET_GENESIS_HASH: &str =
    "92b197aa1fba0f63633922c61c92375c9c074a93e85963554f5499fe1450d0e5";
const TESTNET_SPEC_ID: &str = "ckb_testnet";
const TESTNET_GENESIS_HASH: &str =
    "10639e0895502b5688a6be8cf69460d76541bfa4821629d86d62ba0aae3f9606";

/// Built-in mainnet bootnodes (CKB v0.119.0 `resource/ckb.toml`).
const MAINNET_BOOTNODES: &[&str] = &[
    "/ip4/16.163.82.218/tcp/8114/p2p/QmaZMemLXSsxKUrYNucjEbPxVX3rBKsGhWW2muWtWxUWyh",
    "/ip4/35.79.196.111/tcp/8114/p2p/QmYCRVonLfP18LSoz2WCHaXDorUYxuUMfhtcXK1TuZ1iwF",
    "/ip4/13.234.144.148/tcp/8114/p2p/QmbT7QimcrcD5k2znoJiWpxoESxang6z1Gy9wof1rT1LKR",
    "/ip4/34.64.120.143/tcp/8114/p2p/QmejEJEbDcGGMp4D6WtftMMVLkR1ZuBfMgyLFDMJymkDt6",
    "/ip4/3.218.170.86/tcp/8114/p2p/QmShw2vtVt49wJagc1zGQXGS6LkQTcHxnEV3xs6y8MAmQN",
    "/ip4/35.236.107.161/tcp/8114/p2p/QmSRj57aa9sR2AiTvMyrEea8n1sEM1cDTrfb2VHVJxnGuu",
    "/ip4/23.101.191.12/tcp/8114/p2p/QmexvXVDiRt2FBGptgK4gBJusWyyTEEaHeuCAa35EPNkZS",
    "/ip4/20.151.143.237/tcp/8114/p2p/QmNsGNQjYA6iP472bNnNE2GR31kCYBifhY1XcaUxRjZ1py",
    "/ip4/52.59.155.249/tcp/8114/p2p/QmRHqhSGMGm5FtnkW8D6T83X7YwaiMAZXCXJJaKzQEo3rb",
    "/ip4/3.10.216.39/tcp/8114/p2p/QmagxSv7GNwKXQE7mi1iDjFHghjUpbqjBgqSot7PmMJqHA",
    "/ip4/13.37.172.80/tcp/8114/p2p/QmXJg4iKbQzMpLhX75RyDn89Mv7N2H8vLePBR7kgZf6hYk",
    "/ip4/34.118.49.255/tcp/8114/p2p/QmeCzzVmSAU5LNYAeXhdJj8TCq335aJMqUxcvZXERBWdgS",
    "/ip4/40.115.75.216/tcp/8114/p2p/QmW3P1WYtuz9hitqctKnRZua2deHXhNePNjvtc9Qjnwp4q",
    "/ip4/34.176.239.95/tcp/8114/p2p/QmQoWrmuFauCn3zZ2mYYKAciG9opTbjzC2wVEfWveZNDt8",
    "/ip4/13.245.217.98/tcp/8114/p2p/Qmf4t1SzFhRWuGcFcgs7r4pXvkACsz3FcaBMcmMKQMMpn7",
];

/// Built-in testnet bootnodes (CKB v0.119.0 `resource/ckb.toml`).
const TESTNET_BOOTNODES: &[&str] = &[
    "/ip4/18.217.146.65/tcp/8111/p2p/QmT6DFfm18wtbJz3y4aPNn3ac86N4d4p4xtfQRRPf73frC",
    "/ip4/18.136.60.221/tcp/8111/p2p/QmTt6HeNakL8Fpmevrhdna7J4NzEMf9pLchf1CXtmtSrwb",
    "/ip4/35.176.207.239/tcp/8111/p2p/QmSJTsMsMGBjzv1oBNwQU36VhQRxc2WQpFoRu1ZifYKrjZ",
    "/ip4/13.228.149.113/tcp/8111/p2p/QmQoTR39rBkpZVgLApDGDoFnJ2YDBS9hYeiib1Z6aoAdEf",
    "/ip4/157.241.73.87/tcp/8111/p2p/QmSPkAyXqsWpRiS7HpHLTProVdhQWLKFHCXbRjaLpJj7ZL",
    "/ip4/4.241.132.26/tcp/8111/p2p/QmX5D6aJiAQ5Fxn4BfVqSn6zrgyuQM1oXVC9yvmzLuHXnx",
    "/ip4/52.147.120.180/tcp/8111/p2p/QmPcJY2gZLUm66szYA9QaG1P3rzwseWCMgbj6AyNCyW4G2",
    "/ip4/18.167.196.121/tcp/8111/p2p/QmQMjFrNGaphzfHin3mbYybbJcFMDUihKAcknquYvm9J3W",
    "/ip4/34.216.103.183/tcp/8111/p2p/Qmd41MaByDprkC5gP1XBKgamZ9DTLNk37zbPgwtiWCzRV6",
    "/ip4/3.98.152.180/tcp/8111/p2p/QmWVuW5KquiWDSqgMJRFW1xRtVqkYJrWz6S9NNk6fFn3wh",
    "/ip4/18.192.147.65/tcp/8111/p2p/QmWcEhsMNRcfJit62EbKgzpgtAJZX1G3Ur4shXjcvLsYDb",
    "/ip4/13.236.13.195/tcp/8111/p2p/QmfUTZxsse7rFJTJfoUv8bbStoDLETxst5nJEpJozNuAnH",
];

/// Version string used to build the discovery `GetNodes` request (`REUSE_PORT_VERSION`).
const GET_NODES_VERSION: u32 = 1;
/// Maximum number of addresses to request in one `GetNodes` (matches `MAX_ADDR_TO_SEND`).
const GET_NODES_COUNT: u32 = 1000;
/// After the handshake completes (node is reachable), how long to wait for the `Nodes` reply
/// before returning. Bounded and independent of the reachability timeout.
const DISCOVERY_GRACE: Duration = Duration::from_secs(3);
/// Poll interval while waiting for handshake / discovery.
const POLL_INTERVAL: Duration = Duration::from_millis(150);

fn identify_name(spec_id: &str, genesis_hash: &str) -> String {
    format!("/{}/{}", spec_id, &genesis_hash[..8])
}

/// Per-session capture of discovery-advertised addresses: `session id → advertised multiaddrs`.
type DiscoveryCapture = Arc<Mutex<HashMap<PeerIndex, Vec<String>>>>;

// ---------------------------------------------------------------------------
// Discovery-only protocol handler
// ---------------------------------------------------------------------------

/// A discovery-only [`CKBProtocolHandler`]: on connect it asks the peer for its address book
/// (`GetNodes`) and records the `Nodes` reply per session. Registered on the Discovery protocol
/// id in place of the built-in discovery handler so the crawler can read the reply back.
struct DiscoveryProbeHandler {
    capture: DiscoveryCapture,
}

#[async_trait]
impl CKBProtocolHandler for DiscoveryProbeHandler {
    async fn init(&mut self, _nc: Arc<dyn CKBProtocolContext + Sync>) {}

    async fn connected(
        &mut self,
        nc: Arc<dyn CKBProtocolContext + Sync>,
        peer_index: PeerIndex,
        _version: &str,
    ) {
        // Feeler interrogation: request the peer's address book. Honored because we are the
        // outbound side of the connection (RFC 0012).
        if let Err(err) = nc
            .async_send_message_to(peer_index, encode_get_nodes())
            .await
        {
            tracing::debug!(?err, %peer_index, "crawler: send GetNodes failed");
        }
    }

    async fn received(
        &mut self,
        _nc: Arc<dyn CKBProtocolContext + Sync>,
        peer_index: PeerIndex,
        data: Bytes,
    ) {
        let addrs = decode_nodes_addrs(&data);
        if addrs.is_empty() {
            return;
        }
        if let Ok(mut map) = self.capture.lock() {
            map.entry(peer_index).or_default().extend(addrs);
        }
    }

    async fn disconnected(
        &mut self,
        _nc: Arc<dyn CKBProtocolContext + Sync>,
        peer_index: PeerIndex,
    ) {
        if let Ok(mut map) = self.capture.lock() {
            map.remove(&peer_index);
        }
    }
}

/// Encode a discovery `GetNodes` request. Mirrors `ckb-network`'s own encoding, reusing the
/// molecule types re-exported through `ckb_types::packed`.
fn encode_get_nodes() -> Bytes {
    let listen_port = packed::PortOpt::new_builder().set(None).build();
    let get_nodes2 = packed::GetNodes2::new_builder()
        .listen_port(listen_port)
        .count(GET_NODES_COUNT.pack())
        .version(GET_NODES_VERSION.pack())
        // Empty required flags → ask the peer for its full address book.
        .required_flags(0u64.pack())
        .build();
    let get_nodes = packed::GetNodes::new_unchecked(get_nodes2.as_bytes());
    let payload = packed::DiscoveryPayload::new_builder()
        .set(get_nodes)
        .build();
    let msg = packed::DiscoveryMessage::new_builder()
        .payload(payload)
        .build();
    Bytes::from(msg.as_bytes().to_vec())
}

/// Decode a discovery message and, if it is a `Nodes` reply, return the advertised multiaddrs as
/// strings. Anything else (including `GetNodes` or malformed data) yields an empty vec.
fn decode_nodes_addrs(data: &Bytes) -> Vec<String> {
    let reader = match packed::DiscoveryMessageReader::from_compatible_slice(data.as_ref()) {
        Ok(r) => r,
        Err(_) => return Vec::new(),
    };
    let mut out = Vec::new();
    if let packed::DiscoveryPayloadUnionReader::Nodes(nodes) = reader.payload().to_enum() {
        for node in nodes.items().iter() {
            for addr in node.addresses().iter() {
                if let Ok(ma) = Multiaddr::try_from(addr.raw_data().to_vec()) {
                    out.push(ma.to_string());
                }
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Spawn adapter — `ckb-network` starts on a `ckb_spawn::Spawn` handle.
// ---------------------------------------------------------------------------

/// Minimal [`ckb_spawn::Spawn`] over `tokio::spawn`. Requires an active tokio runtime, which is
/// guaranteed because [`CkbProber::new`] is only called from the async crawler service loop.
struct TokioSpawn;

impl ckb_spawn::Spawn for TokioSpawn {
    fn spawn_task<F>(&self, task: F)
    where
        F: std::future::Future<Output = ()> + Send + 'static,
    {
        tokio::spawn(task);
    }
}

// ---------------------------------------------------------------------------
// CkbProber
// ---------------------------------------------------------------------------

/// Real prober backed by a long-lived `ckb-network` [`NetworkController`].
pub struct CkbProber {
    controller: NetworkController,
    network_state: Arc<NetworkState>,
    capture: DiscoveryCapture,
    /// protocol id → protocol name, for reporting a peer's negotiated protocols.
    proto_names: HashMap<ProtocolId, String>,
    bootnodes: Vec<String>,
    dial_timeout: Duration,
}

impl CkbProber {
    /// Build a prober for `network` ("mainnet"/"testnet").
    ///
    /// Fails fast if the network is unknown or no bootnodes can be resolved (config override else
    /// built-in defaults). Must be called from within a tokio runtime.
    pub fn new(network: &str, cfg: &CrawlerConfig) -> Result<Self> {
        let (spec_id, genesis_hash, builtin_bootnodes) = match network {
            "mainnet" => (MAINNET_SPEC_ID, MAINNET_GENESIS_HASH, MAINNET_BOOTNODES),
            "testnet" => (TESTNET_SPEC_ID, TESTNET_GENESIS_HASH, TESTNET_BOOTNODES),
            other => {
                return Err(anyhow!(
                    "unsupported crawler network '{other}': expected 'mainnet' or 'testnet'"
                ))
            }
        };

        // Bootnodes: config override wins, else the built-in defaults. No seeds ⇒ fail fast.
        let bootnodes: Vec<String> = if cfg.bootnodes.is_empty() {
            builtin_bootnodes.iter().map(|s| s.to_string()).collect()
        } else {
            cfg.bootnodes.clone()
        };
        if bootnodes.is_empty() {
            return Err(anyhow!(
                "crawler cannot start: no bootnodes for network '{network}' \
                 (set [crawler].bootnodes or use a network with built-in bootnodes)"
            ));
        }

        let net_id = identify_name(spec_id, genesis_hash);

        // Ephemeral p2p identity + peer store live under a per-network temp dir. This is
        // network-layer state (exempt from the chain stores' persistence rules); losing it only
        // costs a fresh node id next start.
        let p2p_dir = std::env::temp_dir().join("ckbadger-crawler").join(network);
        std::fs::create_dir_all(&p2p_dir)
            .with_context(|| format!("create crawler p2p dir {}", p2p_dir.display()))?;

        let config = build_network_config(p2p_dir);
        // required_flags = empty ⇒ we interrogate every peer that identifies (no capability
        // gate); ckb2023 left false ⇒ no protocol-version gate on our side.
        let network_state = Arc::new(
            NetworkState::from_config(config)
                .map_err(|e| anyhow!("init crawler network state: {e}"))?
                .required_flags(Flags::empty()),
        );

        // Custom discovery handler in place of the built-in one (Discovery omitted from
        // config.support_protocols so there is no protocol-id collision).
        let capture: DiscoveryCapture = Arc::new(Mutex::new(HashMap::new()));
        let discovery = CKBProtocol::new_with_support_protocol(
            SupportProtocols::Discovery,
            Box::new(DiscoveryProbeHandler {
                capture: Arc::clone(&capture),
            }),
            Arc::clone(&network_state),
        );

        // Announce all capability flags so the widest set of peers (full nodes require
        // SYNC|DISCOVERY|RELAY) accept us and open Discovery; we never open Sync/Relay ourselves.
        let announce = (
            net_id,
            format!("ckbadger-crawler/{}", env!("CARGO_PKG_VERSION")),
            Flags::all(),
        );
        let controller = NetworkService::new(
            Arc::clone(&network_state),
            vec![discovery],
            Vec::new(), // no required protocols → peers are not evicted for missing Sync
            announce,
        )
        .start(&TokioSpawn)
        .map_err(|e| anyhow!("start crawler network service: {e}"))?;

        let proto_names = controller
            .protocols()
            .into_iter()
            .map(|(id, name, _versions)| (id, name))
            .collect();

        Ok(Self {
            controller,
            network_state,
            capture,
            proto_names,
            bootnodes,
            dial_timeout: Duration::from_secs(cfg.dial_timeout_secs.max(1)),
        })
    }

    /// Find the currently-connected peer for `peer_id`, if any.
    fn find_peer(&self, peer_id: &PeerId) -> Option<(PeerIndex, Peer)> {
        self.controller
            .connected_peers()
            .into_iter()
            .find(|(_, p)| extract_peer_id(&p.connected_addr).as_ref() == Some(peer_id))
    }

    /// Snapshot the discovery addresses captured for `session` so far.
    fn peek_discovered(&self, session: PeerIndex) -> Vec<String> {
        self.capture
            .lock()
            .ok()
            .and_then(|m| m.get(&session).cloned())
            .unwrap_or_default()
    }

    /// Disconnect `peer_id` (feeler behaviour) and drop its capture entry.
    fn disconnect(&self, peer_id: &PeerId) {
        if let Some((session, _)) = self.find_peer(peer_id) {
            if let Ok(mut m) = self.capture.lock() {
                m.remove(&session);
            }
            self.controller.remove_node(peer_id);
        }
    }

    /// Block until the peer completes Identify (= handshake done = reachable), returning its
    /// session id and a snapshot of its peer record.
    async fn await_identify(&self, peer_id: &PeerId) -> (PeerIndex, Peer) {
        loop {
            if let Some((session, peer)) = self.find_peer(peer_id) {
                if peer.identify_info.is_some() {
                    return (session, peer);
                }
            }
            tokio::time::sleep(POLL_INTERVAL).await;
        }
    }

    /// Wait a bounded grace period for the discovery `Nodes` reply for `session`.
    async fn await_discovery(&self, session: PeerIndex) -> Vec<String> {
        let deadline = tokio::time::Instant::now() + DISCOVERY_GRACE;
        loop {
            let discovered = self.peek_discovered(session);
            if !discovered.is_empty() || tokio::time::Instant::now() >= deadline {
                return discovered;
            }
            tokio::time::sleep(POLL_INTERVAL).await;
        }
    }

    /// Assemble a [`ProbeOutcome`] from a reachable peer's record + captured discovery addresses.
    fn assemble(&self, peer_id: &PeerId, peer: &Peer, mut discovered: Vec<String>) -> ProbeOutcome {
        let info = peer.identify_info.as_ref();
        let protocols = peer
            .protocols
            .keys()
            .filter_map(|id| self.proto_names.get(id).cloned())
            .collect();
        discovered.sort();
        discovered.dedup();
        ProbeOutcome {
            peer_id: peer_id.as_bytes().to_vec(),
            client_version: info.map(|i| i.client_version.clone()).unwrap_or_default(),
            flags: info.map(|i| i.flags.bits()).unwrap_or(0),
            protocols,
            own_addrs: peer.listened_addrs.iter().map(|a| a.to_string()).collect(),
            rtt_ms: peer
                .ping_rtt
                .map(|d| u32::try_from(d.as_millis()).unwrap_or(u32::MAX)),
            discovered_addrs: discovered,
        }
    }
}

#[async_trait]
impl Prober for CkbProber {
    async fn probe(&self, addr: &str) -> Result<Option<ProbeOutcome>> {
        // Malformed address or one lacking a peer id is an unreachable observation, not an error.
        let multiaddr: Multiaddr = match addr.parse() {
            Ok(m) => m,
            Err(_) => return Ok(None),
        };
        let peer_id = match extract_peer_id(&multiaddr) {
            Some(p) => p,
            None => return Ok(None),
        };

        // Outbound identify dial; on identify success ckb-network auto-opens Discovery + Ping.
        self.network_state
            .dial_identify(self.controller.p2p_control(), multiaddr);

        // Phase 1: bounded wait for the handshake. Timeout ⇒ unreachable (Ok(None)).
        let reached = tokio::time::timeout(self.dial_timeout, self.await_identify(&peer_id)).await;
        let (session, peer) = match reached {
            Ok(v) => v,
            Err(_elapsed) => {
                self.disconnect(&peer_id);
                return Ok(None);
            }
        };

        // Phase 2: node is reachable; give the Discovery reply a brief, bounded grace.
        let discovered = self.await_discovery(session).await;
        // Refresh the peer record so a late-arriving ping rtt is reflected.
        let peer = self.find_peer(&peer_id).map(|(_, p)| p).unwrap_or(peer);
        let outcome = self.assemble(&peer_id, &peer, discovered);

        self.disconnect(&peer_id);
        Ok(Some(outcome))
    }

    fn bootnodes(&self) -> Vec<String> {
        self.bootnodes.clone()
    }
}

/// Build the `ckb-network` config for a discovery-only, outbound-only feeler crawler.
fn build_network_config(path: PathBuf) -> NetworkConfig {
    NetworkConfig {
        path,
        // All connections are outbound (we dial); no inbound slots.
        max_peers: 4096,
        max_outbound_peers: 4096,
        ping_interval_secs: 15,
        ping_timeout_secs: 20,
        // 0 ⇒ disable the auto outbound-peer service; the engine drives all dials explicitly.
        connect_outbound_interval_secs: 0,
        // Built-in protocols to register. Discovery is intentionally omitted (custom handler);
        // Identify is always registered by ckb-network regardless. No Sync/Relay/Feeler.
        support_protocols: vec![
            SupportProtocol::Ping,
            SupportProtocol::Identify,
            SupportProtocol::DisconnectMessage,
        ],
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identify_name_matches_ckb_format() {
        // `/{spec_id}/{genesis_hash[..8]}` — the exact string CKB nodes announce in Identify.
        assert_eq!(
            identify_name(MAINNET_SPEC_ID, MAINNET_GENESIS_HASH),
            "/ckb/92b197aa"
        );
        assert_eq!(
            identify_name(TESTNET_SPEC_ID, TESTNET_GENESIS_HASH),
            "/ckb_testnet/10639e08"
        );
    }

    #[test]
    fn get_nodes_roundtrips_as_a_discovery_message() {
        // The request we send must decode as a valid discovery message (a `GetNodes`, so
        // `decode_nodes_addrs` — which only extracts `Nodes` — yields nothing).
        let bytes = encode_get_nodes();
        assert!(packed::DiscoveryMessageReader::from_compatible_slice(bytes.as_ref()).is_ok());
        assert!(decode_nodes_addrs(&bytes).is_empty());
    }

    #[test]
    fn decode_ignores_malformed_data() {
        assert!(decode_nodes_addrs(&Bytes::from_static(b"not molecule")).is_empty());
    }
}
