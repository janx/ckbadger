//! Activity query operations.

use crate::keys;
use crate::store::*;
use crate::types::*;

use crate::bytes_to_hex;

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
            let actions: TxActions = bincode::deserialize(&value)?;
            validate_tx_actions_identity(&actions, block_num, tx_idx, &tx_hash_from_key)?;

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
            for ((block_num, tx_idx, tx_hash), value_result) in rows.iter().zip(action_values) {
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

// TODO: Task 8 — update tests for TxActions
#[cfg(test)]
mod tests {}
