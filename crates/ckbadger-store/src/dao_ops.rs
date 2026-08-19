//! DAO operations.

use rocksdb::{ColumnFamily, IteratorMode, Snapshot};
use std::collections::HashMap;
#[cfg(test)]
use std::sync::{Mutex, OnceLock};

use ckbadger_common::dao::calculate_dao_compensation_from_ar;

use crate::keys;
use crate::store::{CkbadgerStore, KvResult};
use crate::types::{DaoCompensationBreakdown, DaoDepositCacheEntry};

const DAO_BY_BLOCK_OUTPOINT_OFFSET: usize = 8;
const DAO_BY_STATUS_OUTPOINT_OFFSET: usize = 10;
const DAO_BY_LOCK_OUTPOINT_OFFSET: usize = 40;

/// One view of the DAO tables, held for a whole listing.
///
/// A primary is written concurrently by the indexer, so its view has to be a
/// RocksDB snapshot. A secondary cannot snapshot at all — `GetSnapshot` there
/// fails with "not supported in secondary mode" — so its view is pinned by
/// [`crate::read_view`] instead (the API holds one pin per request) and reads
/// go straight to the DB.
///
/// Having both behind one type is the point: the index-row/entry contract is
/// then enforced in exactly one place, against whichever view the caller has.
enum DaoReadView<'a> {
    Snapshot(Snapshot<'a>),
    /// Reads go straight to the DB. Sound only where the view cannot move
    /// mid-read: a secondary inside a pinned read scope, or the indexer's own
    /// writer thread, which is the sole writer and cannot race itself.
    Pinned,
}

impl<'a> DaoReadView<'a> {
    /// The strongest view this store can give.
    fn open(store: &'a CkbadgerStore) -> Self {
        if store.is_secondary() {
            Self::Pinned
        } else {
            Self::Snapshot(store.snapshot())
        }
    }

    fn iterator_cf(
        &self,
        store: &'a CkbadgerStore,
        cf: &ColumnFamily,
        mode: IteratorMode<'_>,
    ) -> Box<dyn Iterator<Item = KvResult> + '_> {
        match self {
            Self::Snapshot(snapshot) => Box::new(snapshot.iterator_cf(cf, mode)),
            Self::Pinned => Box::new(store.iterator_cf(cf, mode)),
        }
    }

    fn load_dao_entry(
        &self,
        store: &CkbadgerStore,
        outpoint_key: &[u8],
        index_name: &str,
        index_key: &[u8],
    ) -> anyhow::Result<DaoDepositCacheEntry> {
        match self {
            Self::Snapshot(snapshot) => store.load_dao_entry_for_index_from_snapshot(
                snapshot,
                outpoint_key,
                index_name,
                index_key,
            ),
            Self::Pinned => store.load_dao_entry_for_index(outpoint_key, index_name, index_key),
        }
    }
}

use crate::bytes_to_hex;

/// Post-batch DAO deposit entries staged in an uncommitted batch, keyed by
/// deposit outpoint. Covers new deposits and phase-1 withdraw requests, whose
/// full post-batch entry the writer already materializes.
pub type StagedDaoEntries = HashMap<Vec<u8>, DaoDepositCacheEntry>;

/// Phase-2 completions staged in an uncommitted batch, keyed by deposit
/// outpoint, valued by `(withdraw_block, withdraw_tx_hash)`. Kept separate from
/// [`StagedDaoEntries`] so the caller never has to reconstruct the completed
/// entry — and therefore never duplicates the compensation calculation.
pub type StagedDaoCompletions = HashMap<Vec<u8>, (i64, Vec<u8>)>;

/// Compensation frozen at a deposit's phase-1 withdraw-request AR.
///
/// This is the single definition of the frozen value. `dao_compensation_for_entry_at`
/// uses it for both phase-1 and completed deposits (validating any stored
/// compensation against it), and pre-commit snapshot construction uses it when
/// projecting a staged phase-2 completion.
pub fn dao_frozen_request_compensation(entry: &DaoDepositCacheEntry) -> anyhow::Result<i64> {
    let deposit_ar = u64::try_from(entry.deposit_ar).map_err(|_| {
        anyhow::anyhow!(
            "negative DAO deposit AR: deposit_block={}, deposit_ar={}",
            entry.deposit_block_number,
            entry.deposit_ar
        )
    })?;
    let request_ar = entry.withdraw_request_ar.ok_or_else(|| {
        anyhow::anyhow!(
            "DAO deposit missing withdraw request AR: deposit_block={}, request_block={:?}",
            entry.deposit_block_number,
            entry.withdraw_request_block
        )
    })?;
    let request_ar = u64::try_from(request_ar).map_err(|_| {
        anyhow::anyhow!(
            "negative DAO withdraw request AR: deposit_block={}, request_block={}, withdraw_request_ar={}",
            entry.deposit_block_number,
            entry.withdraw_request_block.unwrap_or_default(),
            request_ar
        )
    })?;
    // RFC-0023 derives `counted_capacity` from the WITHDRAWING cell — the
    // phase-1 request cell — not from the original deposit cell. The DAO type
    // script does not enforce lock preservation, so a request may carry a
    // different lock and therefore a different occupied capacity.
    let request_occupied_capacity =
        entry.withdraw_request_occupied_capacity.ok_or_else(|| {
            anyhow::anyhow!(
                "DAO deposit missing withdraw request occupied capacity: deposit_block={}, request_block={:?}, status={}",
                entry.deposit_block_number,
                entry.withdraw_request_block,
                entry.status
            )
        })?;
    calculate_dao_compensation_from_ar(
        entry.capacity,
        request_occupied_capacity,
        deposit_ar,
        request_ar,
    )
}

/// Project a committed phase-1 deposit forward through a phase-2 completion
/// that is staged in an uncommitted batch.
///
/// Read-only projection for pre-commit snapshot construction: the persisted
/// entry is written by the indexer's DAO writer, never by this function. The
/// claimed amount is the frozen request-AR value, so the projected entry
/// satisfies the same validation `dao_compensation_for_entry_at` applies to a
/// committed completed deposit.
fn dao_entry_with_staged_completion(
    entry: &DaoDepositCacheEntry,
    withdraw_block: i64,
    withdraw_tx: &[u8],
) -> anyhow::Result<DaoDepositCacheEntry> {
    if entry.status != 1 {
        anyhow::bail!(
            "staged DAO completion applied to a non phase-1 deposit: deposit_block={}, status={}, withdraw_block={}",
            entry.deposit_block_number,
            entry.status,
            withdraw_block
        );
    }
    let request_block = entry.withdraw_request_block.ok_or_else(|| {
        anyhow::anyhow!(
            "staged DAO completion on a deposit without a withdraw request block: deposit_block={}, withdraw_block={}",
            entry.deposit_block_number,
            withdraw_block
        )
    })?;
    if withdraw_block <= request_block {
        anyhow::bail!(
            "staged DAO completion block is not after the withdraw request: deposit_block={}, request_block={}, withdraw_block={}",
            entry.deposit_block_number,
            request_block,
            withdraw_block
        );
    }
    let compensation = dao_frozen_request_compensation(entry)?;
    Ok(DaoDepositCacheEntry {
        status: 2,
        withdraw_block: Some(withdraw_block),
        withdraw_tx: Some(withdraw_tx.to_vec()),
        compensation: Some(compensation),
        ..entry.clone()
    })
}

