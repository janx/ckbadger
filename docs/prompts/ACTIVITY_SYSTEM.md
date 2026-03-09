# Activity System Design

## Philosophy

Activities are **interpretations, not facts**. A simple form of activity is the interpretation of a per-owner position change in a single transaction: how much CKB moved, how much occupied capacity changed, which assets were affected, and who the counterparties were.

More sophisticated activity systems may interpret two owners' position changes in a single transaction as a single 'swap' activity rather than two separate activities. Since UTXO transactions are atomic action bundles involving multiple parties, the combination possibilities and thus possible interpretations are endless.

This document describes the concepts, principles and ideas of activity. The document uses the design of a simple Activity system as an example, it's not a specification.

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

`CF_ACTIVITIES` — registered in `DOMAIN_CFS` and `HIGH_WRITE_CFS` (large batch optimization tier). Lives in the domain store (mutable, supports delete on rollback).

### Key Encoding

```
lock_hash(32B) + block_num_desc(8B BE) + tx_idx_desc(4B BE) + tx_hash(32B) = 76 bytes
```

`block_num_desc = i64::MAX - block_num`, `tx_idx_desc = i32::MAX - tx_idx` — descending order so prefix scan returns newest activities first.

```rust
pub fn encode_activity_key(lock_hash: &[u8], block_num: i64, tx_idx: i32, tx_hash: &[u8]) -> Vec<u8>;
pub fn decode_activity_key(key: &[u8]) -> (Vec<u8>, i64, i32, Vec<u8>);
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

Activities live in the domain store and are directly deleted during reorg rollback:

- Rollback performs a range scan on `CF_ACTIVITIES` for each affected lock_hash and deletes entries belonging to rolled-back blocks.
- Same approach applies to `CF_ADDR_TXS` and collection activity CFs (`CF_OBJECT_COLLECTION_ACTIVITIES`, `CF_IDENTITY_COLLECTION_ACTIVITIES`).
- No ghost entries, no canonical filtering needed — direct deletion keeps the domain store clean.

See `docs/prompts/REORG_HANDLING.md` for the authoritative rollback boundary.

## Activity Builder Algorithm

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
