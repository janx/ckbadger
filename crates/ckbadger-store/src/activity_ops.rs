//! Activity query operations.

use crate::global_bucket::GlobalBucket;
use crate::keys;
use crate::store::*;
use crate::types::*;

use crate::bytes_to_hex;

/// Necessary-but-not-sufficient filter check using only the tag bitmask
/// stored in `AddrTxValue`. Used to skip CF_TX_ACTIONS reads for entries
/// that can't possibly match. Sufficient for all filters except `protocol:X`,
/// where this only verifies `TAG_PROTOCOL` is set and the protocol-name
/// match still requires the full `TxActions` payload.
fn tags_pass_filter(tags: u16, filter: Option<&str>) -> bool {
    match filter {
        None | Some("") | Some("all") => true,
        Some("ckb") => {
            let non_ckb_mask = TAG_TOKEN
                | TAG_OBJECT
                | TAG_IDENTITY
                | TAG_DAO
                | TAG_PROTOCOL
                | TAG_TYPE_CALL
                | TAG_LOCK_CALL;
            tags & non_ckb_mask == 0
        }
        Some("token") => tags & TAG_TOKEN != 0,
        Some("object") => tags & TAG_OBJECT != 0,
        Some("identity") => tags & TAG_IDENTITY != 0,
        Some("dao") => tags & TAG_DAO != 0,
        Some("type_call") => tags & TAG_TYPE_CALL != 0,
        Some("lock_call") => tags & TAG_LOCK_CALL != 0,
        Some("script") => tags & (TAG_TYPE_CALL | TAG_LOCK_CALL) != 0,
        Some(proto) if proto.starts_with("protocol:") => tags & TAG_PROTOCOL != 0,
        Some(_) => false,
    }
}

/// A page of completed `TxActions` with the block headers its completion read
/// (one per distinct block): a reader that also needs a header field (the
/// chain time) takes it from here instead of reading the headers again.
pub type TxActionsPage = (
    Vec<TxActions>,
    std::collections::HashMap<i64, CachedBlockHeader>,
);

/// `activities` holds no cellbase row: the writers skip cellbases (their
/// actions are interpreted from the node when a page asks). One here is a
/// writer bug, named by its position.
pub fn ensure_no_cellbase_activity(rows: &[TxActions]) -> anyhow::Result<()> {
    if let Some(actions) = rows.iter().find(|actions| actions.is_cellbase) {
        anyhow::bail!(
            "activities holds a cellbase row at block={}, tx_idx={} (tx 0x{})",
            actions.block_number,
            actions.tx_index,
            bytes_to_hex(&actions.tx_hash)
        );
    }
    Ok(())
}

/// One transaction a reader has already resolved: its canonical position,
/// its `tx_index` entry there and its block's hash. The input of
/// [`CkbadgerStore::tx_actions_of`], which completes `activities` rows from it
/// without reading `tx_index` or `block_headers` again.
#[derive(Debug, Clone, Copy)]
pub struct ResolvedTx<'a> {
    pub block_number: i64,
    pub tx_index: i32,
    pub entry: &'a TxIndexEntry,
    pub block_hash: &'a [u8],
}

