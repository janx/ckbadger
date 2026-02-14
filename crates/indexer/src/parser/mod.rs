pub mod block;
pub mod cell;
pub mod dao;
pub mod dotbit;
pub mod mnft;
pub mod rgbpp;
pub mod script;
pub mod spore;
pub mod transaction;
pub mod udt;

pub use block::BlockParser;
pub use cell::CellParser;
pub use dao::{DaoParser, DaoState, ParsedDaoDeposit, ParsedDaoWithdrawRequest};
pub use dotbit::{DotbitParser, ParsedDotbitAccount};
pub use mnft::{MnftParser, ParsedMnftClass, ParsedMnftIssuer, ParsedMnftToken};
pub use rgbpp::{RgbppLockArgs, RgbppLockType, RgbppParser};
pub use script::ScriptParser;
pub use spore::{ParsedClusterCell, ParsedSporeCell, SporeParser};
pub use transaction::TransactionParser;
pub use udt::{ParsedUdtCell, ParsedUdtTransfer, UdtParser, UdtStandard};

/// Converts bytes to a UTF-8 string safe for PostgreSQL TEXT columns.
/// PostgreSQL TEXT columns cannot contain null bytes (0x00), so we strip them.
/// Invalid UTF-8 sequences are replaced with the Unicode replacement character.
pub fn bytes_to_pg_string(data: &[u8]) -> String {
    String::from_utf8_lossy(data).replace('\0', "").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bytes_to_pg_string_valid_utf8() {
        let data = b"hello world";
        assert_eq!(bytes_to_pg_string(data), "hello world");
    }

    #[test]
    fn test_bytes_to_pg_string_with_null_bytes() {
        let data = b"hello\x00world";
        assert_eq!(bytes_to_pg_string(data), "helloworld");
    }

    #[test]
    fn test_bytes_to_pg_string_only_null_bytes() {
        let data = b"\x00\x00\x00";
        assert_eq!(bytes_to_pg_string(data), "");
    }

    #[test]
    fn test_bytes_to_pg_string_with_invalid_utf8() {
        // Invalid UTF-8: 0xFF is not valid
        let data = b"hello\xFFworld";
        let result = bytes_to_pg_string(data);
        assert!(result.contains("hello"));
        assert!(result.contains("world"));
        // Invalid byte replaced with replacement character
        assert!(result.contains('\u{FFFD}'));
    }

    #[test]
    fn test_bytes_to_pg_string_mixed_null_and_invalid() {
        let data = b"test\x00\xFF\x00data";
        let result = bytes_to_pg_string(data);
        // Null bytes should be stripped, invalid UTF-8 replaced
        assert!(!result.contains('\0'));
        assert!(result.contains("test"));
        assert!(result.contains("data"));
    }
}
