use std::sync::LazyLock;

use crate::rpc::parse_hex_to_bytes;

// Mainnet code hashes
pub const RGBPP_LOCK_CODE_HASH_MAINNET: &str =
    "0xbc6c568a1a0d0a09f6844dc9d74ddb4343c32143ff25f727c59edf4fb72d6936";
pub const BTC_TIME_LOCK_CODE_HASH_MAINNET: &str =
    "0x70d64497a075bd651e98ac030455ea200637ee325a12ad08aff03f1a117e5a62";

// Testnet3 code hashes
pub const RGBPP_LOCK_CODE_HASH_TESTNET: &str =
    "0x61ca7a4796a4eb19ca4f0d065cb9b10ddcf002f10f7cbb810c706cb6bb5c3248";
pub const BTC_TIME_LOCK_CODE_HASH_TESTNET: &str =
    "0x00cdf8fab0f8ac638758ebf5ea5e4052b1d71e8a77b9f43139718621f6849326";

// Signet code hashes
pub const RGBPP_LOCK_CODE_HASH_SIGNET: &str =
    "0xd07598deec7ce7b5665310386b4abd06a6d48843e953c5cc2112ad0d5a220364";
pub const BTC_TIME_LOCK_CODE_HASH_SIGNET: &str =
    "0x80a09eca26d77cea1f5a69471c59481be7404febf40ee90f886c36a948385b55";

static RGBPP_MAINNET: LazyLock<Vec<u8>> =
    LazyLock::new(|| parse_hex_to_bytes(RGBPP_LOCK_CODE_HASH_MAINNET));
static RGBPP_TESTNET: LazyLock<Vec<u8>> =
    LazyLock::new(|| parse_hex_to_bytes(RGBPP_LOCK_CODE_HASH_TESTNET));
static RGBPP_SIGNET: LazyLock<Vec<u8>> =
    LazyLock::new(|| parse_hex_to_bytes(RGBPP_LOCK_CODE_HASH_SIGNET));
static BTC_TIME_MAINNET: LazyLock<Vec<u8>> =
    LazyLock::new(|| parse_hex_to_bytes(BTC_TIME_LOCK_CODE_HASH_MAINNET));
static BTC_TIME_TESTNET: LazyLock<Vec<u8>> =
    LazyLock::new(|| parse_hex_to_bytes(BTC_TIME_LOCK_CODE_HASH_TESTNET));
static BTC_TIME_SIGNET: LazyLock<Vec<u8>> =
    LazyLock::new(|| parse_hex_to_bytes(BTC_TIME_LOCK_CODE_HASH_SIGNET));

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RgbppLockType {
    RgbppLock,
    BtcTimeLock,
    Other,
}

#[derive(Debug, Clone)]
pub struct RgbppLockArgs {
    pub out_index: u32,
    pub btc_txid: String,
}

pub struct RgbppParser;

impl RgbppParser {
    pub fn is_rgbpp_lock_code_hash(code_hash: &[u8], is_mainnet: bool) -> bool {
        if is_mainnet {
            code_hash == RGBPP_MAINNET.as_slice()
        } else {
            code_hash == RGBPP_TESTNET.as_slice() || code_hash == RGBPP_SIGNET.as_slice()
        }
    }

    pub fn is_btc_time_lock_code_hash(code_hash: &[u8], is_mainnet: bool) -> bool {
        if is_mainnet {
            code_hash == BTC_TIME_MAINNET.as_slice()
        } else {
            code_hash == BTC_TIME_TESTNET.as_slice() || code_hash == BTC_TIME_SIGNET.as_slice()
        }
    }

    pub fn detect_lock_type(code_hash: &[u8], is_mainnet: bool) -> RgbppLockType {
        if Self::is_rgbpp_lock_code_hash(code_hash, is_mainnet) {
            RgbppLockType::RgbppLock
        } else if Self::is_btc_time_lock_code_hash(code_hash, is_mainnet) {
            RgbppLockType::BtcTimeLock
        } else {
            RgbppLockType::Other
        }
    }

    pub fn parse_rgbpp_lock_args(args: &[u8]) -> Option<RgbppLockArgs> {
        if args.len() < 36 {
            return None;
        }

        let out_index = u32::from_le_bytes(args[0..4].try_into().ok()?);

        let mut btc_txid = [0u8; 32];
        btc_txid.copy_from_slice(&args[4..36]);
        btc_txid.reverse();

        Some(RgbppLockArgs {
            out_index,
            btc_txid: hex::encode(btc_txid),
        })
    }

