//! Real CKB L1 p2p prober, built **directly on tentacle** (not `ckb-network`'s
//! `NetworkService`).
//!
//! # Why tentacle-direct
//!
//! `ckb-network` only exposes custom protocols through [`ckb_network::CKBProtocol`], whose
//! `build()` unconditionally attaches `before_send(compress)` / `before_receive(decompress)`.
//! But CKB nodes register **Discovery / Identify as *built-in* protocols with NO compression**
//! (only Sync / Relay / Time are compressed). Routing our Discovery handler through
//! `CKBProtocol` therefore prefixed every `GetNodes` with a spurious compression-flag byte,
//! which shifts the molecule length header so real peers fail to decode it and never reply
//! with `Nodes` — the crawler then never learned any peer beyond its bootnodes.
//!
//! So we build the tentacle `Service` ourselves and register **Identify + Discovery
//! uncompressed** via [`SupportProtocols::build_meta_with_service_handle`] (the same path CKB's
//! built-in protocols use). This mirrors cryptape's own `ckb-node-probe`.
//!
//! # Probe shape
//!
//! Each probe is short-lived (dial → grab identify → brief discovery grace → disconnect), so we
//! stay under a full node's 10s "must open Sync" eviction floor and never need the Sync protocol.
//! We are *outbound-only*; inbound sessions are rejected. Identify is grab-and-go: we read the
//! peer's Identify (verifying the network id) but do not send our own — the peer emits its
//! Identify immediately on connect, well within the probe window.

use std::borrow::Cow;
use std::collections::{BTreeSet, HashMap};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;

use ckb_network::{extract_peer_id, SupportProtocols};
use ckb_types::{packed, prelude::*};
use ckbadger_common::network::{
    MAINNET_GENESIS_HASH, MAINNET_SPEC_ID, TESTNET_GENESIS_HASH, TESTNET_SPEC_ID,
};
use ckbadger_config::CrawlerConfig;
use p2p::{
    builder::ServiceBuilder,
    bytes::Bytes,
    context::{ProtocolContext, ProtocolContextMutRef, ServiceContext},
    multiaddr::{Multiaddr, Protocol as MultiProtocol},
    secio::{PeerId, PublicKey, SecioKeyPair},
    service::{
        ProtocolHandle, ProtocolMeta, ServiceAsyncControl, ServiceError, ServiceEvent,
        TargetProtocol,
    },
    traits::{ServiceHandle, ServiceProtocol},
    ProtocolId, SessionId,
};

use crate::prober::{ProbeCandidate, ProbeObservation, ProbeOutcome, ProbeResult, Prober};

