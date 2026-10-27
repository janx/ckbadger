//! The chain's consensus parameters, read from the node — the API's one source
//! for the NC-Max proposal window, the orphan-rate target and the block limits.
//!
//! Every derivation that depends on these (proposal commitment windows, the
//! committable/expired split, mempool block packing, the pipeline snapshot,
//! the uncle-rate chart's target line) reads them from here. They come from the node's `get_consensus` RPC once
//! per process and are then cached; when the node cannot answer, the request
//! that needed them fails with the RPC error — there is no hardcoded mainnet
//! value standing in, because a testnet or devnet chain may differ.

use serde::{Deserialize, Serialize};
use std::ops::RangeInclusive;
use tokio::sync::OnceCell;

use crate::pool::source::parse_hex_u64;

const GET_CONSENSUS: &str = "get_consensus";

/// The NC-Max transaction proposal window (RFC-0020): a short id proposed in
/// block `P` may be committed in blocks `P + closest ..= P + farthest`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProposalWindow {
    closest: i64,
    farthest: i64,
}

impl ProposalWindow {
    /// Build a window from the node's values. A proposal can never commit in
    /// the block that proposes it, and the window cannot be inverted; a node
    /// reporting otherwise is an error, not something to repair.
    pub fn new(closest: u64, farthest: u64) -> Result<Self, String> {
        if closest == 0 || closest > farthest {
            return Err(format!(
                "invalid tx_proposal_window {{closest: {closest}, farthest: {farthest}}}: \
                 expected 1 <= closest <= farthest"
            ));
        }
        let to_i64 = |value: u64, field: &str| {
            i64::try_from(value)
                .map_err(|_| format!("tx_proposal_window.{field} {value} does not fit i64"))
        };
        Ok(Self {
            closest: to_i64(closest, "closest")?,
            farthest: to_i64(farthest, "farthest")?,
        })
    }

    pub fn closest(&self) -> i64 {
        self.closest
    }

    pub fn farthest(&self) -> i64 {
        self.farthest
    }

    /// Blocks in which a proposal made in `proposer_block` may be committed.
    pub fn commit_range(&self, proposer_block: i64) -> RangeInclusive<i64> {
        (proposer_block + self.closest)..=(proposer_block + self.farthest)
    }

    /// Blocks whose proposal zones a transaction committed in `commit_block`
    /// may have been proposed in. Numbers below genesis are not blocks; the
    /// caller intersects with the chain it has.
    pub fn proposal_range(&self, commit_block: i64) -> RangeInclusive<i64> {
        (commit_block - self.farthest)..=(commit_block - self.closest)
    }
}

/// The NC-Max orphan-rate target (RFC-0020): the uncle rate the difficulty
/// adjustment steers each epoch toward, `numerator / denominator` (mainnet
/// `1/40` = 2.5%). Kept as the node's exact fraction, never as a float.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OrphanRateTarget {
    numerator: u32,
    denominator: u32,
}

impl OrphanRateTarget {
    /// Build the target from the node's values. A zero denominator is not a
    /// rate, and a value beyond `u32` is not one any CKB chain uses; both are
    /// errors, not something to repair.
    pub fn new(numerator: u64, denominator: u64) -> Result<Self, String> {
        if denominator == 0 {
            return Err(format!(
                "invalid orphan_rate_target {numerator}/{denominator}: denominator is 0"
            ));
        }
        let to_u32 = |value: u64, field: &str| {
            u32::try_from(value)
                .map_err(|_| format!("orphan_rate_target.{field} {value} does not fit u32"))
        };
        Ok(Self {
            numerator: to_u32(numerator, "numer")?,
            denominator: to_u32(denominator, "denom")?,
        })
    }

    pub fn numerator(&self) -> u32 {
        self.numerator
    }

    pub fn denominator(&self) -> u32 {
        self.denominator
    }
}

/// The consensus parameters the API derives from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsensusParams {
    pub tx_proposal_window: ProposalWindow,
    pub orphan_rate_target: OrphanRateTarget,
    /// Serialized-size limit of a whole block, in bytes.
    pub max_block_bytes: u64,
    /// Total verification-cycles limit of a block.
    pub max_block_cycles: u64,
    /// Maximum number of proposal short ids one block may carry.
    pub max_block_proposals_limit: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HardforkFeature {
    pub rfc: String,
    pub activation_epoch: Option<u64>,
}

