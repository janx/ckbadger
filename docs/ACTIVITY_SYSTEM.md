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

Activities are **interpretations, not facts**. A CKB transaction is an atomic UTXO bundle that can involve multiple parties and asset types simultaneously. The activity system interprets each transaction as a set of per-owner position changes: how much CKB moved, how much occupied capacity changed, which assets were affected, and who the counterparties were.

This is deliberately the simplest useful interpretation. More sophisticated systems could interpret two owners' position changes as a "swap" or "liquidity provision". Since UTXO transactions are atomic action bundles, the combination possibilities are endless — the current system captures the building blocks.

## Architecture Overview

```
Fetcher (RPC)  →  Parser (CPU)  →  Writer (DB)  →  API (read)  →  Frontend (display)
                                       │
                        ┌───────────────┼───────────────┐
                        ▼               ▼               ▼
                  CF_ACTIVITIES   CF_ADDR_TXS    CF_STATS_CHAIN
                  (bundles)       (address idx)  (daily/hourly)
```

**Write path**: The indexer pipeline builds `TxActivityBundle` per transaction, writes bundles to `CF_ACTIVITIES`, thin index entries to `CF_ADDR_TXS`, and accumulates stats into `CF_STATS_CHAIN`.

**Read path**: The API reads bundles from `CF_ACTIVITIES`, resolves per-owner `ActivityEntry` from bundles, validates canonicality, and returns paginated JSON. The frontend classifies and renders.

## Data Structures

### TxActivityBundle — Canonical Storage Type

One bundle per canonical transaction. Contains all owner position changes.

```rust
// crates/ckbadger-store/src/types.rs:1029-1037
pub struct TxActivityBundle {
    pub tx_hash: Vec<u8>,                    // 32-byte tx hash
    pub block_hash: Vec<u8>,                 // 32-byte block hash
    pub block_number: i64,
    pub tx_index: i32,
    pub timestamp: i64,                      // Unix epoch seconds
    pub is_cellbase: bool,
    pub owners: Vec<OwnerActivityDelta>,     // All owners in this tx, sorted by lock_hash
}
```

### OwnerActivityDelta — Per-Owner Position Change

```rust
// crates/ckbadger-store/src/types.rs:1014-1026
pub struct OwnerActivityDelta {
    pub lock_hash: Vec<u8>,                  // 32-byte lock script hash (owner identity)
    pub lock_code_hash: Vec<u8>,             // Lock script code hash
    pub lock_hash_type: i16,                 // Script hash type
    pub lock_args: Vec<u8>,                  // Lock script args
    pub ckb_delta: i128,                     // Net CKB change (shannons) — i128 for overflow safety
    pub used_delta: i64,                     // Net occupied capacity change (shannons)
    pub has_type_script: bool,               // Whether any cell had a type script
    pub involved_script_code_hashes: Vec<Vec<u8>>,  // All lock + type code hashes seen
    pub asset_changes: Vec<AssetChange>,     // Classified asset changes
    pub type_calls: Option<Vec<TypeCallEntry>>, // Unrecognized type scripts
    pub lock_calls: Option<Vec<LockCallEntry>>, // Non-standard lock scripts
    pub protocol_actions: Vec<ProtocolAction>,   // Detected protocol-level actions
    pub peers: Vec<Vec<u8>>,                 // Lock hashes of ALL other parties
}
```

**Key design decisions:**

- **`ckb_delta` is i128**: CKB amounts are u64 shannons, but deltas can overflow i64 when a single owner has massive input/output imbalance across many cells.
- **`has_type_script`**: Distinguishes pure CKB transfers from transactions involving smart contracts. A tx with `has_type_script=true` but empty `asset_changes` is classified as "Unknown" (unrecognized contract).
- **`peers` captures ALL counterparties**: If Alice sends CKB to Bob in a tx that also involves Carol, all three are peers of each other. This enables transaction graph analysis and counterparty identification.
- **`type_calls` is `Option<Vec<>>`**: `None` when no unrecognized type scripts; `Some(vec![])` is not used (empty vec is semantically identical to None but wastes space).
- **`lock_calls` is `Option<Vec<>>`**: `None` when no non-standard lock scripts; same semantics as `type_calls`.

### ActivityEntry — Read-Model Helper

