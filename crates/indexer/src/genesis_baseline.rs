//! Derive the `GenesisBaseline` from block-0 chain data (single calc path).

use anyhow::{bail, Result};
use ckbadger_common::burn_policy::BurnPolicy;
use ckbadger_store::GenesisBaseline;

/// A genesis output reduced to what the burn policy needs.
#[derive(Debug, Clone)]
pub struct GenesisCell {
    pub capacity: i64,
    pub lock_args: Vec<u8>,
}

/// Compute the genesis economic baseline from the genesis DAO header + cells.
///
/// `total_issuance` = DAO C field (`dao[0..8]`, LE u64) — the exact on-chain
/// issuance. `burnt` = Σ capacity of genesis cells whose `lock_args` match the
/// policy (0 if no policy). `virtual_occupied` = burnt × ratio.
pub fn compute_genesis_baseline(
    genesis_dao: &[u8],
    genesis_cells: &[GenesisCell],
    policy: Option<&BurnPolicy>,
) -> Result<GenesisBaseline> {
    if genesis_dao.len() < 32 {
        bail!(
            "genesis DAO header too short: {} bytes (need 32)",
            genesis_dao.len()
        );
    }
    let total_issuance = u64::from_le_bytes(genesis_dao[0..8].try_into()?) as i128;

    let (burnt, virtual_occupied) = match policy {
        Some(p) => {
            let mut burnt: i128 = 0;
            for cell in genesis_cells {
                if cell.lock_args.as_slice() == p.lock_args {
                    burnt = burnt
                        .checked_add(cell.capacity as i128)
                        .ok_or_else(|| anyhow::anyhow!("genesis burnt capacity overflow"))?;
                }
            }
            let virtual_occupied = burnt
                .checked_mul(p.occupied_ratio_num as i128)
                .ok_or_else(|| anyhow::anyhow!("virtual_occupied overflow"))?
                / (p.occupied_ratio_denom as i128);
            (burnt, virtual_occupied)
        }
        None => (0, 0),
    };

    Ok(GenesisBaseline {
        total_issuance,
        burnt,
        virtual_occupied,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ckbadger_common::burn_policy::burn_policy;
    use ckbadger_common::dao::SATOSHI_PUBKEY_HASH;

    // Real mainnet genesis DAO header (fetched from mainnet.ckb.dev).
    const MAINNET_GENESIS_DAO_HEX: &str =
        "8874337e541ea12e0000c16ff286230029bfa3320800000000710b00c0fefe06";

    fn dao_bytes() -> Vec<u8> {
        (0..32)
            .map(|i| u8::from_str_radix(&MAINNET_GENESIS_DAO_HEX[i * 2..i * 2 + 2], 16).unwrap())
            .collect()
    }

    #[test]
    fn derives_real_mainnet_baseline_exactly() {
        // The Satoshi burn cell (8.4B CKB) plus one unrelated genesis cell.
        let cells = vec![
            GenesisCell {
                capacity: 840_000_000_000_000_000,
                lock_args: SATOSHI_PUBKEY_HASH.to_vec(),
            },
            GenesisCell {
                capacity: 1_000_000_000_000,
                lock_args: vec![0xaa; 20],
            },
        ];
        let b = compute_genesis_baseline(&dao_bytes(), &cells, burn_policy("mainnet").as_ref())
            .unwrap();
        assert_eq!(
            b.total_issuance, 3_360_000_145_238_488_200,
            "genesis DAO C field (exact, not 33.6B)"
        );
        assert_eq!(b.burnt, 840_000_000_000_000_000, "8.4B Satoshi burn");
        assert_eq!(
            b.virtual_occupied, 504_000_000_000_000_000,
            "8.4B * 6/10 = 5.04B"
        );
    }

    #[test]
    fn no_policy_yields_zero_burn() {
        let cells = vec![GenesisCell {
            capacity: 840_000_000_000_000_000,
            lock_args: SATOSHI_PUBKEY_HASH.to_vec(),
        }];
        let b = compute_genesis_baseline(&dao_bytes(), &cells, None).unwrap();
        assert_eq!(b.burnt, 0);
        assert_eq!(b.virtual_occupied, 0);
        assert_eq!(b.total_issuance, 3_360_000_145_238_488_200);
    }

    #[test]
    fn short_dao_errors() {
        assert!(compute_genesis_baseline(&[0u8; 8], &[], None).is_err());
    }
}
