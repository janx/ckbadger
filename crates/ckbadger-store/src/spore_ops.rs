//! Spore-specific store operations.

use crate::keys;
use crate::store::CkbadgerStore;
use crate::types::{ClusterDailyDelta, ObjectEntry, SporeDailyDelta, SporeTypeIndex};

#[cfg(test)]
use crate::batch::StoreBatch;

pub(crate) type SporeBatchEntry = (Vec<u8>, Option<ObjectEntry>);
pub(crate) type SporeOutpointLookup = (Vec<u8>, i16, Vec<u8>);
/// `(spore_id, content_type, collection_id)` for undecoded DOB spores.
pub type UndecodedDobEntry = (Vec<u8>, String, Option<Vec<u8>>);

use crate::bytes_to_hex;

impl CkbadgerStore {
    pub fn get_spore(&self, id: &[u8]) -> anyhow::Result<Option<ObjectEntry>> {
        match self.get_cf(self.cf_spore_data(), id)? {
            Some(value) => Ok(Some(bincode::deserialize(&value)?)),
            None => Ok(None),
        }
    }

    /// Batch-fetch multiple spore/DOB entries by ID in a single RocksDB multi_get.
    pub fn get_spores_batch(&self, ids: &[Vec<u8>]) -> anyhow::Result<Vec<SporeBatchEntry>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let cf = self.cf_spore_data();
        let cf_keys: Vec<(&rocksdb::ColumnFamily, &[u8])> =
            ids.iter().map(|id| (cf, id.as_slice())).collect();
        let values = self.multi_get_cf(cf_keys);
        let mut result = Vec::with_capacity(ids.len());
        for (id, value_result) in ids.iter().zip(values) {
            let entry = match value_result {
                Ok(Some(value)) => Some(bincode::deserialize::<ObjectEntry>(&value).map_err(
                    |e| {
                        anyhow::anyhow!(
                            "failed to deserialize spore entry in get_spores_batch: spore_id=0x{}, error={}",
                            bytes_to_hex(id),
                            e
                        )
                    },
                )?),
                Ok(None) => None,
                Err(e) => {
                    return Err(anyhow::anyhow!(
                        "rocksdb multi_get failed in get_spores_batch: spore_id=0x{}, error={}",
                        bytes_to_hex(id),
                        e
                    ));
                }
            };
            result.push((id.clone(), entry));
        }
        Ok(result)
    }

    pub fn put_spore_direct(&self, id: &[u8], entry: &ObjectEntry) -> anyhow::Result<()> {
        let value = bincode::serialize(entry)?;
        self.put_cf(self.cf_spore_data(), id, &value)
    }

    pub fn put_dob_decoded_direct(
        &self,
        spore_id: &[u8],
        entry: &crate::types::DobDecodedEntry,
    ) -> anyhow::Result<()> {
        let outcome = crate::types::DecodeOutcome::Decoded(entry.clone());
        let value = bincode::serialize(&outcome)?;
        self.put_cf(self.cf_dob_decoded(), spore_id, &value)
    }

    /// List all spores.
    pub fn list_spores(&self, limit: usize) -> anyhow::Result<Vec<(Vec<u8>, ObjectEntry)>> {
        let iter = self.iterator_cf(self.cf_spore_data(), rocksdb::IteratorMode::Start);
        let mut results = Vec::new();

        for item in iter {
            let (key, value) = item.map_err(|e| {
                anyhow::anyhow!("failed to iterate spore_data in list_spores: {}", e)
            })?;
            let entry: ObjectEntry = bincode::deserialize(&value).map_err(|e| {
                anyhow::anyhow!(
                    "failed to deserialize spore entry in list_spores: spore_id=0x{}, error={}",
                    bytes_to_hex(&key),
                    e
                )
            })?;
            results.push((key.to_vec(), entry));
            if results.len() >= limit {
                break;
            }
        }
        Ok(results)
    }

    /// List spores belonging to a specific cluster using the secondary index.
    pub fn list_spores_by_cluster(
        &self,
        cluster_id: &[u8],
        limit: usize,
    ) -> anyhow::Result<Vec<(Vec<u8>, ObjectEntry)>> {
        let iter = self.prefix_iterator_cf(self.cf_spore_by_cluster(), cluster_id);
        let mut spore_ids: Vec<Vec<u8>> = Vec::new();

        for item in iter {
            let (key, _) = item.map_err(|e| {
                anyhow::anyhow!(
                    "failed to iterate spore_by_cluster in list_spores_by_cluster: {}",
                    e
                )
            })?;
            if !key.starts_with(cluster_id) {
                break;
            }
            // Key: cluster_id(32B) + spore_id(32B) = 64 bytes
            if key.len() == 64 {
                spore_ids.push(key[32..64].to_vec());
                if spore_ids.len() >= limit {
                    break;
                }
            }
        }

        let entries = self.get_spores_batch(&spore_ids)?;
        Ok(entries
            .into_iter()
            .filter_map(|(spore_id, entry)| entry.map(|entry| (spore_id, entry)))
            .collect())
    }

    /// Count spores in a cluster using the secondary index.
    pub fn count_spores_in_cluster(&self, cluster_id: &[u8]) -> anyhow::Result<i64> {
        let iter = self.prefix_iterator_cf(self.cf_spore_by_cluster(), cluster_id);
        let mut count: i64 = 0;

        for item in iter {
            let (key, _) = item.map_err(|e| {
                anyhow::anyhow!(
                    "failed to iterate spore_by_cluster in count_spores_in_cluster: {}",
                    e
                )
            })?;
            if !key.starts_with(cluster_id) {
                break;
            }
            count += 1;
        }
        Ok(count)
    }

    pub fn get_spore_type_index(
        &self,
        type_script_hash: &[u8],
    ) -> anyhow::Result<Option<SporeTypeIndex>> {
        let key = keys::encode_spore_type_index_key(type_script_hash);
        match self.get_cf(self.cf_stats_spore(), &key)? {
            Some(value) => Ok(Some(bincode::deserialize(&value)?)),
            None => Ok(None),
        }
    }

    pub fn put_spore_type_index_direct(
        &self,
        type_script_hash: &[u8],
        index: &SporeTypeIndex,
    ) -> anyhow::Result<()> {
        let key = keys::encode_spore_type_index_key(type_script_hash);
        let value = bincode::serialize(index)?;
        self.put_cf(self.cf_stats_spore(), &key, &value)
    }

    pub fn get_spore_id_by_outpoint(
        &self,
        tx_hash: &[u8],
        output_index: i16,
    ) -> anyhow::Result<Option<Vec<u8>>> {
        let key = keys::encode_spore_outpoint_key(tx_hash, output_index);
        match self.get_cf(self.cf_stats_spore(), &key)? {
            // The value is the item id verbatim, at its natural width: 32 bytes
            // for spores and `.bit Cell`, 20 or 32 for did:ckb. Truncating to a
            // fixed 32 bytes would drop shorter ids on the floor, and the live
            // consume path resolves an item through exactly this lookup.
            Some(value) if !value.is_empty() => Ok(Some(value.to_vec())),
            Some(_) => Err(anyhow::anyhow!(
                "empty spore outpoint value in get_spore_id_by_outpoint: tx_hash=0x{}, output_index={}",
                bytes_to_hex(tx_hash),
                output_index
            )),
            None => Ok(None),
        }
    }

    pub fn get_spore_ids_by_outpoints_batch(
        &self,
        outpoints: &[(&[u8], i16)],
    ) -> anyhow::Result<Vec<SporeOutpointLookup>> {
        let cf = self.cf_stats_spore();
        let keys: Vec<[u8; keys::SPORE_OUTPOINT_KEY_SIZE]> = outpoints
            .iter()
            .map(|(tx_hash, idx)| keys::encode_spore_outpoint_key(tx_hash, *idx))
            .collect();
        let cf_keys: Vec<(&rocksdb::ColumnFamily, &[u8])> =
            keys.iter().map(|k| (cf, k.as_slice())).collect();
        let values = self.multi_get_cf(cf_keys);

        let mut results = Vec::new();
        for (i, value_result) in values.into_iter().enumerate() {
            let (tx_hash, idx) = outpoints[i];
            match value_result {
                Ok(Some(value)) => {
                    // Item ids are stored verbatim at their natural width
                    // (32 bytes for spores/`.bit Cell`, 20 or 32 for did:ckb).
                    if value.is_empty() || value.len() > keys::SPORE_OUTPOINT_BY_ID_MAX_ID_LEN {
                        return Err(anyhow::anyhow!(
                            "invalid spore outpoint value length in get_spore_ids_by_outpoints_batch: tx_hash=0x{}, output_index={}, value_len={}, allowed=1..={}",
                            bytes_to_hex(tx_hash),
                            idx,
                            value.len(),
                            keys::SPORE_OUTPOINT_BY_ID_MAX_ID_LEN
                        ));
                    }
                    results.push((tx_hash.to_vec(), idx, value.to_vec()));
                }
                Ok(None) => {}
                Err(e) => {
                    return Err(anyhow::anyhow!(
                        "rocksdb multi_get failed in get_spore_ids_by_outpoints_batch: tx_hash=0x{}, output_index={}, error={}",
                        bytes_to_hex(tx_hash),
                        idx,
                        e
                    ));
                }
            }
        }
        Ok(results)
    }

    /// List all historical outpoints recorded for a spore/identity ID.
    /// Uses the reverse index (id → outpoints) for O(log N) prefix scan.
    ///
    /// Ids are variable-width (see [`keys::spore_outpoint_by_id_key_len`]), so
    /// a longer id starting with these same bytes shares the scan prefix and
    /// its rows can sort between this id's rows. Foreign lengths are therefore
    /// skipped, never treated as the end of the scan.
    pub fn list_spore_outpoints_by_spore_id(
        &self,
        spore_id: &[u8],
    ) -> anyhow::Result<Vec<(Vec<u8>, i16)>> {
        let prefix = keys::encode_spore_outpoint_by_id_prefix(spore_id);
        let expected_key_len = keys::spore_outpoint_by_id_key_len(spore_id.len());
        let iter = self.prefix_iterator_cf(self.cf_stats_spore(), &prefix);
        let mut outpoints = Vec::new();

        for item in iter {
            let (key, _value) = item.map_err(|e| {
                anyhow::anyhow!(
                    "failed to iterate stats_spore in list_spore_outpoints_by_spore_id: {}",
                    e
                )
            })?;
            if !key.starts_with(&prefix) {
                break;
            }
            if key.len() != expected_key_len {
                // A different id that happens to start with these bytes.
                continue;
            }
            outpoints.push(keys::decode_spore_outpoint_by_id_key(&key));
        }

        Ok(outpoints)
    }

    pub fn get_cluster_daily_delta(
        &self,
        cluster_id: &[u8],
        date_yyyymmdd: u32,
    ) -> anyhow::Result<Option<ClusterDailyDelta>> {
        let key = keys::encode_cluster_daily_key(cluster_id, date_yyyymmdd);
        match self.get_cf(self.cf_stats_spore(), &key)? {
            Some(value) => Ok(Some(bincode::deserialize(&value)?)),
            None => Ok(None),
        }
    }

    pub fn put_cluster_daily_delta(
        &self,
        cluster_id: &[u8],
        date_yyyymmdd: u32,
        delta: &ClusterDailyDelta,
    ) -> anyhow::Result<()> {
        let key = keys::encode_cluster_daily_key(cluster_id, date_yyyymmdd);
        let value = bincode::serialize(delta)?;
        self.put_cf(self.cf_stats_spore(), &key, &value)
    }

    pub fn list_cluster_daily_deltas(
        &self,
        cluster_id: &[u8],
    ) -> anyhow::Result<Vec<(u32, ClusterDailyDelta)>> {
        self.list_cluster_daily_deltas_in_range(cluster_id, None, None)
    }

    pub fn list_cluster_daily_deltas_in_range(
        &self,
        cluster_id: &[u8],
        from_date_yyyymmdd: Option<u32>,
        to_date_yyyymmdd: Option<u32>,
    ) -> anyhow::Result<Vec<(u32, ClusterDailyDelta)>> {
        let prefix = keys::encode_cluster_daily_prefix(cluster_id);
        let start_key =
            keys::encode_cluster_daily_key(cluster_id, from_date_yyyymmdd.unwrap_or(u32::MIN));
        let iter = self.iterator_cf(
            self.cf_stats_spore(),
            rocksdb::IteratorMode::From(&start_key, rocksdb::Direction::Forward),
        );
        let mut results = Vec::new();

        for item in iter {
            let (key, value) = item.map_err(|e| {
                anyhow::anyhow!(
                    "failed to iterate stats_spore in list_cluster_daily_deltas_in_range: {}",
                    e
                )
            })?;
            if !key.starts_with(&prefix) {
                break;
            }
            if key.len() != keys::CLUSTER_DAILY_KEY_SIZE {
                continue;
            }
            let (_, date) = keys::decode_cluster_daily_key(&key);
            if let Some(to_date) = to_date_yyyymmdd {
                if date > to_date {
                    break;
                }
            }
            let delta: ClusterDailyDelta = bincode::deserialize(&value).map_err(|e| {
                anyhow::anyhow!(
                    "failed to deserialize cluster daily delta in list_cluster_daily_deltas_in_range: cluster_id=0x{}, date={}, error={}",
                    bytes_to_hex(cluster_id),
                    date,
                    e
                )
            })?;
            results.push((date, delta));
        }

        Ok(results)
    }

    // ---- DOB decoded cache ----

    pub fn get_dob_decode_outcome(
        &self,
        spore_id: &[u8],
    ) -> anyhow::Result<Option<crate::types::DecodeOutcome>> {
        match self.get_cf(self.cf_dob_decoded(), spore_id)? {
            Some(value) => Ok(Some(bincode::deserialize(&value)?)),
            None => Ok(None),
        }
    }

    /// Success-only convenience: returns `Some` only for a `Decoded` outcome.
    /// A `Failed` outcome (or absence) returns `None`.
    pub fn get_dob_decoded(
        &self,
        spore_id: &[u8],
    ) -> anyhow::Result<Option<crate::types::DobDecodedEntry>> {
        Ok(self
            .get_dob_decode_outcome(spore_id)?
            .and_then(|o| match o {
                crate::types::DecodeOutcome::Decoded(e) => Some(e),
                crate::types::DecodeOutcome::Failed(_) => None,
            }))
    }

    /// List spores with DOB content types that have not yet been decoded.
    ///
    /// Returns `(spore_id, content_type, collection_id)` tuples.
    /// Uses keyset pagination via `after_key` for incremental scanning.
    pub fn list_undecoded_dob_spores(
        &self,
        limit: usize,
        after_key: Option<&[u8]>,
    ) -> anyhow::Result<Vec<UndecodedDobEntry>> {
        use crate::types::{ObjectEntry, ObjectExtra};

        let mode = match after_key {
            Some(key) => rocksdb::IteratorMode::From(key, rocksdb::Direction::Forward),
            None => rocksdb::IteratorMode::Start,
        };
        let iter = self.iterator_cf(self.cf_spore_data(), mode);
        let mut results = Vec::new();
        let mut skip_first = after_key.is_some();

        for item in iter {
            let (key, value) = item.map_err(|e| {
                anyhow::anyhow!(
                    "failed to iterate spore_data in list_undecoded_dob_spores: {}",
                    e
                )
            })?;
            if skip_first {
                skip_first = false;
                if after_key.is_some_and(|ak| ak == key.as_ref()) {
                    continue;
                }
            }
            let entry: ObjectEntry = bincode::deserialize(&value)?;
            if let ObjectExtra::Spore { content_type, .. } = &entry.extra {
                if content_type.to_ascii_lowercase().starts_with("dob/")
                    && self.get_cf(self.cf_dob_decoded(), &key)?.is_none()
                {
                    results.push((
                        key.to_vec(),
                        content_type.clone(),
                        entry.collection_id.clone(),
                    ));
                    if results.len() >= limit {
                        break;
                    }
                }
            }
        }
        Ok(results)
    }

    pub fn get_spore_daily_delta(
        &self,
        spore_id: &[u8],
        date_yyyymmdd: u32,
    ) -> anyhow::Result<Option<SporeDailyDelta>> {
        let key = keys::encode_spore_daily_key(spore_id, date_yyyymmdd);
        match self.get_cf(self.cf_stats_spore(), &key)? {
            Some(value) => Ok(Some(bincode::deserialize(&value)?)),
            None => Ok(None),
        }
    }

    pub fn put_spore_daily_delta(
        &self,
        spore_id: &[u8],
        date_yyyymmdd: u32,
        delta: &SporeDailyDelta,
    ) -> anyhow::Result<()> {
        let key = keys::encode_spore_daily_key(spore_id, date_yyyymmdd);
        let value = bincode::serialize(delta)?;
        self.put_cf(self.cf_stats_spore(), &key, &value)
    }

    pub fn list_spore_daily_deltas(
        &self,
        spore_id: &[u8],
    ) -> anyhow::Result<Vec<(u32, SporeDailyDelta)>> {
        self.list_spore_daily_deltas_in_range(spore_id, None, None)
    }

    pub fn list_spore_daily_deltas_in_range(
        &self,
        spore_id: &[u8],
        from_date_yyyymmdd: Option<u32>,
        to_date_yyyymmdd: Option<u32>,
    ) -> anyhow::Result<Vec<(u32, SporeDailyDelta)>> {
        let prefix = keys::encode_spore_daily_prefix(spore_id);
        let start_key =
            keys::encode_spore_daily_key(spore_id, from_date_yyyymmdd.unwrap_or(u32::MIN));
        let iter = self.iterator_cf(
            self.cf_stats_spore(),
            rocksdb::IteratorMode::From(&start_key, rocksdb::Direction::Forward),
        );
        let mut results = Vec::new();

        for item in iter {
            let (key, value) = item.map_err(|e| {
                anyhow::anyhow!(
                    "failed to iterate stats_spore in list_spore_daily_deltas_in_range: {}",
                    e
                )
            })?;
            if !key.starts_with(&prefix) {
                break;
            }
            if key.len() != keys::SPORE_DAILY_KEY_SIZE {
                continue;
            }
            let (_, date) = keys::decode_spore_daily_key(&key);
            if let Some(to_date) = to_date_yyyymmdd {
                if date > to_date {
                    break;
                }
            }
            let delta: SporeDailyDelta = bincode::deserialize(&value).map_err(|e| {
                anyhow::anyhow!(
                    "failed to deserialize spore daily delta in list_spore_daily_deltas_in_range: spore_id=0x{}, date={}, error={}",
                    bytes_to_hex(spore_id),
                    date,
                    e
                )
            })?;
            results.push((date, delta));
        }

        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ObjectExtra;
    use crate::types::{ClusterDailyDelta, SporeDailyDelta, SporeTypeIndex};
    use tempfile::TempDir;

    fn test_store() -> (TempDir, CkbadgerStore) {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        (dir, store)
    }

    /// Non-aliasing, both directions: a 20-byte item id whose bytes are a
    /// prefix of a 32-byte item id shares the scan prefix, so neither id may
    /// ever see the other's outpoints. Real shapes: did:ckb ids are the
    /// type-script args verbatim (both widths occur on live testnet), while
    /// spores and `.bit Cell` are always 32 bytes.
    #[test]
    fn test_list_spore_outpoints_does_not_alias_between_short_and_long_ids() {
        use crate::batch::StoreBatch;

        let (_dir, store) = test_store();

        let short_id = vec![0x11u8; 20];
        let mut long_id = short_id.clone();
        long_id.extend_from_slice(&[0x11u8; 12]);
        assert_eq!(long_id.len(), 32);
        assert!(long_id.starts_with(&short_id));

        let short_tx = [0xA1u8; 32];
        let long_tx = [0xB2u8; 32];

        let mut batch = StoreBatch::new(&store);
        batch.put_spore_outpoint(&short_tx, 0, &short_id);
        batch.put_spore_outpoint(&short_tx, 1, &short_id);
        batch.put_spore_outpoint(&long_tx, 0, &long_id);
        batch.commit().unwrap();

        let short_outpoints = store.list_spore_outpoints_by_spore_id(&short_id).unwrap();
        assert_eq!(
            short_outpoints,
            vec![(short_tx.to_vec(), 0), (short_tx.to_vec(), 1)],
            "20-byte id must see exactly its own outpoints"
        );

        let long_outpoints = store.list_spore_outpoints_by_spore_id(&long_id).unwrap();
        assert_eq!(
            long_outpoints,
            vec![(long_tx.to_vec(), 0)],
            "32-byte id must see exactly its own outpoints"
        );
    }

    /// The aliasing hazard is order-sensitive: a longer id's rows can sort
    /// *between* a shorter id's rows, so a scan that stops at the first
    /// foreign key would silently truncate real results.
    #[test]
    fn test_list_spore_outpoints_skips_interleaved_longer_id_rows() {
        use crate::batch::StoreBatch;

        let (_dir, store) = test_store();

        let short_id = vec![0x22u8; 20];
        // Long id shares the 20-byte prefix and its 21st byte (0x00) sorts
        // BEFORE the first outpoint byte (0xF0) of the short id's second row,
        // placing the long id's key inside the short id's key range.
        let mut long_id = short_id.clone();
        long_id.extend_from_slice(&[0x00u8; 12]);

        let low_tx = [0x0Fu8; 32];
        let high_tx = [0xF0u8; 32];
        let long_tx = [0x77u8; 32];

        let mut batch = StoreBatch::new(&store);
        batch.put_spore_outpoint(&low_tx, 0, &short_id);
        batch.put_spore_outpoint(&high_tx, 0, &short_id);
        batch.put_spore_outpoint(&long_tx, 0, &long_id);
        batch.commit().unwrap();

        let short_outpoints = store.list_spore_outpoints_by_spore_id(&short_id).unwrap();
        assert_eq!(
            short_outpoints,
            vec![(low_tx.to_vec(), 0), (high_tx.to_vec(), 0)],
            "an interleaved longer-id row must be skipped, not truncate the scan"
        );
        assert_eq!(
            store.list_spore_outpoints_by_spore_id(&long_id).unwrap(),
            vec![(long_tx.to_vec(), 0)]
        );
    }

    /// Spore and `.bit Cell` ids are always 32 bytes; widening the index key
    /// must not change what a 32-byte id sees.
    #[test]
    fn test_list_spore_outpoints_for_32_byte_ids_is_unchanged() {
        use crate::batch::StoreBatch;

        let (_dir, store) = test_store();
        let spore_id = vec![0x33u8; 32];
        let other_id = vec![0x44u8; 32];
        let tx_a = [0xC1u8; 32];
        let tx_b = [0xC2u8; 32];

        let mut batch = StoreBatch::new(&store);
        batch.put_spore_outpoint(&tx_a, 0, &spore_id);
        batch.put_spore_outpoint(&tx_b, 2, &spore_id);
        batch.put_spore_outpoint(&tx_a, 1, &other_id);
        batch.commit().unwrap();

        assert_eq!(
            store.list_spore_outpoints_by_spore_id(&spore_id).unwrap(),
            vec![(tx_a.to_vec(), 0), (tx_b.to_vec(), 2)]
        );
        assert_eq!(
            store.list_spore_outpoints_by_spore_id(&other_id).unwrap(),
            vec![(tx_a.to_vec(), 1)]
        );
        assert!(store
            .list_spore_outpoints_by_spore_id(&[0x55u8; 32])
            .unwrap()
            .is_empty());
    }

    #[test]
    fn test_dob_outcome_read_write_and_undecoded_skip() {
        use crate::batch::StoreBatch;
        use crate::types::{
            CompositionTier, DobDecodeFailure, DobDecodeFailureCategory, DobDecodedEntry,
            ObjectEntry, ObjectExtra, ObjectStandard, SporeMediaProfile,
        };

        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();

        // Two dob/0 spores so list_undecoded has candidates.
        let mk = |content: &str| ObjectEntry {
            standard: ObjectStandard::Spore,
            collection_id: Some(vec![0x11; 32]),
            token_id: None,
            owner_lock_hash: Some(vec![0x33; 32]),
            name: None,
            description: None,
            is_live: true,
            created_at_block: 1,
            created_at_tx: vec![0x44; 32],
            extra: ObjectExtra::Spore {
                content_type: content.to_string(),
                content_length: 3,
                media_profile: SporeMediaProfile {
                    tier: CompositionTier::PureCkb,
                    sources: vec![],
                    issues: vec![],
                },
            },
        };
        let decoded_id = [0xAA_u8; 32];
        let failed_id = [0xBB_u8; 32];
        store.put_spore_direct(&decoded_id, &mk("dob/0")).unwrap();
        store.put_spore_direct(&failed_id, &mk("dob/0")).unwrap();

        // Write one Decoded, one Failed.
        let mut b = StoreBatch::new(&store);
        b.put_dob_decoded(
            &decoded_id,
            &DobDecodedEntry {
                steps: vec![],
                media_sources: vec![],
                decoded_at: 1,
            },
        );
        b.put_dob_decode_failure(
            &failed_id,
            &DobDecodeFailure {
                category: DobDecodeFailureCategory::ClusterNotFound,
                message: "cluster entry not found".to_string(),
                failed_at: 2,
            },
        );
        b.commit().unwrap();

        // get_dob_decode_outcome returns the right variant.
        match store.get_dob_decode_outcome(&decoded_id).unwrap().unwrap() {
            crate::types::DecodeOutcome::Decoded(e) => assert_eq!(e.decoded_at, 1),
            _ => panic!("expected Decoded"),
        }
        match store.get_dob_decode_outcome(&failed_id).unwrap().unwrap() {
            crate::types::DecodeOutcome::Failed(f) => {
                assert_eq!(f.category, DobDecodeFailureCategory::ClusterNotFound)
            }
            _ => panic!("expected Failed"),
        }

        // get_dob_decoded is success-only.
        assert!(store.get_dob_decoded(&decoded_id).unwrap().is_some());
        assert!(store.get_dob_decoded(&failed_id).unwrap().is_none());

        // list_undecoded skips BOTH (decoded and failed count as processed).
        let undecoded = store.list_undecoded_dob_spores(100, None).unwrap();
        assert!(undecoded.iter().all(|(k, _, _)| k != &decoded_id.to_vec()));
        assert!(undecoded.iter().all(|(k, _, _)| k != &failed_id.to_vec()));
    }

    #[test]
    fn test_spore_type_index_roundtrip() {
        let (_dir, store) = test_store();
        let type_script_hash = [0x11u8; 32];
        let index = SporeTypeIndex {
            spore_id: vec![0x22; 32],
            cluster_id: Some(vec![0x33; 32]),
        };

        store
            .put_spore_type_index_direct(&type_script_hash, &index)
            .unwrap();

        let loaded = store
            .get_spore_type_index(&type_script_hash)
            .unwrap()
            .unwrap();
        assert_eq!(loaded.spore_id, index.spore_id);
        assert_eq!(loaded.cluster_id, index.cluster_id);
    }

    #[test]
    fn test_spore_outpoint_roundtrip_and_batch_lookup() {
        let (_dir, store) = test_store();
        let tx_a = [0xA1u8; 32];
        let tx_b = [0xB2u8; 32];
        let spore_a = [0x11u8; 32];
        let spore_b = [0x22u8; 32];

        let mut batch = StoreBatch::new(&store);
        batch.put_spore_outpoint(&tx_a, 1, &spore_a);
        batch.put_spore_outpoint(&tx_b, 2, &spore_b);
        batch.commit().unwrap();

        let single = store.get_spore_id_by_outpoint(&tx_a, 1).unwrap().unwrap();
        assert_eq!(single, spore_a.to_vec());
        assert!(store.get_spore_id_by_outpoint(&tx_a, 9).unwrap().is_none());

        let outpoints: Vec<(&[u8], i16)> = vec![(&tx_a, 1), (&tx_b, 2), (&tx_a, 9)];
        let results = store.get_spore_ids_by_outpoints_batch(&outpoints).unwrap();
        assert_eq!(results.len(), 2);
        assert!(results
            .iter()
            .any(|(tx, idx, id)| tx == tx_a.as_slice() && *idx == 1 && id == spore_a.as_slice()));
        assert!(results
            .iter()
            .any(|(tx, idx, id)| tx == tx_b.as_slice() && *idx == 2 && id == spore_b.as_slice()));

        let mut spore_outpoints = store.list_spore_outpoints_by_spore_id(&spore_a).unwrap();
        spore_outpoints.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
        assert_eq!(spore_outpoints.len(), 1);
        assert_eq!(spore_outpoints[0], (tx_a.to_vec(), 1));
    }

    /// Item ids are stored verbatim at their natural width, so a shorter id is
    /// valid data — only widths outside the indexable range are corruption.
    #[test]
    fn test_get_spore_ids_by_outpoints_batch_fails_on_unindexable_value_width() {
        let (_dir, store) = test_store();

        for (tx_byte, bad_value) in [(0xA1u8, Vec::new()), (0xA2u8, vec![0x11u8; 33])] {
            let tx = [tx_byte; 32];
            let key = keys::encode_spore_outpoint_key(&tx, 1);
            store
                .put_cf(store.cf_stats_spore(), &key, &bad_value)
                .unwrap();

            let outpoints: Vec<(&[u8], i16)> = vec![(&tx, 1)];
            let err = store
                .get_spore_ids_by_outpoints_batch(&outpoints)
                .unwrap_err();
            assert!(err.to_string().contains(
                "invalid spore outpoint value length in get_spore_ids_by_outpoints_batch"
            ));
        }
    }

    /// A 20-byte did:ckb item id must survive the outpoint lookup verbatim —
    /// truncating or dropping it would break live-sync item resolution.
    #[test]
    fn test_spore_outpoint_lookups_return_20_byte_ids_verbatim() {
        use crate::batch::StoreBatch;

        let (_dir, store) = test_store();
        let tx = [0xD1u8; 32];
        let short_id = vec![0x5Au8; 20];

        let mut batch = StoreBatch::new(&store);
        batch.put_spore_outpoint(&tx, 0, &short_id);
        batch.commit().unwrap();

        assert_eq!(
            store.get_spore_id_by_outpoint(&tx, 0).unwrap(),
            Some(short_id.clone())
        );

        let outpoints: Vec<(&[u8], i16)> = vec![(&tx, 0)];
        assert_eq!(
            store.get_spore_ids_by_outpoints_batch(&outpoints).unwrap(),
            vec![(tx.to_vec(), 0, short_id)]
        );
    }

    #[test]
    fn test_list_spores_fails_on_invalid_payload() {
        let (_dir, store) = test_store();
        let spore_id = [0x11u8; 32];
        store
            .put_cf(store.cf_spore_data(), &spore_id, b"invalid-spore-payload")
            .unwrap();

        let err = store.list_spores(10).unwrap_err();
        assert!(err
            .to_string()
            .contains("failed to deserialize spore entry in list_spores"));
    }

    #[test]
    fn test_list_cluster_daily_deltas_fails_on_invalid_payload() {
        let (_dir, store) = test_store();
        let cluster_id = [0x44u8; 32];
        let key = keys::encode_cluster_daily_key(&cluster_id, 20260219);
        store
            .put_cf(store.cf_stats_spore(), &key, b"invalid-cluster-daily")
            .unwrap();

        let err = store.list_cluster_daily_deltas(&cluster_id).unwrap_err();
        assert!(err.to_string().contains(
            "failed to deserialize cluster daily delta in list_cluster_daily_deltas_in_range"
        ));
    }

    #[test]
    fn test_list_spore_daily_deltas_fails_on_invalid_payload() {
        let (_dir, store) = test_store();
        let spore_id = [0x55u8; 32];
        let key = keys::encode_spore_daily_key(&spore_id, 20260219);
        store
            .put_cf(store.cf_stats_spore(), &key, b"invalid-spore-daily")
            .unwrap();

        let err = store.list_spore_daily_deltas(&spore_id).unwrap_err();
        assert!(err.to_string().contains(
            "failed to deserialize spore daily delta in list_spore_daily_deltas_in_range"
        ));
    }

    #[test]
    fn test_list_spores_by_cluster_returns_entries() {
        let (_dir, store) = test_store();
        let cluster_id = [0x44u8; 32];
        let spore_a = [0xA1u8; 32];
        let spore_b = [0xB2u8; 32];
        let other_cluster = [0x55u8; 32];
        let spore_other = [0xC3u8; 32];

        let make_entry = |created_at_block: i64| ObjectEntry {
            standard: crate::types::ObjectStandard::Spore,
            collection_id: Some(cluster_id.to_vec()),
            token_id: None,
            owner_lock_hash: Some(vec![0x11; 32]),
            name: Some("spore".to_string()),
            description: None,
            is_live: true,
            created_at_block,
            created_at_tx: vec![0x22; 32],
            extra: ObjectExtra::Spore {
                content_type: "text/plain".to_string(),
                content_length: 5,
                media_profile: Default::default(),
            },
        };

        let mut batch = StoreBatch::new(&store);
        batch.put_spore(&spore_a, &make_entry(10));
        batch.put_spore(&spore_b, &make_entry(20));
        batch.put_spore(
            &spore_other,
            &ObjectEntry {
                collection_id: Some(other_cluster.to_vec()),
                ..make_entry(30)
            },
        );
        batch.put_spore_by_cluster(&cluster_id, &spore_a);
        batch.put_spore_by_cluster(&cluster_id, &spore_b);
        batch.put_spore_by_cluster(&other_cluster, &spore_other);
        batch.commit().unwrap();

        let rows = store.list_spores_by_cluster(&cluster_id, 10).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].0, spore_a.to_vec());
        assert_eq!(rows[1].0, spore_b.to_vec());
    }

    #[test]
    fn test_list_spores_by_cluster_fails_on_invalid_spore_payload() {
        let (_dir, store) = test_store();
        let cluster_id = [0x66u8; 32];
        let spore_id = [0x77u8; 32];

        let mut batch = StoreBatch::new(&store);
        batch.put_spore_by_cluster(&cluster_id, &spore_id);
        batch.commit().unwrap();
        store
            .put_cf(store.cf_spore_data(), &spore_id, b"invalid-spore-payload")
            .unwrap();

        let err = store.list_spores_by_cluster(&cluster_id, 10).unwrap_err();
        assert!(err
            .to_string()
            .contains("failed to deserialize spore entry in get_spores_batch"));
    }

    #[test]
    fn test_cluster_and_spore_daily_delta_roundtrip() {
        let (_dir, store) = test_store();
        let cluster_id = [0x44u8; 32];
        let spore_id = [0x55u8; 32];

        store
            .put_cluster_daily_delta(
                &cluster_id,
                20260219,
                &ClusterDailyDelta {
                    owned_capacity_delta: 1000,
                    owned_knowledge_delta: 600,
                },
            )
            .unwrap();
        store
            .put_spore_daily_delta(
                &spore_id,
                20260219,
                &SporeDailyDelta {
                    owned_capacity_delta: 100,
                    owned_knowledge_delta: 61,
                },
            )
            .unwrap();

        let cluster = store
            .get_cluster_daily_delta(&cluster_id, 20260219)
            .unwrap()
            .unwrap();
        assert_eq!(cluster.owned_capacity_delta, 1000);
        assert_eq!(cluster.owned_knowledge_delta, 600);

        let spore = store
            .get_spore_daily_delta(&spore_id, 20260219)
            .unwrap()
            .unwrap();
        assert_eq!(spore.owned_capacity_delta, 100);
        assert_eq!(spore.owned_knowledge_delta, 61);

        let cluster_list = store.list_cluster_daily_deltas(&cluster_id).unwrap();
        assert_eq!(cluster_list.len(), 1);
        assert_eq!(cluster_list[0].0, 20260219);

        let spore_list = store.list_spore_daily_deltas(&spore_id).unwrap();
        assert_eq!(spore_list.len(), 1);
        assert_eq!(spore_list[0].0, 20260219);

        let cluster_ranged = store
            .list_cluster_daily_deltas_in_range(&cluster_id, Some(20260219), Some(20260219))
            .unwrap();
        assert_eq!(cluster_ranged.len(), 1);
        assert_eq!(cluster_ranged[0].0, 20260219);

        let spore_ranged = store
            .list_spore_daily_deltas_in_range(&spore_id, Some(20260219), Some(20260219))
            .unwrap();
        assert_eq!(spore_ranged.len(), 1);
        assert_eq!(spore_ranged[0].0, 20260219);
    }
}
