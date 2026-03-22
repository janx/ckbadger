//! Decoder binary disk cache.
//!
//! Provides a two-tier cache (in-memory HashMap + on-disk files) for RISC-V
//! decoder binaries fetched from the CKB chain. Cache keys are derived from
//! the decoder's code_hash or type_id.

use anyhow::{Context, Result};
use tracing::info;

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// Disk-backed cache for decoder RISC-V binaries with in-memory promotion.
///
/// On first access, binaries are fetched from the chain and written to both
/// disk and memory. Subsequent accesses serve from memory. If the process
/// restarts, disk hits are promoted back into the in-memory map.
pub struct DecoderBinaryCache {
    cache_dir: PathBuf,
    memory: Mutex<HashMap<String, Arc<Vec<u8>>>>,
}

impl DecoderBinaryCache {
    /// Create a new cache backed by the given directory.
    ///
    /// Creates the directory (and parents) if it does not exist.
    pub fn new(cache_dir: &Path) -> Result<Self> {
        fs::create_dir_all(cache_dir)
            .with_context(|| format!("failed to create cache dir: {}", cache_dir.display()))?;

        Ok(Self {
            cache_dir: cache_dir.to_path_buf(),
            memory: Mutex::new(HashMap::new()),
        })
    }

    /// Look up a cached decoder binary by key.
    ///
    /// Checks the in-memory map first, then falls back to disk. On a disk
    /// hit the binary is promoted into memory for subsequent lookups.
    pub fn get(&self, key: &str) -> Option<Arc<Vec<u8>>> {
        // Fast path: in-memory hit
        {
            let mem = self.memory.lock().unwrap();
            if let Some(data) = mem.get(key) {
                return Some(Arc::clone(data));
            }
        }

        // Slow path: disk hit — promote to memory
        let path = self.disk_path(key);
        if let Ok(data) = fs::read(&path) {
            let arc = Arc::new(data);
            let mut mem = self.memory.lock().unwrap();
            mem.insert(key.to_string(), Arc::clone(&arc));
            Some(arc)
        } else {
            None
        }
    }

    /// Store a decoder binary in both disk and memory.
    pub fn put(&self, key: &str, binary: &[u8]) -> Result<()> {
        let path = self.disk_path(key);
        fs::write(&path, binary)
            .with_context(|| format!("failed to write cache file: {}", path.display()))?;

        let mut mem = self.memory.lock().unwrap();
        mem.insert(key.to_string(), Arc::new(binary.to_vec()));

        info!(key, bytes = binary.len(), "cached decoder binary");
        Ok(())
    }

    /// Compute the on-disk path for a cache key.
    fn disk_path(&self, key: &str) -> PathBuf {
        self.cache_dir.join(format!("{key}.bin"))
    }

    /// Build a cache key for a code_hash-referenced decoder.
    pub fn code_hash_key(hash: &[u8]) -> String {
        format!("code_hash_{}", hex::encode(hash))
    }

    /// Build a cache key for a type_id-referenced decoder.
    pub fn type_id_key(hash: &[u8]) -> String {
        format!("type_id_{}", hex::encode(hash))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_put_and_get_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let cache = DecoderBinaryCache::new(dir.path()).unwrap();

        let key = "test_binary";
        let data = vec![0xDE, 0xAD, 0xBE, 0xEF, 1, 2, 3, 4];

        assert!(cache.get(key).is_none());

        cache.put(key, &data).unwrap();

        let retrieved = cache.get(key).unwrap();
        assert_eq!(*retrieved, data);
    }

    #[test]
    fn test_disk_persistence_across_instances() {
        let dir = tempfile::tempdir().unwrap();
        let key = "persistent_binary";
        let data = vec![10, 20, 30, 40, 50];

        // Write with first cache instance
        {
            let cache = DecoderBinaryCache::new(dir.path()).unwrap();
            cache.put(key, &data).unwrap();
        }

        // Read with a new cache instance (empty memory, disk should have it)
        {
            let cache = DecoderBinaryCache::new(dir.path()).unwrap();
            let retrieved = cache.get(key).unwrap();
            assert_eq!(*retrieved, data);
        }
    }

    #[test]
    fn test_code_hash_key_format() {
        let hash = vec![0xAB, 0xCD, 0xEF];
        assert_eq!(DecoderBinaryCache::code_hash_key(&hash), "code_hash_abcdef");
    }

    #[test]
    fn test_type_id_key_format() {
        let hash = vec![0x01, 0x02, 0x03];
        assert_eq!(DecoderBinaryCache::type_id_key(&hash), "type_id_010203");
    }

    #[test]
    fn test_get_missing_key_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let cache = DecoderBinaryCache::new(dir.path()).unwrap();
        assert!(cache.get("nonexistent").is_none());
    }
}
