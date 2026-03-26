# Activity System Analysis

Complete technical reference for the ckbadger activity system — from blockchain data to pixels on screen.

## Table of Contents

- [Philosophy](#philosophy)
- [Architecture Overview](#architecture-overview)
- [Data Structures](#data-structures)
- [Activity Builder](#activity-builder)
- [Storage Layer](#storage-layer)
- [API Layer](#api-layer)
- [Frontend Layer](#frontend-layer)
- [Pipeline Integration](#pipeline-integration)
- [Reorg Handling](#reorg-handling)
- [Statistics Aggregation](#statistics-aggregation)
- [Protocol Grouping](#protocol-grouping)
- [File Reference](#file-reference)

---

## Philosophy

Activities are **interpretations layered on facts**. A CKB transaction is an atomic UTXO bundle that can involve multiple parties and asset types simultaneously. The activity system interprets each transaction through a three-layer model (see `docs/prompts/ACTIVITY_DESIGN.md`):

- **Layer 1 — CKB Position** (per-participant, always present): ckb_delta, used_delta
- **Layer 2 — Item Delta** (per-participant balance sheet): token/object/identity position changes
- **Layer 3 — Protocol Action** (TX level): cross-user behaviors and item actions (DAO, RGB++, Fiber, Stable++)

The storage model stores one `TxActions` record per transaction. TX-level data (protocol_actions, type_calls, lock_calls) is stored once. Per-participant data (ckb_delta, used_delta, item_deltas, tags) is stored per participant within the same record.

## Architecture Overview

```
Fetcher (RPC)  →  Parser (CPU)  →  Writer (DB)  →  API (read)  →  Frontend (display)
                                       │
                        ┌───────────────┼───────────────┐
                        ▼               ▼               ▼
                  CF_TX_ACTIONS   CF_ADDR_TXS    CF_STATS_CHAIN
                  (TxActions)    (address idx)  (daily/hourly)
```

**Write path**: The indexer pipeline builds `TxActions` per transaction, writes records to `CF_TX_ACTIONS`, thin index entries to `CF_ADDR_TXS`, and accumulates stats into `CF_STATS_CHAIN`.

**Read path**: The API reads `TxActions` from `CF_TX_ACTIONS`, extracts per-participant data for address feeds or expands all participants for global feeds, validates canonicality, and returns paginated JSON. The frontend classifies and renders.

## Data Structures

### TxActions — Canonical Storage Type

One record per canonical transaction. TX-level fields are stored once; per-participant data is in the `participants` vector.

```rust
// crates/ckbadger-store/src/types.rs
pub struct TxActions {
    pub tx_hash: Vec<u8>,                    // 32-byte tx hash
    pub block_hash: Vec<u8>,                 // 32-byte block hash
    pub block_number: i64,
    pub tx_index: i32,
    pub timestamp: i64,                      // Unix epoch seconds
    pub is_cellbase: bool,
    pub protocol_actions: Vec<ProtocolAction>,  // Layer 3: TX-level (stored once)
    pub type_calls: Vec<TypeCallEntry>,         // Unrecognized type scripts (stored once)
    pub lock_calls: Vec<LockCallEntry>,         // Non-standard lock scripts (stored once)
    pub participants: Vec<ParticipantDelta>,    // All participants, sorted by lock_hash
}
```

**Key design decisions:**

- **TX-level fields stored once**: `protocol_actions`, `type_calls`, and `lock_calls` are properties of the transaction, not individual participants. Storing them once eliminates redundancy.
- **`participants` sorted by lock_hash**: Deterministic ordering enables consistent serialization and efficient lookup.

### ParticipantDelta — Per-Participant Position Change

```rust
// crates/ckbadger-store/src/types.rs
pub struct ParticipantDelta {
    pub lock_hash: Vec<u8>,          // 32-byte lock script hash (participant identity)
    pub ckb_delta: i128,             // Net CKB change (shannons) — i128 for overflow safety
    pub used_delta: i64,             // Net occupied capacity change (shannons)
    pub item_deltas: Vec<ItemDelta>, // Layer 2: position changes for tokens/objects/identities
    pub tags: u16,                   // Bitmask classification for fast filtering
}
```

**Key design decisions:**

- **`ckb_delta` is i128**: CKB amounts are u64 shannons, but deltas can overflow i64 when a single participant has massive input/output imbalance across many cells.
- **`tags` bitmask**: Enables O(1) filter matching without inspecting item_deltas or protocol_actions. Set during the build phase.
- **No lock script components**: Unlike the old `OwnerActivityDelta`, `ParticipantDelta` does not carry lock_code_hash, lock_hash_type, lock_args, or peers. Lock script details are resolved from the address store at API response time; peers are all other participants in the same `TxActions` record.

### ItemDelta — Uniform Item Position Change

Replaces the old `AssetChange` tagged enum with a uniform structure for all item types.

```rust
// crates/ckbadger-store/src/types.rs
pub struct ItemDelta {
    pub item_id: Vec<u8>,  // type_script_hash (token) or object/identity ID
    pub kind: u8,          // ITEM_KIND_TOKEN=0, ITEM_KIND_OBJECT=1, ITEM_KIND_IDENTITY=2
    pub delta: i128,       // Signed amount: tokens use precise amounts; objects/identities use +1/-1
}
```

**Item kind constants:**

| Constant             | Value | Meaning                           |
| -------------------- | ----- | --------------------------------- |
| `ITEM_KIND_TOKEN`    | 0     | Fungible token (sUDT, xUDT)       |
| `ITEM_KIND_OBJECT`   | 1     | Non-fungible object (Spore, mNFT) |
| `ITEM_KIND_IDENTITY` | 2     | Identity (.bit, did:ckb)          |

**Key design decisions:**

- **Uniform structure**: All item types share the same struct. No tagged enum variants — the `kind` field discriminates. This is extensible to future asset types without structural changes.
- **No action enum**: Mint/transfer/burn classification is derived from the pattern of deltas across participants (see `docs/prompts/ACTIVITY_DESIGN.md`), not stored explicitly.
- **No DAO variants**: DAO operations are captured at Layer 3 (protocol_actions), not Layer 2 (item_deltas). A DAO cell is CKB in a different state, not a separate portfolio item.

### Tags Bitmask — Per-Participant Classification

Tags enable fast filtering without deserializing and inspecting nested data.

```rust
// crates/ckbadger-store/src/types.rs
pub const TAG_TOKEN: u16     = 1 << 0;  // 0x01 — has token item_deltas
pub const TAG_OBJECT: u16    = 1 << 1;  // 0x02 — has object item_deltas
pub const TAG_IDENTITY: u16  = 1 << 2;  // 0x04 — has identity item_deltas
pub const TAG_DAO: u16       = 1 << 3;  // 0x08 — involved in DAO operation
pub const TAG_PROTOCOL: u16  = 1 << 4;  // 0x10 — TX has protocol_actions
pub const TAG_CELLBASE: u16  = 1 << 5;  // 0x20 — cellbase transaction
pub const TAG_TYPE_CALL: u16 = 1 << 6;  // 0x40 — TX has unrecognized type scripts
pub const TAG_LOCK_CALL: u16 = 1 << 7;  // 0x80 — TX has non-standard lock scripts
```

Tags are set during the build phase and stored on each `ParticipantDelta`. A single participant can have multiple tags set simultaneously (e.g., `TAG_TOKEN | TAG_PROTOCOL`).

### TypeCallEntry — Unrecognized Type Script

```rust
// crates/ckbadger-store/src/types.rs
pub struct TypeCallEntry {
    pub type_code_hash: Vec<u8>,
    pub type_hash_type: i16,
    pub type_args: Vec<u8>,
}
```

### LockCallEntry — Non-Standard Lock Script

Lock calls capture interactions with non-standard lock scripts (anything other than the 16 well-known access-control locks like secp256k1, multisig, omni-lock, etc.).

```rust
// crates/ckbadger-store/src/types.rs
pub struct LockCallEntry {
    pub lock_code_hash: Vec<u8>,
    pub lock_hash_type: i16,
    pub lock_args: Vec<u8>,
}
```

### ProtocolAction — Layer 3 Protocol Detection

Protocol actions capture high-level protocol behaviors detected from cross-layer signals (e.g., lock script patterns, cell structure). Unlike item deltas which are derived from type scripts, protocol actions are detected by `ProtocolDetector` implementations that analyze the full transaction context.

```rust
// crates/ckbadger-store/src/types.rs
pub struct ProtocolAction {
    pub protocol: String,       // e.g., "rgbpp", "dao"
    pub action: String,         // e.g., "leap_to_ckb", "deposit", "withdraw_complete"
    pub metadata: serde_json::Value,  // Protocol-specific details
}
```

**DAO is now a protocol action**: DAO deposit, withdraw request, and withdraw complete are all captured as `ProtocolAction { protocol: "dao", action: "deposit"|"withdraw_request"|"withdraw_complete", ... }` at Layer 3. They are NOT recorded as item deltas at Layer 2 (a DAO cell is CKB in a different state, not a portfolio item).

### DailyActivityStats — Aggregation Counters

```rust
// crates/ckbadger-store/src/types.rs
pub struct DailyActivityStats {
    pub transfer_count: u32,              // Pure CKB transfers
    pub dao_deposit_count: u32,
    pub dao_withdraw_request_count: u32,
    pub dao_withdraw_complete_count: u32,
    pub token_count: u32,                 // UDT transfers
    pub object_count: u32,                // Spore/mNFT
    pub identity_count: u32,              // .bit/did:ckb
    pub script_call_count: u32,           // Unrecognized scripts
    pub unknown_count: u32,               // Fallback (should be 0)
    pub coinbase_count: u32,              // Mining rewards
    pub unique_address_count: u32,        // Distinct lock_hashes
    pub total_ckb_moved: u128,            // Sum of |ckb_delta| across all participants
    pub script_counts: HashMap<String, u32>,  // Per-code_hash counts
    pub protocol_action_counts: HashMap<String, u32>,  // Per-protocol action counts
}
```

### ObjectCollectionActivityEntry — Pre-Computed Collection Feed

```rust
// crates/ckbadger-store/src/types.rs
pub struct ObjectCollectionActivityEntry {
    pub tx_hash: Vec<u8>,
    pub block_hash: Vec<u8>,
    pub timestamp_ms: i64,
    pub actions: Vec<AssetAction>,       // Aggregated actions within one tx
}
```

---

## Activity Builder

**File**: `crates/indexer/src/db/writer/activities.rs`

The builder is a **pure function**: given transaction data and protocol detectors, it emits one `TxActions` per transaction with no side effects. Participants within a record are sorted deterministically by `lock_hash`.

### Entry Points

```rust
// activities.rs
pub fn build_tx_actions_for_block(
    txs: &[TxView<'_>],
    detectors: &[Box<dyn ProtocolDetector>],
) -> Result<Vec<TxActions>>

pub fn build_tx_actions_for_block_no_detectors(
    txs: &[TxView<'_>],
) -> Result<Vec<TxActions>>
```

**Parameters:**

- `txs`: All transactions in a block, with parsed cell data
- `detectors`: Protocol detectors (DAO, RGB++, Fiber, Stable++) for Layer 3 analysis

### Code Hash Classification

`CodeHashes` is a struct initialized eagerly via `OnceLock`. Contains two maps:

- `type_scripts: HashMap<Vec<u8>, AssetKind>` — Maps type_code_hash bytes to asset kind
- `standard_locks: HashSet<Vec<u8>>` — 16 well-known lock script code_hashes to exclude from lock call detection

```rust
enum AssetKind {
    Udt,          // sUDT, xUDT, and xudt_compatible scripts
    Dao,          // Nervos DAO
    SporeDid,     // did:ckb (Spore DID variant)
    Spore,        // Spore/DOB item
    Cluster,      // Spore cluster
    MnftToken,    // mNFT token
    Dotbit,       // .bit account
}
```

**Hardcoded entries (13):**

| Asset Kind | Code Hash Source                   | Count |
| ---------- | ---------------------------------- | ----- |
| Udt        | SUDT, XUDT data1, XUDT type        | 3     |
| Dao        | DAO_CODE_HASH                      | 1     |
| SporeDid   | SPORE_CODE_HASH_MAINNET_DID        | 1     |
| Spore      | mainnet v2, testnet v1, testnet v2 | 3     |
| Cluster    | mainnet v2, testnet v1, testnet v2 | 3     |
| MnftToken  | MNFT_TOKEN_CODE_HASH               | 1     |
| Dotbit     | DOTBIT_ACCOUNT_CELL_TYPE_ID        | 1     |

**Bundled entries (dynamic):** build.rs extracts additional code_hashes from scripts with `decoderType: "udt"` in the token-labels data. These are `xudt_compatible` scripts (Stable++ Asset, ccBTC Asset, wCKB Asset, USDI Asset) that share the xUDT cell data layout. Loaded via `include_bytes!` at compile time, parsed as JSON, and inserted as `AssetKind::Udt` with `entry().or_insert()` (hardcoded entries take precedence).

### Standard Lock Exclusion

The `standard_locks` set contains 16 code_hashes for well-known access-control lock scripts. These are excluded from lock call detection because they represent standard ownership (who can spend a cell), not protocol actions.

| Lock Script                 | Variants     |
| --------------------------- | ------------ |
| secp256k1-blake160          | default      |
| secp256k1-blake160-multisig | multisig     |
| anyone-can-pay              | ACP          |
| omni-lock                   | omni         |
| PW-lock                     | PW           |
| JoyID                       | COTA, Subkey |

Output cells whose lock code_hash appears in `standard_locks` are not recorded as lock calls.

### Per-Transaction Algorithm

The builder uses an `OwnerAccum` accumulator struct per lock_hash:

```
1. Process inputs:
   - Group by lock_script_hash → OwnerAccum
   - Record lock script components (code_hash, hash_type, args)
   - Sum input_capacity and input_occupied_capacity
   - classify_input() → UDT amounts, DAO flags, Object/Identity IDs, type calls

2. Process outputs:
   - Group by lock_script_hash → OwnerAccum (same map, may create new entries)
   - Record lock script components
   - Sum output_capacity
   - Compute occupied_capacity: (8 + lock_size + type_size + data_size) × 100_000_000
   - classify_output() → UDT amounts, DAO ops, Object/Identity IDs, type calls

3. Detect lock calls:
   - For each output cell, check lock code_hash against `standard_locks`
   - If NOT in standard_locks → record as lock call (TX-level, deduplicated)

4. Run protocol detectors:
   - Each ProtocolDetector analyzes the full transaction context
   - Emit ProtocolAction entries (TX-level, stored once)

5. Build per-participant deltas (sorted by lock_hash):
   - ckb_delta = Σ output_capacity - Σ input_capacity
   - used_delta = Σ output_occupied - Σ input_occupied
   - Derive item_deltas from accumulated data:
     • UDT: delta = output_amount - input_amount per type_script_hash (kind=TOKEN)
     • Object/Identity: set comparison (input IDs vs output IDs → +1/-1) (kind=OBJECT/IDENTITY)
   - Compute tags bitmask from item_deltas, protocol_actions, type_calls, lock_calls

6. Wrap into single TxActions record with TX-level + per-participant data
```

### Classification Functions

**`classify_input()`**: Processes input cell type script.

- Matches code_hash against `CodeHashes` type_scripts lookup
- DAO: records deposit/withdraw flags for protocol detector consumption
- UDT: parse amount from `udt_amount` field (pre-fetched) or cell data via `UdtParser::parse_amount()`
- Spore/did:ckb: extract type_args as object/identity ID
- Unrecognized: call `record_type_call()` → stored in `type_calls`

**`classify_output()`**: Processes output cell type script.

- DAO: data == `[0u8; 8]` → deposit; data decodes to non-zero deposit_block → withdraw request
- UDT: parse amount from output cell data
- Spore/did:ckb: extract type_args as ID
- Unrecognized: call `record_type_call()`

**`emit_object_changes()`**: Set comparison for Object assets.

- ID in outputs only → delta = +1
- ID in both inputs and outputs → delta = 0 (not recorded)
- ID in inputs only → delta = -1

**`emit_identity_changes()`**: Same logic for Identity assets.

### UDT Amount Parsing

For inputs, the builder uses `InputCellView.udt_amount` (pre-fetched during sync from the append-only cell store) or falls back to parsing cell data. For outputs, amounts are parsed directly from `ParsedCell.data` via `UdtParser::parse_amount()` (first 16 bytes as little-endian u128).

---

## Storage Layer

**Files**: `crates/ckbadger-store/src/activity_ops.rs`, `keys.rs`, `batch.rs`

All activity storage is in the **domain store** (mutable, supports delete on rollback). Nothing in append-only.

### Column Families

| CF                                  | Key Size  | Value                           | Purpose                       |
| ----------------------------------- | --------- | ------------------------------- | ----------------------------- |
| `CF_TX_ACTIONS`                     | 44 bytes  | `TxActions` (postcard)          | Per-tx actions record         |
| `CF_ADDR_TXS`                       | 76 bytes  | empty                           | Address → tx index            |
| `CF_OBJECT_COLLECTION_ACTIVITIES`   | 108 bytes | `ObjectCollectionActivityEntry` | Spore/mNFT collection feeds   |
| `CF_IDENTITY_COLLECTION_ACTIVITIES` | 108 bytes | `ObjectCollectionActivityEntry` | .bit/did:ckb collection feeds |
| `CF_STATS_CHAIN` (prefixed)         | variable  | `DailyActivityStats` (postcard) | Hourly/daily aggregation      |

**Note**: `CF_TX_ACTIONS` uses the RocksDB string `"activities"` (same physical column family name as the old `CF_ACTIVITIES`). The Rust constant was renamed for clarity.

`CF_TX_ACTIONS` is registered in `MEGA_WRITE_CFS` for large batch optimization.

### Key Encoding — CF_TX_ACTIONS

```
block_num_desc(8B BE) + tx_idx_desc(4B BE) + tx_hash(32B) = 44 bytes

block_num_desc = i64::MAX - block_num
tx_idx_desc   = i32::MAX - tx_idx
```

**Descending order**: Forward RocksDB iteration yields newest transactions first. This enables efficient "latest N" queries and reorg cleanup (newest entries are encountered first during scan).

```rust
// keys.rs
pub fn encode_tx_actions_key(block_num: i64, tx_idx: i32, tx_hash: &[u8]) -> Vec<u8>;
pub fn decode_tx_actions_key(key: &[u8]) -> (i64, i32, Vec<u8>);
pub fn encode_tx_actions_seek_after_key(block_num: i64, tx_idx: i32) -> Vec<u8>;
```

The seek-after key pads tx_hash with `0xFF` bytes for cursor-based pagination positioning.

### Key Encoding — CF_ADDR_TXS

```
lock_hash(32B) + block_num_desc(8B BE) + tx_idx_desc(4B BE) + tx_hash(32B) = 76 bytes
```

**Prefix scan property**: Prefix by `lock_hash` lists all transactions for an address in descending order.

```rust
// keys.rs
pub fn encode_addr_tx_key(lock_hash: &[u8], block_num: i64, tx_idx: i32, tx_hash: &[u8]) -> Vec<u8>;
pub fn decode_addr_tx_key(key: &[u8]) -> (Vec<u8>, i64, i32, Vec<u8>);
pub fn encode_addr_tx_seek_after_key(lock_hash: &[u8], block_num: i64, tx_idx: i32) -> Vec<u8>;
```

### Key Encoding — Collection Activities

```
collection_id(32B padded) + block_num_desc(8B) + tx_idx_desc(4B) + block_hash(32B) + tx_hash(32B) = 108 bytes
```

### Value Encoding

All values use `postcard::to_allocvec()` — compact varint-encoded binary, fast to serialize/deserialize.

### Query Operations

**`list_tx_actions_recent(limit, cursor)`**: Forward scan on `CF_TX_ACTIONS`, returns newest `TxActions` records first.

**`get_latest_activities()`**: Scans `CF_TX_ACTIONS`, skips cellbase transactions, returns up to 64 `TxActions` records for the global feed.

**`list_activities(lock_hash, limit, cursor, filter)`**: Scans `CF_ADDR_TXS` by lock_hash prefix, multi-gets `TxActions` from `CF_TX_ACTIONS`, applies filter via `matches_activity_filter()`.

**`get_tx_actions(block_num, tx_idx, tx_hash)`**: Point lookup of a single `TxActions` record.

### Write Operations

```rust
// batch.rs
StoreBatch::put_tx_actions(&mut self, actions: &TxActions)
StoreBatch::put_addr_tx(&mut self, lock_hash: &[u8], block_num: i64, tx_idx: i32, tx_hash: &[u8])
```

### Activity Filter Matching

```rust
// activity_ops.rs
pub fn matches_activity_filter(
    actions: &TxActions,
    lock_hash: &[u8],
    filter: Option<&str>,
) -> bool
```

Filter matching uses the **per-participant tags bitmask** for O(1) classification. The function finds the participant matching `lock_hash` and checks their tags:

| Filter               | Condition                                                                     |
| -------------------- | ----------------------------------------------------------------------------- |
| `"ckb"`              | Tags has none of TOKEN, OBJECT, IDENTITY, DAO, PROTOCOL, TYPE_CALL, LOCK_CALL |
| `"token"`            | `tags & TAG_TOKEN != 0`                                                       |
| `"nft"` / `"object"` | `tags & TAG_OBJECT != 0`                                                      |
| `"identity"`         | `tags & TAG_IDENTITY != 0`                                                    |
| `"dao"`              | `tags & TAG_DAO != 0`                                                         |
| `"type_call"`        | `tags & TAG_TYPE_CALL != 0`                                                   |
| `"lock_call"`        | `tags & TAG_LOCK_CALL != 0`                                                   |
| `"protocol:*"`       | `tags & TAG_PROTOCOL != 0` AND protocol_actions matches protocol name         |
| `None` / `"all"`     | Always matches                                                                |

---

## API Layer

**File**: `crates/api/src/routes/activities.rs`

### Endpoints

**`GET /addresses/{addr}/activities`** — Per-address activity feed with pagination and filtering.

Query parameters:

- `limit` (default: 20)
- `cursor` — format: `"block_num:tx_idx"`, e.g., `"1234567:3"`
- `filter` — one of: `all`, `ckb`, `token`, `nft`, `dao`, `type_call`, `lock_call`

Response: `CursorPaginatedResponse<ActivityResponse>` with `items`, `next_cursor`, `has_more`.

**`GET /activities/latest`** — Global latest activities feed (homepage).

Query parameters:

- `limit` (default: 8, max: 64)

Response: `Vec<GlobalActivityResponse>` with per-participant data.

### Response Types

```rust
/// Per-address activity response — shows one participant's view of a transaction.
pub struct ActivityResponse {
    pub tx_hash: String,
    pub block_number: i64,
    pub tx_index: i32,
    pub timestamp: String,
    // This participant's Layer 1
    pub ckb_delta: String,          // String for i128 precision
    pub used_delta: String,
    pub is_cellbase: bool,
    // This participant's Layer 2
    pub item_deltas: Vec<ItemDeltaResponse>,
    // TX-level Layer 3
    pub type_calls: Vec<TypeCallResponse>,
    pub lock_calls: Vec<LockCallResponse>,
    pub protocol_actions: Vec<ProtocolActionResponse>,
    // Other participants (CKB addresses)
    pub participants: Vec<String>,
    pub tags: u16,
}

/// Global activity response — shows all participants in a transaction.
pub struct GlobalActivityResponse {
    pub tx_hash: String,
    pub block_number: i64,
    pub tx_index: i32,
    pub timestamp: String,
    pub is_cellbase: bool,
    // TX-level Layer 3
    pub protocol_actions: Vec<ProtocolActionResponse>,
    pub type_calls: Vec<TypeCallResponse>,
    pub lock_calls: Vec<LockCallResponse>,
    // All participants with their per-participant data
    pub participants: Vec<ParticipantResponse>,
}

/// A single participant within a global activity response.
pub struct ParticipantResponse {
    pub address: String,
    pub ckb_delta: String,
    pub used_delta: String,
    pub item_deltas: Vec<ItemDeltaResponse>,
    pub tags: u16,
}
```

**`ItemDeltaResponse`** is a tagged enum serialized with `#[serde(tag = "kind")]`:

- `"token"` — `typeScriptHash`, `delta`, `symbol`, `decimals`
- `"object"` — `objectId`, `delta` (+1/-1)
- `"identity"` — `identityId`, `delta` (+1/-1)

```rust
pub struct TypeCallResponse {
    pub type_code_hash: String,
    pub type_hash_type: String,
    pub type_args: String,
    pub script_hash: String,        // Computed from code_hash + hash_type + args
    pub script_name: Option<String>,    // Resolved from ScriptInfo DB
}

pub struct LockCallResponse {
    pub lock_code_hash: String,
    pub lock_hash_type: String,
    pub lock_args: String,
    pub script_hash: String,        // Computed from code_hash + hash_type + args
    pub script_name: Option<String>,    // Resolved from ScriptInfo DB
    pub decoded: Option<Value>,     // Decoded args (e.g., RGB++ btcTxid)
}

pub struct ProtocolActionResponse {
    pub protocol: String,           // e.g., "rgbpp", "dao"
    pub action: String,             // e.g., "leap_to_ckb", "deposit"
    pub metadata: Value,            // Protocol-specific details
}
```

### Lock Args Decoder Registry

`LOCK_ARGS_DECODERS` is a `LazyLock<HashMap<Vec<u8>, fn(&[u8]) -> Option<Value>>>` that maps lock code_hashes to decoder functions. Currently registered decoders:

| Lock Script   | Decoder Output                                                     |
| ------------- | ------------------------------------------------------------------ |
| RGB++ Lock    | `{"protocol": "rgbpp", "btcTxid": "...", "outIndex": N}`           |
| BTC Time Lock | `{"protocol": "rgbpp", "action": "btcTimeLock", "btcTxid": "..."}` |

**RGB++ decoder**: args[0..4] = out_index (u32 LE), args[4..36] = btc_txid (32 bytes LE, reversed for display). Minimum 36 bytes.

**BTC Time Lock decoder**: btc_txid is the last 32 bytes of args (matching `RgbppParser::extract_btc_txid_from_btc_time_lock_args` in the indexer). Minimum 36 bytes.

### Canonicality Validation

The API validates that each activity entry matches the canonical chain position. This is necessary because during reorgs, activity entries from orphaned blocks may linger until rollback completes.

Each entry is checked: `entry.block_number == canonical_block_num && entry.tx_index == canonical_tx_idx && entry.block_hash == canonical_block_hash`. Non-matching entries are silently dropped.

The `list_canonical_activities_page()` function implements a loop that scans entries in chunks, filters orphans, and continues scanning until `limit` canonical entries are found or the address runs out of entries.

### Script Info Resolution

Script names are resolved via `ScriptInfo` lookup and cached per-request:

```rust
fn resolve_script_info_cached(store, cache, code_hash) -> Option<&ScriptInfo>
```

The cache avoids repeated DB lookups for the same code_hash within a single API request. Used for both type call and lock call name resolution.

---

## Frontend Layer

### Activity Classification

**File**: `frontend/lib/activity-classify.ts`

The frontend classifies each global activity for display purposes. Classification uses the tags bitmask from participants and inspects protocol_actions for DAO sub-types:

```typescript
type ActivityType =
  | 'daoDeposit'
  | 'daoWithdrawRequest'
  | 'daoWithdrawComplete'
  | 'token'
  | 'object'
  | 'identity'
  | 'protocolAction'
  | 'typeCall'
  | 'ckbTransfer';

interface ClassifiedActivity {
  displayType: ActivityType;
  activity: GlobalActivity;
  primaryProtocolAction: ActivityProtocolAction | null;
  primaryItemDelta: ItemDelta | null;
  primaryTypeCall: ActivityTypeCall | null;
  primaryLockCall: ActivityLockCall | null;
}
```

**Priority order** (highest to lowest):

1. DAO protocol actions (deposit, withdraw_request, withdraw_complete) — checked via `protocol_actions` with `protocol === 'dao'`
2. Other protocol actions (`protocol_actions` non-empty)
3. Token item deltas (`TAG_TOKEN` in combined tags)
4. Object item deltas (`TAG_OBJECT` in combined tags)
5. Identity item deltas (`TAG_IDENTITY` in combined tags)
6. Type calls (unrecognized type scripts)
7. CKB transfer (fallback)

The `primaryItemDelta` is the first item delta found across all participants. The `primaryTypeCall` is the first type call (if any). The `primaryLockCall` is always the first lock call (if any). These are used for the homepage stream display where only one badge per activity is shown.

### Frontend Types

```typescript
// frontend/lib/api.ts

type ItemDelta =
  | { kind: 'token'; typeScriptHash: string; delta: string; symbol?: string; decimals?: number }
  | { kind: 'object'; objectId: string; delta: number }
  | { kind: 'identity'; identityId: string; delta: number };

interface Activity {
  txHash: string;
  blockNumber: number;
  txIndex: number;
  timestamp: string;
  ckbDelta: string;
  usedDelta: string;
  isCellbase: boolean;
  itemDeltas: ItemDelta[];
  typeCalls: ActivityTypeCall[];
  lockCalls: ActivityLockCall[];
  protocolActions: ActivityProtocolAction[];
  participants: string[]; // CKB addresses of other participants
  tags: number; // Bitmask
}

interface ParticipantInfo {
  address: string;
  ckbDelta: string;
  usedDelta: string;
  itemDeltas: ItemDelta[];
  tags: number;
}

interface GlobalActivity {
  txHash: string;
  blockNumber: number;
  txIndex: number;
  timestamp: string;
  isCellbase: boolean;
  protocolActions: ActivityProtocolAction[];
  typeCalls: ActivityTypeCall[];
  lockCalls: ActivityLockCall[];
  participants: ParticipantInfo[];
}
```

### Homepage Rendering — Latest Activities

**File**: `frontend/components/latest-activities.tsx`

The `LatestActivities` component fetches via `api.getLatestActivities(32)` with 10s polling. Each activity is classified and rendered as a `StreamItem` component based on its type:

- `StreamItemCkbTransfer` — CKB amount delta with jade/red coloring + optional `LockCallBadge`
- `StreamItemDaoDeposit` / `StreamItemDaoWithdrawRequest` / `StreamItemDaoWithdrawComplete` — Gold-colored DAO badge
- `StreamItemToken` — Pink/magenta token transfer with symbol + optional `LockCallBadge`
- `StreamItemObject` — Lavender object badge with link to detail page
- `StreamItemIdentity` — Aqua identity badge with link to detail page
- `StreamItemTypeCall` — Amber type call with function-call syntax (`TypeCallExpr`)
- `StreamItemProtocolAction` — Violet protocol action with protocol name + lock call expression (`LockCallExpr`)

Each stream item shows: badge + label (left), timestamp (right), address link (left), value/delta (right).

New items animate in with a glow/bounce effect tracked via `newItemKeys` state. Max visible items: 20 (`MAX_STREAM_ITEMS`).

### Address Page Rendering — Activity Event Rows

**File**: `frontend/components/activity-event-row.tsx`

The `ActivityEventGroup` component renders all events for a single transaction as a group. Unlike the homepage (one badge per activity), the address page shows ALL item deltas, type calls, lock calls, and the CKB delta as separate sub-rows within one tx group.

**Layout:**

- **Narrow** (<md): Stacked card with tx hash, block number, time, and events
- **Wide** (>=md): Grid cells with 4 columns: tx info, badge, value, time

The CKB event is always the last sub-row. A single tx group may show: DAO Protocol Action + Token Transfer + Type Call + Lock Call + CKB Transfer — all as separate rows.

### Color Scheme

| Activity Type   | Color           | Icon |
| --------------- | --------------- | ---- |
| DAO             | `text-gold`     | ◆    |
| Token           | `text-pink`     | ◈    |
| Spore/Object    | `text-lavender` | ✦    |
| Identity        | `text-aqua`     | ✶    |
| Type call       | `text-amber`    | ⚙    |
| Lock call       | `text-violet`   | ⚡   |
| Protocol action | `text-violet`   | ⚡   |
| CKB transfer    | `text-jade`     | ↗    |
| Coinbase        | `text-gold`     | ★    |

### TypeCallExpr Component

Renders a type call in function-call syntax:

```
ScriptName(0xargs...)
```

Where `ScriptName` is a link to the script detail page (resolved from `scriptName` or formatted from hash_type + code_hash prefix). The args are truncated.

### LockCallExpr Component

Renders a lock call in function-call syntax, same visual pattern as `TypeCallExpr`:

```
LockName(0xargs...)
```

Where `LockName` is a link to the script detail page (resolved from `scriptName` or formatted from hash_type + code_hash prefix).

### LockCallBadge Component

Compact uppercase pill showing protocol name or lock script name. Used inline on `StreamItemCkbTransfer` and `StreamItemToken` to indicate lock script involvement without being the primary classification.

---

## Pipeline Integration

### Bulk Sync Path

**File**: `crates/indexer/src/sync/batch.rs`

During bulk sync, activities are written in a dedicated parallel thread:

1. Build `TxView` from parsed block data + prefetched input cells
2. Call `build_tx_actions_for_block()` with protocol detectors
3. For each `TxActions`, accumulate daily/hourly stats, then `activity_batch.put_tx_actions()`
4. Write `addr_tx` entries for each participant in the main domain batch
5. Commit activity batch with `commit_phase_no_wal()` (WAL disabled for bulk)

Bulk sync skips the undo journal (per BULK_SYNC.md rules 5-7). Activities go directly to `StoreBatch::put_tx_actions()` and `StoreBatch::put_addr_tx()`.

### Live Sync Path

**File**: `crates/indexer/src/sync/batch.rs`

During live sync, activities are written through the undo helper:

1. Same `build_tx_actions_for_block()` call with protocol detectors
2. Same stats accumulation
3. `put_tx_actions()` via `sync::undo::put_tx_actions()` — records to domain batch (no undo log needed since rollback deletes directly)
4. `put_addr_tx()` via `sync::undo::put_addr_tx()` — same pattern

The undo helpers in `sync/undo.rs` wrap `StoreBatch` writes. Activities and addr_txs are in the domain store, so rollback deletes entries directly rather than using an undo log.

---

## Reorg Handling

**File**: `crates/ckbadger-store/src/reorg_ops.rs`

Activities use **direct deletion** during reorg rollback — no ghost entries, no canonical filtering needed.

### Rollback Steps

**Stage 8b — Delete tx actions** :
Scan `CF_TX_ACTIONS` from start. Since keys are in descending block_num order, newest entries come first. Delete all entries where `block_num > rollback_to`. Break when `block_num <= rollback_to` (all remaining entries are valid).

**Stage 8c — Delete addr_txs** :
Full scan of `CF_ADDR_TXS`. Extract block_num from each key. Delete entries where `block_num > rollback_to`. Cannot break early because addr_txs keys are prefixed by lock_hash (entries from different addresses are interleaved).

**Stage 8d/8e — Delete collection activities** :
Same pattern for `CF_OBJECT_COLLECTION_ACTIVITIES` and `CF_IDENTITY_COLLECTION_ACTIVITIES`. Also counts surviving entries per collection for aggregate rebuild.

### Key Invariants

- Keys in descending block_num order allow efficient early termination for `CF_TX_ACTIONS`
- After rollback, the domain store contains only canonical entries — no secondary cleanup needed
- The API's canonicality check provides a safety net during the brief window between chain reorg detection and rollback completion

---

## Statistics Aggregation

**File**: `crates/indexer/src/db/writer/statistics.rs`

Activity stats are accumulated per-transaction from `TxActions` and aggregated at two time granularities.

### Classification for Stats

```rust
// statistics.rs
fn accumulate_tx_actions_stats(tx_actions: &TxActions, stats: &mut DailyActivityStats)
```

Stats classification iterates over each participant's tags bitmask and item_deltas, and the TX-level protocol_actions:

1. **Coinbase** — counted separately, excluded from all other metrics
2. **DAO** — if `TAG_DAO` is set (from protocol_actions with `protocol == "dao"`)
3. **Token** — if `TAG_TOKEN` is set
4. **Object** — if `TAG_OBJECT` is set
5. **Identity** — if `TAG_IDENTITY` is set
6. **Script call** — if `TAG_TYPE_CALL` is set
7. **Transfer** — if no tags besides possible TAG_LOCK_CALL (pure CKB)
8. **Unknown** — fallback (should be 0)

**Note**: Token, object, identity, and script_call get boolean flags — a participant with 3 token changes is counted once in `token_count`.

### Additional Metrics

- **`total_ckb_moved`**: Sum of `|ckb_delta|` across all non-cellbase participants (u128)
- **`unique_address_count`**: Distinct lock_hashes per day/hour (computed from HashSet of `[u8; 32]`)
- **`script_counts`**: Per-code_hash activity counts (hex string keys → u32 counts)
- **`protocol_action_counts`**: Per-protocol action counts (e.g., `"rgbpp" => 5`, `"dao" => 12`)

### Storage

- **Daily**: Key prefix `0x1D` + `YYYYMMDD` in `CF_STATS_CHAIN`
- **Hourly**: Key prefix `0x1E` + `YYYYMMDDHH` in `CF_STATS_CHAIN`

Stats are accumulated in memory during batch processing, then merged with existing values using `update_daily_activity_stats()` / `update_hourly_activity_stats()`. The merge reads existing stats, adds the accumulated values, and writes back — this is necessary because a single day/hour may span multiple batches.

### API Endpoints

- **`GET /stats/daily-activities`** — Returns daily activity stats for charting (list of day-level breakdowns)
- **`GET /stats/activity-summary-24h`** — Aggregates all hourly buckets within a 24h rolling window for the summary card

---

## Protocol Grouping

Protocol identification is handled by Layer 3 `ProtocolDetector` implementations in the indexer. Each detector recognizes protocol-specific script patterns and emits `ProtocolAction` entries.

Active detectors:

- **RgbppDetector** — RGB++ leap/transfer actions via lock script transitions
- **FiberDetector** — Fiber Network channel lifecycle (open/close/force_close/settlement)
- **StableppDetector** — Stable++ CDP vault lifecycle (open_vault/borrow/repay/close_vault/liquidation/redemption)

**DAO protocol actions** (deposit, withdraw_request, withdraw_complete) are emitted directly by the activity builder during the build phase, not by a separate `ProtocolDetector` implementation. DAO detection uses the existing `AssetKind::Dao` classification from `CodeHashes`.

The `docs/script-name-overrides.json` `protocols` field retains code_hash groupings as reference metadata but is no longer used at runtime for protocol identification.

---

## File Reference

### Core Data Structures

| File                                 | Content                                                                                                                      |
| ------------------------------------ | ---------------------------------------------------------------------------------------------------------------------------- |
| `crates/ckbadger-store/src/types.rs` | TxActions, ParticipantDelta, ItemDelta, TypeCallEntry, LockCallEntry, ProtocolAction, DailyActivityStats, tag/kind constants |

### Activity Builder

| File                                                | Content                                                                                                                 |
| --------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------- |
| `crates/indexer/src/db/writer/activities.rs`        | build_tx_actions_for_block(), CodeHashes, classify_input/output, emit_object/identity_changes, DAO protocol action emit |
| `crates/indexer/src/db/writer/rgbpp_detector.rs`    | RgbppDetector: ProtocolDetector impl (leap_to_ckb, leap_to_btc, transfer, btc_time_locked, receive)                     |
| `crates/indexer/src/db/writer/fiber_detector.rs`    | FiberDetector: ProtocolDetector impl (open, close, force_close, settlement)                                             |
| `crates/indexer/src/db/writer/stablepp_detector.rs` | StableppDetector: ProtocolDetector impl (open_vault, borrow, repay, close_vault, adjust, liquidation, redemption)       |
| `crates/indexer/src/build.rs`                       | Compile-time extraction of xudt_compatible code_hashes                                                                  |

### Storage

| File                                        | Content                                                               |
| ------------------------------------------- | --------------------------------------------------------------------- |
| `crates/ckbadger-store/src/activity_ops.rs` | list_activities(), get_latest_activities(), matches_activity_filter() |
| `crates/ckbadger-store/src/keys.rs`         | Key encoding/decoding for CF_TX_ACTIONS (44B), CF_ADDR_TXS (76B)      |
| `crates/ckbadger-store/src/batch.rs`        | put_tx_actions(), put_addr_tx()                                       |
| `crates/ckbadger-store/src/reorg_ops.rs`    | Activity rollback (stages 8b-8e)                                      |

### API

| File                                  | Content                                                                |
| ------------------------------------- | ---------------------------------------------------------------------- |
| `crates/api/src/routes/activities.rs` | Endpoints, response types, canonicality validation, LOCK_ARGS_DECODERS |

### Statistics

| File                                         | Content                                                             |
| -------------------------------------------- | ------------------------------------------------------------------- |
| `crates/indexer/src/db/writer/statistics.rs` | accumulate_tx_actions_stats(), update_daily/hourly_activity_stats() |

### Frontend

| File                                         | Content                                                                                                                |
| -------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- |
| `frontend/lib/api.ts`                        | ItemDelta, ActivityTypeCall, ActivityLockCall, ActivityProtocolAction, Activity, GlobalActivity, ParticipantInfo types |
| `frontend/lib/activity-classify.ts`          | classifyActivity(), ClassifiedActivity, ActivityType                                                                   |
| `frontend/components/latest-activities.tsx`  | Homepage stream: StreamItem\* components                                                                               |
| `frontend/components/activity-event-row.tsx` | Address page: ActivityEventGroup, TypeCallExpr, LockCallExpr, LockCallBadge                                            |

### Pipeline Integration

| File                               | Content                                  |
| ---------------------------------- | ---------------------------------------- |
| `crates/indexer/src/sync/batch.rs` | Bulk sync and live sync activity writing |
| `crates/indexer/src/sync/undo.rs`  | put_tx_actions/put_addr_tx undo wrappers |

### Documentation

| File                              | Content                                         |
| --------------------------------- | ----------------------------------------------- |
| `docs/prompts/ACTIVITY_DESIGN.md` | Three-layer design specification and principles |
| `docs/STORE_SCHEMA.md`            | Column family reference                         |
| `docs/script-name-overrides.json` | Protocol grouping data                          |
