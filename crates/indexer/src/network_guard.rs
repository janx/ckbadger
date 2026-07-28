//! Startup guards that bind an indexer run to exactly one chain-network.

use anyhow::{bail, Result};
use ckbadger_common::hardfork::normalize_network;
use ckbadger_common::network::{genesis_hash_matches, known_genesis_hash};
use ckbadger_store::CkbadgerStore;

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
    let configured = normalize_network(configured)
        .ok_or_else(|| anyhow::anyhow!("unknown configured chain-network '{configured}'"))?;
    if let Some(db_net) = existing {
        let db_canonical = normalize_network(db_net).ok_or_else(|| {
            anyhow::anyhow!(
                "DB network identity '{db_net}' is unknown; purge and re-sync before starting"
            )
        })?;
        if db_canonical != configured {
            bail!(
                "DB network mismatch: this store was synced for '{db_net}' but the \
                 config selects '{configured}'. Use a separate workdir per network, \
                 or purge and re-sync. (Refusing to mix chains.)"
            );
        }
    }
    Ok(())
}

/// Validate and, only after validation, initialize the domain store's network
/// identity. An old untagged store is adopted only when its persisted block 0
/// proves it belongs to the connected node's chain.
pub fn establish_db_network_identity(
    store: &CkbadgerStore,
    configured: &str,
    node_genesis_hash: &str,
) -> Result<()> {
    let configured_canonical = normalize_network(configured)
        .ok_or_else(|| anyhow::anyhow!("unknown configured chain-network '{configured}'"))?;
    let existing = store.get_network_identity()?;
    verify_db_network(existing.as_deref(), configured_canonical)?;
    if existing.is_some() {
        return Ok(());
    }

    if store.get_sync_tip_block()?.is_some() {
        let stored_genesis = store.get_block_header(0)?.ok_or_else(|| {
            anyhow::anyhow!(
                "untagged domain store has block_headers data but no block 0; \
                 the store is inconsistent — purge and re-sync"
            )
        })?;
        let node_genesis = node_genesis_hash.trim_start_matches("0x");
        let stored_genesis_hex = hex::encode(&stored_genesis.hash);
        if !stored_genesis_hex.eq_ignore_ascii_case(node_genesis) {
            bail!(
                "untagged domain store's stored genesis 0x{stored_genesis_hex} does not match \
                 connected node genesis {node_genesis_hash}; purge and re-sync the correct network"
            );
        }
    }

    store.set_network_identity(configured_canonical)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ckbadger_store::batch::StoreBatch;
    use ckbadger_store::types::CachedBlockHeader;
    use ckbadger_store::CkbadgerStore;

    fn header(hash: Vec<u8>) -> CachedBlockHeader {
        CachedBlockHeader {
            hash,
            parent_hash: vec![0; 32],
            timestamp: 0,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
            dao: vec![0; 32],
            transactions_count: 1,
            uncles_count: 0,
            proposals_count: 0,
            compact_target: 0,
            miner_lock_hash: None,
            cycles: None,
        }
    }

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
    fn genesis_guard_rejects_unknown_network() {
        let err = verify_genesis_hash(
            "devnet",
            "0x92b197aa1fba0f63633922c61c92375c9c074a93e85963554f5499fe1450d0e5",
        )
        .unwrap_err();
        assert!(err.to_string().contains("unknown"));
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

        // Historical aliases identify the same canonical chain.
        assert!(verify_db_network(Some("ckb"), "mainnet").is_ok());
        assert!(verify_db_network(Some("pudge"), "testnet").is_ok());
    }

    #[test]
    fn empty_db_is_stamped_with_canonical_network() {
        let dir = tempfile::TempDir::new().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();

        establish_db_network_identity(
            &store,
            "ckb",
            "0x92b197aa1fba0f63633922c61c92375c9c074a93e85963554f5499fe1450d0e5",
        )
        .unwrap();

        assert_eq!(
            store.get_network_identity().unwrap().as_deref(),
            Some("mainnet")
        );
    }

    #[test]
    fn untagged_nonempty_db_requires_matching_stored_genesis() {
        let dir = tempfile::TempDir::new().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        let mainnet_hash =
            hex::decode("92b197aa1fba0f63633922c61c92375c9c074a93e85963554f5499fe1450d0e5")
                .unwrap();
        let mut batch = StoreBatch::new(&store);
        batch.put_block_header(0, &header(mainnet_hash));
        batch.commit().unwrap();

        let err = establish_db_network_identity(
            &store,
            "testnet",
            "0x10639e0895502b5688a6be8cf69460d76541bfa4821629d86d62ba0aae3f9606",
        )
        .unwrap_err()
        .to_string();

        assert!(err.contains("stored genesis"));
        assert!(err.contains("purge and re-sync"));
        assert_eq!(store.get_network_identity().unwrap(), None);

        establish_db_network_identity(
            &store,
            "mainnet",
            "0x92b197aa1fba0f63633922c61c92375c9c074a93e85963554f5499fe1450d0e5",
        )
        .unwrap();
        assert_eq!(
            store.get_network_identity().unwrap().as_deref(),
            Some("mainnet")
        );
    }

    #[test]
    fn untagged_nonempty_db_without_block_zero_is_rejected() {
        let dir = tempfile::TempDir::new().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        let mut batch = StoreBatch::new(&store);
        batch.put_block_header(1, &header(vec![1; 32]));
        batch.commit().unwrap();

        let err = establish_db_network_identity(
            &store,
            "mainnet",
            "0x92b197aa1fba0f63633922c61c92375c9c074a93e85963554f5499fe1450d0e5",
        )
        .unwrap_err()
        .to_string();

        assert!(err.contains("block 0"));
        assert!(err.contains("block_headers"));
        assert_eq!(store.get_network_identity().unwrap(), None);
    }
}
