# Decoded Media Blob Storage Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace inline `svg_markup` storage in `DobDecodedEntry` with content-addressed filesystem blobs, supporting multiple decoded media per spore and future binary formats.

**Architecture:** Decoded media written as flat files under `<work_dir>/media/<collection_8hex>/<blake2b_hash>`. `DobDecodedEntry` stores only metadata (`DecodedMedia` structs). New API endpoint serves blobs with correct Content-Type. Frontend fetches media via URL instead of inline data.

**Tech Stack:** Rust (ckb-hash blake2b, tokio fs), Axum 0.8 (streaming response), React/TanStack Query (media URL fetching)

**Spec:** `docs/superpowers/specs/2026-03-23-decoded-media-blob-storage-design.md`

---

### Task 1: Store Types — Add DecodedMedia, Update DobDecodedEntry

**Files:**
- Modify: `crates/ckbadger-store/src/types.rs:354-369`

- [ ] **Step 1: Add DecodedMedia struct and update DobDecodedEntry**

In `crates/ckbadger-store/src/types.rs`, add `DecodedMedia` after `SporeMediaSource` (after line 337) and replace `svg_markup` in `DobDecodedEntry`:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DecodedMedia {
    pub media_type: String,
    pub role: Option<String>,
    pub size: u64,
    pub hash: String,
    pub step: Option<u32>,
}
```

Update `DobDecodedEntry` (lines 354-363):

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DobDecodedEntry {
    pub traits: Vec<DobDecodedTrait>,
    #[serde(default)]
    pub media: Vec<DecodedMedia>,
    pub media_sources: Vec<SporeMediaSource>,
    pub decoded_at: i64,
}
```

- [ ] **Step 2: Run cargo check to verify**

Run: `cargo check -p ckbadger-store`
Expected: Compilation errors in downstream crates referencing `svg_markup` — that's expected, we'll fix those in later tasks.

- [ ] **Step 3: Commit**

```bash
git add crates/ckbadger-store/src/types.rs
git commit -m "feat(store): replace svg_markup with DecodedMedia in DobDecodedEntry"
```

---

### Task 2: Media Blob Store

**Files:**
- Create: `crates/indexer/src/media_store.rs`
- Modify: `crates/indexer/src/lib.rs` (add module)

- [ ] **Step 1: Write tests for MediaBlobStore**

Create `crates/indexer/src/media_store.rs` with the test module first:

```rust
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

/// Content-addressed blob store for decoded media files.
///
/// Layout: `<root>/media/<collection_8hex>/<blake2b_hex>`
pub struct MediaBlobStore {
    media_dir: PathBuf,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_write_and_read_blob() {
        let dir = tempfile::tempdir().unwrap();
        let store = MediaBlobStore::new(dir.path().join("media"));
        let collection_id = vec![0xAB; 32];
        let content = b"<svg><circle r='10'/></svg>";

        let hash = store.write(&collection_id, content).unwrap();
        assert!(!hash.is_empty());

        let read_back = store.read(&collection_id, &hash).unwrap();
        assert_eq!(read_back, content);
    }

    #[test]
    fn test_content_addressed_dedup() {
        let dir = tempfile::tempdir().unwrap();
        let store = MediaBlobStore::new(dir.path().join("media"));
        let collection_id = vec![0xAB; 32];
        let content = b"same content";

        let hash1 = store.write(&collection_id, content).unwrap();
        let hash2 = store.write(&collection_id, content).unwrap();
        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_collection_short_hash() {
        assert_eq!(
            MediaBlobStore::collection_dir_name(&[0xAB, 0xCD, 0xEF, 0x01, 0x23]),
            "abcdef01"
        );
    }

    #[test]
    fn test_read_nonexistent_blob_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let store = MediaBlobStore::new(dir.path().join("media"));
        let collection_id = vec![0xAB; 32];
        let result = store.read(&collection_id, "nonexistent");
        assert!(result.is_err());
    }

    #[test]
    fn test_blob_path() {
        let store = MediaBlobStore::new(PathBuf::from("/data/media"));
        let collection_id = vec![0xAB, 0xCD, 0xEF, 0x01, 0x99, 0x99];
        let path = store.blob_path(&collection_id, "deadbeef");
        assert_eq!(path, PathBuf::from("/data/media/abcdef01/deadbeef"));
    }
}
```

- [ ] **Step 2: Implement MediaBlobStore**