pub fn parse_hardfork_features(result: &serde_json::Value) -> Result<Vec<HardforkFeature>, String> {
    let entries = result
        .get("hardfork_features")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| "get_consensus.hardfork_features must be an array".to_string())?;
    let mut seen = std::collections::HashSet::new();
    let mut features = Vec::with_capacity(entries.len());
    for (index, entry) in entries.iter().enumerate() {
        let rfc = entry
            .get("rfc")
            .and_then(serde_json::Value::as_str)
            .filter(|value| value.len() == 4 && value.bytes().all(|byte| byte.is_ascii_digit()))
            .ok_or_else(|| format!("get_consensus.hardfork_features[{index}].rfc is invalid"))?;
        if !seen.insert(rfc) {
            return Err(format!(
                "get_consensus.hardfork_features has duplicate RFC {rfc}"
            ));
        }
        let epoch = entry.get("epoch_number").ok_or_else(|| {
            format!("get_consensus.hardfork_features[{index}].epoch_number is missing")
        })?;
        let activation_epoch = if epoch.is_null() {
            None
        } else {
            let hex = epoch.as_str().ok_or_else(|| {
                format!("get_consensus.hardfork_features[{index}].epoch_number is not hex")
            })?;
            Some(parse_hex_u64(
                hex,
                &format!("get_consensus.hardfork_features[{index}].epoch_number"),
            )?)
        };
        features.push(HardforkFeature {
            rfc: rfc.to_string(),
            activation_epoch,
        });
    }
    features.sort_by(|left, right| left.rfc.cmp(&right.rfc));
    Ok(features)
}

/// The `get_consensus` fields this crate reads. Everything else the node
/// returns is ignored.
#[derive(Debug, Deserialize)]
struct RpcConsensus {
    tx_proposal_window: RpcProposalWindow,
    orphan_rate_target: RpcRational,
    max_block_bytes: String,
    max_block_cycles: String,
    max_block_proposals_limit: String,
}

#[derive(Debug, Deserialize)]
struct RpcProposalWindow {
    closest: String,
    farthest: String,
}

/// A `RationalU256` as the node serializes it: hex `numer` and `denom`.
#[derive(Debug, Deserialize)]
struct RpcRational {
    numer: String,
    denom: String,
}

impl ConsensusParams {
    /// Parse the `result` object of a `get_consensus` response. A missing or
    /// malformed field is an error naming it.
    pub fn from_get_consensus(result: &serde_json::Value) -> Result<Self, String> {
        let raw: RpcConsensus = serde_json::from_value(result.clone())
            .map_err(|e| format!("{GET_CONSENSUS} result malformed: {e}"))?;
        let field =
            |value: &str, name: &str| parse_hex_u64(value, &format!("{GET_CONSENSUS}.{name}"));
        Ok(Self {
            tx_proposal_window: ProposalWindow::new(
                field(
                    &raw.tx_proposal_window.closest,
                    "tx_proposal_window.closest",
                )?,
                field(
                    &raw.tx_proposal_window.farthest,
                    "tx_proposal_window.farthest",
                )?,
            )
            .map_err(|e| format!("{GET_CONSENSUS}: {e}"))?,
            orphan_rate_target: OrphanRateTarget::new(
                field(&raw.orphan_rate_target.numer, "orphan_rate_target.numer")?,
                field(&raw.orphan_rate_target.denom, "orphan_rate_target.denom")?,
            )
            .map_err(|e| format!("{GET_CONSENSUS}: {e}"))?,
            max_block_bytes: field(&raw.max_block_bytes, "max_block_bytes")?,
            max_block_cycles: field(&raw.max_block_cycles, "max_block_cycles")?,
            max_block_proposals_limit: field(
                &raw.max_block_proposals_limit,
                "max_block_proposals_limit",
            )?,
        })
    }
}

#[derive(Debug, Serialize)]
struct RpcRequest {
    id: u64,
    jsonrpc: &'static str,
    method: &'static str,
    params: [(); 0],
}

#[derive(Debug, Deserialize)]
struct RpcResponse {
    result: Option<serde_json::Value>,
    error: Option<RpcError>,
}

#[derive(Debug, Deserialize)]
struct RpcError {
    code: i64,
    message: String,
}

/// Ask the node at `rpc_url` for its consensus parameters. Every failure —
/// transport, RPC error, empty result, malformed field — is an error that
/// names `get_consensus`.
pub async fn load_consensus(rpc_url: &str) -> Result<ConsensusParams, String> {
    let result = fetch_consensus_result(rpc_url).await?;
    ConsensusParams::from_get_consensus(&result)
}

