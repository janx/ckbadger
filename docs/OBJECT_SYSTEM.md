# Object System

ckbadger indexes two on-chain NFT standards: **Spore** and **m-NFT**. Spore objects can optionally use the **DOB** (Digital Object Blueprint) protocol to encode generative content via on-chain decoders.

## Standards

| Standard      | Storage Key     | Content                              | Decode                       |
| ------------- | --------------- | ------------------------------------ | ---------------------------- |
| Spore         | `CF_SPORE_DATA` | Raw cell data (image, text, DOB DNA) | None for raw; CKB-VM for DOB |
| Spore Cluster | `CF_SPORE_DATA` | Cluster name + description (JSON)    | N/A (metadata container)     |
| m-NFT Token   | `CF_MNFT_DATA`  | Token metadata                       | N/A                          |
| m-NFT Class   | `CF_MNFT_DATA`  | Class template                       | N/A                          |
| m-NFT Issuer  | `CF_MNFT_DATA`  | Issuer authority                     | N/A                          |

Each object is stored as an `ObjectEntry` with standard-specific data in `ObjectExtra`.

## Spore Content Types

A spore's `content_type` (MIME) determines how its cell data is interpreted:

| Content Type                    | Frontend Standard | Behavior                                |
| ------------------------------- | ----------------- | --------------------------------------- |
| `image/png`, `image/jpeg`, etc. | `plain-image`     | Raw bytes displayed as image            |
| `image/svg+xml`                 | `plain-svg`       | Raw SVG in cell data                    |
| `text/plain`, `text/html`       | `plain-text`      | Raw text in cell data                   |
| `dob/0`                         | `dob/0`           | DNA decoded by single decoder           |
| `dob/1`                         | `dob/1`           | DNA decoded by multi-step decoder chain |
| other                           | `generic`         | Hex view only                           |

## DOB Protocol

DOB spores store a short DNA hex string as cell content. The DNA is decoded by on-chain decoder binaries specified in the parent cluster's description JSON.

### DOB/0: Single Decoder

```
Cluster Description:
{
  "dob": {
    "ver": 0,
    "decoder": { "type": "code_hash", "hash": "0x..." },
    "pattern": [["TraitName", "Type", offset, length, "patternType", args], ...]
  }
}

Execution:
  decoder(DNA, pattern) → DobTraitGroup[] JSON
```

One decoder binary, one execution, one output. The pattern defines how DNA bytes map to trait values (options, range, utf8, raw).

### DOB/1: Decoder Chain

```
Cluster Description:
{
  "dob": {
    "ver": 1,
    "decoders": [
      { "decoder": { "type": "code_hash", "hash": "0x..." }, "pattern": [...] },
      { "decoder": { "type": "code_hash", "hash": "0x..." }, "pattern": [...] }
    ]
  }
}

Execution:
  decoder_0(DNA, pattern_0)          → output_0
  decoder_1(DNA, pattern_1, output_0) → output_1
```

Multiple decoders execute sequentially. Each receives the original DNA, its own pattern, and the previous decoder's output. Typical chain: decoder 0 extracts traits from DNA, decoder 1 renders SVG from those traits.

### Decoder Output

Each decoder's output is either:

- **JSON trait groups**: `[{"name":"Background","traits":[{"String":"Blue"}]}]` — parsed into traits
- **Media content**: SVG, HTML, or binary — stored as-is

Both forms are preserved. The decode worker stores each step's raw output independently.

## Data Flow

```
┌─────────────────────────────────────────────────────────┐
│ 1. Sync (Indexer)                                       │
│                                                         │
│   Parse tx outputs → detect Spore cells by code_hash    │
│   Extract: spore_id, content_type, content, cluster_id  │
│   Analyze media profile (composition tier, sources)     │
│   Write ObjectEntry → CF_SPORE_DATA                     │
└───────────────────────────┬─────────────────────────────┘
                            │ dob/* content types
┌───────────────────────────▼─────────────────────────────┐
│ 2. DOB Decode (Background Worker)                       │
│                                                         │
│   Poll undecoded DOB spores from CF_SPORE_DATA          │
│   Load cluster description → parse decoder refs         │
│   Fetch decoder binaries from chain (cached on disk)    │
│   Execute decoder chain in CKB-VM                       │
│   Store each step's raw output → dob_decode/{hash}      │
│   Parse traits from JSON outputs                        │
│   Write DobDecodedEntry → CF_DOB_DECODED                │
└───────────────────────────┬─────────────────────────────┘
                            │
┌───────────────────────────▼─────────────────────────────┐
│ 3. API (Read-only)                                      │
│                                                         │
│   GET /spore/objects/{id}        → SporeResponse        │
│   GET /spore/objects/{id}/decode → merged traits +      │
│                                    per-step media       │
│   GET /spore/objects/{id}/media/{hash} → raw blob       │
│   GET /spore/objects/{id}/render → SVG on-the-fly       │
└───────────────────────────┬─────────────────────────────┘
                            │
┌───────────────────────────▼─────────────────────────────┐
│ 4. Frontend                                             │
│                                                         │
│   Classify standard from content_type                   │
│   Filter displayable traits per standard                │
│   Build media composition view with per-step labels     │
│   Render preview from decoded media or raw cell bytes   │
└─────────────────────────────────────────────────────────┘
```

