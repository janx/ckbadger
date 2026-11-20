//! The layout of the domain store: the one stamp that tells a build whether
//! it may read and write a store (lane rd/api-lock-names, the fix round's M1).
//!
//! A store states the layout it was built with (`sync_meta` `store_layout`,
//! a little-endian u32); a build states the layout it writes and reads
//! ([`STORE_LAYOUT`]). The indexer owns the stamp: it stamps a fresh store
//! before its first write and refuses any other store before writing
//! anything (`ckbadger_indexer::store_layout`); the API reads the same check
//! when it opens the store. A mismatch is never repaired: delete RocksDB and
//! re-sync from genesis.

use crate::keys::sync_meta_keys;
use crate::store::{
    CkbadgerStore, CF_LIVE_CODE_CELLS_BY_TYPE, CF_SYNC_META, CF_TOKEN_LABELS, DOMAIN_CFS,
};
use rocksdb::DB;

/// The layout of the domain store this build writes and reads.
///
/// One number for every layout change that needs a re-sync, raised by the
/// change that needs it:
///
/// - 1: the script changes alone (rd/script-pair-store, never re-synced on
///   its own). A store stamped 1 lacks the changes of 2.
/// - 2: the re-sync set (resync/all), one number for all of its changes:
///   - pair-keyed script labels: `script_info` is keyed
///     `code_hash ‖ hash_type` and holds the labels of the registry's pseudo
///     deployments only (Zero Lock, Type ID), no usage; a script is named by
///     the code its pair runs, and `DailyActivityStats.script_counts` is
///     keyed `"<code hash hex>:<hash type label>"`;
///   - usage counters of generic type references by their live code: a
///     generic (non Type ID) type reference is counted in a family by the
///     binaries its live code cells hold (`ReferenceCode`, the one family
///     rule);
///   - one-kind activity rows: each daily / hourly activity row counts each
///     non-cellbase transaction in exactly one kind (`ActivityKindCounts`;
///     the row's strict decode is [`crate::types::ACTIVITY_STATS_LAYOUT`]);
///   - the senders bit: a participant's tags carry `TAG_INPUT` when its lock
///     is the lock of at least one input (`tx_actions`, `addr_txs`,
///     `addr_txs_by_prefix` values).
/// - 3: live sync reads a batch's token max-supply claims in every batch,
///   as bulk build does (resync/all): a build stamped 2 read them only in a
///   batch that moved a UDT, so a store it live-synced can miss a claim
///   (`tokens` max supply).
/// - 4: one max-supply rule for both sync paths: a token's max supply is the
///   first claim at or after its first block, in chain order
///   (`token_helpers::token_max_supply_claim`). A store stamped 3 took, live,
///   whichever claim its batch cuts let through and, by bulk, a claim older
///   than the token.
/// - 5: an Omnilock max-supply claim counts for a token only when the token's
///   owner lock (the first 32 bytes of its type args) is the supply-mode
///   Omnilock holding the claim's info cell. A store stamped 4 can show a max
///   supply that any lock wrote for any token.
/// - 6: protocols by their pair (resync/protocol-pairs): the indexer
///   recognises a protocol and a standard lock by the pair its script runs
///   (`ProtocolRegistry::script`), so a cell locked by a protocol lock's code
///   hash under another hash type writes no protocol action, Fiber channel,
///   `.cell` listing or max-supply claim, and is a called lock
///   (`lock_calls`); a standard lock's binary by a data hash type is a
///   standard lock. A store stamped 5 holds those rows by code hash alone.
/// - 7: the recognised type codes that ran (resync/final, C7 of
///   resync/type-codes): each `tx_actions` row ends with
///   `recognized_type_codes`, every type script code the indexer recognises
///   that ran in the transaction, each pair (code hash, hash type) once, as
///   the one registry recognises it, beside the unrecognised `type_calls`
///   (the row's strict decode, [`crate::types::decode_stored_tx_actions`],
///   refuses a row without it: the row's own guard); a party for which a
///   recognised type ran that moved none of its items and took no DAO step
///   carries `TAG_TYPE_CALL` (a wallet merging its own token cells counts
///   under Script, never as a CKB transfer). A store stamped 6 lacks them.
/// - 8: a token row only for a token the chain holds (resync fix round M1):
///   label import keeps the curated labels in `token_labels` and writes no
///   `tokens` row, so a token's first block is the block of its first cell on
///   both paths and a claim made before it is never kept on its row; a row
///   holds what the chain says and the API joins the label when it reads the
///   token (fix round 2, B). A store stamped 7 holds label rows with first
///   block 0, from which live admitted a claim made before the token existed. And the
///   xUDT-extension claimant bound by its script (fix round S1): an
///   extension's cell is the Unique Cell by its registered pair, the last
///   such output with the extension's args giving the bound metadata and the
///   claim on both paths; a store stamped 7 took a claim and metadata from
///   any cell with those 20 bytes of args. And one pair rule for the
///   xUDT-compatible deployments (logic NOTE N3): a compatible deployment's
///   version binary by a data hash type is a UDT, as the xUDT binary is; a
///   store stamped 7 recognised their references only. And the live code
///   cells by their type script (fix round S3, `live_code_cells_by_type`):
///   one row per live code cell under a type script that is not Type ID,
///   from which the live rollup and the API settle a generic type reference
///   by its code cells only; a store stamped 7 has no such column family.
///   Layout 8 is unreleased, and the stamp stayed 8 through the fix rounds
///   that changed what its rows hold (round 2's B: a token row the chain's
///   alone; round 3: a creating open makes the newest column families first,
///   and a stored label states no standard): no store stamped 8 outlives its
///   test or its scratch run (a paired bulk run's stores are deleted by its
///   runner), and the owner's re-sync starts from a purge (the lead's ruling).
///
/// A store from before layouts were stamped carries no stamp: its
/// `script_info` rows are keyed by the code hash alone and sum every hash
/// type's usage, its activity rows are another layout, its participants
/// carry no senders bit.
pub const STORE_LAYOUT: u32 = 8;