pub async fn load_hardfork_features(rpc_url: &str) -> Result<Vec<HardforkFeature>, String> {
    let result = fetch_consensus_result(rpc_url).await?;
    parse_hardfork_features(&result)
}

async fn fetch_consensus_result(rpc_url: &str) -> Result<serde_json::Value, String> {
    let response: RpcResponse = crate::utils::shared_http_client()
        .post(rpc_url)
        .json(&RpcRequest {
            id: 1,
            jsonrpc: "2.0",
            method: GET_CONSENSUS,
            params: [],
        })
        .send()
        .await
        .map_err(|e| format!("{GET_CONSENSUS} request to {rpc_url} failed: {e}"))?
        .json()
        .await
        .map_err(|e| format!("{GET_CONSENSUS} response from {rpc_url} undecodable: {e}"))?;

    if let Some(error) = response.error {
        return Err(format!(
            "{GET_CONSENSUS} RPC error {}: {}",
            error.code, error.message
        ));
    }
    let result = response
        .result
        .ok_or_else(|| format!("{GET_CONSENSUS} returned no result"))?;
    Ok(result)
}

/// Process-wide holder of the node's consensus parameters: loaded on first use,
/// then served from memory. A failed load is not cached — the next request asks
/// the node again — and nothing is ever substituted for it.
#[derive(Debug, Default)]
pub struct ConsensusCell(OnceCell<ConsensusParams>);

impl ConsensusCell {
    /// Empty: the first [`ConsensusCell::get_or_load`] asks the node.
    pub fn new() -> Self {
        Self(OnceCell::new())
    }

    /// Already known (test fixtures hand the API a chain's parameters this way).
    pub fn preloaded(params: ConsensusParams) -> Self {
        Self(OnceCell::new_with(Some(params)))
    }

