use anyhow::{anyhow, bail};

pub const SHANNON: u64 = 100_000_000;

/// DAO cell occupied capacity: 102 CKB in shannons.
/// Every DAO deposit cell has this minimum storage cost.
pub const DAO_OCCUPIED_CAPACITY: u64 = 102_00000000;

/// Return the interest-bearing free capacity of `cell_count` DAO cells.
///
/// DAO compensation accrues only to `capacity - occupied_capacity`; passing
/// full deposit capacity into the secondary-issuance split overstates DAO
/// compensation, especially on networks with many small deposits.
pub fn dao_total_free_capacity(total_capacity: i128, cell_count: i64) -> anyhow::Result<i128> {
    if total_capacity < 0 || cell_count < 0 {
        bail!(
            "negative DAO aggregate while computing free capacity: total_capacity={}, cell_count={}",
            total_capacity,
            cell_count
        );
    }
    let occupied = i128::from(cell_count)
        .checked_mul(i128::from(DAO_OCCUPIED_CAPACITY))
        .ok_or_else(|| anyhow!("DAO occupied capacity multiplication overflow"))?;
    if total_capacity < occupied {
        bail!(
            "DAO aggregate capacity below occupied capacity: total_capacity={}, cell_count={}, occupied={}",
            total_capacity,
            cell_count,
            occupied
        );
    }
    total_capacity.checked_sub(occupied).ok_or_else(|| {
        anyhow!(
            "DAO free capacity subtraction overflow: total_capacity={}, cell_count={}, occupied={}",
            total_capacity,
            cell_count,
            occupied
        )
    })
}

/// Convert a signed change in DAO cell capacity/count into the matching signed
/// change in interest-bearing free capacity.
pub fn dao_free_capacity_delta(
    capacity_delta: i128,
    cell_count_delta: i64,
) -> anyhow::Result<i128> {
    let occupied_delta = i128::from(cell_count_delta)
        .checked_mul(i128::from(DAO_OCCUPIED_CAPACITY))
        .ok_or_else(|| anyhow!("DAO occupied capacity delta multiplication overflow"))?;
    capacity_delta.checked_sub(occupied_delta).ok_or_else(|| {
        anyhow!(
            "DAO free capacity delta overflow: capacity_delta={}, cell_count_delta={}, occupied_delta={}",
            capacity_delta,
            cell_count_delta,
            occupied_delta
        )
    })
}

/// Calculate DAO compensation from accumulated rate values.
///
/// Formula (per RFC-0023): `free_capacity * ar_withdraw / ar_deposit - free_capacity`
/// where `free_capacity = capacity - DAO_OCCUPIED_CAPACITY`.
pub fn calculate_dao_compensation_from_ar(
    capacity: i64,
    ar_deposit: u64,
    ar_withdraw: u64,
) -> anyhow::Result<i64> {
    if ar_deposit == 0 {
        bail!(
            "invalid zero deposit AR while calculating DAO compensation: capacity={}, ar_deposit={}, ar_withdraw={}",
            capacity,
            ar_deposit,
            ar_withdraw
        );
    }

    let free_capacity = u128::try_from(dao_total_free_capacity(i128::from(capacity), 1)?)
        .map_err(|_| anyhow!("DAO free capacity is negative: capacity={}", capacity))?;
    let gross = free_capacity
        .checked_mul(ar_withdraw as u128)
        .ok_or_else(|| anyhow!("DAO compensation multiply overflow"))?
        / (ar_deposit as u128);
    let compensation_u128 = gross.checked_sub(free_capacity).ok_or_else(|| {
        anyhow!(
            "DAO compensation underflow: free_capacity={}, ar_deposit={}, ar_withdraw={}",
            free_capacity,
            ar_deposit,
            ar_withdraw
        )
    })?;
    i64::try_from(compensation_u128)
        .map_err(|_| anyhow!("DAO compensation exceeds i64: {}", compensation_u128))
}