impl CkbadgerStore {
    /// The `TxActions` of a page of transactions the reader has already
    /// resolved, in order: one `multi_get` on `activities` for the page's
    /// non-cellbase transactions, completed from the reader's entries and
    /// block hashes (no `tx_index` or `block_headers` read). A cellbase has no
    /// `activities` row and reads as `None`; a non-cellbase transaction
    /// without its row is an error naming it.
    pub fn tx_actions_of(&self, txs: &[ResolvedTx<'_>]) -> anyhow::Result<Vec<Option<TxActions>>> {
        let positions: Vec<(i64, i32)> = txs
            .iter()
            .filter(|tx| !tx.entry.is_cellbase)
            .map(|tx| (tx.block_number, tx.tx_index))
            .collect();
        let mut stored = self.stored_tx_actions_at(&positions)?.into_iter();
        txs.iter()
            .map(|tx| {
                if tx.entry.is_cellbase {
                    return Ok(None);
                }
                let row = stored
                    .next()
                    .expect("one stored row per non-cellbase position")
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                            "activities has no row for non-cellbase transaction 0x{} at block={}, tx_idx={}",
                            bytes_to_hex(&tx.entry.tx_hash),
                            tx.block_number,
                            tx.tx_index
                        )
                    })?;
                Ok(Some(TxActions::from_stored(
                    row,
                    tx.block_number,
                    tx.tx_index,
                    tx.entry,
                    tx.block_hash.to_vec(),
                )))
            })
            .collect()
    }

    /// Stored `activities` rows completed into `TxActions` — the one way a
    /// reader turns a row into what the API serves: the tx hash, timestamp and
    /// cellbase flag from `entries` (`tx_index` at each row's position), the
    /// block hash from `block_headers`, read with one `multi_get` for every
    /// distinct block of the slice. A block without a header is an error. The
    /// headers read come back with the rows.
    fn tx_actions_from_stored(
        &self,
        rows: Vec<(i64, i32, StoredTxActions)>,
        entries: &[TxIndexEntry],
    ) -> anyhow::Result<TxActionsPage> {
        anyhow::ensure!(
            rows.len() == entries.len(),
            "activities completion: {} rows but {} tx_index entries",
            rows.len(),
            entries.len()
        );
        let mut blocks: Vec<i64> = rows.iter().map(|(block, _, _)| *block).collect();
        blocks.sort_unstable();
        blocks.dedup();
        let headers = self.get_block_headers_batch(&blocks)?;
        let actions = rows
            .into_iter()
            .zip(entries)
            .map(|((block, tx_idx, stored), entry)| {
                let header = headers.get(&block).ok_or_else(|| {
                    anyhow::anyhow!(
                        "activities row at block={block}, tx_idx={tx_idx} has no block header"
                    )
                })?;
                Ok(TxActions::from_stored(
                    stored,
                    block,
                    tx_idx,
                    entry,
                    header.hash.clone(),
                ))
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
        Ok((actions, headers))
    }

    /// [`Self::tx_actions_from_stored`] for rows whose `tx_index` entries are
    /// not read yet: one `multi_get` on `tx_index` for the slice.
    fn complete_tx_actions(
        &self,
        rows: Vec<(i64, i32, StoredTxActions)>,
    ) -> anyhow::Result<TxActionsPage> {
        let positions: Vec<(i64, i32)> = rows.iter().map(|(b, t, _)| (*b, *t)).collect();
        let entries = self.tx_index_entries_at("activities", &positions)?;
        self.tx_actions_from_stored(rows, &entries)
    }

    /// The stored `activities` rows at `positions`, in order, read with one
    /// `multi_get` (`None` where a position has no row).
    fn stored_tx_actions_at(
        &self,
        positions: &[(i64, i32)],
    ) -> anyhow::Result<Vec<Option<StoredTxActions>>> {
        if positions.is_empty() {
            return Ok(Vec::new());
        }
        let cf = self.cf_tx_actions();
        let keys: Vec<[u8; keys::TX_ACTIONS_KEY_SIZE]> = positions
            .iter()
            .map(|(block, tx_idx)| keys::encode_tx_actions_key(*block, *tx_idx))
            .collect();
        let values =
            self.multi_get_cf_sorted(keys.iter().map(|key| (cf, key.as_slice())).collect());
        positions
            .iter()
            .zip(values)
            .map(|((block, tx_idx), value)| {
                value
                    .map_err(|e| {
                        anyhow::anyhow!(
                            "reading activities at block={block}, tx_idx={tx_idx} failed: {e}"
                        )
                    })?
                    .map(|bytes| {
                        decode_stored_tx_actions(&bytes).map_err(|e| {
                            anyhow::anyhow!("activities at block={block}, tx_idx={tx_idx}: {e:#}")
                        })
                    })
                    .transpose()
            })
            .collect()
    }

    /// The `TxActions` of the transaction at `(block_num, tx_idx)`, which must
    /// be `tx_hash` (a caller that names the hash expects that transaction
    /// there; another one is an error).
    pub fn get_tx_actions(
        &self,
        block_num: i64,
        tx_idx: i32,
        tx_hash: &[u8],
    ) -> anyhow::Result<Option<TxActions>> {
        let Some(actions) = self.get_tx_actions_at(block_num, tx_idx)? else {
            return Ok(None);
        };
        anyhow::ensure!(
            actions.tx_hash == tx_hash,
            "activities at block={}, tx_idx={} is transaction 0x{}, not 0x{}",
            block_num,
            tx_idx,
            bytes_to_hex(&actions.tx_hash),
            bytes_to_hex(tx_hash)
        );
        Ok(Some(actions))
    }

    pub fn list_tx_actions_recent(
        &self,
        limit: usize,
        cursor: Option<(i64, i32)>,
    ) -> anyhow::Result<Vec<TxActions>> {
        self.list_tx_actions_recent_in_range(limit, cursor, None)
    }

    /// Recent actions strictly above `after_block`, stopping at the key before
    /// deserializing older rows. Used by incremental, read-only API windows.
    /// The page is completed with one `tx_index` and one `block_headers` read.
    pub fn list_tx_actions_recent_in_range(
        &self,
        limit: usize,
        cursor: Option<(i64, i32)>,
        after_block: Option<i64>,
    ) -> anyhow::Result<Vec<TxActions>> {
        Ok(self
            .list_tx_actions_recent_page(limit, cursor, after_block)?
            .0)
    }

    /// [`Self::list_tx_actions_recent_in_range`] with the block headers the
    /// page's completion read.
    pub fn list_tx_actions_recent_page(
        &self,
        limit: usize,
        cursor: Option<(i64, i32)>,
        after_block: Option<i64>,
    ) -> anyhow::Result<TxActionsPage> {
        if limit == 0 {
            return Ok((Vec::new(), std::collections::HashMap::new()));
        }

        let start_key = cursor
            .map(|(block_num, tx_idx)| keys::encode_tx_actions_seek_after_key(block_num, tx_idx));

        let iter = match start_key.as_ref() {
            Some(key) => self.iterator_cf(
                self.cf_tx_actions(),
                rocksdb::IteratorMode::From(key, rocksdb::Direction::Forward),
            ),
            None => self.iterator_cf(self.cf_tx_actions(), rocksdb::IteratorMode::Start),
        };

        let mut rows = Vec::new();
        for item in iter {
            let (key, value) = item.map_err(|e| {
                anyhow::anyhow!(
                    "failed to iterate tx actions in list_tx_actions_recent: {}",
                    e
                )
            })?;
            let (block_num, tx_idx) = keys::decode_stored_tx_actions_key(&key)?;
            if after_block.is_some_and(|number| block_num <= number) {
                break;
            }
            let stored = decode_stored_tx_actions(&value).map_err(|e| {
                anyhow::anyhow!("activities at block={block_num}, tx_idx={tx_idx}: {e:#}")
            })?;
            rows.push((block_num, tx_idx, stored));
            if rows.len() >= limit {
                break;
            }
        }

        self.complete_tx_actions(rows)
    }

    /// Positions `(block_num, tx_idx)` of one global activity bucket, newest
    /// first, strictly older than `cursor` when one is given.
    ///
    /// `CF_TX_ACTIONS_BY_BUCKET` is maintained by the indexer on both sync
    /// paths and deleted above the fork point on rollback, so every row names a
    /// canonical transaction; readers do not re-check canonicity.
    pub fn list_tx_actions_by_bucket(
        &self,
        bucket: GlobalBucket,
        cursor: Option<(i64, i32)>,
        limit: usize,
    ) -> anyhow::Result<Vec<(i64, i32)>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        // Seeking to `key ‖ 0x00` lands on the first key strictly after the
        // cursor's own key, so the cursor row itself is never returned.
        let start: Vec<u8> = match cursor {
            Some((block_num, tx_idx)) => {
                let mut seek =
                    keys::encode_tx_actions_by_bucket_key(bucket, block_num, tx_idx).to_vec();
                seek.push(0);
                seek
            }
            None => vec![bucket.as_byte()],
        };
        let iter = self.iterator_cf(
            self.cf_tx_actions_by_bucket(),
            rocksdb::IteratorMode::From(&start, rocksdb::Direction::Forward),
        );
        let mut positions = Vec::with_capacity(limit);
        for item in iter {
            let (key, _) = item.map_err(|e| {
                anyhow::anyhow!(
                    "failed to iterate tx_actions_by_bucket: bucket={}, cursor={:?}: {}",
                    bucket.as_filter(),
                    cursor,
                    e
                )
            })?;
            if key.first() != Some(&bucket.as_byte()) {
                break;
            }
            let (_, block_num, tx_idx) = keys::decode_tx_actions_by_bucket_key(&key)?;
            positions.push((block_num, tx_idx));
            if positions.len() >= limit {
                break;
            }
        }
        Ok(positions)
    }

    /// The `TxActions` row at one transaction position (`None` when the
    /// position has no `activities` row: a cellbase, or no transaction).
    pub fn get_tx_actions_at(
        &self,
        block_num: i64,
        tx_idx: i32,
    ) -> anyhow::Result<Option<TxActions>> {
        let [stored] = self
            .stored_tx_actions_at(&[(block_num, tx_idx)])?
            .try_into()
            .expect("one position reads one row");
        let Some(stored) = stored else {
            return Ok(None);
        };
        let [actions] = self
            .complete_tx_actions(vec![(block_num, tx_idx, stored)])?
            .0
            .try_into()
            .expect("one row completes to one TxActions");
        Ok(Some(actions))
    }

    /// One page of a global activity bucket with each row's `TxActions`
    /// loaded: newest first, strictly older than `cursor`. A bucket row whose
    /// `TxActions` is missing, or that does not classify into this bucket, is
    /// an index invariant violation and fails with its position.
    pub fn list_tx_actions_page_by_bucket(
        &self,
        bucket: GlobalBucket,
        cursor: Option<(i64, i32)>,
        limit: usize,
    ) -> anyhow::Result<Vec<TxActions>> {
        Ok(self
            .list_tx_actions_page_by_bucket_with_entries(bucket, cursor, limit)?
            .into_iter()
            .map(|(actions, _)| actions)
            .collect())
    }

    /// [`Self::list_tx_actions_page_by_bucket`] with each row's `tx_index`
    /// entry — the completion's own read — for a reader that serves the
    /// entries too (one `tx_index` read for the page, not two).
    pub fn list_tx_actions_page_by_bucket_with_entries(
        &self,
        bucket: GlobalBucket,
        cursor: Option<(i64, i32)>,
        limit: usize,
    ) -> anyhow::Result<Vec<(TxActions, TxIndexEntry)>> {
        let rows = self.list_stored_tx_actions_page_by_bucket(bucket, cursor, limit)?;
        let positions: Vec<(i64, i32)> = rows.iter().map(|(b, t, _)| (*b, *t)).collect();
        let entries = self.tx_index_entries_at("activities", &positions)?;
        let (actions, _) = self.tx_actions_from_stored(rows, &entries)?;
        Ok(actions.into_iter().zip(entries).collect())
    }

    /// The stored `activities` rows of one page of a global bucket, newest
    /// first, strictly older than `cursor`: what the transactions did, not
    /// completed (no `tx_index` or `block_headers` read), for a reader that
    /// only counts their actions. A bucket row without its `activities` row,
    /// or whose row does not classify into this bucket, is an index invariant
    /// violation and fails with its position.
    pub fn list_stored_tx_actions_page_by_bucket(
        &self,
        bucket: GlobalBucket,
        cursor: Option<(i64, i32)>,
        limit: usize,
    ) -> anyhow::Result<Vec<(i64, i32, StoredTxActions)>> {
        let positions = self.list_tx_actions_by_bucket(bucket, cursor, limit)?;
        let mut rows = Vec::with_capacity(positions.len());
        for ((block_num, tx_idx), stored) in positions
            .iter()
            .copied()
            .zip(self.stored_tx_actions_at(&positions)?)
        {
            let stored = stored.ok_or_else(|| {
                anyhow::anyhow!(
                    "tx_actions_by_bucket row has no tx actions: bucket={}, block_num={}, tx_idx={}",
                    bucket.as_filter(),
                    block_num,
                    tx_idx
                )
            })?;
            let classified = crate::global_bucket::classify_stored_global_bucket(&stored);
            anyhow::ensure!(
                classified == bucket,
                "tx_actions_by_bucket row disagrees with its tx actions: bucket={}, classified={}, \
                 block_num={}, tx_idx={}",
                bucket.as_filter(),
                classified.as_filter(),
                block_num,
                tx_idx
            );
            rows.push((block_num, tx_idx, stored));
        }
        Ok(rows)
    }

    /// Return up to 64 most recent TxActions. `activities` holds no cellbase
    /// row (cellbase actions are never materialized), so these are the 64
    /// newest rows; a cellbase row is an error.
    pub fn get_latest_activities(&self) -> anyhow::Result<Vec<TxActions>> {
        const LATEST_ACTIVITY_LIMIT: usize = 64;
        let results = self.list_tx_actions_recent(LATEST_ACTIVITY_LIMIT, None)?;
        ensure_no_cellbase_activity(&results)?;
        Ok(results)
    }

    /// List activities for an address (lock_hash), newest first.
    ///
    /// Optionally start after the given `(block_num, tx_idx)` cursor.
    /// An optional `filter` narrows results: "ckb", "token", "object",
    /// "identity", "dao", "type_call", "lock_call", "script" (type or lock
    /// call), "protocol:X".
    /// Returns `Vec<TxActions>` for cursor construction via block_number/tx_index.
    pub fn list_activities(
        &self,
        lock_hash: &[u8],
        limit: usize,
        cursor: Option<(i64, i32)>,
        filter: Option<&str>,
    ) -> anyhow::Result<Vec<TxActions>> {
        if lock_hash.len() != 32 {
            anyhow::bail!(
                "list_activities expects 32-byte lock_hash, got {} bytes",
                lock_hash.len()
            );
        }
        if limit == 0 {
            return Ok(Vec::new());
        }

        let scan_limit = limit.max(128);
        let mut results = Vec::with_capacity(limit);
        let mut scan_cursor = cursor;
        loop {
            let rows = self.list_addr_tx_positions_with_kind(lock_hash, scan_limit, scan_cursor)?;
            if rows.is_empty() {
                break;
            }

            // Always advance the scan cursor by the last row we saw, even if the
            // tag pre-filter excludes the entire chunk — otherwise filtered scans
            // on top addresses would re-read the same chunk forever.
            let last_seen = rows.last().map(|(b, t, _, _)| (*b, *t));

            // Pre-filter on AddrTxValue.tags from the thin index so we don't
            // multi_get CF_TX_ACTIONS for entries that can't match. This is the
            // perf-critical path for filtered scans on high-cardinality addresses.
            let survivors: Vec<(i64, i32, &'static str)> = rows
                .iter()
                .filter(|(_, _, addr_tx_value, _)| tags_pass_filter(addr_tx_value.tags, filter))
                .map(|(block_num, tx_idx, _, kind)| (*block_num, *tx_idx, *kind))
                .collect();
            let surviving_positions: Vec<(i64, i32)> =
                survivors.iter().map(|(b, t, _)| (*b, *t)).collect();
            // The survivors' transactions and activity rows: one tx_index and
            // one activities read for the chunk (and one block_headers read
            // to complete them).
            let entries = self.tx_index_entries_named(&surviving_positions, |i| {
                format!("{} row of 0x{}", survivors[i].2, bytes_to_hex(lock_hash))
            })?;
            let stored = self.stored_tx_actions_at(&surviving_positions)?;
            let mut kept_rows = Vec::with_capacity(stored.len());
            let mut kept_entries = Vec::with_capacity(stored.len());
            for (((block_num, tx_idx, kind), entry), stored) in
                survivors.iter().zip(entries).zip(stored)
            {
                match stored {
                    Some(stored) => {
                        kept_rows.push((*block_num, *tx_idx, stored));
                        kept_entries.push(entry);
                    }
                    // A cellbase has an addr_txs row and, by design, no
                    // activities row; any other transaction without one is a
                    // lost row.
                    None if entry.is_cellbase => {}
                    None => anyhow::bail!(
                        "{kind} row of 0x{} at block={}, tx_idx={} (tx 0x{}) has no activities row",
                        bytes_to_hex(lock_hash),
                        block_num,
                        tx_idx,
                        bytes_to_hex(&entry.tx_hash)
                    ),
                }
            }
            for actions in self.tx_actions_from_stored(kept_rows, &kept_entries)?.0 {
                if Self::matches_activity_filter(&actions, lock_hash, filter)? {
                    results.push(actions);
                    if results.len() >= limit {
                        return Ok(results);
                    }
                }
            }

            if rows.len() < scan_limit {
                break;
            }
            let Some(next_cursor) = last_seen else {
                break;
            };
            scan_cursor = Some(next_cursor);
        }

        Ok(results)
    }

    /// Check whether a TxActions matches the given activity filter for a specific participant.
    ///
    /// Finds the participant matching `lock_hash`, then checks the `tags`
    /// bitmask. The address's rows (`addr_txs` / `addr_txs_by_prefix`) name
    /// only transactions it takes part in, and their keys are positions, so a
    /// transaction without the address as a participant is a stale row of
    /// another transaction once at that position: an error naming the
    /// address and the position, for every filter — never a row to skip.
    pub fn matches_activity_filter(
        actions: &TxActions,
        lock_hash: &[u8],
        filter: Option<&str>,
    ) -> anyhow::Result<bool> {
        let lock = <&[u8; 32]>::try_from(lock_hash).map_err(|_| {
            anyhow::anyhow!(
                "activity filter expects a 32-byte lock hash, got {} bytes",
                lock_hash.len()
            )
        })?;
        // One matcher for both participant identities: a full lock hash
        // compares whole, a protocol-named 20-byte prefix its first 20 bytes.
        let p = actions
            .participants
            .iter()
            .find(|p| p.id.matches(lock))
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "activity of address 0x{} at block={}, tx_idx={} (tx 0x{}) has no participant \
                     for the address: a stale addr_txs row",
                    bytes_to_hex(lock_hash),
                    actions.block_number,
                    actions.tx_index,
                    bytes_to_hex(&actions.tx_hash)
                )
            })?;
        Ok(match filter {
            None | Some("all") => true,
            Some(f) => {
                let tags = p.tags;
                match f {
                    "ckb" => {
                        // Pure CKB: no token, object, identity, dao, protocol, type_call, lock_call tags
                        let non_ckb_mask = TAG_TOKEN
                            | TAG_OBJECT
                            | TAG_IDENTITY
                            | TAG_DAO
                            | TAG_PROTOCOL
                            | TAG_TYPE_CALL
                            | TAG_LOCK_CALL;
                        tags & non_ckb_mask == 0
                    }
                    "token" => tags & TAG_TOKEN != 0,
                    "object" => tags & TAG_OBJECT != 0,
                    "identity" => tags & TAG_IDENTITY != 0,
                    "dao" => tags & TAG_DAO != 0,
                    "type_call" => tags & TAG_TYPE_CALL != 0,
                    "lock_call" => tags & TAG_LOCK_CALL != 0,
                    // Any script call: a type script OR a lock script ran
                    // for this participant (BA-35's single "Script" chip).
                    "script" => tags & (TAG_TYPE_CALL | TAG_LOCK_CALL) != 0,
                    proto if proto.starts_with("protocol:") => {
                        let protocol_name = &proto["protocol:".len()..];
                        tags & TAG_PROTOCOL != 0
                            && actions
                                .protocol_actions
                                .iter()
                                .any(|a| a.protocol == protocol_name)
                    }
                    _ => false,
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::batch::StoreBatch;
    use tempfile::TempDir;

    fn make_tx_actions(block_num: i64, tx_idx: i32, tx_hash_byte: u8) -> TxActions {
        TxActions {
            tx_hash: vec![tx_hash_byte; 32],
            block_hash: vec![0xBB; 32],
            block_number: block_num,
            tx_index: tx_idx,
            timestamp: 1_700_000_000 + block_num,
            is_cellbase: false,
            protocol_actions: vec![],
            type_calls: vec![],
            lock_calls: vec![],
            participants: vec![ParticipantDelta {
                id: ParticipantId::Lock([0xAA; 32]),
                ckb_delta: 100,
                used_delta: 0,
                item_deltas: vec![],
                tags: 0,
                roles: 0,
            }],
        }
    }

    /// `actions` as the writers leave it: its `activities` row, and the
    /// `tx_index` entry and block header its position resolves to.
    pub(crate) fn seed_activity(batch: &mut StoreBatch, actions: &TxActions) {
        batch.put_tx_actions(actions);
        batch.put_tx_index(
            actions.block_number,
            actions.tx_index,
            &TxIndexEntry {
                tx_hash: actions.tx_hash.as_slice().try_into().unwrap(),
                is_cellbase: actions.is_cellbase,
                timestamp: actions.timestamp,
                inputs_count: 1,
                outputs_count: 1,
                fee: 0,
                tx_size: 0,
                cycles: None,
                semantic_tags: 0,
            },
        );
        batch.put_block_header(
            actions.block_number,
            &CachedBlockHeader {
                hash: actions.block_hash.clone(),
                timestamp: actions.timestamp,
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
                chain_time_ms: actions.timestamp,
            },
        );
    }

    #[test]
    fn test_put_and_get_tx_actions_roundtrip() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();

        let actions = make_tx_actions(100, 0, 0xAA);
        let mut batch = StoreBatch::new(&store);
        seed_activity(&mut batch, &actions);
        batch.commit().unwrap();

        let got = store
            .get_tx_actions(100, 0, &[0xAA; 32])
            .unwrap()
            .expect("should find tx actions");
        assert_eq!(got.block_number, 100);
        assert_eq!(got.tx_index, 0);
        assert_eq!(got.tx_hash, vec![0xAA; 32]);
        assert_eq!(got.participants.len(), 1);
        assert_eq!(got.participants[0].ckb_delta, 100);
    }

    #[test]
    fn test_list_tx_actions_recent() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();

        let mut batch = StoreBatch::new(&store);
        // Insert 3 txs in ascending order (keys sort ascending by block_num desc + tx_idx desc)
        seed_activity(&mut batch, &make_tx_actions(100, 0, 0x01));
        seed_activity(&mut batch, &make_tx_actions(100, 1, 0x02));
        seed_activity(&mut batch, &make_tx_actions(101, 0, 0x03));
        batch.commit().unwrap();

        let results = store.list_tx_actions_recent(10, None).unwrap();
        assert_eq!(results.len(), 3);
    }

    #[test]
    fn recent_range_stops_before_deserializing_older_blocks() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        let mut batch = StoreBatch::new(&store);
        seed_activity(&mut batch, &make_tx_actions(100, 0, 1));
        seed_activity(&mut batch, &make_tx_actions(101, 0, 2));
        seed_activity(&mut batch, &make_tx_actions(101, 1, 3));
        batch.commit().unwrap();
        let old_key = keys::encode_tx_actions_key(100, 0);
        store
            .put_cf(store.cf_tx_actions(), &old_key, b"unreadable old payload")
            .unwrap();
        let first = store
            .list_tx_actions_recent_in_range(1, None, Some(100))
            .unwrap();
        assert_eq!(first[0].tx_index, 1);
        let rest = store
            .list_tx_actions_recent_in_range(512, Some((101, 1)), Some(100))
            .unwrap();
        assert_eq!(rest.len(), 1);
        assert_eq!(rest[0].block_number, 101);
        assert_eq!(rest[0].tx_index, 0);
        assert!(store.list_tx_actions_recent(512, None).is_err());
    }

    #[test]
    fn test_list_tx_actions_recent_with_limit() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();

        let mut batch = StoreBatch::new(&store);
        seed_activity(&mut batch, &make_tx_actions(100, 0, 0x01));
        seed_activity(&mut batch, &make_tx_actions(100, 1, 0x02));
        seed_activity(&mut batch, &make_tx_actions(101, 0, 0x03));
        batch.commit().unwrap();

        let results = store.list_tx_actions_recent(2, None).unwrap();
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_matches_activity_filter_all() {
        let actions = make_tx_actions(100, 0, 0xAA);
        let lock_hash = vec![0xAA; 32];
        assert!(CkbadgerStore::matches_activity_filter(&actions, &lock_hash, None).unwrap());
        assert!(CkbadgerStore::matches_activity_filter(&actions, &lock_hash, Some("all")).unwrap());
    }

    #[test]
    fn test_matches_activity_filter_ckb() {
        let actions = make_tx_actions(100, 0, 0xAA);
        let lock_hash = vec![0xAA; 32];
        // tags=0 means pure CKB
        assert!(CkbadgerStore::matches_activity_filter(&actions, &lock_hash, Some("ckb")).unwrap());
    }

    #[test]
    fn test_matches_activity_filter_token() {
        let mut actions = make_tx_actions(100, 0, 0xAA);
        actions.participants[0].tags = TAG_TOKEN;
        let lock_hash = vec![0xAA; 32];
        assert!(
            CkbadgerStore::matches_activity_filter(&actions, &lock_hash, Some("token")).unwrap()
        );
        assert!(
            !CkbadgerStore::matches_activity_filter(&actions, &lock_hash, Some("ckb")).unwrap()
        );
    }

    /// Review P1-3: with position keys, a transaction in which the address
    /// takes no part is the only sign of a stale address row — an error
    /// naming the address and the position, for every filter.
    #[test]
    fn test_matches_activity_filter_missing_participant_is_an_error() {
        let actions = make_tx_actions(100, 0, 0xAA);
        let unknown_lock = vec![0xFF; 32];
        for filter in [None, Some("all"), Some("token")] {
            let err = CkbadgerStore::matches_activity_filter(&actions, &unknown_lock, filter)
                .unwrap_err()
                .to_string();
            assert!(
                err.contains(&format!(
                    "activity of address 0x{} at block=100, tx_idx=0",
                    "ff".repeat(32)
                )) && err.contains("a stale addr_txs row"),
                "{filter:?}: {err}"
            );
        }
    }

    /// Delta review N6: `activities` holds no cellbase row (the writers skip
    /// cellbases); the feeds that read exactly their page refuse one, naming
    /// its position, instead of serving it.
    #[test]
    fn a_cellbase_row_in_activities_is_an_error() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        let mut batch = StoreBatch::new(&store);
        seed_activity(&mut batch, &make_tx_actions(100, 1, 0x55));
        let mut cellbase = make_tx_actions(101, 0, 0x66);
        cellbase.is_cellbase = true;
        seed_activity(&mut batch, &cellbase);
        batch.commit().unwrap();
        let err = store.get_latest_activities().unwrap_err().to_string();
        assert!(
            err.contains(&format!(
                "activities holds a cellbase row at block=101, tx_idx=0 (tx 0x{})",
                "66".repeat(32)
            )),
            "{err}"
        );
        let rows = store.list_tx_actions_recent(10, None).unwrap();
        assert!(ensure_no_cellbase_activity(&rows).is_err());
        assert!(ensure_no_cellbase_activity(&rows[1..]).is_ok());
    }

    /// Review 27: a caller that names the hash it expects at a position gets
    /// an error when the position holds another transaction.
    #[test]
    fn get_tx_actions_fails_when_the_position_holds_another_transaction() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        let mut batch = StoreBatch::new(&store);
        seed_activity(&mut batch, &make_tx_actions(5, 1, 0x11));
        batch.commit().unwrap();
        let err = store
            .get_tx_actions(5, 1, &[0x22; 32])
            .unwrap_err()
            .to_string();
        assert!(
            err.contains(&format!(
                "activities at block=5, tx_idx=1 is transaction 0x{}, not 0x{}",
                "11".repeat(32),
                "22".repeat(32)
            )),
            "{err}"
        );
    }

    /// Review 27: an address row of a non-cellbase transaction without its
    /// `activities` row is a lost row, named with the row kind, the address
    /// and the position.
    #[test]
    fn list_activities_fails_on_a_non_cellbase_position_without_an_activities_row() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        let lock = [0xAA; 32];
        let mut batch = StoreBatch::new(&store);
        let actions = make_tx_actions(100, 2, 0x33);
        seed_activity(&mut batch, &actions);
        batch.put_addr_tx(&lock, 100, 2, &AddrTxValue::new(1, false, true, 0));
        batch.commit().unwrap();
        // The activities row goes; the tx_index row and the address row stay.
        store
            .delete_cf(store.cf_tx_actions(), &keys::encode_tx_actions_key(100, 2))
            .unwrap();
        let err = store
            .list_activities(&lock, 10, None, None)
            .unwrap_err()
            .to_string();
        assert!(
            err.contains(&format!(
                "addr_txs row of 0x{} at block=100, tx_idx=2 (tx 0x{}) has no activities row",
                "aa".repeat(32),
                "33".repeat(32)
            )),
            "{err}"
        );
    }

    /// Review 27: completing a page needs the block header of every row; a
    /// row whose block has none is an error naming the position.
    #[test]
    fn completing_activities_fails_on_a_block_without_its_header() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        let mut batch = StoreBatch::new(&store);
        seed_activity(&mut batch, &make_tx_actions(100, 0, 0x44));
        batch.commit().unwrap();
        store
            .delete_cf(store.cf_block_headers(), &keys::encode_block_num(100))
            .unwrap();
        let err = store
            .list_tx_actions_recent(10, None)
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("activities row at block=100, tx_idx=0 has no block header"),
            "{err}"
        );
    }

    /// Review P1-3: a stale `addr_txs` row (its position now holds a
    /// transaction the address takes no part in) fails the address feed
    /// instead of shortening the page.
    #[test]
    fn list_activities_fails_on_an_addr_txs_row_of_a_transaction_without_the_address() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        let lock = [0xFE; 32];
        let mut batch = StoreBatch::new(&store);
        // The transaction at 100:0 names only 0xAA..; the address's row there
        // is stale.
        seed_activity(&mut batch, &make_tx_actions(100, 0, 0xAA));
        batch.put_addr_tx(&lock, 100, 0, &AddrTxValue::new(1, false, true, 0));
        batch.commit().unwrap();

        for filter in [None, Some("ckb")] {
            let err = store
                .list_activities(&lock, 10, None, filter)
                .unwrap_err()
                .to_string();
            assert!(
                err.contains(&format!(
                    "activity of address 0x{} at block=100, tx_idx=0",
                    "fe".repeat(32)
                )),
                "{filter:?}: {err}"
            );
        }
    }

    #[test]
    fn test_matches_activity_filter_script_is_type_or_lock_call() {
        let lock = vec![0xAA; 32];
        for (tags, expected) in [
            (TAG_TYPE_CALL, true),
            (TAG_LOCK_CALL, true),
            (TAG_TYPE_CALL | TAG_LOCK_CALL, true),
            (TAG_TOKEN, false),
            (0, false),
        ] {
            let mut actions = make_tx_actions(100, 0, 0xAA);
            actions.participants[0].tags = tags;
            assert_eq!(
                CkbadgerStore::matches_activity_filter(&actions, &lock, Some("script")).unwrap(),
                expected,
                "tags {tags:#b}"
            );
            assert_eq!(tags_pass_filter(tags, Some("script")), expected);
        }
    }

    #[test]
    fn test_matches_activity_filter_protocol() {
        let mut actions = make_tx_actions(100, 0, 0xAA);
        actions.participants[0].tags = TAG_PROTOCOL;
        actions.protocol_actions = vec![ProtocolAction::new(
            "rgbpp",
            "transfer",
            serde_json::json!({}),
        )];
        let lock_hash = vec![0xAA; 32];
        assert!(CkbadgerStore::matches_activity_filter(
            &actions,
            &lock_hash,
            Some("protocol:rgbpp")
        )
        .unwrap());
        assert!(!CkbadgerStore::matches_activity_filter(
            &actions,
            &lock_hash,
            Some("protocol:fiber")
        )
        .unwrap());
    }

    #[test]
    fn test_list_activities_filter_skips_cf_tx_actions_read_when_tags_reject() {
        // Asserts the perf optimization: when filter=token and the AddrTxValue.tags
        // does NOT contain TAG_TOKEN, list_activities must reject the entry from the
        // thin index alone without ever touching CF_TX_ACTIONS for it.
        //
        // Proof technique: write deliberately CORRUPT CF_TX_ACTIONS bytes for the
        // non-matching entry. If the implementation reads them, bincode::deserialize
        // returns an error. If the optimization is correct, the entry is skipped on
        // the AddrTxValue.tags check before the read happens.
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();

        let lock = vec![0xAA; 32];
        let matching_tx_hash = [0x01; 32];

        let mut batch = StoreBatch::new(&store);
        // Matching entry: AddrTxValue.tags has TAG_TOKEN. Real TxActions also written.
        batch.put_addr_tx(&lock, 100, 0, &AddrTxValue::new(50, false, true, TAG_TOKEN));
        batch.put_tx_index(
            100,
            0,
            &TxIndexEntry {
                tx_hash: matching_tx_hash,
                is_cellbase: false,
                timestamp: 1_700_000_100,
                inputs_count: 1,
                outputs_count: 1,
                fee: 0,
                tx_size: 0,
                cycles: None,
                semantic_tags: 0,
            },
        );
        let matching_actions = TxActions {
            tx_hash: matching_tx_hash.to_vec(),
            block_hash: vec![0xBB; 32],
            block_number: 100,
            tx_index: 0,
            timestamp: 1_700_000_100,
            is_cellbase: false,
            protocol_actions: vec![],
            type_calls: vec![],
            lock_calls: vec![],
            participants: vec![ParticipantDelta {
                id: ParticipantId::lock(&lock).unwrap(),
                ckb_delta: 50,
                used_delta: 0,
                item_deltas: vec![],
                tags: TAG_TOKEN,
                roles: 0,
            }],
        };
        seed_activity(&mut batch, &matching_actions);

        // Non-matching entry: AddrTxValue.tags = 0 (no TAG_TOKEN). CF_TX_ACTIONS
        // intentionally corrupt — list_activities(filter=token) must NOT read it.
        // No tx_index row either: a rejected entry is not resolved.
        batch.put_addr_tx(&lock, 99, 0, &AddrTxValue::new(10, false, true, 0));
        batch.commit().unwrap();

        let corrupt_key = keys::encode_tx_actions_key(99, 0);
        store
            .put_cf(store.cf_tx_actions(), &corrupt_key, &[0xFF; 7])
            .unwrap();

        let results = store
            .list_activities(&lock, 10, None, Some("token"))
            .expect("filter=token must skip non-token entries without reading their CF_TX_ACTIONS");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].block_number, 100);
        assert_eq!(results[0].tx_index, 0);
    }

    /// I6 (b): a wrong-width `activities` key is corruption the recent-window
    /// scan used to step over; it now names the key.
    #[test]
    fn list_tx_actions_recent_in_range_rejects_a_key_of_the_wrong_width() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        let short_key = [0x00; 13];
        store
            .put_cf(store.cf_tx_actions(), &short_key, b"orphan")
            .unwrap();
        let err = store
            .list_tx_actions_recent_in_range(10, None, None)
            .unwrap_err()
            .to_string();
        assert!(
            err.contains(&hex::encode(short_key)) && err.contains("13 bytes, expected 12"),
            "{err}"
        );
    }

    #[test]
    fn test_list_activities_rejects_non_32_byte_lock_hash() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        let err = store
            .list_activities(&[0xAA; 16], 10, None, None)
            .unwrap_err();
        assert!(err.to_string().contains("32-byte lock_hash"));
    }
}

