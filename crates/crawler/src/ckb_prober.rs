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
use ckbadger_store::{AddressProbeResult, DiscoveryEvidence};
use p2p::{
    builder::ServiceBuilder,
    bytes::Bytes,
    context::{ProtocolContext, ProtocolContextMutRef, ServiceContext},
    error::{DialerErrorKind, HandshakeErrorKind, TransportErrorKind},
    multiaddr::{Multiaddr, Protocol as MultiProtocol},
    secio::{PeerId, PublicKey, SecioKeyPair},
    service::{
        ProtocolHandle, ProtocolMeta, ServiceAsyncControl, ServiceError, ServiceEvent,
        TargetProtocol,
    },
    traits::{ServiceHandle, ServiceProtocol},
    ProtocolId, SessionId,
};

use crate::prober::{ProbeCandidate, ProbeOutcome, ProbeResult, Prober};

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

/// Discovery v1 tells the remote to treat our ephemeral outbound source port as
/// a reusable listen address. This crawler has no listener, so it must use the
/// baseline v0 contract and must not cause peers to gossip an undialable alias.
const GET_NODES_VERSION: u32 = 0;
/// Maximum number of addresses to request in one `GetNodes` (matches `MAX_ADDR_TO_SEND`).
const GET_NODES_COUNT: u32 = 1000;
/// After the handshake completes (node is reachable), how long to wait for the `Nodes` reply
/// before returning. Bounded and independent of the reachability timeout.
const DISCOVERY_GRACE: Duration = Duration::from_secs(3);
/// Poll interval while waiting for handshake / discovery.
const POLL_INTERVAL: Duration = Duration::from_millis(150);
/// If tentacle fails to emit either an address-matched dial terminal or an authenticated session
/// after its configured connect/secio timeout, fail the service instead of starting another alias.
const DIAL_TERMINAL_WATCHDOG: Duration = Duration::from_secs(30);

fn duration_ms(duration: Duration, peer_id: &PeerId) -> Result<u64> {
    u64::try_from(duration.as_millis()).with_context(|| {
        format!(
            "probe elapsed time exceeds u64 milliseconds: peer_id={peer_id}, elapsed={duration:?}"
        )
    })
}

fn elapsed_between(start: Instant, terminal: Instant, peer_id: &PeerId) -> Result<Duration> {
    terminal.checked_duration_since(start).with_context(|| {
        format!(
            "crawler terminal event predates probe start: peer_id={peer_id}, start={start:?}, terminal={terminal:?}"
        )
    })
}

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

#[derive(Debug, PartialEq, Eq)]
enum DiscoveryMessageObservation {
    Nodes {
        announce: bool,
        addrs: Vec<String>,
        rejected_addrs: u64,
    },
    Malformed,
    Unexpected,
}

