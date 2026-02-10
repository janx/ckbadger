//! Token operations.

use crate::keys;
use crate::store::CkbadgerStore;
use crate::types::TokenInfo;

impl CkbadgerStore {
    pub fn get_token(&self, type_hash: &[u8]) -> anyhow::Result<Option<TokenInfo>> {
        match self.get_cf(self.cf_tokens(), type_hash)? {
            Some(value) => Ok(Some(bincode::deserialize(&value)?)),
            None => Ok(None),
        }
    }

    pub fn put_token_direct(&self, type_hash: &[u8], info: &TokenInfo) -> anyhow::Result<()> {
        let value = bincode::serialize(info)?;
        self.put_cf(self.cf_tokens(), type_hash, &value)
    }

    /// List all tokens.
    pub fn list_tokens(&self) -> anyhow::Result<Vec<(Vec<u8>, TokenInfo)>> {
        let iter = self.iterator_cf(self.cf_tokens(), rocksdb::IteratorMode::Start);
        let mut results = Vec::new();

        for item in iter.flatten() {
            let (key, value) = item;
            if let Ok(info) = bincode::deserialize::<TokenInfo>(&value) {
                results.push((key.to_vec(), info));
            }
        }
        Ok(results)
    }

    /// Get token holder balance.
    pub fn get_token_holder_balance(
        &self,
        type_hash: &[u8],
        lock_hash: &[u8],
    ) -> anyhow::Result<Option<i128>> {
        let key = keys::encode_token_holder_key(type_hash, lock_hash);
        match self.get_cf(self.cf_token_holders(), &key)? {
            Some(value) if value.len() == 16 => {
                Ok(Some(i128::from_le_bytes(value[..16].try_into().unwrap())))
            }
            _ => Ok(None),
        }
    }

    /// List holders for a token (prefix scan by type_hash).
    pub fn list_token_holders(
        &self,
        type_hash: &[u8],
        limit: usize,
    ) -> anyhow::Result<Vec<(Vec<u8>, i128)>> {
        let iter = self.prefix_iterator_cf(self.cf_token_holders(), type_hash);
        let mut results = Vec::new();

        for item in iter.flatten() {
            let (key, value) = item;
            if !key.starts_with(type_hash) {
                break;
            }
            // Key: type_hash(32) + lock_hash(32) = 64
            if key.len() == 64 && value.len() == 16 {
                let lock_hash = key[32..64].to_vec();
                let balance = i128::from_le_bytes(value[..16].try_into().unwrap());
                results.push((lock_hash, balance));
                if results.len() >= limit {
                    break;
                }
            }
        }
        Ok(results)
    }
}