    pub async fn get_or_load(&self, rpc_url: &str) -> Result<&ConsensusParams, String> {
        self.0.get_or_try_init(|| load_consensus(rpc_url)).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hardfork_features_preserve_epoch_zero_and_reject_duplicate_or_malformed_evidence() {
        let result: serde_json::Value =
            serde_json::from_str(include_str!("../tests/fixtures/mainnet_get_consensus.json"))
                .unwrap();
        let features = parse_hardfork_features(&result).unwrap();
        assert_eq!(
            features
                .iter()
                .find(|feature| feature.rfc == "0029")
                .unwrap()
                .activation_epoch,
            Some(0)
        );
        assert_eq!(
            features
                .iter()
                .find(|feature| feature.rfc == "0049")
                .unwrap()
                .activation_epoch,
            Some(12293)
        );
        let mut malformed = result.clone();
        malformed["hardfork_features"][0]["epoch_number"] = serde_json::json!("bad-hex");
        assert!(parse_hardfork_features(&malformed)
            .unwrap_err()
            .contains("epoch_number"));
        let mut duplicate = result;
        let first = duplicate["hardfork_features"][0].clone();
        duplicate["hardfork_features"]
            .as_array_mut()
            .unwrap()
            .push(first);
        assert!(parse_hardfork_features(&duplicate)
            .unwrap_err()
            .contains("duplicate RFC"));
    }
    use wiremock::matchers::{body_partial_json, method};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn mainnet_result() -> serde_json::Value {
        serde_json::from_str(include_str!("../tests/fixtures/mainnet_get_consensus.json"))
            .expect("mainnet get_consensus fixture is valid JSON")
    }

    #[test]
    fn parses_mainnet_get_consensus() {
        let params = ConsensusParams::from_get_consensus(&mainnet_result()).unwrap();
        assert_eq!(params.tx_proposal_window.closest(), 2);
        assert_eq!(params.tx_proposal_window.farthest(), 10);
        assert_eq!(params.orphan_rate_target.numerator(), 1);
        assert_eq!(params.orphan_rate_target.denominator(), 40);
        assert_eq!(params.max_block_bytes, 597_000);
        assert_eq!(params.max_block_cycles, 3_500_000_000);
        assert_eq!(params.max_block_proposals_limit, 1500);
    }

    #[test]
    fn missing_field_is_named() {
        let mut result = mainnet_result();
        result.as_object_mut().unwrap().remove("max_block_bytes");
        let err = ConsensusParams::from_get_consensus(&result).unwrap_err();
        assert!(err.contains("max_block_bytes"), "{err}");
    }

    #[test]
    fn malformed_hex_is_an_error() {
        let mut result = mainnet_result();
        result["max_block_cycles"] = serde_json::json!("0xZZ");
        let err = ConsensusParams::from_get_consensus(&result).unwrap_err();
        assert!(err.contains("max_block_cycles"), "{err}");
    }

    #[test]
    fn missing_orphan_rate_target_is_named() {
        let mut result = mainnet_result();
        result.as_object_mut().unwrap().remove("orphan_rate_target");
        let err = ConsensusParams::from_get_consensus(&result).unwrap_err();
        assert!(err.contains("orphan_rate_target"), "{err}");
    }

    #[test]
    fn malformed_orphan_rate_target_is_an_error() {
        for (target, needle) in [
            (
                serde_json::json!({ "numer": "0x1", "denom": "0x0" }),
                "denominator is 0",
            ),
            (
                serde_json::json!({ "numer": "0x1", "denom": "0xZZ" }),
                "orphan_rate_target.denom",
            ),
            (
                serde_json::json!({ "numer": "0x100000000", "denom": "0x28" }),
                "orphan_rate_target.numer",
            ),
            (serde_json::json!({ "numer": "0x1" }), "denom"),
        ] {
            let mut result = mainnet_result();
            result["orphan_rate_target"] = target;
            let err = ConsensusParams::from_get_consensus(&result).unwrap_err();
            assert!(err.contains(needle), "{needle}: {err}");
        }
    }

    #[test]
    fn inverted_window_is_an_error() {
        let mut result = mainnet_result();
        result["tx_proposal_window"] = serde_json::json!({ "closest": "0xb", "farthest": "0xa" });
        let err = ConsensusParams::from_get_consensus(&result).unwrap_err();
        assert!(err.contains("tx_proposal_window"), "{err}");
    }

    #[test]
    fn window_ranges_follow_closest_and_farthest() {
        let window = ProposalWindow::new(2, 10).unwrap();
        assert_eq!(window.commit_range(100), 102..=110);
        assert_eq!(window.proposal_range(110), 100..=108);
    }

    async fn mount_get_consensus(server: &MockServer, body: serde_json::Value, expect: u64) {
        Mock::given(method("POST"))
            .and(body_partial_json(
                serde_json::json!({ "method": "get_consensus" }),
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .expect(expect)
            .mount(server)
            .await;
    }

    #[tokio::test]
    async fn rpc_error_is_an_error() {
        let server = MockServer::start().await;
        mount_get_consensus(
            &server,
            serde_json::json!({
                "jsonrpc": "2.0",
                "id": 1,
                "error": { "code": -32000, "message": "node is starting" }
            }),
            1,
        )
        .await;

        let err = load_consensus(&server.uri()).await.unwrap_err();
        assert!(err.contains("get_consensus"), "{err}");
        assert!(err.contains("node is starting"), "{err}");
    }

    #[tokio::test]
    async fn unreachable_node_is_an_error() {
        // Nothing listens on port 9 (discard) in the test environment.
        let err = load_consensus("http://127.0.0.1:9").await.unwrap_err();
        assert!(err.contains("get_consensus"), "{err}");
    }

    #[tokio::test]
    async fn cell_loads_once_then_serves_from_memory() {
        let server = MockServer::start().await;
        mount_get_consensus(
            &server,
            serde_json::json!({ "jsonrpc": "2.0", "id": 1, "result": mainnet_result() }),
            1,
        )
        .await;

        let cell = ConsensusCell::new();
        let first = cell.get_or_load(&server.uri()).await.unwrap().clone();
        let second = cell.get_or_load(&server.uri()).await.unwrap();
        assert_eq!(&first, second);
        assert_eq!(second.tx_proposal_window.farthest(), 10);
        // `expect(1)` is verified when the server drops.
    }

    #[tokio::test]
    async fn failed_load_is_not_cached() {
        let server = MockServer::start().await;
        let cell = ConsensusCell::new();
        assert!(cell.get_or_load(&server.uri()).await.is_err());

        mount_get_consensus(
            &server,
            serde_json::json!({ "jsonrpc": "2.0", "id": 1, "result": mainnet_result() }),
            1,
        )
        .await;
        let params = cell.get_or_load(&server.uri()).await.unwrap();
        assert_eq!(params.max_block_bytes, 597_000);
    }
}