/// Split an exact per-block change in the DAO header's non-miner secondary
/// pool into `(miner, dao, treasury)` components.
///
/// A negative delta is an on-chain protocol correction, not negative issuance.
/// It is assigned entirely to treasury so miner and DAO compensation remain
/// monotonic while `dao + treasury` still telescopes to the exact S-field
/// change across an upgrade boundary.
pub fn split_secondary_issuance_delta(
    total_issuance: i128,
    occupied_capacity: i128,
    total_deposited_free_capacity: i128,
    non_miner_secondary_delta: i128,
) -> anyhow::Result<(i128, i128, i128)> {
    if total_issuance < 0 || occupied_capacity < 0 || total_deposited_free_capacity < 0 {
        bail!(
            "negative input in secondary issuance split: total_issuance={}, occupied_capacity={}, total_deposited={}, non_miner_secondary_delta={}",
            total_issuance,
            occupied_capacity,
            total_deposited_free_capacity,
            non_miner_secondary_delta
        );
    }
    if total_issuance <= occupied_capacity {
        bail!(
            "invalid DAO C/U relationship: total_issuance={}, occupied_capacity={}, non_miner_secondary_delta={}",
            total_issuance,
            occupied_capacity,
            non_miner_secondary_delta
        );
    }

    let liquid_supply = total_issuance - occupied_capacity;
    if total_deposited_free_capacity > liquid_supply {
        bail!(
            "dao deposited exceeds liquid supply: total_deposited={}, liquid_supply={}, total_issuance={}, occupied_capacity={}",
            total_deposited_free_capacity,
            liquid_supply,
            total_issuance,
            occupied_capacity
        );
    }

    if non_miner_secondary_delta < 0 {
        return Ok((0, 0, non_miner_secondary_delta));
    }
    if non_miner_secondary_delta == 0 {
        return Ok((0, 0, 0));
    }

    let miner = non_miner_secondary_delta
        .checked_mul(occupied_capacity)
        .ok_or_else(|| anyhow!("secondary issuance miner multiplication overflow"))?
        / liquid_supply;
    let dao = non_miner_secondary_delta
        .checked_mul(total_deposited_free_capacity)
        .ok_or_else(|| anyhow!("secondary issuance DAO multiplication overflow"))?
        / liquid_supply;
    let treasury = non_miner_secondary_delta
        .checked_sub(dao)
        .ok_or_else(|| anyhow!("secondary issuance treasury subtraction overflow"))?;

    if miner < 0 || dao < 0 || treasury < 0 {
        bail!(
            "secondary issuance split produced negative component: miner={}, dao={}, treasury={}, non_miner_secondary_delta={}",
            miner,
            dao,
            treasury,
            non_miner_secondary_delta
        );
    }
    Ok((miner, dao, treasury))
}

/// Extract the S field (secondary pool) from a 32-byte DAO header as u64.
pub fn extract_s_from_dao(dao: &[u8]) -> Option<u64> {
    if dao.len() < 24 {
        return None;
    }
    let bytes: [u8; 8] = dao[16..24].try_into().ok()?;
    Some(u64::from_le_bytes(bytes))
}

/// Satoshi's pubkey hash from Bitcoin genesis block coinbase.
/// Used as lock_args for the 8.4B burnt cell in CKB genesis.
pub const SATOSHI_PUBKEY_HASH: [u8; 20] = [
    0x62, 0xe9, 0x07, 0xb1, 0x5c, 0xbf, 0x27, 0xd5, 0x42, 0x53, 0x99, 0xeb, 0xf6, 0xf0, 0xfb, 0x50,
    0xeb, 0xb8, 0x8f, 0x18,
];

