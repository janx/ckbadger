//! Activity query operations.

use crate::keys;
use crate::store::*;
use crate::types::*;

fn bytes_to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

impl CkbadgerStore {
    /// List activities for an address (lock_hash), newest first.
    ///
    /// Optionally start after the given `(block_num, tx_idx)` cursor.
    /// An optional `filter` narrows results: "ckb", "token", "nft", "dao".
    /// Returns `(block_num, tx_idx, entry)` tuples for cursor construction.
    pub fn list_activities(
        &self,
        lock_hash: &[u8],
        limit: usize,
        cursor: Option<(i64, i32)>,
        filter: Option<&str>,
    ) -> anyhow::Result<Vec<(i64, i32, ActivityEntry)>> {
        if lock_hash.len() != 32 {
            anyhow::bail!(
                "list_activities expects 32-byte lock_hash, got {} bytes",
                lock_hash.len()
            );
        }
        if limit == 0 {
            return Ok(Vec::new());
        }

        let prefix = &lock_hash[..32];
        let zero_tx_hash = [0u8; 32];

        // For cursor: seek to the cursor key and skip that exact row.
        // For no cursor: start from the lock_hash prefix (newest first due to desc key).
        let start_key = match cursor {
            Some((block_num, tx_idx)) => {
                keys::encode_activity_owner_key(lock_hash, block_num, tx_idx, &zero_tx_hash)
                    .to_vec()
            }
            None => prefix.to_vec(),
        };

        let iter = self.iterator_cf(
            self.cf_activity_by_owner(),
            rocksdb::IteratorMode::From(&start_key, rocksdb::Direction::Forward),
        );

        let mut results = Vec::new();
        for item in iter {
            let (key, value) = item.map_err(|e| {
                anyhow::anyhow!("failed to iterate activities in list_activities: {}", e)
            })?;
            if !key.starts_with(prefix) {
                break;
            }
            let decoded = keys::decode_activity_owner_key(&key).ok_or_else(|| {
                anyhow::anyhow!(
                    "failed to decode activity owner key in list_activities: key_len={}",
                    key.len()
                )
            })?;
            if let Some((cursor_block, cursor_tx_idx)) = cursor {
                if decoded.block_number == cursor_block && decoded.tx_index == cursor_tx_idx {
                    continue;
                }
            }

            let owner_slot = decode_owner_slot(&value)?;
            let entry = self.load_activity_entry_from_owner_ref(lock_hash, &decoded, owner_slot)?;
            if Self::matches_activity_filter(&entry, filter) {
                results.push((decoded.block_number, decoded.tx_index, entry));
                if results.len() >= limit {
                    break;
                }
            }
        }
        Ok(results)
    }

