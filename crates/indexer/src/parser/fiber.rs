//! Fiber Network lock identification: which cells are channel cells.
//!
//! Lock args are not validated when a cell is created, so anyone can create a
//! cell whose lock code hash is the Fiber funding or commitment lock with any
//! args, type script and data. The shape of those bytes is therefore never an
//! indexer invariant. Whether a cell IS a Fiber channel cell is decided here,
//! by one function per lock: [`parse_funding_cell`] and
//! [`parse_commitment_lock_args`]. A cell either fits a layout some deployed
//! version of the lock itself accepts — then it is a channel cell — or it is an
//! ordinary cell: indexed like any other, never opening, closing or tracking a
//! channel. Both sync paths and the API call these functions and nothing else.
//!
//! The layouts are the deployed upstream lock sources (nervosnetwork/
//! fiber-scripts), every deployment under the same Type ID code hashes below:
//!
//! | Deployment (mainnet / testnet)                       | commitment-lock args          |
//! | ---------------------------------------------------- | ----------------------------- |
//! | — / 2024-05-13, 2024-06-14, 2024-06-28 (P2WSH)        | 20 (witness-script hash)      |
//! | v1 2025-02-28, v2 2025-04-14 / 2024-09-14, v3, v4     | 36, or 56 (pending-HTLC hash) |
//! | v0.6.0 2026-01-14 / 2025-11-25                        | 57                            |
//! | 0.7.0 2026-11-09 / 2026-10-21                         | 57, or 58 ending in 0x01      |
//!
//! The 20-byte P2WSH commitment (testnet only, before the daric redesign of
//! 2024-08-16) carries no pubkey hash, delay or version and is not modelled: a
//! cell with it is an ordinary cell here. Every deployed funding lock reads
//! `args[0..20]` and ignores the rest.

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

/// The commitment-lock features byte of the 58-byte layout:
/// `FEATURE_ONCHAIN_FULL_PAYMENT_HASH`, the only value the 0.7.0 lock accepts
/// (`resolve_htlc_layout`: `58 if args[57] == 0x01`).
pub const FEATURE_ONCHAIN_FULL_PAYMENT_HASH: u8 = 0x01;

/// Funding-lock args as every deployed funding lock reads them:
/// `pubkey_hash.copy_from_slice(&args[0..20])`. Bytes after the first 20 are
/// ignored by the lock, so they are ignored here too.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FundingLockArgs<'a> {
    /// Blake2b-160 of the channel's aggregated public key (`args[0..20]`).
    pub pubkey_hash: &'a [u8; 20],
}

/// The commitment-lock args layouts deployed versions accept; see the module
/// table. The common head (`args[0..36]`) is in [`CommitmentLockArgs`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommitmentLockLayout<'a> {
    /// 36 bytes (daric locks, mainnet v1–v2): no pending HTLCs.
    Legacy36,
    /// 56 bytes (daric locks, mainnet v1–v2): `args[36..56]` is the
    /// blake2b-160 of the pending HTLCs the unlock witness reveals.
    Legacy56 { pending_htlcs_hash: &'a [u8; 20] },
    /// 57 bytes (v0.6.0 onward; 0.7.0's "legacy" HTLC layout):
    /// `args[36..56]` settlement script hash, `args[56]` settlement flag
    /// (0x00 first settlement, anything else a later one).
    Full57 {
        settlement_hash: &'a [u8; 20],
        settlement_flag: u8,
    },
    /// 58 bytes (0.7.0 "v1" HTLC layout): the 57-byte fields plus the
    /// features byte [`FEATURE_ONCHAIN_FULL_PAYMENT_HASH`].
    Full58 {
        settlement_hash: &'a [u8; 20],
        settlement_flag: u8,
    },
}

impl CommitmentLockLayout<'_> {
    /// The layout's name in API responses (`lockFormat`).
    pub fn name(&self) -> &'static str {
        match self {
            Self::Legacy36 => "legacy36",
            Self::Legacy56 { .. } => "legacy56",
            Self::Full57 { .. } => "full57",
            Self::Full58 { .. } => "full58",
        }
    }
}