pub fn dao_compensation_for_entry_at(
    entry: &DaoDepositCacheEntry,
    end_block: i64,
    end_ar: u64,
) -> anyhow::Result<DaoCompensationBreakdown> {
    if entry.capacity < 0 || entry.occupied_capacity < 0 || entry.occupied_capacity > entry.capacity
    {
        anyhow::bail!(
            "invalid DAO deposit capacity in compensation lifecycle: deposit_block={}, capacity={}, occupied_capacity={}, observation_block={}",
            entry.deposit_block_number,
            entry.capacity,
            entry.occupied_capacity,
            end_block
        );
    }
    match entry.status {
        0 => {
            if entry.withdraw_request_tx.is_some()
                || entry.withdraw_request_output_index.is_some()
                || entry.withdraw_request_block.is_some()
                || entry.withdraw_request_ar.is_some()
                || entry.withdraw_block.is_some()
                || entry.withdraw_tx.is_some()
                || entry.compensation.is_some()
            {
                anyhow::bail!(
                    "status-0 DAO deposit has lifecycle fields: deposit_block={}, observation_block={}",
                    entry.deposit_block_number,
                    end_block
                );
            }
        }
        1 | 2 => {
            let request_block = entry.withdraw_request_block.ok_or_else(|| {
                anyhow::anyhow!(
                    "DAO status-{} deposit missing withdraw request block: deposit_block={}, observation_block={}",
                    entry.status,
                    entry.deposit_block_number,
                    end_block
                )
            })?;
            if request_block < entry.deposit_block_number
                || entry.withdraw_request_tx.is_none()
                || entry.withdraw_request_output_index.is_none()
                || entry.withdraw_request_ar.is_none()
            {
                anyhow::bail!(
                    "invalid DAO status-{} request lifecycle: deposit_block={}, request_block={}, has_request_tx={}, has_request_output={}, has_request_ar={}, observation_block={}",
                    entry.status,
                    entry.deposit_block_number,
                    request_block,
                    entry.withdraw_request_tx.is_some(),
                    entry.withdraw_request_output_index.is_some(),
                    entry.withdraw_request_ar.is_some(),
                    end_block
                );
            }
            if entry.status == 1
                && (entry.withdraw_block.is_some()
                    || entry.withdraw_tx.is_some()
                    || entry.compensation.is_some())
            {
                anyhow::bail!(
                    "status-1 DAO deposit has completion fields: deposit_block={}, request_block={}, observation_block={}",
                    entry.deposit_block_number,
                    request_block,
                    end_block
                );
            }
            if entry.status == 2 {
                let withdraw_block = entry.withdraw_block.ok_or_else(|| {
                    anyhow::anyhow!(
                        "completed DAO deposit missing withdraw block: deposit_block={}, request_block={}, observation_block={}",
                        entry.deposit_block_number,
                        request_block,
                        end_block
                    )
                })?;
                if withdraw_block <= request_block
                    || entry.withdraw_tx.is_none()
                    || entry.compensation.is_none()
                {
                    anyhow::bail!(
                        "invalid completed DAO lifecycle: deposit_block={}, request_block={}, withdraw_block={}, has_withdraw_tx={}, has_compensation={}, observation_block={}",
                        entry.deposit_block_number,
                        request_block,
                        withdraw_block,
                        entry.withdraw_tx.is_some(),
                        entry.compensation.is_some(),
                        end_block
                    );
                }
            }
        }
        status => {
            anyhow::bail!(
                "unknown DAO deposit status in compensation lifecycle: status={}, deposit_block={}, observation_block={}",
                status,
                entry.deposit_block_number,
                end_block
            );
        }
    }

    let deposit_ar = u64::try_from(entry.deposit_ar).map_err(|_| {
        anyhow::anyhow!(
            "negative DAO deposit AR: deposit_block={}, deposit_ar={}, observation_block={}",
            entry.deposit_block_number,
            entry.deposit_ar,
            end_block
        )
    })?;

    if entry.deposit_block_number > end_block {
        return Ok(DaoCompensationBreakdown::default());
    }

    let frozen_compensation = || -> anyhow::Result<i64> {
        dao_frozen_request_compensation(entry)
            .map_err(|error| anyhow::anyhow!("{} (observation_block={})", error, end_block))
    };

    if entry.withdraw_block.is_some_and(|block| block <= end_block) {
        let claimed = entry.compensation.ok_or_else(|| {
            anyhow::anyhow!(
                "completed DAO deposit missing compensation: deposit_block={}, withdraw_block={}, observation_block={}",
                entry.deposit_block_number,
                entry.withdraw_block.unwrap_or_default(),
                end_block
            )
        })?;
        if claimed < 0 {
            anyhow::bail!(
                "completed DAO deposit has negative compensation: deposit_block={}, withdraw_block={}, compensation={}",
                entry.deposit_block_number,
                entry.withdraw_block.unwrap_or_default(),
                claimed
            );
        }
        let expected = frozen_compensation()?;
        if claimed != expected {
            anyhow::bail!(
                "stored DAO compensation differs from exact request-AR calculation: deposit_block={}, request_block={}, withdraw_block={}, stored={}, expected={}",
                entry.deposit_block_number,
                entry.withdraw_request_block.unwrap_or_default(),
                entry.withdraw_block.unwrap_or_default(),
                claimed,
                expected
            );
        }
        return Ok(DaoCompensationBreakdown {
            claimed: i128::from(claimed),
            ..Default::default()
        });
    }

    let request_has_happened = entry
        .withdraw_request_block
        .is_some_and(|block| block <= end_block);
    let compensation = if request_has_happened {
        let expected = frozen_compensation()?;
        if let Some(stored) = entry.compensation {
            if stored < 0 {
                anyhow::bail!(
                    "phase-1 DAO deposit has negative stored compensation: deposit_block={}, request_block={}, compensation={}",
                    entry.deposit_block_number,
                    entry.withdraw_request_block.unwrap_or_default(),
                    stored
                );
            }
            if stored != expected {
                anyhow::bail!(
                    "stored DAO compensation differs from exact request-AR calculation: deposit_block={}, request_block={}, stored={}, expected={}, observation_block={}",
                    entry.deposit_block_number,
                    entry.withdraw_request_block.unwrap_or_default(),
                    stored,
                    expected,
                    end_block
                );
            }
        }
        expected
    } else {
        calculate_dao_compensation_from_ar(
            entry.capacity,
            entry.occupied_capacity,
            deposit_ar,
            end_ar,
        )?
    };

    Ok(DaoCompensationBreakdown {
        unclaimed: i128::from(compensation),
        active_unmade: if request_has_happened {
            0
        } else {
            i128::from(compensation)
        },
        ..Default::default()
    })
}

#[cfg(test)]
type DaoStatusPaginationHook = Box<dyn Fn(&CkbadgerStore, &[u8]) + Send + Sync + 'static>;

#[cfg(test)]
fn dao_status_pagination_hook_cell() -> &'static Mutex<Option<DaoStatusPaginationHook>> {
    static CELL: OnceLock<Mutex<Option<DaoStatusPaginationHook>>> = OnceLock::new();
    CELL.get_or_init(|| Mutex::new(None))
}

#[cfg(test)]
fn set_dao_status_pagination_hook(hook: Option<DaoStatusPaginationHook>) {
    let mut guard = dao_status_pagination_hook_cell()
        .lock()
        .expect("dao_status_pagination_hook lock poisoned");
    *guard = hook;
}

#[cfg(test)]
fn run_dao_status_pagination_hook(store: &CkbadgerStore, outpoint_key: &[u8]) {
    let hook = {
        let mut guard = dao_status_pagination_hook_cell()
            .lock()
            .expect("dao_status_pagination_hook lock poisoned");
        guard.take()
    };
    if let Some(hook) = hook {
        hook(store, outpoint_key);
    }
}

impl CkbadgerStore {
    fn load_dao_entry_for_index(
        &self,
        outpoint_key: &[u8],
        index_name: &str,
        index_key: &[u8],
    ) -> anyhow::Result<DaoDepositCacheEntry> {
        match self.get_dao_deposit(outpoint_key)? {
            Some(entry) => Ok(entry),
            None => anyhow::bail!(
                "stale {} index points to missing dao deposit: index_key=0x{}, outpoint_key=0x{}",
                index_name,
                bytes_to_hex(index_key),
                bytes_to_hex(outpoint_key)
            ),
        }
    }

    fn load_dao_entry_for_index_from_snapshot(
        &self,
        snapshot: &Snapshot<'_>,
        outpoint_key: &[u8],
        index_name: &str,
        index_key: &[u8],
    ) -> anyhow::Result<DaoDepositCacheEntry> {
        match snapshot.get_cf(self.cf_dao_deposits(), outpoint_key)? {
            Some(value) => Ok(bincode::deserialize(&value).map_err(|e| {
                anyhow::anyhow!(
                    "failed to deserialize dao deposit entry while reading {} index: index_key=0x{}, outpoint_key=0x{}, error={}",
                    index_name,
                    bytes_to_hex(index_key),
                    bytes_to_hex(outpoint_key),
                    e
                )
            })?),
            None => anyhow::bail!(
                "stale {} index points to missing dao deposit: index_key=0x{}, outpoint_key=0x{}",
                index_name,
                bytes_to_hex(index_key),
                bytes_to_hex(outpoint_key)
            ),
        }
    }