#[cfg(test)]
mod prefix_participant_filter_tests {
    use crate::store::CkbadgerStore;
    use crate::types::{ParticipantDelta, ParticipantId, TxActions, TAG_IDENTITY};

    #[test]
    fn matches_activity_filter_accepts_prefix_participant() {
        let lock = [0xAAu8; 32];
        let mut prefix = [0u8; 20];
        prefix.copy_from_slice(&lock[..20]);
        let actions = TxActions {
            tx_hash: vec![0xA1; 32],
            block_hash: vec![0xBB; 32],
            block_number: 10,
            tx_index: 0,
            timestamp: 1_700_000_000,
            is_cellbase: false,
            protocol_actions: vec![],
            type_calls: vec![],
            lock_calls: vec![],
            participants: vec![ParticipantDelta {
                id: ParticipantId::LockPrefix(prefix),
                ckb_delta: 0,
                used_delta: 0,
                item_deltas: vec![],
                tags: TAG_IDENTITY,
                roles: 0,
            }],
        };
        assert!(CkbadgerStore::matches_activity_filter(&actions, &lock, Some("identity")).unwrap());
        assert!(!CkbadgerStore::matches_activity_filter(&actions, &lock, Some("token")).unwrap());
    }
}

/// `CF_TX_ACTIONS_BY_BUCKET`: the global activity bucket index.
#[cfg(test)]
mod global_bucket_tests {
    use super::*;
    use crate::batch::StoreBatch;
    use tempfile::TempDir;

