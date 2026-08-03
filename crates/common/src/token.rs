use std::fmt;
use std::str::FromStr;

use ckb_types::U256;

/// Exact aggregate balance for fungible tokens.
///
/// A single sUDT cell stores a `u128` amount, but a holder balance and total
/// live supply sum amounts across many cells and therefore require a wider
/// domain. The fixed-width representation also keeps RocksDB ranked keys
/// lexicographically sortable.
#[derive(Clone, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct TokenBalance(U256);

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TokenBalanceError {
    #[error("token balance encoding must be exactly {expected} bytes, got {actual}")]
    InvalidEncodedLength { expected: usize, actual: usize },
    #[error("invalid token balance encoding: {0}")]
    InvalidEncoding(String),
    #[error("invalid token balance decimal `{value}`: {reason}")]
    InvalidDecimal { value: String, reason: String },
}

impl TokenBalance {
    pub const ENCODED_LEN: usize = 32;

    pub fn zero() -> Self {
        Self(U256::zero())
    }

    pub fn is_zero(&self) -> bool {
        self.0.is_zero()
    }

    pub fn checked_add(&self, rhs: &Self) -> Option<Self> {
        self.0.checked_add(&rhs.0).map(Self)
    }

    pub fn checked_sub(&self, rhs: &Self) -> Option<Self> {
        self.0.checked_sub(&rhs.0).map(Self)
    }

    /// Narrow back to the on-chain cell width.
    ///
    /// Returns `None` when the aggregate has grown past a single cell's `u128`
    /// domain. Callers that must hand the value to a `u128` boundary use this to
    /// detect the overflow explicitly instead of wrapping or saturating.
    pub fn to_u128(&self) -> Option<u128> {
        let bytes = self.to_be_bytes();
        let (high, low) = bytes.split_at(Self::ENCODED_LEN - 16);
        if high.iter().any(|byte| *byte != 0) {
            return None;
        }
        Some(u128::from_be_bytes(
            low.try_into().expect("low half is exactly 16 bytes"),
        ))
    }

    /// Stable RocksDB representation: fixed-width unsigned big-endian.
    pub fn to_be_bytes(&self) -> [u8; Self::ENCODED_LEN] {
        let mut bytes = [0u8; Self::ENCODED_LEN];
        self.0
            .into_big_endian(&mut bytes)
            .expect("TokenBalance output has the fixed U256 width");
        bytes
    }

    pub fn from_be_bytes(bytes: &[u8]) -> Result<Self, TokenBalanceError> {
        if bytes.len() != Self::ENCODED_LEN {
            return Err(TokenBalanceError::InvalidEncodedLength {
                expected: Self::ENCODED_LEN,
                actual: bytes.len(),
            });
        }
        U256::from_big_endian(bytes)
            .map(Self)
            .map_err(|err| TokenBalanceError::InvalidEncoding(err.to_string()))
    }
}

impl From<u128> for TokenBalance {
    fn from(value: u128) -> Self {
        Self(U256::from(value))
    }
}

impl From<&TokenBalance> for TokenBalance {
    fn from(value: &TokenBalance) -> Self {
        value.clone()
    }
}

impl PartialEq<u128> for TokenBalance {
    fn eq(&self, other: &u128) -> bool {
        self == &Self::from(*other)
    }
}

impl fmt::Debug for TokenBalance {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, formatter)
    }
}

impl fmt::Display for TokenBalance {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, formatter)
    }
}

impl FromStr for TokenBalance {
    type Err = TokenBalanceError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        U256::from_dec_str(value)
            .map(Self)
            .map_err(|err| TokenBalanceError::InvalidDecimal {
                value: value.to_owned(),
                reason: err.to_string(),
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aggregate_above_u128_round_trips_exactly() {
        let amount = TokenBalance::from(200u128 << 120);
        let total = amount.checked_add(&amount).unwrap();

        assert_eq!(total.to_string(), "531691198313966349161522824112137830400");
        let max_u128 = TokenBalance::from(u128::MAX);
        assert!(total > max_u128);
        assert_eq!(
            TokenBalance::from_be_bytes(&total.to_be_bytes()).unwrap(),
            total
        );
        assert_eq!(total.to_string().parse::<TokenBalance>().unwrap(), total);
    }

    #[test]
    fn fixed_width_decode_rejects_wrong_length() {
        let error = TokenBalance::from_be_bytes(&[0u8; 16]).unwrap_err();
        assert_eq!(
            error,
            TokenBalanceError::InvalidEncodedLength {
                expected: 32,
                actual: 16
            }
        );
    }

    #[test]
    fn to_u128_narrows_only_within_the_cell_width() {
        assert_eq!(TokenBalance::zero().to_u128(), Some(0));
        assert_eq!(TokenBalance::from(u128::MAX).to_u128(), Some(u128::MAX));

        let above = TokenBalance::from(u128::MAX)
            .checked_add(&TokenBalance::from(1))
            .unwrap();
        assert_eq!(above.to_u128(), None);
    }

    #[test]
    fn checked_sub_rejects_underflow() {
        assert!(TokenBalance::zero()
            .checked_sub(&TokenBalance::from(1))
            .is_none());
    }
}
