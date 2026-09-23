//! `.cell` (DotCell) name cell parsing.
//!
//! Layout and semantics are spec §1.2–§1.5, verified byte-for-byte against
//! both networks on 2026-09-23. The data types live in `ckbadger_store::types`
//! so the identity entry, the parser and the API read path share one
//! vocabulary; this module owns the decoding.

use ckbadger_store::types::derive_dotcell_id;

use super::registry::{ProtocolScript, PROTOCOL_REGISTRY};

pub use ckbadger_store::types::{DotCellNameData, DotCellRecord};

/// The only layout version ever written on either network.
pub const DOTCELL_LAYOUT_VERSION: u8 = 3;
/// Fixed header length; the label runs from here to the end of the data.
pub const DOTCELL_HEADER_LEN: usize = 98;
pub const DOTCELL_ID_LEN: usize = 20;
/// `next == 0` marks the end of the uniqueness ring.
pub const DOTCELL_ZERO_ID: [u8; 20] = [0u8; 20];
/// Sale Lock args = `seller_lock_hash(32) ‖ price_shannons(u64 LE)`.
pub const DOTCELL_SALE_LOCK_ARGS_LEN: usize = 40;
/// The 30-day grace period the contract enforces after expiry, in seconds.
pub const DOTCELL_GRACE_SECONDS: u64 = 2_592_000;

pub struct DotCellParser;

impl DotCellParser {
    pub fn is_account_type_script(code_hash: &[u8]) -> bool {
        PROTOCOL_REGISTRY.is(code_hash, ProtocolScript::DotCellAccount)
    }

    pub fn is_account_lock(code_hash: &[u8]) -> bool {
        PROTOCOL_REGISTRY.is(code_hash, ProtocolScript::DotCellAccountLock)
    }

    pub fn is_sale_lock(code_hash: &[u8]) -> bool {
        PROTOCOL_REGISTRY.is(code_hash, ProtocolScript::DotCellSaleLock)
    }

    /// `blake2b(label)[..20]`. One implementation, in the store crate, because
    /// the API resolves `alice.cell` to an id with the same function.
    pub fn derive_id(label: &str) -> [u8; 20] {
        derive_dotcell_id(label)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::test_helpers::real_dotcell as fixture;
    use crate::rpc::parse_hex_to_bytes;

    #[test]
    fn account_type_script_is_recognised_on_both_networks() {
        for hex in [
            fixture::ACCOUNT_TYPE_CODE_HASH_MAINNET,
            fixture::ACCOUNT_TYPE_CODE_HASH_TESTNET,
        ] {
            assert!(
                DotCellParser::is_account_type_script(&parse_hex_to_bytes(hex)),
                "{hex} must be the Cells Account type script"
            );
        }
        for hex in [
            fixture::ACCOUNT_LOCK_CODE_HASH_MAINNET,
            fixture::SALE_LOCK_CODE_HASH_MAINNET,
            fixture::PRICE_TYPE_CODE_HASH_MAINNET,
        ] {
            assert!(
                !DotCellParser::is_account_type_script(&parse_hex_to_bytes(hex)),
                "{hex} is not the Account type script"
            );
        }
    }

    #[test]
    fn account_lock_and_sale_lock_are_distinct_on_both_networks() {
        for hex in [
            fixture::ACCOUNT_LOCK_CODE_HASH_MAINNET,
            fixture::ACCOUNT_LOCK_CODE_HASH_TESTNET,
        ] {
            let bytes = parse_hex_to_bytes(hex);
            assert!(DotCellParser::is_account_lock(&bytes), "{hex} account lock");
            assert!(!DotCellParser::is_sale_lock(&bytes));
        }
        for hex in [
            fixture::SALE_LOCK_CODE_HASH_MAINNET,
            fixture::SALE_LOCK_CODE_HASH_TESTNET,
        ] {
            let bytes = parse_hex_to_bytes(hex);
            assert!(DotCellParser::is_sale_lock(&bytes), "{hex} sale lock");
            assert!(!DotCellParser::is_account_lock(&bytes));
        }
    }

    #[test]
    fn derive_id_matches_protocol_vectors() {
        for (label, id) in [
            ("", "0x44f4c69744d5f8c55d642062949dcae49bc4e7ef"),
            ("support", "0x62d71147ac82b83c8531126cacb0d2f072bfd94a"),
            ("cellula", "0x629e27043d21c27bf12624ad840ac1ce848b3424"),
            ("maria", "0x2224948f63975a7a0741139cd5d2a45b9fb02c03"),
            ("joaom", "0x241e3586a41eb75dd6d68bf555acea74d6649ed5"),
            ("apt", "0x37561deee27ed512016aa4fd60418487fec9f944"),
            ("abuse", "0xa8d5f7507b9f3d30090253a741c1c80cb0cb121c"),
            (
                "v3-first-name",
                "0x4144e782dfaadeeb07625e11e4b6de717893aacb",
            ),
            (
                "shop.v3-first-name",
                "0xbb008a3e9045554d5b1b609c072b59b404320f9f",
            ),
            (
                "cartaoprova3695",
                "0xac485eebad1a642cc759559e02782791ad3ae89c",
            ),
            ("ref43euew", "0xeb3945173ac92a68c3158acfda30d9f19e609015"),
        ] {
            assert_eq!(
                DotCellParser::derive_id(label).to_vec(),
                parse_hex_to_bytes(id),
                "derive_id({label:?})"
            );
        }
    }
}
