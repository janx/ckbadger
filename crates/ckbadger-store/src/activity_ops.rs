//! Activity query operations.

use crate::global_bucket::{classify_global_bucket, GlobalBucket};
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

fn validate_tx_actions_identity(
    actions: &TxActions,
    block_num: i64,
    tx_idx: i32,
    tx_hash_from_key: &[u8],
) -> anyhow::Result<()> {
    if actions.block_number != block_num || actions.tx_index != tx_idx {
        anyhow::bail!(
            "tx actions key/value location mismatch: key_block_num={}, value_block_num={}, key_tx_idx={}, value_tx_idx={}",
            block_num,
            actions.block_number,
            tx_idx,
            actions.tx_index
        );
    }
    if actions.tx_hash != tx_hash_from_key {
        anyhow::bail!(
            "tx actions key/value tx_hash mismatch: block_num={}, tx_idx={}, key_tx_hash=0x{}, value_tx_hash=0x{}",
            block_num,
            tx_idx,
            bytes_to_hex(tx_hash_from_key),
            bytes_to_hex(&actions.tx_hash)
        );
    }
    Ok(())
}

impl CkbadgerStore {
    pub fn get_tx_actions(
        &self,
        block_num: i64,
        tx_idx: i32,
        tx_hash: &[u8],
    ) -> anyhow::Result<Option<TxActions>> {
        let key = keys::encode_tx_actions_key(block_num, tx_idx, tx_hash);
        match self.get_cf(self.cf_tx_actions(), &key)? {
            Some(value) => {
                let actions: TxActions = bincode::deserialize(&value)?;
                validate_tx_actions_identity(&actions, block_num, tx_idx, tx_hash)?;
                Ok(Some(actions))
            }
            None => Ok(None),
        }
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
    pub fn list_tx_actions_recent_in_range(
        &self,
        limit: usize,
        cursor: Option<(i64, i32)>,
        after_block: Option<i64>,
    ) -> anyhow::Result<Vec<TxActions>> {
        if limit == 0 {
            return Ok(Vec::new());
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

        let mut results = Vec::new();
        for item in iter {
            let (key, value) = item.map_err(|e| {
                anyhow::anyhow!(
                    "failed to iterate tx actions in list_tx_actions_recent: {}",
                    e
                )
            })?;
            if key.len() != keys::TX_ACTIONS_KEY_SIZE {
                continue;
            }

            let (block_num, tx_idx, tx_hash_from_key) = keys::decode_tx_actions_key(&key);
            if after_block.is_some_and(|number| block_num <= number) {
                break;
            }

            let actions: TxActions = bincode::deserialize(&value)?;
            validate_tx_actions_identity(&actions, block_num, tx_idx, &tx_hash_from_key)?;

            results.push(actions);
            if results.len() >= limit {
                break;
            }
        }

        Ok(results)
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

    /// The `TxActions` row at one transaction position, found by its 12-byte
    /// position prefix. Two rows at one position is a broken rollback, never a
    /// choice to make here.
    pub fn get_tx_actions_at(
        &self,
        block_num: i64,
        tx_idx: i32,
    ) -> anyhow::Result<Option<TxActions>> {
        let prefix = keys::encode_tx_actions_position_prefix(block_num, tx_idx);
        let mut iter = self.prefix_iterator_cf(self.cf_tx_actions(), &prefix);
        let Some(first) = iter.next() else {
            return Ok(None);
        };
        let (key, value) = first.map_err(|e| {
            anyhow::anyhow!(
                "failed to read tx actions at block_num={}, tx_idx={}: {}",
                block_num,
                tx_idx,
                e
            )
        })?;
        if key.len() != keys::TX_ACTIONS_KEY_SIZE {
            anyhow::bail!(
                "tx actions key at block_num={}, tx_idx={} has {} bytes, expected {}",
                block_num,
                tx_idx,
                key.len(),
                keys::TX_ACTIONS_KEY_SIZE
            );
        }
        if let Some(second) = iter.next() {
            let (second_key, _) = second.map_err(|e| {
                anyhow::anyhow!(
                    "failed to read tx actions at block_num={}, tx_idx={}: {}",
                    block_num,
                    tx_idx,
                    e
                )
            })?;
            anyhow::bail!(
                "two tx actions rows at one position: block_num={}, tx_idx={}, keys=0x{} and 0x{}",
                block_num,
                tx_idx,
                bytes_to_hex(&key),
                bytes_to_hex(&second_key)
            );
        }
        let (_, _, tx_hash_from_key) = keys::decode_tx_actions_key(&key);
        let actions: TxActions = bincode::deserialize(&value).map_err(|e| {
            anyhow::anyhow!(
                "failed to deserialize tx actions at block_num={}, tx_idx={}, tx_hash=0x{}: {}",
                block_num,
                tx_idx,
                bytes_to_hex(&tx_hash_from_key),
                e
            )
        })?;
        validate_tx_actions_identity(&actions, block_num, tx_idx, &tx_hash_from_key)?;
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
        self.list_tx_actions_by_bucket(bucket, cursor, limit)?
            .into_iter()
            .map(|(block_num, tx_idx)| {
                let actions = self.get_tx_actions_at(block_num, tx_idx)?.ok_or_else(|| {
                    anyhow::anyhow!(
                        "tx_actions_by_bucket row has no tx actions: bucket={}, block_num={}, tx_idx={}",
                        bucket.as_filter(),
                        block_num,
                        tx_idx
                    )
                })?;
                let classified = classify_global_bucket(&actions);
                if classified != Some(bucket) {
                    anyhow::bail!(
                        "tx_actions_by_bucket row disagrees with its tx actions: bucket={}, classified={:?}, block_num={}, tx_idx={}, tx_hash=0x{}",
                        bucket.as_filter(),
                        classified.map(GlobalBucket::as_filter),
                        block_num,
                        tx_idx,
                        bytes_to_hex(&actions.tx_hash)
                    );
                }
                Ok(actions)
            })
            .collect()
    }

    /// Return up to 64 most recent non-cellbase TxActions.
    pub fn get_latest_activities(&self) -> anyhow::Result<Vec<TxActions>> {
        const LATEST_ACTIVITY_LIMIT: usize = 64;

        let mut results = Vec::with_capacity(LATEST_ACTIVITY_LIMIT);
        let mut cursor = None;

        loop {
            let actions_list = self.list_tx_actions_recent(LATEST_ACTIVITY_LIMIT, cursor)?;
            if actions_list.is_empty() {
                break;
            }

            let batch_len = actions_list.len();
            let mut last_seen = None;
            for actions in actions_list {
                last_seen = Some((actions.block_number, actions.tx_index));
                if actions.is_cellbase {
                    continue;
                }
                results.push(actions);
                if results.len() >= LATEST_ACTIVITY_LIMIT {
                    return Ok(results);
                }
            }

            if batch_len < LATEST_ACTIVITY_LIMIT {
                break;
            }
            let Some(next_cursor) = last_seen else {
                break;
            };
            cursor = Some(next_cursor);
        }

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
        let activity_cf = self.cf_tx_actions();

        loop {
            let rows = self.list_addr_txs_recent(lock_hash, scan_limit, scan_cursor)?;
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
            let surviving: Vec<&(i64, i32, Vec<u8>, AddrTxValue)> = rows
                .iter()
                .filter(|(_, _, _, addr_tx_value)| tags_pass_filter(addr_tx_value.tags, filter))
                .collect();

            if !surviving.is_empty() {
                let action_keys: Vec<Vec<u8>> = surviving
                    .iter()
                    .map(|(block_num, tx_idx, tx_hash, _)| {
                        keys::encode_tx_actions_key(*block_num, *tx_idx, tx_hash)
                    })
                    .collect();
                let action_refs: Vec<(&rocksdb::ColumnFamily, &[u8])> = action_keys
                    .iter()
                    .map(|key| (activity_cf, key.as_slice()))
                    .collect();
                let action_values = self.multi_get_cf(action_refs);

                for ((block_num, tx_idx, tx_hash, _addr_tx_value), value_result) in
                    surviving.iter().zip(action_values)
                {
                    let value = match value_result {
                        Ok(Some(value)) => value,
                        Ok(None) => {
                            // Cellbase txs are written to CF_ADDR_TXS but intentionally
                            // excluded from CF_TX_ACTIONS (see batch.rs/bulk_build). Skip.
                            continue;
                        }
                        Err(e) => {
                            anyhow::bail!(
                                "rocksdb multi_get failed in list_activities: lock_hash=0x{}, block_num={}, tx_idx={}, tx_hash=0x{}, error={}",
                                bytes_to_hex(lock_hash),
                                block_num,
                                tx_idx,
                                bytes_to_hex(tx_hash),
                                e
                            );
                        }
                    };
                    let actions: TxActions = bincode::deserialize(&value).map_err(|e| {
                        anyhow::anyhow!(
                            "failed to deserialize tx actions in list_activities: lock_hash=0x{}, block_num={}, tx_idx={}, tx_hash=0x{}, error={}",
                            bytes_to_hex(lock_hash),
                            block_num,
                            tx_idx,
                            bytes_to_hex(tx_hash),
                            e
                        )
                    })?;
                    validate_tx_actions_identity(&actions, *block_num, *tx_idx, tx_hash)?;
                    if Self::matches_activity_filter(&actions, lock_hash, filter) {
                        results.push(actions);
                        if results.len() >= limit {
                            return Ok(results);
                        }
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
    /// Finds the participant matching `lock_hash`, then checks the `tags` bitmask.
    pub fn matches_activity_filter(
        actions: &TxActions,
        lock_hash: &[u8],
        filter: Option<&str>,
    ) -> bool {
        match filter {
            None | Some("all") => true,
            Some(f) => {
                // Find the participant matching this lock_hash. One matcher for
                // both participant identities: a full lock hash compares whole,
                // a protocol-named 20-byte prefix compares its first 20 bytes.
                let Ok(lock) = <&[u8; 32]>::try_from(lock_hash) else {
                    return false;
                };
                let participant = actions.participants.iter().find(|p| p.id.matches(lock));
                let Some(p) = participant else {
                    // No matching participant — should not happen if data is consistent,
                    // but don't match any filter if participant is missing.
                    return false;
                };
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
                        if tags & TAG_PROTOCOL == 0 {
                            return false;
                        }
                        let protocol_name = &proto["protocol:".len()..];
                        actions
                            .protocol_actions
                            .iter()
                            .any(|a| a.protocol == protocol_name)
                    }
                    _ => false,
                }
            }
        }
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

    #[test]
    fn test_put_and_get_tx_actions_roundtrip() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();

        let actions = make_tx_actions(100, 0, 0xAA);
        let mut batch = StoreBatch::new(&store);
        batch.put_tx_actions(&actions);
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
        batch.put_tx_actions(&make_tx_actions(100, 0, 0x01));
        batch.put_tx_actions(&make_tx_actions(100, 1, 0x02));
        batch.put_tx_actions(&make_tx_actions(101, 0, 0x03));
        batch.commit().unwrap();

        let results = store.list_tx_actions_recent(10, None).unwrap();
        assert_eq!(results.len(), 3);
    }

    #[test]
    fn recent_range_stops_before_deserializing_older_blocks() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        let mut batch = StoreBatch::new(&store);
        batch.put_tx_actions(&make_tx_actions(100, 0, 1));
        batch.put_tx_actions(&make_tx_actions(101, 0, 2));
        batch.put_tx_actions(&make_tx_actions(101, 1, 3));
        batch.commit().unwrap();
        let old_key = keys::encode_tx_actions_key(100, 0, &[1; 32]);
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
        batch.put_tx_actions(&make_tx_actions(100, 0, 0x01));
        batch.put_tx_actions(&make_tx_actions(100, 1, 0x02));
        batch.put_tx_actions(&make_tx_actions(101, 0, 0x03));
        batch.commit().unwrap();

        let results = store.list_tx_actions_recent(2, None).unwrap();
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_matches_activity_filter_all() {
        let actions = make_tx_actions(100, 0, 0xAA);
        let lock_hash = vec![0xAA; 32];
        assert!(CkbadgerStore::matches_activity_filter(
            &actions, &lock_hash, None
        ));
        assert!(CkbadgerStore::matches_activity_filter(
            &actions,
            &lock_hash,
            Some("all")
        ));
    }

    #[test]
    fn test_matches_activity_filter_ckb() {
        let actions = make_tx_actions(100, 0, 0xAA);
        let lock_hash = vec![0xAA; 32];
        // tags=0 means pure CKB
        assert!(CkbadgerStore::matches_activity_filter(
            &actions,
            &lock_hash,
            Some("ckb")
        ));
    }

    #[test]
    fn test_matches_activity_filter_token() {
        let mut actions = make_tx_actions(100, 0, 0xAA);
        actions.participants[0].tags = TAG_TOKEN;
        let lock_hash = vec![0xAA; 32];
        assert!(CkbadgerStore::matches_activity_filter(
            &actions,
            &lock_hash,
            Some("token")
        ));
        assert!(!CkbadgerStore::matches_activity_filter(
            &actions,
            &lock_hash,
            Some("ckb")
        ));
    }

    #[test]
    fn test_matches_activity_filter_missing_participant() {
        let actions = make_tx_actions(100, 0, 0xAA);
        let unknown_lock = vec![0xFF; 32];
        // No matching participant -> false for any filter
        assert!(!CkbadgerStore::matches_activity_filter(
            &actions,
            &unknown_lock,
            Some("token")
        ));
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
                CkbadgerStore::matches_activity_filter(&actions, &lock, Some("script")),
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
        ));
        assert!(!CkbadgerStore::matches_activity_filter(
            &actions,
            &lock_hash,
            Some("protocol:fiber")
        ));
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
        let non_matching_tx_hash = [0x02; 32];

        let mut batch = StoreBatch::new(&store);
        // Matching entry: AddrTxValue.tags has TAG_TOKEN. Real TxActions also written.
        batch.put_addr_tx(
            &lock,
            100,
            0,
            &matching_tx_hash,
            &AddrTxValue::new(50, false, true, TAG_TOKEN),
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
        batch.put_tx_actions(&matching_actions);

        // Non-matching entry: AddrTxValue.tags = 0 (no TAG_TOKEN). CF_TX_ACTIONS
        // intentionally corrupt — list_activities(filter=token) must NOT read it.
        batch.put_addr_tx(
            &lock,
            99,
            0,
            &non_matching_tx_hash,
            &AddrTxValue::new(10, false, true, 0),
        );
        batch.commit().unwrap();

        let corrupt_key = keys::encode_tx_actions_key(99, 0, &non_matching_tx_hash);
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
        assert!(CkbadgerStore::matches_activity_filter(
            &actions,
            &lock,
            Some("identity")
        ));
        assert!(!CkbadgerStore::matches_activity_filter(
            &actions,
            &lock,
            Some("token")
        ));
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
        domain.write_batch(batch.into_write_batch()).unwrap();

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
            batch.put_tx_actions(&bucket_fixture(block, tx, tags));
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

    #[test]
    fn get_tx_actions_at_finds_one_row_and_rejects_two() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        assert!(store.get_tx_actions_at(5, 1).unwrap().is_none());

        let mut batch = StoreBatch::new(&store);
        batch.put_tx_actions(&bucket_fixture(5, 1, 0));
        batch.put_tx_actions(&bucket_fixture(5, 2, 0));
        batch.commit().unwrap();
        let got = store.get_tx_actions_at(5, 1).unwrap().expect("row at 5:1");
        assert_eq!((got.block_number, got.tx_index), (5, 1));

        // A second transaction hash at the same position: what a rollback that
        // forgot a row would leave behind.
        let mut stale = bucket_fixture(5, 1, 0);
        stale.tx_hash = vec![0xEE; 32];
        let mut batch = StoreBatch::new(&store);
        batch.put_tx_actions(&stale);
        batch.commit().unwrap();
        let err = store.get_tx_actions_at(5, 1).unwrap_err().to_string();
        assert!(
            err.contains("two tx actions rows at one position: block_num=5, tx_idx=1"),
            "{err}"
        );
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
        batch.put_tx_actions(&bucket_fixture(12, 0, TAG_TOKEN));
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
            err.contains("bucket=object, classified=Some(\"token\"), block_num=12, tx_idx=0"),
            "{err}"
        );
    }
}
