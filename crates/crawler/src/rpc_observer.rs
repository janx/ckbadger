//! Direct CKB-session observations obtained from the configured local node.
//!
//! These facts are independent from the crawler's outbound Identify probes. In
//! particular, [`SessionInitiator::Peer`] proves that the remote peer initiated
//! a real CKB session to the configured local node, but says nothing about
//! whether the crawler can dial any address reported for that session.

use std::collections::HashSet;
use std::str::FromStr;
use std::time::Duration;

use anyhow::{anyhow, Context};
use async_trait::async_trait;
use ckb_jsonrpc_types::{LocalNode, RemoteNode};
use ckbadger_common::network::{MAINNET_GENESIS_HASH, TESTNET_GENESIS_HASH};
use ckbadger_store::SessionInitiator;
use p2p::secio::PeerId;
use serde::{Deserialize, Serialize};
use serde_json::Value;

const LOCAL_NODE_INFO_ID: u64 = 1;
const GET_PEERS_ID: u64 = 2;
const GENESIS_HASH_ID: u64 = 3;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalProtocolSnapshot {
    pub id: u64,
    pub name: String,
    pub support_versions: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteProtocolSnapshot {
    pub id: u64,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalObserverSnapshot {
    pub peer_id: Vec<u8>,
    pub client_version: String,
    pub active: bool,
    pub addresses: Vec<String>,
    pub protocols: Vec<LocalProtocolSnapshot>,
    pub connections: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectSessionSnapshot {
    pub peer_id: Vec<u8>,
    pub client_version: String,
    /// Addresses reported by `get_peers`. These are session facts only. An
    /// inbound connection can expose a temporary source port, so callers must
    /// never automatically turn these into crawler dial aliases.
    pub session_addresses: Vec<String>,
    pub initiator: SessionInitiator,
    pub connected_duration_ms: u64,
    pub last_ping_duration_ms: Option<u64>,
    pub protocols: Vec<RemoteProtocolSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalPeerSnapshot {
    pub observer: LocalObserverSnapshot,
    pub sessions: Vec<DirectSessionSnapshot>,
}

#[async_trait]
pub trait LocalPeerObserver: Send + Sync {
    async fn observe(&self) -> anyhow::Result<LocalPeerSnapshot>;
}

#[derive(Clone)]
pub struct CkbRpcPeerObserver {
    client: reqwest::Client,
    rpc_url: String,
    expected_genesis_hash: &'static str,
}

impl CkbRpcPeerObserver {
    pub fn new(rpc_url: impl Into<String>, network: &str) -> anyhow::Result<Self> {
        let expected_genesis_hash = match ckbadger_config::canonical_network_name(network)? {
            "mainnet" => MAINNET_GENESIS_HASH,
            "testnet" => TESTNET_GENESIS_HASH,
            canonical => anyhow::bail!(
                "canonical CKB network has no crawler genesis identity: network={canonical}"
            ),
        };
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
            .build()
            .context("failed to build crawler CKB RPC client")?;
        Ok(Self {
            client,
            rpc_url: rpc_url.into(),
            expected_genesis_hash,
        })
    }
}

#[derive(Debug, Serialize)]
struct JsonRpcRequest {
    jsonrpc: &'static str,
    id: u64,
    method: &'static str,
    params: Vec<Value>,
}

impl JsonRpcRequest {
    fn new(id: u64, method: &'static str) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            method,
            params: Vec::new(),
        }
    }
}

#[derive(Debug, Deserialize)]
struct JsonRpcError {
    code: i64,
    message: String,
}

#[derive(Debug, Deserialize)]
struct JsonRpcResponse {
    id: u64,
    result: Option<Value>,
    error: Option<JsonRpcError>,
}

fn take_result(
    responses: &mut Vec<JsonRpcResponse>,
    expected_id: u64,
    method: &str,
) -> anyhow::Result<Value> {
    let index = responses
        .iter()
        .position(|response| response.id == expected_id)
        .with_context(|| format!("CKB RPC batch omitted {method} response: id={expected_id}"))?;
    let response = responses.swap_remove(index);
    if let Some(error) = response.error {
        anyhow::bail!(
            "CKB RPC {method} failed: code={}, message={}",
            error.code,
            error.message
        );
    }
    response
        .result
        .with_context(|| format!("CKB RPC {method} returned no result: id={expected_id}"))
}

fn peer_id_bytes(node_id: &str, role: &str) -> anyhow::Result<Vec<u8>> {
    PeerId::from_str(node_id)
        .with_context(|| format!("CKB RPC returned malformed {role} node_id: {node_id}"))
        .map(|peer_id| peer_id.as_bytes().to_vec())
}

fn canonical_strings(values: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut values: Vec<String> = values.into_iter().collect();
    values.sort();
    values.dedup();
    values
}

fn canonical_genesis_hash<'a>(hash: &'a str, role: &str) -> anyhow::Result<&'a str> {
    let normalized = hash.strip_prefix("0x").unwrap_or(hash);
    if normalized.len() != 64 || hex::decode(normalized).is_err() {
        anyhow::bail!("{role} genesis hash is not exactly 32-byte hex: value={hash}");
    }
    Ok(normalized)
}

fn decode_snapshot(
    mut responses: Vec<JsonRpcResponse>,
    expected_genesis_hash: &str,
) -> anyhow::Result<LocalPeerSnapshot> {
    if responses.len() != 3 {
        anyhow::bail!(
            "CKB RPC peer-observation batch returned unexpected response count: expected=3, actual={}",
            responses.len()
        );
    }
    let actual_genesis_hash: String = serde_json::from_value(take_result(
        &mut responses,
        GENESIS_HASH_ID,
        "get_block_hash(genesis)",
    )?)
    .context("CKB RPC genesis block hash result has an invalid shape")?;
    let actual_normalized = canonical_genesis_hash(&actual_genesis_hash, "CKB RPC")?;
    let expected_normalized = canonical_genesis_hash(expected_genesis_hash, "configured network")?;
    if !actual_normalized.eq_ignore_ascii_case(expected_normalized) {
        anyhow::bail!(
            "configured CKB RPC network mismatch for peer observation: expected_genesis_hash={}, actual_genesis_hash={}",
            expected_genesis_hash,
            actual_genesis_hash
        );
    }
    let local: LocalNode = serde_json::from_value(take_result(
        &mut responses,
        LOCAL_NODE_INFO_ID,
        "local_node_info",
    )?)
    .context("CKB RPC local_node_info result has an invalid shape")?;
    let peers: Vec<RemoteNode> =
        serde_json::from_value(take_result(&mut responses, GET_PEERS_ID, "get_peers")?)
            .context("CKB RPC get_peers result has an invalid shape")?;
    if !responses.is_empty() {
        return Err(anyhow!(
            "CKB RPC peer-observation batch contained an unexpected response id: id={}",
            responses[0].id
        ));
    }

    let observer_peer_id = peer_id_bytes(&local.node_id, "local observer")?;
    let observer = LocalObserverSnapshot {
        peer_id: observer_peer_id.clone(),
        client_version: local.version,
        active: local.active,
        addresses: canonical_strings(local.addresses.into_iter().map(|address| address.address)),
        protocols: {
            let mut protocols: Vec<_> = local
                .protocols
                .into_iter()
                .map(|protocol| LocalProtocolSnapshot {
                    id: protocol.id.value(),
                    name: protocol.name,
                    support_versions: canonical_strings(protocol.support_versions),
                })
                .collect();
            protocols.sort_by(|left, right| {
                left.id
                    .cmp(&right.id)
                    .then_with(|| left.name.cmp(&right.name))
            });
            protocols
        },
        connections: local.connections.value(),
    };

    let mut seen_sessions = HashSet::new();
    let mut sessions = Vec::with_capacity(peers.len());
    for peer in peers {
        let peer_id = peer_id_bytes(&peer.node_id, "remote peer")?;
        if peer_id == observer_peer_id {
            anyhow::bail!(
                "CKB RPC get_peers returned the local observer as a remote peer: peer_id=0x{}",
                hex::encode(&peer_id)
            );
        }
        let initiator = if peer.is_outbound {
            SessionInitiator::Observer
        } else {
            SessionInitiator::Peer
        };
        if !seen_sessions.insert(peer_id.clone()) {
            anyhow::bail!(
                "CKB RPC get_peers returned duplicate remote peer rows: peer_id=0x{}",
                hex::encode(&peer_id)
            );
        }
        let mut protocols: Vec<_> = peer
            .protocols
            .into_iter()
            .map(|protocol| RemoteProtocolSnapshot {
                id: protocol.id.value(),
                version: protocol.version,
            })
            .collect();
        protocols.sort_by(|left, right| {
            left.id
                .cmp(&right.id)
                .then_with(|| left.version.cmp(&right.version))
        });
        sessions.push(DirectSessionSnapshot {
            peer_id,
            client_version: peer.version,
            session_addresses: canonical_strings(
                peer.addresses.into_iter().map(|address| address.address),
            ),
            // CKB's contract: is_outbound means the *local node* established
            // the connection. False is therefore the positive observation
            // that the remote peer initiated a session to this observer.
            initiator,
            connected_duration_ms: peer.connected_duration.value(),
            last_ping_duration_ms: peer.last_ping_duration.map(|duration| duration.value()),
            protocols,
        });
    }
    sessions.sort_by(|left, right| {
        (&left.peer_id, left.initiator).cmp(&(&right.peer_id, right.initiator))
    });
    Ok(LocalPeerSnapshot { observer, sessions })
}

#[async_trait]
impl LocalPeerObserver for CkbRpcPeerObserver {
    async fn observe(&self) -> anyhow::Result<LocalPeerSnapshot> {
        let responses = self
            .client
            .post(&self.rpc_url)
            .json(&[
                JsonRpcRequest::new(LOCAL_NODE_INFO_ID, "local_node_info"),
                JsonRpcRequest::new(GET_PEERS_ID, "get_peers"),
                JsonRpcRequest {
                    jsonrpc: "2.0",
                    id: GENESIS_HASH_ID,
                    method: "get_block_hash",
                    params: vec![Value::String("0x0".to_string())],
                },
            ])
            .send()
            .await
            .with_context(|| format!("failed to call CKB peer RPC: url={}", self.rpc_url))?
            .error_for_status()
            .with_context(|| format!("CKB peer RPC returned an HTTP error: url={}", self.rpc_url))?
            .json::<Vec<JsonRpcResponse>>()
            .await
            .context("CKB peer RPC returned invalid JSON batch data")?;
        decode_snapshot(responses, self.expected_genesis_hash)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OBSERVER_ID: &str = "QmaZMemLXSsxKUrYNucjEbPxVX3rBKsGhWW2muWtWxUWyh";
    const PEER_A_ID: &str = "QmYCRVonLfP18LSoz2WCHaXDorUYxuUMfhtcXK1TuZ1iwF";
    const PEER_B_ID: &str = "QmbT7QimcrcD5k2znoJiWpxoESxang6z1Gy9wof1rT1LKR";

    fn responses() -> Vec<JsonRpcResponse> {
        serde_json::from_value(serde_json::json!([
            {
                "jsonrpc": "2.0",
                "id": 2,
                "result": [
                    {
                        "version": "peer-inbound",
                        "node_id": PEER_A_ID,
                        "addresses": [],
                        "is_outbound": false,
                        "connected_duration": "0x2f",
                        "last_ping_duration": null,
                        "sync_state": null,
                        "protocols": [{"id": "0x1", "version": "0.0.1"}]
                    },
                    {
                        "version": "peer-outbound",
                        "node_id": PEER_B_ID,
                        "addresses": [{
                            "address": "/ip4/192.0.2.1/tcp/54321/p2p/QmbT7QimcrcD5k2znoJiWpxoESxang6z1Gy9wof1rT1LKR",
                            "score": "0x1"
                        }],
                        "is_outbound": true,
                        "connected_duration": "0x10",
                        "last_ping_duration": "0x2",
                        "sync_state": null,
                        "protocols": []
                    }
                ]
            },
            {
                "jsonrpc": "2.0",
                "id": 1,
                "result": {
                    "version": "local-ckb",
                    "node_id": OBSERVER_ID,
                    "active": true,
                    "addresses": [],
                    "protocols": [{
                        "id": "0x1",
                        "name": "/ckb/discovery",
                        "support_versions": ["0.0.1"]
                    }],
                    "connections": "0x2"
                }
            },
            {
                "jsonrpc": "2.0",
                "id": 3,
                "result": format!("0x{MAINNET_GENESIS_HASH}")
            }
        ]))
        .unwrap()
    }

    #[test]
    fn decodes_direction_from_the_local_observer_vantage() {
        let snapshot = decode_snapshot(responses(), MAINNET_GENESIS_HASH).unwrap();

        assert_eq!(snapshot.observer.client_version, "local-ckb");
        assert_eq!(snapshot.observer.connections, 2);
        assert_eq!(snapshot.sessions.len(), 2);
        assert_eq!(snapshot.sessions[0].initiator, SessionInitiator::Peer);
        assert!(snapshot.sessions[0].session_addresses.is_empty());
        assert_eq!(snapshot.sessions[0].connected_duration_ms, 47);
        assert_eq!(snapshot.sessions[1].initiator, SessionInitiator::Observer);
        assert_eq!(snapshot.sessions[1].last_ping_duration_ms, Some(2));
    }

    #[test]
    fn addressless_peer_session_is_valid_positive_evidence() {
        let snapshot = decode_snapshot(responses(), MAINNET_GENESIS_HASH).unwrap();
        let inbound = &snapshot.sessions[0];

        assert_eq!(inbound.initiator, SessionInitiator::Peer);
        assert!(inbound.session_addresses.is_empty());
    }

    #[test]
    fn rpc_errors_are_not_silently_treated_as_empty_peer_sets() {
        let mut responses = responses();
        responses[0] = serde_json::from_value(serde_json::json!({
            "jsonrpc": "2.0",
            "id": 2,
            "error": {"code": -32601, "message": "method not found"}
        }))
        .unwrap();

        let error = decode_snapshot(responses, MAINNET_GENESIS_HASH).unwrap_err();
        assert!(error.to_string().contains("get_peers failed"));
        assert!(error.to_string().contains("method not found"));
    }

    #[test]
    fn duplicate_remote_peer_rows_fail_fast() {
        let mut responses = responses();
        let peers = responses
            .iter_mut()
            .find(|response| response.id == GET_PEERS_ID)
            .unwrap()
            .result
            .as_mut()
            .unwrap()
            .as_array_mut()
            .unwrap();
        peers.push(peers[0].clone());

        let error = decode_snapshot(responses, MAINNET_GENESIS_HASH).unwrap_err();
        assert!(error.to_string().contains("duplicate remote peer rows"));
    }

    #[test]
    fn opposite_direction_duplicate_remote_peer_rows_fail_fast() {
        let mut responses = responses();
        let peers = responses
            .iter_mut()
            .find(|response| response.id == GET_PEERS_ID)
            .unwrap()
            .result
            .as_mut()
            .unwrap()
            .as_array_mut()
            .unwrap();
        let mut duplicate = peers[0].clone();
        duplicate["is_outbound"] = Value::Bool(true);
        peers.push(duplicate);

        let error = decode_snapshot(responses, MAINNET_GENESIS_HASH).unwrap_err();
        assert!(error.to_string().contains("duplicate remote peer rows"));
    }

    #[test]
    fn rpc_sessions_are_rejected_when_genesis_does_not_match_configured_network() {
        let error = decode_snapshot(responses(), TESTNET_GENESIS_HASH).unwrap_err();

        assert!(error.to_string().contains("network mismatch"));
        assert!(error.to_string().contains(MAINNET_GENESIS_HASH));
        assert!(error.to_string().contains(TESTNET_GENESIS_HASH));
    }
}