/// Decode one Discovery message without conflating a valid empty `Nodes`
/// payload, malformed bytes, and a valid non-`Nodes` payload.
fn decode_discovery_message(data: &Bytes) -> Result<DiscoveryMessageObservation> {
    let reader = match packed::DiscoveryMessageReader::from_compatible_slice(data.as_ref()) {
        Ok(r) => r,
        Err(_) => return Ok(DiscoveryMessageObservation::Malformed),
    };
    let mut out = Vec::new();
    let mut rejected_addrs = 0u64;
    match reader.payload().to_enum() {
        packed::DiscoveryPayloadUnionReader::Nodes(nodes) => {
            let announce = match nodes.announce().as_slice()[0] {
                0 => false,
                1 => true,
                _ => return Ok(DiscoveryMessageObservation::Malformed),
            };
            for node in nodes.items().iter() {
                for addr in node.addresses().iter() {
                    match Multiaddr::try_from(addr.raw_data().to_vec()) {
                        Ok(ma) => out.push(ma.to_string()),
                        Err(_) => {
                            rejected_addrs = rejected_addrs
                                .checked_add(1)
                                .context("Discovery rejected-address counter overflow")?
                        }
                    }
                }
            }
            Ok(DiscoveryMessageObservation::Nodes {
                announce,
                addrs: out,
                rejected_addrs,
            })
        }
        _ => Ok(DiscoveryMessageObservation::Unexpected),
    }
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

struct PeerCapture {
    dial_addr: Multiaddr,
    session_id: Option<SessionId>,
    client_version: Option<String>,
    flags: u64,
    listen_addrs: Vec<String>,
    /// Names of protocols we successfully opened with this peer.
    opened_protocols: BTreeSet<String>,
    discovered_addrs: Vec<String>,
    discovery: DiscoveryEvidence,
    received_discovery_response: bool,
    session_opened_at: Option<Instant>,
    /// First authenticated Identify message, timestamped at callback delivery so
    /// executor polling latency cannot move it across the absolute deadline.
    identify: Option<TimedIdentify>,
    dial_request_failed_at: Option<Instant>,
    dial_timeout_at: Option<Instant>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IdentifyKind {
    SameNetwork,
    ForeignNetwork,
    Rejected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TimedIdentify {
    kind: IdentifyKind,
    observed_at: Instant,
}

impl PeerCapture {
    fn new(dial_addr: Multiaddr) -> Self {
        Self {
            dial_addr,
            session_id: None,
            client_version: None,
            flags: 0,
            listen_addrs: Vec::new(),
            opened_protocols: BTreeSet::new(),
            discovered_addrs: Vec::new(),
            discovery: DiscoveryEvidence::default(),
            received_discovery_response: false,
            session_opened_at: None,
            identify: None,
            dial_request_failed_at: None,
            dial_timeout_at: None,
        }
    }

    fn record_identify(&mut self, kind: IdentifyKind) -> bool {
        if self.identify.is_some() {
            return false;
        }
        self.identify = Some(TimedIdentify {
            kind,
            observed_at: Instant::now(),
        });
        true
    }

    /// Return the first typed pre-deadline terminal that can classify this
    /// address without waiting for the deadline itself. Session/no-Identify and
    /// transport timeout are deliberately classified only at the deadline.
    fn typed_terminal_at_or_before(&self, deadline: Instant) -> Option<TimedIdentifyState> {
        let identify = self.identify.and_then(|identify| {
            (identify.observed_at <= deadline).then_some(TimedIdentifyState {
                state: match identify.kind {
                    IdentifyKind::SameNetwork => IdentifyState::SameNetwork,
                    IdentifyKind::ForeignNetwork => IdentifyState::ForeignNetwork,
                    IdentifyKind::Rejected => IdentifyState::Rejected,
                },
                observed_at: identify.observed_at,
            })
        });
        let dial_failure = self.dial_request_failed_at.and_then(|observed_at| {
            (observed_at <= deadline).then_some(TimedIdentifyState {
                state: IdentifyState::DialRequestFailed,
                observed_at,
            })
        });
        match (identify, dial_failure) {
            (Some(identify), Some(dial_failure)) => {
                Some(if identify.observed_at <= dial_failure.observed_at {
                    identify
                } else {
                    dial_failure
                })
            }
            (Some(identify), None) => Some(identify),
            (None, Some(dial_failure)) => Some(dial_failure),
            (None, None) => None,
        }
    }

    fn record_discovery_nodes(
        &mut self,
        announce: bool,
        addrs: Vec<String>,
        rejected_addrs: u64,
    ) -> Result<()> {
        let valid_nodes_messages = self
            .discovery
            .valid_nodes_messages
            .checked_add(1)
            .context("Discovery valid-Nodes message counter overflow")?;
        let classified_messages = if announce {
            self.discovery
                .valid_announce_messages
                .checked_add(1)
                .context("Discovery valid-announce message counter overflow")?
        } else {
            self.discovery
                .valid_response_messages
                .checked_add(1)
                .context("Discovery valid-response message counter overflow")?
        };
        let rejected_advertised_addresses = self
            .discovery
            .rejected_advertised_addresses
            .checked_add(rejected_addrs)
            .context("Discovery rejected-address counter overflow")?;
        self.discovery.valid_nodes_messages = valid_nodes_messages;
        if announce {
            self.discovery.valid_announce_messages = classified_messages;
        } else {
            self.discovery.valid_response_messages = classified_messages;
        }
        self.discovery.rejected_advertised_addresses = rejected_advertised_addresses;
        if !announce {
            self.received_discovery_response = true;
        }
        // A regular GetNodes response and later announce messages are both
        // positive address observations. Union every valid payload received in
        // the grace window; omission from a later random payload is not negative
        // evidence and must not erase an earlier address.
        self.discovered_addrs.extend(addrs);
        self.discovered_addrs.sort();
        self.discovered_addrs.dedup();
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IdentifyState {
    SameNetwork,
    ForeignNetwork,
    Rejected,
    DialRequestFailed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TimedIdentifyState {
    state: IdentifyState,
    observed_at: Instant,
}

type Captures = Arc<Mutex<HashMap<Vec<u8>, PeerCapture>>>;
type ServiceHealth = Arc<Mutex<Option<String>>>;

/// The authenticated peer id of a session (from the secio-verified remote pubkey).
fn session_peer_id(pubkey: &Option<PublicKey>) -> Option<PeerId> {
    pubkey.as_ref().map(PeerId::from_public_key)
}

fn dialer_error_is_timeout(error: &DialerErrorKind) -> bool {
    match error {
        DialerErrorKind::IoError(error) => error.kind() == std::io::ErrorKind::TimedOut,
        DialerErrorKind::HandshakeError(HandshakeErrorKind::Timeout(_)) => true,
        DialerErrorKind::TransportError(TransportErrorKind::Io(error)) => {
            error.kind() == std::io::ErrorKind::TimedOut
        }
        DialerErrorKind::TransportError(TransportErrorKind::DnsResolverError(_, error)) => {
            error.kind() == std::io::ErrorKind::TimedOut
        }
        _ => false,
    }
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
    health: ServiceHealth,
}

impl CrawlerHandler {
    fn record_dial_error(&self, key: &[u8], address: &Multiaddr, timed_out: bool) {
        match self.captures.lock() {
            Ok(mut guard) => {
                if let Some(capture) = guard
                    .get_mut(key)
                    .filter(|capture| capture.dial_addr == *address)
                {
                    let observed_at = Instant::now();
                    if timed_out {
                        capture.dial_timeout_at.get_or_insert(observed_at);
                    } else {
                        capture.dial_request_failed_at.get_or_insert(observed_at);
                    }
                }
            }
            Err(_) => self.mark_fatal("crawler peer-capture state is poisoned"),
        }
    }

    fn record_session_open(&self, key: &[u8], address: &Multiaddr, session_id: SessionId) -> bool {
        match self.captures.lock() {
            Ok(mut guard) => {
                let Some(capture) = guard
                    .get_mut(key)
                    .filter(|capture| capture.dial_addr == *address)
                else {
                    return false;
                };
                if capture
                    .session_id
                    .is_some_and(|current| current != session_id)
                {
                    return false;
                }
                capture.session_id = Some(session_id);
                capture.session_opened_at.get_or_insert_with(Instant::now);
                true
            }
            Err(_) => {
                self.mark_fatal("crawler peer-capture state is poisoned");
                false
            }
        }
    }

    fn record_session_event<F: FnOnce(&mut PeerCapture)>(
        &self,
        key: &[u8],
        address: &Multiaddr,
        session_id: SessionId,
        update: F,
    ) -> bool {
        match self.captures.lock() {
            Ok(mut guard) => {
                let Some(capture) = guard.get_mut(key).filter(|capture| {
                    capture.dial_addr == *address && capture.session_id == Some(session_id)
                }) else {
                    return false;
                };
                update(capture);
                true
            }
            Err(_) => {
                self.mark_fatal("crawler peer-capture state is poisoned");
                false
            }
        }
    }

    fn mark_fatal(&self, reason: impl Into<String>) {
        if let Ok(mut health) = self.health.lock() {
            if health.is_none() {
                *health = Some(reason.into());
            }
        }
    }

    fn checked_discovery_inc(
        &self,
        key: &[u8],
        address: &Multiaddr,
        session_id: SessionId,
        field: &'static str,
        update: impl FnOnce(&mut DiscoveryEvidence) -> Option<()>,
    ) -> bool {
        let mut overflowed = false;
        let matched = self.record_session_event(key, address, session_id, |capture| {
            if update(&mut capture.discovery).is_none() {
                overflowed = true;
            }
        });
        if overflowed {
            self.mark_fatal(format!("crawler Discovery counter overflow: field={field}"));
        }
        matched
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
        let key = peer_id.as_bytes().to_vec();
        let matched =
            self.record_session_event(&key, &context.session.address, session_id, |capture| {
                if !proto_name.is_empty() {
                    capture.opened_protocols.insert(proto_name);
                }
            });
        if !matched {
            let _ = context.disconnect(session_id).await;
            return;
        }
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
        let session_id = context.session.id;
        let address = &context.session.address;
        let matched = if proto_id == self.identify_id {
            match parse_identify(&data) {
                Some(parsed) if parsed.net_name == *self.net_id => {
                    self.record_session_event(&key, address, session_id, |capture| {
                        if capture.record_identify(IdentifyKind::SameNetwork) {
                            capture.client_version = Some(parsed.client_version);
                            capture.flags = parsed.flags;
                            capture.listen_addrs = parsed.listen_addrs;
                        }
                    })
                }
                Some(_) => self.record_session_event(&key, address, session_id, |capture| {
                    capture.record_identify(IdentifyKind::ForeignNetwork);
                }),
                None => self.record_session_event(&key, address, session_id, |capture| {
                    capture.record_identify(IdentifyKind::Rejected);
                }),
            }
        } else if proto_id == self.discovery_id {
            match decode_discovery_message(&data) {
                Ok(DiscoveryMessageObservation::Nodes {
                    announce,
                    addrs,
                    rejected_addrs,
                }) => {
                    let mut record_error = None;
                    let matched = self.record_session_event(&key, address, session_id, |capture| {
                        if let Err(error) =
                            capture.record_discovery_nodes(announce, addrs, rejected_addrs)
                        {
                            record_error = Some(error);
                        }
                    });
                    if let Some(error) = record_error {
                        self.mark_fatal(format!(
                            "crawler failed to record Discovery Nodes evidence: {error:#}"
                        ));
                    }
                    matched
                }
                Ok(DiscoveryMessageObservation::Malformed) => self.checked_discovery_inc(
                    &key,
                    address,
                    session_id,
                    "malformed_messages",
                    |discovery| {
                        discovery.malformed_messages =
                            discovery.malformed_messages.checked_add(1)?;
                        Some(())
                    },
                ),
                Ok(DiscoveryMessageObservation::Unexpected) => self.checked_discovery_inc(
                    &key,
                    address,
                    session_id,
                    "unexpected_messages",
                    |discovery| {
                        discovery.unexpected_messages =
                            discovery.unexpected_messages.checked_add(1)?;
                        Some(())
                    },
                ),
                Err(error) => {
                    self.mark_fatal(format!(
                        "crawler failed to classify Discovery message: {error:#}"
                    ));
                    true
                }
            }
        } else {
            true
        };
        if !matched {
            let _ = context.disconnect(session_id).await;
        }
    }
}

#[async_trait]
impl ServiceHandle for CrawlerHandler {
    async fn handle_error(&mut self, _context: &mut ServiceContext, error: ServiceError) {
        let detail = format!("{error:?}");
        match error {
            ServiceError::DialerError { address, error } => {
                let Some(peer_id) = extract_peer_id(&address) else {
                    self.mark_fatal(format!(
                        "crawler received unkeyed dial error: address={address}, error={detail}"
                    ));
                    return;
                };
                self.record_dial_error(
                    peer_id.as_bytes(),
                    &address,
                    dialer_error_is_timeout(&error),
                );
            }
            ServiceError::ProtocolHandleError { .. } | ServiceError::ListenError { .. } => {
                self.mark_fatal(format!("crawler tentacle service failed: {detail}"));
            }
            ServiceError::ProtocolSelectError { .. }
            | ServiceError::ProtocolError { .. }
            | ServiceError::SessionTimeout { .. }
            | ServiceError::MuxerError { .. }
            | ServiceError::SessionBlocked { .. } => {
                // These are session-scoped remote observations. The authenticated
                // session marker and Identify deadline classify the exact milestone.
            }
        }
    }

    async fn handle_event(&mut self, context: &mut ServiceContext, event: ServiceEvent) {
        if let ServiceEvent::SessionOpen { session_context } = event {
            // Outbound-only crawler: never serve inbound peers.
            if session_context.ty.is_inbound() {
                let _ = context.disconnect(session_context.id).await;
                return;
            }
            if let Some(peer_id) = session_peer_id(&session_context.remote_pubkey) {
                let session_id = session_context.id;
                if !self.record_session_open(
                    peer_id.as_bytes(),
                    &session_context.address,
                    session_id,
                ) {
                    let _ = context.disconnect(session_id).await;
                }
            } else {
                let _ = context.disconnect(session_context.id).await;
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
    health: ServiceHealth,
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
        let health: ServiceHealth = Arc::new(Mutex::new(None));
        let handler = CrawlerHandler {
            net_id: Arc::new(net_id),
            captures: Arc::clone(&captures),
            identify_id: SupportProtocols::Identify.protocol_id(),
            discovery_id: SupportProtocols::Discovery.protocol_id(),
            health: Arc::clone(&health),
        };

        // Identify + Discovery, both UNCOMPRESSED (built-in framing). No Sync: probes are short.
        let metas: Vec<ProtocolMeta> = [SupportProtocols::Identify, SupportProtocols::Discovery]
            .into_iter()
            .map(|proto| {
                let h = handler.clone();
                proto.build_meta_with_service_handle(move || ProtocolHandle::Callback(Box::new(h)))
            })
            .collect();

        let mut builder = ServiceBuilder::<SecioKeyPair>::new()
            .forever(true)
            .timeout(dial_timeout);
        for meta in metas {
            builder = builder.insert_protocol(meta);
        }
        let key = SecioKeyPair::secp256k1_generated();
        let mut service = builder.handshake_type(key.into()).build(handler);
        let control = service.control().to_owned();
        let service_health = Arc::clone(&health);
        tokio::spawn(async move {
            service.run().await;
            if let Ok(mut health) = service_health.lock() {
                if health.is_none() {
                    *health = Some("crawler tentacle service terminated unexpectedly".into());
                }
            }
        });

        Ok(Self {
            control,
            captures,
            bootnodes,
            dial_timeout,
            health,
        })
    }

    fn ensure_healthy(&self) -> Result<()> {
        let health = self
            .health
            .lock()
            .map_err(|_| anyhow!("crawler service-health state is poisoned"))?;
        if let Some(reason) = health.as_ref() {
            return Err(anyhow!(reason.clone()));
        }
        Ok(())
    }

    fn mark_unhealthy(&self, reason: String) {
        if let Ok(mut health) = self.health.lock() {
            if health.is_none() {
                *health = Some(reason);
            }
        }
    }

    async fn await_dial_terminal(&self, key: &[u8]) -> Result<()> {
        loop {
            self.ensure_healthy()?;
            let terminal = {
                let captures = self
                    .captures
                    .lock()
                    .map_err(|_| anyhow!("crawler peer-capture state is poisoned"))?;
                let capture = captures
                    .get(key)
                    .context("crawler peer capture disappeared before dial terminal")?;
                capture.session_id.is_some()
                    || capture.dial_request_failed_at.is_some()
                    || capture.dial_timeout_at.is_some()
            };
            if terminal {
                return Ok(());
            }
            tokio::time::sleep(POLL_INTERVAL).await;
        }
    }

    /// Wait for an Identify classification or an address-attributed dial-request
    /// failure. Deadline-only results are intentionally left to the caller's
    /// absolute timer.
    async fn await_identify(&self, key: &[u8]) -> Result<()> {
        loop {
            self.ensure_healthy()?;
            let ready = self
                .captures
                .lock()
                .map_err(|_| anyhow!("crawler peer-capture state is poisoned"))?
                .get(key)
                .is_some_and(|capture| {
                    capture.identify.is_some() || capture.dial_request_failed_at.is_some()
                });
            if ready {
                return Ok(());
            }
            tokio::time::sleep(POLL_INTERVAL).await;
        }
    }

    /// Wait a bounded grace period for the non-announce response to our
    /// GetNodes request. Announcements received first are retained but cannot
    /// close the probe before the requested response arrives.
    async fn await_discovery(&self, key: &[u8]) -> Result<()> {
        let deadline = Instant::now() + DISCOVERY_GRACE;
        loop {
            self.ensure_healthy()?;
            let has_valid_response = self
                .captures
                .lock()
                .map_err(|_| anyhow!("crawler peer-capture state is poisoned"))?
                .get(key)
                .is_some_and(|c| c.received_discovery_response);
            if has_valid_response || Instant::now() >= deadline {
                return Ok(());
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
            .map_err(|_| anyhow!("crawler peer-capture state is poisoned"))?
            .remove(key)
            .ok_or_else(|| anyhow!("missing peer capture after same-network Identify"))?;
        if !cap
            .identify
            .is_some_and(|identify| identify.kind == IdentifyKind::SameNetwork)
        {
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
                discovery: cap.discovery,
            },
            session_id,
        ))
    }

    /// Disconnect the session recorded for `key` (feeler behaviour) if still open.
    async fn disconnect(&self, key: &[u8]) -> Result<()> {
        let session_id = self
            .captures
            .lock()
            .map_err(|_| anyhow!("crawler peer-capture state is poisoned"))?
            .get(key)
            .and_then(|c| c.session_id);
        if let Some(id) = session_id {
            self.control
                .disconnect(id)
                .await
                .context("crawler control closed while disconnecting probe session")?;
        }
        Ok(())
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
        self.ensure_healthy()?;
        let Some(candidate) = Self::normalize_candidate(addr, Some(expected_peer_id))? else {
            return Err(anyhow!(
                "scheduled crawler address is malformed or mismatches its peer id: peer_id=0x{}, addr={}",
                hex::encode(expected_peer_id),
                addr
            ));
        };
        let multiaddr: Multiaddr = candidate
            .addr
            .parse()
            .context("normalized crawler address must parse")?;
        let peer_id = PeerId::from_bytes(candidate.peer_id)
            .context("normalized crawler peer id must be valid")?;
        let key = peer_id.as_bytes().to_vec();

        {
            let mut captures = self
                .captures
                .lock()
                .map_err(|_| anyhow!("crawler peer-capture state is poisoned"))?;
            if captures.contains_key(&key) {
                return Err(anyhow!(
                    "crawler peer already has an active capture: peer_id={peer_id}, addr={multiaddr}"
                ));
            }
            captures.insert(key.clone(), PeerCapture::new(multiaddr.clone()));
        }

        let start = Instant::now();
        let identify_deadline = start
            .checked_add(self.dial_timeout)
            .context("crawler Identify deadline overflow")?;
        // Dial + open Identify & Discovery. Tentacle reports an address-attributed
        // dial result asynchronously through the service handler.
        if let Err(error) = self
            .control
            .dial(multiaddr.clone(), TargetProtocol::All)
            .await
        {
            self.captures
                .lock()
                .map_err(|_| anyhow!("crawler peer-capture state is poisoned"))?
                .remove(&key);
            return Err(error).context("crawler control closed while submitting dial request");
        }

        // Phase 1: one absolute deadline measured from probe start. Callback timestamps,
        // not this polling future's wake-up time, decide on which side of the boundary an
        // event occurred.
        let identify_wait = tokio::time::timeout_at(
            tokio::time::Instant::from_std(identify_deadline),
            self.await_identify(&key),
        )
        .await;
        if let Ok(result) = identify_wait {
            result?;
        } else {
            self.ensure_healthy()?;
        }
        let typed_terminal = {
            let captures = self
                .captures
                .lock()
                .map_err(|_| anyhow!("crawler peer-capture state is poisoned"))?;
            captures
                .get(&key)
                .with_context(|| {
                    format!(
                        "crawler peer capture disappeared at Identify deadline: peer_id={peer_id}, addr={multiaddr}"
                    )
                })?
                .typed_terminal_at_or_before(identify_deadline)
        };

        let Some(terminal) = typed_terminal else {
            // Do not release this peer-id slot until the address-matched attempt has a
            // terminal. Otherwise a delayed callback could contaminate the next alias.
            match tokio::time::timeout(DIAL_TERMINAL_WATCHDOG, self.await_dial_terminal(&key)).await
            {
                Ok(result) => result?,
                Err(_) => {
                    let reason = format!(
                        "crawler dial emitted no terminal after Identify deadline: peer_id={peer_id}, addr={multiaddr}, watchdog={DIAL_TERMINAL_WATCHDOG:?}"
                    );
                    self.mark_unhealthy(reason.clone());
                    return Err(anyhow!(reason));
                }
            }
            let (session_id, session_opened_at) = {
                let captures = self
                    .captures
                    .lock()
                    .map_err(|_| anyhow!("crawler peer-capture state is poisoned"))?;
                let capture = captures.get(&key).with_context(|| {
                    format!(
                        "crawler peer capture disappeared after dial terminal: peer_id={peer_id}, addr={multiaddr}"
                    )
                })?;
                (capture.session_id, capture.session_opened_at)
            };
            if let Some(session_id) = session_id {
                self.control
                    .disconnect(session_id)
                    .await
                    .context("crawler control closed while disconnecting timed-out session")?;
            }
            self.captures
                .lock()
                .map_err(|_| anyhow!("crawler peer-capture state is poisoned"))?
                .remove(&key);
            let deadline_elapsed = elapsed_between(start, identify_deadline, &peer_id)?;
            return ProbeResult::failed(
                if session_opened_at.is_some_and(|observed_at| observed_at <= identify_deadline) {
                    AddressProbeResult::AuthenticatedSessionWithoutIdentifyBeforeDeadline
                } else {
                    AddressProbeResult::NoAuthenticatedSessionBeforeDeadline
                },
                duration_ms(deadline_elapsed, &peer_id)?,
            );
        };

        let terminal_elapsed = elapsed_between(start, terminal.observed_at, &peer_id)?;
        let elapsed_ms = duration_ms(terminal_elapsed, &peer_id)?;
        if terminal.state != IdentifyState::SameNetwork {
            self.disconnect(&key).await?;
            self.captures
                .lock()
                .map_err(|_| anyhow!("crawler peer-capture state is poisoned"))?
                .remove(&key);
            let observation = match terminal.state {
                IdentifyState::ForeignNetwork => AddressProbeResult::ForeignNetwork,
                IdentifyState::Rejected => AddressProbeResult::MalformedIdentify,
                IdentifyState::DialRequestFailed => AddressProbeResult::DialRequestFailed,
                IdentifyState::SameNetwork => {
                    return Err(anyhow!(
                        "same-network Identify entered failure classification: peer_id={peer_id}"
                    ))
                }
            };
            return ProbeResult::failed(observation, elapsed_ms);
        }

        // Phase 2: node is reachable; give the Discovery reply a brief, bounded grace.
        self.await_discovery(&key).await?;
        let (outcome, session_id) = self.take_outcome(&key, &peer_id, terminal_elapsed)?;
        if let Some(session_id) = session_id {
            self.control
                .disconnect(session_id)
                .await
                .context("crawler control closed while disconnecting successful probe")?;
        }
        Ok(ProbeResult::reachable(outcome, elapsed_ms))
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
        // the decoder classifies it as valid but unexpected for a response).
        let bytes = encode_get_nodes();
        assert!(packed::DiscoveryMessageReader::from_compatible_slice(bytes.as_ref()).is_ok());
        assert_eq!(
            decode_discovery_message(&bytes).unwrap(),
            DiscoveryMessageObservation::Unexpected
        );
        let reader = packed::DiscoveryMessageReader::from_compatible_slice(bytes.as_ref()).unwrap();
        let packed::DiscoveryPayloadUnionReader::GetNodes(get_nodes) = reader.payload().to_enum()
        else {
            panic!("crawler GetNodes request encoded another Discovery payload");
        };
        assert_eq!(
            u32::from_le_bytes(get_nodes.version().raw_data().try_into().unwrap()),
            0
        );
        assert!(get_nodes.listen_port().to_opt().is_none());
    }

    #[test]
    fn decode_ignores_malformed_data() {
        assert_eq!(
            decode_discovery_message(&Bytes::from_static(b"not molecule")).unwrap(),
            DiscoveryMessageObservation::Malformed
        );
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
    fn encode_nodes_with_announce(addrs: &[&str], is_announce: bool) -> Bytes {
        let announce = packed::Bool::new_builder()
            .set([u8::from(is_announce).into()])
            .build();
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

    fn encode_nodes(addrs: &[&str]) -> Bytes {
        encode_nodes_with_announce(addrs, false)
    }

    #[test]
    fn discovery_decoder_distinguishes_empty_and_non_empty_nodes() {
        assert_eq!(
            decode_discovery_message(&encode_nodes(&[])).unwrap(),
            DiscoveryMessageObservation::Nodes {
                announce: false,
                addrs: vec![],
                rejected_addrs: 0,
            }
        );
        let advertised = "/ip4/9.9.9.9/tcp/8114";
        assert_eq!(
            decode_discovery_message(&encode_nodes(&[advertised])).unwrap(),
            DiscoveryMessageObservation::Nodes {
                announce: false,
                addrs: vec![advertised.to_string()],
                rejected_addrs: 0,
            }
        );
        assert_eq!(
            decode_discovery_message(&encode_nodes_with_announce(&[advertised], true)).unwrap(),
            DiscoveryMessageObservation::Nodes {
                announce: true,
                addrs: vec![advertised.to_string()],
                rejected_addrs: 0,
            }
        );
    }

    #[test]
    fn peer_capture_unions_response_and_announce_payloads_with_exact_message_counts() {
        let mut capture = PeerCapture::new("/ip4/127.0.0.1/tcp/8114".parse().unwrap());
        capture
            .record_discovery_nodes(true, vec!["addrA".into()], 1)
            .unwrap();
        assert!(!capture.received_discovery_response);
        capture
            .record_discovery_nodes(false, vec!["addrB".into(), "addrA".into()], 2)
            .unwrap();

        assert_eq!(capture.discovery.valid_nodes_messages, 2);
        assert_eq!(capture.discovery.valid_response_messages, 1);
        assert_eq!(capture.discovery.valid_announce_messages, 1);
        assert_eq!(capture.discovery.rejected_advertised_addresses, 3);
        assert_eq!(capture.discovered_addrs, vec!["addrA", "addrB"]);
        assert!(capture.received_discovery_response);
    }

    fn capture_test_handler(captures: Captures) -> CrawlerHandler {
        CrawlerHandler {
            net_id: Arc::new("/ckbtest/deadbeef".into()),
            captures,
            identify_id: SupportProtocols::Identify.protocol_id(),
            discovery_id: SupportProtocols::Discovery.protocol_id(),
            health: Arc::new(Mutex::new(None)),
        }
    }

    #[test]
    fn stale_address_attributed_dial_error_does_not_mutate_current_attempt() {
        let key = b"peer".to_vec();
        let current: Multiaddr = "/ip4/127.0.0.1/tcp/8114".parse().unwrap();
        let stale: Multiaddr = "/ip4/127.0.0.1/tcp/8115".parse().unwrap();
        let captures = Arc::new(Mutex::new(HashMap::from([(
            key.clone(),
            PeerCapture::new(current.clone()),
        )])));
        let handler = capture_test_handler(Arc::clone(&captures));

        handler.record_dial_error(&key, &stale, false);

        assert!(captures.lock().unwrap()[&key]
            .dial_request_failed_at
            .is_none());
        handler.record_dial_error(&key, &current, false);
        assert!(captures.lock().unwrap()[&key]
            .dial_request_failed_at
            .is_some());
    }

    #[test]
    fn stale_session_and_identify_do_not_mutate_current_attempt() {
        let key = b"peer".to_vec();
        let current: Multiaddr = "/ip4/127.0.0.1/tcp/8114".parse().unwrap();
        let stale: Multiaddr = "/ip4/127.0.0.1/tcp/8115".parse().unwrap();
        let current_session = SessionId::new(2);
        let stale_session = SessionId::new(1);
        let captures = Arc::new(Mutex::new(HashMap::from([(
            key.clone(),
            PeerCapture::new(current.clone()),
        )])));
        let handler = capture_test_handler(Arc::clone(&captures));

        assert!(!handler.record_session_open(&key, &stale, stale_session));
        assert!(handler.record_session_open(&key, &current, current_session));
        assert!(
            !handler.record_session_event(&key, &stale, stale_session, |capture| {
                capture.record_identify(IdentifyKind::SameNetwork);
            })
        );
        assert!(
            !handler.record_session_event(&key, &current, stale_session, |capture| {
                capture.record_identify(IdentifyKind::SameNetwork);
            })
        );
        assert!(captures.lock().unwrap()[&key].identify.is_none());
        assert!(
            handler.record_session_event(&key, &current, current_session, |capture| {
                capture.record_identify(IdentifyKind::SameNetwork);
            })
        );
        assert_eq!(
            captures.lock().unwrap()[&key]
                .identify
                .expect("matched Identify is retained")
                .kind,
            IdentifyKind::SameNetwork
        );
    }

    #[test]
    fn deadline_snapshot_classifies_pre_deadline_identify_without_waiter_poll() {
        let deadline = Instant::now();
        let before_deadline = deadline.checked_sub(Duration::from_millis(1)).unwrap();
        let mut capture = PeerCapture::new("/ip4/127.0.0.1/tcp/8114".parse().unwrap());
        capture.identify = Some(TimedIdentify {
            kind: IdentifyKind::SameNetwork,
            observed_at: before_deadline,
        });

        // The await future has not been polled; classification is derived only
        // from the callback's durable-in-memory event timestamp.
        assert_eq!(
            capture.typed_terminal_at_or_before(deadline),
            Some(TimedIdentifyState {
                state: IdentifyState::SameNetwork,
                observed_at: before_deadline,
            })
        );
    }

    #[test]
    fn deadline_snapshot_excludes_identify_recorded_after_deadline() {
        let deadline = Instant::now();
        let after_deadline = deadline.checked_add(Duration::from_millis(1)).unwrap();
        let mut capture = PeerCapture::new("/ip4/127.0.0.1/tcp/8114".parse().unwrap());
        capture.identify = Some(TimedIdentify {
            kind: IdentifyKind::SameNetwork,
            observed_at: after_deadline,
        });

        assert_eq!(capture.typed_terminal_at_or_before(deadline), None);
    }

    /// A minimal CKB-node stand-in: sends a same-network Identify on connect and answers
    /// `GetNodes` with a fixed `Nodes` — all UNCOMPRESSED, like a real node's built-in protocols.
    #[derive(Clone)]
    enum MockIdentify {
        Valid(Arc<String>),
        Malformed,
        Silent,
    }

    #[derive(Clone)]
    struct MockPeer {
        identify: MockIdentify,
        advertised: Option<Arc<String>>,
        identify_id: ProtocolId,
        discovery_id: ProtocolId,
    }

    #[async_trait]
    impl ServiceProtocol for MockPeer {
        async fn init(&mut self, _context: &mut ProtocolContext) {}

        async fn connected(&mut self, context: ProtocolContextMutRef<'_>, _version: &str) {
            if context.proto_id() == self.identify_id {
                match &self.identify {
                    MockIdentify::Valid(net_id) => {
                        let msg = encode_identify(net_id, 0b1111, "mock-client/1.0", &[]);
                        let _ = context.send_message(msg).await;
                    }
                    MockIdentify::Malformed => {
                        let _ = context
                            .send_message(Bytes::from_static(b"not an identify message"))
                            .await;
                    }
                    MockIdentify::Silent => {}
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
                            .send_message(match &self.advertised {
                                Some(advertised) if advertised.is_empty() => encode_nodes(&[]),
                                Some(advertised) => encode_nodes(&[advertised]),
                                None => return,
                            })
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
    async fn start_mock_peer_with_reply(
        identify: MockIdentify,
        advertised: Option<&str>,
    ) -> (String, Arc<AtomicUsize>) {
        let key = SecioKeyPair::secp256k1_generated();
        let peer_id = key.peer_id();
        let closed_sessions = Arc::new(AtomicUsize::new(0));
        let handler = MockPeer {
            identify,
            advertised: advertised.map(|value| Arc::new(value.to_string())),
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

    async fn start_mock_peer_with_options(
        net_id: Option<&str>,
        advertised: Option<&str>,
    ) -> (String, Arc<AtomicUsize>) {
        start_mock_peer_with_reply(
            net_id
                .map(|net_id| MockIdentify::Valid(Arc::new(net_id.to_string())))
                .unwrap_or(MockIdentify::Silent),
            advertised,
        )
        .await
    }

    async fn start_mock_peer_with_identify(
        net_id: Option<&str>,
        advertised: &str,
    ) -> (String, Arc<AtomicUsize>) {
        start_mock_peer_with_options(net_id, Some(advertised)).await
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
        assert_eq!(
            result.observation,
            AddressProbeResult::SameNetworkIdentified
        );
        let outcome = result.outcome.expect("reachable probe has an outcome");
        assert_eq!(outcome.discovery.valid_nodes_messages, 1);

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

    #[tokio::test(flavor = "multi_thread", worker_threads = 3)]
    async fn probe_distinguishes_valid_empty_discovery_from_no_reply() {
        let net_id = "/ckbtest/deadbeef".to_string();
        let (empty_addr, _) = start_mock_peer_with_options(Some(&net_id), Some("")).await;
        let (silent_addr, _) = start_mock_peer_with_options(Some(&net_id), None).await;
        let prober = CkbProber::start(net_id, vec![], Duration::from_secs(2)).unwrap();
        let empty = prober
            .candidate_from_addr(&empty_addr, None)
            .unwrap()
            .unwrap();
        let silent = prober
            .candidate_from_addr(&silent_addr, None)
            .unwrap()
            .unwrap();

        let (empty_result, silent_result) = tokio::join!(
            prober.probe(&empty.peer_id, &empty.addr),
            prober.probe(&silent.peer_id, &silent.addr)
        );
        let empty_outcome = empty_result.unwrap().outcome.unwrap();
        let silent_outcome = silent_result.unwrap().outcome.unwrap();

        assert_eq!(empty_outcome.discovery.valid_nodes_messages, 1);
        assert!(empty_outcome.discovered_addrs.is_empty());
        assert_eq!(silent_outcome.discovery, DiscoveryEvidence::default());
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
            assert_eq!(
                result.unwrap().observation,
                AddressProbeResult::SameNetworkIdentified
            );
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
        assert_eq!(result.observation, AddressProbeResult::ForeignNetwork);
        assert!(result.outcome.is_none());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn malformed_identify_is_reported_from_real_protocol_callback() {
        let (dial_addr, _) = start_mock_peer_with_reply(MockIdentify::Malformed, None).await;
        let prober = CkbProber::start(
            "/ckbtest/deadbeef".to_string(),
            vec![],
            Duration::from_secs(2),
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

        assert_eq!(result.observation, AddressProbeResult::MalformedIdentify);
        assert!(result.outcome.is_none());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn refused_loopback_dial_is_reported_as_typed_dial_request_failure() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let remote_key = SecioKeyPair::secp256k1_generated();
        let addr = format!(
            "/ip4/127.0.0.1/tcp/{}/p2p/{}",
            port,
            remote_key.peer_id().to_base58()
        );
        let prober = CkbProber::start(
            "/ckbtest/deadbeef".to_string(),
            vec![],
            Duration::from_secs(2),
        )
        .unwrap();
        let candidate = prober.candidate_from_addr(&addr, None).unwrap().unwrap();

        let result = prober
            .probe(&candidate.peer_id, &candidate.addr)
            .await
            .unwrap();

        assert_eq!(result.observation, AddressProbeResult::DialRequestFailed);
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

        assert_eq!(
            result.observation,
            AddressProbeResult::AuthenticatedSessionWithoutIdentifyBeforeDeadline
        );
        assert!(result.outcome.is_none());
        tokio::time::timeout(Duration::from_secs(2), async {
            while closed_sessions.load(Ordering::SeqCst) == 0 {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("timed-out feeler probe must disconnect its session");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn pre_authentication_timeout_is_distinct_from_missing_identify() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            let (_socket, _) = listener.accept().await.unwrap();
            tokio::time::sleep(Duration::from_secs(2)).await;
        });
        let remote_key = SecioKeyPair::secp256k1_generated();
        let addr = format!(
            "/ip4/127.0.0.1/tcp/{}/p2p/{}",
            port,
            remote_key.peer_id().to_base58()
        );
        let prober = CkbProber::start(
            "/ckbtest/deadbeef".to_string(),
            vec![],
            Duration::from_millis(250),
        )
        .unwrap();
        let candidate = prober.candidate_from_addr(&addr, None).unwrap().unwrap();

        let result = prober
            .probe(&candidate.peer_id, &candidate.addr)
            .await
            .unwrap();

        assert_eq!(
            result.observation,
            AddressProbeResult::NoAuthenticatedSessionBeforeDeadline
        );
    }

    #[tokio::test]
    async fn unhealthy_local_service_is_fatal_before_dial() {
        let prober = CkbProber::start(
            "/ckbtest/deadbeef".to_string(),
            vec![],
            Duration::from_secs(1),
        )
        .unwrap();
        *prober.health.lock().unwrap() = Some("injected service termination".into());

        let error = prober
            .probe(
                b"not-used",
                "/ip4/127.0.0.1/tcp/1/p2p/QmaZMemLXSsxKUrYNucjEbPxVX3rBKsGhWW2muWtWxUWyh",
            )
            .await
            .unwrap_err();

        assert!(error.to_string().contains("injected service termination"));
    }

    #[tokio::test]
    async fn malformed_scheduled_address_is_an_invariant_error() {
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
        let error = prober
            .probe(b"not-a-peer", "not-a-multiaddr")
            .await
            .unwrap_err();
        assert!(error.to_string().contains("scheduled crawler address"));
    }
}