Add the implementation above the `#[cfg(test)]` module in the same file:

```rust
impl MediaBlobStore {
    pub fn new(media_dir: PathBuf) -> Self {
        Self { media_dir }
    }

    /// Write a blob and return its content hash.
    /// Skips writing if an identical blob already exists.
    pub fn write(&self, collection_id: &[u8], content: &[u8]) -> Result<String> {
        let hash = Self::content_hash(content);
        let path = self.blob_path(collection_id, &hash);

        if path.exists() {
            return Ok(hash);
        }

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).with_context(|| {
                format!("failed to create media directory: {}", parent.display())
            })?;
        }

        // Write atomically via temp file to avoid partial reads
        let tmp_path = path.with_extension("tmp");
        std::fs::write(&tmp_path, content).with_context(|| {
            format!("failed to write media blob: {}", tmp_path.display())
        })?;
        std::fs::rename(&tmp_path, &path).with_context(|| {
            format!("failed to rename media blob: {} -> {}", tmp_path.display(), path.display())
        })?;

        Ok(hash)
    }

    /// Read a blob by its content hash.
    pub fn read(&self, collection_id: &[u8], hash: &str) -> Result<Vec<u8>> {
        let path = self.blob_path(collection_id, hash);
        std::fs::read(&path).with_context(|| {
            format!("failed to read media blob: {}", path.display())
        })
    }

    pub fn blob_path(&self, collection_id: &[u8], hash: &str) -> PathBuf {
        self.media_dir
            .join(Self::collection_dir_name(collection_id))
            .join(hash)
    }

    pub fn collection_dir_name(collection_id: &[u8]) -> String {
        hex::encode(&collection_id[..4.min(collection_id.len())])
    }

    fn content_hash(content: &[u8]) -> String {
        use ckb_hash::new_blake2b;
        let mut hasher = new_blake2b();
        hasher.update(content);
        let mut hash = [0u8; 32];
        hasher.finalize(&mut hash);
        hex::encode(hash)
    }
}
```

- [ ] **Step 3: Register module in lib.rs**

In `crates/indexer/src/lib.rs`, add:

```rust
pub mod media_store;
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p ckbadger-indexer media_store -- --nocapture`
Expected: All 5 tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/indexer/src/media_store.rs crates/indexer/src/lib.rs
git commit -m "feat(indexer): add MediaBlobStore for content-addressed media files"
```

---

### Task 3: Media Type Sniffing

**Files:**
- Modify: `crates/indexer/src/media_store.rs` (add sniff function)

- [ ] **Step 1: Write sniff tests**

Add to the `tests` module in `media_store.rs`:

```rust
#[test]
fn test_sniff_svg() {
    assert_eq!(
        sniff_media_type(b"<svg xmlns='http://www.w3.org/2000/svg'><circle/></svg>"),
        "image/svg+xml"
    );
}

#[test]
fn test_sniff_svg_with_leading_whitespace() {
    assert_eq!(
        sniff_media_type(b"  \n<svg><rect/></svg>"),
        "image/svg+xml"
    );
}

#[test]
fn test_sniff_json_array() {
    assert_eq!(
        sniff_media_type(b"[{\"name\":\"bg\",\"value\":\"blue\"}]"),
        "application/json"
    );
}

#[test]
fn test_sniff_json_object() {
    assert_eq!(
        sniff_media_type(b"{\"traits\":[]}"),
        "application/json"
    );
}

#[test]
fn test_sniff_plain_text() {
    assert_eq!(
        sniff_media_type(b"hello world"),
        "text/plain"
    );
}

