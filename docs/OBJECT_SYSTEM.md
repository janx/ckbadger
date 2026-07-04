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
│   Write DecodeOutcome → CF_DOB_DECODED                  │
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

| CF                       | Key                       | Value                   | Purpose                       |
| ------------------------ | ------------------------- | ----------------------- | ----------------------------- |
| `CF_SPORE_DATA`          | spore_id (32B)            | ObjectEntry (bincode)   | Spore + cluster entries       |
| `CF_SPORE_BY_CLUSTER`    | cluster_id + spore_id     | empty                   | Index: spores in cluster      |
| `CF_DOB_DECODED`         | spore_id (32B)            | DecodeOutcome (bincode) | Cached decode outcome         |
| `CF_MNFT_DATA`           | object_id                 | ObjectEntry (bincode)   | m-NFT entries                 |
| `CF_MNFT_BY_COLLECTION`  | collection_id + object_id | empty                   | Index: mNFTs in collection    |
| `CF_MNFT_COLLECTION_AGG` | collection_id             | aggregate (bincode)     | Pre-computed collection stats |

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

The `/decode` endpoint reports a `status` for the spore, read from its single `CF_DOB_DECODED` outcome:

- **`decoded`** — decode succeeded; `traits` and `media` are populated as below.
- **`failed`** — decode was attempted and deterministically failed; `issues` carries the human-readable reason and `traits`/`media` are empty (see [Decode Failure Handling](#decode-failure-handling)).
- **`pending`** — the background worker has not produced an outcome yet (not-yet-run, or the last attempt failed transiently); it will be retried.

For a `decoded` spore the endpoint assembles data from per-step storage:

1. **Traits**: merged from all steps in order. Step 0 traits form the base; later steps can override same-name traits but don't erase earlier unique traits.
2. **Media**: one entry per decoder step, each with its MIME type, size, hash, and URL.
3. **Render**: if any step's traits contain `<svg` markup, or the cluster has DOB/1 SVG patterns, a render URL is added.

## Decode Failure Handling

`CF_DOB_DECODED` stores a `DecodeOutcome` per spore — either `Decoded(DobDecodedEntry)` or `Failed(DobDecodeFailure)` — so one lookup answers "what happened to this spore's decode." This keeps a single authoritative read path (no parallel success/failure sources) and stops the worker from re-attempting permanently-undecodable spores on every run.

The decode worker classifies each failure (typed `DobDecodeError` in `crates/indexer/src/sync/dob_decode_error.rs`) as **transient** or **deterministic**:

- **Transient** (RPC/node fetch of the spore cell or decoder binary, internal IO): never persisted. The spore stays undecoded and is retried next run, so a briefly-unavailable node self-heals.
- **Deterministic** (bad or dangling on-chain data, or a decoder that rejects immutable DNA): persisted once as `Failed`, then skipped thereafter — zero repeated CKB-VM/RPC work, zero recurring warnings.

Because `list_undecoded_dob_spores` is a presence check on `CF_DOB_DECODED`, writing a `Failed` entry removes the spore from the retry set; a transient failure writes nothing and keeps it in the set.

### DobDecodeFailure Schema

```
DobDecodeFailure
├── category: DobDecodeFailureCategory   // stable taxonomy (below)
├── message: String                      // human-readable detail → API `issues`
└── failed_at: i64                       // epoch seconds recorded
```

Deterministic categories (`DobDecodeFailureCategory`):

| Category                 | Meaning                                                                                                 |
| ------------------------ | ------------------------------------------------------------------------------------------------------- |
| `Clusterless`            | Clusterless "Sole Spore" (collection_id is the sole-spores sentinel) — no DOB cluster to decode against |
| `ClusterNotFound`        | A real (non-sentinel) cluster_id not present in the index                                               |
| `ClusterMetadataInvalid` | Cluster exists but metadata is unusable (no description, not JSON, missing `dob`, or bad decoder ref)   |
| `DecoderNotFound`        | Referenced decoder cell (code_hash or type_id) has no live cell                                         |
| `DecoderExecutionFailed` | Decoder binary ran and rejected the spore (non-zero exit, etc.)                                         |
| `DnaInvalid`             | On-chain content could not yield valid DNA                                                              |
| `Other`                  | Any other deterministic failure                                                                         |

The taxonomy is bincode-serialized in RocksDB, so new variants may only be **appended at the end**. The API surfaces the `message` (not the category enum) in the decode response's `issues`.

**Rebuild note**: deterministic failures are remembered only for this DB's lifetime. A `ckbadger purge` + re-sync from genesis re-attempts and re-classifies every spore, so a dependency that legitimately appears on-chain later self-heals on rebuild. (The `CF_DOB_DECODED` value format changed from a bare `DobDecodedEntry` to `DecodeOutcome`, so shipping this feature required a re-sync.)

## Frontend Per-Standard Behavior

| Standard      | Trait Filter           | Media Labels                                           | Preview Source                   |
| ------------- | ---------------------- | ------------------------------------------------------ | -------------------------------- |
| `plain-image` | N/A (no traits)        | N/A                                                    | Raw cell bytes (base64)          |
| `plain-svg`   | N/A                    | N/A                                                    | Raw SVG from cell data           |
| `plain-text`  | N/A                    | N/A                                                    | None                             |
| `dob/0`       | All traits shown       | "Decoded Output", "SVG Render"                         | Decoded media or render endpoint |
| `dob/1`       | Filter SVG/image blobs | "Decoder Chain Output" (with step count), "SVG Render" | Decoded media or render endpoint |
| `generic`     | N/A                    | N/A                                                    | None                             |

## CellLife: Hash-Seeded Game of Life Visualization

Each object in gallery views has an identicon generated by Conway's Game of Life, seeded deterministically from the object's hex hash. This gives every object a unique, living visual identity.

### How It Works

```
hex hash → hashToBytes() → seedGrid() → stepGrid() loop → canvas render
```

1. **Seed**: Hash bytes are bit-walked to populate an NxN grid (default 8x8). Border cells stay dead. Each bit in the hash determines whether an interior cell is alive.
2. **Color**: Single-chain objects use CKB jade (`#2edba3`). Dual-chain objects (BTC+CKB, determined by `mediaProfile.tier === 'btc_ckb'`) add BTC gold (`#f2c55c`). Color assignment uses a 16-byte offset in the hash for the second color channel.
3. **Shape**: Hash byte[16] selects one of 8 cell shapes (circle, square, diamond, triangle, hexagon, cross, star, rounded-square).
4. **Speed**: Hash byte[17] derives the animation interval (300–600ms).
5. **Evolution**: Standard B3/S23 rules. Survivors keep their color; newborns inherit the majority neighbor color (jade wins ties). Reseeds after extinction or 250 generations.

### Visual Layers

Each alive cell is rendered with a 4-layer bloom on a retina-aware canvas:

| Layer       | Scale | Opacity | Purpose              |
| ----------- | ----- | ------- | -------------------- |
| Outer bloom | 1.8x  | 8%      | Soft glow halo       |
| Inner bloom | 1.3x  | 18%     | Concentrated glow    |
| Cell body   | 1.0x  | 75%     | Main visible shape   |
| Core        | 0.45x | 90%     | Bright center accent |

Dead cells fade out at 0.12 opacity per tick (0.7x scale, 30% base opacity).

### Interaction & Accessibility

- **Hover pause**: Animation freezes on mouse enter, resumes on leave.
- **Reduced motion**: Respects `prefers-reduced-motion` — renders generation 0 only, no animation.
- **Outer glow**: CSS `glow-breathe` keyframe (4s ease-in-out loop) with per-instance phase offset derived from hash byte[0].

### Gallery Usage

The `ObjectGalleryPanel` chooses between `CellLife` and `CellLifePlaceholder` based on spore media tier:

- `pure_ckb` tier → `CellLife` (jade only)
- `btc_ckb` tier → `CellLife` with `isDualChain` (jade + gold)
- All other tiers / non-Spore standards → `CellLifePlaceholder` (static "?" box)

## Key Files

| Component          | File                                                  |
| ------------------ | ----------------------------------------------------- |
| Store types        | `crates/ckbadger-store/src/types.rs`                  |
| Spore parser       | `crates/indexer/src/parser/spore.rs`                  |
| Media analysis     | `crates/indexer/src/parser/media_source.rs`           |
| DOB decoder        | `crates/dob-decoder/src/lib.rs`                       |
| Decode worker      | `crates/indexer/src/sync/dob_decode_worker.rs`        |
| Decode error types | `crates/indexer/src/sync/dob_decode_error.rs`         |
| Blob store         | `crates/indexer/src/media_store.rs`                   |
| API routes         | `crates/api/src/routes/spore.rs`                      |
| Frontend standard  | `frontend/lib/object-standard.ts`                     |
| Frontend page      | `frontend/app/objects/[sporeId]/client-page.tsx`      |
| GoL engine         | `frontend/lib/game-of-life.ts`                        |
| CellLife component | `frontend/components/object/cell-life.tsx`            |
| Gallery panel      | `frontend/components/object/object-gallery-panel.tsx` |
| CellLife tests     | `frontend/__tests__/components/cell-life.test.tsx`    |