/// Commitment-lock args that fit a deployed layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommitmentLockArgs<'a> {
    /// `args[0..20]`: blake2b-160 of the commitment's public key.
    pub pubkey_hash: &'a [u8; 20],
    /// `args[20..28]`, little-endian: the delay as a CKB `since` value. The
    /// lock reads it only on settlement paths (never on revocation), so any
    /// value is accepted here.
    pub delay_epoch: u64,
    /// `args[28..36]`, big-endian: the commitment version (the lock compares
    /// the eight bytes lexicographically).
    pub version: u64,
    pub layout: CommitmentLockLayout<'a>,
}

impl<'a> CommitmentLockArgs<'a> {
    /// The settlement script hash of the 57- and 58-byte layouts.
    pub fn settlement_hash(&self) -> Option<&'a [u8; 20]> {
        match self.layout {
            CommitmentLockLayout::Full57 {
                settlement_hash, ..
            }
            | CommitmentLockLayout::Full58 {
                settlement_hash, ..
            } => Some(settlement_hash),
            CommitmentLockLayout::Legacy36 | CommitmentLockLayout::Legacy56 { .. } => None,
        }
    }

    /// The settlement flag of the 57- and 58-byte layouts.
    pub fn settlement_flag(&self) -> Option<u8> {
        match self.layout {
            CommitmentLockLayout::Full57 {
                settlement_flag, ..
            }
            | CommitmentLockLayout::Full58 {
                settlement_flag, ..
            } => Some(settlement_flag),
            CommitmentLockLayout::Legacy36 | CommitmentLockLayout::Legacy56 { .. } => None,
        }
    }
}

/// Returns true if the given code_hash matches a Fiber funding-lock (any network).
pub fn is_funding_lock(code_hash: &[u8]) -> bool {
    crate::parser::registry::PROTOCOL_REGISTRY.is(
        code_hash,
        crate::parser::registry::ProtocolScript::FiberFunding,
    )
}

/// Returns true if the given code_hash matches a Fiber commitment-lock (any network).
pub fn is_commitment_lock(code_hash: &[u8]) -> bool {
    crate::parser::registry::PROTOCOL_REGISTRY.is(
        code_hash,
        crate::parser::registry::ProtocolScript::FiberCommitment,
    )
}

/// Returns all Fiber lock code hashes (funding + commitment, mainnet + testnet) as byte vectors.
/// Used for populating PROTOCOL_ACTION_LOCKS.
pub fn all_fiber_lock_code_hashes() -> Vec<Vec<u8>> {
    vec![
        parse_hex_to_bytes(FUNDING_LOCK_CODE_HASH_MAINNET),
        parse_hex_to_bytes(FUNDING_LOCK_CODE_HASH_TESTNET),
        parse_hex_to_bytes(COMMITMENT_LOCK_CODE_HASH_MAINNET),
        parse_hex_to_bytes(COMMITMENT_LOCK_CODE_HASH_TESTNET),
    ]
}

/// Byte width of the UDT amount prefix carried by a UDT-funded funding cell.
pub const FUNDING_UDT_AMOUNT_LEN: usize = 16;

/// Parse the token amount held by a UDT-funded Fiber funding cell.
///
/// Every CKB UDT standard seen funding a Fiber channel encodes the amount as a
/// 16-byte little-endian prefix of cell data. Verified against live funding
/// cells of all four kinds present on chain (xUDT `0x25c29dc3…`, USDI
/// `0xcc9dc33e…`, Stable++ Asset `0x1142755a…`, sUDT `0xc5e5dcf2…`).
///
/// This is the single parse path for funding-cell UDT amounts, shared by the
/// live detector and the bulk reducer. Returns `None` when the data is too
/// short to hold an amount; [`parse_funding_cell`] never accepts a typed
/// funding cell whose data is that short, so a caller that gets `None` for a
/// cell the arbiter accepted has found an indexer bug.
pub fn parse_funding_udt_amount(data: &[u8]) -> Option<u128> {
    crate::parser::udt::UdtParser::parse_amount(data)
}

