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
use crate::store::{CkbadgerStore, CF_SYNC_META, DOMAIN_CFS};
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
/// has not stamped it yet. Any other is the inner error: another stamp, rows
/// and no stamp (a build from before stamps), or column families that are not
/// this build's (an earlier build's store, empty or not). The outer error is
/// a read failure.
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
        let mut opts = rocksdb::Options::default();
        opts.create_if_missing(true);
        opts.create_missing_column_families(true);
        let column_families: Vec<&str> = DOMAIN_CFS
            .iter()
            .copied()
            .filter(|name| !ADDED_SINCE_LAYOUT_7.contains(name))
            .collect();
        let db = rocksdb::DB::open_cf(&opts, path, column_families).unwrap();
        let sync_meta = db.cf_handle(CF_SYNC_META).unwrap();
        db.put_cf(sync_meta, sync_meta_keys::NETWORK_IDENTITY, b"mainnet")
            .unwrap();
        if let Some(stamp) = stamp {
            db.put_cf(sync_meta, sync_meta_keys::STORE_LAYOUT, stamp.to_le_bytes())
                .unwrap();
        }
    }

    /// What a refusal must leave as it found it: the store directory's files
    /// (name, size) and the store's column families, each sorted.
    pub fn store_files_and_column_families(path: &Path) -> (Vec<(String, u64)>, Vec<String>) {
        let mut files: Vec<(String, u64)> = std::fs::read_dir(path)
            .unwrap()
            .map(|entry| {
                let entry = entry.unwrap();
                (
                    entry.file_name().to_string_lossy().into_owned(),
                    entry.metadata().unwrap().len(),
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
