use anyhow::{ensure, Context, Result};
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

/// Derive the treasury portion of DAO `S`.
///
/// RFC-0023 defines `S` as all unissued secondary issuance, including both
/// unmade DAO compensation and treasury funds. The treasury portion is
/// therefore exactly `S - unmade_dao_interests`.
pub fn dao_treasury(snapshot: &DaoDailySnapshot) -> Result<i128> {
    ensure!(
        snapshot.secondary_pool >= 0,
        "negative secondary_pool in dao_daily_snapshots for {}: {}",
        snapshot.date,
        snapshot.secondary_pool
    );
    ensure!(
        snapshot.unmade_dao_interests >= 0,
        "negative unmade_dao_interests in dao_daily_snapshots for {}: {}",
        snapshot.date,
        snapshot.unmade_dao_interests
    );

    let treasury = snapshot
        .secondary_pool
        .checked_sub(snapshot.unmade_dao_interests)
        .with_context(|| {
            format!(
                "treasury underflow in dao_daily_snapshots for {}: secondary_pool={}, unmade_dao_interests={}",
                snapshot.date, snapshot.secondary_pool, snapshot.unmade_dao_interests
            )
        })?;
    ensure!(
        treasury >= 0,
        "unmade DAO interests exceed secondary_pool for {}: secondary_pool={}, unmade_dao_interests={}",
        snapshot.date,
        snapshot.secondary_pool,
        snapshot.unmade_dao_interests
    );
    Ok(treasury)
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
            unmade_dao_interests: 100,
            unclaimed_compensation: 0,
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

    #[test]
    fn rejects_unmade_interest_above_secondary_pool() {
        let mut snapshot = snapshot();
        snapshot.unmade_dao_interests = 301;
        let error = dao_supply(&snapshot, 400).unwrap_err();
        assert!(error
            .to_string()
            .contains("unmade DAO interests exceed secondary_pool"));
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
