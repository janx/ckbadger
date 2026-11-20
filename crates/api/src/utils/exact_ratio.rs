/// The whole part, the first `decimals` digits and the remainder of a nonnegative ratio, exactly:
/// the long division both `format_ratio` (half up) and `format_ratio_cut` (cut) write from.
fn ratio_digits(
    numerator: i128,
    denominator: i128,
    decimals: u32,
) -> Result<(u128, Vec<u8>, u128, u128), String> {
    if numerator < 0 || denominator <= 0 {
        return Err(format!("invalid ratio {numerator}/{denominator}"));
    }
    if decimals > 18 {
        return Err(format!("unsupported ratio precision {decimals}"));
    }
    let divisor = denominator as u128;
    let whole = numerator as u128 / divisor;
    let mut remainder = numerator as u128 % divisor;
    let mut digits = Vec::with_capacity(decimals as usize);
    for _ in 0..decimals {
        // Ten modular additions produce one decimal digit without ever
        // multiplying a large numerator or denominator.
        let prior_remainder = remainder;
        let mut next_remainder = 0u128;
        let mut digit = 0u8;
        for _ in 0..10 {
            if next_remainder >= divisor - prior_remainder {
                next_remainder -= divisor - prior_remainder;
                digit += 1;
            } else {
                next_remainder += prior_remainder;
            }
        }
        digits.push(digit);
        remainder = next_remainder;
    }
    Ok((whole, digits, remainder, divisor))
}

/// `whole.digits`, or `whole` with no digits.
fn write_digits(whole: u128, digits: Vec<u8>) -> String {
    if digits.is_empty() {
        whole.to_string()
    } else {
        format!(
            "{whole}.{}",
            digits
                .into_iter()
                .map(|digit| char::from(b'0' + digit))
                .collect::<String>()
        )
    }
}

/// Format a nonnegative rational number cut to `decimals` places, never rounded: a figure a chart
/// page states shows no digit that is not the exact value's (the D1 hand-back, item 1). The caller
/// chooses the unit: pass `numerator * 100` for a percentage.
pub fn format_ratio_cut(
    numerator: i128,
    denominator: i128,
    decimals: u32,
) -> Result<String, String> {
    let (whole, digits, _, _) = ratio_digits(numerator, denominator, decimals)?;
    Ok(write_digits(whole, digits))
}

/// Format a nonnegative rational number with exact half-up decimal rounding.
/// The caller chooses the unit: pass `numerator * 100` for a percentage.
pub fn format_ratio(numerator: i128, denominator: i128, decimals: u32) -> Result<String, String> {
    let (mut whole, mut digits, remainder, divisor) =
        ratio_digits(numerator, denominator, decimals)?;
    if remainder >= divisor / 2 + divisor % 2 {
        let mut carry = true;
        for digit in digits.iter_mut().rev() {
            if *digit == 9 {
                *digit = 0;
            } else {
                *digit += 1;
                carry = false;
                break;
            }
        }
        if carry {
            whole += 1;
        }
    }
    Ok(write_digits(whole, digits))
}

/// Format a nonnegative ratio as a percentage without multiplying an i128
/// numerator by 100 (which would overflow for valid chain-sized aggregates).
pub fn format_percent(numerator: i128, denominator: i128, decimals: u32) -> Result<String, String> {
    let ratio = format_ratio(
        numerator,
        denominator,
        decimals
            .checked_add(2)
            .ok_or("percentage precision overflow")?,
    )?;
    percent_from_ratio(&ratio, decimals)
}

/// The same percentage cut at `decimals`, never rounded (lane D1's third round): a share a chart
/// page states shows no digit that is not the exact value's.
pub fn format_percent_cut(
    numerator: i128,
    denominator: i128,
    decimals: u32,
) -> Result<String, String> {
    let ratio = format_ratio_cut(
        numerator,
        denominator,
        decimals
            .checked_add(2)
            .ok_or("percentage precision overflow")?,
    )?;
    percent_from_ratio(&ratio, decimals)
}

/// A ratio written at `decimals + 2` places, as a percentage at `decimals`: the point moved two
/// places right.
fn percent_from_ratio(ratio: &str, decimals: u32) -> Result<String, String> {
    let (whole, fraction) = ratio
        .split_once('.')
        .ok_or("percentage requires fractional digits")?;
    let (integer_tail, decimal_tail) = fraction.split_at(2);
    let integer = format!("{whole}{integer_tail}");
    let integer = integer.trim_start_matches('0');
    let integer = if integer.is_empty() { "0" } else { integer };
    if decimals == 0 {
        Ok(integer.to_string())
    } else {
        Ok(format!("{integer}.{decimal_tail}"))
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
    fn a_cut_percentage_never_carries() {
        assert_eq!(format_percent_cut(2, 3, 6).unwrap(), "66.666666");
        assert_eq!(format_percent(2, 3, 6).unwrap(), "66.666667");
        assert_eq!(format_percent_cut(1, 1, 0).unwrap(), "100");
    }

    #[test]
    fn a_cut_ratio_drops_the_digits_past_its_places_and_never_carries() {
        assert_eq!(format_ratio_cut(200, 3, 2).unwrap(), "66.66");
        assert_eq!(format_ratio(200, 3, 2).unwrap(), "66.67");
        assert_eq!(format_ratio_cut(1, 8, 2).unwrap(), "0.12");
        assert_eq!(format_ratio_cut(19_999, 200, 2).unwrap(), "99.99");
        assert_eq!(format_ratio_cut(3, 2, 0).unwrap(), "1");
        assert_eq!(
            format_ratio_cut(i128::MAX, 1, 2).unwrap(),
            format!("{}.00", i128::MAX)
        );
        assert!(format_ratio_cut(-1, 2, 2).is_err());
    }

    #[test]
    fn exact_half_up_ratio_survives_double_precision_and_ties() {
        assert_eq!(
            format_ratio(1_005_000_000_000_000, 1_000_000_000_000_000, 2).unwrap(),
            "1.01"
        );
        assert_eq!(format_ratio(1, 8, 2).unwrap(), "0.13");
        assert_eq!(format_ratio(0, 8, 4).unwrap(), "0.0000");
        assert_eq!(format_ratio(3, 2, 0).unwrap(), "2");
        assert_eq!(
            format_ratio(i128::MAX, 1, 2).unwrap(),
            format!("{}.00", i128::MAX)
        );
        assert_eq!(
            format_ratio(i128::MAX - 1, i128::MAX, 6).unwrap(),
            "1.000000"
        );
        assert!(format_ratio(1, 0, 2).is_err());
        assert!(format_ratio(-1, 2, 2).is_err());
    }

    #[test]
    fn exact_percent_handles_full_i128_without_scaled_numerator() {
        assert_eq!(format_percent(1, 8, 6).unwrap(), "12.500000");
        assert_eq!(
            format_percent(i128::MAX, i128::MAX, 6).unwrap(),
            "100.000000"
        );
        assert_eq!(format_percent(1, 2, 0).unwrap(), "50");
    }

    #[test]
    fn empty_share_is_absent_but_broken_share_is_an_error() {
        assert_eq!(format_share_4(0, 0).unwrap(), None);
        assert_eq!(format_share_4(1, 2).unwrap().as_deref(), Some("0.5000"));
        assert!(format_share_4(1, 0).is_err());
        assert!(format_share_4(3, 2).is_err());
    }
}