## Storage Layout

### RocksDB (Domain Store)

| CF                       | Key                       | Value                      | Purpose                       |
| ------------------------ | ------------------------- | -------------------------- | ----------------------------- |
| `CF_SPORE_DATA`          | spore_id (32B)            | ObjectEntry (postcard)     | Spore + cluster entries       |
| `CF_SPORE_BY_CLUSTER`    | cluster_id + spore_id     | empty                      | Index: spores in cluster      |
| `CF_DOB_DECODED`         | spore_id (32B)            | DobDecodedEntry (postcard) | Cached decode results         |
| `CF_MNFT_DATA`           | object_id                 | ObjectEntry (postcard)     | m-NFT entries                 |
| `CF_MNFT_BY_COLLECTION`  | collection_id + object_id | empty                      | Index: mNFTs in collection    |
| `CF_MNFT_COLLECTION_AGG` | collection_id             | aggregate (postcard)       | Pre-computed collection stats |

### Filesystem (DOB Decode Blobs)

```
{workdir}/dob_decode/{collection_8hex}/{blake2b_hex}
```

Each decoder step's raw output is stored as a content-addressed blob. Writes are atomic (temp file + rename). Content-addressed by blake2b hash of the output bytes.

## DobDecodedEntry Schema

```
DobDecodedEntry
├── steps: Vec<DobDecodedStep>     // one per decoder in chain
│   ├── step: u32                  // 0-indexed position
│   ├── media_type: String         // sniffed MIME (application/json, image/svg+xml, ...)
│   ├── size: u64                  // raw output byte count
│   ├── hash: String               // blake2b hex → blob filename
│   └── traits: Vec<DobDecodedTrait>  // parsed if output is valid JSON
├── media_sources: Vec<SporeMediaSource>  // URIs found in trait values
└── decoded_at: i64                // timestamp
```

Each step's raw output is preserved independently. The API merges traits across steps for display (later steps override same-name traits from earlier steps).

## Media Composition

Every spore has a `SporeMediaProfile` computed during indexing:

- **Tier**: PureCkb | BtcCkb | DecentralizedMixture | CentralizedMixture | Unknown
- **Sources**: extracted URIs with scheme and dependency tier
- **Issues**: parsing errors or validation problems

Tier represents the storage dependency of the spore's content — fully on-chain (PureCkb) vs depending on external infrastructure (IPFS, HTTP, etc.).

## API Decode Response

The `/decode` endpoint assembles data from per-step storage:

1. **Traits**: merged from all steps in order. Step 0 traits form the base; later steps can override same-name traits but don't erase earlier unique traits.
2. **Media**: one entry per decoder step, each with its MIME type, size, hash, and URL.
3. **Render**: if any step's traits contain `<svg` markup, or the cluster has DOB/1 SVG patterns, a render URL is added.

## Frontend Per-Standard Behavior

| Standard      | Trait Filter           | Media Labels                                           | Preview Source                   |
| ------------- | ---------------------- | ------------------------------------------------------ | -------------------------------- |
| `plain-image` | N/A (no traits)        | N/A                                                    | Raw cell bytes (base64)          |
| `plain-svg`   | N/A                    | N/A                                                    | Raw SVG from cell data           |
| `plain-text`  | N/A                    | N/A                                                    | None                             |
| `dob/0`       | All traits shown       | "Decoded Output", "SVG Render"                         | Decoded media or render endpoint |
| `dob/1`       | Filter SVG/image blobs | "Decoder Chain Output" (with step count), "SVG Render" | Decoded media or render endpoint |
| `generic`     | N/A                    | N/A                                                    | None                             |

## Key Files

| Component         | File                                             |
| ----------------- | ------------------------------------------------ |
| Store types       | `crates/ckbadger-store/src/types.rs`             |
| Spore parser      | `crates/indexer/src/parser/spore.rs`             |
| Media analysis    | `crates/indexer/src/parser/media_source.rs`      |
| DOB decoder       | `crates/dob-decoder/src/lib.rs`                  |
| Decode worker     | `crates/indexer/src/sync/dob_decode_worker.rs`   |
| Blob store        | `crates/indexer/src/media_store.rs`              |
| API routes        | `crates/api/src/routes/spore.rs`                 |
| Frontend standard | `frontend/lib/object-standard.ts`                |
| Frontend page     | `frontend/app/objects/[sporeId]/client-page.tsx` |
