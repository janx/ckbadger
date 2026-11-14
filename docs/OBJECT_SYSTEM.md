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

A spore's `content_type` (MIME) determines how its cell data is presented:

| Content Type                    | Behavior                                             |
| ------------------------------- | ---------------------------------------------------- |
| `image/png`, `image/jpeg`, etc. | Raw bytes displayed as image                         |
| `image/svg+xml`                 | Raw SVG in cell data                                 |
| `text/plain`, `text/html`       | Raw text in cell data; only an embedded SVG is drawn |
| `dob/0`, `dob/1`                | DNA decoded by on-chain decoders                     |
| other                           | No picture; the cell's fields in the evidence tier   |

A `dob/*` content type marks the spore as DOB-decodable, but it does **not**
decide which DOB protocol version runs — see below.

## DOB Protocol

DOB spores store DNA as cell content. The DNA is decoded by on-chain decoder binaries specified in the parent cluster's description JSON.

### Version Dispatch (cluster `dob.ver`, not `content_type`)

The protocol version comes from the **cluster metadata's `dob.ver`**, mirroring
the official server's `ClusterDescriptionField::unbox_dob`:

| `dob.ver` in cluster metadata | Dispatch                            |
| ----------------------------- | ----------------------------------- |
| absent or `null`              | DOB/0 (per spec)                    |
| `0` / `1`                     | DOB/0 / DOB/1                       |
| any other number, non-integer | Loud `ClusterMetadataInvalid` error |

The spore's own `content_type` is used only as a consistency cross-check; it is
never a dispatch source and never silently defaults to 0. Mainnet has clusters
declaring `ver: 1` while holding spores typed `dob/0` — those must run the DOB/1
chain, which is exactly what the official reference server returns.

### DNA Content Forms

`parse_dna_hex_from_content()` (`crates/indexer/src/parser/media_source.rs`)
accepts both official forms, matching the reference server's
`decode_spore_content`:

| First content byte | Form       | DNA                                                                                       |
| ------------------ | ---------- | ----------------------------------------------------------------------------------------- |
| `0x00`             | Raw binary | Hex encoding of the remaining bytes                                                       |
| anything else      | UTF-8 text | JSON string, JSON array (first item), JSON object `{"dna": "..."}`, or a plain hex string |

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

## Frontend Behavior by Content Type

The object page (`frontend/components/asset/spore-detail.tsx`) lists a DOB's traits in its hero
(`frontend/components/asset/object-hero.tsx`, filtered by `isReadableTrait` in
`frontend/lib/dob-genes.ts`) and picks its picture in `frontend/components/asset/object-media.tsx`
with `detectPreview` (`frontend/lib/preview-utils.ts`):

| Content Type                                                                    | Traits in the hero                                                                                        | Picture                                         |
| ------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------- | ----------------------------------------------- |
| `dob/0`, `dob/1`                                                                | Every trait but DOB/1's `prev.*` render hints and media values (a URI, SVG source), which stay on the gel | The decoded image media, the highest step first |
| `image/png`, `image/jpeg`, `image/gif`, `image/webp`, `image/avif`, `image/bmp` | None                                                                                                      | The cell's bytes, up to 2 MB                    |
| `image/svg+xml`                                                                 | None                                                                                                      | The SVG in the cell's data, up to 256 KB        |
| `text/*`, JSON, XML, JavaScript                                                 | None                                                                                                      | An `<svg>` inside the text, up to 256 KB        |
| other                                                                           | None                                                                                                      | None                                            |

A media source on Bitcoin (`btcfs`) comes first: the page says the image lives on Bitcoin and
names the inscription, and fetches nothing. Without a picture it shows a placeholder ("Decode
pending · awaiting DOB execution." while a DOB waits for its decoder, else "No renderable media
available.").

## Key Files

| Component          | File                                             |
| ------------------ | ------------------------------------------------ |
| Store types        | `crates/ckbadger-store/src/types.rs`             |
| Spore parser       | `crates/indexer/src/parser/spore.rs`             |
| Media analysis     | `crates/indexer/src/parser/media_source.rs`      |
| DOB decoder        | `crates/dob-decoder/src/lib.rs`                  |
| Decode worker      | `crates/indexer/src/sync/dob_decode_worker.rs`   |
| Decode error types | `crates/indexer/src/sync/dob_decode_error.rs`    |
| Blob store         | `crates/indexer/src/media_store.rs`              |
| API routes         | `crates/api/src/routes/spore.rs`                 |
| Frontend page      | `frontend/app/objects/[sporeId]/client-page.tsx` |
