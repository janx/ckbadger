use std::sync::LazyLock;

use crate::rpc::parse_hex_to_bytes;

// Stable++ Asset (type script) — xudt_compatible
pub const ASSET_CODE_HASH_MAINNET: &str =
    "0x26a33e0815888a4a0614a0b7d09fa951e0993ff21e55905510104a0b1312032b";
pub const ASSET_CODE_HASH_TESTNET: &str =
    "0x1142755a044bf2ee358cba9f2da187ce928c91cd4dc8692ded0337efa677d21a";

// Stable++ Pool (type script)
pub const POOL_CODE_HASH_MAINNET: &str =
    "0x26622198b66240e437e323e0fecf1c26ba3c8c28a45f03ed3ebb9f7f2bdc0055";

// Stable++ Intent Lock (lock script)
pub const INTENT_LOCK_CODE_HASH_MAINNET: &str =
    "0x56fb632a13abdad7308d2e034baae1cb049e8e8ff23cc7c0b69449f617549733";

// Stable++ Vault Lock (lock script)
pub const VAULT_LOCK_CODE_HASH_MAINNET: &str =
    "0xff352022029a6ecf03e8a838b979a46e1231f05f9a3df9b4198f7eeb4afc2e67";

static ASSET_MAINNET: LazyLock<Vec<u8>> =
    LazyLock::new(|| parse_hex_to_bytes(ASSET_CODE_HASH_MAINNET));
static ASSET_TESTNET: LazyLock<Vec<u8>> =
    LazyLock::new(|| parse_hex_to_bytes(ASSET_CODE_HASH_TESTNET));
static POOL_MAINNET: LazyLock<Vec<u8>> =
    LazyLock::new(|| parse_hex_to_bytes(POOL_CODE_HASH_MAINNET));
static INTENT_MAINNET: LazyLock<Vec<u8>> =
    LazyLock::new(|| parse_hex_to_bytes(INTENT_LOCK_CODE_HASH_MAINNET));
static VAULT_MAINNET: LazyLock<Vec<u8>> =
    LazyLock::new(|| parse_hex_to_bytes(VAULT_LOCK_CODE_HASH_MAINNET));

pub fn is_stablepp_asset(code_hash: &[u8]) -> bool {
    code_hash == ASSET_MAINNET.as_slice() || code_hash == ASSET_TESTNET.as_slice()
}

pub fn is_stablepp_pool(code_hash: &[u8]) -> bool {
    code_hash == POOL_MAINNET.as_slice()
}

pub fn is_stablepp_intent_lock(code_hash: &[u8]) -> bool {
    code_hash == INTENT_MAINNET.as_slice()
}

pub fn is_stablepp_vault_lock(code_hash: &[u8]) -> bool {
    code_hash == VAULT_MAINNET.as_slice()
}