/// What [`STORE_LAYOUT`] changed, for the refusal's words.
pub const STORE_LAYOUT_CHANGE: &str = "scripts labelled, named and counted by their pair (code \
     hash, hash type), generic type references counted by their live code, each transaction in \
     one activity kind, the senders bit on participants, a token's max supply its first claim \
     at or after its first block on both sync paths, made by the token's owner lock, protocols \
     recognised by their pair, the recognised type codes that ran on each activity, a token \
     row only once the chain holds the token and holding what the chain says, \
     an xUDT extension's Unique Cell bound by its pair, the live code cells indexed by their type \
     script";

/// A store this build may use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreLayoutState {
    /// No row in any domain column family: nothing to misread. The indexer
    /// stamps it before its first write.
    Fresh,
    /// Stamped with [`STORE_LAYOUT`].
    Current,
    /// Still being created (resync fix round 3, M): RocksDB writes `CURRENT`
    /// before a creating open of this build has made every column family, so
    /// a reader can meet the store holding the first ones of
    /// [`domain_creation_order`] and no row and no stamp. Nothing to misread,
    /// and nothing to refuse: look again. A reader drops what it opened (a
    /// secondary never gains a column family made after it opened); the
    /// indexer's open makes the rest.
    NotReady,
}

/// The domain column families the newest layouts added, newest first: a
/// creating open makes them before every other ([`domain_creation_order`]).
/// Layout 8 added both.
///
/// THE RULE FOR THE NEXT LAYOUT: a layout's new column families are created
/// first. A layout that adds a column family puts it at the FRONT of this list
/// (and raises [`STORE_LAYOUT`]), so a store still being created always holds
/// the newest column families once it holds any, and a store of an earlier
/// layout, which lacks them, is never such a store and is refused by its
/// column families alone (resync fix round 3, M).
pub const DOMAIN_CFS_CREATED_FIRST: [&str; 2] = [CF_TOKEN_LABELS, CF_LIVE_CODE_CELLS_BY_TYPE];

/// The order in which a creating open makes the domain column families: the
/// order of its descriptors (`CkbadgerStore::open_domain`; RocksDB's
/// `DB::Open` makes each missing column family in that order, one MANIFEST
/// append each, after `NewDB` has written `CURRENT` with `default` alone).
/// [`DOMAIN_CFS_CREATED_FIRST`] first, then the rest in [`DOMAIN_CFS`] order
/// (resync fix round 3, M): a store a creating open of this build has not
/// finished holds `default` and the first column families of this order,
/// which always include the newest ones; an earlier build's store, which
/// lacks them, is never such a store.
pub fn domain_creation_order() -> Vec<&'static str> {
    DOMAIN_CFS_CREATED_FIRST
        .iter()
        .copied()
        .chain(
            DOMAIN_CFS
                .iter()
                .copied()
                .filter(|name| !DOMAIN_CFS_CREATED_FIRST.contains(name)),
        )
        .collect()
}

/// Whether a store's column families are what a creating open of this build
/// leaves before it has made them all (resync fix round 3, M): none that
/// this build does not know, and the ones it lacks exactly the last ones of
/// [`domain_creation_order`].
fn still_being_created(column_families: &StoreColumnFamilies) -> bool {
    if !column_families.unknown.is_empty() || column_families.missing.is_empty() {
        return false;
    }
    let order = domain_creation_order();
    let Some(made) = order.len().checked_sub(column_families.missing.len()) else {
        return false;
    };
    order[made..]
        .iter()
        .all(|name| column_families.missing.iter().any(|lacked| lacked == name))
}

/// How a store's column families differ from this build's (resync fix
/// round 2, A): the ones this build has that the store lacks, and the ones
/// the store has that this build does not know. Empty for a store this build
/// opened as primary (its open creates every column family it lacks, and
/// refuses one it does not know); a secondary opens the ones it holds.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StoreColumnFamilies {
    pub missing: Vec<String>,
    pub unknown: Vec<String>,
}

