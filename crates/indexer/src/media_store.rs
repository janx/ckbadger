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

/// Infer MIME type from content bytes.
///
/// Detection rules (evaluated in order):
/// 1. Empty content -> `"application/octet-stream"`
/// 2. Not valid UTF-8 -> `"application/octet-stream"`
/// 3. Known binary magic bytes (PNG, JPEG, GIF, WebP) -> image MIME
/// 4. Trimmed text starts with `<svg` (case-insensitive) -> `"image/svg+xml"`
/// 5. Trimmed text starts with `<!DOCTYPE html` / `<html` -> `"text/html"`
/// 6. Trimmed text starts with `<?xml` -> `"application/xml"`
/// 7. JSON-shaped text that parses as valid JSON -> `"application/json"`
/// 8. Everything else -> `"text/plain"`
pub fn sniff_media_type(content: &[u8]) -> &'static str {
    if content.is_empty() {
        return "application/octet-stream";
    }

    // Binary magic bytes — checked BEFORE UTF-8 attempt.
    if content.starts_with(&[0x89, b'P', b'N', b'G']) {
        return "image/png";
    }
    if content.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return "image/jpeg";
    }
    if content.starts_with(b"GIF87a") || content.starts_with(b"GIF89a") {
        return "image/gif";
    }
    if content.len() >= 12 && &content[0..4] == b"RIFF" && &content[8..12] == b"WEBP" {
        return "image/webp";
    }

    let text = match std::str::from_utf8(content) {
        Ok(s) => s,
        Err(_) => return "application/octet-stream",
    };

    let trimmed = text.trim();
    let lower = trimmed.as_bytes();

    // SVG (case-insensitive: handles <svg, <SVG, <Svg, etc.)
    if trimmed.len() >= 4 {
        let svg_prefix: String = trimmed.chars().take(4).collect::<String>().to_lowercase();
        if svg_prefix == "<svg" {
            return "image/svg+xml";
        }
    }

    // HTML
    if lower.len() >= 15 {
        let prefix_lower: String = trimmed.chars().take(15).collect::<String>().to_lowercase();
        if prefix_lower.starts_with("<!doctype html") || prefix_lower.starts_with("<html") {
            return "text/html";
        }
    }

    // XML (non-SVG, non-HTML)
    if trimmed.starts_with("<?xml") {
        return "application/xml";
    }

    // JSON
    let looks_like_json = (trimmed.starts_with('[') && trimmed.ends_with(']'))
        || (trimmed.starts_with('{') && trimmed.ends_with('}'));

    if looks_like_json && serde_json::from_str::<serde_json::Value>(trimmed).is_ok() {
        return "application/json";
    }

    "text/plain"
}

/// Maximum blob size (10 MiB). Decoder outputs exceeding this are likely bugs,
/// not valid media — CKB-VM decoded content should never be this large.
const MAX_BLOB_SIZE: usize = 10 * 1024 * 1024;

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
        if content.len() > MAX_BLOB_SIZE {
            anyhow::bail!(
                "media blob exceeds maximum size: {} bytes > {} bytes limit, collection=0x{}",
                content.len(),
                MAX_BLOB_SIZE,
                hex::encode(&collection_id[..collection_id.len().min(4)])
            );
        }

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

        // Atomic write: temp file in the same directory, then rename.
        // Include thread ID to avoid collisions when concurrent workers produce
        // identical content for the same collection (same hash = same temp name).
        let tid = std::thread::current().id();
        let temp_path = dir.join(format!(".tmp_{hash}_{tid:?}"));
        fs::write(&temp_path, content)
            .with_context(|| format!("failed to write temp blob: {}", temp_path.display()))?;
        match fs::rename(&temp_path, &path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound && path.exists() => {
                // Another writer already placed the identical blob — dedup race, not an error.
            }
            Err(e) => {
                return Err(anyhow::Error::new(e).context(format!(
                    "failed to rename temp blob {} -> {}",
                    temp_path.display(),
                    path.display()
                )));
            }
        }

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

    #[test]
    fn test_sniff_svg() {
        let content = b"<svg xmlns=\"http://www.w3.org/2000/svg\"><circle/></svg>";
        assert_eq!(sniff_media_type(content), "image/svg+xml");
    }

    #[test]
    fn test_sniff_svg_with_leading_whitespace() {
        let content = b"  \n  <svg><rect/></svg>";
        assert_eq!(sniff_media_type(content), "image/svg+xml");
    }

    #[test]
    fn test_sniff_svg_mixed_case() {
        assert_eq!(
            sniff_media_type(b"<Svg xmlns=\"http://www.w3.org/2000/svg\"/>"),
            "image/svg+xml"
        );
        assert_eq!(sniff_media_type(b"<SVG><rect/></SVG>"), "image/svg+xml");
        assert_eq!(
            sniff_media_type(b"<sVg viewBox=\"0 0 100 100\"/>"),
            "image/svg+xml"
        );
    }

    #[test]
    fn test_sniff_json_array() {
        let content = b"[{\"trait\":\"color\",\"value\":\"red\"}]";
        assert_eq!(sniff_media_type(content), "application/json");
    }

    #[test]
    fn test_sniff_json_object() {
        let content = b"{\"name\":\"spore\",\"traits\":[]}";
        assert_eq!(sniff_media_type(content), "application/json");
    }

    #[test]
    fn test_sniff_plain_text() {
        let content = b"hello world, this is just text";
        assert_eq!(sniff_media_type(content), "text/plain");
    }

    #[test]
    fn test_sniff_empty() {
        assert_eq!(sniff_media_type(b""), "application/octet-stream");
    }

    #[test]
    fn test_sniff_html_doctype() {
        let content = b"<!DOCTYPE html><html><body>hello</body></html>";
        assert_eq!(sniff_media_type(content), "text/html");
    }

    #[test]
    fn test_sniff_html_tag() {
        let content = b"<html lang=\"en\"><head></head><body></body></html>";
        assert_eq!(sniff_media_type(content), "text/html");
    }

    #[test]
    fn test_sniff_xml() {
        let content = b"<?xml version=\"1.0\" encoding=\"UTF-8\"?><root/>";
        assert_eq!(sniff_media_type(content), "application/xml");
    }

    #[test]
    fn test_sniff_png_magic() {
        let content = &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        assert_eq!(sniff_media_type(content), "image/png");
    }

    #[test]
    fn test_sniff_jpeg_magic() {
        let content = &[0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10];
        assert_eq!(sniff_media_type(content), "image/jpeg");
    }

    #[test]
    fn test_sniff_gif_magic() {
        let content = b"GIF89a\x01\x00\x01\x00";
        assert_eq!(sniff_media_type(content), "image/gif");
    }

    #[test]
    fn test_sniff_webp_magic() {
        let mut content = vec![0u8; 12];
        content[..4].copy_from_slice(b"RIFF");
        content[8..12].copy_from_slice(b"WEBP");
        assert_eq!(sniff_media_type(&content), "image/webp");
    }

    #[test]
    fn test_sniff_non_utf8_binary() {
        let content = &[0x00, 0xFF, 0xFE, 0x80, 0x90];
        assert_eq!(sniff_media_type(content), "application/octet-stream");
    }
}