pub fn is_stablepp_script(code_hash: &[u8]) -> bool {
    is_stablepp_asset(code_hash)
        || is_stablepp_pool(code_hash)
        || is_stablepp_intent_lock(code_hash)
        || is_stablepp_vault_lock(code_hash)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rpc::parse_hex_to_bytes;

    // --- is_stablepp_asset tests ---

    #[test]
    fn test_is_stablepp_asset_mainnet() {
        let code_hash = parse_hex_to_bytes(ASSET_CODE_HASH_MAINNET);
        assert!(is_stablepp_asset(&code_hash));
    }

    #[test]
    fn test_is_stablepp_asset_testnet() {
        let code_hash = parse_hex_to_bytes(ASSET_CODE_HASH_TESTNET);
        assert!(is_stablepp_asset(&code_hash));
    }

    #[test]
    fn test_is_stablepp_asset_rejects_pool() {
        let code_hash = parse_hex_to_bytes(POOL_CODE_HASH_MAINNET);
        assert!(!is_stablepp_asset(&code_hash));
    }

    // --- is_stablepp_pool tests ---

    #[test]
    fn test_is_stablepp_pool_mainnet() {
        let code_hash = parse_hex_to_bytes(POOL_CODE_HASH_MAINNET);
        assert!(is_stablepp_pool(&code_hash));
    }

    #[test]
    fn test_is_stablepp_pool_rejects_asset() {
        let code_hash = parse_hex_to_bytes(ASSET_CODE_HASH_MAINNET);
        assert!(!is_stablepp_pool(&code_hash));
    }

    // --- is_stablepp_intent_lock tests ---

    #[test]
    fn test_is_stablepp_intent_lock_mainnet() {
        let code_hash = parse_hex_to_bytes(INTENT_LOCK_CODE_HASH_MAINNET);
        assert!(is_stablepp_intent_lock(&code_hash));
    }

    #[test]
    fn test_is_stablepp_intent_lock_rejects_vault() {
        let code_hash = parse_hex_to_bytes(VAULT_LOCK_CODE_HASH_MAINNET);
        assert!(!is_stablepp_intent_lock(&code_hash));
    }

    // --- is_stablepp_vault_lock tests ---

    #[test]
    fn test_is_stablepp_vault_lock_mainnet() {
        let code_hash = parse_hex_to_bytes(VAULT_LOCK_CODE_HASH_MAINNET);
        assert!(is_stablepp_vault_lock(&code_hash));
    }

    #[test]
    fn test_is_stablepp_vault_lock_rejects_intent() {
        let code_hash = parse_hex_to_bytes(INTENT_LOCK_CODE_HASH_MAINNET);
        assert!(!is_stablepp_vault_lock(&code_hash));
    }

    // --- is_stablepp_script tests ---

    #[test]
    fn test_is_stablepp_script_matches_asset() {
        let code_hash = parse_hex_to_bytes(ASSET_CODE_HASH_MAINNET);
        assert!(is_stablepp_script(&code_hash));
    }

    #[test]
    fn test_is_stablepp_script_matches_pool() {
        let code_hash = parse_hex_to_bytes(POOL_CODE_HASH_MAINNET);
        assert!(is_stablepp_script(&code_hash));
    }

    #[test]
    fn test_is_stablepp_script_matches_intent() {
        let code_hash = parse_hex_to_bytes(INTENT_LOCK_CODE_HASH_MAINNET);
        assert!(is_stablepp_script(&code_hash));
    }

    #[test]
    fn test_is_stablepp_script_matches_vault() {
        let code_hash = parse_hex_to_bytes(VAULT_LOCK_CODE_HASH_MAINNET);
        assert!(is_stablepp_script(&code_hash));
    }

    // --- negative tests ---

    #[test]
    fn test_is_stablepp_script_rejects_zero() {
        let code_hash = vec![0u8; 32];
        assert!(!is_stablepp_script(&code_hash));
    }

    #[test]
    fn test_is_stablepp_script_rejects_arbitrary() {
        let code_hash = vec![0xAA; 32];
        assert!(!is_stablepp_script(&code_hash));
    }

    #[test]
    fn test_all_hashes_are_32_bytes() {
        assert_eq!(parse_hex_to_bytes(ASSET_CODE_HASH_MAINNET).len(), 32);
        assert_eq!(parse_hex_to_bytes(ASSET_CODE_HASH_TESTNET).len(), 32);
        assert_eq!(parse_hex_to_bytes(POOL_CODE_HASH_MAINNET).len(), 32);
        assert_eq!(parse_hex_to_bytes(INTENT_LOCK_CODE_HASH_MAINNET).len(), 32);
        assert_eq!(parse_hex_to_bytes(VAULT_LOCK_CODE_HASH_MAINNET).len(), 32);
    }

    #[test]
    fn test_all_hashes_are_distinct() {
        let hashes = [
            parse_hex_to_bytes(ASSET_CODE_HASH_MAINNET),
            parse_hex_to_bytes(ASSET_CODE_HASH_TESTNET),
            parse_hex_to_bytes(POOL_CODE_HASH_MAINNET),
            parse_hex_to_bytes(INTENT_LOCK_CODE_HASH_MAINNET),
            parse_hex_to_bytes(VAULT_LOCK_CODE_HASH_MAINNET),
        ];
        for i in 0..hashes.len() {
            for j in (i + 1)..hashes.len() {
                assert_ne!(
                    hashes[i], hashes[j],
                    "hashes at index {} and {} collide",
                    i, j
                );
            }
        }
    }
}