    /// Decode one `dao_by_status_block` row and load the deposit it indexes,
    /// enforcing that key and entry agree.
    ///
    /// Both readers of this CF — the paginated listing and the full scan — go
    /// through here, so the index/entry contract has a single definition. The
    /// bug this guards against is shaped exactly like a fix landing on one copy
    /// of the checks and missing the others.
    fn resolve_dao_status_index_row(
        &self,
        view: &DaoReadView<'_>,
        key: &[u8],
        status: i16,
    ) -> anyhow::Result<(Vec<u8>, DaoDepositCacheEntry)> {
        anyhow::ensure!(
            key.len() == keys::DAO_BY_STATUS_BLOCK_KEY_SIZE,
            "invalid dao_by_status_block key length: expected {}, got {}",
            keys::DAO_BY_STATUS_BLOCK_KEY_SIZE,
            key.len()
        );

        let indexed_status = i16::from_be_bytes(
            key[..2]
                .try_into()
                .expect("status key prefix should be exactly 2 bytes"),
        );
        let deposit_block = i64::MAX
            - i64::from_be_bytes(
                key[2..10]
                    .try_into()
                    .expect("status key block section should be exactly 8 bytes"),
            );
        let outpoint_key = &key[DAO_BY_STATUS_OUTPOINT_OFFSET
            ..DAO_BY_STATUS_OUTPOINT_OFFSET + keys::OUTPOINT_KEY_SIZE];
        #[cfg(test)]
        run_dao_status_pagination_hook(self, outpoint_key);

        let entry = view.load_dao_entry(self, outpoint_key, "dao_by_status_block", key)?;
        anyhow::ensure!(
            indexed_status == status,
            "dao_by_status_block prefix mismatch: expected status={}, got status={}, index_key=0x{}",
            status,
            indexed_status,
            bytes_to_hex(key)
        );
        anyhow::ensure!(
            entry.status == status,
            "dao_by_status_block stale status: expected={}, actual={}, outpoint_key=0x{}",
            status,
            entry.status,
            bytes_to_hex(outpoint_key)
        );
        anyhow::ensure!(
            entry.deposit_block_number == deposit_block,
            "dao_by_status_block stale block: index_block={}, entry_block={}, outpoint_key=0x{}",
            deposit_block,
            entry.deposit_block_number,
            bytes_to_hex(outpoint_key)
        );

        Ok((outpoint_key.to_vec(), entry))
    }

    fn delete_dao_secondary_indexes_direct(
        &self,
        outpoint_key: &[u8],
        entry: &DaoDepositCacheEntry,
    ) -> anyhow::Result<()> {
        let by_block_key = keys::encode_dao_by_block_key(entry.deposit_block_number, outpoint_key);
        let by_lock_key = keys::encode_dao_by_lock_block_key(
            &entry.lock_script_hash,
            entry.deposit_block_number,
            outpoint_key,
        );
        let by_status_key = keys::encode_dao_by_status_block_key(
            entry.status,
            entry.deposit_block_number,
            outpoint_key,
        );

        self.delete_cf(self.cf_dao_by_block(), &by_block_key)?;
        self.delete_cf(self.cf_dao_by_lock_block(), &by_lock_key)?;
        self.delete_cf(self.cf_dao_by_status_block(), &by_status_key)?;
        Ok(())
    }

    fn put_dao_secondary_indexes_direct(
        &self,
        outpoint_key: &[u8],
        entry: &DaoDepositCacheEntry,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(
            outpoint_key.len() == keys::OUTPOINT_KEY_SIZE,
            "put_dao_secondary_indexes_direct expected outpoint {} bytes, got {}",
            keys::OUTPOINT_KEY_SIZE,
            outpoint_key.len()
        );
        anyhow::ensure!(
            entry.lock_script_hash.len() == 32,
            "put_dao_secondary_indexes_direct expected lock hash 32 bytes, got {}",
            entry.lock_script_hash.len()
        );

        let by_block_key = keys::encode_dao_by_block_key(entry.deposit_block_number, outpoint_key);
        let by_lock_key = keys::encode_dao_by_lock_block_key(
            &entry.lock_script_hash,
            entry.deposit_block_number,
            outpoint_key,
        );
        let by_status_key = keys::encode_dao_by_status_block_key(
            entry.status,
            entry.deposit_block_number,
            outpoint_key,
        );

        self.put_cf(self.cf_dao_by_block(), &by_block_key, &[])?;
        self.put_cf(self.cf_dao_by_lock_block(), &by_lock_key, &[])?;
        self.put_cf(self.cf_dao_by_status_block(), &by_status_key, &[])?;
        Ok(())
    }

    pub fn get_dao_deposit(
        &self,
        outpoint_key: &[u8],
    ) -> anyhow::Result<Option<DaoDepositCacheEntry>> {
        match self.get_cf(self.cf_dao_deposits(), outpoint_key)? {
            Some(value) => Ok(Some(bincode::deserialize(&value)?)),
            None => Ok(None),
        }
    }

    pub fn put_dao_deposit_direct(
        &self,
        outpoint_key: &[u8],
        entry: &DaoDepositCacheEntry,
    ) -> anyhow::Result<()> {
        if let Some(existing) = self.get_dao_deposit(outpoint_key)? {
            self.delete_dao_secondary_indexes_direct(outpoint_key, &existing)?;
        }

        let value = bincode::serialize(entry)?;
        self.put_cf(self.cf_dao_deposits(), outpoint_key, &value)?;
        self.put_dao_secondary_indexes_direct(outpoint_key, entry)
    }

    pub fn get_dao_deposit_by_withdraw_tx(
        &self,
        withdraw_tx_hash: &[u8],
        withdraw_output_index: i16,
    ) -> anyhow::Result<Option<Vec<u8>>> {
        let key = keys::encode_outpoint(withdraw_tx_hash, withdraw_output_index);
        self.get_cf(self.cf_dao_by_withdraw_tx(), &key)
    }

    /// Scan all DAO deposits (prefix scan) without materializing the full table in memory.
    pub fn scan_dao_deposits<F>(&self, mut visitor: F) -> anyhow::Result<()>
    where
        F: FnMut(&[u8], &DaoDepositCacheEntry) -> anyhow::Result<()>,
    {
        let iter = self.iterator_cf(self.cf_dao_deposits(), IteratorMode::Start);

        for item in iter {
            let (key, value) = item.map_err(|e| {
                anyhow::anyhow!("failed to iterate dao_deposits in scan_dao_deposits: {}", e)
            })?;
            let entry: DaoDepositCacheEntry = bincode::deserialize(&value).map_err(|e| {
                anyhow::anyhow!(
                    "failed to deserialize dao deposit entry in scan_dao_deposits: outpoint_key=0x{}, error={}",
                    bytes_to_hex(&key),
                    e
                )
            })?;
            visitor(&key, &entry)?;
        }
        Ok(())
    }

    /// Compute exact lifecycle-based DAO compensation at a historical block.
    ///
    /// Completed withdrawals contribute claimed compensation, phase-1 cells
    /// contribute compensation frozen at their request AR, and status-0 cells
    /// use the observation block's AR.
    pub fn compute_dao_compensation_breakdown_at(
        &self,
        end_block: i64,
        end_ar: u64,
    ) -> anyhow::Result<DaoCompensationBreakdown> {
        self.compute_dao_compensation_breakdown_at_with_staged(
            end_block,
            end_ar,
            &StagedDaoEntries::new(),
            &StagedDaoCompletions::new(),
        )
    }