    fn load_activity_entry_from_owner_ref(
        &self,
        lock_hash: &[u8],
        owner_ref: &keys::DecodedActivityOwnerKey,
        owner_slot: u16,
    ) -> anyhow::Result<ActivityEntry> {
        let envelope_key = keys::encode_activity_tx_envelope_key(
            owner_ref.block_number,
            owner_ref.tx_index,
            &owner_ref.tx_hash,
        );
        let envelope_value = self
            .get_cf(self.cf_activity_tx_envelopes(), &envelope_key)?
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "missing activity envelope for owner ref: lock_hash=0x{}, block_number={}, tx_index={}, tx_hash=0x{}",
                    bytes_to_hex(lock_hash),
                    owner_ref.block_number,
                    owner_ref.tx_index,
                    bytes_to_hex(&owner_ref.tx_hash)
                )
            })?;
        let envelope: ActivityTxEnvelope = bincode::deserialize(&envelope_value)?;
        if envelope.block_number != owner_ref.block_number
            || envelope.tx_index != owner_ref.tx_index
            || envelope.tx_hash != owner_ref.tx_hash
        {
            anyhow::bail!(
                "activity envelope metadata mismatch for owner ref: lock_hash=0x{}, owner_ref=({}, {}, 0x{}), envelope=({}, {}, 0x{})",
                bytes_to_hex(lock_hash),
                owner_ref.block_number,
                owner_ref.tx_index,
                bytes_to_hex(&owner_ref.tx_hash),
                envelope.block_number,
                envelope.tx_index,
                bytes_to_hex(&envelope.tx_hash)
            );
        }

        let owner_slot = owner_slot as usize;
        if owner_slot >= envelope.owner_views.len() || owner_slot >= envelope.participants.len() {
            anyhow::bail!(
                "activity owner slot out of bounds: lock_hash=0x{}, owner_slot={}, participants_len={}, owner_views_len={}, tx_hash=0x{}",
                bytes_to_hex(lock_hash),
                owner_slot,
                envelope.participants.len(),
                envelope.owner_views.len(),
                bytes_to_hex(&envelope.tx_hash)
            );
        }
        if envelope.participants[owner_slot].as_slice() != lock_hash {
            anyhow::bail!(
                "activity owner ref participant mismatch: lock_hash=0x{}, participant=0x{}, owner_slot={}, tx_hash=0x{}",
                bytes_to_hex(lock_hash),
                bytes_to_hex(&envelope.participants[owner_slot]),
                owner_slot,
                bytes_to_hex(&envelope.tx_hash)
            );
        }

        let owner_view = &envelope.owner_views[owner_slot];
        let peers = envelope
            .participants
            .iter()
            .enumerate()
            .filter(|(idx, _)| *idx != owner_slot)
            .map(|(_, participant)| participant.clone())
            .collect();

        Ok(ActivityEntry {
            tx_hash: envelope.tx_hash,
            block_number: envelope.block_number,
            tx_index: envelope.tx_index,
            timestamp: envelope.timestamp,
            ckb_delta: owner_view.ckb_delta,
            occupied_delta: owner_view.occupied_delta,
            is_cellbase: envelope.is_cellbase,
            asset_changes: owner_view.asset_changes.clone(),
            peers,
        })
    }

    fn matches_activity_filter(entry: &ActivityEntry, filter: Option<&str>) -> bool {
        match filter {
            None | Some("all") => true,
            Some("ckb") => entry.asset_changes.is_empty(),
            Some("token") => entry
                .asset_changes
                .iter()
                .any(|c| matches!(c, AssetChange::Token { .. })),
            Some("nft") => entry
                .asset_changes
                .iter()
                .any(|c| matches!(c, AssetChange::Nft { .. } | AssetChange::Dob { .. })),
            Some("dao") => entry.asset_changes.iter().any(|c| {
                matches!(
                    c,
                    AssetChange::DaoDeposit { .. }
                        | AssetChange::DaoWithdrawRequest { .. }
                        | AssetChange::DaoWithdrawComplete { .. }
                )
            }),
            Some(_) => false,
        }
    }
}