    fn bucket_fixture(block_num: i64, tx_idx: i32, tags: u16) -> TxActions {
        let mut tx_hash = vec![0u8; 32];
        tx_hash[..8].copy_from_slice(&block_num.to_be_bytes());
        tx_hash[8..12].copy_from_slice(&tx_idx.to_be_bytes());
        TxActions {
            tx_hash,
            block_hash: vec![0xBB; 32],
            block_number: block_num,
            tx_index: tx_idx,
            timestamp: 1_700_000_000 + block_num,
            is_cellbase: false,
            protocol_actions: vec![],
            type_calls: vec![],
            lock_calls: vec![],
            participants: vec![ParticipantDelta {
                id: ParticipantId::Lock([0xAA; 32]),
                ckb_delta: 100,
                used_delta: 0,
                item_deltas: vec![],
                tags,
                roles: 0,
            }],
        }
    }

    fn bucket_rows(store: &CkbadgerStore) -> Vec<(GlobalBucket, i64, i32)> {
        store
            .iterator_cf(
                store.cf_tx_actions_by_bucket(),
                rocksdb::IteratorMode::Start,
            )
            .map(|item| {
                let (key, value) = item.unwrap();
                assert!(value.is_empty(), "bucket rows carry no value");
                keys::decode_tx_actions_by_bucket_key(&key).unwrap()
            })
            .collect()
    }

