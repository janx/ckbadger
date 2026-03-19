use std::sync::LazyLock;

use crate::rpc::parse_hex_to_bytes;

// Mainnet code hashes
pub const FUNDING_LOCK_CODE_HASH_MAINNET: &str =
    "0xe45b1f8f21bff23137035a3ab751d75b36a981deec3e7820194b9c042967f4f1";
pub const COMMITMENT_LOCK_CODE_HASH_MAINNET: &str =
    "0x2d45c4d3ed3e942f1945386ee82a5d1b7e4bb16d7fe1ab015421174ab747406c";

// Testnet code hashes
pub const FUNDING_LOCK_CODE_HASH_TESTNET: &str =
    "0x6c67887fe201ee0c7853f1682c0b77c0e6214044c156c7558269390a8afa6d7c";
pub const COMMITMENT_LOCK_CODE_HASH_TESTNET: &str =
    "0x740dee83f87c6f309824d8fd3fbdd3c8380ee6fc9acc90b1a748438afcdf81d8";

static FUNDING_MAINNET: LazyLock<Vec<u8>> =
    LazyLock::new(|| parse_hex_to_bytes(FUNDING_LOCK_CODE_HASH_MAINNET));
static FUNDING_TESTNET: LazyLock<Vec<u8>> =
    LazyLock::new(|| parse_hex_to_bytes(FUNDING_LOCK_CODE_HASH_TESTNET));
static COMMITMENT_MAINNET: LazyLock<Vec<u8>> =
    LazyLock::new(|| parse_hex_to_bytes(COMMITMENT_LOCK_CODE_HASH_MAINNET));
static COMMITMENT_TESTNET: LazyLock<Vec<u8>> =
    LazyLock::new(|| parse_hex_to_bytes(COMMITMENT_LOCK_CODE_HASH_TESTNET));

/// Minimum length for funding lock args: 20 bytes (pubkey_hash).
const FUNDING_LOCK_ARGS_MIN_LEN: usize = 20;

/// Minimum length for commitment lock args (short format):
/// pubkey_hash (20B) + delay_epoch (8B) + version (8B) = 36B.
/// Full format adds: settlement_hash (20B) + settlement_flag (1B) = 57B.
const COMMITMENT_LOCK_ARGS_MIN_LEN: usize = 36;

/// Length threshold for the full commitment lock args format (with settlement fields).
const COMMITMENT_LOCK_ARGS_FULL_LEN: usize = 57;

/// Parsed funding lock args from a Fiber funding-lock cell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FundingLockArgs {
    /// Blake2b-160 hash of the aggregated public key (20 bytes).
    pub pubkey_hash: Vec<u8>,
}

/// Parsed commitment lock args from a Fiber commitment-lock cell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitmentLockArgs {
    /// Blake2b-160 hash of the local public key (20 bytes).
    pub pubkey_hash: Vec<u8>,
    /// Delay epoch for revocation (8 bytes, little-endian).
    pub delay_epoch: u64,
    /// Version of the commitment (8 bytes, big-endian).
    pub version: u64,
    /// Hash of the settlement transaction (20 bytes). Present only in the full (>=57B) format.
    pub settlement_hash: Option<Vec<u8>>,
    /// Settlement flag (1 byte). Present only in the full (>=57B) format.
    pub settlement_flag: Option<u8>,
}

/// Returns true if the given code_hash matches a Fiber funding-lock (mainnet or testnet).
pub fn is_funding_lock(code_hash: &[u8]) -> bool {
    code_hash == FUNDING_MAINNET.as_slice() || code_hash == FUNDING_TESTNET.as_slice()
}

/// Returns true if the given code_hash matches a Fiber commitment-lock (mainnet or testnet).
pub fn is_commitment_lock(code_hash: &[u8]) -> bool {
    code_hash == COMMITMENT_MAINNET.as_slice() || code_hash == COMMITMENT_TESTNET.as_slice()
}

/// Returns all Fiber lock code hashes (funding + commitment, mainnet + testnet) as byte vectors.
/// Used for populating PROTOCOL_ACTION_LOCKS.
pub fn all_fiber_lock_code_hashes() -> Vec<Vec<u8>> {
    vec![
        FUNDING_MAINNET.clone(),
        FUNDING_TESTNET.clone(),
        COMMITMENT_MAINNET.clone(),
        COMMITMENT_TESTNET.clone(),
    ]
}

