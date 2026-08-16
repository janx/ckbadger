use anyhow::{anyhow, bail, Result};

use ckbadger_store::LiveCellSummary;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LiveCellSummaryClass {
    Dao,
    TypedNonDao,
    Plain,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LiveCellSummaryProjection {
    class: LiveCellSummaryClass,
    data_bearing: bool,
}

impl LiveCellSummaryProjection {
    pub(crate) fn from_parts(
        has_type_script: bool,
        has_type_code_hash: bool,
        is_dao: bool,
        data_size: i32,
        context: CellSummaryContext<'_>,
    ) -> Result<Self> {
        if has_type_script != has_type_code_hash {
            bail!(
                "live-cell summary type-script projection mismatch: {} has_type_script={} has_type_code_hash={}",
                context,
                has_type_script,
                has_type_code_hash,
            );
        }
        if is_dao && !has_type_script {
            bail!(
                "live-cell summary DAO classification without a type script: {}",
                context
            );
        }
        if data_size < 0 {
            bail!(
                "live-cell summary saw negative cell data size: {} data_size={}",
                context,
                data_size,
            );
        }

        let class = if is_dao {
            LiveCellSummaryClass::Dao
        } else if has_type_script {
            LiveCellSummaryClass::TypedNonDao
        } else {
            LiveCellSummaryClass::Plain
        };
        Ok(Self {
            class,
            data_bearing: data_size > 0,
        })
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct CellSummaryContext<'a> {
    transition: &'static str,
    block_number: i64,
    tx_hash: &'a [u8],
    output_index: i64,
}

impl<'a> CellSummaryContext<'a> {
    pub(crate) fn new(
        transition: &'static str,
        block_number: i64,
        tx_hash: &'a [u8],
        output_index: i64,
    ) -> Self {
        Self {
            transition,
            block_number,
            tx_hash,
            output_index,
        }
    }
}

impl std::fmt::Display for CellSummaryContext<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "transition={} block={} outpoint=0x{}:{}",
            self.transition,
            self.block_number,
            hex::encode(self.tx_hash),
            self.output_index,
        )
    }
}

/// Small exact reducer shared by normal and bulk sync. It has no collection
/// proportional to chain size; each transition is constant work.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LiveCellSummaryCounter {
    dao: u64,
    typed_non_dao: u64,
    plain: u64,
    data_bearing: u64,
}

impl LiveCellSummaryCounter {
    pub(crate) fn from_summary(summary: &LiveCellSummary) -> Result<Self> {
        summary.validate()?;
        Ok(Self {
            dao: summary.dao,
            typed_non_dao: summary.typed_non_dao,
            plain: summary.plain,
            data_bearing: summary.data_bearing,
        })
    }

    pub(crate) fn after_birth(
        self,
        projection: LiveCellSummaryProjection,
        context: CellSummaryContext<'_>,
    ) -> Result<Self> {
        self.after_transition(projection, context, true)
    }

    pub(crate) fn after_spend(
        self,
        projection: LiveCellSummaryProjection,
        context: CellSummaryContext<'_>,
    ) -> Result<Self> {
        self.after_transition(projection, context, false)
    }

    fn after_transition(
        mut self,
        projection: LiveCellSummaryProjection,
        context: CellSummaryContext<'_>,
        is_birth: bool,
    ) -> Result<Self> {
        let class_counter = match projection.class {
            LiveCellSummaryClass::Dao => &mut self.dao,
            LiveCellSummaryClass::TypedNonDao => &mut self.typed_non_dao,
            LiveCellSummaryClass::Plain => &mut self.plain,
        };
        *class_counter = update_counter(*class_counter, is_birth, "class", context)?;
        if projection.data_bearing {
            self.data_bearing =
                update_counter(self.data_bearing, is_birth, "data_bearing", context)?;
        }
        Ok(self)
    }

    pub(crate) fn snapshot(self, block_number: i64, block_hash: &[u8]) -> Result<LiveCellSummary> {
        let tip_block_hash: [u8; 32] = block_hash.try_into().map_err(|_| {
            anyhow!(
                "live-cell summary block hash must be exactly 32 bytes: block={} actual_bytes={}",
                block_number,
                block_hash.len(),
            )
        })?;
        let summary = LiveCellSummary {
            tip_block_number: block_number,
            tip_block_hash,
            dao: self.dao,
            typed_non_dao: self.typed_non_dao,
            plain: self.plain,
            data_bearing: self.data_bearing,
        };
        summary.validate()?;
        Ok(summary)
    }
}

fn update_counter(
    current: u64,
    is_birth: bool,
    counter: &str,
    context: CellSummaryContext<'_>,
) -> Result<u64> {
    if is_birth {
        current.checked_add(1).ok_or_else(|| {
            anyhow!(
                "live-cell summary counter overflow: counter={} current={} {}",
                counter,
                current,
                context,
            )
        })
    } else {
        current.checked_sub(1).ok_or_else(|| {
            anyhow!(
                "live-cell summary counter underflow: counter={} current={} {}",
                counter,
                current,
                context,
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context(transition: &'static str) -> CellSummaryContext<'static> {
        CellSummaryContext::new(transition, 42, &[0xAB; 32], 1)
    }

    #[test]
    fn reducer_keeps_classes_mutually_exclusive_and_data_orthogonal() {
        assert_eq!(std::mem::size_of::<LiveCellSummaryCounter>(), 32);
        let plain = LiveCellSummaryProjection::from_parts(false, false, false, 0, context("birth"))
            .unwrap();
        let typed =
            LiveCellSummaryProjection::from_parts(true, true, false, 16, context("birth")).unwrap();
        let dao =
            LiveCellSummaryProjection::from_parts(true, true, true, 8, context("birth")).unwrap();

        let counter = LiveCellSummaryCounter::default()
            .after_birth(plain, context("birth"))
            .unwrap()
            .after_birth(typed, context("birth"))
            .unwrap()
            .after_birth(dao, context("birth"))
            .unwrap();
        let summary = counter.snapshot(42, &[0x42; 32]).unwrap();

        assert_eq!(summary.live_cells().unwrap(), 3);
        assert_eq!(summary.dao, 1);
        assert_eq!(summary.typed_non_dao, 1);
        assert_eq!(summary.plain, 1);
        assert_eq!(summary.data_bearing, 2);
    }

    #[test]
    fn reducer_birth_then_spend_is_exact_and_underflow_fails() {
        let typed =
            LiveCellSummaryProjection::from_parts(true, true, false, 1, context("birth")).unwrap();
        let counter = LiveCellSummaryCounter::default()
            .after_birth(typed, context("birth"))
            .unwrap()
            .after_spend(typed, context("spend"))
            .unwrap();
        assert_eq!(
            counter
                .snapshot(42, &[0x42; 32])
                .unwrap()
                .live_cells()
                .unwrap(),
            0
        );

        let error = counter.after_spend(typed, context("spend")).unwrap_err();
        assert!(error.to_string().contains("counter underflow"));
        assert!(error.to_string().contains("block=42"));
    }

    #[test]
    fn projection_rejects_incomplete_type_metadata_and_negative_data() {
        assert!(
            LiveCellSummaryProjection::from_parts(true, false, false, 0, context("birth"))
                .unwrap_err()
                .to_string()
                .contains("projection mismatch")
        );
        assert!(
            LiveCellSummaryProjection::from_parts(false, false, false, -1, context("birth"))
                .unwrap_err()
                .to_string()
                .contains("negative cell data size")
        );
    }
}
