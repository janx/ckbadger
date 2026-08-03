// UTXOSwap Intent Lock (lock script)
pub const INTENT_LOCK_CODE_HASH_MAINNET: &str =
    "0x3547c9aa563804e47ba3ebd37e6012e447c91a238f7aa71b1a75319f11df060e";
pub const INTENT_LOCK_CODE_HASH_TESTNET: &str =
    "0x4e9c30c8d6ce275740fbe69eae49c3d8c213578c5bd066f4938fe3c7dec6e101";

/// Intent args decoding lives in `ckbadger-common` so the indexer's persisted
/// activity metadata and the API's live `lockCalls` display share ONE decode
/// path and one set of field names. Re-exported here for parser-local callers.
pub use ckbadger_common::utxoswap::{parse_intent_args, IntentPayload, IntentType};

pub fn is_intent_lock(code_hash: &[u8]) -> bool {
    crate::parser::registry::PROTOCOL_REGISTRY.is(
        code_hash,
        crate::parser::registry::ProtocolScript::UtxoSwapIntent,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rpc::parse_hex_to_bytes;

    #[test]
    fn test_is_intent_lock_mainnet() {
        let code_hash = parse_hex_to_bytes(INTENT_LOCK_CODE_HASH_MAINNET);
        assert!(is_intent_lock(&code_hash));
    }

    #[test]
    fn test_is_intent_lock_testnet() {
        let code_hash = parse_hex_to_bytes(INTENT_LOCK_CODE_HASH_TESTNET);
        assert!(is_intent_lock(&code_hash));
    }

    #[test]
    fn test_is_intent_lock_rejects_other() {
        let code_hash = vec![0xAA; 32];
        assert!(!is_intent_lock(&code_hash));

        let zero = vec![0u8; 32];
        assert!(!is_intent_lock(&zero));
    }

    #[test]
    fn test_all_hashes_are_32_bytes() {
        assert_eq!(parse_hex_to_bytes(INTENT_LOCK_CODE_HASH_MAINNET).len(), 32);
        assert_eq!(parse_hex_to_bytes(INTENT_LOCK_CODE_HASH_TESTNET).len(), 32);
    }

    #[test]
    fn test_hashes_are_distinct() {
        let mainnet = parse_hex_to_bytes(INTENT_LOCK_CODE_HASH_MAINNET);
        let testnet = parse_hex_to_bytes(INTENT_LOCK_CODE_HASH_TESTNET);
        assert_ne!(
            mainnet, testnet,
            "mainnet and testnet hashes must be distinct"
        );
    }

    /// The re-exported decoder is the one from `ckbadger-common` — decoding a
    /// real mainnet AddLiquidity vector here proves the indexer is not carrying
    /// a second, divergent copy of the layout.
    #[test]
    fn reexported_decoder_is_the_shared_one() {
        let args = parse_hex_to_bytes("0x0001d85947f67df16556a1caef3b7f939a69fb2329273406698f36e9bdf46db404176859b0ba3a6b00000000000000000000000000000000013a219800000000000000000000000000805e9700000000000000000000000000506c0300000000000000000000000000ee670300000000000000000000000000");
        let parsed = parse_intent_args(&args).expect("real vector must parse");
        assert_eq!(parsed.intent_type, IntentType::AddLiquidity);
        assert_eq!(
            parsed.payload,
            IntentPayload::AddLiquidity {
                desired_x: 9_969_978,
                min_x: 9_920_128,
                desired_y: 224_336,
                min_y: 223_214,
            }
        );
    }
}