    /// The live writer puts a transaction through `StoreBatch::put_tx_actions`;
    /// that one call must also put its bucket row, in the same domain batch,
    /// and nothing for a cellbase.
    #[test]
    fn put_tx_actions_writes_its_bucket_row_in_the_same_domain_batch() {
        let dir = TempDir::new().unwrap();
        let domain = CkbadgerStore::open_domain(dir.path().join("domain")).unwrap();
        let append = CkbadgerStore::open_append_only(dir.path().join("append-only")).unwrap();

        let token = bucket_fixture(100, 2, TAG_TOKEN);
        let mut cellbase = bucket_fixture(100, 0, 0);
        cellbase.is_cellbase = true;
        let mut batch = StoreBatch::new(&domain);
        batch.put_tx_actions(&token);
        batch.put_tx_actions(&cellbase);
        assert_eq!(batch.len(), 3, "two TxActions rows + one bucket row");
        // `into_write_batch` refuses a batch carrying append-only operations.
        domain
            .write_batch(batch.into_write_batch().unwrap())
            .unwrap();

        assert_eq!(bucket_rows(&domain), vec![(GlobalBucket::Token, 100, 2)]);
        assert!(
            append
                .iterator_cf(append.cf_cells(), rocksdb::IteratorMode::Start)
                .next()
                .is_none(),
            "the bucket write path never touches the append-only store"
        );
    }

