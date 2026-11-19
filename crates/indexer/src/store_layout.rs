//! The store layout guard: the indexer owns the domain store's layout stamp
//! (`ckbadger_store::STORE_LAYOUT`, `sync_meta` `store_layout`).
//!
//! Every process that writes the chain stores calls [`ensure_store_layout`]
//! before its first write: the indexer (`entry::run_indexer_sync`, before the
//! network identity, its first write) and `ckbadger label-import`
//! (`entry::run_label_import`). A fresh store is stamped; a store of the
//! build's layout passes; any other store is refused, nothing written, with
//! both layouts named and the remedy. The API reads the same check when it
//! opens the store and never stamps.

use anyhow::Result;
use ckbadger_store::{CkbadgerStore, StoreLayoutState, STORE_LAYOUT};

use crate::lifecycle::RebuildRequiredError;

/// Stamp a fresh store with [`STORE_LAYOUT`], pass a store of that layout,
/// and refuse any other (`RebuildRequiredError`) before anything is written.
pub fn ensure_store_layout(store: &CkbadgerStore) -> Result<()> {
    match store.check_store_layout()? {
        Ok(StoreLayoutState::Current) => Ok(()),
        Ok(StoreLayoutState::Fresh) => store.set_store_layout(STORE_LAYOUT),
        Err(mismatch) => Err(anyhow::Error::new(RebuildRequiredError::new(
            mismatch.diagnosis(),
        ))),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use ckbadger_store::store::DOMAIN_CFS;
    use ckbadger_store::types::ScriptInfo;

    /// Every row of every domain column family, in order: what a refusal must
    /// leave as it found it.
    pub(crate) fn domain_rows(store: &CkbadgerStore) -> Vec<(String, Vec<u8>, Vec<u8>)> {
        let mut rows = Vec::new();
        for name in DOMAIN_CFS {
            if !store.cf_handle_exists(name) {
                continue;
            }
            for row in store.iterator_cf(store.cf(name), rocksdb::IteratorMode::Start) {
                let (key, value) = row.unwrap();
                rows.push((name.to_string(), key.to_vec(), value.to_vec()));
            }
        }
        rows
    }

    /// A store an older build wrote: its network identity and a `script_info`
    /// row keyed by the code hash alone, and no layout stamp.
    pub(crate) fn seed_pre_layout_store(store: &CkbadgerStore) {
        store.set_network_identity("mainnet").unwrap();
        let value = bincode::serialize(&ScriptInfo {
            name: "Default Lock".into(),
            ..Default::default()
        })
        .unwrap();
        store
            .put_cf(store.cf_script_info(), &[0x9b; 32], &value)
            .unwrap();
    }

    #[test]
    fn a_fresh_store_is_stamped_with_the_builds_layout() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        ensure_store_layout(&store).unwrap();
        assert_eq!(store.get_store_layout().unwrap(), Some(STORE_LAYOUT));
        // and passes from then on
        ensure_store_layout(&store).unwrap();
    }

    #[test]
    fn a_store_from_before_layouts_is_refused_before_anything_is_written() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        seed_pre_layout_store(&store);
        let before = domain_rows(&store);

        let error = ensure_store_layout(&store).unwrap_err();
        assert!(crate::lifecycle::is_rebuild_required(&error), "{error:#}");
        let words = error.to_string();
        assert!(
            words.contains("no store layout stamp")
                && words.contains(&format!("store layout {STORE_LAYOUT}"))
                && words.contains("delete RocksDB and re-sync from genesis"),
            "{words}"
        );
        assert_eq!(domain_rows(&store), before, "a refusal writes nothing");
        assert_eq!(store.get_store_layout().unwrap(), None);
    }

    /// The re-sync set (resync/all) changes the layout once for all of its
    /// changes. A store built by rd/script-pair-store alone carries stamp 1
    /// (scripts by their pair) but its activity rows count a transaction in
    /// several kinds and its participants carry no senders bit: refused, with
    /// nothing written.
    #[test]
    fn a_store_stamped_by_the_script_pair_branch_alone_is_refused_before_anything_is_written() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        store.set_network_identity("mainnet").unwrap();
        // that branch's label row: keyed by the pair (code hash ‖ hash type)
        let mut key = vec![0x9b; 32];
        key.push(1);
        let value = bincode::serialize(&ScriptInfo {
            name: "Default Lock".into(),
            ..Default::default()
        })
        .unwrap();
        store.put_cf(store.cf_script_info(), &key, &value).unwrap();
        store.set_store_layout(1).unwrap();
        let before = domain_rows(&store);

        let error = ensure_store_layout(&store).unwrap_err();
        assert!(crate::lifecycle::is_rebuild_required(&error), "{error:#}");
        let words = error.to_string();
        assert!(
            words.contains("store layout 1")
                && words.contains(&format!("store layout {STORE_LAYOUT}"))
                && words.contains("delete RocksDB and re-sync from genesis"),
            "{words}"
        );
        assert_eq!(domain_rows(&store), before, "a refusal writes nothing");
        assert_eq!(store.get_store_layout().unwrap(), Some(1));
    }

    /// A store built by resync/int (fix/activity-kinds and the senders bit)
    /// alone carries no stamp: its one-kind activity rows are there, its
    /// script labels are keyed by the code hash alone. Refused, nothing
    /// written.
    #[test]
    fn a_store_built_by_the_activity_kinds_branch_alone_is_refused_before_anything_is_written() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        store.set_network_identity("mainnet").unwrap();
        // that branch's day row: the one-kind layout, its prefix and all
        store
            .put_daily_activity_stats(
                "20261119",
                &ckbadger_store::types::DailyActivityStats::default(),
            )
            .unwrap();
        let before = domain_rows(&store);

        let error = ensure_store_layout(&store).unwrap_err();
        assert!(crate::lifecycle::is_rebuild_required(&error), "{error:#}");
        let words = error.to_string();
        assert!(
            words.contains("no store layout stamp")
                && words.contains(&format!("store layout {STORE_LAYOUT}"))
                && words.contains("delete RocksDB and re-sync from genesis"),
            "{words}"
        );
        assert_eq!(domain_rows(&store), before, "a refusal writes nothing");
        assert_eq!(store.get_store_layout().unwrap(), None);
    }

    /// C7 (resync/type-codes) changes the layout once more: a store built by
    /// resync/all alone carries stamp 2, and its activities rows end at the
    /// participants, without the recognised type codes that ran (the row's
    /// strict decode refuses such a row too, the row's own guard). Refused
    /// before anything is written, both layouts named.
    #[test]
    fn a_store_stamped_by_the_resync_set_alone_is_refused_before_anything_is_written() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        store.set_network_identity("mainnet").unwrap();
        store.set_store_layout(2).unwrap();
        let before = domain_rows(&store);

        let error = ensure_store_layout(&store).unwrap_err();
        assert!(crate::lifecycle::is_rebuild_required(&error), "{error:#}");
        let words = error.to_string();
        assert!(
            words.contains("store layout 2")
                && words.contains(&format!("store layout {STORE_LAYOUT}"))
                && words.contains("recognised type codes")
                && words.contains("delete RocksDB and re-sync from genesis"),
            "{words}"
        );
        assert_eq!(domain_rows(&store), before, "a refusal writes nothing");
        assert_eq!(store.get_store_layout().unwrap(), Some(2));
    }

    #[test]
    fn a_store_of_another_layout_is_refused_naming_both() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        store.set_store_layout(STORE_LAYOUT + 1).unwrap();
        let before = domain_rows(&store);
        let error = ensure_store_layout(&store).unwrap_err();
        assert!(crate::lifecycle::is_rebuild_required(&error));
        assert!(
            error
                .to_string()
                .contains(&format!("store layout {}", STORE_LAYOUT + 1)),
            "{error}"
        );
        assert_eq!(domain_rows(&store), before);
    }
}