// ---------------------------------------------------------------------------
// APC model constants (CKB Explorer-compatible)
// ---------------------------------------------------------------------------

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
///
/// `genesis_issuance` is the exact genesis block total issuance in shannons
/// (the DAO `C` field of block 0), derived per-network from the persisted
/// `GenesisBaseline`. It seeds the theoretical cumulative primary issuance so
/// mainnet and testnet share one calculation path with a network-correct base
/// instead of a hardcoded 33.6B approximation.
pub fn calculate_estimated_apc(
    epoch_number: i64,
    epoch_index: i32,
    epoch_length: i32,
    genesis_issuance: i128,
) -> f64 {
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
        let ti = apc_theoretical_total_issuance(w[0] as i64, norm_idx, norm_len, genesis_issuance);

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
fn apc_theoretical_total_issuance(
    epoch_number: i64,
    epoch_index: f64,
    epoch_length: f64,
    genesis_issuance: i128,
) -> f64 {
    apc_theoretical_primary(epoch_number, genesis_issuance)
        + apc_theoretical_secondary(epoch_number, epoch_index, epoch_length)
}

/// Cumulative primary issuance: genesis + sum of completed halving periods + partial current period.
fn apc_theoretical_primary(epoch_number: i64, genesis_issuance: i128) -> f64 {
    let periods = (epoch_number as f64 / EPOCHS_PER_HALVING).floor() as i32;
    let mut cumulative = genesis_issuance as f64;
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

    /// Exact mainnet genesis total issuance (DAO `C` field of block 0), in
    /// shannons — 33.6B CKB + 145238488200 shannons of genesis rounding, NOT
    /// the 33.6B approximation previously hardcoded here.
    const MAINNET_GENESIS_ISSUANCE: i128 = 3_360_000_145_238_488_200;

    #[test]
    fn secondary_issuance_negative_s_delta_is_absorbed_by_treasury() {
        assert_eq!(
            split_secondary_issuance_delta(1_000, 100, 200, -30).unwrap(),
            (0, 0, -30)
        );
    }

    #[test]
    fn dao_free_capacity_excludes_occupied_capacity_per_cell() {
        assert_eq!(
            dao_total_free_capacity(1_000 * i128::from(SHANNON), 2).unwrap(),
            796 * i128::from(SHANNON)
        );
    }

    #[test]
    fn dao_free_capacity_delta_handles_withdrawal() {
        assert_eq!(
            dao_free_capacity_delta(-500 * i128::from(SHANNON), -1).unwrap(),
            -398 * i128::from(SHANNON)
        );
    }

    #[test]
    fn dao_free_capacity_rejects_capacity_below_occupied() {
        let error = dao_total_free_capacity(100 * i128::from(SHANNON), 1).unwrap_err();
        assert!(error.to_string().contains("below occupied capacity"));
    }

    #[test]
    fn secondary_issuance_split_is_exact_for_positive_delta() {
        assert_eq!(
            split_secondary_issuance_delta(1_000, 100, 200, 90).unwrap(),
            (10, 20, 70)
        );
    }

    #[test]
    fn secondary_issuance_split_rejects_invalid_chain_state() {
        let err = split_secondary_issuance_delta(1_000, 900, 200, 10).unwrap_err();
        assert!(err.to_string().contains("exceeds liquid supply"));
    }

    #[test]
    fn test_apc_at_genesis() {
        // Epoch 0, index 0, length 1800
        let apc = calculate_estimated_apc(0, 0, 1800, MAINNET_GENESIS_ISSUANCE);
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
        let apc = calculate_estimated_apc(13900, 500, 1800, MAINNET_GENESIS_ISSUANCE);
        // Explorer shows ~2.05% for current mainnet state
        assert!(
            (apc - 2.05).abs() < 0.1,
            "Expected ~2.05% in 2nd halving, got {:.4}%",
            apc
        );
    }

    #[test]
    fn test_apc_decreases_over_time() {
        let apc_early = calculate_estimated_apc(100, 0, 1800, MAINNET_GENESIS_ISSUANCE);
        let apc_mid = calculate_estimated_apc(5000, 0, 1800, MAINNET_GENESIS_ISSUANCE);
        let apc_late = calculate_estimated_apc(13000, 0, 1800, MAINNET_GENESIS_ISSUANCE);
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
        assert_eq!(
            calculate_estimated_apc(100, 0, 0, MAINNET_GENESIS_ISSUANCE),
            0.0
        );
    }

    #[test]
    fn test_apc_negative_epoch_returns_zero() {
        assert_eq!(
            calculate_estimated_apc(-1, 0, 1800, MAINNET_GENESIS_ISSUANCE),
            0.0
        );
    }

    #[test]
    fn test_apc_spans_halving_boundary() {
        // Deposit window from epoch 7600..9790 spans 1st→2nd halving at 8760
        let apc = calculate_estimated_apc(7600, 0, 1800, MAINNET_GENESIS_ISSUANCE);
        // Should be between pure 1st-halving and pure 2nd-halving APC
        let apc_pure_first = calculate_estimated_apc(5000, 0, 1800, MAINNET_GENESIS_ISSUANCE);
        let apc_pure_second = calculate_estimated_apc(10000, 0, 1800, MAINNET_GENESIS_ISSUANCE);
        assert!(
            apc < apc_pure_first && apc > apc_pure_second,
            "Cross-boundary APC ({:.4}) should be between 1st ({:.4}) and 2nd ({:.4})",
            apc,
            apc_pure_first,
            apc_pure_second
        );
    }

    #[test]
    fn test_apc_exact_genesis_base_is_finite_positive() {
        // Regression: the model must run off the exact genesis DAO `C` (not the
        // old 33.6B literal) and still produce a finite, positive APC.
        let apc = calculate_estimated_apc(0, 0, 1800, MAINNET_GENESIS_ISSUANCE);
        assert!(
            apc.is_finite() && apc > 0.0,
            "expected finite positive APC, got {apc}"
        );
    }

    #[test]
    fn test_apc_varies_with_genesis_issuance() {
        // Proves the genesis_issuance param is wired into the model (not ignored):
        // a larger cumulative base dilutes the secondary-issuance rate → lower APC.
        let exact = calculate_estimated_apc(0, 0, 1800, MAINNET_GENESIS_ISSUANCE);
        let doubled = calculate_estimated_apc(0, 0, 1800, MAINNET_GENESIS_ISSUANCE * 2);
        assert_ne!(
            exact, doubled,
            "genesis_issuance must change APC output (exact {exact} vs doubled {doubled})"
        );
        assert!(
            doubled < exact,
            "larger genesis issuance should dilute APC: doubled {doubled} < exact {exact}"
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
}