    #[test]
    fn tx_actions_by_bucket_is_a_domain_cf_never_append_only() {
        assert!(DOMAIN_CFS.contains(&CF_TX_ACTIONS_BY_BUCKET));
        assert!(!APPEND_CFS.contains(&CF_TX_ACTIONS_BY_BUCKET));
        assert!(!crate::store::is_append_only_cf_name(
            CF_TX_ACTIONS_BY_BUCKET
        ));
        let dir = TempDir::new().unwrap();
        let append = CkbadgerStore::open_append_only(dir.path()).unwrap();
        assert!(!append.has_cf(CF_TX_ACTIONS_BY_BUCKET));
    }

    #[test]
    fn list_tx_actions_by_bucket_pages_one_bucket_newest_first() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        let mut batch = StoreBatch::new(&store);
        for (block, tx, tags) in [
            (300, 1, TAG_OBJECT),
            (300, 4, TAG_TOKEN),
            (250, 2, TAG_OBJECT),
            (250, 9, TAG_OBJECT),
            (100, 1, 0),
            (90, 3, TAG_OBJECT),
            (80, 1, TAG_DAO),
        ] {
            super::tests::seed_activity(&mut batch, &bucket_fixture(block, tx, tags));
        }
        batch.commit().unwrap();

        let all = store
            .list_tx_actions_by_bucket(GlobalBucket::Object, None, 10)
            .unwrap();
        assert_eq!(all, vec![(300, 1), (250, 9), (250, 2), (90, 3)]);

