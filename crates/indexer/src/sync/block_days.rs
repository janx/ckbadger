//! The UTC+8 stats day of a block, for the HODL wave and cell-distribution
//! trackers (store refactor lane B, B-4).
//!
//! A consumed cell leaves the HODL capacity of its creation day, and an
//! address's cohort is the month of its first-seen block, so both trackers
//! need the day of an earlier block. That day is `stats_day` of the block's
//! canonical chain time, the value its stored header carries
//! (`CachedBlockHeader.chain_time_ms`, written by `insert_blocks_batch` from
//! the batch's running-maximum clock) — there is no second record of it.
//!
//! - Live sync ([`LiveBlockDays`]): a block of the batch being written takes
//!   its day from the batch's own chain times (the headers it is about to
//!   write); an earlier block from its stored header, read once per batch.
//! - Bulk build ([`BulkBlockDays`]): the run reads no store, so it keeps the
//!   day of every block it has seen, as day transitions, in memory only.

use std::collections::HashMap;

use anyhow::{anyhow, bail, Result};
use chrono::NaiveDate;
use ckbadger_common::stats_bucket::stats_day;
use ckbadger_store::CkbadgerStore;

/// Block days for one live batch.
pub(crate) struct LiveBlockDays<'a> {
    store: &'a CkbadgerStore,
    first_block: i64,
    /// Day of each block of the batch, by block number.
    batch_days: HashMap<i64, NaiveDate>,
    /// Days of earlier blocks read from their stored headers in this batch.
    stored_days: HashMap<i64, NaiveDate>,
}

impl<'a> LiveBlockDays<'a> {
    /// `blocks` are the batch's `(block number, chain time)` pairs — the
    /// chain times its headers are written with.
    pub(crate) fn new(
        store: &'a CkbadgerStore,
        blocks: impl IntoIterator<Item = (i64, i64)>,
    ) -> Result<Self> {
        let mut batch_days = HashMap::new();
        let mut first_block: Option<i64> = None;
        for (number, chain_time_ms) in blocks {
            if batch_days
                .insert(number, stats_day(chain_time_ms)?)
                .is_some()
            {
                bail!("block {number} appears twice in the live batch");
            }
            first_block = Some(first_block.map_or(number, |first| first.min(number)));
        }
        let first_block =
            first_block.ok_or_else(|| anyhow!("empty live batch has no block days"))?;
        Ok(Self {
            store,
            first_block,
            batch_days,
            stored_days: HashMap::new(),
        })
    }

    /// The UTC+8 stats day of `block`: a block of this batch or an earlier
    /// canonical block. A block above the batch, or an earlier block without a
    /// stored header, is an error.
    pub(crate) fn day_of(&mut self, block: i64) -> Result<NaiveDate> {
        if block >= self.first_block {
            return self.batch_days.get(&block).copied().ok_or_else(|| {
                anyhow!(
                    "block {block} is neither in the live batch starting at {} nor below it",
                    self.first_block
                )
            });
        }
        if let Some(day) = self.stored_days.get(&block) {
            return Ok(*day);
        }
        if block < 0 {
            bail!("negative block number {block} has no day");
        }
        let header = self.store.get_block_header(block)?.ok_or_else(|| {
            anyhow!(
                "missing canonical header for block {block} (below the live batch starting at {}): its stats day is unknown",
                self.first_block
            )
        })?;
        let day = stats_day(header.chain_time_ms)?;
        self.stored_days.insert(block, day);
        Ok(day)
    }
}

/// Block days of a bulk run, recorded block by block in ascending order and
/// kept as day transitions (one entry per day).
#[derive(Debug, Default)]
pub(crate) struct BulkBlockDays {
    /// `(first block of the day, day)`, ascending in both.
    transitions: Vec<(i64, NaiveDate)>,
    last_block: Option<i64>,
}