Materialized on-the-fly from bundle + owner delta. Not stored directly — constructed by the API when resolving a specific owner's view of a transaction.

```rust
// crates/ckbadger-store/src/types.rs:985-1004
pub struct ActivityEntry {
    pub tx_hash: Vec<u8>,
    pub block_hash: Vec<u8>,
    pub block_number: i64,
    pub tx_index: i32,
    pub timestamp: i64,
    pub ckb_delta: i128,
    pub used_delta: i64,
    pub is_cellbase: bool,
    pub has_type_script: bool,
    pub asset_changes: Vec<AssetChange>,
    pub type_calls: Option<Vec<TypeCallEntry>>,
    pub lock_calls: Option<Vec<LockCallEntry>>,
    pub protocol_actions: Vec<ProtocolAction>,
    pub peers: Vec<Vec<u8>>,
}
```

### AssetChange — Tagged Enum

```rust
// crates/ckbadger-store/src/types.rs:1051-1079
pub enum AssetChange {
    Token {
        type_script_hash: Vec<u8>,       // Identifies the specific UDT
        delta: i128,                     // Positive = received, negative = sent
        symbol: Option<String>,          // e.g., "SEAL", "RUSD"
        decimals: Option<u8>,            // e.g., 8
    },
    Object {                             // Spore, mNFT
        object_id: Vec<u8>,
        standard: String,                // "spore", "m-nft"
        action: AssetAction,             // Mint | Transfer | Burn
    },
    Identity {                           // .bit, did:ckb
        identity_id: Vec<u8>,
        standard: String,                // "dotbit", "did_ckb"
        action: AssetAction,             // Mint | Transfer | Burn | Recycle | Renew | Update
    },
    DaoDeposit { capacity: i64 },
    DaoWithdrawRequest { capacity: i64, deposit_block: i64 },
    DaoWithdrawComplete { capacity: i64, compensation: i64 },
}
```

### AssetAction — NFT/Identity Action Enum

```rust
// crates/ckbadger-store/src/types.rs:1082-1092
pub enum AssetAction {
    Mint,       // ID only in outputs
    Transfer,   // ID in both inputs and outputs
    Burn,       // ID only in inputs
    Recycle,    // .bit: expired account removed
    Renew,      // .bit: expiry extended
    Update,     // .bit: metadata changed
}
```

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

### ProtocolAction — Cross-Layer Protocol Detection

Protocol actions capture high-level protocol behaviors detected from cross-layer signals (e.g., lock script patterns, cell structure). Unlike asset changes which are derived from type scripts, protocol actions are detected by `ProtocolDetector` implementations that analyze the full transaction context.

```rust
// crates/ckbadger-store/src/types.rs
pub struct ProtocolAction {
    pub protocol: String,       // e.g., "rgbpp"
    pub action: String,         // e.g., "leap_to_ckb", "transfer", "btc_time_locked"
    pub metadata: serde_json::Value,  // Protocol-specific details (e.g., btc_txid)
}
```

The `protocol_actions` field appears on both `OwnerActivityDelta` (write path) and `ActivityEntry` (read path).

### LatestActivityItem — Global Feed Item

Wraps `ActivityEntry` with lock script context for CKB address resolution.

```rust
// crates/ckbadger-store/src/types.rs:1042-1048
pub struct LatestActivityItem {
    pub lock_hash: Vec<u8>,
    pub lock_code_hash: Vec<u8>,
    pub lock_hash_type: i16,
    pub lock_args: Vec<u8>,
    pub entry: ActivityEntry,
}
```

### DailyActivityStats — Aggregation Counters

```rust
// crates/ckbadger-store/src/types.rs:1099-1130
pub struct DailyActivityStats {
    pub transfer_count: u32,              // Pure CKB transfers
    pub dao_deposit_count: u32,
    pub dao_withdraw_request_count: u32,
    pub dao_withdraw_complete_count: u32,
    pub token_count: u32,                 // UDT transfers
    pub object_count: u32,                // Spore/mNFT
    pub identity_count: u32,              // .bit/did:ckb
    pub script_call_count: u32,           // Unrecognized scripts
    pub unknown_count: u32,               // has_type_script but no asset (should be 0)
    pub coinbase_count: u32,              // Mining rewards
    pub unique_address_count: u32,        // Distinct lock_hashes
    pub total_ckb_moved: u128,            // Sum of |ckb_delta| across all owners
    pub script_counts: HashMap<String, u32>,  // Per-code_hash counts (hex string keys)
    pub protocol_action_counts: HashMap<String, u32>,  // Per-protocol action counts (e.g., "rgbpp" => 5)
}
```