/// The funding-lock args every deployed funding lock accepts: at least 20
/// bytes, of which it reads the first 20. `None` for shorter args, which no
/// funding lock can ever unlock (`&args[0..20]` aborts the script).
pub fn parse_funding_lock_args(args: &[u8]) -> Option<FundingLockArgs<'_>> {
    args.first_chunk::<20>()
        .map(|pubkey_hash| FundingLockArgs { pubkey_hash })
}

/// The one arbiter for funding-lock cells: whether a cell whose lock code hash
/// is a Fiber funding lock is a channel funding cell. Its args must fit
/// [`parse_funding_lock_args`], and a cell that carries a type script funds a
/// UDT channel, so its data (`data_len` bytes) must hold the 16-byte
/// little-endian amount. `None`: an ordinary cell, never a channel.
///
/// `typed` and `data_len` are the cell's own, so the cell classifies the same
/// when it is created and when it is spent.
pub fn parse_funding_cell(
    lock_args: &[u8],
    typed: bool,
    data_len: usize,
) -> Option<FundingLockArgs<'_>> {
    if typed && data_len < FUNDING_UDT_AMOUNT_LEN {
        return None;
    }
    parse_funding_lock_args(lock_args)
}

/// The one arbiter for commitment-lock cells: whether a cell whose lock code
/// hash is a Fiber commitment lock is a channel commitment cell. Its args
/// must have a layout some deployed commitment lock accepts (36, 56, 57, or
/// 58 bytes ending in [`FEATURE_ONCHAIN_FULL_PAYMENT_HASH`]); anything else is
/// `None`, an ordinary cell, never a channel.
pub fn parse_commitment_lock_args(args: &[u8]) -> Option<CommitmentLockArgs<'_>> {
    let (head, rest) = args.split_first_chunk::<36>()?;
    let layout = match rest.len() {
        0 => CommitmentLockLayout::Legacy36,
        20 => CommitmentLockLayout::Legacy56 {
            pending_htlcs_hash: rest.first_chunk::<20>()?,
        },
        21 | 22 => {
            let (settlement_hash, tail) = rest.split_first_chunk::<20>()?;
            match *tail {
                [settlement_flag] => CommitmentLockLayout::Full57 {
                    settlement_hash,
                    settlement_flag,
                },
                [settlement_flag, FEATURE_ONCHAIN_FULL_PAYMENT_HASH] => {
                    CommitmentLockLayout::Full58 {
                        settlement_hash,
                        settlement_flag,
                    }
                }
                _ => return None,
            }
        }
        _ => return None,
    };
    let (pubkey_hash, tail) = head.split_first_chunk::<20>()?;
    let (delay, version) = tail.split_first_chunk::<8>()?;
    Some(CommitmentLockArgs {
        pubkey_hash,
        delay_epoch: u64::from_le_bytes(*delay),
        version: u64::from_be_bytes(*version.first_chunk::<8>()?),
        layout,
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

    // --- parse_funding_lock_args / parse_funding_cell tests ---

    #[test]
    fn test_parse_funding_lock_args_valid() {
        let pubkey_hash = [0xAA; 20];
        let parsed = parse_funding_lock_args(&pubkey_hash).unwrap();
        assert_eq!(parsed.pubkey_hash, &pubkey_hash);
    }

    /// Every deployed funding lock reads `args[0..20]` and ignores the rest
    /// (fiber-scripts funding-lock `pubkey_hash.copy_from_slice(&args[0..20])`),
    /// so longer args are funding args with the same pubkey hash.
    #[test]
    fn test_parse_funding_lock_args_with_extra_bytes() {
        let mut args = vec![0xBB; 20];
        args.extend_from_slice(&[0xFF; 10]); // extra bytes beyond 20
        let parsed = parse_funding_lock_args(&args).unwrap();
        assert_eq!(parsed.pubkey_hash, &[0xBB; 20]);
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

    #[test]
    fn funding_cell_untyped_needs_only_the_args() {
        let args = [0xAA; 20];
        assert_eq!(
            parse_funding_cell(&args, false, 0).unwrap().pubkey_hash,
            &args
        );
        assert!(parse_funding_cell(&args[..19], false, 0).is_none());
    }

    #[test]
    fn funding_cell_typed_needs_data_that_holds_the_udt_amount() {
        let args = [0xAA; 20];
        assert!(parse_funding_cell(&args, true, FUNDING_UDT_AMOUNT_LEN - 1).is_none());
        assert!(parse_funding_cell(&args, true, 8).is_none());
        assert!(parse_funding_cell(&args, true, 0).is_none());
        assert!(parse_funding_cell(&args, true, FUNDING_UDT_AMOUNT_LEN).is_some());
        assert!(parse_funding_cell(&args, true, 24).is_some());
        assert!(parse_funding_cell(&args[..5], true, FUNDING_UDT_AMOUNT_LEN).is_none());
    }

    // --- parse_commitment_lock_args tests ---

    fn commitment_args_of_len(len: usize) -> Vec<u8> {
        let mut args = vec![0x11; 20];
        args.extend_from_slice(&0xa000_0100_0000_0001u64.to_le_bytes());
        args.extend_from_slice(&1u64.to_be_bytes());
        args.resize(len.max(36), 0x22);
        args.truncate(len);
        args
    }

    #[test]
    fn test_parse_commitment_lock_args_valid() {
        let mut args = Vec::with_capacity(57);
        let pubkey_hash = [0x11; 20];
        args.extend_from_slice(&pubkey_hash);

        let delay_epoch: u64 = 0x2000_0400_0000_0006; // example epoch value
        args.extend_from_slice(&delay_epoch.to_le_bytes());

        let version: u64 = 1;
        args.extend_from_slice(&version.to_be_bytes());

        let settlement_hash = [0x22; 20];
        args.extend_from_slice(&settlement_hash);

        let settlement_flag: u8 = 0x01;
        args.push(settlement_flag);

        assert_eq!(args.len(), 57);

        let parsed = parse_commitment_lock_args(&args).unwrap();
        assert_eq!(parsed.pubkey_hash, &pubkey_hash);
        assert_eq!(parsed.delay_epoch, delay_epoch);
        assert_eq!(parsed.version, version);
        assert_eq!(
            parsed.layout,
            CommitmentLockLayout::Full57 {
                settlement_hash: &settlement_hash,
                settlement_flag,
            }
        );
        assert_eq!(parsed.settlement_hash(), Some(&settlement_hash));
        assert_eq!(parsed.settlement_flag(), Some(settlement_flag));
    }

    #[test]
    fn test_parse_commitment_lock_args_short_format() {
        // 36-byte format: pubkey_hash(20) + delay_epoch(8) + version(8)
        let mut args = Vec::with_capacity(36);
        let pubkey_hash = [0x55; 20];
        args.extend_from_slice(&pubkey_hash);

        let delay_epoch: u64 = 42;
        args.extend_from_slice(&delay_epoch.to_le_bytes());

        let version: u64 = 3;
        args.extend_from_slice(&version.to_be_bytes());

        assert_eq!(args.len(), 36);

        let parsed = parse_commitment_lock_args(&args).unwrap();
        assert_eq!(parsed.pubkey_hash, &pubkey_hash);
        assert_eq!(parsed.delay_epoch, delay_epoch);
        assert_eq!(parsed.version, version);
        assert_eq!(parsed.layout, CommitmentLockLayout::Legacy36);
        assert_eq!(parsed.settlement_hash(), None);
        assert_eq!(parsed.settlement_flag(), None);
    }

    /// The daric locks (mainnet v1-v2) accept 56 bytes: the 36-byte head plus
    /// the blake2b-160 of the pending HTLCs (their ArgsLenError is
    /// `args.len() != 36 && args.len() != 56`).
    #[test]
    fn commitment_args_56_carry_the_pending_htlcs_hash() {
        let mut args = commitment_args_of_len(36);
        args.extend_from_slice(&[0x77; 20]);
        let parsed = parse_commitment_lock_args(&args).unwrap();
        assert_eq!(
            parsed.layout,
            CommitmentLockLayout::Legacy56 {
                pending_htlcs_hash: &[0x77; 20],
            }
        );
        assert_eq!(parsed.settlement_hash(), None);
        assert_eq!(parsed.settlement_flag(), None);
        assert_eq!(parsed.pubkey_hash, &[0x11; 20]);
    }

    /// The 0.7.0 lock's v1 layout: 58 bytes ending in
    /// FEATURE_ONCHAIN_FULL_PAYMENT_HASH; settlement hash and flag as in 57.
    #[test]
    fn commitment_args_58_with_full_payment_hash_feature() {
        let mut args = commitment_args_of_len(36);
        args.extend_from_slice(&[0x33; 20]);
        args.push(0x00);
        args.push(FEATURE_ONCHAIN_FULL_PAYMENT_HASH);
        let parsed = parse_commitment_lock_args(&args).unwrap();
        assert_eq!(
            parsed.layout,
            CommitmentLockLayout::Full58 {
                settlement_hash: &[0x33; 20],
                settlement_flag: 0x00,
            }
        );
        assert_eq!(parsed.settlement_flag(), Some(0x00));
    }

    /// Lengths between the deployed layouts fit no lock (the old
    /// `test_parse_commitment_lock_args_between_short_and_full` accepted 40
    /// bytes as the 36-byte format).
    #[test]
    fn commitment_args_between_layouts_are_rejected() {
        for len in [21, 35, 37, 40, 55] {
            assert!(
                parse_commitment_lock_args(&commitment_args_of_len(len)).is_none(),
                "{len}-byte args fit no deployed commitment-lock layout"
            );
        }
    }

    /// No deployed lock accepts more than 58 bytes (the old
    /// `test_parse_commitment_lock_args_with_extra_bytes` accepted 60 bytes as
    /// the 57-byte format).
    #[test]
    fn commitment_args_longer_than_58_are_rejected() {
        for len in [59, 60, 64] {
            assert!(
                parse_commitment_lock_args(&commitment_args_of_len(len)).is_none(),
                "{len}-byte args fit no deployed commitment-lock layout"
            );
        }
    }

    #[test]
    fn commitment_args_58_need_the_full_payment_hash_feature() {
        for features in [0x00u8, 0x02, 0xff] {
            let mut args = commitment_args_of_len(57);
            args.push(features);
            assert!(
                parse_commitment_lock_args(&args).is_none(),
                "58-byte args with features 0x{features:02x} are rejected by the 0.7.0 lock"
            );
        }
    }

    /// The 20-byte P2WSH commitment of the 2024 testnet deployments is a
    /// different protocol (args = witness-script hash) and is not modelled.
    #[test]
    fn commitment_args_20_byte_p2wsh_is_not_modelled() {
        assert!(parse_commitment_lock_args(&[0x44; 20]).is_none());
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
        assert_eq!(parsed.pubkey_hash, &[0u8; 20]);
        assert_eq!(parsed.delay_epoch, 0);
        assert_eq!(parsed.version, 0);
        assert_eq!(parsed.settlement_hash(), Some(&[0u8; 20]));
        assert_eq!(parsed.settlement_flag(), Some(0));
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

    /// Real mainnet commitment args: the 36-byte force close 0x2bce4f4c…
    /// (block 15676315) and the 58-byte 0.7.0 partial settlement 0x255f83e7…
    /// (block 20967938, channel 0xe1a939e4…).
    #[test]
    fn commitment_args_real_mainnet_vectors() {
        let short = parse_hex_to_bytes(
            "0xac47cad13bef8a2b025abb8726b72bc59d6ffdef06000000000100a00000000000000000",
        );
        let parsed = parse_commitment_lock_args(&short).unwrap();
        assert_eq!(parsed.layout.name(), "legacy36");
        assert_eq!(parsed.delay_epoch, 11_529_216_145_580_097_542);
        assert_eq!(parsed.version, 0);

        let v1 = parse_hex_to_bytes(
            "0x5c494edc86cce3a4d5692364453193e6ed9c695401000000000100a00000000000000002f4bb16e4d4b49bb983db6d5ec191cd860c06712b0101",
        );
        let parsed = parse_commitment_lock_args(&v1).unwrap();
        assert_eq!(parsed.layout.name(), "full58");
        assert_eq!(
            parsed.pubkey_hash.as_slice(),
            parse_hex_to_bytes("0x5c494edc86cce3a4d5692364453193e6ed9c6954")
        );
        assert_eq!(parsed.delay_epoch, 11_529_216_145_580_097_537);
        assert_eq!(parsed.version, 2);
        assert_eq!(
            parsed.settlement_hash().unwrap().as_slice(),
            parse_hex_to_bytes("0xf4bb16e4d4b49bb983db6d5ec191cd860c06712b")
        );
        assert_eq!(parsed.settlement_flag(), Some(0x01));
    }

    #[test]
    fn commitment_layout_names() {
        let head = commitment_args_of_len(36);
        let with = |tail: &[u8]| [head.as_slice(), tail].concat();
        let name = |args: &[u8]| parse_commitment_lock_args(args).unwrap().layout.name();
        assert_eq!(name(&head), "legacy36");
        assert_eq!(name(&with(&[0; 20])), "legacy56");
        assert_eq!(name(&with(&[0; 21])), "full57");
        assert_eq!(name(&with(&[[0; 21].as_slice(), &[1]].concat())), "full58");
    }

    // --- parse_funding_udt_amount tests ---

    /// Real UDT-funded funding cells captured from the testnet node
    /// (2026-08-03). One per UDT kind present on chain, covering every
    /// standard that funds a Fiber channel today.
    #[test]
    fn test_parse_funding_udt_amount_real_funding_cells() {
        // (funding tx, cell data hex, expected amount)
        let vectors: [(&str, &str, u128); 4] = [
            // xUDT 0x25c29dc3… — tx 0x669579d99c12672d589b38e364e13af566b8e113e0d6f239debdd5a76cb5b59b:0
            (
                "0x669579d99c12672d589b38e364e13af566b8e113e0d6f239debdd5a76cb5b59b",
                "0x00d6b45f150000000000000000000000",
                91_800_000_000,
            ),
            // USDI 0xcc9dc33e… — tx 0x4d49307f8d0572947e53bfbf35b06ce9c56a4affa43eef1a3ded311b67e28e4c:0
            (
                "0x4d49307f8d0572947e53bfbf35b06ce9c56a4affa43eef1a3ded311b67e28e4c",
                "0x32ca9a3b000000000000000000000000",
                1_000_000_050,
            ),
            // Stable++ Asset 0x1142755a… — tx 0x6b1f20dd62f84db5c4cd5847163de1b1098d3c9eadd19f2e62d901527b8751bd:0
            (
                "0x6b1f20dd62f84db5c4cd5847163de1b1098d3c9eadd19f2e62d901527b8751bd",
                "0x00e40b54020000000000000000000000",
                10_000_000_000,
            ),
            // sUDT 0xc5e5dcf2… — tx 0xb137cc910b6b44033eb03bfaec451a8d428d811c7ed8dc211564dd47f56d909f:0
            (
                "0xb137cc910b6b44033eb03bfaec451a8d428d811c7ed8dc211564dd47f56d909f",
                "0x29000000000000000000000000000000",
                41,
            ),
        ];

        for (funding_tx, data_hex, expected) in vectors {
            let data = parse_hex_to_bytes(data_hex);
            assert_eq!(data.len(), FUNDING_UDT_AMOUNT_LEN);
            assert_eq!(
                parse_funding_udt_amount(&data),
                Some(expected),
                "amount mismatch for funding cell {funding_tx}"
            );
        }
    }

    /// A CKB-only funding cell carries empty data — there is no amount to read.
    /// Vector: testnet tx 0x4400f63c…:0 (`outputs_data[0] == "0x"`).
    #[test]
    fn test_parse_funding_udt_amount_ckb_only_funding_cell_is_none() {
        assert_eq!(parse_funding_udt_amount(&[]), None);
    }

    #[test]
    fn test_parse_funding_udt_amount_short_data_is_none() {
        assert_eq!(parse_funding_udt_amount(&[0u8; 15]), None);
    }

    #[test]
    fn test_parse_funding_udt_amount_ignores_trailing_extension_data() {
        // xUDT cells may carry extension data after the amount prefix.
        let mut data = 12_345u128.to_le_bytes().to_vec();
        data.extend_from_slice(&[0xAB; 8]);
        assert_eq!(parse_funding_udt_amount(&data), Some(12_345));
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
