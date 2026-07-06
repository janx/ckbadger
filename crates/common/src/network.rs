//! Chain-network identity: per-network genesis hash, spec id, explorer URL.
//!
//! NOTE: "network" here means the CKB chain (mainnet/testnet), NOT the p2p
//! crawler store (`CF_NET_*`). Single source of truth for values the crawler,
//! indexer, and CLI all need.

use crate::hardfork::{normalize_network, NETWORK_MAINNET, NETWORK_TESTNET};

/// Genesis block hash (64-hex, no `0x`). Source: CKB v0.119.0 `resource/`.
pub const MAINNET_GENESIS_HASH: &str =
    "92b197aa1fba0f63633922c61c92375c9c074a93e85963554f5499fe1450d0e5";
pub const TESTNET_GENESIS_HASH: &str =
    "10639e0895502b5688a6be8cf69460d76541bfa4821629d86d62ba0aae3f9606";

/// CKB chain spec id (the `chain.spec` name announced in the node's Identify).
pub const MAINNET_SPEC_ID: &str = "ckb";
pub const TESTNET_SPEC_ID: &str = "ckb_testnet";

const MAINNET_EXPLORER_API: &str = "https://mainnet-api.explorer.nervos.org";
const TESTNET_EXPLORER_API: &str = "https://testnet-api.explorer.nervos.org";

/// Canonical genesis hash for a network (accepts aliases like `ckb`/`pudge`).
pub fn known_genesis_hash(network: &str) -> Option<&'static str> {
    match normalize_network(network)? {
        NETWORK_MAINNET => Some(MAINNET_GENESIS_HASH),
        NETWORK_TESTNET => Some(TESTNET_GENESIS_HASH),
        _ => None,
    }
}

/// CKB spec id for a network.
pub fn spec_id(network: &str) -> Option<&'static str> {
    match normalize_network(network)? {
        NETWORK_MAINNET => Some(MAINNET_SPEC_ID),
        NETWORK_TESTNET => Some(TESTNET_SPEC_ID),
        _ => None,
    }
}

/// CKB Explorer REST API base for a network (used by `verify`).
pub fn explorer_api_url(network: &str) -> Option<&'static str> {
    match normalize_network(network)? {
        NETWORK_MAINNET => Some(MAINNET_EXPLORER_API),
        NETWORK_TESTNET => Some(TESTNET_EXPLORER_API),
        _ => None,
    }
}

/// True iff `rpc_hash` (as returned by `get_block_hash(0)`, may be `0x`-prefixed
/// and mixed-case) equals the known genesis hash for `network`.
pub fn genesis_hash_matches(network: &str, rpc_hash: &str) -> bool {
    let Some(expected) = known_genesis_hash(network) else {
        return false;
    };
    let actual = rpc_hash.trim_start_matches("0x").to_ascii_lowercase();
    actual == expected
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn genesis_hashes_resolve_per_network() {
        assert_eq!(
            known_genesis_hash("mainnet"),
            Some("92b197aa1fba0f63633922c61c92375c9c074a93e85963554f5499fe1450d0e5"),
        );
        assert_eq!(
            known_genesis_hash("testnet"),
            Some("10639e0895502b5688a6be8cf69460d76541bfa4821629d86d62ba0aae3f9606"),
        );
        assert_eq!(known_genesis_hash("devnet"), None);
        // aliases normalize via hardfork::normalize_network
        assert_eq!(known_genesis_hash("ckb"), known_genesis_hash("mainnet"));
        assert_eq!(known_genesis_hash("pudge"), known_genesis_hash("testnet"));
    }

    #[test]
    fn genesis_hash_matches_normalizes_prefix_and_case() {
        // RPC returns a 0x-prefixed, possibly mixed-case hash
        assert!(genesis_hash_matches(
            "mainnet",
            "0x92B197AA1FBA0F63633922C61C92375C9C074A93E85963554F5499FE1450D0E5"
        ));
        assert!(!genesis_hash_matches("mainnet", "0xdeadbeef"));
        assert!(!genesis_hash_matches("devnet", "0x92b197aa"));
    }

    #[test]
    fn spec_ids_per_network() {
        assert_eq!(spec_id("mainnet"), Some("ckb"));
        assert_eq!(spec_id("testnet"), Some("ckb_testnet"));
        assert_eq!(spec_id("devnet"), None);
    }

    #[test]
    fn explorer_urls_per_network() {
        assert_eq!(
            explorer_api_url("mainnet"),
            Some("https://mainnet-api.explorer.nervos.org")
        );
        assert_eq!(
            explorer_api_url("testnet"),
            Some("https://testnet-api.explorer.nervos.org")
        );
        assert_eq!(explorer_api_url("devnet"), None);
    }
}
