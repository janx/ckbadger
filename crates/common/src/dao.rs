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

// ---------------------------------------------------------------------------
// APC model constants (CKB Explorer-compatible)
// ---------------------------------------------------------------------------

/// Genesis block total issuance: 33.6B CKB in shannons.
const GENESIS_ISSUANCE: f64 = 3_360_000_000_000_000_000.0;

/// Base primary issuance: 4.2B CKB/year in shannons. Halves every 4 years.
const PRIMARY_PER_YEAR_BASE: f64 = 420_000_000_000_000_000.0;

/// Secondary issuance: 1.344B CKB/year in shannons (constant).
const SECONDARY_PER_YEAR: f64 = 134_400_000_000_000_000.0;

/// Epochs per natural year.
const EPOCHS_PER_YEAR: f64 = 2190.0;

/// Epochs per halving period (4 years).
const EPOCHS_PER_HALVING: f64 = 8760.0;

/// Secondary issuance per epoch in shannons.
const SECONDARY_PER_EPOCH: f64 = SECONDARY_PER_YEAR / EPOCHS_PER_YEAR;

/// Estimate APC using the CKB Explorer's continuous-compounding model.
///
/// Models a hypothetical 1-year deposit starting at the given epoch,
/// compounding across halving boundaries if necessary.
///
/// Uses `rate = ln(1 + (alpha+1) * sn / C) / (alpha+1)` where alpha is the
/// ratio of primary to secondary issuance per epoch for the current halving
/// period, sn is secondary issuance over the segment, and C is the
/// theoretical cumulative total issuance.
pub fn calculate_estimated_apc(epoch_number: i64, epoch_index: i32, epoch_length: i32) -> f64 {
    if epoch_length == 0 || epoch_number < 0 {
        return 0.0;
    }

    let start = epoch_number as f64;
    let end = start + EPOCHS_PER_YEAR - 1.0;

    // Normalize epoch_index to reference length 1800 (matches explorer).
    let norm_idx = (epoch_index as f64) * 1800.0 / (epoch_length as f64);
    let norm_len = 1800.0_f64;

    // Find halving-boundary checkpoints within [start, end].
    let cp_start = ((start + 1.0) / EPOCHS_PER_HALVING).ceil() * EPOCHS_PER_HALVING;
    let cp_end = ((end + 1.0) / EPOCHS_PER_HALVING).floor() * EPOCHS_PER_HALVING;

    let mut checkpoints: Vec<f64> = Vec::new();
    if cp_start <= cp_end {
        let count = ((cp_end - cp_start) / EPOCHS_PER_HALVING + 1.0) as usize;
        for i in 0..count {
            checkpoints.push(i as f64 * EPOCHS_PER_HALVING + cp_start - 1.0);
        }
    }
    if checkpoints.is_empty() || checkpoints[0] > start {
        checkpoints.insert(0, start);
    }
    if checkpoints.last().copied().unwrap_or(f64::NEG_INFINITY) < end {
        checkpoints.push(end);
    }

    // Compound rate across segments between halving boundaries.
    let mut compound = 1.0_f64;
    for w in checkpoints.windows(2) {
        let seg_start_frac = w[0] + norm_idx / norm_len;
        let seg_end_frac = w[1] + norm_idx / norm_len;
        let sn = SECONDARY_PER_EPOCH * (seg_end_frac - seg_start_frac);

        let alpha = apc_alpha(w[0] as i64);
        let ti = apc_theoretical_total_issuance(w[0] as i64, norm_idx, norm_len);

        if ti > 0.0 {
            let rate = ((alpha + 1.0) * sn / ti + 1.0).ln() / (alpha + 1.0);
            compound *= 1.0 + rate;
        }
    }

    let rate = compound - 1.0;
    // Truncate to 4 decimal places (matching explorer).
    (rate * 100.0 * 10000.0).floor() / 10000.0
}

/// Alpha = primary_per_epoch / secondary_per_epoch for the halving period
/// containing `epoch_number`.
fn apc_alpha(epoch_number: i64) -> f64 {
    let period = ((epoch_number + 1) as f64 / EPOCHS_PER_HALVING).floor() as i32;
    let primary_per_epoch = PRIMARY_PER_YEAR_BASE / 2_f64.powi(period) / EPOCHS_PER_YEAR;
    primary_per_epoch / SECONDARY_PER_EPOCH
}

/// Theoretical cumulative total issuance at a given epoch (primary + secondary).
fn apc_theoretical_total_issuance(epoch_number: i64, epoch_index: f64, epoch_length: f64) -> f64 {
    apc_theoretical_primary(epoch_number)
        + apc_theoretical_secondary(epoch_number, epoch_index, epoch_length)
}

