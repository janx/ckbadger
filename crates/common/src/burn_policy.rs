//! Per-network genesis burn policy: which genesis cells count as "burnt" and
//! the occupied-capacity ratio applied to them (CKB-Explorer accounting).
//!
//! The burnt AMOUNT is derived from chain (sum of matching genesis cell
//! capacities); this only declares the POLICY. mainnet & testnet both use the
//! Satoshi dead-address cell. Unknown networks (e.g. a future devnet) return
//! `None` → burnt = 0 until a policy is declared.

use crate::dao::SATOSHI_PUBKEY_HASH;
use crate::hardfork::{normalize_network, NETWORK_MAINNET, NETWORK_TESTNET};

/// Declares the genesis burn convention for a network.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BurnPolicy {
    /// Genesis cells whose lock `args` equal this are burnt.
    pub lock_args: &'static [u8],
    /// Numerator of the fraction of burnt capacity treated as "occupied".
    pub occupied_ratio_num: u128,
    /// Denominator of that fraction.
    pub occupied_ratio_denom: u128,
}

/// The burn policy for `network`, or `None` if the network declares none.
pub fn burn_policy(network: &str) -> Option<BurnPolicy> {
    match normalize_network(network)? {
        NETWORK_MAINNET | NETWORK_TESTNET => Some(BurnPolicy {
            lock_args: &SATOSHI_PUBKEY_HASH,
            occupied_ratio_num: 6,
            occupied_ratio_denom: 10,
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dao::SATOSHI_PUBKEY_HASH;

    #[test]
    fn mainnet_and_testnet_use_satoshi_burn_6_10() {
        for net in ["mainnet", "testnet", "ckb", "pudge"] {
            let p = burn_policy(net).unwrap_or_else(|| panic!("policy for {net}"));
            assert_eq!(p.lock_args, &SATOSHI_PUBKEY_HASH);
            assert_eq!((p.occupied_ratio_num, p.occupied_ratio_denom), (6, 10));
        }
    }

    #[test]
    fn unknown_network_has_no_policy() {
        assert!(burn_policy("devnet").is_none());
    }
}
