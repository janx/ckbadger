# Activity System Design

## Philosophy

Activities are **interpretations, not facts**. A simple form of activity is the interpretation of a per-owner position change in a single transaction: how much CKB moved, how much occupied capacity changed, which assets were affected, and who the counterparties were.

More sophisticated activity systems may interpret two owners' position changes in a single transaction as a single 'swap' activity rather than two separate activities. Since UTXO transactions are atomic action bundles involving multiple parties, the combination possibilities and thus possible interpretations are endless.

This document describes the concepts, principles and ideas of activity. The document uses the design of a simple Activity system as an example, it's not a specification.

## Core Concepts

### What is an Activity?

For each transaction, every address (lock_script_hash) that appears as an input or output owner gets exactly one `ActivityEntry`. That entry captures:

| Field            | Meaning                                                                |
| ---------------- | ---------------------------------------------------------------------- |
| `ckb_delta`      | Net CKB change in shannons (`output_capacity - input_capacity`)        |
| `occupied_delta` | Net occupied capacity change in shannons                               |
| `asset_changes`  | Token transfers, DAO operations, Object/Identity mints/transfers/burns |
| `peers`          | All OTHER lock_script_hashes in the same transaction                   |
| `is_cellbase`    | Whether this is a mining reward transaction                            |

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

### TxActivityBundle (canonical store type)

One bundle per canonical transaction. Contains all owner position changes.

```rust
pub struct TxActivityBundle {
    pub tx_hash: Vec<u8>,        // 32-byte transaction hash
    pub block_hash: Vec<u8>,     // 32-byte block hash
    pub block_number: i64,
    pub tx_index: i32,
    pub timestamp: i64,          // Unix timestamp
    pub is_cellbase: bool,
    pub owners: Vec<OwnerActivityDelta>,
}

pub struct OwnerActivityDelta {
    pub lock_hash: Vec<u8>,
    pub lock_code_hash: Vec<u8>,
    pub lock_hash_type: i16,
    pub lock_args: Vec<u8>,
    pub ckb_delta: i128,         // Net CKB change (shannons) — i128 for overflow safety
    pub used_delta: i64,         // Net occupied capacity change (shannons)
    pub has_type_script: bool,   // Whether any cell for this owner had a type script
    pub involved_script_code_hashes: Vec<Vec<u8>>,
    pub asset_changes: Vec<AssetChange>,
    pub script_calls: Option<Vec<ScriptCallEntry>>,
    pub peers: Vec<Vec<u8>>,     // Lock hashes of other parties
}
```

### ActivityEntry (read-model helper)

`ActivityEntry` is still used as an API/read-model type materialized on-the-fly from a bundle + owner delta. It is not stored directly.

### AssetChange (tagged enum)

```rust
pub enum AssetChange {
    Token {
        type_script_hash: Vec<u8>,
        delta: i128,             // Positive = received, negative = sent
        symbol: Option<String>,  // e.g. "SEAL"
        decimals: Option<u8>,    // e.g. 8
    },
    Object {                     // Spore
        object_id: Vec<u8>,
        standard: String,        // "spore"
        action: AssetAction,     // Mint | Transfer | Burn | Recycle | Renew | Update
    },
    Identity {                   // mNFT, .bit, did:ckb
        identity_id: Vec<u8>,
        standard: String,        // "m-nft", "dotbit", "did_ckb"
        action: AssetAction,
    },
    DaoDeposit { capacity: i64 },
    DaoWithdrawRequest { capacity: i64, deposit_block: i64 },
    DaoWithdrawComplete { capacity: i64, compensation: i64 },
    ScriptCall {
        type_code_hash: Vec<u8>, // Unrecognized type script interaction
    },
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

Additional .bit-specific actions:

- **Recycle**: expired account removed from chain (capacity refunded)
- **Renew**: account expiry extended (no ownership change)
- **Update**: metadata changed (edit_records, edit_manager, etc.)

### Activity Classification

Each activity is exclusively classified for stats aggregation using `has_type_script`:

| Classification                  | Condition                                                           |
| ------------------------------- | ------------------------------------------------------------------- |
| DAO (deposit/withdraw/complete) | Has DAO asset change                                                |
| Token                           | Has Token asset change                                              |
| Object                          | Has Object asset change                                             |
| Identity                        | Has Identity asset change                                           |
| ScriptCall                      | Has ScriptCall asset change (unrecognized type script)              |
| Transfer                        | No asset changes AND `has_type_script == false` (pure CKB transfer) |
| Unknown                         | No asset changes AND `has_type_script == true` (fallback)           |

## Storage Schema

### Column Families

**`CF_ACTIVITIES`** — registered in `DOMAIN_CFS` and `HIGH_WRITE_CFS` (large batch optimization tier). Lives in the domain store (mutable, supports delete on rollback). Stores one `TxActivityBundle` per canonical transaction.

**`CF_ADDR_TXS`** — lightweight secondary index. Key encodes `lock_hash + block_num_desc + tx_idx_desc + tx_hash`. Value is **empty** (tx_hash is extracted from the key). Used to look up which bundles an address participated in.

### Key Encoding (CF_ACTIVITIES)

```
block_num_desc(8B BE) + tx_idx_desc(4B BE) + tx_hash(32B) = 44 bytes
```

`block_num_desc = i64::MAX - block_num`, `tx_idx_desc = i32::MAX - tx_idx` — descending order so forward iteration yields newest transactions first.

```rust
pub fn encode_tx_activity_bundle_key(block_num: i64, tx_idx: i32, tx_hash: &[u8]) -> Vec<u8>;
pub fn decode_tx_activity_bundle_key(key: &[u8]) -> (i64, i32, Vec<u8>);
```

### Value Encoding

`bincode::serialize(TxActivityBundle)` — compact binary, fast to serialize/deserialize.

### Query Patterns

**Global latest activities**: Forward scan on `CF_ACTIVITIES` from start, expanding `OwnerActivityDelta` entries from each bundle, skipping cellbase. No secondary index needed.

**Per-address activities**: Scan `CF_ADDR_TXS` by lock_hash prefix → `multi_get` bundle rows from `CF_ACTIVITIES` → resolve per-owner `ActivityEntry` from bundle.

```rust
impl CkbadgerStore {
    pub fn list_activities(
        &self,
        lock_hash: &[u8],
        limit: usize,
        cursor: Option<(i64, i32)>,
        filter: Option<&str>,
    ) -> Result<Vec<(i64, i32, ActivityEntry)>>;

