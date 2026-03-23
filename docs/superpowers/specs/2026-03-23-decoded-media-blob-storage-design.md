# Decoded Media Blob Storage Design

## Problem

`DobDecodedEntry.svg_markup: Option<String>` is too narrow:

1. Only captures SVG, ignoring other media formats (GIF, JPG, GLSL)
2. Large payloads stored inline in RocksDB bloat the domain store
3. DOB decode chains can produce multiple media outputs (intermediate + final), all currently discarded except SVG

## Design

Store decoded media as content-addressed blobs on the filesystem. DB entries hold only lightweight metadata referencing the blobs.

### Storage Types

```rust
pub struct DobDecodedEntry {
    pub traits: Vec<DobDecodedTrait>,
    pub media: Vec<DecodedMedia>,           // replaces svg_markup
    pub media_sources: Vec<SporeMediaSource>,
    pub decoded_at: i64,
}

pub struct DecodedMedia {
    pub media_type: String,     // MIME type, sniffed by ckbadger
    pub role: Option<String>,   // semantic role (future: decoder-provided)
    pub size: u64,              // byte count
    pub hash: String,           // blake2b content hash, also the filename
    pub step: Option<u32>,      // decode chain step index; final product = max
}
```

### Filesystem Layout

```
<work_dir>/media/<collection_short_hash>/<blob_hash>
```

- `collection_short_hash`: first 8 hex chars (4 bytes) of the collection/cluster ID. Sole spores use the sentinel collection ID.
- `blob_hash`: blake2b hash of the blob content (hex). No file extension.
- Flat within each collection directory. Content-addressed: identical blobs across spores stored once.

### Decode Worker Write Flow

```
CKB-VM decode (per step in chain)
  → raw_output (String)
  → sniff media type (detect <svg, JSON structure, future: magic bytes)
  → blake2b(raw_output bytes) → hash
  → write blob to media/<collection_8hex>/<hash>
  → build DecodedMedia { media_type, size, hash, step, role: None }

After all steps:
  → DobDecodedEntry { traits, media: vec![...], media_sources, decoded_at }
  → put_dob_decoded to CF_DOB_DECODED (metadata only, no inline blob)
```

For DOB/1 decode chains, each step's intermediate output goes through the same sniff → hash → write → DecodedMedia pipeline. The final product is the highest step index.

### Media Type Detection

ckbadger sniffs the raw output to infer MIME type:

- Contains `<svg` → `image/svg+xml`
- Valid JSON array of trait groups → `application/json`
- Future: magic byte detection for binary formats (when CKB-VM supports binary output)

Role assignment is `None` for now. Future decoders may provide structured output with explicit role metadata.

### API

**New endpoint:**

```
GET /spore/objects/{spore_id}/media/{hash}
```

- Validates `hash` exists in the spore's `DobDecodedEntry.media` list
- Reads blob from `media/<collection_8hex>/<hash>`
- Returns binary body with `Content-Type` from `DecodedMedia.media_type`

**Modified endpoint:**

```
GET /spore/objects/{spore_id}/decode
```

Response changes:
- Remove: `svg_markup: Option<String>`
- Add: `media: Vec<DecodedMediaResponse>`

```rust
pub struct DecodedMediaResponse {
    pub media_type: String,
    pub role: Option<String>,
    pub size: u64,
    pub hash: String,
    pub step: Option<u32>,
    pub url: String,  // "/spore/objects/{spore_id}/media/{hash}"
}
```

### Frontend

**Type changes:**
- `SporeDobDecoded`: remove `svgMarkup`, add `media: DecodedMediaItem[]`

**Preview detection:**
- Find the final product (highest `step`) from `media` list
- `image/svg+xml` → fetch from media URL, render in iframe sandbox
- `image/*` → `<img src={url}>`
- Other → no preview, show metadata only in Media Compositions panel

**On-Chain Media panel (left side, paired with DOB traits):**
- SporePreview using fetched media
- Meta info (content type, size)

**Media Compositions panel:**
- On-chain section: list all decoded media entries (type, size, role, step)
- Off-chain section: existing URI source list (unchanged)

### Migration

Development-only project. No migration code needed.

1. `DobDecodedEntry` schema is a breaking change — old entries won't deserialize
2. Clear `CF_DOB_DECODED` (or delete DB and re-sync)
3. DOB decode worker re-runs, populates `media/` directory and new-schema entries
4. `media/` directory starts empty, fills as decode proceeds

### Config

No new config fields. Uses existing `work_dir` root to derive `<work_dir>/media/` path, same pattern as `<work_dir>/data/decoder-cache/`.