#[test]
fn test_sniff_empty() {
    assert_eq!(
        sniff_media_type(b""),
        "application/octet-stream"
    );
}
```

- [ ] **Step 2: Implement sniff_media_type**

Add as a public function in `media_store.rs`:

```rust
/// Infer MIME type from content bytes.
/// Currently handles text-based formats (SVG, JSON).
/// Future: magic byte detection for binary formats.
pub fn sniff_media_type(content: &[u8]) -> &'static str {
    if content.is_empty() {
        return "application/octet-stream";
    }

    // Try to interpret as UTF-8 text for text-based detection
    let text = match std::str::from_utf8(content) {
        Ok(s) => s,
        Err(_) => return "application/octet-stream",
    };

    let trimmed = text.trim_start();

    if trimmed.starts_with("<svg") || trimmed.starts_with("<SVG") {
        return "image/svg+xml";
    }

    if (trimmed.starts_with('[') && trimmed.ends_with(']'))
        || (trimmed.starts_with('{') && trimmed.ends_with('}'))
    {
        if serde_json::from_str::<serde_json::Value>(trimmed).is_ok() {
            return "application/json";
        }
    }

    "text/plain"
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p ckbadger-indexer media_store -- --nocapture`
Expected: All tests pass (previous 5 + new 6 = 11).

- [ ] **Step 4: Commit**

```bash
git add crates/indexer/src/media_store.rs
git commit -m "feat(indexer): add media type sniffing for decoded content"
```

---

### Task 4: Config — Add media_dir to WorkDir

**Files:**
- Modify: `crates/config/src/lib.rs:160-223`

- [ ] **Step 1: Add media_dir field to WorkDir**

In `crates/config/src/lib.rs`, add `media_dir` field to the `WorkDir` struct (after `perf_dir`):

```rust
pub media_dir: PathBuf,
```

In `WorkDir::resolve()`, add the path derivation (alongside other path computations):

```rust
media_dir: root.join("media"),
```

- [ ] **Step 2: Run cargo check**

Run: `cargo check -p ckbadger-config`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add crates/config/src/lib.rs
git commit -m "feat(config): add media_dir to WorkDir"
```

---

### Task 5: Decode Worker — Write Blobs Instead of Inline SVG

**Files:**
- Modify: `crates/indexer/src/sync/dob_decode_worker.rs:37-52` (add media_store field)
- Modify: `crates/indexer/src/sync/dob_decode_worker.rs:432-528` (decode_single_spore)
- Modify: `crates/indexer/src/sync/dob_decode_worker.rs:193-216` (batch write section)
- Modify: `crates/indexer/src/sync/dob_decode_worker.rs:268-313` (update_spore_media_profile — update has_renderable_image logic)
- Modify: `crates/indexer/src/sync/indexer.rs` (pass media_dir when constructing DobDecodeWorker)

- [ ] **Step 1: Add MediaBlobStore to DobDecodeWorker and DecodeContext**

In the `DobDecodeWorker` struct, add:

```rust
media_store: Arc<MediaBlobStore>,
```

Update `DobDecodeWorker::new()` to accept and store `media_dir: PathBuf`, constructing `MediaBlobStore::new(media_dir)`.

Add `media_store: Arc<MediaBlobStore>` to `DecodeContext` struct as well.

- [ ] **Step 2: Update decode_single_spore to produce DecodedMedia**

Replace the SVG detection block (lines 512-527) with:

```rust
use crate::media_store::{sniff_media_type, MediaBlobStore};
use ckbadger_store::types::DecodedMedia;

// Extract media sources from decoded trait values
let media_sources = extract_media_sources_from_traits(&decoded.traits);

// Store raw output as a media blob
let raw_bytes = decoded.raw_output.as_bytes();
let media_type = sniff_media_type(raw_bytes);
let collection_id = collection_id.unwrap_or(&[] as &[u8]);
let hash = ctx.media_store.write(collection_id, raw_bytes)?;

let mut media = vec![DecodedMedia {
    media_type: media_type.to_string(),
    role: None,
    size: raw_bytes.len() as u64,
    hash,
    step: Some(0),
}];

// For DOB/1 chains, intermediate outputs are stored at earlier steps.
// Currently only the final output is captured by the decoder;
// intermediate step storage will be added when decode_dob1_chain
// exposes per-step outputs.

Ok(DobDecodedEntry {
    traits,
    media,
    media_sources,
    decoded_at: chrono::Utc::now().timestamp(),
})
```

- [ ] **Step 3: Update has_renderable_image logic in update_spore_media_profile**

In the `run()` method where `has_renderable_image` is set (around line 202), change from:

```rust
let has_renderable_image = entry.svg_markup.is_some();
```

to:

```rust
let has_renderable_image = entry.media.iter().any(|m| {
    m.media_type.starts_with("image/")
});
```

- [ ] **Step 4: Update indexer.rs to pass media_dir**

In `crates/indexer/src/sync/indexer.rs`, where `DobDecodeWorker::new()` is called, pass `work_dir.media_dir.clone()` (or equivalent path) as the new parameter.

- [ ] **Step 5: Run cargo check**

Run: `cargo check -p ckbadger-indexer`
Expected: PASS (non-test build)

- [ ] **Step 6: Commit**

