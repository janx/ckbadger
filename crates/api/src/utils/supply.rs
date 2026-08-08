use anyhow::{ensure, Context, Result};
use ckbadger_store::types::dao_treasury_split;
use ckbadger_store::DaoDailySnapshot;

/// Exact supply components derived from one end-of-day DAO snapshot.
///
/// `circulating` follows the protocol/explorer definition:
/// `C - genesis_burnt - S`, where `S` is the complete unissued secondary
/// issuance pool. `liquid` additionally excludes active NervosDAO principal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DaoSupply {
    pub treasury: i128,
    pub burnt: i128,
    pub circulating: i128,
    pub dao_locked: i128,
    pub liquid: i128,
}

/// Derive the treasury portion of DAO `S` for one daily snapshot.
///
/// Delegates to [`dao_treasury_split`], the single derivation shared with the
/// indexer write paths, and only adds the snapshot date for locating a bad row.
pub fn dao_treasury(snapshot: &DaoDailySnapshot) -> Result<i128> {
    dao_treasury_split(snapshot.secondary_pool, snapshot.unclaimed_compensation)
        .with_context(|| format!("in dao_daily_snapshots for {}", snapshot.date))
}

/// Derive all user-facing supply components from the canonical DAO fields.
///
/// A zero `total_issuance` denotes an old/missing snapshot field. Every other
/// invalid state fails with date and operands instead of selecting a fallback
/// calculation.
pub fn dao_supply(snapshot: &DaoDailySnapshot, genesis_burnt: i128) -> Result<Option<DaoSupply>> {
    if snapshot.total_issuance == 0 {
        return Ok(None);
    }
    ensure!(
        snapshot.total_issuance > 0,
        "negative total_issuance in dao_daily_snapshots for {}: {}",
        snapshot.date,
        snapshot.total_issuance
    );
    ensure!(
        genesis_burnt >= 0,
        "negative genesis burnt baseline for {}: {}",
        snapshot.date,
        genesis_burnt
    );
    ensure!(
        snapshot.total_deposited >= 0,
        "negative total_deposited in dao_daily_snapshots for {}: {}",
        snapshot.date,
        snapshot.total_deposited
    );

    let treasury = dao_treasury(snapshot)?;
    let burnt = genesis_burnt.checked_add(treasury).with_context(|| {
        format!(
            "burnt supply overflow for {}: genesis_burnt={}, treasury={}",
            snapshot.date, genesis_burnt, treasury
        )
    })?;
    let circulating = snapshot
        .total_issuance
        .checked_sub(genesis_burnt)
        .and_then(|value| value.checked_sub(snapshot.secondary_pool))
        .with_context(|| {
            format!(
                "circulating supply arithmetic overflow for {}: total_issuance={}, genesis_burnt={}, secondary_pool={}",
                snapshot.date,
                snapshot.total_issuance,
                genesis_burnt,
                snapshot.secondary_pool
            )
        })?;
    ensure!(
        circulating >= 0,
        "negative circulating supply for {}: total_issuance={}, genesis_burnt={}, secondary_pool={}",
        snapshot.date,
        snapshot.total_issuance,
        genesis_burnt,
        snapshot.secondary_pool
    );

    let liquid = circulating
        .checked_sub(snapshot.total_deposited)
        .with_context(|| {
            format!(
                "liquid supply arithmetic overflow for {}: circulating={}, dao_locked={}",
                snapshot.date, circulating, snapshot.total_deposited
            )
        })?;
    ensure!(
        liquid >= 0,
        "DAO locked capacity exceeds circulating supply for {}: circulating={}, dao_locked={}",
        snapshot.date,
        circulating,
        snapshot.total_deposited
    );

    Ok(Some(DaoSupply {
        treasury,
        burnt,
        circulating,
        dao_locked: snapshot.total_deposited,
        liquid,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot() -> DaoDailySnapshot {
        DaoDailySnapshot {
            date: "2026-07-23".to_string(),
            total_deposited: 200,
            depositors_count: 0,
            new_deposits: 0,
            withdrawals: 0,
            compensation: 0,
            cumulative_deposit_amount: 0,
            total_issuance: 2_000,
            secondary_pool: 300,
            occupied_capacity: 0,
            cum_miner_secondary: 0,
            cum_dao_compensation: 0,
            cum_treasury: 999,
            unclaimed_compensation: 100,
            frozen_phase1_compensation: 0,
            cumulative_depositors: 0,
            daily_depositor_addresses: 0,
            protocol_deposited: None,
        }
    }

    #[test]
    fn derives_circulating_from_full_unissued_secondary_pool() {
        let supply = dao_supply(&snapshot(), 400).unwrap().unwrap();
        assert_eq!(
            supply,
            DaoSupply {
                treasury: 200,
                burnt: 600,
                circulating: 1_300,
                dao_locked: 200,
                liquid: 1_100,
            }
        );
    }

    #[test]
    fn ignores_legacy_cumulative_treasury_as_an_alternate_path() {
        let mut snapshot = snapshot();
        snapshot.cum_treasury = 1;
        assert_eq!(dao_treasury(&snapshot).unwrap(), 200);
    }

    /// The protocol subtracts a deposit's interest from `S` only when the
    /// phase-2 completion transaction runs (`withdrawed_interests`). Interest
    /// already frozen on a phase-1 withdraw-request cell is therefore still
    /// inside `S`, and it is already counted in `cum_dao_compensation`
    /// (= claimed + unclaimed). Leaving it in treasury as well counts the same
    /// shannons twice, so `treasury + unclaimed` must equal `S` exactly.
    #[test]
    fn treasury_plus_unclaimed_compensation_equals_secondary_pool() {
        let mut snapshot = snapshot();
        snapshot.secondary_pool = 1_000;
        // 200 accruing on live deposits + 100 frozen on phase-1 request cells.
        snapshot.unclaimed_compensation = 300;

        let treasury = dao_treasury(&snapshot).unwrap();
        assert_eq!(
            treasury + snapshot.unclaimed_compensation,
            snapshot.secondary_pool,
            "treasury must exclude ALL unmade DAO interest, not just the status-0 share"
        );
        assert_eq!(treasury, 700);
    }

    #[test]
    fn rejects_unmade_interest_above_secondary_pool() {
        let mut snapshot = snapshot();
        snapshot.unclaimed_compensation = 301;
        let error = dao_supply(&snapshot, 400).unwrap_err();
        let message = format!("{error:#}");
        assert!(
            message.contains("unmade DAO interests exceed secondary_pool"),
            "{message}"
        );
        assert!(message.contains("2026-07-23"), "{message}");
    }

    #[test]
    fn rejects_dao_locked_above_circulating_supply() {
        let mut snapshot = snapshot();
        snapshot.total_deposited = 1_301;
        let error = dao_supply(&snapshot, 400).unwrap_err();
        assert!(error
            .to_string()
            .contains("DAO locked capacity exceeds circulating supply"));
    }

    #[test]
    fn missing_total_issuance_returns_none() {
        let mut snapshot = snapshot();
        snapshot.total_issuance = 0;
        assert_eq!(dao_supply(&snapshot, 400).unwrap(), None);
    }
}
