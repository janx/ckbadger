//! NFT (Spore, mNFT, DotBit) operations.

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