```bash
git add crates/indexer/src/sync/dob_decode_worker.rs crates/indexer/src/sync/indexer.rs
git commit -m "feat(indexer): write decoded media as filesystem blobs"
```

---

### Task 6: API — Update Decode Endpoint, Add Media Serving

**Files:**
- Modify: `crates/api/src/routes/spore.rs:267-276` (SporeDobDecodeResponse)
- Modify: `crates/api/src/routes/spore.rs:1095-1156` (decode_spore handler)
- Modify: `crates/api/src/routes/spore.rs:23-51` (routes registration)
- Modify: `crates/api/src/lib.rs` (add media_dir to AppState)

- [ ] **Step 1: Add media_dir to AppState**

In `crates/api/src/lib.rs`, add to `AppState`:

```rust
pub media_dir: PathBuf,
```

Populate it from config/work_dir when constructing AppState.

- [ ] **Step 2: Add DecodedMediaResponse and update SporeDobDecodeResponse**

In `crates/api/src/routes/spore.rs`, add:

```rust
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DecodedMediaResponse {
    pub media_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    pub size: u64,
    pub hash: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub step: Option<u32>,
    pub url: String,
}
```

Update `SporeDobDecodeResponse`:
- Remove: `svg_markup: Option<String>`
- Add: `media: Vec<DecodedMediaResponse>`

- [ ] **Step 3: Update decode_spore handler**

Replace the `svg_markup` mapping with media URL construction:

```rust
let media = entry.media.iter().map(|m| DecodedMediaResponse {
    media_type: m.media_type.clone(),
    role: m.role.clone(),
    size: m.size,
    hash: m.hash.clone(),
    step: m.step,
    url: format!("/spore/objects/0x{}/media/{}", hex::encode(&id), m.hash),
}).collect();
```

- [ ] **Step 4: Add serve_media handler**

```rust
async fn serve_media(
    State(state): State<Arc<AppState>>,
    Path((spore_id, hash)): Path<(String, String)>,
) -> Result<axum::response::Response, ApiError> {
    let id = hex::decode(spore_id.strip_prefix("0x").unwrap_or(&spore_id))
        .map_err(|_| ApiError::bad_request("Invalid spore ID"))?;

    // Look up spore to get collection_id for directory resolution
    let store = state.store.clone();
    let id_c = id.clone();
    let entry = tokio::task::spawn_blocking(move || store.get_dob_decoded(&id_c))
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .map_err(|e| ApiError::internal(e.to_string()))?
        .ok_or_else(|| ApiError::not_found("Decoded data not found"))?;

    // Validate hash belongs to this spore
    let media_entry = entry
        .media
        .iter()
        .find(|m| m.hash == hash)
        .ok_or_else(|| ApiError::not_found("Media not found"))?;

    // Get collection_id from spore entry for directory path
    let store = state.store.clone();
    let id_c = id.clone();
    let spore_entry = tokio::task::spawn_blocking(move || store.get_spore(&id_c))
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .map_err(|e| ApiError::internal(e.to_string()))?
        .ok_or_else(|| ApiError::not_found("Spore not found"))?;

    let collection_id = spore_entry.collection_id.as_deref().unwrap_or(&[]);
    let media_store = ckbadger_indexer::media_store::MediaBlobStore::new(
        state.media_dir.clone(),
    );

    let content_type = media_entry.media_type.clone();
    let blob = tokio::task::spawn_blocking(move || {
        media_store.read(collection_id, &hash)
    })
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?
    .map_err(|e| ApiError::internal(e.to_string()))?;

    Ok(axum::response::Response::builder()
        .header("content-type", content_type)
        .header("cache-control", "public, max-age=31536000, immutable")
        .body(axum::body::Body::from(blob))
        .unwrap())
}
```

- [ ] **Step 5: Register the route**

In `routes()`, add:

```rust
.route("/spore/objects/{spore_id}/media/{hash}", get(serve_media))
```

- [ ] **Step 6: Run cargo check**

Run: `cargo check -p ckbadger-api`
Expected: PASS

- [ ] **Step 7: Commit**

```bash
git add crates/api/src/routes/spore.rs crates/api/src/lib.rs
git commit -m "feat(api): add media blob serving endpoint, update decode response"
```

---

### Task 7: Frontend Types and API

**Files:**
- Modify: `frontend/lib/api.ts:991-1003` (types)
- Modify: `frontend/lib/api.ts:1990-1992` (method)