impl BulkBlockDays {
    /// Record the next block of the run and its day. Blocks arrive
    /// consecutively and days never decrease (the chain time is a running
    /// maximum).
    pub(crate) fn record(&mut self, block: i64, day: NaiveDate) -> Result<()> {
        if let Some(last) = self.last_block {
            if block != last + 1 {
                bail!("bulk block days: block {block} recorded after block {last}");
            }
        }
        match self.transitions.last() {
            Some((_, last_day)) if *last_day == day => {}
            Some((_, last_day)) if *last_day > day => bail!(
                "bulk block days: block {block} has day {day} before the previous block's {last_day}"
            ),
            _ => self.transitions.push((block, day)),
        }
        self.last_block = Some(block);
        Ok(())
    }

    /// The day of a recorded block; a block outside the recorded range is an
    /// error.
    pub(crate) fn day_of(&self, block: i64) -> Result<NaiveDate> {
        let (Some((first, _)), Some(last)) = (self.transitions.first(), self.last_block) else {
            bail!("bulk block days: no block recorded, day of block {block} unknown");
        };
        if block < *first || block > last {
            bail!("bulk block days: block {block} is outside the recorded blocks {first}..={last}");
        }
        let idx = self.transitions.partition_point(|(b, _)| *b <= block);
        Ok(self.transitions[idx - 1].1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ckbadger_store::batch::StoreBatch;
    use ckbadger_store::CachedBlockHeader;

    fn day(text: &str) -> NaiveDate {
        NaiveDate::parse_from_str(text, "%Y%m%d").unwrap()
    }

    // 2024-01-15 23:59:00 and 2024-01-16 00:01:00 UTC+8.
    const JAN15_LATE: i64 = 1_705_334_340_000;
    const JAN16_EARLY: i64 = 1_705_334_460_000;

    fn header(chain_time_ms: i64) -> CachedBlockHeader {
        CachedBlockHeader {
            hash: vec![0; 32],
            timestamp: chain_time_ms,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
            dao: vec![0; 32],
            transactions_count: 1,
            uncles_count: 0,
            proposals_count: 0,
            compact_target: 0,
            miner_lock_hash: None,
            cycles: None,
            chain_time_ms,
        }
    }

    #[test]
    fn live_days_come_from_the_batch_and_the_stored_headers() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        let mut batch = StoreBatch::new(&store);
        batch.put_block_header(8, &header(JAN15_LATE));
        batch.commit().unwrap();

        let mut days = LiveBlockDays::new(&store, [(10, JAN15_LATE), (11, JAN16_EARLY)]).unwrap();
        assert_eq!(days.day_of(10).unwrap(), day("20240115"));
        assert_eq!(days.day_of(11).unwrap(), day("20240116"));
        assert_eq!(days.day_of(8).unwrap(), day("20240115"));
        // Cached: a second read of block 8 needs no header.
        store
            .delete_cf(
                store.cf_block_headers(),
                &ckbadger_store::keys::encode_block_num(8),
            )
            .unwrap();
        assert_eq!(days.day_of(8).unwrap(), day("20240115"));

        let err = days.day_of(9).unwrap_err();
        assert!(
            err.to_string()
                .contains("missing canonical header for block 9"),
            "{err}"
        );
        let err = days.day_of(12).unwrap_err();
        assert!(
            err.to_string().contains("neither in the live batch"),
            "{err}"
        );
    }

    #[test]
    fn bulk_days_resolve_recorded_blocks_only() {
        let mut days = BulkBlockDays::default();
        assert!(days.day_of(0).is_err());
        days.record(100, day("20240115")).unwrap();
        days.record(101, day("20240115")).unwrap();
        days.record(102, day("20240116")).unwrap();
        days.record(103, day("20240118")).unwrap();
        assert_eq!(days.day_of(100).unwrap(), day("20240115"));
        assert_eq!(days.day_of(101).unwrap(), day("20240115"));
        assert_eq!(days.day_of(102).unwrap(), day("20240116"));
        assert_eq!(days.day_of(103).unwrap(), day("20240118"));
        assert_eq!(days.transitions.len(), 3, "one transition per day");
        assert!(days.day_of(99).is_err(), "no block before the run's first");
        assert!(
            days.day_of(104).is_err(),
            "no block after the last recorded"
        );

        assert!(
            days.record(105, day("20240118")).is_err(),
            "blocks are consecutive"
        );
        assert!(
            days.record(104, day("20240117")).is_err(),
            "days never decrease"
        );
    }
}
