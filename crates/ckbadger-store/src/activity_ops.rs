//! Activity query operations.

use std::collections::HashMap;

use crate::keys;
use crate::store::*;
use crate::types::*;

use crate::bytes_to_hex;

impl CkbadgerStore {
    /// Reconstruct TxActions from a CF_TX_ACTIONS key-value pair.
    /// Uses block_header_cache to avoid redundant CF_BLOCK_HEADERS reads.
    fn reconstruct_tx_actions(
        &self,
        key: &[u8],
        value: &[u8],
        header_cache: &mut HashMap<i64, (Vec<u8>, i64)>,
    ) -> anyhow::Result<TxActions> {
        let (block_number, tx_index, tx_hash) = keys::decode_tx_actions_key(key);
        let compact: TxActionsCompact = postcard::from_bytes(value)?;
        let (block_hash, timestamp) = match header_cache.entry(block_number) {
            std::collections::hash_map::Entry::Occupied(e) => e.get().clone(),
            std::collections::hash_map::Entry::Vacant(e) => {
                let header = self.get_block_header(block_number)?.ok_or_else(|| {
                    anyhow::anyhow!(
                        "missing block header for CF_TX_ACTIONS reconstruction: block={}",
                        block_number
                    )
                })?;
                let pair = (header.hash.clone(), header.timestamp);
                e.insert(pair.clone());
                pair
            }
        };
        Ok(TxActions::from_compact(
            compact,
            block_number,
            tx_index,
            tx_hash,
            block_hash,
            timestamp,
        ))
    }