/// Cumulative primary issuance: genesis + sum of completed halving periods + partial current period.
fn apc_theoretical_primary(epoch_number: i64) -> f64 {
    let periods = (epoch_number as f64 / EPOCHS_PER_HALVING).floor() as i32;
    let mut cumulative = GENESIS_ISSUANCE;
    for i in 0..periods {
        cumulative += PRIMARY_PER_YEAR_BASE * 4.0 / 2_f64.powi(i);
    }
    let remaining = epoch_number as f64 + 1.0 - periods as f64 * EPOCHS_PER_HALVING;
    cumulative += PRIMARY_PER_YEAR_BASE * remaining / EPOCHS_PER_YEAR / 2_f64.powi(periods);
    cumulative
}

/// Cumulative secondary issuance up to the given fractional epoch.
fn apc_theoretical_secondary(epoch_number: i64, epoch_index: f64, epoch_length: f64) -> f64 {
    let frac = epoch_number as f64 + epoch_index / epoch_length;
    let epochs = if frac > 0.0 { frac + 1.0 } else { frac };
    epochs * SECONDARY_PER_EPOCH
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_apc_at_genesis() {
        // Epoch 0, index 0, length 1800
        let apc = calculate_estimated_apc(0, 0, 1800);
        // At genesis: alpha=3.125 (1st halving), total_issuance≈33.6B
        // Model gives ~3.70% (lower than the naive 1.344B/25.2B=5.33%
        // because the model accounts for primary issuance dilution via alpha)
        assert!(
            apc > 3.0 && apc < 4.5,
            "Expected ~3.7% at genesis, got {:.4}%",
            apc
        );
    }

    #[test]
    fn test_apc_second_halving() {
        // ~6.3 years in: epoch ≈ 13900, 2nd halving period
        // alpha = 1.5625, theoretical total_issuance ≈ 63.9B
        let apc = calculate_estimated_apc(13900, 500, 1800);
        // Explorer shows ~2.05% for current mainnet state
        assert!(
            (apc - 2.05).abs() < 0.1,
            "Expected ~2.05% in 2nd halving, got {:.4}%",
            apc
        );
    }

    #[test]
    fn test_apc_decreases_over_time() {
        let apc_early = calculate_estimated_apc(100, 0, 1800);
        let apc_mid = calculate_estimated_apc(5000, 0, 1800);
        let apc_late = calculate_estimated_apc(13000, 0, 1800);
        assert!(
            apc_early > apc_mid && apc_mid > apc_late,
            "APC should decrease over time: {:.4} > {:.4} > {:.4}",
            apc_early,
            apc_mid,
            apc_late
        );
    }

    #[test]
    fn test_apc_zero_epoch_length_returns_zero() {
        assert_eq!(calculate_estimated_apc(100, 0, 0), 0.0);
    }

    #[test]
    fn test_apc_negative_epoch_returns_zero() {
        assert_eq!(calculate_estimated_apc(-1, 0, 1800), 0.0);
    }

    #[test]
    fn test_apc_spans_halving_boundary() {
        // Deposit window from epoch 7600..9790 spans 1st→2nd halving at 8760
        let apc = calculate_estimated_apc(7600, 0, 1800);
        // Should be between pure 1st-halving and pure 2nd-halving APC
        let apc_pure_first = calculate_estimated_apc(5000, 0, 1800);
        let apc_pure_second = calculate_estimated_apc(10000, 0, 1800);
        assert!(
            apc < apc_pure_first && apc > apc_pure_second,
            "Cross-boundary APC ({:.4}) should be between 1st ({:.4}) and 2nd ({:.4})",
            apc,
            apc_pure_first,
            apc_pure_second
        );
    }

    #[test]
    fn test_apc_alpha_first_halving() {
        // 1st halving: primary=4.2B/year, secondary=1.344B/year
        let alpha = apc_alpha(0);
        assert!(
            (alpha - 3.125).abs() < 0.001,
            "Expected alpha=3.125 for 1st halving, got {}",
            alpha
        );
    }

    #[test]
    fn test_apc_alpha_second_halving() {
        // 2nd halving: primary=2.1B/year, secondary=1.344B/year
        let alpha = apc_alpha(9000);
        assert!(
            (alpha - 1.5625).abs() < 0.001,
            "Expected alpha=1.5625 for 2nd halving, got {}",
            alpha
        );
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
}
