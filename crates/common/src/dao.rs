pub const SHANNON: u64 = 100_000_000;

/// 8.4B CKB burnt at genesis (in shannons).
/// This is 25% of the 33.6B genesis issuance, split as:
/// - 5.04B (15%) hard-coded as "occupied" capacity (ensures miner rewards)
/// - 3.36B (10%) hard-coded as "liquid" (ensures treasury/burnt portion)
///
/// See: <https://medium.com/nervosnetwork/nervos-ckbyte-distribution-and-why-we-are-burning-25-in-the-genesis-block-9a7ddf7f6779>
pub const GENESIS_BURNT: u128 = 840_000_000_000_000_000;
pub const SECONDARY_ISSUANCE_PER_YEAR: u64 = 134_400_000_000_000_000;

/// Satoshi's pubkey hash from Bitcoin genesis block coinbase.
/// Used as lock_args for the 8.4B burnt cell in CKB genesis.
pub const SATOSHI_PUBKEY_HASH: [u8; 20] = [
    0x62, 0xe9, 0x07, 0xb1, 0x5c, 0xbf, 0x27, 0xd5, 0x42, 0x53, 0x99, 0xeb, 0xf6, 0xf0, 0xfb, 0x50,
    0xeb, 0xb8, 0x8f, 0x18,
];

/// 60% of the burnt 8.4B is treated as "occupied" for secondary issuance calculation.
pub const GENESIS_SPECIAL_BURN_CELL_OCCUPIED_RATIO_NUM: u64 = 6;
pub const GENESIS_SPECIAL_BURN_CELL_OCCUPIED_RATIO_DENOM: u64 = 10;

/// Virtual occupied capacity of the Satoshi cell: 5.04B CKB in shannons.
/// = 8.4B * 60% = 5.04B
pub const GENESIS_SPECIAL_BURN_CELL_VIRTUAL_OCCUPIED: u128 = 504_000_000_000_000_000;

/// Check if a cell is the genesis special burn cell (8.4B burnt CKB).
/// Must match both the Satoshi pubkey hash AND be from genesis block.
pub fn is_genesis_special_burn_cell(lock_args: &[u8], created_at_block: i64) -> bool {
    lock_args == SATOSHI_PUBKEY_HASH && created_at_block == 0
}

pub fn calculate_estimated_apc(total_issuance: u64, secondary_burnt: u128) -> f64 {
    let total = total_issuance as u128;
    assert!(
        total >= GENESIS_BURNT,
        "total_issuance ({total}) < GENESIS_BURNT ({GENESIS_BURNT}): corrupt DAO data"
    );
    let after_genesis = total - GENESIS_BURNT;
    assert!(
        after_genesis >= secondary_burnt,
        "circulating underflow: total_issuance={total}, genesis_burnt={GENESIS_BURNT}, secondary_burnt={secondary_burnt}"
    );
    let circulating = after_genesis - secondary_burnt;

    if circulating > 0 {
        (SECONDARY_ISSUANCE_PER_YEAR as f64 / circulating as f64) * 100.0
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_apc_at_genesis() {
        let total_issuance: u64 = 33_600_000_000 * SHANNON;
        let secondary_burnt: u128 = 0;

        let apc = calculate_estimated_apc(total_issuance, secondary_burnt);

        // circulating = 33.6B - 8.4B = 25.2B
        // APC = 1.344B / 25.2B * 100 = 5.33%
        assert!(
            (apc - 5.33).abs() < 0.01,
            "Expected ~5.33%, got {:.2}%",
            apc
        );
    }

    #[test]
    fn test_apc_with_secondary_burnt() {
        let total_issuance: u64 = 40_000_000_000 * SHANNON;
        let secondary_burnt: u128 = 1_000_000_000 * SHANNON as u128;

        let apc = calculate_estimated_apc(total_issuance, secondary_burnt);

        // circulating = 40B - 8.4B - 1B = 30.6B
        // APC = 1.344B / 30.6B * 100 = 4.39%
        assert!(
            (apc - 4.39).abs() < 0.01,
            "Expected ~4.39%, got {:.2}%",
            apc
        );
    }

    #[test]
    #[should_panic(expected = "total_issuance (0) < GENESIS_BURNT")]
    fn test_apc_zero_issuance_panics() {
        calculate_estimated_apc(0, 0);
    }

    #[test]
    fn test_is_genesis_special_burn_cell_matches() {
        assert!(is_genesis_special_burn_cell(&SATOSHI_PUBKEY_HASH, 0));
    }

    #[test]
    fn test_is_genesis_special_burn_cell_rejects_wrong_hash() {
        assert!(!is_genesis_special_burn_cell(&[0u8; 20], 0));
        assert!(!is_genesis_special_burn_cell(&[0xff; 20], 0));
    }

    #[test]
    fn test_is_genesis_special_burn_cell_rejects_non_genesis_block() {
        assert!(!is_genesis_special_burn_cell(&SATOSHI_PUBKEY_HASH, 1));
        assert!(!is_genesis_special_burn_cell(&SATOSHI_PUBKEY_HASH, 2983824));
    }

    #[test]
    fn test_is_genesis_special_burn_cell_rejects_wrong_length() {
        assert!(!is_genesis_special_burn_cell(&SATOSHI_PUBKEY_HASH[..19], 0));
        let mut longer = [0u8; 21];
        longer[..20].copy_from_slice(&SATOSHI_PUBKEY_HASH);
        assert!(!is_genesis_special_burn_cell(&longer, 0));
    }

    #[test]
    fn test_genesis_special_burn_cell_virtual_occupied_is_60_percent() {
        // 8.4B * 60% = 5.04B
        let expected = (GENESIS_BURNT as f64 * 0.6) as u128;
        assert_eq!(GENESIS_SPECIAL_BURN_CELL_VIRTUAL_OCCUPIED, expected);
    }

    #[test]
    fn test_apc_uses_circulating_not_total_issuance() {
        let total_issuance: u64 = 33_600_000_000 * SHANNON;

        // Wrong: using total_issuance directly would give ~4.0%
        let wrong_apc = (SECONDARY_ISSUANCE_PER_YEAR as f64 / total_issuance as f64) * 100.0;
        assert!((wrong_apc - 4.0).abs() < 0.01);

        // Correct: using circulating supply gives ~5.33%
        let correct_apc = calculate_estimated_apc(total_issuance, 0);
        assert!((correct_apc - 5.33).abs() < 0.01);

        assert!(correct_apc > wrong_apc + 1.0);
    }
}
