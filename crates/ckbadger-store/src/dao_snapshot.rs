//! One exact calculation path for a DAO daily snapshot.
use crate::types::{DaoCompensationBreakdown, DaoDailySnapshot};
use anyhow::{anyhow, ensure, Context, Result};
use chrono::NaiveDate;
use ckbadger_common::dao::extract_dao_csu;
use ckbadger_common::stats_bucket::stats_day;

#[derive(Debug, Clone, Copy, Default)]
pub struct DaoSnapshotDelta {
    pub active_capacity: i128,
    pub protocol_capacity: i128,
    pub gross_deposits: i128,
    pub deposits: i64,
    pub withdrawals: i64,
    pub active_deposits: i64,
    pub cumulative_depositors: i64,
    pub miner_secondary: i128,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DaoObservationKind {
    SealedDay,
    IncompleteTip,
}

pub struct DaoSnapshotObservation<'a> {
    pub kind: DaoObservationKind,
    pub date: NaiveDate,
    pub block_number: i64,
    pub chain_time_ms: i64,
    pub dao_field: &'a [u8],
    pub daily_depositor_addresses: i64,
    pub compensation: DaoCompensationBreakdown,
}

/// Reduce one canonical day. `previous=None` is the additive identity only
/// when the caller proves this reducer started at genesis; live and reorg
/// reject a missing prior row for any later starting block.
pub fn reduce_dao_daily_snapshot(
    previous: Option<&DaoDailySnapshot>,
    delta: DaoSnapshotDelta,
    observation: DaoSnapshotObservation<'_>,
) -> Result<DaoDailySnapshot> {
    let context = || {
        format!(
            "DAO {:?} date={} block={}",
            observation.kind, observation.date, observation.block_number
        )
    };
    ensure!(
        observation.block_number >= 0,
        "{}: negative observation block",
        context()
    );
    ensure!(
        stats_day(observation.chain_time_ms)? == observation.date,
        "{}: end-header chain_time_ms={} belongs to a different day",
        context(),
        observation.chain_time_ms
    );
    ensure!(
        observation.dao_field.len() == 32,
        "{}: invalid DAO field length {}",
        context(),
        observation.dao_field.len()
    );
    let (c, s, u) = extract_dao_csu(observation.dao_field)
        .ok_or_else(|| anyhow!("{}: invalid DAO C/S/U", context()))?;
    let ar = u64::from_le_bytes(observation.dao_field[8..16].try_into()?);
    ensure!(ar > 0, "{}: zero DAO AR", context());
    ensure!(
        observation.daily_depositor_addresses >= 0,
        "{}: negative daily depositor address count",
        context()
    );
    if let Some(previous) = previous {
        let previous_date =
            NaiveDate::parse_from_str(&previous.date, "%Y-%m-%d").with_context(|| {
                format!(
                    "{}: invalid previous snapshot date {}",
                    context(),
                    previous.date
                )
            })?;
        ensure!(
            previous_date <= observation.date,
            "{}: previous snapshot is from future date {}",
            context(),
            previous.date
        );
        ensure!(
            previous.protocol_deposited.is_some(),
            "{}: previous snapshot lacks protocol deposited capacity",
            context()
        );
    }
    let add_i128 = |name: &str, prior: i128, change: i128| -> Result<i128> {
        ensure!(prior >= 0, "{}: negative prior {name}: {prior}", context());
        let value = prior
            .checked_add(change)
            .ok_or_else(|| anyhow!("{}: {name} overflow: {prior} + {change}", context()))?;
        ensure!(value >= 0, "{}: {name} negative: {value}", context());
        Ok(value)
    };
    let add_i64 = |name: &str, prior: i64, change: i64| -> Result<i64> {
        ensure!(prior >= 0, "{}: negative prior {name}: {prior}", context());
        let value = prior
            .checked_add(change)
            .ok_or_else(|| anyhow!("{}: {name} overflow: {prior} + {change}", context()))?;
        ensure!(value >= 0, "{}: {name} negative: {value}", context());
        Ok(value)
    };
    let previous_i128 = |f: fn(&DaoDailySnapshot) -> i128| previous.map(f).unwrap_or(0);
    let previous_i64 = |f: fn(&DaoDailySnapshot) -> i64| previous.map(f).unwrap_or(0);
    let total_deposited = add_i128(
        "total_deposited",
        previous_i128(|s| s.total_deposited),
        delta.active_capacity,
    )?;
    let protocol_deposited = add_i128(
        "protocol_deposited",
        previous.and_then(|s| s.protocol_deposited).unwrap_or(0),
        delta.protocol_capacity,
    )?;
    ensure!(
        protocol_deposited >= total_deposited,
        "{}: protocol capacity {} below active capacity {}",
        context(),
        protocol_deposited,
        total_deposited
    );
    let gross = add_i128(
        "cumulative_deposit_amount",
        previous_i128(|s| s.cumulative_deposit_amount),
        delta.gross_deposits,
    )?;
    let deposits = add_i64(
        "new_deposits",
        previous_i64(|s| s.new_deposits),
        delta.deposits,
    )?;
    let withdrawals = add_i64(
        "withdrawals",
        previous_i64(|s| s.withdrawals),
        delta.withdrawals,
    )?;
    let active_deposits = add_i64(
        "depositors_count",
        previous_i64(|s| s.depositors_count),
        delta.active_deposits,
    )?;
    let cumulative_depositors = add_i64(
        "cumulative_depositors",
        previous_i64(|s| s.cumulative_depositors),
        delta.cumulative_depositors,
    )?;
    let miner = add_i128(
        "cum_miner_secondary",
        previous_i128(|s| s.cum_miner_secondary),
        delta.miner_secondary,
    )?;
    ensure!(
        observation.compensation.claimed >= 0 && observation.compensation.unclaimed >= 0,
        "{}: negative compensation",
        context()
    );
    let dao_compensation = observation
        .compensation
        .total()
        .ok_or_else(|| anyhow!("{}: DAO compensation overflow", context()))?;
    let treasury = observation.compensation.treasury(s).with_context(context)?;
    let frozen = observation
        .compensation
        .frozen_phase1()
        .with_context(context)?;
    ensure!(
        c >= s && c >= u,
        "{}: invalid end-header C/S/U: C={c} S={s} U={u}",
        context()
    );
    Ok(DaoDailySnapshot {
        date: observation.date.format("%Y-%m-%d").to_string(),
        total_deposited,
        depositors_count: active_deposits,
        new_deposits: deposits,
        withdrawals,
        compensation: observation.compensation.claimed,
        cumulative_deposit_amount: gross,
        total_issuance: c,
        secondary_pool: s,
        occupied_capacity: u,
        cum_miner_secondary: miner,
        cum_dao_compensation: dao_compensation,
        cum_treasury: treasury,
        unclaimed_compensation: observation.compensation.unclaimed,
        frozen_phase1_compensation: frozen,
        cumulative_depositors,
        daily_depositor_addresses: observation.daily_depositor_addresses,
        protocol_deposited: Some(protocol_deposited),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dao(c: u64, ar: u64, s: u64, u: u64) -> [u8; 32] {
        let mut bytes = [0; 32];
        for (index, value) in [c, ar, s, u].iter().enumerate() {
            bytes[index * 8..index * 8 + 8].copy_from_slice(&value.to_le_bytes());
        }
        bytes
    }

    #[test]
    fn exact_snapshot_accumulates_and_derives_one_consistent_partition() {
        let date = NaiveDate::from_ymd_opt(2024, 9, 15).unwrap();
        let dao = dao(1000, 10, 100, 50);
        let snapshot = reduce_dao_daily_snapshot(
            None,
            DaoSnapshotDelta {
                active_capacity: 200,
                protocol_capacity: 200,
                gross_deposits: 200,
                deposits: 1,
                active_deposits: 1,
                cumulative_depositors: 1,
                miner_secondary: 7,
                ..Default::default()
            },
            DaoSnapshotObservation {
                kind: DaoObservationKind::SealedDay,
                date,
                block_number: 42,
                chain_time_ms: 1_726_330_000_000,
                dao_field: &dao,
                daily_depositor_addresses: 1,
                compensation: DaoCompensationBreakdown {
                    claimed: 3,
                    unclaimed: 5,
                    active_unmade: 4,
                },
            },
        )
        .unwrap();
        assert_eq!(snapshot.cum_dao_compensation, 8);
        assert_eq!(snapshot.cum_treasury, 95);
        assert_eq!(snapshot.frozen_phase1_compensation, 1);
        assert_eq!(snapshot.protocol_deposited, Some(200));
        let mut corrupted = snapshot.clone();
        corrupted.total_deposited = -1;
        let error = reduce_dao_daily_snapshot(
            Some(&corrupted),
            DaoSnapshotDelta {
                active_capacity: 1,
                ..Default::default()
            },
            DaoSnapshotObservation {
                kind: DaoObservationKind::IncompleteTip,
                date,
                block_number: 43,
                chain_time_ms: 1_726_330_000_000,
                dao_field: &dao,
                daily_depositor_addresses: 1,
                compensation: DaoCompensationBreakdown::default(),
            },
        )
        .unwrap_err();
        assert!(
            error.to_string().contains("negative prior total_deposited"),
            "{error:#}"
        );
        let err = reduce_dao_daily_snapshot(
            Some(&snapshot),
            DaoSnapshotDelta {
                active_capacity: -201,
                ..Default::default()
            },
            DaoSnapshotObservation {
                kind: DaoObservationKind::IncompleteTip,
                date,
                block_number: 43,
                chain_time_ms: 1_726_330_000_000,
                dao_field: &dao,
                daily_depositor_addresses: 1,
                compensation: DaoCompensationBreakdown::default(),
            },
        )
        .unwrap_err();
        assert!(err.to_string().contains("total_deposited negative"));
    }

    #[test]
    fn rejects_invalid_header_and_overflow() {
        let date = NaiveDate::from_ymd_opt(2024, 9, 15).unwrap();
        for dao in [dao(1000, 0, 100, 50), dao(10, 1, 100, 50)] {
            let err = reduce_dao_daily_snapshot(
                None,
                DaoSnapshotDelta::default(),
                DaoSnapshotObservation {
                    kind: DaoObservationKind::SealedDay,
                    date,
                    block_number: 3,
                    chain_time_ms: 1_726_330_000_000,
                    dao_field: &dao,
                    daily_depositor_addresses: 0,
                    compensation: DaoCompensationBreakdown::default(),
                },
            )
            .unwrap_err();
            assert!(err.to_string().contains("DAO"));
        }
        let dao = dao(1000, 1, 100, 50);
        let wrong_day = reduce_dao_daily_snapshot(
            None,
            DaoSnapshotDelta::default(),
            DaoSnapshotObservation {
                kind: DaoObservationKind::SealedDay,
                date,
                block_number: 3,
                chain_time_ms: 1_726_330_000_000 - 86_400_000,
                dao_field: &dao,
                daily_depositor_addresses: 0,
                compensation: DaoCompensationBreakdown::default(),
            },
        )
        .unwrap_err();
        assert!(wrong_day.to_string().contains("different day"));
        let err = reduce_dao_daily_snapshot(
            None,
            DaoSnapshotDelta {
                active_capacity: i128::MAX,
                protocol_capacity: i128::MAX,
                ..Default::default()
            },
            DaoSnapshotObservation {
                kind: DaoObservationKind::SealedDay,
                date,
                block_number: 3,
                chain_time_ms: 1_726_330_000_000,
                dao_field: &dao,
                daily_depositor_addresses: 0,
                compensation: DaoCompensationBreakdown::default(),
            },
        )
        .unwrap();
        assert_eq!(err.total_deposited, i128::MAX);
        assert!(reduce_dao_daily_snapshot(
            Some(&err),
            DaoSnapshotDelta {
                active_capacity: 1,
                protocol_capacity: 1,
                ..Default::default()
            },
            DaoSnapshotObservation {
                kind: DaoObservationKind::IncompleteTip,
                date,
                block_number: 4,
                chain_time_ms: 1_726_330_000_000,
                dao_field: &dao,
                daily_depositor_addresses: 0,
                compensation: DaoCompensationBreakdown::default(),
            }
        )
        .unwrap_err()
        .to_string()
        .contains("overflow"));
    }
}