/// Parses funding lock args from raw bytes.
/// Layout: pubkey_hash (20 bytes).
/// Returns None if args is too short.
pub fn parse_funding_lock_args(args: &[u8]) -> Option<FundingLockArgs> {
    if args.len() < FUNDING_LOCK_ARGS_MIN_LEN {
        return None;
    }

    Some(FundingLockArgs {
        pubkey_hash: args[0..20].to_vec(),
    })
}

/// Parses commitment lock args from raw bytes.
/// Short format (36B): pubkey_hash (20B) + delay_epoch (8B LE) + version (8B BE).
/// Full format (57B): short + settlement_hash (20B) + settlement_flag (1B).
/// Returns None if args is shorter than 36 bytes.
pub fn parse_commitment_lock_args(args: &[u8]) -> Option<CommitmentLockArgs> {
    if args.len() < COMMITMENT_LOCK_ARGS_MIN_LEN {
        return None;
    }

    let pubkey_hash = args[0..20].to_vec();
    let delay_epoch = u64::from_le_bytes(args[20..28].try_into().ok()?);
    let version = u64::from_be_bytes(args[28..36].try_into().ok()?);

    let (settlement_hash, settlement_flag) = if args.len() >= COMMITMENT_LOCK_ARGS_FULL_LEN {
        (Some(args[36..56].to_vec()), Some(args[56]))
    } else {
        (None, None)
    };

    Some(CommitmentLockArgs {
        pubkey_hash,
        delay_epoch,
        version,
        settlement_hash,
        settlement_flag,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rpc::parse_hex_to_bytes;

    // --- is_funding_lock tests ---

    #[test]
    fn test_is_funding_lock_mainnet() {
        let code_hash = parse_hex_to_bytes(FUNDING_LOCK_CODE_HASH_MAINNET);
        assert!(is_funding_lock(&code_hash));
    }

    #[test]
    fn test_is_funding_lock_testnet() {
        let code_hash = parse_hex_to_bytes(FUNDING_LOCK_CODE_HASH_TESTNET);
        assert!(is_funding_lock(&code_hash));
    }

    #[test]
    fn test_is_funding_lock_rejects_commitment_hash() {
        let code_hash = parse_hex_to_bytes(COMMITMENT_LOCK_CODE_HASH_MAINNET);
        assert!(!is_funding_lock(&code_hash));
    }

    #[test]
    fn test_is_funding_lock_rejects_zero() {
        let code_hash = vec![0u8; 32];
        assert!(!is_funding_lock(&code_hash));
    }

    // --- is_commitment_lock tests ---

    #[test]
    fn test_is_commitment_lock_mainnet() {
        let code_hash = parse_hex_to_bytes(COMMITMENT_LOCK_CODE_HASH_MAINNET);
        assert!(is_commitment_lock(&code_hash));
    }

    #[test]
    fn test_is_commitment_lock_testnet() {
        let code_hash = parse_hex_to_bytes(COMMITMENT_LOCK_CODE_HASH_TESTNET);
        assert!(is_commitment_lock(&code_hash));
    }

    #[test]
    fn test_is_commitment_lock_rejects_funding_hash() {
        let code_hash = parse_hex_to_bytes(FUNDING_LOCK_CODE_HASH_MAINNET);
        assert!(!is_commitment_lock(&code_hash));
    }

    #[test]
    fn test_is_commitment_lock_rejects_zero() {
        let code_hash = vec![0u8; 32];
        assert!(!is_commitment_lock(&code_hash));
    }

    // --- all_fiber_lock_code_hashes tests ---

    #[test]
    fn test_all_fiber_lock_code_hashes_count() {
        let hashes = all_fiber_lock_code_hashes();
        assert_eq!(hashes.len(), 4);
    }

    #[test]
    fn test_all_fiber_lock_code_hashes_contains_all() {
        let hashes = all_fiber_lock_code_hashes();
        let funding_mainnet = parse_hex_to_bytes(FUNDING_LOCK_CODE_HASH_MAINNET);
        let funding_testnet = parse_hex_to_bytes(FUNDING_LOCK_CODE_HASH_TESTNET);
        let commitment_mainnet = parse_hex_to_bytes(COMMITMENT_LOCK_CODE_HASH_MAINNET);
        let commitment_testnet = parse_hex_to_bytes(COMMITMENT_LOCK_CODE_HASH_TESTNET);

        assert!(hashes.contains(&funding_mainnet));
        assert!(hashes.contains(&funding_testnet));
        assert!(hashes.contains(&commitment_mainnet));
        assert!(hashes.contains(&commitment_testnet));
    }

    #[test]
    fn test_all_fiber_lock_code_hashes_each_is_32_bytes() {
        for hash in all_fiber_lock_code_hashes() {
            assert_eq!(hash.len(), 32, "each code hash should be 32 bytes");
        }
    }

    // --- parse_funding_lock_args tests ---

    #[test]
    fn test_parse_funding_lock_args_valid() {
        let pubkey_hash = vec![0xAA; 20];
        let parsed = parse_funding_lock_args(&pubkey_hash).unwrap();
        assert_eq!(parsed.pubkey_hash, pubkey_hash);
    }

    #[test]
    fn test_parse_funding_lock_args_with_extra_bytes() {
        let mut args = vec![0xBB; 20];
        args.extend_from_slice(&[0xFF; 10]); // extra bytes beyond 20
        let parsed = parse_funding_lock_args(&args).unwrap();
        assert_eq!(parsed.pubkey_hash, vec![0xBB; 20]);
    }

    #[test]
    fn test_parse_funding_lock_args_too_short() {
        let args = vec![0xAA; 19];
        assert!(parse_funding_lock_args(&args).is_none());
    }

    #[test]
    fn test_parse_funding_lock_args_empty() {
        assert!(parse_funding_lock_args(&[]).is_none());
    }

    // --- parse_commitment_lock_args tests ---

    #[test]
    fn test_parse_commitment_lock_args_valid() {
        let mut args = Vec::with_capacity(57);
        let pubkey_hash = vec![0x11; 20];
        args.extend_from_slice(&pubkey_hash);

        let delay_epoch: u64 = 0x2000_0400_0000_0006; // example epoch value
        args.extend_from_slice(&delay_epoch.to_le_bytes());

        let version: u64 = 1;
        args.extend_from_slice(&version.to_be_bytes());

        let settlement_hash = vec![0x22; 20];
        args.extend_from_slice(&settlement_hash);

        let settlement_flag: u8 = 0x01;
        args.push(settlement_flag);

        assert_eq!(args.len(), 57);

        let parsed = parse_commitment_lock_args(&args).unwrap();
        assert_eq!(parsed.pubkey_hash, pubkey_hash);
        assert_eq!(parsed.delay_epoch, delay_epoch);
        assert_eq!(parsed.version, version);
        assert_eq!(parsed.settlement_hash, Some(settlement_hash));
        assert_eq!(parsed.settlement_flag, Some(settlement_flag));
    }

    #[test]
    fn test_parse_commitment_lock_args_short_format() {
        // 36-byte format: pubkey_hash(20) + delay_epoch(8) + version(8)
        let mut args = Vec::with_capacity(36);
        let pubkey_hash = vec![0x55; 20];
        args.extend_from_slice(&pubkey_hash);

        let delay_epoch: u64 = 42;
        args.extend_from_slice(&delay_epoch.to_le_bytes());

        let version: u64 = 3;
        args.extend_from_slice(&version.to_be_bytes());

        assert_eq!(args.len(), 36);

        let parsed = parse_commitment_lock_args(&args).unwrap();
        assert_eq!(parsed.pubkey_hash, pubkey_hash);
        assert_eq!(parsed.delay_epoch, delay_epoch);
        assert_eq!(parsed.version, version);
        assert_eq!(parsed.settlement_hash, None);
        assert_eq!(parsed.settlement_flag, None);
    }

    #[test]
    fn test_parse_commitment_lock_args_between_short_and_full() {
        // 40 bytes: short fields present, not enough for full settlement fields
        let mut args = vec![0xAA; 20]; // pubkey_hash
        args.extend_from_slice(&1u64.to_le_bytes()); // delay_epoch
        args.extend_from_slice(&1u64.to_be_bytes()); // version
        args.extend_from_slice(&[0xFF; 4]); // extra but not enough for settlement

        let parsed = parse_commitment_lock_args(&args).unwrap();
        assert_eq!(parsed.settlement_hash, None);
        assert_eq!(parsed.settlement_flag, None);
    }

    #[test]
    fn test_parse_commitment_lock_args_with_extra_bytes() {
        let mut args = Vec::with_capacity(60);
        args.extend_from_slice(&[0x33; 20]); // pubkey_hash
        args.extend_from_slice(&100u64.to_le_bytes()); // delay_epoch
        args.extend_from_slice(&2u64.to_be_bytes()); // version
        args.extend_from_slice(&[0x44; 20]); // settlement_hash
        args.push(0x00); // settlement_flag
        args.extend_from_slice(&[0xFF; 3]); // extra

        let parsed = parse_commitment_lock_args(&args).unwrap();
        assert_eq!(parsed.pubkey_hash, vec![0x33; 20]);
        assert_eq!(parsed.delay_epoch, 100);
        assert_eq!(parsed.version, 2);
        assert_eq!(parsed.settlement_hash, Some(vec![0x44; 20]));
        assert_eq!(parsed.settlement_flag, Some(0x00));
    }

    #[test]
    fn test_parse_commitment_lock_args_too_short() {
        let args = vec![0u8; 35]; // one byte short of minimum
        assert!(parse_commitment_lock_args(&args).is_none());
    }

    #[test]
    fn test_parse_commitment_lock_args_empty() {
        assert!(parse_commitment_lock_args(&[]).is_none());
    }

    #[test]
    fn test_parse_commitment_lock_args_zero_values() {
        let args = vec![0u8; 57];
        let parsed = parse_commitment_lock_args(&args).unwrap();
        assert_eq!(parsed.pubkey_hash, vec![0u8; 20]);
        assert_eq!(parsed.delay_epoch, 0);
        assert_eq!(parsed.version, 0);
        assert_eq!(parsed.settlement_hash, Some(vec![0u8; 20]));
        assert_eq!(parsed.settlement_flag, Some(0));
    }

    #[test]
    fn test_parse_commitment_lock_args_endianness() {
        let mut args = Vec::with_capacity(57);
        args.extend_from_slice(&[0u8; 20]); // pubkey_hash

        // delay_epoch = 0x0102030405060708 in LE
        let delay_bytes: [u8; 8] = [0x08, 0x07, 0x06, 0x05, 0x04, 0x03, 0x02, 0x01];
        args.extend_from_slice(&delay_bytes);

        // version = 0x0102030405060708 in BE
        let version_bytes: [u8; 8] = [0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08];
        args.extend_from_slice(&version_bytes);

        args.extend_from_slice(&[0u8; 20]); // settlement_hash
        args.push(0); // settlement_flag

        let parsed = parse_commitment_lock_args(&args).unwrap();
        assert_eq!(parsed.delay_epoch, 0x0102030405060708);
        assert_eq!(parsed.version, 0x0102030405060708);
    }

    // --- cross-function tests ---

    #[test]
    fn test_funding_and_commitment_hashes_are_distinct() {
        let funding_mainnet = parse_hex_to_bytes(FUNDING_LOCK_CODE_HASH_MAINNET);
        let commitment_mainnet = parse_hex_to_bytes(COMMITMENT_LOCK_CODE_HASH_MAINNET);
        assert_ne!(funding_mainnet, commitment_mainnet);

        // Funding hash should not match commitment check
        assert!(!is_commitment_lock(&funding_mainnet));
        // Commitment hash should not match funding check
        assert!(!is_funding_lock(&commitment_mainnet));
    }

    #[test]
    fn test_mainnet_and_testnet_hashes_are_distinct() {
        let funding_mainnet = parse_hex_to_bytes(FUNDING_LOCK_CODE_HASH_MAINNET);
        let funding_testnet = parse_hex_to_bytes(FUNDING_LOCK_CODE_HASH_TESTNET);
        assert_ne!(funding_mainnet, funding_testnet);

        let commitment_mainnet = parse_hex_to_bytes(COMMITMENT_LOCK_CODE_HASH_MAINNET);
        let commitment_testnet = parse_hex_to_bytes(COMMITMENT_LOCK_CODE_HASH_TESTNET);
        assert_ne!(commitment_mainnet, commitment_testnet);
    }
}
