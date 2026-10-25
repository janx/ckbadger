/// Format a nonnegative rational number with exact half-up decimal rounding.
/// The caller chooses the unit: pass `numerator * 100` for a percentage.
pub fn format_ratio(numerator: i128, denominator: i128, decimals: u32) -> Result<String, String> {
    if numerator < 0 || denominator <= 0 {
        return Err(format!("invalid ratio {numerator}/{denominator}"));
    }
    let scale = 10u128
        .checked_pow(decimals)
        .ok_or_else(|| format!("decimal scale 10^{decimals} exceeds u128"))?;
    let scaled = (numerator as u128).checked_mul(scale).ok_or_else(|| {
        format!("ratio {numerator}/{denominator} at {decimals} decimals overflows u128")
    })?;
    let rounded = ckbadger_common::work::rounded_ratio(scaled, denominator as u128)
        .map_err(str::to_string)?;
    if decimals == 0 {
        Ok(rounded.to_string())
    } else {
        Ok(format!(
            "{}.{:0width$}",
            rounded / scale,
            rounded % scale,
            width = decimals as usize
        ))
    }
}

/// A bounded share, with a genuinely empty set represented as absent.
pub fn format_share_4(numerator: i64, denominator: i64) -> Result<Option<String>, String> {
    if numerator < 0 || denominator < 0 || numerator > denominator {
        return Err(format!("invalid share {numerator}/{denominator}"));
    }
    if denominator == 0 {
        return Ok(None);
    }
    format_ratio(numerator as i128, denominator as i128, 4).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_half_up_ratio_survives_double_precision_and_ties() {
        assert_eq!(
            format_ratio(1_005_000_000_000_000, 1_000_000_000_000_000, 2).unwrap(),
            "1.01"
        );
        assert_eq!(format_ratio(1, 8, 2).unwrap(), "0.13");
        assert_eq!(format_ratio(0, 8, 4).unwrap(), "0.0000");
        assert_eq!(format_ratio(3, 2, 0).unwrap(), "2");
        assert!(format_ratio(1, 0, 2).is_err());
        assert!(format_ratio(-1, 2, 2).is_err());
    }

    #[test]
    fn empty_share_is_absent_but_broken_share_is_an_error() {
        assert_eq!(format_share_4(0, 0).unwrap(), None);
        assert_eq!(format_share_4(1, 2).unwrap().as_deref(), Some("0.5000"));
        assert!(format_share_4(1, 0).is_err());
        assert!(format_share_4(3, 2).is_err());
    }
}