- [ ] **Step 1: Update types**

Replace `SporeDobDecoded` interface:

```typescript
interface DecodedMediaItem {
  mediaType: string;
  role: string | null;
  size: number;
  hash: string;
  step: number | null;
  url: string;
}

interface SporeDobDecoded {
  status: string;
  sporeId: string;
  contentType: string;
  dnaHex: string | null;
  traits: DobTrait[];
  media: DecodedMediaItem[];
  issues: string[];
}
```

Export `DecodedMediaItem`.

- [ ] **Step 2: Commit**

```bash
git add frontend/lib/api.ts
git commit -m "feat(frontend): update SporeDobDecoded types for media blobs"
```

---

### Task 8: Frontend Preview Detection

**Files:**
- Modify: `frontend/lib/preview-utils.ts:11-14,61-102`
- Modify: `frontend/lib/dob-render.ts:32-37`

- [ ] **Step 1: Update PreviewKind and detectPreview**

Add a new preview kind for media URLs and update `detectPreview` signature:

```typescript
export type PreviewKind =
  | { type: 'image'; dataUrl: string }
  | { type: 'svg'; markup: string }
  | { type: 'media-url'; url: string; mediaType: string }
  | null;
```

Update `detectPreview` to accept media items instead of `dobSvgMarkup`:

```typescript
export function detectPreview(
  contentType: string,
  contentBytes: Uint8Array | undefined,
  dobMedia: Array<{ mediaType: string; url: string; step: number | null }> | undefined
): PreviewKind {
  // DOB decoded media — find final product (highest step)
  if (dobMedia && dobMedia.length > 0) {
    const sorted = [...dobMedia].sort((a, b) => (b.step ?? 0) - (a.step ?? 0));
    const primary = sorted[0];
    if (primary.mediaType === 'image/svg+xml') {
      return { type: 'media-url', url: primary.url, mediaType: primary.mediaType };
    }
    if (primary.mediaType.startsWith('image/')) {
      return { type: 'media-url', url: primary.url, mediaType: primary.mediaType };
    }
    // Non-image media: no preview
  }

  if (!contentBytes || contentBytes.length === 0) {
    return null;
  }

  const mime = baseMimeType(contentType);

  if (isBinaryImageType(mime) && contentBytes.length <= MAX_IMAGE_BYTES) {
    const base64 = bytesToBase64(contentBytes);
    return { type: 'image', dataUrl: `data:${mime};base64,${base64}` };
  }

  if (isSvgType(mime) && contentBytes.length <= MAX_SVG_BYTES) {
    const text = new TextDecoder('utf-8', { fatal: false }).decode(contentBytes);
    const svg = extractSvgFromText(text);
    if (svg) {
      return { type: 'svg', markup: svg };
    }
  }

  if (isTextLikeForPreview(mime) && contentBytes.length <= MAX_SVG_BYTES) {
    const text = new TextDecoder('utf-8', { fatal: false }).decode(contentBytes);
    const svg = extractSvgFromText(text);
    if (svg) {
      return { type: 'svg', markup: svg };
    }
  }

  return null;
}
```

- [ ] **Step 2: Update DobDecodedContent in dob-render.ts**

Remove `svgMarkup` from the interface:

```typescript
export interface DobDecodedContent {
  dnaHex: string | null;
  traits: DobTrait[];
  issues: string[];
}
```

Update `decodeDobContent` return to remove `svgMarkup`.

- [ ] **Step 3: Commit**

```bash
git add frontend/lib/preview-utils.ts frontend/lib/dob-render.ts
git commit -m "feat(frontend): update preview detection for media URL blobs"
```

---

### Task 9: Frontend SporePreview Component

**Files:**
- Modify: `frontend/components/object/spore-preview.tsx`

- [ ] **Step 1: Handle media-url preview kind**

Add rendering for the new `media-url` type alongside existing `image` and `svg` cases. For `image/svg+xml`, fetch the SVG content and render in a sandboxed iframe. For other image types, render as `<img src={url}>`.

```typescript
// Inside PreviewContent or equivalent:
if (preview.type === 'media-url') {
  if (preview.mediaType.startsWith('image/') && preview.mediaType !== 'image/svg+xml') {
    return (
      <img
        src={apiBase + preview.url}
        alt="Decoded media"
        className="max-h-80 max-w-full rounded object-contain"
      />
    );
  }
  if (preview.mediaType === 'image/svg+xml') {
    // Fetch SVG and render in sandbox iframe, same as existing SVG preview
    // Use a useEffect + fetch pattern or a dedicated SvgMediaPreview component
  }
  return null;
}
```