        let page1 = store
            .list_tx_actions_by_bucket(GlobalBucket::Object, None, 2)
            .unwrap();
        assert_eq!(page1, vec![(300, 1), (250, 9)]);
        let page2 = store
            .list_tx_actions_by_bucket(GlobalBucket::Object, page1.last().copied(), 2)
            .unwrap();
        assert_eq!(page2, vec![(250, 2), (90, 3)], "strictly after the cursor");
        let page3 = store
            .list_tx_actions_by_bucket(GlobalBucket::Object, page2.last().copied(), 2)
            .unwrap();
        assert!(
            page3.is_empty(),
            "never runs into the next bucket: {page3:?}"
        );

        // A cursor that is not itself a row still pages from below it.
        assert_eq!(
            store
                .list_tx_actions_by_bucket(GlobalBucket::Object, Some((260, 0)), 10)
                .unwrap(),
            vec![(250, 9), (250, 2), (90, 3)]
        );
        assert_eq!(
            store
                .list_tx_actions_by_bucket(GlobalBucket::Ckb, None, 10)
                .unwrap(),
            vec![(100, 1)]
        );
        assert_eq!(
            store
                .list_tx_actions_by_bucket(GlobalBucket::Dao, None, 10)
                .unwrap(),
            vec![(80, 1)],
            "the last bucket row before an empty bucket"
        );