    pub fn get_latest_activities(&self) -> Result<Vec<LatestActivityItem>>;
}
```

### Rollback

Activities live in the domain store and are directly deleted during reorg rollback:

- Rollback performs a full scan on `CF_ACTIVITIES` and deletes 44-byte bundle keys belonging to rolled-back blocks.
- Same approach applies to `CF_ADDR_TXS` and collection activity CFs (`CF_OBJECT_COLLECTION_ACTIVITIES`, `CF_IDENTITY_COLLECTION_ACTIVITIES`).
- No ghost entries, no canonical filtering needed — direct deletion keeps the domain store clean.

See `docs/prompts/REORG_HANDLING.md` for the authoritative rollback boundary.

## Hourly & Daily Activity Stats

Activity stats are aggregated at two time granularities for charting and the 24h rolling summary:

- **Daily stats** (`ACTIVITY_DAILY` prefix in `CF_STATS_CHAIN`): key = `0x1D` + `YYYYMMDD`, value = `DailyActivityStats`
- **Hourly stats** (`ACTIVITY_HOURLY` prefix in `CF_STATS_CHAIN`): key = `0x1E` + `YYYYMMDDHH`, value = `DailyActivityStats`

`DailyActivityStats` contains per-classification counts (`transfer_count`, `dao_deposit_count`, `dao_withdraw_request_count`, `dao_withdraw_complete_count`, `token_count`, `object_count`, `identity_count`, `script_call_count`, `unknown_count`, `coinbase_count`), `unique_address_count`, `total_ckb_moved` (u128), and `script_counts` (HashMap of code_hash → count).

The API endpoint `GET /stats/activity-summary-24h` aggregates all hourly buckets within a 24h rolling window. Both daily and hourly stats are cleaned up during reorg rollback via `should_delete_stats_for_replay`.

## Activity Builder Algorithm

The builder is a pure function: given transaction data and pre-fetched cell info, it emits one `TxActivityBundle` per transaction with no side effects. Owners within a bundle are sorted deterministically by `lock_hash`.

### Per-transaction logic

```
1. For each input:
   - Group by lock_script_hash
   - Record lock script components (code_hash, hash_type, args)
   - Sum capacity, occupied_capacity
   - Classify by type_code_hash → accumulate UDT amounts, DOB/NFT IDs

2. For each output:
   - Group by lock_script_hash
   - Record lock script components (code_hash, hash_type, args)
   - Sum capacity, compute occupied_capacity from script sizes
   - Classify by type_code_hash → accumulate UDT amounts, DOB/NFT IDs, DAO ops

3. For each distinct lock_hash (sorted by lock_hash):
   - ckb_delta = Σ output_capacity - Σ input_capacity
   - used_delta = Σ output_occupied - Σ input_occupied
   - peers = all other lock_hashes in this transaction
   - Derive asset_changes from accumulated data
   - Emit OwnerActivityDelta

4. Wrap all OwnerActivityDelta entries into a single TxActivityBundle
```