    /// Exact lifecycle-based DAO compensation at a historical block, observing
    /// deposit mutations staged in an uncommitted batch.
    ///
    /// Staged rows shadow their committed counterparts, so a live-sync batch can
    /// materialize a completed day's snapshot from the same lifecycle state it
    /// is about to commit, inside the one atomic write — instead of writing a
    /// placeholder and correcting it after the commit.
    ///
    /// This is the single summation path: `compute_dao_compensation_breakdown_at`
    /// is this function with empty overlays.
    pub fn compute_dao_compensation_breakdown_at_with_staged(
        &self,
        end_block: i64,
        end_ar: u64,
        staged_entries: &StagedDaoEntries,
        staged_completions: &StagedDaoCompletions,
    ) -> anyhow::Result<DaoCompensationBreakdown> {
        let mut total = DaoCompensationBreakdown::default();
        let mut accumulate = |entry: &DaoDepositCacheEntry| -> anyhow::Result<()> {
            let contribution = dao_compensation_for_entry_at(entry, end_block, end_ar)?;
            total.claimed = total
                .claimed
                .checked_add(contribution.claimed)
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "claimed DAO compensation overflow at observation block {}",
                        end_block
                    )
                })?;
            total.unclaimed = total
                .unclaimed
                .checked_add(contribution.unclaimed)
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "unclaimed DAO compensation overflow at observation block {}",
                        end_block
                    )
                })?;
            total.active_unmade = total
                .active_unmade
                .checked_add(contribution.active_unmade)
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "active unmade DAO compensation overflow at observation block {}",
                        end_block
                    )
                })?;
            Ok(())
        };

        // The NervosDAO lock period puts at least 180 epochs between a withdraw
        // request and its completion, so no deposit can appear in both overlays.
        for outpoint_key in staged_completions.keys() {
            if staged_entries.contains_key(outpoint_key) {
                let (tx_hash, output_index) = keys::decode_outpoint(outpoint_key);
                anyhow::bail!(
                    "DAO deposit staged as both entry and completion in one batch: outpoint=0x{}:{}, observation_block={}",
                    bytes_to_hex(&tx_hash),
                    output_index,
                    end_block
                );
            }
        }

        let mut completions_observed = 0usize;
        self.scan_dao_deposits(|key, entry| {
            if staged_entries.contains_key(key) {
                // Shadowed by a staged entry; folded in below.
                return Ok(());
            }
            if let Some((withdraw_block, withdraw_tx)) = staged_completions.get(key) {
                completions_observed += 1;
                let completed =
                    dao_entry_with_staged_completion(entry, *withdraw_block, withdraw_tx)?;
                return accumulate(&completed);
            }
            accumulate(entry)
        })?;
        if completions_observed != staged_completions.len() {
            anyhow::bail!(
                "staged DAO completion refers to an uncommitted deposit: observed={}, staged={}, observation_block={}",
                completions_observed,
                staged_completions.len(),
                end_block
            );
        }
        for entry in staged_entries.values() {
            accumulate(entry)?;
        }
        Ok(total)
    }

    pub fn scan_dao_deposits_by_status<F>(&self, status: i16, mut visitor: F) -> anyhow::Result<()>
    where
        F: FnMut(&[u8], &DaoDepositCacheEntry) -> anyhow::Result<()>,
    {
        // Indexer-side scan: the caller is the sole writer thread, so the view
        // cannot move under it.
        let view = DaoReadView::Pinned;
        let prefix = keys::encode_dao_by_status_prefix(status);
        let iter = self.prefix_iterator_cf(self.cf_dao_by_status_block(), &prefix);
        for item in iter {
            let (key, _) = item.map_err(|e| {
                anyhow::anyhow!(
                    "failed to iterate dao_by_status_block in scan_dao_deposits_by_status: {}",
                    e
                )
            })?;
            if !key.starts_with(&prefix) {
                break;
            }

            let (outpoint_key, entry) = self.resolve_dao_status_index_row(&view, &key, status)?;
            visitor(&outpoint_key, &entry)?;
        }
        Ok(())
    }

    pub fn scan_dao_deposits_by_lock<F>(
        &self,
        lock_hash: &[u8],
        mut visitor: F,
    ) -> anyhow::Result<()>
    where
        F: FnMut(&[u8], &DaoDepositCacheEntry) -> anyhow::Result<()>,
    {
        anyhow::ensure!(
            lock_hash.len() == 32,
            "scan_dao_deposits_by_lock expected lock hash 32 bytes, got {}",
            lock_hash.len()
        );

        let prefix = keys::encode_dao_by_lock_prefix(lock_hash);
        let iter = self.prefix_iterator_cf(self.cf_dao_by_lock_block(), &prefix);
        for item in iter {
            let (key, _) = item.map_err(|e| {
                anyhow::anyhow!(
                    "failed to iterate dao_by_lock_block in scan_dao_deposits_by_lock: {}",
                    e
                )
            })?;
            if !key.starts_with(&prefix) {
                break;
            }
            anyhow::ensure!(
                key.len() == keys::DAO_BY_LOCK_BLOCK_KEY_SIZE,
                "invalid dao_by_lock_block key length: expected {}, got {}",
                keys::DAO_BY_LOCK_BLOCK_KEY_SIZE,
                key.len()
            );

            let deposit_block = i64::MAX
                - i64::from_be_bytes(
                    key[32..40]
                        .try_into()
                        .expect("lock index block section should be exactly 8 bytes"),
                );
            let outpoint_key = &key[DAO_BY_LOCK_OUTPOINT_OFFSET
                ..DAO_BY_LOCK_OUTPOINT_OFFSET + keys::OUTPOINT_KEY_SIZE];

            let entry = self.load_dao_entry_for_index(outpoint_key, "dao_by_lock_block", &key)?;
            anyhow::ensure!(
                entry.lock_script_hash.as_slice() == prefix.as_slice(),
                "dao_by_lock_block stale lock hash: expected=0x{}, actual=0x{}, outpoint_key=0x{}",
                bytes_to_hex(&prefix),
                bytes_to_hex(&entry.lock_script_hash),
                bytes_to_hex(outpoint_key)
            );
            anyhow::ensure!(
                entry.deposit_block_number == deposit_block,
                "dao_by_lock_block stale block: index_block={}, entry_block={}, outpoint_key=0x{}",
                deposit_block,
                entry.deposit_block_number,
                bytes_to_hex(outpoint_key)
            );

            visitor(outpoint_key, &entry)?;
        }
        Ok(())
    }

    /// Collect unique depositor lock_hashes for deposits at the given block numbers.
    /// Scans CF_DAO_BY_BLOCK for each block and loads the deposit entry to get lock_hash.
    pub fn collect_depositor_lock_hashes_for_blocks(
        &self,
        block_numbers: &[i64],
    ) -> anyhow::Result<std::collections::HashSet<Vec<u8>>> {
        let mut result = std::collections::HashSet::new();
        let cf = self.cf_dao_by_block();
        for &block_num in block_numbers {
            // CF_DAO_BY_BLOCK key = desc_block_num(8) + outpoint(34)
            let prefix = (i64::MAX - block_num).to_be_bytes();
            let iter = self.prefix_iterator_cf(cf, &prefix);
            for item in iter {
                let (key, _) = item.map_err(|e| {
                    anyhow::anyhow!(
                        "failed to iterate dao_by_block for block {}: {}",
                        block_num,
                        e
                    )
                })?;
                if key.len() != keys::DAO_BY_BLOCK_KEY_SIZE || !key.starts_with(&prefix) {
                    break;
                }
                let outpoint_key = &key[8..];
                let entry = self.load_dao_entry_for_index(outpoint_key, "dao_by_block", &key)?;
                // Only count deposits created at this block (not moved here by status update).
                if entry.deposit_block_number == block_num {
                    result.insert(entry.lock_script_hash);
                }
            }
        }
        Ok(result)
    }

    pub fn list_dao_deposits_paginated(
        &self,
        limit: usize,
        cursor_key_exclusive: Option<&[u8]>,
    ) -> anyhow::Result<Vec<(Vec<u8>, DaoDepositCacheEntry)>> {
        let mut rows = Vec::with_capacity(limit);
        if let Some(cursor_key) = cursor_key_exclusive {
            anyhow::ensure!(
                cursor_key.len() == keys::DAO_BY_BLOCK_KEY_SIZE,
                "invalid dao_by_block cursor key length: expected {}, got {}",
                keys::DAO_BY_BLOCK_KEY_SIZE,
                cursor_key.len()
            );
        }
        let iter_mode = if let Some(cursor_key) = cursor_key_exclusive {
            IteratorMode::From(cursor_key, rocksdb::Direction::Forward)
        } else {
            IteratorMode::Start
        };
        let iter = self.iterator_cf(self.cf_dao_by_block(), iter_mode);

        for item in iter {
            let (key, _) = item.map_err(|e| {
                anyhow::anyhow!("failed to iterate dao_by_block in pagination: {}", e)
            })?;
            anyhow::ensure!(
                key.len() == keys::DAO_BY_BLOCK_KEY_SIZE,
                "invalid dao_by_block key length: expected {}, got {}",
                keys::DAO_BY_BLOCK_KEY_SIZE,
                key.len()
            );
            if let Some(cursor_key) = cursor_key_exclusive {
                if key.as_ref() == cursor_key {
                    continue;
                }
            }

            let deposit_block = i64::MAX
                - i64::from_be_bytes(
                    key[..8]
                        .try_into()
                        .expect("block index prefix should be exactly 8 bytes"),
                );

            let outpoint_key = &key[DAO_BY_BLOCK_OUTPOINT_OFFSET
                ..DAO_BY_BLOCK_OUTPOINT_OFFSET + keys::OUTPOINT_KEY_SIZE];
            let entry = self.load_dao_entry_for_index(outpoint_key, "dao_by_block", &key)?;
            anyhow::ensure!(
                entry.deposit_block_number == deposit_block,
                "dao_by_block stale block: index_block={}, entry_block={}, outpoint_key=0x{}",
                deposit_block,
                entry.deposit_block_number,
                bytes_to_hex(outpoint_key)
            );

            rows.push((outpoint_key.to_vec(), entry));
            if rows.len() >= limit {
                break;
            }
        }

        Ok(rows)
    }

    pub fn list_dao_deposits_by_status_paginated(
        &self,
        status: i16,
        limit: usize,
        cursor_key_exclusive: Option<&[u8]>,
    ) -> anyhow::Result<Vec<(Vec<u8>, DaoDepositCacheEntry)>> {
        let mut rows = Vec::with_capacity(limit);
        let prefix = keys::encode_dao_by_status_prefix(status);
        if let Some(cursor_key) = cursor_key_exclusive {
            anyhow::ensure!(
                cursor_key.len() == keys::DAO_BY_STATUS_BLOCK_KEY_SIZE,
                "invalid dao_by_status_block cursor key length: expected {}, got {}",
                keys::DAO_BY_STATUS_BLOCK_KEY_SIZE,
                cursor_key.len()
            );
            anyhow::ensure!(
                cursor_key.starts_with(&prefix),
                "dao_by_status_block cursor does not match status prefix: status={}",
                status
            );
        }
        let start_key = cursor_key_exclusive.unwrap_or(prefix.as_slice());

        // One view for the whole page: the index row and the entry it points at
        // must come from the same one, or a concurrent status transition reads
        // as index corruption.
        let view = DaoReadView::open(self);
        let iter = view.iterator_cf(
            self,
            self.cf_dao_by_status_block(),
            IteratorMode::From(start_key, rocksdb::Direction::Forward),
        );

        for item in iter {
            let (key, _) = item.map_err(|e| {
                anyhow::anyhow!(
                    "failed to iterate dao_by_status_block in pagination: status={}, error={}",
                    status,
                    e
                )
            })?;
            if !key.starts_with(&prefix) {
                break;
            }
            if let Some(cursor_key) = cursor_key_exclusive {
                if key.as_ref() == cursor_key {
                    continue;
                }
            }

            let (outpoint_key, entry) = self.resolve_dao_status_index_row(&view, &key, status)?;
            rows.push((outpoint_key, entry));
            if rows.len() >= limit {
                break;
            }
        }

        Ok(rows)
    }

    pub fn list_dao_deposits_by_lock_paginated(
        &self,
        lock_hash: &[u8],
        limit: usize,
        cursor_key_exclusive: Option<&[u8]>,
    ) -> anyhow::Result<Vec<(Vec<u8>, DaoDepositCacheEntry)>> {
        anyhow::ensure!(
            lock_hash.len() == 32,
            "list_dao_deposits_by_lock_paginated expected lock hash 32 bytes, got {}",
            lock_hash.len()
        );

        let mut rows = Vec::with_capacity(limit);
        let prefix = keys::encode_dao_by_lock_prefix(lock_hash);
        if let Some(cursor_key) = cursor_key_exclusive {
            anyhow::ensure!(
                cursor_key.len() == keys::DAO_BY_LOCK_BLOCK_KEY_SIZE,
                "invalid dao_by_lock_block cursor key length: expected {}, got {}",
                keys::DAO_BY_LOCK_BLOCK_KEY_SIZE,
                cursor_key.len()
            );
            anyhow::ensure!(
                cursor_key.starts_with(&prefix),
                "dao_by_lock_block cursor does not match lock prefix: lock_hash=0x{}",
                bytes_to_hex(&prefix)
            );
        }
        let start_key = cursor_key_exclusive.unwrap_or(prefix.as_slice());
        let iter = self.iterator_cf(
            self.cf_dao_by_lock_block(),
            IteratorMode::From(start_key, rocksdb::Direction::Forward),
        );

        for item in iter {
            let (key, _) = item.map_err(|e| {
                anyhow::anyhow!(
                    "failed to iterate dao_by_lock_block in pagination: lock_hash=0x{}, error={}",
                    bytes_to_hex(&prefix),
                    e
                )
            })?;
            if !key.starts_with(&prefix) {
                break;
            }
            anyhow::ensure!(
                key.len() == keys::DAO_BY_LOCK_BLOCK_KEY_SIZE,
                "invalid dao_by_lock_block key length: expected {}, got {}",
                keys::DAO_BY_LOCK_BLOCK_KEY_SIZE,
                key.len()
            );
            if let Some(cursor_key) = cursor_key_exclusive {
                if key.as_ref() == cursor_key {
                    continue;
                }
            }

            let deposit_block = i64::MAX
                - i64::from_be_bytes(
                    key[32..40]
                        .try_into()
                        .expect("lock index block section should be exactly 8 bytes"),
                );

            let outpoint_key = &key[DAO_BY_LOCK_OUTPOINT_OFFSET
                ..DAO_BY_LOCK_OUTPOINT_OFFSET + keys::OUTPOINT_KEY_SIZE];
            let entry = self.load_dao_entry_for_index(outpoint_key, "dao_by_lock_block", &key)?;
            anyhow::ensure!(
                entry.lock_script_hash.as_slice() == prefix.as_slice(),
                "dao_by_lock_block stale lock hash: expected=0x{}, actual=0x{}, outpoint_key=0x{}",
                bytes_to_hex(&prefix),
                bytes_to_hex(&entry.lock_script_hash),
                bytes_to_hex(outpoint_key)
            );
            anyhow::ensure!(
                entry.deposit_block_number == deposit_block,
                "dao_by_lock_block stale block: index_block={}, entry_block={}, outpoint_key=0x{}",
                deposit_block,
                entry.deposit_block_number,
                bytes_to_hex(outpoint_key)
            );

            rows.push((outpoint_key.to_vec(), entry));
            if rows.len() >= limit {
                break;
            }
        }

        Ok(rows)
    }

    /// List all DAO deposits (prefix scan).
    pub fn list_dao_deposits(&self) -> anyhow::Result<Vec<(Vec<u8>, DaoDepositCacheEntry)>> {
        let mut results = Vec::new();
        self.scan_dao_deposits(|key, entry| {
            results.push((key.to_vec(), entry.clone()));
            Ok(())
        })?;
        Ok(results)
    }

    /// List active (status=0) DAO deposits.
    pub fn list_active_dao_deposits(&self) -> anyhow::Result<Vec<(Vec<u8>, DaoDepositCacheEntry)>> {
        let mut rows = Vec::new();
        self.scan_dao_deposits_by_status(0, |outpoint_key, entry| {
            rows.push((outpoint_key.to_vec(), entry.clone()));
            Ok(())
        })?;
        Ok(rows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::batch::StoreBatch;
    use anyhow::Context;
    use tempfile::TempDir;

    fn lifecycle_entry() -> DaoDepositCacheEntry {
        DaoDepositCacheEntry {
            capacity: 300_00000000,
            occupied_capacity: 142_00000000,
            deposit_block_number: 10,
            deposit_timestamp: 0,
            lock_script_hash: vec![0x11; 32],
            deposit_ar: 10_000,
            status: 2,
            withdraw_request_tx: Some(vec![0x22; 32]),
            withdraw_request_output_index: Some(0),
            withdraw_request_block: Some(20),
            withdraw_request_ar: Some(11_000),
            withdraw_block: Some(30),
            withdraw_tx: Some(vec![0x33; 32]),
            withdraw_request_occupied_capacity: Some(142_00000000),
            withdraw_to_output_index: Some(0),
            compensation: Some(15_80000000),
        }
    }

    /// Real mainnet vector: deposit consumed by phase-1 at block 6012563,
    /// completed at block 6201594. The depositor changed lock at phase-1 —
    /// the deposit cell carried a 33-byte-args lock (occupied 115 CKB) while
    /// the withdraw-request cell carries a standard 20-byte-args secp lock
    /// (occupied 102 CKB).
    ///
    /// Per RFC-0023 the protocol computes `counted_capacity` from the
    /// WITHDRAWING (phase-1 request) cell's occupied capacity, so the exact
    /// compensation is 2358516107 shannons. Using the deposit cell's
    /// occupied capacity instead yields 2357681847 — 834260 shannons short.
    ///
    /// Request outpoint:
    /// 0x5e883663a8e985f96102da878bc0e1c8fb9b39e194d911e542bdc0961407609b:0
    #[test]
    fn frozen_compensation_uses_the_withdraw_request_cells_occupied_capacity() {
        let entry = DaoDepositCacheEntry {
            capacity: 3_685_398_922_674,
            // Deposit cell: 8 + 33 + 33 (lock args) + 33 + 0 + 8 = 115 bytes.
            occupied_capacity: 115_00000000,
            deposit_block_number: 5_954_003,
            deposit_timestamp: 0,
            lock_script_hash: vec![0x11; 32],
            deposit_ar: 10_724_098_007_765_377,
            status: 1,
            withdraw_request_tx: Some(vec![0x22; 32]),
            withdraw_request_output_index: Some(0),
            withdraw_request_block: Some(6_012_563),
            withdraw_request_ar: Some(10_730_980_072_768_430),
            // Request cell: 8 + 33 + 20 (lock args) + 33 + 0 + 8 = 102 bytes.
            withdraw_request_occupied_capacity: Some(102_00000000),
            withdraw_block: None,
            withdraw_tx: None,
            withdraw_to_output_index: None,
            compensation: None,
        };

        assert_eq!(
            dao_frozen_request_compensation(&entry).unwrap(),
            2_358_516_107,
            "frozen compensation must use the withdraw-request cell's \
             occupied capacity (102 CKB), not the deposit cell's (115 CKB)"
        );
    }

    /// A phase-1/phase-2 entry without the request cell's occupied capacity
    /// cannot produce the protocol's exact compensation — it must fail loudly
    /// with lifecycle context rather than silently falling back to the
    /// deposit cell's occupied capacity.
    #[test]
    fn frozen_compensation_requires_the_request_occupied_capacity() {
        let mut entry = lifecycle_entry();
        entry.withdraw_request_occupied_capacity = None;

        let err = dao_frozen_request_compensation(&entry).unwrap_err();
        assert!(
            err.to_string()
                .contains("missing withdraw request occupied capacity"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn compensation_breakdown_follows_deposit_request_completion_lifecycle() {
        let entry = lifecycle_entry();

        let active = dao_compensation_for_entry_at(&entry, 15, 10_500).unwrap();
        assert_eq!(
            active,
            DaoCompensationBreakdown {
                claimed: 0,
                unclaimed: 7_90000000,
                active_unmade: 7_90000000,
            }
        );
        assert_eq!(active.frozen_phase1().unwrap(), 0);

        let phase1 = dao_compensation_for_entry_at(&entry, 25, 99_999).unwrap();
        assert_eq!(
            phase1,
            DaoCompensationBreakdown {
                claimed: 0,
                unclaimed: 15_80000000,
                active_unmade: 0,
            }
        );
        assert_eq!(phase1.frozen_phase1().unwrap(), 15_80000000);

        let completed = dao_compensation_for_entry_at(&entry, 30, 99_999).unwrap();
        assert_eq!(
            completed,
            DaoCompensationBreakdown {
                claimed: 15_80000000,
                unclaimed: 0,
                active_unmade: 0,
            }
        );
        assert_eq!(completed.frozen_phase1().unwrap(), 0);
    }

    #[test]
    fn compensation_breakdown_rejects_negative_claimed_compensation() {
        let mut entry = lifecycle_entry();
        entry.compensation = Some(-1);

        let error = dao_compensation_for_entry_at(&entry, 30, 99_999).unwrap_err();
        assert!(error.to_string().contains("negative compensation"));
    }

    #[test]
    fn compensation_breakdown_rejects_stored_value_that_differs_from_request_ar() {
        let mut entry = lifecycle_entry();
        entry.compensation = Some(15_80000001);

        let error = dao_compensation_for_entry_at(&entry, 30, 99_999).unwrap_err();
        assert!(error
            .to_string()
            .contains("differs from exact request-AR calculation"));
        assert!(error.to_string().contains("expected=1580000000"));
    }

    #[test]
    fn test_list_dao_deposits_fails_on_invalid_payload() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        let outpoint = keys::encode_outpoint(&[0xAB; 32], 1);
        store
            .put_cf(store.cf_dao_deposits(), &outpoint, b"invalid-dao-deposit")
            .unwrap();

        let err = store.list_dao_deposits().unwrap_err();
        assert!(err
            .to_string()
            .contains("failed to deserialize dao deposit entry in scan_dao_deposits"));
    }

    #[test]
    fn test_scan_dao_deposits_visits_rows() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        let outpoint = keys::encode_outpoint(&[0xAB; 32], 1);
        let entry = DaoDepositCacheEntry {
            capacity: 42,
            occupied_capacity: 0,
            deposit_block_number: 100,
            deposit_timestamp: 0,
            lock_script_hash: vec![0xCD; 32],
            deposit_ar: 1,
            status: 0,
            withdraw_request_tx: None,
            withdraw_request_output_index: None,
            withdraw_request_block: None,
            withdraw_request_ar: None,
            withdraw_block: None,
            withdraw_tx: None,
            withdraw_request_occupied_capacity: None,
            withdraw_to_output_index: None,
            compensation: None,
        };
        store.put_dao_deposit_direct(&outpoint, &entry).unwrap();

        let mut visited = Vec::new();
        store
            .scan_dao_deposits(|key, value| {
                visited.push((key.to_vec(), value.clone()));
                Ok(())
            })
            .unwrap();

        assert_eq!(visited.len(), 1);
        assert_eq!(visited[0].0, outpoint);
        assert_eq!(visited[0].1, entry);
    }

    #[test]
    fn test_list_dao_deposits_by_status_paginated_descending() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();

        let mut batch = StoreBatch::new(&store);
        for (tx, status, block) in [
            ([0xA1; 32], 0i16, 30i64),
            ([0xA2; 32], 1, 20),
            ([0xA3; 32], 0, 10),
        ] {
            batch.put_dao_deposit(
                &keys::encode_outpoint(&tx, 0),
                &DaoDepositCacheEntry {
                    capacity: 100,
                    occupied_capacity: 0,
                    deposit_block_number: block,
                    deposit_timestamp: 0,
                    lock_script_hash: vec![0x11; 32],
                    deposit_ar: 1,
                    status,
                    withdraw_request_tx: None,
                    withdraw_request_output_index: None,
                    withdraw_request_block: None,
                    withdraw_request_ar: None,
                    withdraw_block: None,
                    withdraw_tx: None,
                    withdraw_request_occupied_capacity: None,
                    withdraw_to_output_index: None,
                    compensation: None,
                },
            );
        }
        batch.commit().unwrap();

        let first_page = store
            .list_dao_deposits_by_status_paginated(0, 2, None)
            .unwrap();
        assert_eq!(first_page.len(), 2);
        assert_eq!(first_page[0].1.deposit_block_number, 30);
        assert_eq!(first_page[1].1.deposit_block_number, 10);

        let cursor = keys::encode_dao_by_status_block_key(
            0,
            first_page[1].1.deposit_block_number,
            &first_page[1].0,
        );
        let second_page = store
            .list_dao_deposits_by_status_paginated(0, 2, Some(&cursor))
            .unwrap();
        assert!(second_page.is_empty());
    }

    #[test]
    fn test_list_dao_deposits_by_status_paginated_works_in_secondary_mode() {
        let primary_dir = TempDir::new().unwrap();
        let secondary_dir = TempDir::new().unwrap();

        let primary = CkbadgerStore::open_test_unified(primary_dir.path()).unwrap();
        let mut batch = StoreBatch::new(&primary);
        batch.put_dao_deposit(
            &keys::encode_outpoint(&[0xA1; 32], 0),
            &DaoDepositCacheEntry {
                capacity: 100,
                occupied_capacity: 0,
                deposit_block_number: 30,
                deposit_timestamp: 0,
                lock_script_hash: vec![0x11; 32],
                deposit_ar: 1,
                status: 0,
                withdraw_request_tx: None,
                withdraw_request_output_index: None,
                withdraw_request_block: None,
                withdraw_request_ar: None,
                withdraw_block: None,
                withdraw_tx: None,
                withdraw_request_occupied_capacity: None,
                withdraw_to_output_index: None,
                compensation: None,
            },
        );
        batch.put_dao_deposit(
            &keys::encode_outpoint(&[0xA2; 32], 0),
            &DaoDepositCacheEntry {
                capacity: 100,
                occupied_capacity: 0,
                deposit_block_number: 20,
                deposit_timestamp: 0,
                lock_script_hash: vec![0x11; 32],
                deposit_ar: 1,
                status: 1,
                withdraw_request_tx: Some(vec![0xBB; 32]),
                withdraw_request_output_index: Some(0),
                withdraw_request_block: Some(21),
                withdraw_request_ar: Some(2),
                withdraw_block: None,
                withdraw_tx: None,
                withdraw_request_occupied_capacity: Some(0),
                withdraw_to_output_index: None,
                compensation: None,
            },
        );
        batch.commit().unwrap();

        let secondary =
            CkbadgerStore::open_test_unified_secondary(primary_dir.path(), secondary_dir.path())
                .unwrap();
        secondary.refresh().unwrap();

        let rows = secondary
            .list_dao_deposits_by_status_paginated(0, 10, None)
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].1.status, 0);
        assert_eq!(rows[0].1.deposit_block_number, 30);
    }

    #[test]
    fn test_list_dao_deposits_by_status_paginated_survives_mid_scan_entry_update() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        let outpoint = keys::encode_outpoint(&[0xA9; 32], 0);

        let mut batch = StoreBatch::new(&store);
        batch.put_dao_deposit(
            &outpoint,
            &DaoDepositCacheEntry {
                capacity: 100,
                occupied_capacity: 0,
                deposit_block_number: 30,
                deposit_timestamp: 0,
                lock_script_hash: vec![0x11; 32],
                deposit_ar: 1,
                status: 0,
                withdraw_request_tx: None,
                withdraw_request_output_index: None,
                withdraw_request_block: None,
                withdraw_request_ar: None,
                withdraw_block: None,
                withdraw_tx: None,
                withdraw_request_occupied_capacity: None,
                withdraw_to_output_index: None,
                compensation: None,
            },
        );
        batch.commit().unwrap();

        // Simulate a concurrent status transition after index key is read but before
        // dao_deposits row is loaded.
        let outpoint_for_hook = outpoint;
        set_dao_status_pagination_hook(Some(Box::new(move |store, outpoint_key| {
            if outpoint_key != outpoint_for_hook.as_slice() {
                return;
            }
            let mut entry = store
                .get_dao_deposit(outpoint_key)
                .expect("load dao deposit in hook")
                .expect("dao deposit missing in hook");
            entry.status = 1;
            entry.withdraw_request_block = Some(31);
            entry.withdraw_request_tx = Some(vec![0xBB; 32]);
            entry.withdraw_request_output_index = Some(0);
            store
                .put_dao_deposit_direct(outpoint_key, &entry)
                .expect("update dao deposit in hook");
        })));

        let rows = store.list_dao_deposits_by_status_paginated(0, 10, None);
        set_dao_status_pagination_hook(None);

        let rows = rows.expect("status pagination should remain consistent under mid-scan update");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].0, outpoint);
        assert_eq!(rows[0].1.status, 0);

        let current = store.get_dao_deposit(&outpoint).unwrap().unwrap();
        assert_eq!(current.status, 1, "hook must have updated canonical row");
    }

    /// The API reads a secondary, which cannot snapshot: a catch-up landing
    /// between the index-row read and the entry load makes a healthy 0->1
    /// withdraw request look like a stale index row and fails the listing.
    /// Pinning the read view for the scope (what the API middleware does per
    /// request) keeps the catch-up out until the scan is done.
    #[test]
    fn test_status_pagination_holds_one_view_while_catch_up_waits() {
        use std::sync::mpsc;
        use std::sync::Arc;

        let primary_dir = TempDir::new().unwrap();
        let secondary_dir = TempDir::new().unwrap();
        let primary = Arc::new(CkbadgerStore::open_test_unified(primary_dir.path()).unwrap());
        let outpoint = keys::encode_outpoint(&[0xC3; 32], 0);

        let mut batch = StoreBatch::new(&primary);
        batch.put_dao_deposit(
            &outpoint,
            &DaoDepositCacheEntry {
                capacity: 100,
                occupied_capacity: 0,
                deposit_block_number: 30,
                deposit_timestamp: 0,
                lock_script_hash: vec![0x11; 32],
                deposit_ar: 1,
                status: 0,
                withdraw_request_tx: None,
                withdraw_request_output_index: None,
                withdraw_request_block: None,
                withdraw_request_ar: None,
                withdraw_block: None,
                withdraw_tx: None,
                withdraw_request_occupied_capacity: None,
                withdraw_to_output_index: None,
                compensation: None,
            },
        );
        batch.commit().unwrap();

        let secondary = Arc::new(
            CkbadgerStore::open_test_unified_secondary(primary_dir.path(), secondary_dir.path())
                .unwrap(),
        );
        secondary.refresh().unwrap();

        // One pinned read view for this scope, as the API pins one per request.
        let view = crate::read_view::acquire_read();

        let (catcher_tx, catcher_rx) = mpsc::channel();
        let hook_primary = Arc::clone(&primary);
        let hook_secondary = Arc::clone(&secondary);
        let hook_outpoint = outpoint;
        set_dao_status_pagination_hook(Some(Box::new(move |_scanned, outpoint_key| {
            if outpoint_key != hook_outpoint.as_slice() {
                return;
            }
            // The indexer commits the withdraw request mid-scan...
            let mut entry = hook_primary
                .get_dao_deposit(outpoint_key)
                .expect("load dao deposit in hook")
                .expect("dao deposit missing in hook");
            entry.status = 1;
            entry.withdraw_request_block = Some(31);
            entry.withdraw_request_tx = Some(vec![0xBB; 32]);
            entry.withdraw_request_output_index = Some(0);
            entry.withdraw_request_ar = Some(2);
            hook_primary
                .put_dao_deposit_direct(outpoint_key, &entry)
                .expect("update dao deposit in hook");

            // ...and the refresh loop tries to advance the secondary onto it.
            let catcher_store = Arc::clone(&hook_secondary);
            let catcher = std::thread::spawn(move || catcher_store.refresh());
            std::thread::sleep(std::time::Duration::from_millis(150));
            catcher_tx.send(catcher).expect("send catch-up handle");
        })));

        let rows = secondary.list_dao_deposits_by_status_paginated(0, 10, None);
        set_dao_status_pagination_hook(None);

        let rows = rows.expect("a pinned view must not observe a mid-scan catch-up");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].0, outpoint);
        assert_eq!(
            rows[0].1.status, 0,
            "the pinned view serves the pre-catch-up state, whole"
        );

        // Releasing the view lets the queued catch-up land; the next scope sees it.
        drop(view);
        catcher_rx
            .recv()
            .expect("catch-up handle")
            .join()
            .expect("catch-up thread panicked")
            .expect("catch-up failed");

        assert!(secondary
            .list_dao_deposits_by_status_paginated(0, 10, None)
            .unwrap()
            .is_empty());
        let moved = secondary
            .list_dao_deposits_by_status_paginated(1, 10, None)
            .unwrap();
        assert_eq!(moved.len(), 1);
        assert_eq!(moved[0].0, outpoint);
        assert_eq!(moved[0].1.status, 1);
    }

    #[test]
    fn test_scan_dao_deposits_by_lock_visits_only_target_lock() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        let lock_a = vec![0x11; 32];
        let lock_b = vec![0x22; 32];

        let mut batch = StoreBatch::new(&store);
        batch.put_dao_deposit(
            &keys::encode_outpoint(&[0xA1; 32], 0),
            &DaoDepositCacheEntry {
                capacity: 1,
                occupied_capacity: 0,
                deposit_block_number: 10,
                deposit_timestamp: 0,
                lock_script_hash: lock_a.clone(),
                deposit_ar: 1,
                status: 0,
                withdraw_request_tx: None,
                withdraw_request_output_index: None,
                withdraw_request_block: None,
                withdraw_request_ar: None,
                withdraw_block: None,
                withdraw_tx: None,
                withdraw_request_occupied_capacity: None,
                withdraw_to_output_index: None,
                compensation: None,
            },
        );
        batch.put_dao_deposit(
            &keys::encode_outpoint(&[0xA2; 32], 0),
            &DaoDepositCacheEntry {
                capacity: 1,
                occupied_capacity: 0,
                deposit_block_number: 20,
                deposit_timestamp: 0,
                lock_script_hash: lock_b,
                deposit_ar: 1,
                status: 0,
                withdraw_request_tx: None,
                withdraw_request_output_index: None,
                withdraw_request_block: None,
                withdraw_request_ar: None,
                withdraw_block: None,
                withdraw_tx: None,
                withdraw_request_occupied_capacity: None,
                withdraw_to_output_index: None,
                compensation: None,
            },
        );
        batch.commit().unwrap();

        let mut blocks = Vec::new();
        store
            .scan_dao_deposits_by_lock(&lock_a, |_, entry| {
                blocks.push(entry.deposit_block_number);
                Ok(())
            })
            .unwrap();

        assert_eq!(blocks, vec![10]);
    }

    #[test]
    fn test_put_dao_deposit_direct_replaces_status_index() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        let outpoint = keys::encode_outpoint(&[0xAB; 32], 1);

        let mut entry = DaoDepositCacheEntry {
            capacity: 42,
            occupied_capacity: 0,
            deposit_block_number: 100,
            deposit_timestamp: 0,
            lock_script_hash: vec![0xCD; 32],
            deposit_ar: 1,
            status: 0,
            withdraw_request_tx: None,
            withdraw_request_output_index: None,
            withdraw_request_block: None,
            withdraw_request_ar: None,
            withdraw_block: None,
            withdraw_tx: None,
            withdraw_request_occupied_capacity: None,
            withdraw_to_output_index: None,
            compensation: None,
        };
        store.put_dao_deposit_direct(&outpoint, &entry).unwrap();

        entry.status = 1;
        entry.withdraw_request_tx = Some(vec![0xEE; 32]);
        entry.withdraw_request_output_index = Some(0);
        entry.withdraw_request_block = Some(120);
        store.put_dao_deposit_direct(&outpoint, &entry).unwrap();

        let status0 = store
            .list_dao_deposits_by_status_paginated(0, 10, None)
            .unwrap();
        assert!(status0.is_empty());
        let status1 = store
            .list_dao_deposits_by_status_paginated(1, 10, None)
            .unwrap();
        assert_eq!(status1.len(), 1);
        assert_eq!(status1[0].0, outpoint);
    }

    #[test]
    fn test_list_active_dao_deposits_uses_status_index() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();

        let mut batch = StoreBatch::new(&store);
        batch.put_dao_deposit(
            &keys::encode_outpoint(&[0xA1; 32], 0),
            &DaoDepositCacheEntry {
                capacity: 1,
                occupied_capacity: 0,
                deposit_block_number: 10,
                deposit_timestamp: 0,
                lock_script_hash: vec![0x11; 32],
                deposit_ar: 1,
                status: 0,
                withdraw_request_tx: None,
                withdraw_request_output_index: None,
                withdraw_request_block: None,
                withdraw_request_ar: None,
                withdraw_block: None,
                withdraw_tx: None,
                withdraw_request_occupied_capacity: None,
                withdraw_to_output_index: None,
                compensation: None,
            },
        );
        batch.put_dao_deposit(
            &keys::encode_outpoint(&[0xA2; 32], 0),
            &DaoDepositCacheEntry {
                capacity: 1,
                occupied_capacity: 0,
                deposit_block_number: 20,
                deposit_timestamp: 0,
                lock_script_hash: vec![0x11; 32],
                deposit_ar: 1,
                status: 2,
                withdraw_request_tx: Some(vec![0xBB; 32]),
                withdraw_request_output_index: Some(0),
                withdraw_request_block: Some(15),
                withdraw_request_ar: Some(2),
                withdraw_block: Some(30),
                withdraw_tx: Some(vec![0xCC; 32]),
                withdraw_request_occupied_capacity: Some(0),
                withdraw_to_output_index: Some(0),
                compensation: Some(5),
            },
        );
        batch.commit().unwrap();

        let active = store.list_active_dao_deposits().unwrap();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].1.status, 0);
    }

    #[test]
    fn test_list_dao_deposits_by_lock_paginated_descending() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();

        let lock = vec![0x44; 32];
        let mut batch = StoreBatch::new(&store);
        for (tx, block) in [([0xA1; 32], 30i64), ([0xA2; 32], 20), ([0xA3; 32], 10)] {
            batch.put_dao_deposit(
                &keys::encode_outpoint(&tx, 0),
                &DaoDepositCacheEntry {
                    capacity: 100,
                    occupied_capacity: 0,
                    deposit_block_number: block,
                    deposit_timestamp: 0,
                    lock_script_hash: lock.clone(),
                    deposit_ar: 1,
                    status: 0,
                    withdraw_request_tx: None,
                    withdraw_request_output_index: None,
                    withdraw_request_block: None,
                    withdraw_request_ar: None,
                    withdraw_block: None,
                    withdraw_tx: None,
                    withdraw_request_occupied_capacity: None,
                    withdraw_to_output_index: None,
                    compensation: None,
                },
            );
        }
        batch.commit().unwrap();

        let first = store
            .list_dao_deposits_by_lock_paginated(&lock, 2, None)
            .unwrap();
        assert_eq!(first.len(), 2);
        assert_eq!(first[0].1.deposit_block_number, 30);
        assert_eq!(first[1].1.deposit_block_number, 20);

        let cursor =
            keys::encode_dao_by_lock_block_key(&lock, first[1].1.deposit_block_number, &first[1].0);
        let second = store
            .list_dao_deposits_by_lock_paginated(&lock, 2, Some(&cursor))
            .unwrap();
        assert_eq!(second.len(), 1);
        assert_eq!(second[0].1.deposit_block_number, 10);
    }

    #[test]
    fn test_scan_dao_deposits_by_lock_rejects_non_32_byte_hash() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        let err = store
            .scan_dao_deposits_by_lock(&[0x11; 33], |_, _| Ok(()))
            .unwrap_err();
        assert!(
            err.to_string()
                .contains("scan_dao_deposits_by_lock expected lock hash 32 bytes"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn test_list_dao_deposits_by_lock_paginated_rejects_non_32_byte_hash() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        let err = store
            .list_dao_deposits_by_lock_paginated(&[0x11; 33], 1, None)
            .unwrap_err();
        assert!(
            err.to_string()
                .contains("list_dao_deposits_by_lock_paginated expected lock hash 32 bytes"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn test_list_dao_deposits_paginated_cursor_handles_same_block_multiple_rows() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();

        let mut batch = StoreBatch::new(&store);
        for (tx, output_index, block) in [
            ([0xD1; 32], 0i16, 30i64),
            ([0xD2; 32], 1i16, 30i64),
            ([0xD3; 32], 2i16, 30i64),
            ([0xD4; 32], 0i16, 20i64),
        ] {
            batch.put_dao_deposit(
                &keys::encode_outpoint(&tx, output_index),
                &DaoDepositCacheEntry {
                    capacity: 100,
                    occupied_capacity: 0,
                    deposit_block_number: block,
                    deposit_timestamp: 0,
                    lock_script_hash: vec![0x55; 32],
                    deposit_ar: 1,
                    status: 0,
                    withdraw_request_tx: None,
                    withdraw_request_output_index: None,
                    withdraw_request_block: None,
                    withdraw_request_ar: None,
                    withdraw_block: None,
                    withdraw_tx: None,
                    withdraw_request_occupied_capacity: None,
                    withdraw_to_output_index: None,
                    compensation: None,
                },
            );
        }
        batch.commit().unwrap();

        let first_page = store.list_dao_deposits_paginated(2, None).unwrap();
        assert_eq!(first_page.len(), 2);
        assert_eq!(first_page[0].1.deposit_block_number, 30);
        assert_eq!(first_page[1].1.deposit_block_number, 30);

        let cursor =
            keys::encode_dao_by_block_key(first_page[1].1.deposit_block_number, &first_page[1].0);
        let second_page = store.list_dao_deposits_paginated(2, Some(&cursor)).unwrap();
        assert_eq!(second_page.len(), 2);
        assert_eq!(second_page[0].1.deposit_block_number, 30);
        assert_eq!(second_page[1].1.deposit_block_number, 20);
    }

    #[test]
    fn test_list_dao_deposits_paginated_errors_on_stale_index() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();

        let outpoint = keys::encode_outpoint(&[0xAA; 32], 0);
        let index_key = keys::encode_dao_by_block_key(100, &outpoint);
        store
            .put_cf(store.cf_dao_by_block(), &index_key, &[])
            .context("seed stale dao_by_block index")
            .unwrap();

        let err = store.list_dao_deposits_paginated(10, None).unwrap_err();
        assert!(err.to_string().contains("stale dao_by_block index"));
    }
}