        let loaded = store
            .list_tx_actions_page_by_bucket(GlobalBucket::Object, Some((300, 1)), 2)
            .unwrap();
        assert_eq!(
            loaded
                .iter()
                .map(|a| (a.block_number, a.tx_index))
                .collect::<Vec<_>>(),
            vec![(250, 9), (250, 2)]
        );
    }

    /// The key is the position, so a position holds one `activities` row
    /// (a second write replaces the first; the journal rollback leaves no
    /// ghost row beside it), completed from `tx_index` and the block header.
    #[test]
    fn get_tx_actions_at_reads_the_one_row_of_a_position() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        assert!(store.get_tx_actions_at(5, 1).unwrap().is_none());

        let mut batch = StoreBatch::new(&store);
        super::tests::seed_activity(&mut batch, &bucket_fixture(5, 1, 0));
        super::tests::seed_activity(&mut batch, &bucket_fixture(5, 2, 0));
        batch.commit().unwrap();
        let got = store.get_tx_actions_at(5, 1).unwrap().expect("row at 5:1");
        assert_eq!((got.block_number, got.tx_index), (5, 1));
        assert_eq!(got.tx_hash, bucket_fixture(5, 1, 0).tx_hash);

        let mut replaced = bucket_fixture(5, 1, TAG_TOKEN);
        replaced.tx_hash = vec![0xEE; 32];
        let mut batch = StoreBatch::new(&store);
        super::tests::seed_activity(&mut batch, &replaced);
        batch.commit().unwrap();
        let got = store.get_tx_actions_at(5, 1).unwrap().expect("row at 5:1");
        assert_eq!(got.tx_hash, vec![0xEE; 32]);
        assert_eq!(got.participants[0].tags, TAG_TOKEN);
        assert_eq!(
            store.list_tx_actions_recent(10, None).unwrap().len(),
            2,
            "one row per position"
        );
    }

    #[test]
    fn get_tx_actions_at_fails_on_a_position_without_a_transaction() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        let mut batch = StoreBatch::new(&store);
        batch.put_tx_actions(&bucket_fixture(5, 1, 0));
        batch.commit().unwrap();
        let err = store.get_tx_actions_at(5, 1).unwrap_err().to_string();
        assert!(
            err.contains("activities points at block=5, tx_idx=1, which has no tx_index row"),
            "{err}"
        );
    }

    /// Review P1-1: a page of resolved transactions reads its `activities`
    /// rows with one `multi_get` and nothing else; the rows are completed
    /// from the reader's entries and block hashes; a cellbase reads as `None`.
    #[test]
    fn tx_actions_of_a_resolved_page_is_one_multi_get() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        let mut batch = StoreBatch::new(&store);
        super::tests::seed_activity(&mut batch, &bucket_fixture(5, 1, 0));
        super::tests::seed_activity(&mut batch, &bucket_fixture(6, 2, TAG_TOKEN));
        batch.commit().unwrap();
        let entry = |actions: &TxActions| TxIndexEntry {
            tx_hash: actions.tx_hash.as_slice().try_into().unwrap(),
            is_cellbase: false,
            timestamp: actions.timestamp,
            inputs_count: 1,
            outputs_count: 1,
            fee: 0,
            tx_size: 0,
            cycles: None,
            semantic_tags: 0,
        };
        let (first, second) = (bucket_fixture(5, 1, 0), bucket_fixture(6, 2, TAG_TOKEN));
        let (first_entry, second_entry) = (entry(&first), entry(&second));
        let cellbase = TxIndexEntry {
            is_cellbase: true,
            ..entry(&first)
        };
        let page = [
            ResolvedTx {
                block_number: 6,
                tx_index: 2,
                entry: &second_entry,
                block_hash: &[0x66; 32],
            },
            ResolvedTx {
                block_number: 6,
                tx_index: 0,
                entry: &cellbase,
                block_hash: &[0x66; 32],
            },
            ResolvedTx {
                block_number: 5,
                tx_index: 1,
                entry: &first_entry,
                block_hash: &[0x55; 32],
            },
        ];
        store.reset_read_call_counters();
        let actions = store.tx_actions_of(&page).unwrap();
        assert_eq!(store.read_call_counts(), (0, 1), "(get_cf, multi_get_cf)");
        let second_actions = actions[0].as_ref().expect("row at 6:2");
        assert_eq!(
            (second_actions.block_number, second_actions.tx_index),
            (6, 2)
        );
        assert_eq!(second_actions.tx_hash, second.tx_hash);
        assert_eq!(second_actions.block_hash, vec![0x66; 32]);
        assert_eq!(second_actions.participants[0].tags, TAG_TOKEN);
        assert!(actions[1].is_none(), "a cellbase has no activities row");
        let first_actions = actions[2].as_ref().expect("row at 5:1");
        assert_eq!(first_actions.tx_hash, first.tx_hash);
        assert_eq!(first_actions.block_hash, vec![0x55; 32]);

        store.reset_read_call_counters();
        assert!(store.tx_actions_of(&[]).unwrap().is_empty());
        assert_eq!(
            store.read_call_counts(),
            (0, 0),
            "an empty page reads nothing"
        );
    }

    #[test]
    fn tx_actions_of_fails_on_a_non_cellbase_transaction_without_its_row() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        let entry = TxIndexEntry {
            tx_hash: [0x77; 32],
            is_cellbase: false,
            timestamp: 0,
            inputs_count: 1,
            outputs_count: 1,
            fee: 0,
            tx_size: 0,
            cycles: None,
            semantic_tags: 0,
        };
        let err = store
            .tx_actions_of(&[ResolvedTx {
                block_number: 9,
                tx_index: 3,
                entry: &entry,
                block_hash: &[0x99; 32],
            }])
            .unwrap_err()
            .to_string();
        assert!(
            err.contains(&format!(
                "activities has no row for non-cellbase transaction 0x{} at block=9, tx_idx=3",
                "77".repeat(32)
            )),
            "{err}"
        );
    }

    /// Delta review N3: the stored bucket page — what the DAO 24 h counts
    /// read — checks that each row classifies into its bucket, as the
    /// completed page did.
    #[test]
    fn stored_bucket_page_fails_on_a_row_of_another_bucket() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        let mut batch = StoreBatch::new(&store);
        // A token transaction whose bucket row says DAO.
        super::tests::seed_activity(&mut batch, &bucket_fixture(77, 3, TAG_TOKEN));
        batch.commit().unwrap();
        store
            .put_cf(
                store.cf_tx_actions_by_bucket(),
                &keys::encode_tx_actions_by_bucket_key(GlobalBucket::Dao, 77, 3),
                &[],
            )
            .unwrap();
        for result in [
            store
                .list_stored_tx_actions_page_by_bucket(GlobalBucket::Dao, None, 10)
                .map(|_| ()),
            store
                .list_tx_actions_page_by_bucket(GlobalBucket::Dao, None, 10)
                .map(|_| ()),
        ] {
            let err = result.unwrap_err().to_string();
            assert!(
                err.contains(
                    "tx_actions_by_bucket row disagrees with its tx actions: bucket=dao, \
                     classified=token, block_num=77, tx_idx=3"
                ),
                "{err}"
            );
        }
    }

    #[test]
    fn bucket_page_fails_with_position_on_a_row_without_tx_actions() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        store
            .put_cf(
                store.cf_tx_actions_by_bucket(),
                &keys::encode_tx_actions_by_bucket_key(GlobalBucket::Token, 77, 3),
                &[],
            )
            .unwrap();
        let err = store
            .list_tx_actions_page_by_bucket(GlobalBucket::Token, None, 10)
            .unwrap_err()
            .to_string();
        assert!(
            err.contains(
                "tx_actions_by_bucket row has no tx actions: bucket=token, block_num=77, tx_idx=3"
            ),
            "{err}"
        );
    }

    #[test]
    fn bucket_page_fails_when_the_row_disagrees_with_its_tx_actions() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        let mut batch = StoreBatch::new(&store);
        super::tests::seed_activity(&mut batch, &bucket_fixture(12, 0, TAG_TOKEN));
        batch.commit().unwrap();
        store
            .put_cf(
                store.cf_tx_actions_by_bucket(),
                &keys::encode_tx_actions_by_bucket_key(GlobalBucket::Object, 12, 0),
                &[],
            )
            .unwrap();
        let err = store
            .list_tx_actions_page_by_bucket(GlobalBucket::Object, None, 10)
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("bucket=object, classified=token, block_num=12, tx_idx=0"),
            "{err}"
        );
    }
}