fn decode_owner_slot(value: &[u8]) -> anyhow::Result<u16> {
    if value.len() != 2 {
        anyhow::bail!(
            "invalid activity owner ref payload length: expected=2, got={}",
            value.len()
        );
    }
    Ok(u16::from_be_bytes([value[0], value[1]]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::batch::StoreBatch;
    use crate::types::{ActivityTxEnvelope, OwnerActivityViewStored};
    use tempfile::TempDir;

    fn make_activity(block_num: i64, tx_idx: i32) -> ActivityEntry {
        ActivityEntry {
            tx_hash: vec![block_num as u8; 32],
            block_number: block_num,
            tx_index: tx_idx,
            timestamp: 1_700_000_000 + block_num,
            ckb_delta: 0,
            occupied_delta: 0,
            is_cellbase: false,
            asset_changes: vec![],
            peers: vec![],
        }
    }

    fn put_normalized_activity(
        batch: &mut StoreBatch<'_>,
        lock_hash: &[u8],
        entry: &ActivityEntry,
    ) {
        batch.put_activity_tx_envelope(
            entry.block_number,
            entry.tx_index,
            &entry.tx_hash,
            &ActivityTxEnvelope {
                tx_hash: entry.tx_hash.clone(),
                block_number: entry.block_number,
                tx_index: entry.tx_index,
                timestamp: entry.timestamp,
                is_cellbase: entry.is_cellbase,
                participants: vec![lock_hash.to_vec()],
                owner_views: vec![OwnerActivityViewStored {
                    ckb_delta: entry.ckb_delta,
                    occupied_delta: entry.occupied_delta,
                    asset_changes: entry.asset_changes.clone(),
                }],
            },
        );
        batch.put_activity_owner_ref(
            lock_hash,
            entry.block_number,
            entry.tx_index,
            &entry.tx_hash,
            0,
        );
    }

    #[test]
    fn test_list_activities_limit_zero_returns_empty() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_append_only(dir.path()).unwrap();
        let lock = [0xAA; 32];

        let mut batch = StoreBatch::new(&store);
        put_normalized_activity(&mut batch, &lock, &make_activity(100, 0));
        batch.commit().unwrap();

        let rows = store.list_activities(&lock, 0, None, None).unwrap();
        assert!(rows.is_empty());
    }

    #[test]
    fn test_list_activities_unknown_filter_returns_empty() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_append_only(dir.path()).unwrap();
        let lock = [0xAA; 32];

        let mut batch = StoreBatch::new(&store);
        put_normalized_activity(&mut batch, &lock, &make_activity(100, 0));
        batch.commit().unwrap();

        let rows = store.list_activities(&lock, 10, None, Some("tok")).unwrap();
        assert!(rows.is_empty());
    }

    #[test]
    fn test_list_activities_reads_normalized_owner_refs_and_envelopes() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_append_only(dir.path()).unwrap();
        let lock = [0xAA; 32];
        let peer = vec![0xBB; 32];
        let entry = ActivityEntry {
            tx_hash: vec![0x11; 32],
            block_number: 100,
            tx_index: 2,
            timestamp: 1_700_000_123,
            ckb_delta: 42,
            occupied_delta: -7,
            is_cellbase: false,
            asset_changes: vec![AssetChange::DaoDeposit { capacity: 1000 }],
            peers: vec![peer.clone()],
        };

        let mut batch = StoreBatch::new(&store);
        batch.put_activity_tx_envelope(
            entry.block_number,
            entry.tx_index,
            &entry.tx_hash,
            &ActivityTxEnvelope {
                tx_hash: entry.tx_hash.clone(),
                block_number: entry.block_number,
                tx_index: entry.tx_index,
                timestamp: entry.timestamp,
                is_cellbase: entry.is_cellbase,
                participants: vec![lock.to_vec(), peer.clone()],
                owner_views: vec![
                    OwnerActivityViewStored {
                        ckb_delta: entry.ckb_delta,
                        occupied_delta: entry.occupied_delta,
                        asset_changes: entry.asset_changes.clone(),
                    },
                    OwnerActivityViewStored {
                        ckb_delta: -42,
                        occupied_delta: 7,
                        asset_changes: vec![],
                    },
                ],
            },
        );
        batch.put_activity_owner_ref(&lock, entry.block_number, entry.tx_index, &entry.tx_hash, 0);
        batch.commit().unwrap();

        let rows = store.list_activities(&lock, 10, None, Some("dao")).unwrap();
        assert_eq!(rows.len(), 1);
        let (block_num, tx_idx, loaded) = &rows[0];
        assert_eq!(*block_num, entry.block_number);
        assert_eq!(*tx_idx, entry.tx_index);
        assert_eq!(loaded.tx_hash, entry.tx_hash);
        assert_eq!(loaded.ckb_delta, entry.ckb_delta);
        assert_eq!(loaded.occupied_delta, entry.occupied_delta);
        assert_eq!(loaded.peers, vec![peer]);
        assert!(matches!(
            loaded.asset_changes.as_slice(),
            [AssetChange::DaoDeposit { capacity }] if *capacity == 1000
        ));
    }
}