    pub fn extract_btc_txid_from_btc_time_lock_args(args: &[u8]) -> Option<String> {
        if args.len() < 36 {
            return None;
        }

        let mut btc_txid = [0u8; 32];
        btc_txid.copy_from_slice(&args[args.len() - 32..]);
        btc_txid.reverse();

        Some(hex::encode(btc_txid))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rpc::parse_hex_to_bytes;

    #[test]
    fn test_is_rgbpp_lock_code_hash_mainnet() {
        let code_hash = parse_hex_to_bytes(RGBPP_LOCK_CODE_HASH_MAINNET);
        assert!(RgbppParser::is_rgbpp_lock_code_hash(&code_hash, true));
        assert!(!RgbppParser::is_rgbpp_lock_code_hash(&code_hash, false));
    }

    #[test]
    fn test_is_rgbpp_lock_code_hash_testnet() {
        let code_hash = parse_hex_to_bytes(RGBPP_LOCK_CODE_HASH_TESTNET);
        assert!(!RgbppParser::is_rgbpp_lock_code_hash(&code_hash, true));
        assert!(RgbppParser::is_rgbpp_lock_code_hash(&code_hash, false));
    }

    #[test]
    fn test_is_rgbpp_lock_code_hash_signet() {
        let code_hash = parse_hex_to_bytes(RGBPP_LOCK_CODE_HASH_SIGNET);
        assert!(!RgbppParser::is_rgbpp_lock_code_hash(&code_hash, true));
        assert!(RgbppParser::is_rgbpp_lock_code_hash(&code_hash, false));
    }

    #[test]
    fn test_is_btc_time_lock_code_hash_mainnet() {
        let code_hash = parse_hex_to_bytes(BTC_TIME_LOCK_CODE_HASH_MAINNET);
        assert!(RgbppParser::is_btc_time_lock_code_hash(&code_hash, true));
        assert!(!RgbppParser::is_btc_time_lock_code_hash(&code_hash, false));
    }

    #[test]
    fn test_is_btc_time_lock_code_hash_testnet() {
        let code_hash = parse_hex_to_bytes(BTC_TIME_LOCK_CODE_HASH_TESTNET);
        assert!(!RgbppParser::is_btc_time_lock_code_hash(&code_hash, true));
        assert!(RgbppParser::is_btc_time_lock_code_hash(&code_hash, false));
    }

    #[test]
    fn test_detect_lock_type() {
        let rgbpp = parse_hex_to_bytes(RGBPP_LOCK_CODE_HASH_MAINNET);
        assert_eq!(
            RgbppParser::detect_lock_type(&rgbpp, true),
            RgbppLockType::RgbppLock
        );

        let btc_time = parse_hex_to_bytes(BTC_TIME_LOCK_CODE_HASH_MAINNET);
        assert_eq!(
            RgbppParser::detect_lock_type(&btc_time, true),
            RgbppLockType::BtcTimeLock
        );

        let other = vec![0u8; 32];
        assert_eq!(
            RgbppParser::detect_lock_type(&other, true),
            RgbppLockType::Other
        );
    }

    #[test]
    fn test_parse_rgbpp_lock_args() {
        let args =
            hex::decode("0200000006ec22c2def100bba3e295a1ff279c490d227151bf3166a4f3f008906c849399")
                .unwrap();

        let parsed = RgbppParser::parse_rgbpp_lock_args(&args).unwrap();
        assert_eq!(parsed.out_index, 2);
        assert_eq!(
            parsed.btc_txid,
            "9993846c9008f0f3a46631bf5171220d499c27ffa195e2a3bb00f1dec222ec06"
        );
    }

    #[test]
    fn test_parse_rgbpp_lock_args_too_short() {
        let args = vec![0u8; 35];
        assert!(RgbppParser::parse_rgbpp_lock_args(&args).is_none());
    }

    #[test]
    fn test_parse_rgbpp_lock_args_zero_index() {
        let mut args = vec![0u8; 36];
        let btc_txid =
            hex::decode("1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef")
                .unwrap();
        args[4..36].copy_from_slice(&btc_txid);

        let parsed = RgbppParser::parse_rgbpp_lock_args(&args).unwrap();
        assert_eq!(parsed.out_index, 0);
        assert_eq!(
            parsed.btc_txid,
            "efcdab9078563412efcdab9078563412efcdab9078563412efcdab9078563412"
        );
    }

    #[test]
    fn test_extract_btc_txid_from_btc_time_lock_args() {
        let mut args = vec![0u8; 100];
        let btc_txid =
            hex::decode("1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef")
                .unwrap();
        args[68..100].copy_from_slice(&btc_txid);

        let extracted = RgbppParser::extract_btc_txid_from_btc_time_lock_args(&args).unwrap();
        assert_eq!(
            extracted,
            "efcdab9078563412efcdab9078563412efcdab9078563412efcdab9078563412"
        );
    }

    #[test]
    fn test_extract_btc_txid_from_btc_time_lock_args_too_short() {
        let args = vec![0u8; 35];
        assert!(RgbppParser::extract_btc_txid_from_btc_time_lock_args(&args).is_none());
    }
}
