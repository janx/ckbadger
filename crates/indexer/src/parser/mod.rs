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
