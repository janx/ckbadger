//! Exact Shannon fee rates. The rational value is retained for comparison and
//! aggregation; only serialized readouts are rounded half-up to two decimals.

use num_bigint::BigUint;
use num_integer::Integer;
use std::cmp::Ordering;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FeeRate {
    pub fee: u64,
    pub size: u64,
}

impl FeeRate {
    pub fn new(fee: u64, size: u64) -> Result<Self, String> {
        if size == 0 {
            return Err("transaction fee-rate size must be positive".to_string());
        }
        Ok(Self { fee, size })
    }

    pub fn format(self, bytes_per_unit: u64) -> String {
        format_rational(
            BigUint::from(self.fee) * BigUint::from(bytes_per_unit),
            BigUint::from(self.size),
        )
    }
}

impl Ord for FeeRate {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.fee as u128 * other.size as u128).cmp(&(other.fee as u128 * self.size as u128))
    }
}

impl PartialOrd for FeeRate {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

pub fn mean_rate(rates: &[FeeRate], bytes_per_unit: u64) -> Option<String> {
    if rates.is_empty() {
        return None;
    }
    let (sum_num, sum_den) = sum_rates(rates);
    Some(format_rational(
        sum_num * BigUint::from(bytes_per_unit),
        sum_den * BigUint::from(rates.len()),
    ))
}

pub fn median_rate(sorted_rates: &[FeeRate], bytes_per_unit: u64) -> Option<String> {
    if sorted_rates.is_empty() {
        return None;
    }
    let middle = sorted_rates.len() / 2;
    if sorted_rates.len() % 2 == 1 {
        return Some(sorted_rates[middle].format(bytes_per_unit));
    }
    let (sum_num, sum_den) = sum_rates(&sorted_rates[middle - 1..=middle]);
    Some(format_rational(
        sum_num * BigUint::from(bytes_per_unit),
        sum_den * BigUint::from(2u8),
    ))
}

fn sum_rates(rates: &[FeeRate]) -> (BigUint, BigUint) {
    let mut numerator = BigUint::from(0u8);
    let mut denominator = BigUint::from(1u8);
    for rate in rates {
        numerator = numerator * BigUint::from(rate.size) + BigUint::from(rate.fee) * &denominator;
        denominator *= BigUint::from(rate.size);
        let divisor = numerator.gcd(&denominator);
        numerator /= &divisor;
        denominator /= divisor;
    }
    (numerator, denominator)
}

fn format_rational(numerator: BigUint, denominator: BigUint) -> String {
    let scaled = numerator * BigUint::from(100u16);
    let (mut quotient, remainder) = scaled.div_rem(&denominator);
    if remainder * BigUint::from(2u8) >= denominator {
        quotient += BigUint::from(1u8);
    }
    let (whole, cents) = quotient.div_rem(&BigUint::from(100u16));
    format!("{whole}.{cents:0>2}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_above_ieee_safe_integer_without_ties() {
        let lower = FeeRate::new(9_007_199_254_740_992, 1).unwrap();
        let higher = FeeRate::new(9_007_199_254_740_993, 1).unwrap();
        assert!(higher > lower);
        assert_eq!(higher.format(1), "9007199254740993.00");
    }

    #[test]
    fn mean_is_arithmetic_mean_of_each_exact_rate() {
        let rates = [FeeRate::new(1, 3).unwrap(), FeeRate::new(1, 2).unwrap()];
        assert_eq!(mean_rate(&rates, 1000), Some("416.67".to_string()));
        assert_eq!(median_rate(&rates, 1), Some("0.42".to_string()));
    }

    #[test]
    fn zero_size_rejected_and_empty_aggregate_is_absent() {
        assert!(FeeRate::new(1, 0).is_err());
        assert_eq!(mean_rate(&[], 1000), None);
        assert_eq!(median_rate(&[], 1), None);
    }
}
