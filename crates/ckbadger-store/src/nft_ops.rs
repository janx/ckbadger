//! NFT (Spore, mNFT, DotBit) operations.

use std::collections::{HashMap, HashSet};

use crate::batch::StoreBatch;
use crate::keys;
use crate::store::CkbadgerStore;
use crate::types::{
    ClusterDailyDelta, NftCollectionAggregate, NftDailyDelta, NftEntry, NftTypeIndex,
    SporeDailyDelta, SporeEntry, SporeTypeIndex,
};

type DotbitLiveOutpoint = (Vec<u8>, i16);
type DotbitLiveOutpointMap = HashMap<Vec<u8>, DotbitLiveOutpoint>;

impl CkbadgerStore {
    pub fn get_spore(&self, id: &[u8]) -> anyhow::Result<Option<SporeEntry>> {
        match self.get_cf(self.cf_spore_data(), id)? {
            Some(value) => Ok(Some(bincode::deserialize(&value)?)),
            None => Ok(None),
        }
    }

    pub fn put_spore_direct(&self, id: &[u8], entry: &SporeEntry) -> anyhow::Result<()> {
        let value = bincode::serialize(entry)?;
        self.put_cf(self.cf_spore_data(), id, &value)
    }

    /// List all spores.
    pub fn list_spores(&self, limit: usize) -> anyhow::Result<Vec<(Vec<u8>, SporeEntry)>> {
        let iter = self.iterator_cf(self.cf_spore_data(), rocksdb::IteratorMode::Start);
        let mut results = Vec::new();

        for item in iter.flatten() {
            let (key, value) = item;
            if let Ok(entry) = bincode::deserialize::<SporeEntry>(&value) {
                results.push((key.to_vec(), entry));
                if results.len() >= limit {
                    break;
                }
            }
        }
        Ok(results)
    }

    /// List spores belonging to a specific cluster using the secondary index.
    pub fn list_spores_by_cluster(
        &self,
        cluster_id: &[u8],
        limit: usize,
    ) -> anyhow::Result<Vec<(Vec<u8>, SporeEntry)>> {
        let iter = self.prefix_iterator_cf(self.cf_spore_by_cluster(), cluster_id);
        let mut results = Vec::new();

        for item in iter.flatten() {
            let (key, _) = item;
            if !key.starts_with(cluster_id) {
                break;
            }
            // Key: cluster_id(32B) + spore_id(32B) = 64 bytes
            if key.len() == 64 {
                let spore_id = key[32..64].to_vec();
                if let Ok(Some(entry)) = self.get_spore(&spore_id) {
                    results.push((spore_id, entry));
                    if results.len() >= limit {
                        break;
                    }
                }
            }
        }
        Ok(results)
    }

