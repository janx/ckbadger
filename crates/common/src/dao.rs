use anyhow::{anyhow, bail};

pub const SHANNON: u64 = 100_000_000;

/// Standard secp256k1 DAO cell occupied capacity: 102 CKB in shannons.
///
/// Indexed deposits persist their exact occupied capacity; this constant is
/// only for contexts such as the public calculator that do not receive scripts.
pub const DAO_OCCUPIED_CAPACITY: u64 = 102_00000000;

/// Compute a cell's exact occupied capacity from its serialized fields.
pub fn occupied_capacity_shannons(
    data_size: usize,
    lock_args_len: usize,
    type_args_len: Option<usize>,
) -> anyhow::Result<i64> {
    let lock_script_size = 33_i128
        .checked_add(i128::try_from(lock_args_len)?)
        .ok_or_else(|| anyhow!("lock script size overflow"))?;
    let type_script_size = match type_args_len {
        Some(len) => 33_i128
            .checked_add(i128::try_from(len)?)
            .ok_or_else(|| anyhow!("type script size overflow"))?,
        None => 0,
    };
    let bytes = 8_i128
        .checked_add(lock_script_size)
        .and_then(|value| value.checked_add(type_script_size))
        .and_then(|value| value.checked_add(i128::try_from(data_size).ok()?))
        .ok_or_else(|| anyhow!("occupied capacity byte-size overflow"))?;
    let occupied = bytes
        .checked_mul(i128::from(SHANNON))
        .ok_or_else(|| anyhow!("occupied capacity multiplication overflow"))?;
    i64::try_from(occupied).map_err(|_| anyhow!("occupied capacity exceeds i64: {}", occupied))
}