impl StoreColumnFamilies {
    /// The difference between `expected` (this build's column families of
    /// the store's class) and `existing` (the store's, `default` aside).
    pub fn of(expected: &[&str], existing: &[String]) -> Self {
        let missing = expected
            .iter()
            .filter(|name| !existing.iter().any(|have| have == *name))
            .map(|name| name.to_string())
            .collect();
        let unknown = existing
            .iter()
            .filter(|have| have.as_str() != "default" && !expected.contains(&have.as_str()))
            .cloned()
            .collect();
        Self { missing, unknown }
    }

    pub fn is_empty(&self) -> bool {
        self.missing.is_empty() && self.unknown.is_empty()
    }
}

/// A store this build must not use: built with another layout, or before
/// layouts were stamped (`found: None`), or with column families that are not
/// this build's (`column_families`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreLayoutMismatch {
    pub found: Option<u32>,
    pub column_families: StoreColumnFamilies,
}

impl StoreLayoutMismatch {
    /// A store stamped `found` (or not stamped) with this build's column
    /// families.
    pub fn stamped(found: Option<u32>) -> Self {
        Self {
            found,
            column_families: StoreColumnFamilies::default(),
        }
    }

    /// What is wrong, both layouts named, without the remedy (the indexer's
    /// `RebuildRequiredError` adds it with the network's store paths).
    pub fn diagnosis(&self) -> String {
        let built = match self.found {
            Some(layout) => format!("store layout {layout}"),
            None if self.column_families.is_empty() => {
                "no store layout stamp (a build from before store layouts were stamped)".to_string()
            }
            None => "no store layout stamp".to_string(),
        };
        let mut column_families = Vec::new();
        if !self.column_families.missing.is_empty() {
            column_families.push(format!(
                "without the column families {}",
                self.column_families.missing.join(", ")
            ));
        }
        if !self.column_families.unknown.is_empty() {
            column_families.push(format!(
                "with the column families {}, which this build does not know",
                self.column_families.unknown.join(", ")
            ));
        }
        let column_families = if column_families.is_empty() {
            String::new()
        } else {
            format!(" and {}", column_families.join(" and "))
        };
        format!(
            "the domain store was built with {built}{column_families}; this build reads and \
             writes store layout {STORE_LAYOUT} ({STORE_LAYOUT_CHANGE})"
        )
    }
}

impl std::fmt::Display for StoreLayoutMismatch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}: delete RocksDB and re-sync from genesis",
            self.diagnosis()
        )
    }
}

impl std::error::Error for StoreLayoutMismatch {}

impl CkbadgerStore {
    /// The layout stamp, if the store carries one. A value that is not four
    /// bytes is an error, named: it is no layout of any build.
    pub fn get_store_layout(&self) -> anyhow::Result<Option<u32>> {
        stamp_in(self.raw_db())
    }

    /// Stamp the store with `layout`. The indexer stamps a fresh store, and
    /// only a fresh one ([`Self::check_store_layout`]).
    pub fn set_store_layout(&self, layout: u32) -> anyhow::Result<()> {
        self.put_cf(
            self.cf_sync_meta(),
            sync_meta_keys::STORE_LAYOUT,
            &layout.to_le_bytes(),
        )
    }

    /// Whether no domain column family holds a row: a store no build has
    /// written yet.
    pub fn holds_no_domain_rows(&self) -> anyhow::Result<bool> {
        Ok(!holds_domain_rows_in(self.raw_db())?)
    }

    /// THE layout check, read by every opener: the indexer and label import
    /// before their first write, the API, `status`, the TUI and the
    /// sequencer on the secondary they open; and before an open that could
    /// change the store, by [`inspect_domain_store`]. The rule is
    /// [`layout_of`].
    pub fn check_store_layout(
        &self,
    ) -> anyhow::Result<Result<StoreLayoutState, StoreLayoutMismatch>> {
        layout_of(self.raw_db(), self.column_families())
    }
}

/// The layout stamp in `db`'s `sync_meta`, if any (none when the store has no
/// `sync_meta` at all).
fn stamp_in(db: &DB) -> anyhow::Result<Option<u32>> {
    let Some(sync_meta) = db.cf_handle(CF_SYNC_META) else {
        return Ok(None);
    };
    match db.get_cf(sync_meta, sync_meta_keys::STORE_LAYOUT)? {
        None => Ok(None),
        Some(bytes) => {
            let bytes: [u8; 4] = bytes.as_slice().try_into().map_err(|_| {
                anyhow::anyhow!(
                    "store_layout record is {} bytes, not 4: delete RocksDB and re-sync from \
                     genesis",
                    bytes.len()
                )
            })?;
            Ok(Some(u32::from_le_bytes(bytes)))
        }
    }
}

/// Whether any domain column family `db` opened holds a row.
fn holds_domain_rows_in(db: &DB) -> anyhow::Result<bool> {
    for name in DOMAIN_CFS {
        let Some(cf) = db.cf_handle(name) else {
            continue;
        };
        if let Some(row) = db.iterator_cf(cf, rocksdb::IteratorMode::Start).next() {
            row.map_err(|e| anyhow::anyhow!("read the first row of {name}: {e}"))?;
            return Ok(true);
        }
    }
    Ok(false)
}

