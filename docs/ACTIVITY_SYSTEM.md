# Activity System Design

## Philosophy

Activities are **facts, not interpretations**. An activity records a per-owner position change in a single transaction: how much CKB moved, how much occupied capacity changed, which assets were affected, and who the counterparties were.

Previous attempts at an activity system tried to label transactions ("send", "receive", "swap") — inherently subjective and incomplete. This design avoids interpretation entirely. The raw position deltas are the truth; the frontend can layer interpretation on top if needed.

## Core Concepts

### What is an Activity?

For each transaction, every address (lock_script_hash) that appears as an input or output owner gets exactly one `ActivityEntry`. That entry captures:

| Field            | Meaning                                                         |
| ---------------- | --------------------------------------------------------------- |
| `ckb_delta`      | Net CKB change in shannons (`output_capacity - input_capacity`) |
| `occupied_delta` | Net occupied capacity change in shannons                        |
| `asset_changes`  | Token transfers, DAO operations, NFT/DOB mints/transfers/burns  |
| `peers`          | All OTHER lock_script_hashes in the same transaction            |
| `is_cellbase`    | Whether this is a mining reward transaction                     |

### Why `occupied_delta`?

CKByte has a dual nature: when a cell exists, part of its capacity is "occupied" (paying for storage), and the rest is "liquid". Tracking `occupied_delta` separately lets consumers distinguish:

- **Productive capital**: CKB locked to store data (scripts, UDT cells, NFTs)
- **Liquid asset**: Free CKB that can be transferred or deposited into DAO

