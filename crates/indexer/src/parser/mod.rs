pub mod bit_cell;
pub mod block;
pub mod cell;
pub mod dao;
pub mod dotbit;
pub mod fiber;
pub mod media_source;
pub mod mnft;
pub mod registry;
pub mod rgbpp;
pub mod script;
pub mod spore;
pub mod stablepp;
pub mod transaction;
pub mod udt;
pub mod utxoswap;

pub use bit_cell::{BitCellParser, ParsedBitCell, ParsedBitCellOutput};
pub use block::BlockParser;
pub use cell::CellParser;
pub use dao::{DaoParser, DaoState, ParsedDaoDeposit, ParsedDaoWithdrawRequest};
pub use dotbit::{DotbitParser, ParsedDotbitAccount, ParsedDotbitAccountOutput};
pub use media_source::{analyze_spore_media_profile, build_dob1_svg, extract_dob1_pattern};
pub use mnft::{MnftParser, ParsedMnftClass, ParsedMnftIssuer, ParsedMnftToken};
pub use rgbpp::{RgbppLockArgs, RgbppLockType, RgbppParser};
pub use script::ScriptParser;
pub use spore::{ParsedClusterCell, ParsedSporeCell, SporeParser};
pub use transaction::TransactionParser;
pub use udt::{ParsedUdtCell, ParsedUdtTransfer, UdtParser, UdtStandard};

/// Converts bytes to a safe UTF-8 string.
/// Null bytes (0x00) are stripped and invalid UTF-8 sequences are replaced
/// with the Unicode replacement character.
pub fn bytes_to_safe_string(data: &[u8]) -> String {
    String::from_utf8_lossy(data).replace('\0', "").to_string()
}

/// Parses a hex-encoded CKB capacity string into an `i64`.
/// Accepts with or without `0x` prefix. Panics on invalid hex or overflow.
pub fn parse_capacity_i64(capacity_hex: &str) -> i64 {
    let raw = capacity_hex;
    let hex = capacity_hex.strip_prefix("0x").unwrap_or(capacity_hex);
    let cap = u64::from_str_radix(hex, 16)
        .unwrap_or_else(|e| panic!("invalid capacity hex '{}': {}", raw, e));
    i64::try_from(cap).unwrap_or_else(|_| panic!("capacity exceeds i64 range '{}': {}", raw, cap))
}

/// Validates that transaction outputs and outputs_data have the same length.
pub fn validate_outputs_data_len(
    outputs: &[crate::rpc::CellOutput],
    outputs_data: &[String],
    tx_hash: &str,
) {
    assert_eq!(
        outputs.len(),
        outputs_data.len(),
        "outputs/outputs_data length mismatch: tx_hash={}, outputs={}, data={}",
        tx_hash,
        outputs.len(),
        outputs_data.len()
    );
}

#[cfg(test)]
pub(crate) mod test_helpers {
    use crate::rpc::Script;

    pub fn create_lock_script() -> Script {
        Script {
            code_hash: "0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8"
                .to_string(),
            hash_type: "type".to_string(),
            args: "0x927f3e74dceb87c81ba65a19da4f098b4de75a0d".to_string(),
        }
    }

    /// Real did:ckb cells captured from a live CKB testnet node
    /// (2026-08-01 data-audit evidence, `get_cells` by the did:ckb type script).
    /// The item id served by the API is the full type-script `args`, which on
    /// chain is NOT fixed-width: of 421 live testnet cells, 390 carry 32-byte
    /// args and 31 carry 20-byte args. Cell data is a 4-byte prefix plus a
    /// molecule table wrapping a CBOR DID document; the indexer does not parse
    /// it (identity name is not derived from data).
    pub mod real_did_ckb {
        use crate::rpc::{CellOutput, Script};

        /// docs/metadata/scripts/did-ckb.toml `[testnet]` canonical_ref_hash.
        pub const TYPE_CODE_HASH_TESTNET: &str =
            "0x510150477b10d6ab551a509b71265f3164e9fd4137fcb5a4322f49f03092c7c5";
        /// docs/metadata/scripts/did-ckb.toml `[mainnet]` canonical_ref_hash.
        pub const TYPE_CODE_HASH_MAINNET: &str =
            "0x4a06164dc34dccade5afe3e847a97b6db743e79f5477fa3295acf02849c5984a";

        /// Live testnet cell 0x00290adc…:0 (block 18082860), 32-byte args.
        pub const CELL_32_TX_HASH: &str =
            "0x00290adcd8421a397dacc2a4442fc63cd32bc1611961ea52ce42191bc99795da";
        pub const CELL_32_ARGS: &str =
            "0x004c0201f73ba1604beaaa6f83cc40873de89d02c255711f38caba45fa383176";
        pub const CELL_32_CAPACITY: &str = "0x826299e00";
        pub const CELL_32_LOCK_CODE_HASH: &str =
            "0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8";
        pub const CELL_32_LOCK_ARGS: &str = "0xc59ba632699a1bebf009ddcb241c15338114fec0";
        pub const CELL_32_DATA: &str = "0x00000000dc0000000c000000dc000000cc000000a3687365727669636573a16b617470726f746f5f706473a264747970657819417470726f746f506572736f6e616c4461746153657276657268656e64706f696e7475687474703a2f2f6c6f63616c686f73743a383030306b616c736f4b6e6f776e4173816f61743a2f2f616c6963652e7465737473766572696669636174696f6e4d6574686f6473a167617470726f746f78396469643a6b65793a7a51337368703248566b4432566f4536376b6b6a62476d3859385a73357a44415a667774636a355772656d65537656474a";