/// THE layout rule over a store's facts: its stamp against [`STORE_LAYOUT`],
/// its column families against this build's, whether it holds a row. A store
/// of this build's column families and layout is `Current`; one of this
/// build's column families with no row and no stamp is `Fresh`: this build
/// created it (its open creates every column family before any write) and
/// has not stamped it yet. One holding the first column families of
/// [`domain_creation_order`], no row and no stamp, is `NotReady`: a creating
/// open of this build is still making them, or stopped before it made them
/// all (resync fix round 3, M). Any other is the inner error, each with
/// evidence of another layout: another stamp, rows and no stamp (a build from
/// before stamps), a column family this build does not know, or a set of
/// column families no creating open of this build leaves (an earlier build's
/// store, empty or not). The outer error is a read failure.
fn layout_of(
    db: &DB,
    column_families: &StoreColumnFamilies,
) -> anyhow::Result<Result<StoreLayoutState, StoreLayoutMismatch>> {
    let found = stamp_in(db)?;
    if column_families.is_empty() {
        match found {
            Some(layout) if layout == STORE_LAYOUT => return Ok(Ok(StoreLayoutState::Current)),
            None if !holds_domain_rows_in(db)? => return Ok(Ok(StoreLayoutState::Fresh)),
            _ => {}
        }
    } else if found.is_none() && still_being_created(column_families) && !holds_domain_rows_in(db)?
    {
        return Ok(Ok(StoreLayoutState::NotReady));
    }
    Ok(Err(StoreLayoutMismatch {
        found,
        column_families: column_families.clone(),
    }))
}

/// What the domain store at `primary_path` is, read without writing anything
/// in it, for an opener whose open could create or change something
/// (resync fix round 2, A: the indexer's startup, label import, verify):
/// `None` while no store exists there (no `CURRENT`), else [`layout_of`].
///
/// The read: the store's column families listed from its MANIFEST, then a
/// secondary of the domain column families it holds, at `secondary_path`
/// (the only directory the read writes in: RocksDB's info log), with
/// options of its own: not the process's shared block cache, which the
/// first open in a process sizes for the opens after it, and no create
/// flag. The secondary is closed when this returns.
pub fn inspect_domain_store(
    primary_path: &std::path::Path,
    secondary_path: &std::path::Path,
) -> anyhow::Result<Option<Result<StoreLayoutState, StoreLayoutMismatch>>> {
    if !primary_path.join("CURRENT").exists() {
        return Ok(None);
    }
    let mut opts = rocksdb::Options::default();
    let mut table = rocksdb::BlockBasedOptions::default();
    // Index and filter blocks through a small cache of the read's own, not
    // held for every table file the secondary keeps open.
    table.set_block_cache(&rocksdb::Cache::new_lru_cache(8 << 20));
    table.set_cache_index_and_filter_blocks(true);
    opts.set_block_based_table_factory(&table);
    let existing = DB::list_cf(&opts, primary_path).map_err(|e| {
        anyhow::anyhow!(
            "list the column families of the domain store at '{}': {e}",
            primary_path.display()
        )
    })?;
    let column_families = StoreColumnFamilies::of(DOMAIN_CFS, &existing);
    let held: Vec<&str> = DOMAIN_CFS
        .iter()
        .copied()
        .filter(|name| existing.iter().any(|have| have == name))
        .collect();
    let db = DB::open_cf_as_secondary(&opts, primary_path, secondary_path, held).map_err(|e| {
        anyhow::anyhow!(
            "read the domain store at '{}' (secondary at '{}'): {e}",
            primary_path.display(),
            secondary_path.display()
        )
    })?;
    Ok(Some(layout_of(&db, &column_families)?))
}

/// A domain store as an earlier build left it, for the tests of every opener
/// (resync fix round 2, A): written with plain RocksDB, never through this
/// build's open (which would create the column families this build added).
#[cfg(any(test, feature = "test-store"))]
pub mod earlier_layout_for_test {
    use crate::keys::sync_meta_keys;
    use crate::store::{CF_LIVE_CODE_CELLS_BY_TYPE, CF_SYNC_META, CF_TOKEN_LABELS, DOMAIN_CFS};
    use std::path::Path;

    /// The column families this build added since layout 7, which an earlier
    /// build's store lacks.
    pub const ADDED_SINCE_LAYOUT_7: [&str; 2] = [CF_TOKEN_LABELS, CF_LIVE_CODE_CELLS_BY_TYPE];

    /// Create, at `path`, a domain store with every domain column family of
    /// this build but [`ADDED_SINCE_LAYOUT_7`], holding one row (the network
    /// identity) and the layout stamp `stamp`, if any.
    pub fn create_earlier_layout_domain_store(path: &Path, stamp: Option<u32>) {
        create_domain_store_by_hand(path, &earlier_layout_column_families(), true, stamp);
    }