/// Calculate DAO compensation from accumulated rate values.
///
/// Formula (per RFC-0023): `free_capacity * ar_withdraw / ar_deposit - free_capacity`
/// where `free_capacity = capacity - occupied_capacity`.
pub fn calculate_dao_compensation_from_ar(
    capacity: i64,
    occupied_capacity: i64,
    ar_deposit: u64,
    ar_withdraw: u64,
) -> anyhow::Result<i64> {
    if capacity < 0 || occupied_capacity < 0 {
        bail!(
            "negative DAO capacity while calculating compensation: capacity={}, occupied_capacity={}",
            capacity,
            occupied_capacity
        );
    }
    if occupied_capacity > capacity {
        bail!(
            "DAO cell capacity below occupied capacity: capacity={}, occupied_capacity={}",
            capacity,
            occupied_capacity
        );
    }
    if ar_deposit == 0 {
        bail!(
            "invalid zero deposit AR while calculating DAO compensation: capacity={}, occupied_capacity={}, ar_deposit={}, ar_withdraw={}",
            capacity,
            occupied_capacity,
            ar_deposit,
            ar_withdraw
        );
    }

    let free_capacity = u128::try_from(
        capacity
            .checked_sub(occupied_capacity)
            .ok_or_else(|| anyhow!("DAO free-capacity subtraction overflow"))?,
    )
    .map_err(|_| {
        anyhow!(
            "DAO free capacity is negative: capacity={}, occupied_capacity={}",
            capacity,
            occupied_capacity
        )
    })?;
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

/// Per-block total secondary issuance `s_i` from the epoch schedule.
///
/// The consensus constant `secondary_epoch_reward` (from the node's
/// `get_consensus`) is divided evenly over the epoch; the division remainder
/// is distributed as +1 shannon to the first
/// `secondary_epoch_reward % epoch_length` blocks of the epoch — the same
/// rule CKB applies to the primary epoch reward. A block's position within
/// its epoch (`epoch_index`) and the epoch length both come from the block
/// header's packed epoch field, so `s_i` derives purely from the header plus
/// this one consensus constant.
pub fn secondary_block_issuance(
    epoch_index: i64,
    epoch_length: i64,
    secondary_epoch_reward: u64,
) -> anyhow::Result<u64> {
    if epoch_length <= 0 || epoch_index < 0 || epoch_index >= epoch_length {
        bail!(
            "invalid epoch position while deriving secondary block issuance: epoch_index={}, epoch_length={}, secondary_epoch_reward={}",
            epoch_index,
            epoch_length,
            secondary_epoch_reward
        );
    }
    let length = epoch_length as u64;
    let base = secondary_epoch_reward / length;
    let remainder = secondary_epoch_reward % length;
    if (epoch_index as u64) < remainder {
        Ok(base + 1)
    } else {
        Ok(base)
    }
}

/// Exact miner portion of block `i`'s secondary issuance, per RFC-0023:
/// `floor(s_i * U_{i-1} / C_{i-1})`, where `s_i` is the block's total
/// secondary issuance from the epoch schedule and `U`/`C` are the occupied
/// capacity and total issuance from the PARENT block's DAO header field.
///
/// This is the protocol's own direct split — the same value the node reports
/// as `miner_reward.secondary` in `get_block_economic_state`. It must never
/// be reconstructed from the S-pool delta plus claimed compensation: that
/// reconstruction couples the mining series to DAO-claim recognition and
/// carries an inherent flooring drift.
///
/// The genesis block has no parent state to split against — its own
/// secondary share `s_0` enters the genesis `S` pool in full and the node
/// defines no miner reward for block 0 — so callers handle block 0
/// explicitly instead of calling this.
pub fn calculate_miner_secondary_issuance(
    secondary_issuance: u64,
    parent_total_issuance: i128,
    parent_occupied_capacity: i128,
) -> anyhow::Result<i128> {
    if parent_total_issuance <= 0
        || parent_occupied_capacity < 0
        || parent_occupied_capacity >= parent_total_issuance
    {
        bail!(
            "invalid parent DAO C/U relationship while splitting secondary issuance: parent_total_issuance={}, parent_occupied_capacity={}, secondary_issuance={}",
            parent_total_issuance,
            parent_occupied_capacity,
            secondary_issuance
        );
    }

    let miner = (secondary_issuance as i128)
        .checked_mul(parent_occupied_capacity)
        .ok_or_else(|| {
            anyhow!(
                "secondary issuance miner multiplication overflow: secondary_issuance={}, parent_occupied_capacity={}",
                secondary_issuance,
                parent_occupied_capacity
            )
        })?
        / parent_total_issuance;
    Ok(miner)
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

    /// Consensus `secondary_epoch_reward` shared by mainnet and testnet
    /// (verified against both nodes' `get_consensus`): 61,369,863,013,698
    /// shannons per epoch (1.344B CKB/year over 2190 epochs).
    const SECONDARY_EPOCH_REWARD: u64 = 61_369_863_013_698;

    // -- secondary_block_issuance (real mainnet vectors) --------------------
    //
    // Every expected value below was captured from a mainnet node:
    // `issuance.secondary` of `get_block_economic_state`, cross-checked
    // against the header's packed epoch field.

    #[test]
    fn secondary_block_issuance_genesis_epoch_block_1() {
        // Block 1: epoch 0, index 1, length 1743. 61369863013698 / 1743 =
        // 35209330472 rem 1002 → +1 shannon for the first 1002 blocks.
        assert_eq!(
            secondary_block_issuance(1, 1743, SECONDARY_EPOCH_REWARD).unwrap(),
            35_209_330_473
        );
    }

    #[test]
    fn secondary_block_issuance_epoch_977_block_1600000() {
        // Block 1600000: epoch 977, index 202, length 1797.
        assert_eq!(
            secondary_block_issuance(202, 1797, SECONDARY_EPOCH_REWARD).unwrap(),
            34_151_287_153
        );
    }

    #[test]
    fn secondary_block_issuance_first_halving_epoch_8760() {
        // Blocks 11487788 (index 0) and 11487790 (index 2): epoch 8760
        // (first primary halving), length 1354. Secondary is unaffected by
        // the halving: 61369863013698 / 1354 = 45324861900 rem 1041.
        assert_eq!(
            secondary_block_issuance(0, 1354, SECONDARY_EPOCH_REWARD).unwrap(),
            45_324_861_901
        );
        assert_eq!(
            secondary_block_issuance(2, 1354, SECONDARY_EPOCH_REWARD).unwrap(),
            45_324_861_901
        );
    }

    #[test]
    fn secondary_block_issuance_remainder_boundary_epoch_14670() {
        // Epoch 14670, length 1800: 61369863013698 / 1800 = 34094368340
        // rem 1698. Block 20040784 (index 1697) is the LAST +1 block;
        // block 20040785 (index 1698) is the first base block. Both values
        // node-verified via get_block_economic_state.
        assert_eq!(
            secondary_block_issuance(1697, 1800, SECONDARY_EPOCH_REWARD).unwrap(),
            34_094_368_341
        );
        assert_eq!(
            secondary_block_issuance(1698, 1800, SECONDARY_EPOCH_REWARD).unwrap(),
            34_094_368_340
        );
    }

    #[test]
    fn secondary_block_issuance_rejects_invalid_epoch_position() {
        for (index, length) in [(0, 0), (-1, 1800), (1800, 1800), (5, -3)] {
            let err = secondary_block_issuance(index, length, SECONDARY_EPOCH_REWARD).unwrap_err();
            assert!(
                err.to_string().contains("invalid epoch position"),
                "expected invalid-epoch-position error for index={index} length={length}, got: {err}"
            );
        }
    }

    // -- calculate_miner_secondary_issuance (real mainnet vectors) ----------
    //
    // Expected values are `miner_reward.secondary` from the node's
    // `get_block_economic_state`; C/U inputs are the PARENT block's DAO field.

    #[test]
    fn miner_secondary_block_1_splits_against_genesis_state() {
        // Block 1: parent = genesis. C_0 = 3360000145238488200 (33.6B + the
        // genesis block's own p_0+s_0 share of 145238488200 shannons),
        // U_0 = 504120308900000000 (incl. 5.04B virtual occupied).
        assert_eq!(
            calculate_miner_secondary_issuance(
                35_209_330_473,
                3_360_000_145_238_488_200,
                504_120_308_900_000_000
            )
            .unwrap(),
            5_282_660_055
        );
    }

    #[test]
    fn miner_secondary_block_1600000() {
        // 2020-04 block; the S-delta reconstruction yields 4781307094 here
        // (one shannon high) — the direct split matches the node exactly.
        assert_eq!(
            calculate_miner_secondary_issuance(
                34_151_287_153,
                3_607_356_675_738_101_574,
                505_043_338_100_000_000
            )
            .unwrap(),
            4_781_307_093
        );
    }

    #[test]
    fn miner_secondary_block_4460004_uncle_era() {
        // 2021 block (epoch 3372, index 1410, length 1800).
        assert_eq!(
            secondary_block_issuance(1410, 1800, SECONDARY_EPOCH_REWARD).unwrap(),
            34_094_368_341
        );
        assert_eq!(
            calculate_miner_secondary_issuance(
                34_094_368_341,
                4_213_822_410_958_901_500,
                505_618_106_200_000_000
            )
            .unwrap(),
            4_090_995_839
        );
    }

    #[test]
    fn miner_secondary_block_11487790_post_halving() {
        assert_eq!(
            calculate_miner_secondary_issuance(
                45_324_861_901,
                5_577_600_232_289_909_804,
                519_018_915_500_000_000
            )
            .unwrap(),
            4_217_667_041
        );
    }

    #[test]
    fn miner_secondary_block_20040000_recent() {
        assert_eq!(
            calculate_miner_secondary_issuance(
                34_094_368_341,
                6_507_087_985_083_702_342,
                519_971_469_200_000_000
            )
            .unwrap(),
            2_724_428_936
        );
    }

    #[test]
    fn miner_secondary_rejects_invalid_parent_state() {
        // C <= U, C = 0, U < 0 — all invalid chain states must bail loudly.
        for (c, u) in [(1_000, 1_000), (1_000, 2_000), (0, 0), (1_000, -1)] {
            let err = calculate_miner_secondary_issuance(10, c, u).unwrap_err();
            assert!(
                err.to_string()
                    .contains("invalid parent DAO C/U relationship"),
                "expected invalid C/U error for C={c} U={u}, got: {err}"
            );
        }
    }

    #[test]
    fn compensation_uses_the_cells_actual_occupied_capacity() {
        let compensation = calculate_dao_compensation_from_ar(
            300 * SHANNON as i64,
            142 * SHANNON as i64,
            10_000,
            11_000,
        )
        .unwrap();

        assert_eq!(compensation, 15_80000000);
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
