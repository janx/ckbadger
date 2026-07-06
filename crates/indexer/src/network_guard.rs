//! Startup guards that bind an indexer run to exactly one chain-network.

use anyhow::{bail, Result};
use ckbadger_common::network::{genesis_hash_matches, known_genesis_hash};

/// Reject when the node's genesis block hash does not match `network`.
pub fn verify_genesis_hash(network: &str, node_genesis_hash: &str) -> Result<()> {
    let Some(expected) = known_genesis_hash(network) else {
        bail!("unknown chain-network '{network}' (expected mainnet or testnet)");
    };
    if !genesis_hash_matches(network, node_genesis_hash) {
        bail!(
            "genesis hash mismatch for network '{network}': node reports {node} \
             but expected 0x{expected}. The CKB node's chain does not match the \
             configured network — check [ckb].rpc_url / [ckb].network.",
            node = node_genesis_hash,
            expected = expected,
        );
    }
    Ok(())
}

/// Reject when the DB was previously synced for a different network.
pub fn verify_db_network(existing: Option<&str>, configured: &str) -> Result<()> {
    if let Some(db_net) = existing {
        if db_net != configured {
            bail!(
                "DB network mismatch: this store was synced for '{db_net}' but the \
                 config selects '{configured}'. Use a separate workdir per network, \
                 or purge and re-sync. (Refusing to mix chains.)"
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn genesis_guard_accepts_matching_and_rejects_mismatch() {
        // mainnet genesis hash, 0x-prefixed, from the node
        assert!(verify_genesis_hash(
            "mainnet",
            "0x92b197aa1fba0f63633922c61c92375c9c074a93e85963554f5499fe1450d0e5"
        )
        .is_ok());

        let err = verify_genesis_hash(
            "testnet",
            "0x92b197aa1fba0f63633922c61c92375c9c074a93e85963554f5499fe1450d0e5",
        )
        .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("testnet"));
        assert!(msg.contains("genesis"));
    }

    #[test]
    fn db_identity_guard() {
        // Fresh DB (no tag) is fine.
        assert!(verify_db_network(None, "mainnet").is_ok());
        // Same network is fine.
        assert!(verify_db_network(Some("mainnet"), "mainnet").is_ok());
        // Mismatch is rejected with both names in the message.
        let err = verify_db_network(Some("mainnet"), "testnet").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("mainnet") && msg.contains("testnet"));
    }
}