    pub fn get_tx_actions(
        &self,
        block_num: i64,
        tx_idx: i32,
        tx_hash: &[u8],
    ) -> anyhow::Result<Option<TxActions>> {
        let key = keys::encode_tx_actions_key(block_num, tx_idx, tx_hash);
        match self.get_cf(self.cf_tx_actions(), &key)? {
            Some(value) => {
                let mut header_cache = HashMap::new();
                let actions = self.reconstruct_tx_actions(&key, &value, &mut header_cache)?;
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
        let mut header_cache = HashMap::new();
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

            let actions = self.reconstruct_tx_actions(&key, &value, &mut header_cache)?;
            results.push(actions);
            if results.len() >= limit {
                break;
            }
        }

        Ok(results)
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
                // is_cellbase is always false in compact form, but keep the
                // guard for safety in case the invariant changes.
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
    /// An optional `filter` narrows results: "ckb", "token", "nft"/"object",
    /// "identity", "dao", "type_call", "lock_call", "protocol:X".
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
        let mut header_cache = HashMap::new();

        loop {
            let rows = self.list_addr_txs_recent(lock_hash, scan_limit, scan_cursor)?;
            if rows.is_empty() {
                break;
            }

            let action_keys: Vec<Vec<u8>> = rows
                .iter()
                .map(|(block_num, tx_idx, tx_hash)| {
                    keys::encode_tx_actions_key(*block_num, *tx_idx, tx_hash)
                })
                .collect();
            let action_refs: Vec<(&rocksdb::ColumnFamily, &[u8])> = action_keys
                .iter()
                .map(|key| (activity_cf, key.as_slice()))
                .collect();
            let action_values = self.multi_get_cf(action_refs);

            let mut last_seen = None;
            for (((block_num, tx_idx, tx_hash), value_result), action_key) in
                rows.iter().zip(action_values).zip(action_keys.iter())
            {
                last_seen = Some((*block_num, *tx_idx));
                let value = match value_result {
                    Ok(Some(value)) => value,
                    Ok(None) => {
                        anyhow::bail!(
                            "missing tx actions for addr_txs entry: lock_hash=0x{}, block_num={}, tx_idx={}, tx_hash=0x{}",
                            bytes_to_hex(lock_hash),
                            block_num,
                            tx_idx,
                            bytes_to_hex(tx_hash)
                        );
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
                let actions = self
                    .reconstruct_tx_actions(action_key, &value, &mut header_cache)
                    .map_err(|e| {
                        anyhow::anyhow!(
                            "failed to reconstruct tx actions in list_activities: lock_hash=0x{}, block_num={}, tx_idx={}, tx_hash=0x{}, error={}",
                            bytes_to_hex(lock_hash),
                            block_num,
                            tx_idx,
                            bytes_to_hex(tx_hash),
                            e
                        )
                    })?;
                if Self::matches_activity_filter(&actions, lock_hash, filter) {
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
    /// Finds the participant matching `lock_hash`, then checks the `tags` bitmask.
    pub fn matches_activity_filter(
        actions: &TxActions,
        lock_hash: &[u8],
        filter: Option<&str>,
    ) -> bool {
        match filter {
            None | Some("all") => true,
            Some(f) => {
                // Find the participant matching this lock_hash
                let participant = actions
                    .participants
                    .iter()
                    .find(|p| p.lock_hash == lock_hash);
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
                    "object" | "nft" => tags & TAG_OBJECT != 0,
                    "identity" => tags & TAG_IDENTITY != 0,
                    "dao" => tags & TAG_DAO != 0,
                    "type_call" => tags & TAG_TYPE_CALL != 0,
                    "lock_call" => tags & TAG_LOCK_CALL != 0,
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

    fn make_block_header(block_num: i64) -> CachedBlockHeader {
        CachedBlockHeader {
            hash: vec![0xBB; 32],
            timestamp: 1_700_000_000 + block_num,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
            dao: vec![0; 32],
            transactions_count: 1,
        }
    }

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
                lock_hash: vec![0xAA; 32],
                ckb_delta: 100,
                used_delta: 0,
                item_deltas: vec![],
                tags: 0,
            }],
        }
    }

    #[test]
    fn test_put_and_get_tx_actions_roundtrip() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();

        let actions = make_tx_actions(100, 0, 0xAA);
        let mut batch = StoreBatch::new(&store);
        batch.put_block_header(100, &make_block_header(100));
        batch.put_tx_actions(&actions);
        batch.commit().unwrap();

        let got = store
            .get_tx_actions(100, 0, &[0xAA; 32])
            .unwrap()
            .expect("should find tx actions");
        assert_eq!(got.block_number, 100);
        assert_eq!(got.tx_index, 0);
        assert_eq!(got.tx_hash, vec![0xAA; 32]);
        assert_eq!(got.block_hash, vec![0xBB; 32]);
        assert_eq!(got.timestamp, 1_700_000_100);
        assert!(!got.is_cellbase);
        assert_eq!(got.participants.len(), 1);
        assert_eq!(got.participants[0].ckb_delta, 100);
    }

    #[test]
    fn test_list_tx_actions_recent() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();

        let mut batch = StoreBatch::new(&store);
        batch.put_block_header(100, &make_block_header(100));
        batch.put_block_header(101, &make_block_header(101));
        // Insert 3 txs in ascending order (keys sort ascending by block_num desc + tx_idx desc)
        batch.put_tx_actions(&make_tx_actions(100, 0, 0x01));
        batch.put_tx_actions(&make_tx_actions(100, 1, 0x02));
        batch.put_tx_actions(&make_tx_actions(101, 0, 0x03));
        batch.commit().unwrap();

        let results = store.list_tx_actions_recent(10, None).unwrap();
        assert_eq!(results.len(), 3);
    }

    #[test]
    fn test_list_tx_actions_recent_with_limit() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();

        let mut batch = StoreBatch::new(&store);
        batch.put_block_header(100, &make_block_header(100));
        batch.put_block_header(101, &make_block_header(101));
        batch.put_tx_actions(&make_tx_actions(100, 0, 0x01));
        batch.put_tx_actions(&make_tx_actions(100, 1, 0x02));
        batch.put_tx_actions(&make_tx_actions(101, 0, 0x03));
        batch.commit().unwrap();

        let results = store.list_tx_actions_recent(2, None).unwrap();
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_compact_roundtrip_reconstructs_all_fields() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();

        let actions = TxActions {
            tx_hash: vec![0xDD; 32],
            block_hash: vec![0xBB; 32],
            block_number: 500,
            tx_index: 2,
            timestamp: 1_700_000_500,
            is_cellbase: false,
            protocol_actions: vec![ProtocolAction::new(
                "rgbpp",
                "transfer",
                serde_json::json!({"amount": 100}),
            )],
            type_calls: vec![TypeCallEntry {
                type_code_hash: vec![0x11; 32],
                type_hash_type: 1,
                type_args: vec![0x22; 20],
            }],
            lock_calls: vec![LockCallEntry {
                lock_code_hash: vec![0x33; 32],
                lock_hash_type: 0,
                lock_args: vec![0x44; 20],
            }],
            participants: vec![ParticipantDelta {
                lock_hash: vec![0xAA; 32],
                ckb_delta: -500,
                used_delta: 100,
                item_deltas: vec![ItemDelta {
                    item_id: vec![0x55; 32],
                    kind: ITEM_KIND_TOKEN,
                    delta: 1000,
                }],
                tags: TAG_TOKEN | TAG_PROTOCOL,
            }],
        };

        let mut batch = StoreBatch::new(&store);
        batch.put_block_header(500, &make_block_header(500));
        batch.put_tx_actions(&actions);
        batch.commit().unwrap();

        let got = store
            .get_tx_actions(500, 2, &[0xDD; 32])
            .unwrap()
            .expect("should find tx actions");

        // Fields from key
        assert_eq!(got.block_number, 500);
        assert_eq!(got.tx_index, 2);
        assert_eq!(got.tx_hash, vec![0xDD; 32]);

        // Fields from block header
        assert_eq!(got.block_hash, vec![0xBB; 32]);
        assert_eq!(got.timestamp, 1_700_000_500);

        // Always false for compact
        assert!(!got.is_cellbase);

        // Preserved fields
        assert_eq!(got.protocol_actions.len(), 1);
        assert_eq!(got.protocol_actions[0].protocol, "rgbpp");
        assert_eq!(got.type_calls.len(), 1);
        assert_eq!(got.type_calls[0].type_code_hash, vec![0x11; 32]);
        assert_eq!(got.lock_calls.len(), 1);
        assert_eq!(got.lock_calls[0].lock_code_hash, vec![0x33; 32]);
        assert_eq!(got.participants.len(), 1);
        assert_eq!(got.participants[0].ckb_delta, -500);
        assert_eq!(got.participants[0].item_deltas.len(), 1);
        assert_eq!(got.participants[0].tags, TAG_TOKEN | TAG_PROTOCOL);
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
    fn test_list_activities_rejects_non_32_byte_lock_hash() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        let err = store
            .list_activities(&[0xAA; 16], 10, None, None)
            .unwrap_err();
        assert!(err.to_string().contains("32-byte lock_hash"));
    }
}