    /// Count spores in a cluster using the secondary index.
    pub fn count_spores_in_cluster(&self, cluster_id: &[u8]) -> anyhow::Result<i64> {
        let iter = self.prefix_iterator_cf(self.cf_spore_by_cluster(), cluster_id);
        let mut count: i64 = 0;

        for item in iter.flatten() {
            let (key, _) = item;
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
        match self.get_cf(self.cf_stats(), &key)? {
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
        self.put_cf(self.cf_stats(), &key, &value)
    }

    pub fn get_spore_id_by_outpoint(
        &self,
        tx_hash: &[u8],
        output_index: i16,
    ) -> anyhow::Result<Option<Vec<u8>>> {
        let key = keys::encode_spore_outpoint_key(tx_hash, output_index);
        match self.get_cf(self.cf_stats(), &key)? {
            Some(value) if value.len() >= 32 => Ok(Some(value[..32].to_vec())),
            _ => Ok(None),
        }
    }

    pub fn get_spore_ids_by_outpoints_batch(
        &self,
        outpoints: &[(&[u8], i16)],
    ) -> Vec<(Vec<u8>, i16, Vec<u8>)> {
        let cf = self.cf_stats();
        let keys: Vec<[u8; keys::SPORE_OUTPOINT_KEY_SIZE]> = outpoints
            .iter()
            .map(|(tx_hash, idx)| keys::encode_spore_outpoint_key(tx_hash, *idx))
            .collect();
        let cf_keys: Vec<(&rocksdb::ColumnFamily, &[u8])> =
            keys.iter().map(|k| (cf, k.as_slice())).collect();
        let values = self.multi_get_cf(cf_keys);

        let mut results = Vec::new();
        for (i, value_result) in values.into_iter().enumerate() {
            if let Ok(Some(value)) = value_result {
                if value.len() >= 32 {
                    let (tx_hash, idx) = outpoints[i];
                    results.push((tx_hash.to_vec(), idx, value[..32].to_vec()));
                }
            }
        }
        results
    }

    pub fn get_mnft_class_id_by_outpoint(
        &self,
        tx_hash: &[u8],
        output_index: i16,
    ) -> anyhow::Result<Option<Vec<u8>>> {
        let key = keys::encode_mnft_class_outpoint_key(tx_hash, output_index);
        match self.get_cf(self.cf_stats(), &key)? {
            Some(value) if !value.is_empty() => Ok(Some(value)),
            _ => Ok(None),
        }
    }

    pub fn get_mnft_token_id_by_outpoint(
        &self,
        tx_hash: &[u8],
        output_index: i16,
    ) -> anyhow::Result<Option<Vec<u8>>> {
        let key = keys::encode_mnft_token_outpoint_key(tx_hash, output_index);
        match self.get_cf(self.cf_stats(), &key)? {
            Some(value) if !value.is_empty() => Ok(Some(value)),
            _ => Ok(None),
        }
    }

    pub fn get_mnft_token_ids_by_outpoints_batch(
        &self,
        outpoints: &[(&[u8], i16)],
    ) -> Vec<(Vec<u8>, i16, Vec<u8>)> {
        let cf = self.cf_stats();
        let keys: Vec<[u8; keys::MNFT_TOKEN_OUTPOINT_KEY_SIZE]> = outpoints
            .iter()
            .map(|(tx_hash, idx)| keys::encode_mnft_token_outpoint_key(tx_hash, *idx))
            .collect();
        let cf_keys: Vec<(&rocksdb::ColumnFamily, &[u8])> =
            keys.iter().map(|k| (cf, k.as_slice())).collect();
        let values = self.multi_get_cf(cf_keys);

        let mut results = Vec::new();
        for (i, value_result) in values.into_iter().enumerate() {
            if let Ok(Some(value)) = value_result {
                if !value.is_empty() {
                    let (tx_hash, idx) = outpoints[i];
                    results.push((tx_hash.to_vec(), idx, value));
                }
            }
        }
        results
    }

    pub fn get_dotbit_account_id_by_outpoint(
        &self,
        tx_hash: &[u8],
        output_index: i16,
    ) -> anyhow::Result<Option<Vec<u8>>> {
        let key = keys::encode_dotbit_account_outpoint_key(tx_hash, output_index);
        match self.get_cf(self.cf_stats(), &key)? {
            Some(value) if !value.is_empty() => Ok(Some(value)),
            _ => Ok(None),
        }
    }

    pub fn get_dotbit_account_ids_by_outpoints_batch(
        &self,
        outpoints: &[(&[u8], i16)],
    ) -> Vec<(Vec<u8>, i16, Vec<u8>)> {
        let cf = self.cf_stats();
        let keys: Vec<[u8; keys::DOTBIT_ACCOUNT_OUTPOINT_KEY_SIZE]> = outpoints
            .iter()
            .map(|(tx_hash, idx)| keys::encode_dotbit_account_outpoint_key(tx_hash, *idx))
            .collect();
        let cf_keys: Vec<(&rocksdb::ColumnFamily, &[u8])> =
            keys.iter().map(|k| (cf, k.as_slice())).collect();
        let values = self.multi_get_cf(cf_keys);

        let mut results = Vec::new();
        for (i, value_result) in values.into_iter().enumerate() {
            if let Ok(Some(value)) = value_result {
                if !value.is_empty() {
                    let (tx_hash, idx) = outpoints[i];
                    results.push((tx_hash.to_vec(), idx, value));
                }
            }
        }
        results
    }

    /// Resolve live dotbit account outpoints by account IDs.
    ///
    /// Scans dotbit outpoint index in `stats` and validates liveness via `live_cells`.
    /// Returns account_id -> (tx_hash, output_index) for accounts that currently have
    /// a live outpoint.
    pub fn get_live_dotbit_outpoints_by_account_ids(
        &self,
        account_ids: &[Vec<u8>],
    ) -> anyhow::Result<DotbitLiveOutpointMap> {
        let targets: HashSet<Vec<u8>> = account_ids.iter().cloned().collect();
        if targets.is_empty() {
            return Ok(HashMap::new());
        }

        let prefix = [keys::STATS_PREFIX_DOTBIT_ACCOUNT_OUTPOINT];
        let iter = self.prefix_iterator_cf(self.cf_stats(), &prefix);
        let mut resolved: DotbitLiveOutpointMap = HashMap::with_capacity(targets.len());

        for item in iter.flatten() {
            let (key, value) = item;
            if key.first() != Some(&keys::STATS_PREFIX_DOTBIT_ACCOUNT_OUTPOINT) {
                break;
            }
            if key.len() != keys::DOTBIT_ACCOUNT_OUTPOINT_KEY_SIZE {
                anyhow::bail!(
                    "invalid dotbit outpoint key length: expected {}, got {}",
                    keys::DOTBIT_ACCOUNT_OUTPOINT_KEY_SIZE,
                    key.len()
                );
            }
            if !targets.contains(value.as_ref()) {
                continue;
            }

            let (tx_hash, output_index) = keys::decode_dotbit_account_outpoint_key(&key);
            if self.get_cell(&tx_hash, output_index)?.is_none() {
                continue;
            }

            if let Some((existing_tx_hash, existing_output_index)) = resolved.get(value.as_ref()) {
                if existing_tx_hash != &tx_hash || *existing_output_index != output_index {
                    anyhow::bail!(
                        "multiple live dotbit outpoints for account_id=0x{:x?}: first=0x{:x?}-{}, second=0x{:x?}-{}",
                        value.as_ref(),
                        existing_tx_hash,
                        existing_output_index,
                        tx_hash,
                        output_index
                    );
                }
            } else {
                resolved.insert(value.to_vec(), (tx_hash, output_index));
            }

            if resolved.len() == targets.len() {
                break;
            }
        }

        Ok(resolved)
    }

    pub fn get_nft_type_index(
        &self,
        type_script_hash: &[u8],
    ) -> anyhow::Result<Option<NftTypeIndex>> {
        let key = keys::encode_nft_type_index_key(type_script_hash);
        match self.get_cf(self.cf_stats(), &key)? {
            Some(value) => Ok(Some(bincode::deserialize(&value)?)),
            None => Ok(None),
        }
    }

    pub fn put_nft_type_index_direct(
        &self,
        type_script_hash: &[u8],
        index: &NftTypeIndex,
    ) -> anyhow::Result<()> {
        let key = keys::encode_nft_type_index_key(type_script_hash);
        let value = bincode::serialize(index)?;
        self.put_cf(self.cf_stats(), &key, &value)
    }

    pub fn get_cluster_daily_delta(
        &self,
        cluster_id: &[u8],
        date_yyyymmdd: u32,
    ) -> anyhow::Result<Option<ClusterDailyDelta>> {
        let key = keys::encode_cluster_daily_key(cluster_id, date_yyyymmdd);
        match self.get_cf(self.cf_stats(), &key)? {
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
        self.put_cf(self.cf_stats(), &key, &value)
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
            self.cf_stats(),
            rocksdb::IteratorMode::From(&start_key, rocksdb::Direction::Forward),
        );
        let mut results = Vec::new();

        for item in iter.flatten() {
            let (key, value) = item;
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
            if let Ok(delta) = bincode::deserialize::<ClusterDailyDelta>(&value) {
                results.push((date, delta));
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
        match self.get_cf(self.cf_stats(), &key)? {
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
        self.put_cf(self.cf_stats(), &key, &value)
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
            self.cf_stats(),
            rocksdb::IteratorMode::From(&start_key, rocksdb::Direction::Forward),
        );
        let mut results = Vec::new();

        for item in iter.flatten() {
            let (key, value) = item;
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
            if let Ok(delta) = bincode::deserialize::<SporeDailyDelta>(&value) {
                results.push((date, delta));
            }
        }

        Ok(results)
    }

    pub fn get_nft_daily_delta(
        &self,
        collection_id: &[u8],
        date_yyyymmdd: u32,
    ) -> anyhow::Result<Option<NftDailyDelta>> {
        let key = keys::encode_nft_daily_key(collection_id, date_yyyymmdd);
        match self.get_cf(self.cf_stats(), &key)? {
            Some(value) => Ok(Some(bincode::deserialize(&value)?)),
            None => Ok(None),
        }
    }

    pub fn put_nft_daily_delta(
        &self,
        collection_id: &[u8],
        date_yyyymmdd: u32,
        delta: &NftDailyDelta,
    ) -> anyhow::Result<()> {
        let key = keys::encode_nft_daily_key(collection_id, date_yyyymmdd);
        let value = bincode::serialize(delta)?;
        self.put_cf(self.cf_stats(), &key, &value)
    }

    pub fn list_nft_daily_deltas(
        &self,
        collection_id: &[u8],
    ) -> anyhow::Result<Vec<(u32, NftDailyDelta)>> {
        self.list_nft_daily_deltas_in_range(collection_id, None, None)
    }

    pub fn list_nft_daily_deltas_in_range(
        &self,
        collection_id: &[u8],
        from_date_yyyymmdd: Option<u32>,
        to_date_yyyymmdd: Option<u32>,
    ) -> anyhow::Result<Vec<(u32, NftDailyDelta)>> {
        let prefix = keys::encode_nft_daily_prefix(collection_id);
        let start_key =
            keys::encode_nft_daily_key(collection_id, from_date_yyyymmdd.unwrap_or(u32::MIN));
        let iter = self.iterator_cf(
            self.cf_stats(),
            rocksdb::IteratorMode::From(&start_key, rocksdb::Direction::Forward),
        );
        let mut results = Vec::new();

        for item in iter.flatten() {
            let (key, value) = item;
            if !key.starts_with(&prefix) {
                break;
            }
            if key.len() != keys::NFT_DAILY_KEY_SIZE {
                continue;
            }
            let (_, date) = keys::decode_nft_daily_key(&key);
            if let Some(to_date) = to_date_yyyymmdd {
                if date > to_date {
                    break;
                }
            }
            if let Ok(delta) = bincode::deserialize::<NftDailyDelta>(&value) {
                results.push((date, delta));
            }
        }

        Ok(results)
    }

    pub fn get_nft(&self, id: &[u8]) -> anyhow::Result<Option<NftEntry>> {
        match self.get_cf(self.cf_nft_data(), id)? {
            Some(value) => Ok(Some(bincode::deserialize(&value)?)),
            None => Ok(None),
        }
    }

    pub fn put_nft_direct(&self, id: &[u8], entry: &NftEntry) -> anyhow::Result<()> {
        let value = bincode::serialize(entry)?;
        self.put_cf(self.cf_nft_data(), id, &value)
    }

    /// Backfill the spore-by-cluster secondary index from existing spore data.
    #[allow(clippy::manual_is_multiple_of)]
    /// Gated by a marker key in sync_meta to ensure it only runs once.
    pub fn migrate_spore_by_cluster_index(&self) -> anyhow::Result<u64> {
        let marker = b"migration:spore_by_cluster";
        if self.get_cf(self.cf_sync_meta(), marker)?.is_some() {
            return Ok(0); // Already migrated
        }

        let spores = self.list_spores(1_000_000)?;
        let mut count = 0u64;
        let mut batch = StoreBatch::new(self);

        for (spore_id, entry) in &spores {
            if entry.standard.is_cluster() {
                continue;
            }
            if let Some(ref cluster_id) = entry.collection_id {
                if cluster_id.len() >= 32 && spore_id.len() >= 32 {
                    batch.put_spore_by_cluster(cluster_id, spore_id);
                    count += 1;

                    // Commit in chunks to avoid huge batches
                    if count % 10_000 == 0 {
                        batch.commit()?;
                        batch = StoreBatch::new(self);
                    }
                }
            }
        }

        // Write migration marker
        batch.put_sync_meta(marker, b"done");
        batch.commit()?;

        Ok(count)
    }

    /// Get pre-aggregated data for an NFT collection.
    pub fn get_nft_collection_aggregate(
        &self,
        collection_id: &[u8],
    ) -> anyhow::Result<Option<NftCollectionAggregate>> {
        match self.get_cf(self.cf_nft_collection_agg(), collection_id)? {
            Some(value) => Ok(Some(bincode::deserialize(&value)?)),
            None => Ok(None),
        }
    }

    /// List all NFT collection aggregates. Scans the small `nft_collection_agg` CF.
    pub fn list_nft_collection_aggregates(
        &self,
    ) -> anyhow::Result<Vec<(Vec<u8>, NftCollectionAggregate)>> {
        let iter = self.iterator_cf(self.cf_nft_collection_agg(), rocksdb::IteratorMode::Start);
        let mut results = Vec::new();

        for item in iter.flatten() {
            let (key, value) = item;
            if let Ok(agg) = bincode::deserialize::<NftCollectionAggregate>(&value) {
                results.push((key.to_vec(), agg));
            }
        }
        Ok(results)
    }

    /// List all NFTs.
    pub fn list_nfts(&self, limit: usize) -> anyhow::Result<Vec<(Vec<u8>, NftEntry)>> {
        let iter = self.iterator_cf(self.cf_nft_data(), rocksdb::IteratorMode::Start);
        let mut results = Vec::new();

        for item in iter.flatten() {
            let (key, value) = item;
            if let Ok(entry) = bincode::deserialize::<NftEntry>(&value) {
                results.push((key.to_vec(), entry));
                if results.len() >= limit {
                    break;
                }
            }
        }
        Ok(results)
    }

    /// List NFT IDs in a collection via the `nft_by_collection` secondary index.
    ///
    /// Pagination is keyset-based by `nft_id` lexicographic order.
    /// - `cursor = None` starts from the beginning.
    /// - `cursor = Some(id)` starts AFTER that id.
    pub fn list_nft_ids_by_collection(
        &self,
        collection_id: &[u8],
        cursor: Option<&[u8]>,
        limit: usize,
    ) -> anyhow::Result<Vec<Vec<u8>>> {
        if limit == 0 {
            return Ok(Vec::new());
        }

        let prefix = keys::encode_nft_by_collection_prefix(collection_id);
        let start_nft_id = cursor.unwrap_or(&[]);
        let start_key = keys::encode_nft_by_collection_key(collection_id, start_nft_id);

        let iter = self.iterator_cf(
            self.cf_nft_by_collection(),
            rocksdb::IteratorMode::From(&start_key, rocksdb::Direction::Forward),
        );
        let mut results = Vec::new();

        for item in iter.flatten() {
            let (key, _) = item;
            if !key.starts_with(&prefix) {
                break;
            }

            if cursor.is_some() && key.as_ref() == start_key.as_slice() {
                continue;
            }

            let Some((_, nft_id)) = keys::decode_nft_by_collection_key(&key) else {
                anyhow::bail!("invalid nft_by_collection key length: {}", key.len());
            };
            if nft_id.is_empty() {
                anyhow::bail!("invalid empty nft_id in nft_by_collection key");
            }

            results.push(nft_id);
            if results.len() >= limit {
                break;
            }
        }

        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{
        ClusterDailyDelta, NftDailyDelta, NftStandard, NftTypeIndex, SporeDailyDelta,
        SporeTypeIndex,
    };
    use tempfile::TempDir;

    fn test_store() -> (TempDir, CkbadgerStore) {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open(dir.path()).unwrap();
        (dir, store)
    }

    #[test]
    fn test_get_nft_collection_aggregate_missing() {
        let (_dir, store) = test_store();
        let id = [0x01u8; 32];
        assert!(store.get_nft_collection_aggregate(&id).unwrap().is_none());
    }

    #[test]
    fn test_put_and_get_nft_collection_aggregate() {
        let (_dir, store) = test_store();
        let id = [0x01u8; 32];
        let agg = NftCollectionAggregate {
            name: Some("Test mNFT Class".to_string()),
            standard: NftStandard::MnftClass,
            total_count: 50,
            live_count: 42,
        };

        let mut batch = StoreBatch::new(&store);
        batch.put_nft_collection_aggregate(&id, &agg);
        batch.commit().unwrap();

        let result = store.get_nft_collection_aggregate(&id).unwrap().unwrap();
        assert_eq!(result.name.as_deref(), Some("Test mNFT Class"));
        assert_eq!(result.standard, NftStandard::MnftClass);
        assert_eq!(result.total_count, 50);
        assert_eq!(result.live_count, 42);
    }

    #[test]
    fn test_list_nft_collection_aggregates() {
        let (_dir, store) = test_store();
        let id_a = [0x01u8; 32];
        let id_b = [0x02u8; 32];

        let mut batch = StoreBatch::new(&store);
        batch.put_nft_collection_aggregate(
            &id_a,
            &NftCollectionAggregate {
                name: Some("Class A".to_string()),
                standard: NftStandard::MnftClass,
                total_count: 10,
                live_count: 8,
            },
        );
        batch.put_nft_collection_aggregate(
            &id_b,
            &NftCollectionAggregate {
                name: Some("DotBit".to_string()),
                standard: NftStandard::DotBit,
                total_count: 100,
                live_count: 90,
            },
        );
        batch.commit().unwrap();

        let results = store.list_nft_collection_aggregates().unwrap();
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_nft_collection_aggregate_default() {
        let agg = NftCollectionAggregate::default();
        assert!(agg.name.is_none());
        assert_eq!(agg.standard, NftStandard::MnftClass);
        assert_eq!(agg.total_count, 0);
        assert_eq!(agg.live_count, 0);
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
        let results = store.get_spore_ids_by_outpoints_batch(&outpoints);
        assert_eq!(results.len(), 2);
        assert!(results
            .iter()
            .any(|(tx, idx, id)| tx == tx_a.as_slice() && *idx == 1 && id == spore_a.as_slice()));
        assert!(results
            .iter()
            .any(|(tx, idx, id)| tx == tx_b.as_slice() && *idx == 2 && id == spore_b.as_slice()));
    }

    #[test]
    fn test_mnft_and_dotbit_outpoint_roundtrip_and_batch_lookup() {
        let (_dir, store) = test_store();
        let tx_a = [0xC1u8; 32];
        let tx_b = [0xC2u8; 32];
        let mnft_class_id = [0x31u8; 24];
        let mnft_token_id = [0x41u8; 28];
        let dotbit_account_id = [0x51u8; 20];

        let mut batch = StoreBatch::new(&store);
        batch.put_mnft_class_outpoint(&tx_a, 3, &mnft_class_id);
        batch.put_mnft_token_outpoint(&tx_a, 4, &mnft_token_id);
        batch.put_dotbit_account_outpoint(&tx_b, 5, &dotbit_account_id);
        batch.commit().unwrap();

        let class_id = store
            .get_mnft_class_id_by_outpoint(&tx_a, 3)
            .unwrap()
            .unwrap();
        let token_id = store
            .get_mnft_token_id_by_outpoint(&tx_a, 4)
            .unwrap()
            .unwrap();
        let dotbit_id = store
            .get_dotbit_account_id_by_outpoint(&tx_b, 5)
            .unwrap()
            .unwrap();
        assert_eq!(class_id, mnft_class_id.to_vec());
        assert_eq!(token_id, mnft_token_id.to_vec());
        assert_eq!(dotbit_id, dotbit_account_id.to_vec());

        let mnft_outpoints: Vec<(&[u8], i16)> = vec![(&tx_a, 4), (&tx_a, 9)];
        let mnft_results = store.get_mnft_token_ids_by_outpoints_batch(&mnft_outpoints);
        assert_eq!(mnft_results.len(), 1);
        assert_eq!(mnft_results[0].0, tx_a.to_vec());
        assert_eq!(mnft_results[0].1, 4);
        assert_eq!(mnft_results[0].2, mnft_token_id.to_vec());

        let dotbit_outpoints: Vec<(&[u8], i16)> = vec![(&tx_b, 5), (&tx_b, 8)];
        let dotbit_results = store.get_dotbit_account_ids_by_outpoints_batch(&dotbit_outpoints);
        assert_eq!(dotbit_results.len(), 1);
        assert_eq!(dotbit_results[0].0, tx_b.to_vec());
        assert_eq!(dotbit_results[0].1, 5);
        assert_eq!(dotbit_results[0].2, dotbit_account_id.to_vec());
    }

    #[test]
    fn test_get_live_dotbit_outpoints_by_account_ids_prefers_live_cells() {
        let (_dir, store) = test_store();
        let account_id = vec![0x61u8; 20];
        let old_tx = vec![0x71u8; 32];
        let live_tx = vec![0x72u8; 32];
        let old_idx = 1i16;
        let live_idx = 2i16;

        let mut batch = StoreBatch::new(&store);
        // Historical outpoint (no live cell now)
        batch.put_dotbit_account_outpoint(&old_tx, old_idx, &account_id);
        // Current outpoint with a live cell
        batch.put_dotbit_account_outpoint(&live_tx, live_idx, &account_id);
        batch.put_cell(
            &live_tx,
            live_idx,
            &crate::types::LiveCellInfo {
                capacity: 100_00000000,
                created_at_block: 10,
                lock_script_hash: vec![0x01; 32],
                lock_code_hash: vec![0x02; 32],
                lock_hash_type: 1,
                lock_args: vec![],
                type_script_hash: Some(vec![0x03; 32]),
                type_code_hash: Some(vec![0x04; 32]),
                type_args: Some(account_id.clone()),
                data_size: 0,
                occupied_capacity: 61_00000000,
                udt_amount: None,
            },
        );
        batch.commit().unwrap();

        let outpoints = store
            .get_live_dotbit_outpoints_by_account_ids(std::slice::from_ref(&account_id))
            .unwrap();
        let (tx_hash, output_index) = outpoints.get(&account_id).unwrap();
        assert_eq!(tx_hash, &live_tx);
        assert_eq!(*output_index, live_idx);
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
                    live_capacity_delta: 1000,
                    live_occupied_capacity_delta: 600,
                },
            )
            .unwrap();
        store
            .put_spore_daily_delta(
                &spore_id,
                20260219,
                &SporeDailyDelta {
                    live_capacity_delta: 100,
                    live_occupied_capacity_delta: 61,
                },
            )
            .unwrap();

        let cluster = store
            .get_cluster_daily_delta(&cluster_id, 20260219)
            .unwrap()
            .unwrap();
        assert_eq!(cluster.live_capacity_delta, 1000);
        assert_eq!(cluster.live_occupied_capacity_delta, 600);

        let spore = store
            .get_spore_daily_delta(&spore_id, 20260219)
            .unwrap()
            .unwrap();
        assert_eq!(spore.live_capacity_delta, 100);
        assert_eq!(spore.live_occupied_capacity_delta, 61);

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

    #[test]
    fn test_nft_type_index_and_nft_daily_delta_roundtrip() {
        let (_dir, store) = test_store();
        let type_script_hash = [0x66u8; 32];
        let collection_id = [0x77u8; 24];

        store
            .put_nft_type_index_direct(
                &type_script_hash,
                &NftTypeIndex {
                    collection_id: collection_id.to_vec(),
                },
            )
            .unwrap();
        let loaded_index = store
            .get_nft_type_index(&type_script_hash)
            .unwrap()
            .unwrap();
        assert_eq!(loaded_index.collection_id, collection_id.to_vec());

        store
            .put_nft_daily_delta(
                &collection_id,
                20260219,
                &NftDailyDelta {
                    live_capacity_delta: 500,
                    live_occupied_capacity_delta: 320,
                },
            )
            .unwrap();
        let loaded_daily = store
            .get_nft_daily_delta(&collection_id, 20260219)
            .unwrap()
            .unwrap();
        assert_eq!(loaded_daily.live_capacity_delta, 500);
        assert_eq!(loaded_daily.live_occupied_capacity_delta, 320);

        let list = store.list_nft_daily_deltas(&collection_id).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].0, 20260219);

        let ranged = store
            .list_nft_daily_deltas_in_range(&collection_id, Some(20260219), Some(20260219))
            .unwrap();
        assert_eq!(ranged.len(), 1);
        assert_eq!(ranged[0].0, 20260219);
    }

    #[test]
    fn test_list_nft_ids_by_collection_pagination() {
        let (_dir, store) = test_store();
        let collection_id = [0x88u8; 24];
        let nft_a = [0x01u8; 20];
        let nft_b = [0x02u8; 20];
        let nft_c = [0x03u8; 20];

        let mut batch = StoreBatch::new(&store);
        batch.put_nft_by_collection(&collection_id, &nft_b);
        batch.put_nft_by_collection(&collection_id, &nft_c);
        batch.put_nft_by_collection(&collection_id, &nft_a);
        batch.commit().unwrap();

        let first = store
            .list_nft_ids_by_collection(&collection_id, None, 2)
            .unwrap();
        assert_eq!(first.len(), 2);
        assert_eq!(first[0], nft_a.to_vec());
        assert_eq!(first[1], nft_b.to_vec());

        let second = store
            .list_nft_ids_by_collection(&collection_id, Some(&first[1]), 2)
            .unwrap();
        assert_eq!(second, vec![nft_c.to_vec()]);
    }
}