- [ ] **Step 2: Commit**

```bash
git add frontend/components/object/spore-preview.tsx
git commit -m "feat(frontend): render media-url previews in SporePreview"
```

---

### Task 10: Frontend Spore Detail Page — Wire Everything

**Files:**
- Modify: `frontend/app/objects/[sporeId]/client-page.tsx:157-162,285-307,1047-1101,1305-1370`

- [ ] **Step 1: Update dobContent computation**

Change the `dobContent` memo (lines 285-302) to use `media` instead of `svgMarkup`:

```typescript
const dobContent = useMemo(() => {
  if (decodedDobByApi) {
    return {
      dnaHex: decodedDobByApi.dnaHex,
      traits: decodedDobByApi.traits,
      media: decodedDobByApi.media ?? [],
      issues: decodedDobByApi.issues,
    };
  }
  if (!spore) {
    return null;
  }
  const local = decodeDobContent({
    sporeContentType: spore.contentType,
    contentText: sporePayload?.textContent,
    clusterDescription: cluster?.description,
  });
  return local ? { ...local, media: [] } : null;
}, [cluster?.description, decodedDobByApi, spore, sporePayload?.textContent]);
```

- [ ] **Step 2: Update preview detection call**

Change the `detectPreview` call (around line 303-307):

```typescript
const preview = useMemo(
  () =>
    detectPreview(
      spore?.contentType ?? '',
      sporePayload?.contentBytes,
      dobContent?.media?.map((m) => ({ mediaType: m.mediaType, url: m.url, step: m.step }))
    ),
  [spore?.contentType, sporePayload?.contentBytes, dobContent?.media]
);
```

- [ ] **Step 3: Update On-Chain Media panel**

In the side-by-side On-Chain Media panel (around lines 1075-1098), replace the inline SVG/text payload display with a media list:

```typescript
{dobContent?.media && dobContent.media.length > 0 && (
  <div className="space-y-2">
    {dobContent.media.map((m) => (
      <div key={m.hash} className="border-base-border bg-base-surface/50 rounded border p-2.5">
        <div className="flex items-baseline gap-2">
          <span className="text-text-dim font-mono text-[10px] uppercase tracking-wider">
            {m.role ?? `Step ${m.step ?? 0}`}
          </span>
          <span className="text-text font-mono text-[10px]">
            {m.mediaType} · {formatNumber(m.size)} bytes
          </span>
        </div>
      </div>
    ))}
  </div>
)}
```

- [ ] **Step 4: Update Media Compositions panel**

In the Media Compositions panel (around lines 1344-1367), replace the inline SVG/payload snippet with media entries:

```typescript
{dobContent?.media && dobContent.media.length > 0 && (
  <div className="space-y-1.5">
    {dobContent.media.map((m) => (
      <div key={m.hash} className="flex items-baseline gap-2">
        <span className="bg-base-elevated text-text-dim inline-block rounded px-1.5 py-0.5 font-mono text-[10px] uppercase">
          {m.mediaType.split('/')[1] ?? m.mediaType}
        </span>
        <span className="text-text font-mono text-[10px]">
          {formatNumber(m.size)} bytes
        </span>
        <span className="text-text-dim font-mono text-[10px]">
          {m.role ?? `step ${m.step ?? 0}`}
        </span>
      </div>
    ))}
  </div>
)}
```

- [ ] **Step 5: Run type-check and lint**

Run: `cd frontend && pnpm type-check && pnpm lint`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add frontend/app/objects/[sporeId]/client-page.tsx
git commit -m "feat(frontend): wire media blobs into spore detail page"
```

---

### Task 11: Migration — Clear CF_DOB_DECODED

**Files:**
- No code changes needed

- [ ] **Step 1: Document in commit message**

After all code changes are merged, the user needs to clear `CF_DOB_DECODED` to trigger re-decode. This happens automatically if the DB is rebuilt, or can be done manually. No migration code needed per spec (development project, not production).

- [ ] **Step 2: Final cargo check and frontend build**

Run: `cargo check && cd frontend && pnpm type-check && pnpm lint`
Expected: PASS

- [ ] **Step 3: Commit any remaining fixups**

```bash
git add -A
git commit -m "chore: final fixups for decoded media blob storage"
```
