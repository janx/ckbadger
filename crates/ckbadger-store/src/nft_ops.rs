//! NFT (Spore, mNFT, DotBit) operations.

use crate::batch::StoreBatch;
use crate::store::CkbadgerStore;
use crate::types::{NftEntry, SporeEntry};

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
}