### ObjectCollectionActivityEntry — Pre-Computed Collection Feed

```rust
// crates/ckbadger-store/src/types.rs:1137-1143
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

The builder is a **pure function**: given transaction data and pre-fetched cell info, it emits one `TxActivityBundle` per transaction with no side effects. Owners within a bundle are sorted deterministically by `lock_hash`.

### Entry Point

```rust
// activities.rs:130-138
pub fn build_activity_bundles_for_block(
    txs: &[TxView<'_>],
    token_info_cache: &HashMap<Vec<u8>, (Option<String>, Option<u8>)>,
) -> Vec<TxActivityBundle>
```

**Parameters:**

- `txs`: All transactions in a block, with parsed cell data
- `token_info_cache`: Pre-fetched UDT symbol/decimals by type_script_hash

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
   - If NOT in standard_locks → record as lock call for the cell's owner
   - Stored in `unrecognized_lock_calls: BTreeSet<(code_hash, hash_type, args)>`
   - BTreeSet deduplicates identical lock scripts within a single owner

4. Build per-owner deltas (sorted by lock_hash):
   - ckb_delta = Σ output_capacity - Σ input_capacity
   - used_delta = Σ output_occupied - Σ input_occupied
   - peers = all other lock_hashes in this transaction
   - Derive asset_changes from accumulated data:
     • UDT: delta = output_amount - input_amount per type_script_hash
     • DAO deposit: output with DAO type and data == 0
     • DAO withdraw request: output with DAO type and deposit_block > 0
     • DAO withdraw complete: input with is_dao_withdraw_request flag
     • Object/Identity: set comparison (input IDs vs output IDs → mint/transfer/burn)

5. Wrap all OwnerActivityDelta entries into single TxActivityBundle
```

### Classification Functions

**`classify_input()`**: Processes input cell type script.

- Matches code_hash against `CodeHashes` type_scripts lookup
- DAO: if `is_dao_withdraw_request`, record in `dao_withdraw_completes`
- UDT: parse amount from `udt_amount` field (pre-fetched) or cell data via `UdtParser::parse_amount()`
- Spore/did:ckb: extract type_args as object/identity ID
- Unrecognized: call `record_type_call()` → stored in `type_calls`

**`classify_output()`**: Processes output cell type script.

- DAO: data == `[0u8; 8]` → deposit; data decodes to non-zero deposit_block → withdraw request
- UDT: parse amount from output cell data
- Spore/did:ckb: extract type_args as ID
- Unrecognized: call `record_type_call()`

**`emit_object_changes()`** (line 656): Set comparison for Object assets.

- ID in outputs only → Mint
- ID in both inputs and outputs → Transfer
- ID in inputs only → Burn

**`emit_identity_changes()`** (line 690): Same logic for Identity assets.

### UDT Amount Parsing

For inputs, the builder uses `InputCellView.udt_amount` (pre-fetched during sync from the append-only cell store) or falls back to parsing cell data. For outputs, amounts are parsed directly from `ParsedCell.data` via `UdtParser::parse_amount()` (first 16 bytes as little-endian u128).

---

## Storage Layer

**Files**: `crates/ckbadger-store/src/activity_ops.rs`, `keys.rs`, `batch.rs`

All activity storage is in the **domain store** (mutable, supports delete on rollback). Nothing in append-only.

### Column Families

| CF                                  | Key Size  | Value                           | Purpose                       |
| ----------------------------------- | --------- | ------------------------------- | ----------------------------- |
| `CF_ACTIVITIES`                     | 44 bytes  | `TxActivityBundle` (bincode)    | Per-tx activity bundle        |
| `CF_ADDR_TXS`                       | 76 bytes  | empty                           | Address → tx index            |
| `CF_OBJECT_COLLECTION_ACTIVITIES`   | 108 bytes | `ObjectCollectionActivityEntry` | Spore/mNFT collection feeds   |
| `CF_IDENTITY_COLLECTION_ACTIVITIES` | 108 bytes | `ObjectCollectionActivityEntry` | .bit/did:ckb collection feeds |
| `CF_STATS_CHAIN` (prefixed)         | variable  | `DailyActivityStats` (bincode)  | Hourly/daily aggregation      |

`CF_ACTIVITIES` is registered in `HIGH_WRITE_CFS` for large batch optimization.

### Key Encoding — CF_ACTIVITIES

```
block_num_desc(8B BE) + tx_idx_desc(4B BE) + tx_hash(32B) = 44 bytes

block_num_desc = i64::MAX - block_num
tx_idx_desc   = i32::MAX - tx_idx
```

**Descending order**: Forward RocksDB iteration yields newest transactions first. This enables efficient "latest N" queries and reorg cleanup (newest entries are encountered first during scan).

```rust
// keys.rs:905-940
pub fn encode_tx_activity_bundle_key(block_num: i64, tx_idx: i32, tx_hash: &[u8]) -> Vec<u8>;
pub fn decode_tx_activity_bundle_key(key: &[u8]) -> (i64, i32, Vec<u8>);
pub fn encode_tx_activity_bundle_seek_after_key(block_num: i64, tx_idx: i32) -> Vec<u8>;
```

The seek-after key pads tx_hash with `0xFF` bytes for cursor-based pagination positioning.

### Key Encoding — CF_ADDR_TXS

```
lock_hash(32B) + block_num_desc(8B BE) + tx_idx_desc(4B BE) + tx_hash(32B) = 76 bytes
```

**Prefix scan property**: Prefix by `lock_hash` lists all transactions for an address in descending order.

```rust
// keys.rs:174-227
pub fn encode_addr_tx_key(lock_hash: &[u8], block_num: i64, tx_idx: i32, tx_hash: &[u8]) -> Vec<u8>;
pub fn decode_addr_tx_key(key: &[u8]) -> (Vec<u8>, i64, i32, Vec<u8>);
pub fn encode_addr_tx_seek_after_key(lock_hash: &[u8], block_num: i64, tx_idx: i32) -> Vec<u8>;
```

### Key Encoding — Collection Activities

```
collection_id(32B padded) + block_num_desc(8B) + tx_idx_desc(4B) + block_hash(32B) + tx_hash(32B) = 108 bytes
```

### Value Encoding

All values use `bincode::serialize()` — compact binary, fast to serialize/deserialize.

### Query Operations

**`list_tx_activity_bundles_recent(limit, cursor)`**: Forward scan on `CF_ACTIVITIES`, returns newest bundles first.

**`get_latest_activities()`**: Scans `CF_ACTIVITIES`, expands each bundle's owners into `LatestActivityItem` entries, skips cellbase transactions, returns up to 64 items.

**`list_activities(lock_hash, limit, cursor, filter)`**: Scans `CF_ADDR_TXS` by lock_hash prefix, multi-gets bundles from `CF_ACTIVITIES`, resolves per-owner entry, applies filter.

### Write Operations

```rust
// batch.rs:972-980
StoreBatch::put_tx_activity_bundle(&mut self, bundle: &TxActivityBundle)
StoreBatch::put_addr_tx(&mut self, lock_hash: &[u8], block_num: i64, tx_idx: i32, tx_hash: &[u8])
```

### Activity Filter Matching

```rust
// activity_ops.rs:300-326
fn matches_activity_filter(entry: &ActivityEntry, filter: Option<&str>) -> bool
```

| Filter               | Condition                                                              |
| -------------------- | ---------------------------------------------------------------------- |
| `"ckb"`              | `asset_changes.is_empty() && !has_type_script`                         |
| `"token"`            | Has `AssetChange::Token`                                               |
| `"nft"` / `"object"` | Has `AssetChange::Object`                                              |
| `"dao"`              | Has DAO asset change variant                                           |
| `"type_call"`        | Has `type_calls` with entries                                          |
| `"lock_call"`        | Has `lock_calls` with entries                                          |
| `"protocol:*"`       | Has `protocol_actions` matching protocol name (e.g., `protocol:rgbpp`) |
| `None` / `"all"`     | Always matches                                                         |

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

Response: `Vec<GlobalActivityResponse>` with `address` field (CKB address computed from lock script).

### Response Types

```rust
pub struct ActivityResponse {
    pub tx_hash: String,
    pub block_number: i64,
    pub tx_index: i32,
    pub timestamp: String,
    pub ckb_delta: String,          // String for i128 precision
    pub used_delta: String,
    pub is_cellbase: bool,
    pub asset_changes: Vec<AssetChangeResponse>,
    pub type_calls: Vec<TypeCallResponse>,
    pub lock_calls: Vec<LockCallResponse>,
    pub protocol_actions: Vec<ProtocolActionResponse>,
    pub peers: Vec<String>,         // CKB addresses
}

pub struct GlobalActivityResponse {
    pub address: String,            // CKB address of the activity owner
    // ...same fields as ActivityResponse
}

pub struct TypeCallResponse {
    pub type_code_hash: String,
    pub type_hash_type: String,
    pub type_args: String,
    pub script_hash: String,        // Computed from code_hash + hash_type + args
    pub script_name: Option<String>,    // Resolved from ScriptInfo DB
    pub protocol_name: Option<String>,  // Resolved from PROTOCOL_INDEX
}

pub struct LockCallResponse {
    pub lock_code_hash: String,
    pub lock_hash_type: String,
    pub lock_args: String,
    pub script_hash: String,        // Computed from code_hash + hash_type + args
    pub script_name: Option<String>,    // Resolved from ScriptInfo DB
    pub role: String,               // "protocol_action" or "access_control"
    pub decoded: Option<Value>,     // Decoded args (e.g., RGB++ btcTxid)
}

pub struct ProtocolActionResponse {
    pub protocol: String,           // e.g., "rgbpp"
    pub action: String,             // e.g., "leap_to_ckb", "transfer"
    pub metadata: Value,            // Protocol-specific details
}
```

### Lock Call Role Classification

The API classifies each lock call's role at response time:

- **`protocol_action`**: Lock code_hash is in `PROTOCOL_ACTION_LOCKS` set — creating a cell with this lock IS a protocol state change (e.g., UTXOSwap intent lock, Fiber funding lock)
- **`access_control`**: All other non-standard locks — the lock defines who can spend, but interacting with it is not itself a protocol action (e.g., RGB++ lock)

`PROTOCOL_ACTION_LOCKS` is currently an empty `HashSet` (UTXOSwap/Fiber code_hashes not yet publicly available). All non-standard locks currently receive `access_control` role.

### Lock Args Decoder Registry

`LOCK_ARGS_DECODERS` is a `LazyLock<HashMap<Vec<u8>, fn(&[u8]) -> Option<Value>>>` that maps lock code_hashes to decoder functions. Currently registered decoders:

| Lock Script   | Decoder Output                                                     |
| ------------- | ------------------------------------------------------------------ |
| RGB++ Lock    | `{"protocol": "rgbpp", "btcTxid": "...", "outIndex": N}`           |
| BTC Time Lock | `{"protocol": "rgbpp", "action": "btcTimeLock", "btcTxid": "..."}` |

**RGB++ decoder**: args[0..4] = out_index (u32 LE), args[4..36] = btc_txid (32 bytes LE, reversed for display). Minimum 36 bytes.

**BTC Time Lock decoder**: btc_txid is the last 32 bytes of args (matching `RgbppParser::extract_btc_txid_from_btc_time_lock_args` in the indexer). Minimum 36 bytes.

`AssetChangeResponse` is a tagged enum serialized with `#[serde(tag = "type")]`:

- `"token"` — `typeScriptHash`, `delta`, `symbol`, `decimals`
- `"object"` — `objectId`, `standard`, `action`
- `"identity"` — `identityId`, `standard`, `action`
- `"daoDeposit"` — `capacity`
- `"daoWithdrawRequest"` — `capacity`, `depositBlock`
- `"daoWithdrawComplete"` — `capacity`, `compensation`

### Canonicality Validation

The API validates that each activity entry matches the canonical chain position. This is necessary because during reorgs, activity entries from orphaned blocks may linger until rollback completes.

```rust
// activities.rs:384-403
fn canonical_activity_locations(store, rows) -> HashMap<tx_hash → (block_num, tx_idx, block_hash)>
```

Each entry is checked: `entry.block_number == canonical_block_num && entry.tx_index == canonical_tx_idx && entry.block_hash == canonical_block_hash`. Non-matching entries are silently dropped.

The `list_canonical_activities_page()` function implements a loop that scans `ACTIVITY_SCAN_CHUNK_SIZE` entries at a time, filters orphans, and continues scanning until `limit` canonical entries are found or the address runs out of entries.

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

The frontend re-classifies each activity for display purposes. This is independent of the backend stats classification — the frontend uses a priority-based single-type classification:

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
  type: ActivityType;
  activity: GlobalActivity;
  primaryAssetChange: ActivityAssetChange | null;
  primaryTypeCall: ActivityTypeCall | null;
  primaryLockCall: ActivityLockCall | null;
}
```

**Priority order** (highest to lowest):

1. Protocol actions (`protocol_actions` non-empty)
2. DAO changes (deposit, withdraw request, withdraw complete)
3. Token changes
4. Object changes (Spore, mNFT)
5. Identity changes (.bit, did:ckb)
6. Protocol action lock calls (`role === 'protocol_action'`)
7. Type calls (unrecognized type scripts)
8. CKB transfer (fallback)

The `primaryAssetChange` is the first matching asset in priority order. The `primaryTypeCall` is the first type call (if any). The `primaryLockCall` is always the first lock call (if any). These are used for the homepage stream display where only one badge per activity is shown.

**Protocol action vs access control display rule**: A protocol action lock call with no asset change → classified as `protocolAction` (independent activity type). A protocol action lock call with an asset change → asset classification takes priority, lock shown as badge. Access control locks → always shown as badge only.

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

The `ActivityEventGroup` component renders all events for a single transaction as a group. Unlike the homepage (one badge per activity), the address page shows ALL asset changes, type calls, lock calls, and the CKB delta as separate sub-rows within one tx group.

**Layout:**

- **Narrow** (<md): Stacked card with tx hash, block number, time, and events
- **Wide** (≥md): Grid cells with 4 columns: tx info, badge, value, time

**Event parts** are generated by helper functions:

- `getAssetEventParts(change)` — Badge + value for each asset change
- `getTypeEventParts(sc)` — Badge + value for each type call
- `getLockEventParts(lc)` — Badge + value for each lock call
- `getCkbEventParts(delta, isCellbase)` — Badge + value for CKB delta (always present)

The CKB event is always the last sub-row. A single tx group may show: DAO Deposit + Token Transfer + Type Call + Lock Call + CKB Transfer — all as separate rows.

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

**File**: `crates/indexer/src/sync/batch.rs` (around line 2860)

During bulk sync, activities are written in a dedicated parallel thread (`T_ACT_activities`):

1. Build `TxView` from parsed block data + prefetched input cells
2. Call `build_activity_bundles_for_block()` with token info cache
3. For each bundle, accumulate daily/hourly stats, then `activity_batch.put_tx_activity_bundle()`
4. Write `addr_tx` entries in the main domain batch (around line 1834)
5. Commit activity batch with `commit_phase_no_wal()` (WAL disabled for bulk)

Bulk sync skips the undo journal (per BULK_SYNC.md rules 5-7). Activities go directly to `StoreBatch::put_tx_activity_bundle()` and `StoreBatch::put_addr_tx()`.

### Live Sync Path

**File**: `crates/indexer/src/sync/batch.rs` (around line 4127)

During live sync, activities are written through the undo helper:

1. Same `build_activity_bundles_for_block()` call
2. Same stats accumulation
3. `put_tx_activity_bundle()` via `sync::undo::put_tx_activity_bundle()` — records to domain batch (no undo log needed since rollback deletes directly)
4. `put_addr_tx()` via `sync::undo::put_addr_tx()` — same pattern

The undo helpers in `sync/undo.rs` wrap `StoreBatch` writes. Activities and addr_txs are in the domain store, so rollback deletes entries directly rather than using an undo log.

### Token Info Cache

The activity builder receives a `HashMap<Vec<u8>, (Option<String>, Option<u8>)>` mapping type_script_hash → (symbol, decimals). This is pre-fetched before activity building so the builder can enrich `AssetChange::Token` entries with symbol and decimals without DB access during the pure computation phase.

---

## Reorg Handling

**File**: `crates/ckbadger-store/src/reorg_ops.rs` (lines 1240-1391)

Activities use **direct deletion** during reorg rollback — no ghost entries, no canonical filtering needed.

### Rollback Steps

**Stage 8b — Delete activity bundles** (lines 1240-1267):
Scan `CF_ACTIVITIES` from start. Since keys are in descending block_num order, newest entries come first. Delete all entries where `block_num > rollback_to`. Break when `block_num <= rollback_to` (all remaining entries are valid).

**Stage 8c — Delete addr_txs** (lines 1270-1297):
Full scan of `CF_ADDR_TXS`. Extract block_num from each key. Delete entries where `block_num > rollback_to`. Cannot break early because addr_txs keys are prefixed by lock_hash (entries from different addresses are interleaved).

**Stage 8d/8e — Delete collection activities** (lines 1299-1391):
Same pattern for `CF_OBJECT_COLLECTION_ACTIVITIES` and `CF_IDENTITY_COLLECTION_ACTIVITIES`. Also counts surviving entries per collection for aggregate rebuild.

### Key Invariants

- Keys in descending block_num order allow efficient early termination for `CF_ACTIVITIES`
- After rollback, the domain store contains only canonical entries — no secondary cleanup needed
- The API's canonicality check (`canonical_activity_locations()`) provides a safety net during the brief window between chain reorg detection and rollback completion

---

## Statistics Aggregation

**File**: `crates/indexer/src/db/writer/statistics.rs`

Activity stats are accumulated per-owner and aggregated at two time granularities.

### Classification for Stats

```rust
// statistics.rs:35-115
fn accumulate_activity_stats_inner(is_cellbase, ckb_delta, has_type_script, asset_changes, type_calls, scripts, stats)
```

Priority-based exclusive classification (one owner counted once):

1. **Coinbase** — counted separately, excluded from all other metrics
2. **DAO** — if any DAO asset change (deposit, withdraw request, withdraw complete)
3. **Token** — if any `AssetChange::Token`
4. **Object** — if any `AssetChange::Object`
5. **Identity** — if any `AssetChange::Identity`
6. **Script call** — if `type_calls` is non-empty
7. **Transfer** — if no asset changes AND `!has_type_script` (pure CKB)
8. **Unknown** — if no asset changes AND `has_type_script` (fallback, should be 0)

**Note**: Unlike the frontend classification, the stats classification counts DAO sub-types separately (deposit, withdraw request, withdraw complete each get their own counter) while the overall classification only sets `has_dao` once. Token, object, identity, and script_call get boolean flags — an owner with 3 token changes is counted once in `token_count`. Lock calls are not currently tracked in stats aggregation.

### Additional Metrics

- **`total_ckb_moved`**: Sum of `|ckb_delta|` across all non-cellbase owners (u128)
- **`unique_address_count`**: Distinct lock_hashes per day/hour (computed from HashSet of `[u8; 32]`)
- **`script_counts`**: Per-code_hash activity counts (hex string keys → u32 counts)

### Storage

- **Daily**: Key prefix `0x1D` + `YYYYMMDD` in `CF_STATS_CHAIN`
- **Hourly**: Key prefix `0x1E` + `YYYYMMDDHH` in `CF_STATS_CHAIN`

Stats are accumulated in memory during batch processing, then merged with existing values using `update_daily_activity_stats()` / `update_hourly_activity_stats()`. The merge reads existing stats, adds the accumulated values, and writes back — this is necessary because a single day/hour may span multiple batches.

### API Endpoints

- **`GET /stats/daily-activities`** — Returns daily activity stats for charting (list of day-level breakdowns)
- **`GET /stats/activity-summary-24h`** — Aggregates all hourly buckets within a 24h rolling window for the summary card

---

## Protocol Grouping

Added to support grouping type calls by protocol (e.g., "Stable++", "Godwoken").

### Data Source

**File**: `docs/script-name-overrides.json`

The `protocols` field maps protocol names to arrays of code_hashes:

```json
{
  "protocols": {
    "Stable++": ["0x26a33e...", "0x56fb63...", "0x266221...", "0xff3520..."],
    "Godwoken": ["0x628b5f...", "0x000f87...", ...]
  }
}
```

### Backend — Reverse Index

**File**: `crates/api/src/routes/activities.rs`

`PROTOCOL_INDEX` is a `LazyLock<HashMap<Vec<u8>, String>>` built from `docs/script-name-overrides.json` at first access. It maps code_hash bytes → protocol name. Used in `convert_type_call()` to populate `protocol_name` on `TypeCallResponse`.

### Frontend — Protocol-Aware Display

**Homepage** (`StreamItemTypeCall`): When `protocolName` exists, shows protocol name as amber prefix with separator:

```
⚙ Stable++ · Pool(0xab12...cd34)
```

Falls back to `⚙ Type call Pool(0xab12...cd34)` when no protocol.

**Address page** (`getTypeEventParts`): Same pattern — protocol name replaces "Type call" in the badge.

---

## File Reference

### Core Data Structures

| File                                 | Content                                                                                                                                         |
| ------------------------------------ | ----------------------------------------------------------------------------------------------------------------------------------------------- |
| `crates/ckbadger-store/src/types.rs` | ActivityEntry, TxActivityBundle, OwnerActivityDelta, AssetChange, TypeCallEntry, LockCallEntry, ProtocolAction, DailyActivityStats, AssetAction |

### Activity Builder

| File                                             | Content                                                                                             |
| ------------------------------------------------ | --------------------------------------------------------------------------------------------------- |
| `crates/indexer/src/db/writer/activities.rs`     | build_activity_bundles_for_block(), CodeHashes, classify_input/output, emit_object/identity_changes |
| `crates/indexer/src/db/writer/rgbpp_detector.rs` | RgbppDetector: ProtocolDetector impl (leap_to_ckb, leap_to_btc, transfer, btc_time_locked, receive) |
| `crates/indexer/build.rs`                        | Compile-time extraction of xudt_compatible code_hashes                                              |

### Storage

| File                                        | Content                                                               |
| ------------------------------------------- | --------------------------------------------------------------------- |
| `crates/ckbadger-store/src/activity_ops.rs` | list_activities(), get_latest_activities(), matches_activity_filter() |
| `crates/ckbadger-store/src/keys.rs`         | Key encoding/decoding for CF_ACTIVITIES (44B), CF_ADDR_TXS (76B)      |
| `crates/ckbadger-store/src/batch.rs`        | put_tx_activity_bundle(), put_addr_tx()                               |
| `crates/ckbadger-store/src/reorg_ops.rs`    | Activity rollback (stages 8b-8e)                                      |

### API

| File                                  | Content                                                                                                       |
| ------------------------------------- | ------------------------------------------------------------------------------------------------------------- |
| `crates/api/src/routes/activities.rs` | Endpoints, response types, canonicality validation, PROTOCOL_INDEX, LOCK_ARGS_DECODERS, PROTOCOL_ACTION_LOCKS |

### Statistics

| File                                         | Content                                                                 |
| -------------------------------------------- | ----------------------------------------------------------------------- |
| `crates/indexer/src/db/writer/statistics.rs` | accumulate_owner_activity_stats(), update_daily/hourly_activity_stats() |

### Frontend

| File                                         | Content                                                                                    |
| -------------------------------------------- | ------------------------------------------------------------------------------------------ |
| `frontend/lib/api.ts`                        | ActivityTypeCall, ActivityLockCall, ActivityProtocolAction, Activity, GlobalActivity types |
| `frontend/lib/activity-classify.ts`          | classifyActivity(), ClassifiedActivity, ActivityType                                       |
| `frontend/components/latest-activities.tsx`  | Homepage stream: StreamItem\* components                                                   |
| `frontend/components/activity-event-row.tsx` | Address page: ActivityEventGroup, TypeCallExpr, LockCallExpr, LockCallBadge                |

### Pipeline Integration

| File                               | Content                                                  |
| ---------------------------------- | -------------------------------------------------------- |
| `crates/indexer/src/sync/batch.rs` | Bulk sync (~2860) and live sync (~4127) activity writing |
| `crates/indexer/src/sync/undo.rs`  | put_tx_activity_bundle/put_addr_tx undo wrappers         |

### Documentation

| File                              | Content                             |
| --------------------------------- | ----------------------------------- |
| `docs/prompts/ACTIVITY_DESIGN.md` | Design specification and principles |
| `docs/STORE_SCHEMA.md`            | Column family reference             |
| `docs/script-name-overrides.json` | Protocol grouping data              |