        /// Live testnet cell 0x1d43c10b…:0 (block 21080336), 20-byte args.
        pub const CELL_20_TX_HASH: &str =
            "0x1d43c10be6dafda1ae5ac0d7d806853e0a05e25f8e7ac1e2ca8abb53c60f7f1a";
        pub const CELL_20_ARGS: &str = "0x00ee044b93fab31c060417d159f9678b7cc154d4";
        pub const CELL_20_CAPACITY: &str = "0xc2166e900";
        pub const CELL_20_LOCK_CODE_HASH: &str =
            "0xd23761b364210735c19c60561d213fb3beae2fd6172743719eff6920e020baac";
        pub const CELL_20_LOCK_ARGS: &str = "0x0001717c37421e22cec26d3fae72e85dd8456d0b9eef";
        pub const CELL_20_DATA: &str = "0x00000000c90000000c000000c9000000b9000000a1687365727669636573a16770726f66696c65a56362696f6b4c6f72656c20497073756d64747970656d56656c6c756d50726f66696c6566617661746172785868747470733a2f2f6170692e64696365626561722e636f6d2f392e782f706978656c2d6172742f706e673f736565643d6469643a636b623a6164786169733474376b7a72796271656337697674366c68726e366d6376677568656e64706f696e7466696e6c696e656b646973706c61794e616d65644d697363";

        fn cell(
            capacity: &str,
            lock_code_hash: &str,
            lock_args: &str,
            type_args: &str,
        ) -> CellOutput {
            CellOutput {
                capacity: capacity.to_string(),
                lock: Script {
                    code_hash: lock_code_hash.to_string(),
                    hash_type: "type".to_string(),
                    args: lock_args.to_string(),
                },
                type_: Some(Script {
                    code_hash: TYPE_CODE_HASH_TESTNET.to_string(),
                    hash_type: "type".to_string(),
                    args: type_args.to_string(),
                }),
            }
        }

        /// The audited 32-byte-args cell as an RPC `CellOutput` + data hex.
        pub fn cell_32() -> (CellOutput, &'static str) {
            (
                cell(
                    CELL_32_CAPACITY,
                    CELL_32_LOCK_CODE_HASH,
                    CELL_32_LOCK_ARGS,
                    CELL_32_ARGS,
                ),
                CELL_32_DATA,
            )
        }

        /// The 20-byte-args cell as an RPC `CellOutput` + data hex.
        pub fn cell_20() -> (CellOutput, &'static str) {
            (
                cell(
                    CELL_20_CAPACITY,
                    CELL_20_LOCK_CODE_HASH,
                    CELL_20_LOCK_ARGS,
                    CELL_20_ARGS,
                ),
                CELL_20_DATA,
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bytes_to_safe_string_valid_utf8() {
        let data = b"hello world";
        assert_eq!(bytes_to_safe_string(data), "hello world");
    }

    #[test]
    fn test_bytes_to_safe_string_with_null_bytes() {
        let data = b"hello\x00world";
        assert_eq!(bytes_to_safe_string(data), "helloworld");
    }

    #[test]
    fn test_bytes_to_safe_string_only_null_bytes() {
        let data = b"\x00\x00\x00";
        assert_eq!(bytes_to_safe_string(data), "");
    }

    #[test]
    fn test_bytes_to_safe_string_with_invalid_utf8() {
        // Invalid UTF-8: 0xFF is not valid
        let data = b"hello\xFFworld";
        let result = bytes_to_safe_string(data);
        assert!(result.contains("hello"));
        assert!(result.contains("world"));
        // Invalid byte replaced with replacement character
        assert!(result.contains('\u{FFFD}'));
    }

    #[test]
    fn test_bytes_to_safe_string_mixed_null_and_invalid() {
        let data = b"test\x00\xFF\x00data";
        let result = bytes_to_safe_string(data);
        // Null bytes should be stripped, invalid UTF-8 replaced
        assert!(!result.contains('\0'));
        assert!(result.contains("test"));
        assert!(result.contains("data"));
    }

    #[test]
    fn test_parse_capacity_i64_with_prefix() {
        assert_eq!(parse_capacity_i64("0x174876e800"), 100_000_000_000);
        assert_eq!(parse_capacity_i64("0x2540be400"), 10_000_000_000);
        assert_eq!(parse_capacity_i64("0x0"), 0);
    }

    #[test]
    fn test_parse_capacity_i64_without_prefix() {
        assert_eq!(parse_capacity_i64("174876e800"), 100_000_000_000);
    }

    #[test]
    #[should_panic(expected = "invalid capacity hex")]
    fn test_parse_capacity_i64_invalid_panics() {
        let _ = parse_capacity_i64("0xzz");
    }

    #[test]
    #[should_panic(expected = "capacity exceeds i64 range")]
    fn test_parse_capacity_i64_overflow_panics() {
        let _ = parse_capacity_i64("0x8000000000000000");
    }

    #[test]
    fn test_validate_outputs_data_len_matching() {
        use crate::rpc::CellOutput;
        let outputs = vec![CellOutput {
            capacity: "0x0".to_string(),
            lock: test_helpers::create_lock_script(),
            type_: None,
        }];
        let outputs_data = vec!["0x".to_string()];
        // Should not panic
        validate_outputs_data_len(&outputs, &outputs_data, "0xaabb");
    }

    #[test]
    #[should_panic(expected = "outputs/outputs_data length mismatch")]
    fn test_validate_outputs_data_len_mismatch_panics() {
        use crate::rpc::CellOutput;
        let outputs = vec![CellOutput {
            capacity: "0x0".to_string(),
            lock: test_helpers::create_lock_script(),
            type_: None,
        }];
        let outputs_data: Vec<String> = vec![];
        validate_outputs_data_len(&outputs, &outputs_data, "0xaabb");
    }
}