For example, creating a UDT cell increases your `occupied_delta` (you're now paying for storage) while decreasing your liquid CKB — even if `ckb_delta` is zero.

### Why store ALL peers?

Financial ledger precision. If Alice sends CKB to Bob in a transaction that also involves Carol, all three are peers of each other. This enables:

- Transaction graph analysis
- Counterparty identification
- Flow tracking across addresses

## Data Model

### ActivityEntry (store type)

```rust
pub struct ActivityEntry {
    pub tx_hash: Vec<u8>,        // 32-byte transaction hash
    pub block_number: i64,
    pub tx_index: i32,
    pub timestamp: i64,          // Unix timestamp
    pub ckb_delta: i128,         // Net CKB change (shannons) — i128 for overflow safety
    pub occupied_delta: i64,     // Net occupied capacity change (shannons)
    pub is_cellbase: bool,
    pub asset_changes: Vec<AssetChange>,
    pub peers: Vec<Vec<u8>>,     // Lock hashes of other parties
}
```

### AssetChange (tagged enum)

```rust
pub enum AssetChange {
    Token {
        type_script_hash: Vec<u8>,
        delta: i128,             // Positive = received, negative = sent
        symbol: Option<String>,  // e.g. "SEAL"
        decimals: Option<u8>,    // e.g. 8
    },
    Dob {                        // Spore, did:ckb
        dob_id: Vec<u8>,
        standard: String,        // "spore", "did_ckb"
        action: AssetAction,     // Mint | Transfer | Burn
    },
    Nft {                        // mNFT, .bit
        nft_id: Vec<u8>,
        standard: String,        // "m-nft", "dotbit"
        action: AssetAction,
    },
    DaoDeposit { capacity: i64 },
    DaoWithdrawRequest { capacity: i64, deposit_block: i64 },
    DaoWithdrawComplete { capacity: i64, compensation: i64 },
}
```

### Asset detection

Assets are classified by matching `type_code_hash` against known code hashes:

| Asset Type    | Code Hash Source                              | Detection                                   |
| ------------- | --------------------------------------------- | ------------------------------------------- |
| sUDT          | `parser::udt::SUDT_CODE_HASH`                 | Parse 16-byte LE amount from cell data      |
| xUDT          | `parser::udt::XUDT_CODE_HASH_*`               | Same as sUDT                                |
| DAO           | `parser::dao::DAO_CODE_HASH`                  | data=0 → deposit; data>0 → withdraw request |
| Spore/DOB     | `parser::spore::SPORE_CODE_HASH_*`            | type_args = DOB ID                          |
| Spore Cluster | `parser::spore::CLUSTER_CODE_HASH_*`          | type_args = cluster ID                      |
| mNFT          | `parser::mnft::MNFT_TOKEN_CODE_HASH`          | type_args = NFT ID                          |
| .bit          | `parser::dotbit::DOTBIT_ACCOUNT_CELL_TYPE_ID` | type_args = account ID                      |

NFT/DOB action detection uses set comparison:

- ID in outputs only → **Mint**
- ID in both inputs and outputs → **Transfer**
- ID in inputs only → **Burn**

## Storage Schema

### Column Family

`CF_ACTIVITIES` — registered in `ALL_CFS` and `HIGH_WRITE_CFS` (large batch optimization tier).

### Key Encoding

```
lock_hash(32B) + block_num_desc(8B BE) + tx_idx(4B BE) = 44 bytes
```

`block_num_desc = i64::MAX - block_num` — descending order so prefix scan returns newest activities first.

```rust
pub fn encode_activity_key(lock_hash: &[u8], block_num: i64, tx_idx: i32) -> Vec<u8>;
pub fn decode_activity_key(key: &[u8]) -> (Vec<u8>, i64, i32);
```

### Value Encoding

`bincode::serialize(ActivityEntry)` — compact binary, fast to serialize/deserialize.

### Query Pattern

Prefix scan on `lock_hash[..32]`, forward iteration (which yields descending block order due to key encoding). Cursor-based pagination with `(block_num, tx_idx)` tuple.

```rust
impl CkbadgerStore {
    pub fn list_activities(
        &self,
        lock_hash: &[u8],
        limit: usize,
        cursor: Option<(i64, i32)>,
    ) -> Result<Vec<(i64, i32, ActivityEntry)>>;
}
```

### Rollback

During reorg, scan entire `CF_ACTIVITIES`, decode `block_num` from key bytes `[32..40]`, delete all entries where `block_num > rollback_to`. Follows the same pattern as other CF rollbacks in `reorg_ops.rs`.

## Activity Builder Algorithm

**File:** `crates/indexer/src/db/writer/activities.rs`

The builder is a pure function: given transaction data and pre-fetched cell info, it emits `(lock_hash, ActivityEntry)` pairs with no side effects.

### Per-transaction logic

```
1. For each input:
   - Group by lock_script_hash
   - Sum capacity, occupied_capacity
   - Classify by type_code_hash → accumulate UDT amounts, DOB/NFT IDs

2. For each output:
   - Group by lock_script_hash
   - Sum capacity, compute occupied_capacity from script sizes
   - Classify by type_code_hash → accumulate UDT amounts, DOB/NFT IDs, DAO ops

3. For each distinct lock_hash:
   - ckb_delta = Σ output_capacity - Σ input_capacity
   - occupied_delta = Σ output_occupied - Σ input_occupied
   - peers = all other lock_hashes in this transaction
   - Derive asset_changes from accumulated data
   - Emit (lock_hash, ActivityEntry)
```

### Occupied capacity formula

Computed once per cell at creation time, stored on `LiveCellInfo`:

```
occupied = (8 + lock_script_size + type_script_size + data_size) × 100_000_000

lock_script_size = 32 (code_hash) + 1 (hash_type) + lock_args.len()
type_script_size = if type_script { 32 + 1 + type_args.len() } else { 0 }
```

This matches CKB's cell capacity requirement calculation (see RFC-0022).

## Sync Pipeline Integration

### Bulk sync (parallel)

A dedicated thread `T_ACT` runs inside the existing `thread::scope`, writing only to `CF_ACTIVITIES` (no column family conflicts with T1–T7).

```
Thread scope:
  T1: Cells + consumption
  T2: Txs + addr deltas + script deltas
  T4: DAO
  T5: UDT/Token transfers
  T6: Spore/NFT data
  T7: Statistics
  T_ACT: Activities          ← NEW
```

T_ACT shares read access to `parsed_blocks`, `input_cell_info`, and `token_info_cache` — all pre-fetched before the thread scope begins.

### Live sync (serial)

Activity writes are added to the existing serial `StoreBatch` after DAO, UDT, and Spore writes, before `batch.commit()`.

### Reorg handling

`store.rollback_activities(rollback_to)` is called alongside existing rollback calls during chain reorganization.

## API Endpoint

### `GET /addresses/{addr}/activities`

| Parameter | Type  | Default  | Description                                                         |
| --------- | ----- | -------- | ------------------------------------------------------------------- |
| `addr`    | path  | required | CKB address (`ckb1...`/`ckt1...`) or `0x`-prefixed lock_script_hash |
| `limit`   | query | 20       | Results per page (1–100)                                            |
| `cursor`  | query | none     | Pagination cursor: `"block_num:tx_idx"`                             |

### Response

```json
{
  "data": [
    {
      "txHash": "0x...",
      "blockNumber": 12345,
      "txIndex": 0,
      "timestamp": "1700000000",
      "ckbDelta": "-10000000000",
      "occupiedDelta": "6100000000",
      "isCellbase": false,
      "assetChanges": [
        {
          "type": "token",
          "typeScriptHash": "0x...",
          "delta": "1000000",
          "symbol": "SEAL",
          "decimals": 8
        },
        {
          "type": "dob",
          "dobId": "0x...",
          "standard": "spore",
          "action": "mint"
        },
        {
          "type": "daoDeposit",
          "capacity": "50000000000"
        }
      ],
      "peers": ["0x..."]
    }
  ],
  "meta": {
    "limit": 20,
    "nextCursor": "12345:0"
  }
}
```

Notes:

- `ckbDelta` and `occupiedDelta` are stringified i128/i64 (shannons) to avoid JSON number precision loss
- `peers` are hex-encoded lock_script_hashes (not addresses — address encoding requires chain context)
- `assetChanges` uses internally tagged union (`"type": "token"`)
- `timestamp` is stringified Unix seconds

## File Map

| File                                  | Role                                                                                             |
| ------------------------------------- | ------------------------------------------------------------------------------------------------ |
| `ckbadger-store/src/types.rs`         | `ActivityEntry`, `AssetChange`, `AssetAction` types; `occupied_capacity` field on `LiveCellInfo` |
| `ckbadger-store/src/store.rs`         | `CF_ACTIVITIES` constant, `cf_activities()` accessor                                             |
| `ckbadger-store/src/keys.rs`          | `encode_activity_key`, `decode_activity_key`                                                     |
| `ckbadger-store/src/batch.rs`         | `put_activity` batch write method                                                                |
| `ckbadger-store/src/activity_ops.rs`  | `list_activities` cursor-paginated query                                                         |
| `ckbadger-store/src/reorg_ops.rs`     | Activity rollback during reorg                                                                   |
| `indexer/src/db/writer/activities.rs` | `build_activities_for_block` — pure builder function                                             |
| `indexer/src/db/writer/cells.rs`      | Computes `occupied_capacity` at cell insertion                                                   |
| `indexer/src/sync/indexer.rs`         | T_ACT thread (bulk), serial writes (live), rollback call                                         |
| `api/src/routes/activities.rs`        | HTTP endpoint and JSON response types                                                            |

## Design Decisions

| Decision                                   | Rationale                                                                                                                |
| ------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ |
| Compute during sync, not lazily            | Activities need consumed cell info that's only available at sync time; lazy recomputation would require replaying blocks |
| Store all peers                            | Financial ledger precision; filtering can happen at query time                                                           |
| Track `occupied_delta`                     | Distinguishes productive capital from liquid CKB — essential for understanding CKB's dual-nature economy                 |
| No deferred writes during bulk sync        | Simplicity first; can optimize later if T_ACT becomes a bottleneck                                                       |
| `i128` for `ckb_delta`                     | A transaction can move more CKB than fits in `i64` when subtracting large inputs from large outputs                      |
| Descending key order                       | Most common query is "recent activities" — descending block_num avoids reverse iteration                                 |
| Bincode serialization                      | Matches all other CFs in the store; compact and fast                                                                     |
| `#[serde(default)]` on `occupied_capacity` | Backward compatibility with existing serialized `LiveCellInfo` data                                                      |

---

_Last updated: 2026-02-14_