    /// The domain column families of an earlier build's store: this build's
    /// but [`ADDED_SINCE_LAYOUT_7`].
    pub fn earlier_layout_column_families() -> Vec<&'static str> {
        DOMAIN_CFS
            .iter()
            .copied()
            .filter(|name| !ADDED_SINCE_LAYOUT_7.contains(name))
            .collect()
    }

    /// Create, at `path`, with plain RocksDB, a domain store holding
    /// `column_families` (and `default`), one row (the network identity in
    /// `sync_meta`) when `row`, and the layout stamp `stamp`, if any, flushed
    /// to a table file (resync fix round 3, tests NOTE-1: an inspection of it
    /// reads a table file, as on a real store).
    pub fn create_domain_store_by_hand(
        path: &Path,
        column_families: &[&str],
        row: bool,
        stamp: Option<u32>,
    ) {
        let mut opts = rocksdb::Options::default();
        opts.create_if_missing(true);
        opts.create_missing_column_families(true);
        let db = rocksdb::DB::open_cf(&opts, path, column_families).unwrap();
        if row || stamp.is_some() {
            let sync_meta = db
                .cf_handle(CF_SYNC_META)
                .expect("a row or a stamp is written in sync_meta");
            if row {
                db.put_cf(sync_meta, sync_meta_keys::NETWORK_IDENTITY, b"mainnet")
                    .unwrap();
            }
            if let Some(stamp) = stamp {
                db.put_cf(sync_meta, sync_meta_keys::STORE_LAYOUT, stamp.to_le_bytes())
                    .unwrap();
            }
            db.flush_cf(sync_meta).unwrap();
        }
    }

    /// What a creating open of this build leaves when it has made the first
    /// `made` domain column families (in [`crate::domain_creation_order`]) and
    /// no more: no row, no stamp.
    pub fn create_store_in_creation(path: &Path, made: usize) {
        let order = crate::layout_ops::domain_creation_order();
        create_domain_store_by_hand(path, &order[..made], false, None);
    }

    /// What a refusal must leave as it found it: the store directory's files
    /// (name, size, a hash of the content: resync fix round 3, tests NOTE-1,
    /// a file rewritten at its size is a change too) and the store's column
    /// families, each sorted.
    pub fn store_files_and_column_families(path: &Path) -> (Vec<(String, u64, u64)>, Vec<String>) {
        use std::hash::{Hash, Hasher};
        let mut files: Vec<(String, u64, u64)> = std::fs::read_dir(path)
            .unwrap()
            .map(|entry| {
                let entry = entry.unwrap();
                let mut hasher = std::collections::hash_map::DefaultHasher::new();
                if entry.file_type().unwrap().is_file() {
                    std::fs::read(entry.path()).unwrap().hash(&mut hasher);
                }
                (
                    entry.file_name().to_string_lossy().into_owned(),
                    entry.metadata().unwrap().len(),
                    hasher.finish(),
                )
            })
            .collect();
        files.sort();
        let mut column_families = rocksdb::DB::list_cf(&rocksdb::Options::default(), path).unwrap();
        column_families.sort();
        (files, column_families)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ScriptInfo;

    fn store() -> (tempfile::TempDir, CkbadgerStore) {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        (dir, store)
    }

    #[test]
    fn a_store_with_no_row_is_fresh() {
        let (_dir, store) = store();
        assert_eq!(
            store.check_store_layout().unwrap(),
            Ok(StoreLayoutState::Fresh)
        );
    }

    #[test]
    fn a_stamped_store_is_current() {
        let (_dir, store) = store();
        store.set_store_layout(STORE_LAYOUT).unwrap();
        assert_eq!(store.get_store_layout().unwrap(), Some(STORE_LAYOUT));
        assert_eq!(
            store.check_store_layout().unwrap(),
            Ok(StoreLayoutState::Current)
        );
    }

    /// A store from before the stamp: rows and no stamp. The words say what
    /// to do.
    #[test]
    fn a_store_with_rows_and_no_stamp_is_refused_with_the_way_out() {
        let (_dir, store) = store();
        store.set_network_identity("mainnet").unwrap();
        let mismatch = store.check_store_layout().unwrap().unwrap_err();
        assert_eq!(mismatch, StoreLayoutMismatch::stamped(None));
        let words = mismatch.to_string();
        assert!(
            words.contains("no store layout stamp")
                && words.contains(&format!("store layout {STORE_LAYOUT}"))
                && words.contains("delete RocksDB and re-sync from genesis"),
            "{words}"
        );
    }

    /// Stamp 1 is the script changes alone (rd/script-pair-store): a store
    /// built by that branch lacks the re-sync set's activity rows and senders
    /// bit, so this build refuses it.
    #[test]
    fn a_store_stamped_with_the_script_changes_alone_is_refused() {
        let (_dir, store) = store();
        store.set_store_layout(1).unwrap();
        assert_eq!(
            store.check_store_layout().unwrap(),
            Err(StoreLayoutMismatch::stamped(Some(1)))
        );
    }

    /// Stamp 2 is the re-sync set before live sync read a batch's max-supply
    /// claims in every batch: a store a build of it live-synced can miss a
    /// claim made in a batch with no UDT movement, so this build refuses it.
    #[test]
    fn a_store_stamped_before_live_claims_were_read_in_every_batch_is_refused() {
        let (_dir, store) = store();
        store.set_store_layout(2).unwrap();
        assert_eq!(
            store.check_store_layout().unwrap(),
            Err(StoreLayoutMismatch::stamped(Some(2)))
        );
    }

    /// Stamp 3 is the re-sync set before one max-supply rule held for both
    /// sync paths: a live store's claim depended on where its batches were
    /// cut, a bulk store took a claim older than the token. Refused.
    #[test]
    fn a_store_stamped_before_the_one_max_supply_rule_is_refused() {
        let (_dir, store) = store();
        store.set_store_layout(3).unwrap();
        assert_eq!(
            store.check_store_layout().unwrap(),
            Err(StoreLayoutMismatch::stamped(Some(3)))
        );
    }

    /// Stamp 4 is the re-sync set before an Omnilock claim was bound to the
    /// token's owner lock: its tokens can show a max supply any lock wrote.
    /// Refused.
    #[test]
    fn a_store_stamped_before_claims_were_bound_to_the_owner_is_refused() {
        let (_dir, store) = store();
        store.set_store_layout(4).unwrap();
        assert_eq!(
            store.check_store_layout().unwrap(),
            Err(StoreLayoutMismatch::stamped(Some(4)))
        );
    }

    /// Stamp 5 is the re-sync set before protocols were read by their pair
    /// (resync/all): its channels, actions, listings, claims and called locks
    /// took a protocol's code hash by any hash type, so this build refuses it.
    #[test]
    fn a_store_stamped_before_protocols_were_read_by_their_pair_is_refused() {
        let (_dir, store) = store();
        store.set_store_layout(5).unwrap();
        assert_eq!(
            store.check_store_layout().unwrap(),
            Err(StoreLayoutMismatch::stamped(Some(5)))
        );
    }

    /// Stamp 6 is the re-sync set before the recognised type codes (resync/protocol-pairs): its
    /// activity rows end at the participants, without the type codes that ran, so this build
    /// refuses it.
    #[test]
    fn a_store_stamped_before_the_recognised_type_codes_is_refused() {
        let (_dir, store) = store();
        store.set_store_layout(6).unwrap();
        assert_eq!(
            store.check_store_layout().unwrap(),
            Err(StoreLayoutMismatch::stamped(Some(6)))
        );
    }

    /// Stamp 7 is the re-sync set before the fix round's M1: label import
    /// wrote a token row with first block 0 for every labelled token, and
    /// live admitted a claim made before the token existed, so this build
    /// refuses it.
    #[test]
    fn a_store_stamped_with_label_placeholder_token_rows_is_refused() {
        let (_dir, store) = store();
        store.set_store_layout(7).unwrap();
        assert_eq!(
            store.check_store_layout().unwrap(),
            Err(StoreLayoutMismatch::stamped(Some(7)))
        );
    }

    #[test]
    fn a_store_of_another_layout_is_refused_naming_both() {
        let (_dir, store) = store();
        store.set_store_layout(STORE_LAYOUT + 1).unwrap();
        let mismatch = store.check_store_layout().unwrap().unwrap_err();
        assert_eq!(
            mismatch,
            StoreLayoutMismatch::stamped(Some(STORE_LAYOUT + 1))
        );
        let words = mismatch.to_string();
        assert!(
            words.contains(&format!("store layout {}", STORE_LAYOUT + 1))
                && words.contains(&format!("store layout {STORE_LAYOUT}")),
            "{words}"
        );
    }

    /// A row in any domain column family makes a store not fresh: the old
    /// layout's 32-byte label row alone is enough.
    #[test]
    fn any_domain_row_makes_a_store_not_fresh() {
        let (_dir, store) = store();
        let value = bincode::serialize(&ScriptInfo {
            name: "Default Lock".into(),
            ..Default::default()
        })
        .unwrap();
        store
            .put_cf(store.cf_script_info(), &[0x9b; 32], &value)
            .unwrap();
        assert_eq!(
            store.check_store_layout().unwrap(),
            Err(StoreLayoutMismatch::stamped(None))
        );
    }

    /// resync fix round 2, A: a store an earlier build left (without the
    /// column families this build added; one row and no stamp, or stamped 7)
    /// is read for what it is, writing nothing in it, and refused: by the
    /// inspection an opener makes before it opens, and by a secondary of it
    /// (the API, `status`, the TUI, the sequencer). The store's files and
    /// column families are as they were, so the earlier build still opens it.
    #[test]
    fn a_store_of_an_earlier_layout_is_read_without_writing_and_refused() {
        use earlier_layout_for_test::{
            create_earlier_layout_domain_store, store_files_and_column_families,
            ADDED_SINCE_LAYOUT_7,
        };
        for stamp in [None, Some(7)] {
            let dir = tempfile::tempdir().unwrap();
            let domain = dir.path().join("domain");
            create_earlier_layout_domain_store(&domain, stamp);
            let before = store_files_and_column_families(&domain);
            let assert_refused = |mismatch: StoreLayoutMismatch| {
                assert_eq!(mismatch.found, stamp);
                let words = mismatch.to_string();
                for added in ADDED_SINCE_LAYOUT_7 {
                    assert!(words.contains(added), "stamp {stamp:?}: {words}");
                }
                assert!(
                    words.contains(&format!(
                        "this build reads and writes store layout {STORE_LAYOUT}"
                    )) && words.contains("delete RocksDB and re-sync from genesis"),
                    "{words}"
                );
            };

            let inspected = inspect_domain_store(&domain, &dir.path().join("inspect"));
            match inspected {
                Ok(Some(Err(mismatch))) => assert_refused(mismatch),
                other => panic!("stamp {stamp:?}: the inspection refuses it, got {other:?}"),
            }

            let secondary =
                CkbadgerStore::open_domain_secondary(&domain, &dir.path().join("secondary"));
            match secondary.map(|store| store.check_store_layout()) {
                Ok(Ok(Err(mismatch))) => assert_refused(mismatch),
                Ok(other) => panic!("stamp {stamp:?}: a secondary refuses it, got {other:?}"),
                Err(error) => {
                    panic!("stamp {stamp:?}: a secondary opens it to refuse it: {error:#}")
                }
            }

            assert_eq!(
                store_files_and_column_families(&domain),
                before,
                "stamp {stamp:?}: the refused store is as it was"
            );
        }
    }

    /// resync fix round 3, tests SHOULD-1: the column families alone refuse
    /// a store no other fact tells apart (each read by the inspection and by
    /// a secondary, nothing written in it): an earlier build's EMPTY store
    /// (its column families, no row, no stamp: never what a creating open of
    /// this build leaves, which makes the newest column families first); an
    /// earlier build's column families stamped with this build's layout; and
    /// this build's column families and one it does not know, stamped with
    /// this build's layout (refused, not opened).
    #[test]
    fn the_column_families_alone_refuse_a_store() {
        use earlier_layout_for_test::{
            create_domain_store_by_hand, earlier_layout_column_families,
            store_files_and_column_families, ADDED_SINCE_LAYOUT_7,
        };
        let earlier = earlier_layout_column_families();
        let mut with_unknown: Vec<&str> = DOMAIN_CFS.to_vec();
        with_unknown.push("a_column_family_of_a_later_build");
        // (the case, its column families, its stamp, the ones missing, the unknown ones)
        type Case<'a> = (
            &'a str,
            &'a [&'a str],
            Option<u32>,
            &'a [&'a str],
            &'a [&'a str],
        );
        let cases: [Case; 3] = [
            (
                "an earlier build's empty store",
                earlier.as_slice(),
                None,
                &ADDED_SINCE_LAYOUT_7,
                &[],
            ),
            (
                "an earlier build's column families stamped with this layout",
                earlier.as_slice(),
                Some(STORE_LAYOUT),
                &ADDED_SINCE_LAYOUT_7,
                &[],
            ),
            (
                "a column family this build does not know",
                with_unknown.as_slice(),
                Some(STORE_LAYOUT),
                &[],
                &["a_column_family_of_a_later_build"],
            ),
        ];
        for (case, column_families, stamp, missing, unknown) in cases {
            let dir = tempfile::tempdir().unwrap();
            let domain = dir.path().join("domain");
            create_domain_store_by_hand(&domain, column_families, false, stamp);
            let before = store_files_and_column_families(&domain);
            let expected = StoreLayoutMismatch {
                found: stamp,
                column_families: StoreColumnFamilies {
                    missing: missing.iter().map(|name| name.to_string()).collect(),
                    unknown: unknown.iter().map(|name| name.to_string()).collect(),
                },
            };
            assert_eq!(
                inspect_domain_store(&domain, &dir.path().join("inspect")).unwrap(),
                Some(Err(expected.clone())),
                "{case}: the inspection refuses it"
            );
            let secondary =
                CkbadgerStore::open_domain_secondary(&domain, &dir.path().join("secondary"))
                    .unwrap();
            assert_eq!(
                secondary.check_store_layout().unwrap(),
                Err(expected),
                "{case}: a secondary refuses it"
            );
            drop(secondary);
            assert_eq!(
                store_files_and_column_families(&domain),
                before,
                "{case}: the refused store is as it was"
            );
        }
    }

    /// resync fix round 3, tests NOTE-2: the inspection's pass paths, on
    /// stores this build's open made: one never stamped (fresh), and one
    /// stamped with its stamp flushed to a table file the store's own options
    /// wrote (the inspection opens with options of its own).
    #[test]
    fn the_inspection_passes_a_fresh_store_and_a_stamped_one() {
        let dir = tempfile::tempdir().unwrap();
        let domain = dir.path().join("domain");
        drop(CkbadgerStore::open_domain(&domain).unwrap());
        assert_eq!(
            inspect_domain_store(&domain, &dir.path().join("inspect")).unwrap(),
            Some(Ok(StoreLayoutState::Fresh))
        );
        {
            let store = CkbadgerStore::open_domain(&domain).unwrap();
            store.set_store_layout(STORE_LAYOUT).unwrap();
            store.flush_column_families(&[CF_SYNC_META]).unwrap();
        }
        assert_eq!(
            inspect_domain_store(&domain, &dir.path().join("inspect")).unwrap(),
            Some(Ok(StoreLayoutState::Current))
        );
    }

    /// resync fix round 3, item 5 (logic N-3, tests NOTE-9): a store path
    /// given with a trailing separator (a config's `data/domain/`) names the
    /// same store, and the inspection of it creates nothing inside it: its
    /// secondary's directory is the store's sibling.
    #[test]
    fn an_inspection_through_a_path_with_a_trailing_separator_creates_nothing_in_the_store() {
        use earlier_layout_for_test::{
            create_earlier_layout_domain_store, store_files_and_column_families,
        };
        let dir = tempfile::tempdir().unwrap();
        let domain = dir.path().join("domain");
        create_earlier_layout_domain_store(&domain, Some(7));
        let before = store_files_and_column_families(&domain);
        let given = std::path::PathBuf::from(format!("{}/", domain.display()));
        let secondary =
            crate::store::secondary_store_path(&given, crate::store::SecondaryStoreOwner::Inspect);
        assert!(matches!(
            inspect_domain_store(&given, &secondary).unwrap(),
            Some(Err(_))
        ));
        assert_eq!(
            store_files_and_column_families(&domain),
            before,
            "nothing is created inside the store"
        );
    }

    /// resync fix round 3, M (logic MUST-1): RocksDB writes `CURRENT` before
    /// a creating open has made every column family, so a reader can meet
    /// this build's store holding `default` and the first column families of
    /// [`domain_creation_order`], no row and no stamp. That store is still
    /// being created: not ready (look again), never refused, by the inspection
    /// an opener makes and by a secondary; nothing is written in it.
    #[test]
    fn a_store_still_being_created_is_not_ready_never_refused() {
        use earlier_layout_for_test::{create_store_in_creation, store_files_and_column_families};
        let all = domain_creation_order().len();
        for made in [0, 1, all / 2, all - 1] {
            let dir = tempfile::tempdir().unwrap();
            let domain = dir.path().join("domain");
            create_store_in_creation(&domain, made);
            let before = store_files_and_column_families(&domain);
            assert_eq!(
                inspect_domain_store(&domain, &dir.path().join("inspect")).unwrap(),
                Some(Ok(StoreLayoutState::NotReady)),
                "{made} of {all} made: the inspection waits"
            );
            let secondary =
                CkbadgerStore::open_domain_secondary(&domain, &dir.path().join("secondary"))
                    .unwrap();
            assert_eq!(
                secondary.check_store_layout().unwrap(),
                Ok(StoreLayoutState::NotReady),
                "{made} of {all} made: a secondary waits"
            );
            drop(secondary);
            assert_eq!(
                store_files_and_column_families(&domain),
                before,
                "{made} of {all} made: nothing is written in it"
            );
        }
    }

    /// A partial set of this build's column families beside THIS build's
    /// stamp is not ready: the stamp says this build made the store, and the
    /// list is the stale half of a reader's two reads (the families listed
    /// before the creating open ended, the stamp read after; resync fix round
    /// 4, A). Beside another layout's stamp, or beside rows with no stamp
    /// (this build stamps before any row), it is refused. Re-pinned on purpose
    /// (round 3 refused it beside this build's stamp too, which the race of
    /// the two reads turns into a refusal of this build's healthy store).
    #[test]
    fn a_partial_store_is_not_ready_beside_this_builds_stamp_and_refused_beside_another_or_rows() {
        use earlier_layout_for_test::create_domain_store_by_hand;
        let order = domain_creation_order();
        let made = &order[..order.len() - 1];
        for (row, stamp, refused) in [
            (false, Some(STORE_LAYOUT), false),
            (true, Some(STORE_LAYOUT), false),
            (false, Some(7), true),
            (true, None, true),
        ] {
            let dir = tempfile::tempdir().unwrap();
            let domain = dir.path().join("domain");
            create_domain_store_by_hand(&domain, made, row, stamp);
            match inspect_domain_store(&domain, &dir.path().join("inspect")).unwrap() {
                Some(Err(mismatch)) if refused => assert_eq!(mismatch.found, stamp),
                Some(Ok(StoreLayoutState::NotReady)) if !refused => {}
                other => panic!("row {row}, stamp {stamp:?}: refused {refused}, got {other:?}"),
            }
        }
    }

    #[test]
    fn a_stamp_that_is_not_four_bytes_is_an_error() {
        let (_dir, store) = store();
        store
            .put_cf(store.cf_sync_meta(), sync_meta_keys::STORE_LAYOUT, &[1, 0])
            .unwrap();
        let err = store.get_store_layout().unwrap_err().to_string();
        assert!(err.contains("store_layout record is 2 bytes"), "{err}");
    }
}
