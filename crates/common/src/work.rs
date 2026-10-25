/// Divide nonnegative exact integers, rounding ties away from zero.
pub fn rounded_ratio(numerator: u128, denominator: u128) -> Result<u128, &'static str> {
    if denominator == 0 {
        return Err("zero denominator in exact work ratio");
    }
    let quotient = numerator / denominator;
    let remainder = numerator % denominator;
    // Compare against ceil(denominator / 2) without doubling the remainder.
    if remainder >= denominator / 2 + denominator % 2 {
        quotient
            .checked_add(1)
            .ok_or("rounded work ratio exceeds u128")
    } else {
        Ok(quotient)
    }
}

/// Exact work per second, rounded half-up to integer H/s.
pub fn hash_rate_hps(work_sum: u128, span_ms: u64) -> Result<u128, &'static str> {
    let numerator = work_sum
        .checked_mul(1000)
        .ok_or("work × 1000 exceeds u128")?;
    rounded_ratio(numerator, span_ms as u128)
}

/// Convert a CKB compact target to exact work, rejecting values above u128.
pub fn compact_difficulty(target: u32) -> Result<u128, String> {
    let difficulty = ckb_types::utilities::compact_to_difficulty(target);
    let work: u128 = difficulty
        .to_string()
        .parse()
        .map_err(|_| format!("compact target {target:#x} difficulty {difficulty} exceeds u128"))?;
    if work == 0 {
        return Err(format!("compact target {target:#x} has zero difficulty"));
    }
    Ok(work)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_half_up_above_f64_precision() {
        assert_eq!(
            rounded_ratio(9_007_199_254_740_993, 1).unwrap(),
            9_007_199_254_740_993
        );
        assert_eq!(rounded_ratio(5, 2).unwrap(), 3);
        assert_eq!(rounded_ratio(4, 2).unwrap(), 2);
        assert_eq!(rounded_ratio(7, 4).unwrap(), 2);
        assert_eq!(rounded_ratio(6, 4).unwrap(), 2);
        assert_eq!(rounded_ratio(u128::MAX, u128::MAX).unwrap(), 1);
        assert_eq!(
            hash_rate_hps(9_007_199_254_740_993, 1000).unwrap(),
            9_007_199_254_740_993
        );
    }

    #[test]
    fn rejects_invalid_or_overflowing_work() {
        assert!(rounded_ratio(1, 0).is_err());
        assert!(hash_rate_hps(u128::MAX, 1).is_err());
        assert!(compact_difficulty(0).is_err());
        assert!(compact_difficulty(0x1a08a97e).unwrap() > 0);
    }
}
