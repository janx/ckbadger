//! Content-addressed blob store for decoded media files.
//!
//! DOB (Digital Object Blueprint) spore decoding via CKB-VM produces media
//! outputs (SVG, JSON, future binary formats). Rather than storing these inline
//! in RocksDB, we store them as content-addressed blobs on the filesystem.
//!
//! Layout: `<root>/media/<collection_8hex>/<blake2b_hex>`
//!
//! Writes are atomic (temp file + rename) to avoid partial reads.

use anyhow::{Context, Result};

use std::fs;
use std::path::PathBuf;

/// Filesystem-backed content-addressed blob store for decoded media.
pub struct MediaBlobStore {
    media_dir: PathBuf,
}

impl MediaBlobStore {
    /// Create a new blob store rooted at `media_dir`.
    pub fn new(media_dir: PathBuf) -> Self {
        Self { media_dir }
    }

    /// Write a blob for the given collection. Returns the content hash (hex).
    ///
    /// If an identical blob already exists (same hash), the write is skipped.
    /// Writes are atomic: content is written to a temp file first, then renamed
    /// into place to prevent partial reads.
    pub fn write(&self, collection_id: &[u8], content: &[u8]) -> Result<String> {
        let hash = Self::content_hash(content);
        let path = self.blob_path(collection_id, &hash);

        // Dedup: skip if identical blob already exists
        if path.exists() {
            return Ok(hash);
        }

        // Ensure collection directory exists
        let dir = path
            .parent()
            .expect("blob_path always has a parent directory");
        fs::create_dir_all(dir)
            .with_context(|| format!("failed to create collection dir: {}", dir.display()))?;

        // Atomic write: temp file in the same directory, then rename
        let temp_path = dir.join(format!(".tmp_{hash}"));
        fs::write(&temp_path, content)
            .with_context(|| format!("failed to write temp blob: {}", temp_path.display()))?;
        fs::rename(&temp_path, &path).with_context(|| {
            format!(
                "failed to rename temp blob {} -> {}",
                temp_path.display(),
                path.display()
            )
        })?;

        Ok(hash)
    }

    /// Read a blob by its content hash within a collection.
    pub fn read(&self, collection_id: &[u8], hash: &str) -> Result<Vec<u8>> {
        let path = self.blob_path(collection_id, hash);
        fs::read(&path).with_context(|| format!("failed to read blob: {}", path.display()))
    }

    /// Compute the full filesystem path for a blob.
    pub fn blob_path(&self, collection_id: &[u8], hash: &str) -> PathBuf {
        let collection_dir = Self::collection_dir_name(collection_id);
        self.media_dir.join(collection_dir).join(hash)
    }

    /// Derive the collection subdirectory name: first 8 hex chars (4 bytes) of
    /// the collection ID.
    pub fn collection_dir_name(collection_id: &[u8]) -> String {
        let take = collection_id.len().min(4);
        hex::encode(&collection_id[..take])
    }

    /// Compute the blake2b content hash of the given bytes, returned as hex.
    ///
    /// Uses `ckb_hash::new_blake2b()` with CKB personalization.
    pub fn content_hash(content: &[u8]) -> String {
        let mut hasher = ckb_hash::new_blake2b();
        hasher.update(content);
        let mut hash = [0u8; 32];
        hasher.finalize(&mut hash);
        hex::encode(hash)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_write_and_read_blob() {
        let dir = tempfile::tempdir().unwrap();
        let store = MediaBlobStore::new(dir.path().join("media"));

        let collection_id = b"\xab\xcd\xef\x01\x23\x45\x67\x89";
        let content = b"<svg>hello world</svg>";

        let hash = store.write(collection_id, content).unwrap();
        let read_back = store.read(collection_id, &hash).unwrap();

        assert_eq!(read_back, content);
    }

    #[test]
    fn test_content_addressed_dedup() {
        let dir = tempfile::tempdir().unwrap();
        let store = MediaBlobStore::new(dir.path().join("media"));

        let collection_id = b"\x01\x02\x03\x04";
        let content = b"identical content";

        let hash1 = store.write(collection_id, content).unwrap();
        let hash2 = store.write(collection_id, content).unwrap();

        assert_eq!(hash1, hash2, "same content must produce same hash");
    }

    #[test]
    fn test_collection_short_hash() {
        // Exactly 4 bytes -> 8 hex chars
        assert_eq!(
            MediaBlobStore::collection_dir_name(&[0xAB, 0xCD, 0xEF, 0x01]),
            "abcdef01"
        );

        // Longer than 4 bytes -> still first 8 hex chars
        assert_eq!(
            MediaBlobStore::collection_dir_name(&[0xAB, 0xCD, 0xEF, 0x01, 0xFF, 0x99]),
            "abcdef01"
        );

        // Shorter than 4 bytes -> uses what's available
        assert_eq!(MediaBlobStore::collection_dir_name(&[0xAB, 0xCD]), "abcd");
    }

    #[test]
    fn test_read_nonexistent_blob_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let store = MediaBlobStore::new(dir.path().join("media"));

        let result = store.read(b"\x01\x02\x03\x04", "deadbeef");
        assert!(result.is_err(), "reading a nonexistent blob must fail");
    }

    #[test]
    fn test_blob_path() {
        let store = MediaBlobStore::new(PathBuf::from("/data/media"));

        let collection_id = b"\xab\xcd\xef\x01\x23";
        let hash = "cafebabe";

        let path = store.blob_path(collection_id, hash);
        assert_eq!(path, PathBuf::from("/data/media/abcdef01/cafebabe"));
    }
}