// ---------------------------------------------------------------------------
// Built-in bootnodes (from the CKB v0.119.0 `resource/ckb.toml`). The per-network
// spec ids and genesis hashes used to build identify_name = `/{spec_id}/{genesis_hash[..8]}`
// are the single source of truth in `ckbadger_common::network` (imported above).
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Discovery message codec (raw molecule; tentacle applies NO compression to
// these protocols, exactly like a real CKB node's built-in Discovery).
// ---------------------------------------------------------------------------

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
// Identify parsing (grab-and-go: we only read the peer's Identify).
// ---------------------------------------------------------------------------

/// The bits we keep from a peer's Identify handshake.
struct ParsedIdentify {
    /// Network identifier (`/{spec_id}/{genesis_hash[..8]}`); used to reject foreign networks.
    net_name: String,
    client_version: String,
    flags: u64,
    listen_addrs: Vec<String>,
}

/// Parse a wire `IdentifyMessage` (the peer wraps a `packed::Identify` payload inside it).
/// Returns `None` for anything that is not a well-formed CKB Identify.
fn parse_identify(data: &[u8]) -> Option<ParsedIdentify> {
    let msg = packed::IdentifyMessageReader::from_compatible_slice(data).ok()?;
    let inner = msg.identify().raw_data();
    let id = packed::IdentifyReader::from_compatible_slice(inner).ok()?;
    let net_name = String::from_utf8(id.name().raw_data().to_vec()).ok()?;
    let client_version = String::from_utf8(id.client_version().raw_data().to_vec()).ok()?;
    let flags: u64 = id.flag().unpack();
    let listen_addrs = msg
        .listen_addrs()
        .iter()
        .filter_map(|a| Multiaddr::try_from(a.bytes().raw_data().to_vec()).ok())
        .map(|a| a.to_string())
        .collect();
    Some(ParsedIdentify {
        net_name,
        client_version,
        flags,
        listen_addrs,
    })
}

// ---------------------------------------------------------------------------
// Per-peer capture shared between the tentacle handler and the prober.
// ---------------------------------------------------------------------------

#[derive(Default)]
struct PeerCapture {
    session_id: Option<SessionId>,
    client_version: Option<String>,
    flags: u64,
    listen_addrs: Vec<String>,
    /// Names of protocols we successfully opened with this peer.
    opened_protocols: BTreeSet<String>,
    discovered_addrs: Vec<String>,
    /// Set once we have parsed a same-network Identify (⇒ reachable).
    identify_seen: bool,
    foreign_identify_seen: bool,
    rejected_identify_seen: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IdentifyState {
    SameNetwork,
    ForeignNetwork,
    Rejected,
}

type Captures = Arc<Mutex<HashMap<Vec<u8>, PeerCapture>>>;

/// The authenticated peer id of a session (from the secio-verified remote pubkey).
fn session_peer_id(pubkey: &Option<PublicKey>) -> Option<PeerId> {
    pubkey.as_ref().map(PeerId::from_public_key)
}

// ---------------------------------------------------------------------------
// Tentacle handler: one `Clone`-shared value used as every protocol's handler
// AND as the service handle. All state lives behind `Arc`s.
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct CrawlerHandler {
    net_id: Arc<String>,
    captures: Captures,
    identify_id: ProtocolId,
    discovery_id: ProtocolId,
}

impl CrawlerHandler {
    fn record<F: FnOnce(&mut PeerCapture)>(&self, key: Vec<u8>, f: F) {
        let mut guard = self.captures.lock().expect("captures poisoned");
        f(guard.entry(key).or_default());
    }

    /// Human-readable name for one of our two registered protocols.
    fn proto_name(&self, proto_id: ProtocolId) -> String {
        if proto_id == self.identify_id {
            SupportProtocols::Identify.name()
        } else if proto_id == self.discovery_id {
            SupportProtocols::Discovery.name()
        } else {
            String::new()
        }
    }
}

#[async_trait]
impl ServiceProtocol for CrawlerHandler {
    async fn init(&mut self, _context: &mut ProtocolContext) {}

    async fn connected(&mut self, context: ProtocolContextMutRef<'_>, _version: &str) {
        let Some(peer_id) = session_peer_id(&context.session.remote_pubkey) else {
            return;
        };
        let proto_id = context.proto_id();
        let proto_name = self.proto_name(proto_id);
        let session_id = context.session.id;
        self.record(peer_id.as_bytes().to_vec(), |c| {
            c.session_id = Some(session_id);
            if !proto_name.is_empty() {
                c.opened_protocols.insert(proto_name);
            }
        });
        // Feeler-style interrogation: ask the peer for its address book. Sent RAW (uncompressed),
        // matching the peer's built-in Discovery framing.
        if proto_id == self.discovery_id {
            let _ = context.send_message(encode_get_nodes()).await;
        }
    }

    async fn received(&mut self, context: ProtocolContextMutRef<'_>, data: Bytes) {
        let Some(peer_id) = session_peer_id(&context.session.remote_pubkey) else {
            return;
        };
        let key = peer_id.as_bytes().to_vec();
        let proto_id = context.proto_id();
        if proto_id == self.identify_id {
            match parse_identify(&data) {
                Some(parsed) if parsed.net_name == *self.net_id => {
                    self.record(key, |c| {
                        c.client_version = Some(parsed.client_version);
                        c.flags = parsed.flags;
                        c.listen_addrs = parsed.listen_addrs;
                        c.identify_seen = true;
                    });
                }
                Some(_) => self.record(key, |c| c.foreign_identify_seen = true),
                None => self.record(key, |c| c.rejected_identify_seen = true),
            }
        } else if proto_id == self.discovery_id {
            let addrs = decode_nodes_addrs(&data);
            if !addrs.is_empty() {
                self.record(key, |c| c.discovered_addrs.extend(addrs));
            }
        }
    }
}

#[async_trait]
impl ServiceHandle for CrawlerHandler {
    async fn handle_error(&mut self, _context: &mut ServiceContext, _error: ServiceError) {}

    async fn handle_event(&mut self, context: &mut ServiceContext, event: ServiceEvent) {
        if let ServiceEvent::SessionOpen { session_context } = event {
            // Outbound-only crawler: never serve inbound peers.
            if session_context.ty.is_inbound() {
                let _ = context.disconnect(session_context.id).await;
                return;
            }
            if let Some(peer_id) = session_peer_id(&session_context.remote_pubkey) {
                let session_id = session_context.id;
                self.record(peer_id.as_bytes().to_vec(), |c| {
                    c.session_id = Some(session_id);
                });
            }
        }
    }
}

// ---------------------------------------------------------------------------
// CkbProber
// ---------------------------------------------------------------------------

/// Real prober backed by a long-lived tentacle `Service` we build and run ourselves.
pub struct CkbProber {
    control: ServiceAsyncControl,
    captures: Captures,
    bootnodes: Vec<String>,
    dial_timeout: Duration,
}

impl CkbProber {
    fn normalize_candidate(addr: &str, peer_hint: Option<&[u8]>) -> Result<Option<ProbeCandidate>> {
        let mut multiaddr: Multiaddr = match addr.parse() {
            Ok(addr) => addr,
            Err(_) => return Ok(None),
        };
        let embedded_peer = extract_peer_id(&multiaddr);
        let peer_id = match (embedded_peer, peer_hint) {
            (Some(embedded), Some(hint)) => {
                let expected = PeerId::from_bytes(hint.to_vec()).with_context(|| {
                    format!("invalid persisted peer id for candidate address '{addr}'")
                })?;
                if embedded != expected {
                    return Ok(None);
                }
                embedded
            }
            (Some(embedded), None) => embedded,
            (None, Some(hint)) => {
                let expected = PeerId::from_bytes(hint.to_vec()).with_context(|| {
                    format!("invalid persisted peer id for own address '{addr}'")
                })?;
                multiaddr.push(MultiProtocol::P2P(Cow::Owned(hint.to_vec())));
                expected
            }
            (None, None) => return Ok(None),
        };
        Ok(Some(ProbeCandidate {
            peer_id: peer_id.as_bytes().to_vec(),
            addr: multiaddr.to_string(),
        }))
    }

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
        if cfg.dial_timeout_secs == 0 {
            return Err(anyhow!(
                "crawler dial_timeout_secs must be greater than zero"
            ));
        }
        for addr in &bootnodes {
            if Self::normalize_candidate(addr, None)?.is_none() {
                return Err(anyhow!(
                    "crawler bootnode must be a valid multiaddr with /p2p peer id: {addr}"
                ));
            }
        }

        let net_id = identify_name(spec_id, genesis_hash);
        Self::start(
            net_id,
            bootnodes,
            Duration::from_secs(cfg.dial_timeout_secs),
        )
    }

    /// Assemble + spawn the tentacle service; return a prober that dials over its control.
    fn start(net_id: String, bootnodes: Vec<String>, dial_timeout: Duration) -> Result<Self> {
        let captures: Captures = Arc::new(Mutex::new(HashMap::new()));
        let handler = CrawlerHandler {
            net_id: Arc::new(net_id),
            captures: Arc::clone(&captures),
            identify_id: SupportProtocols::Identify.protocol_id(),
            discovery_id: SupportProtocols::Discovery.protocol_id(),
        };

        // Identify + Discovery, both UNCOMPRESSED (built-in framing). No Sync: probes are short.
        let metas: Vec<ProtocolMeta> = [SupportProtocols::Identify, SupportProtocols::Discovery]
            .into_iter()
            .map(|proto| {
                let h = handler.clone();
                proto.build_meta_with_service_handle(move || ProtocolHandle::Callback(Box::new(h)))
            })
            .collect();

        let mut builder = ServiceBuilder::<SecioKeyPair>::new().forever(true);
        for meta in metas {
            builder = builder.insert_protocol(meta);
        }
        let key = SecioKeyPair::secp256k1_generated();
        let mut service = builder.handshake_type(key.into()).build(handler);
        let control = service.control().to_owned();
        tokio::spawn(async move { service.run().await });

        Ok(Self {
            control,
            captures,
            bootnodes,
            dial_timeout,
        })
    }

    /// Wait until the peer's same-network Identify has been captured (⇒ reachable).
    async fn await_identify(&self, key: &[u8]) -> IdentifyState {
        loop {
            if let Some(state) = self
                .captures
                .lock()
                .expect("captures poisoned")
                .get(key)
                .and_then(|capture| {
                    if capture.identify_seen {
                        Some(IdentifyState::SameNetwork)
                    } else if capture.foreign_identify_seen {
                        Some(IdentifyState::ForeignNetwork)
                    } else if capture.rejected_identify_seen {
                        Some(IdentifyState::Rejected)
                    } else {
                        None
                    }
                })
            {
                return state;
            }
            tokio::time::sleep(POLL_INTERVAL).await;
        }
    }

    /// Wait a bounded grace period for at least one discovery `Nodes` reply.
    async fn await_discovery(&self, key: &[u8]) {
        let deadline = Instant::now() + DISCOVERY_GRACE;
        loop {
            let has_addrs = self
                .captures
                .lock()
                .expect("captures poisoned")
                .get(key)
                .is_some_and(|c| !c.discovered_addrs.is_empty());
            if has_addrs || Instant::now() >= deadline {
                return;
            }
            tokio::time::sleep(POLL_INTERVAL).await;
        }
    }

    /// Snapshot + remove the capture for `key`, returning both the assembled
    /// outcome and the session that must be disconnected.
    fn take_outcome(
        &self,
        key: &[u8],
        peer_id: &PeerId,
        rtt: Duration,
    ) -> Result<(ProbeOutcome, Option<SessionId>)> {
        let cap = self
            .captures
            .lock()
            .expect("captures poisoned")
            .remove(key)
            .ok_or_else(|| anyhow!("missing peer capture after same-network Identify"))?;
        if !cap.identify_seen {
            return Err(anyhow!(
                "peer capture lost same-network Identify invariant: peer_id={peer_id}"
            ));
        }
        let session_id = cap.session_id;
        let mut discovered = cap.discovered_addrs;
        discovered.sort();
        discovered.dedup();
        Ok((
            ProbeOutcome {
                peer_id: peer_id.as_bytes().to_vec(),
                client_version: cap.client_version.with_context(|| {
                    format!("same-network Identify missing client version: peer_id={peer_id}")
                })?,
                flags: cap.flags,
                protocols: cap.opened_protocols.into_iter().collect(),
                own_addrs: cap.listen_addrs,
                rtt_ms: Some(u32::try_from(rtt.as_millis()).with_context(|| {
                    format!("probe RTT exceeds u32 milliseconds: peer_id={peer_id}, rtt={rtt:?}")
                })?),
                discovered_addrs: discovered,
            },
            session_id,
        ))
    }

    /// Disconnect the session recorded for `key` (feeler behaviour) if still open.
    async fn disconnect(&self, key: &[u8]) {
        let session_id = self
            .captures
            .lock()
            .expect("captures poisoned")
            .get(key)
            .and_then(|c| c.session_id);
        if let Some(id) = session_id {
            let _ = self.control.disconnect(id).await;
        }
    }
}

#[async_trait]
impl Prober for CkbProber {
    fn candidate_from_addr(
        &self,
        addr: &str,
        peer_hint: Option<&[u8]>,
    ) -> Result<Option<ProbeCandidate>> {
        Self::normalize_candidate(addr, peer_hint)
    }

    async fn probe(&self, expected_peer_id: &[u8], addr: &str) -> Result<ProbeResult> {
        let Some(candidate) = Self::normalize_candidate(addr, Some(expected_peer_id))? else {
            return Ok(ProbeResult::failed(ProbeObservation::MalformedAddress));
        };
        let multiaddr: Multiaddr = candidate
            .addr
            .parse()
            .context("normalized crawler address must parse")?;
        let peer_id = PeerId::from_bytes(candidate.peer_id)
            .context("normalized crawler peer id must be valid")?;
        let key = peer_id.as_bytes().to_vec();

        // Fresh capture for this probe (drop any stale entry from a prior round).
        self.captures
            .lock()
            .expect("captures poisoned")
            .remove(&key);

        let start = Instant::now();
        // Dial + open Identify & Discovery. A dial error is an unreachable observation.
        if self
            .control
            .dial(multiaddr.clone(), TargetProtocol::All)
            .await
            .is_err()
        {
            return Ok(ProbeResult::failed(ProbeObservation::DialFailed));
        }

        // Phase 1: bounded wait for a typed Identify result.
        let identify = tokio::time::timeout(self.dial_timeout, self.await_identify(&key)).await;
        let rtt = start.elapsed();
        let state = match identify {
            Ok(state) => state,
            Err(_) => {
                self.disconnect(&key).await;
                self.captures
                    .lock()
                    .expect("captures poisoned")
                    .remove(&key);
                return Ok(ProbeResult::failed(ProbeObservation::TimedOut));
            }
        };
        if state != IdentifyState::SameNetwork {
            self.disconnect(&key).await;
            self.captures
                .lock()
                .expect("captures poisoned")
                .remove(&key);
            return Ok(ProbeResult::failed(match state {
                IdentifyState::ForeignNetwork => ProbeObservation::ForeignNetwork,
                IdentifyState::Rejected => ProbeObservation::HandshakeRejected,
                IdentifyState::SameNetwork => unreachable!("handled above"),
            }));
        }

        // Phase 2: node is reachable; give the Discovery reply a brief, bounded grace.
        self.await_discovery(&key).await;
        let (outcome, session_id) = self.take_outcome(&key, &peer_id, rtt)?;
        if let Some(session_id) = session_id {
            let _ = self.control.disconnect(session_id).await;
        }
        Ok(ProbeResult::reachable(outcome))
    }

    fn bootnodes(&self) -> Vec<String> {
        self.bootnodes.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

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

    /// Build a wire `IdentifyMessage` exactly as a CKB node does (see `ckb-network`
    /// `protocols/identify`), so we can round-trip it through [`parse_identify`].
    fn encode_identify(
        net_name: &str,
        flags: u64,
        client_version: &str,
        listens: &[&str],
    ) -> Bytes {
        let inner = packed::Identify::new_builder()
            .name(net_name.as_bytes().pack())
            .flag(flags.pack())
            .client_version(client_version.as_bytes().pack())
            .build();
        let mut listen_addrs = Vec::new();
        for l in listens {
            let ma: Multiaddr = l.parse().unwrap();
            listen_addrs.push(
                packed::Address::new_builder()
                    .bytes(
                        packed::Bytes::new_builder()
                            .set(ma.to_vec().into_iter().map(Into::into).collect())
                            .build(),
                    )
                    .build(),
            );
        }
        let msg = packed::IdentifyMessage::new_builder()
            .identify(
                packed::Bytes::new_builder()
                    .set(inner.as_bytes().into_iter().map(Into::into).collect())
                    .build(),
            )
            .listen_addrs(packed::AddressVec::new_builder().set(listen_addrs).build())
            .observed_addr(packed::Address::default())
            .build();
        Bytes::from(msg.as_bytes().to_vec())
    }

    #[test]
    fn parse_identify_extracts_name_version_flags_listens() {
        let wire = encode_identify(
            "/ckb/92b197aa",
            0b1111,
            "ckb/0.119.0",
            &["/ip4/1.2.3.4/tcp/8114"],
        );
        let parsed = parse_identify(&wire).expect("valid identify parses");
        assert_eq!(parsed.net_name, "/ckb/92b197aa");
        assert_eq!(parsed.client_version, "ckb/0.119.0");
        assert_eq!(parsed.flags, 0b1111);
        assert_eq!(
            parsed.listen_addrs,
            vec!["/ip4/1.2.3.4/tcp/8114".to_string()]
        );
    }

    #[test]
    fn parse_identify_rejects_garbage() {
        assert!(parse_identify(b"not an identify message").is_none());
    }

    // ---- Loopback regression test: crawler ↔ a mock uncompressed-discovery peer -----------
    //
    // The mock decodes GetNodes RAW (no decompress), exactly like a real CKB node's built-in
    // Discovery. If the crawler ever re-compresses Discovery (the original bug), the mock's raw
    // decode fails, it never replies with `Nodes`, and `discovered_addrs` comes back empty —
    // failing this test.

    use p2p::{
        builder::ServiceBuilder as P2PServiceBuilder,
        secio::SecioKeyPair,
        service::{ProtocolHandle, ProtocolMeta},
    };

    /// Encode a discovery `Nodes` reply advertising `addrs` (mirrors `ckb-network`'s encoding).
    fn encode_nodes(addrs: &[&str]) -> Bytes {
        let announce = packed::Bool::new_builder().set([0u8.into()]).build();
        let mut items = Vec::new();
        for a in addrs {
            let ma: Multiaddr = a.parse().unwrap();
            let raw = packed::Bytes::new_builder()
                .set(ma.to_vec().into_iter().map(Into::into).collect())
                .build();
            let node = packed::Node2::new_builder()
                .addresses(packed::BytesVec::new_builder().set(vec![raw]).build())
                .flags(0u64.pack())
                .build();
            items.push(node);
        }
        let nodes2 = packed::Nodes2::new_builder()
            .announce(announce)
            .items(packed::Node2Vec::new_builder().set(items).build())
            .build();
        let nodes = packed::Nodes::new_unchecked(nodes2.as_bytes());
        let payload = packed::DiscoveryPayload::new_builder().set(nodes).build();
        let msg = packed::DiscoveryMessage::new_builder()
            .payload(payload)
            .build();
        Bytes::from(msg.as_bytes().to_vec())
    }

    /// A minimal CKB-node stand-in: sends a same-network Identify on connect and answers
    /// `GetNodes` with a fixed `Nodes` — all UNCOMPRESSED, like a real node's built-in protocols.
    #[derive(Clone)]
    struct MockPeer {
        net_id: Option<Arc<String>>,
        advertised: Arc<String>,
        identify_id: ProtocolId,
        discovery_id: ProtocolId,
    }

    #[async_trait]
    impl ServiceProtocol for MockPeer {
        async fn init(&mut self, _context: &mut ProtocolContext) {}

        async fn connected(&mut self, context: ProtocolContextMutRef<'_>, _version: &str) {
            if context.proto_id() == self.identify_id {
                if let Some(net_id) = &self.net_id {
                    let msg = encode_identify(net_id, 0b1111, "mock-client/1.0", &[]);
                    let _ = context.send_message(msg).await;
                }
            }
        }

        async fn received(&mut self, context: ProtocolContextMutRef<'_>, data: Bytes) {
            if context.proto_id() == self.discovery_id {
                // RAW decode — no decompress. A compressed GetNodes fails here (the bug).
                if let Ok(reader) =
                    packed::DiscoveryMessageReader::from_compatible_slice(data.as_ref())
                {
                    if matches!(
                        reader.payload().to_enum(),
                        packed::DiscoveryPayloadUnionReader::GetNodes(_)
                    ) {
                        let _ = context
                            .send_message(encode_nodes(&[&self.advertised]))
                            .await;
                    }
                }
            }
        }
    }

    #[derive(Clone)]
    struct MockPeerServiceHandle {
        closed_sessions: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl ServiceHandle for MockPeerServiceHandle {
        async fn handle_error(&mut self, _context: &mut ServiceContext, _error: ServiceError) {}

        async fn handle_event(&mut self, _context: &mut ServiceContext, event: ServiceEvent) {
            if matches!(event, ServiceEvent::SessionClose { .. }) {
                self.closed_sessions.fetch_add(1, Ordering::SeqCst);
            }
        }
    }

    /// Start the mock peer on an ephemeral loopback port; returns its dial
    /// multiaddr and a counter proving the crawler closed its feeler session.
    async fn start_mock_peer_with_identify(
        net_id: Option<&str>,
        advertised: &str,
    ) -> (String, Arc<AtomicUsize>) {
        let key = SecioKeyPair::secp256k1_generated();
        let peer_id = key.peer_id();
        let closed_sessions = Arc::new(AtomicUsize::new(0));
        let handler = MockPeer {
            net_id: net_id.map(|net_id| Arc::new(net_id.to_string())),
            advertised: Arc::new(advertised.to_string()),
            identify_id: SupportProtocols::Identify.protocol_id(),
            discovery_id: SupportProtocols::Discovery.protocol_id(),
        };
        let metas: Vec<ProtocolMeta> = [SupportProtocols::Identify, SupportProtocols::Discovery]
            .into_iter()
            .map(|proto| {
                let h = handler.clone();
                proto.build_meta_with_service_handle(move || ProtocolHandle::Callback(Box::new(h)))
            })
            .collect();
        let mut builder = P2PServiceBuilder::<SecioKeyPair>::new().forever(true);
        for meta in metas {
            builder = builder.insert_protocol(meta);
        }
        let mut service = builder
            .handshake_type(key.into())
            .build(MockPeerServiceHandle {
                closed_sessions: Arc::clone(&closed_sessions),
            });
        let listen = service
            .listen("/ip4/127.0.0.1/tcp/0".parse().unwrap())
            .await
            .expect("mock peer listens");
        tokio::spawn(async move { service.run().await });
        (
            format!("{}/p2p/{}", listen, peer_id.to_base58()),
            closed_sessions,
        )
    }

    async fn start_mock_peer(net_id: &str, advertised: &str) -> (String, Arc<AtomicUsize>) {
        start_mock_peer_with_identify(Some(net_id), advertised).await
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn probe_captures_discovery_from_uncompressed_peer() {
        let net_id = "/ckbtest/deadbeef".to_string();
        let advertised = "/ip4/9.9.9.9/tcp/8114/p2p/QmaZMemLXSsxKUrYNucjEbPxVX3rBKsGhWW2muWtWxUWyh";
        let expected = advertised.parse::<Multiaddr>().unwrap().to_string();

        let (dial_addr, closed_sessions) = start_mock_peer(&net_id, advertised).await;
        let prober =
            CkbProber::start(net_id, vec![], Duration::from_secs(8)).expect("prober starts");

        let candidate = prober
            .candidate_from_addr(&dial_addr, None)
            .expect("candidate normalization is infallible")
            .expect("loopback address contains a peer id");
        let result = prober
            .probe(&candidate.peer_id, &candidate.addr)
            .await
            .expect("probe is infallible");
        assert_eq!(result.observation, ProbeObservation::Reachable);
        let outcome = result.outcome.expect("reachable probe has an outcome");

        assert_eq!(outcome.client_version, "mock-client/1.0");
        assert!(
            outcome.discovered_addrs.contains(&expected),
            "expected discovered addr {expected:?}, got {:?} — an empty list means Discovery \
             was re-compressed and the peer could not decode our GetNodes",
            outcome.discovered_addrs
        );
        tokio::time::timeout(Duration::from_secs(2), async {
            while closed_sessions.load(Ordering::SeqCst) == 0 {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("successful feeler probe must disconnect its session");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn shared_service_probes_distinct_peers_concurrently() {
        let net_id = "/ckbtest/deadbeef".to_string();
        let advertised = "/ip4/9.9.9.9/tcp/8114/p2p/QmaZMemLXSsxKUrYNucjEbPxVX3rBKsGhWW2muWtWxUWyh";
        let mut dial_addrs = Vec::new();
        for _ in 0..4 {
            let (addr, _) = start_mock_peer(&net_id, advertised).await;
            dial_addrs.push(addr);
        }
        let prober = CkbProber::start(net_id, vec![], Duration::from_secs(8)).unwrap();
        let candidates: Vec<ProbeCandidate> = dial_addrs
            .iter()
            .map(|addr| prober.candidate_from_addr(addr, None).unwrap().unwrap())
            .collect();

        let results = futures::future::join_all(
            candidates
                .iter()
                .map(|candidate| prober.probe(&candidate.peer_id, &candidate.addr)),
        )
        .await;
        assert_eq!(results.len(), 4);
        for result in results {
            assert_eq!(result.unwrap().observation, ProbeObservation::Reachable);
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn foreign_identify_is_reported_separately_from_timeout() {
        let (dial_addr, _) = start_mock_peer("/foreign/deadbeef", "/ip4/9.9.9.9/tcp/8114").await;
        let prober = CkbProber::start(
            "/ckbtest/deadbeef".to_string(),
            vec![],
            Duration::from_secs(8),
        )
        .unwrap();
        let candidate = prober
            .candidate_from_addr(&dial_addr, None)
            .unwrap()
            .unwrap();
        let result = prober
            .probe(&candidate.peer_id, &candidate.addr)
            .await
            .unwrap();
        assert_eq!(result.observation, ProbeObservation::ForeignNetwork);
        assert!(result.outcome.is_none());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn missing_identify_is_reported_as_timeout_and_disconnected() {
        let (dial_addr, closed_sessions) =
            start_mock_peer_with_identify(None, "/ip4/9.9.9.9/tcp/8114").await;
        let prober = CkbProber::start(
            "/ckbtest/deadbeef".to_string(),
            vec![],
            Duration::from_millis(250),
        )
        .unwrap();
        let candidate = prober
            .candidate_from_addr(&dial_addr, None)
            .unwrap()
            .unwrap();

        let result = prober
            .probe(&candidate.peer_id, &candidate.addr)
            .await
            .unwrap();

        assert_eq!(result.observation, ProbeObservation::TimedOut);
        assert!(result.outcome.is_none());
        tokio::time::timeout(Duration::from_secs(2), async {
            while closed_sessions.load(Ordering::SeqCst) == 0 {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("timed-out feeler probe must disconnect its session");
    }

    #[tokio::test]
    async fn malformed_address_is_a_typed_observation() {
        let prober = CkbProber::start(
            "/ckbtest/deadbeef".to_string(),
            vec![],
            Duration::from_secs(1),
        )
        .unwrap();
        assert!(prober
            .candidate_from_addr("not-a-multiaddr", None)
            .unwrap()
            .is_none());
        let result = prober
            .probe(b"not-a-peer", "not-a-multiaddr")
            .await
            .unwrap();
        assert_eq!(result.observation, ProbeObservation::MalformedAddress);
    }
}
