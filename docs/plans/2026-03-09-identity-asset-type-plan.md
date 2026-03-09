# Identity Asset Type — Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Refactor asset classification from Token/DOB/NFT/DAO to Token/Object/Identity/DAO — moving .bit and did:ckb into a new Identity asset type, removing the obsolete DOB category, and merging Spore into the renamed Object category.

**Architecture:** Bottom-up refactor across 4 layers: store types → indexer writers → API routes → frontend. Each layer depends on the one below. DB rebuild required after store changes (CF renames + new CF).

**Tech Stack:** Rust (ckbadger-store, ckbadger-indexer, ckbadger-api), TypeScript/React (frontend), RocksDB

**Design doc:** `docs/plans/2026-03-09-identity-asset-type-design.md`

---

## Phase 1: Store Layer

### Task 1: Rename NFT types to Object, remove DOB types, add Identity types

The foundation change. All other tasks depend on this.

**Files:**

- Modify: `crates/ckbadger-store/src/types.rs`

**Step 1: Rename NftStandard → ObjectStandard, merge DOB variants in**

In `types.rs`, replace the `NftStandard` enum (~line 347) with `ObjectStandard`. Add `Spore` and `SporeCluster` variants (from `DobStandard`). Remove `DotBit` and `DidCkb` (moving to Identity).

```rust
/// Object standard identifier.
///
/// Object is an asset type on CKB covering NFTs and digital objects.
/// Each variant represents a specific standard or entity type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ObjectStandard {
    /// A Spore item (individual digital object).
    #[default]
    Spore,
    /// A Spore cluster (collection of Spores).
    SporeCluster,
    /// mNFT issuer (top-level entity that creates classes).
    MnftIssuer,
    /// mNFT class (a collection of mNFT tokens).
    MnftClass,
    /// mNFT token (individual NFT item).
    MnftToken,
}

impl ObjectStandard {
    pub fn as_str(&self) -> &'static str {
        match self {
            ObjectStandard::Spore => "spore",
            ObjectStandard::SporeCluster => "spore_cluster",
            ObjectStandard::MnftIssuer => "mnft_issuer",
            ObjectStandard::MnftClass => "mnft_class",
            ObjectStandard::MnftToken => "mnft",
        }
    }

    pub fn asset_standard(&self) -> &'static str {
        match self {
            ObjectStandard::Spore | ObjectStandard::SporeCluster => "spore",
            ObjectStandard::MnftIssuer | ObjectStandard::MnftClass | ObjectStandard::MnftToken => "m-nft",
        }
    }

    pub fn is_cluster(&self) -> bool {
        matches!(self, ObjectStandard::SporeCluster)
    }
}
```

**Step 2: Rename NftExtra → ObjectExtra, merge DOB variants**

Replace `NftExtra` (~line 386) with `ObjectExtra`. Add Spore/SporeCluster/DidCkb variants from `DobExtra`. Remove DotBit and DidCkb (moving to Identity).

```rust
/// Standard-specific data for Object entries, stored inline via bincode.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ObjectExtra {
    /// Spore item: MIME content type and content byte length.
    Spore {
        content_type: String,
        content_length: i64,
        #[serde(default)]
        media_profile: SporeMediaProfile,
    },
    /// Spore cluster: no extra fields (name/description live on `ObjectEntry`).
    SporeCluster,
    /// mNFT issuer metadata.
    MnftIssuer {
        class_count: u32,
        set_count: u32,
        info: Option<Vec<u8>>,
    },
    /// mNFT class (collection) metadata.
    MnftClass {
        description: Option<String>,
        renderer: Option<String>,
        total: u32,
        issued: u32,
        configure: u8,
    },
    /// mNFT token (individual item) metadata.
    MnftToken {
        token_index: u32,
        characteristic: Vec<u8>,
        configure: u8,
        state: u8,
    },
}
```

**Step 3: Rename NftEntry → ObjectEntry**

Replace `NftEntry` (~line 427) with `ObjectEntry`:

```rust
/// An Object entry stored in the `object_data` column family.
///
/// Covers all Object standards: Spore (item/cluster), mNFT (issuer/class/token).
/// Standard-specific data lives in `extra`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObjectEntry {
    pub standard: ObjectStandard,
    pub collection_id: Option<Vec<u8>>,
    pub token_id: Option<Vec<u8>>,
    pub owner_lock_hash: Option<Vec<u8>>,
    pub name: Option<String>,
    pub description: Option<String>,
    pub is_live: bool,
    pub created_at_block: i64,
    pub created_at_tx: Vec<u8>,
    pub extra: ObjectExtra,
}
```

Note: Added `description` field (was on DobEntry but not NftEntry) and `created_at_tx` (was on DobEntry). Check if NftEntry had `created_at_tx` — if not, add it for uniformity.

**Step 4: Add Identity types**

Add new types after ObjectEntry:

```rust
/// Identity standard identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum IdentityStandard {
    /// .bit (DotBit) domain name account.
    #[default]
    DotBit,
    /// did:ckb decentralized identity.
    DidCkb,
}

impl IdentityStandard {
    pub fn as_str(&self) -> &'static str {
        match self {
            IdentityStandard::DotBit => "dotbit",
            IdentityStandard::DidCkb => "did_ckb",
        }
    }

    pub fn asset_standard(&self) -> &'static str {
        self.as_str()
    }
}

/// Standard-specific data for Identity entries.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum IdentityExtra {
    /// .bit account metadata.
    DotBit {
        expired_at: Option<u64>,
        registered_at: Option<u64>,
        status: Option<u8>,
    },
    /// did:ckb identity: reserved for future fields.
    DidCkb,
}

/// An Identity entry stored in the `identity_data` column family.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdentityEntry {
    pub standard: IdentityStandard,
    pub owner_lock_hash: Option<Vec<u8>>,
    pub name: Option<String>,
    pub is_live: bool,
    pub created_at_block: i64,
    pub created_at_tx: Vec<u8>,
    pub extra: IdentityExtra,
}
```

**Step 5: Rename NftCollectionAggregate → ObjectCollectionAggregate**

Update (~line 461):

```rust
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ObjectCollectionAggregate {
    pub name: Option<String>,
    pub standard: ObjectStandard,
    pub total_count: i64,
    pub live_count: i64,
    #[serde(default)]
    pub holders_count: i64,
    #[serde(default)]
    pub activities_count: i64,
}
```

**Step 6: Rename NftCollectionActivityEntry → ObjectCollectionActivityEntry**

Update (~line 949):

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObjectCollectionActivityEntry {
    pub tx_hash: Vec<u8>,
    #[serde(default)]
    pub block_hash: Vec<u8>,
    pub timestamp_ms: i64,
    pub actions: Vec<AssetAction>,
}
```

**Step 7: Update AssetChange enum**

Replace `Dob` and `Nft` variants (~line 876) with `Object` and `Identity`:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AssetChange {
    Token {
        type_script_hash: Vec<u8>,
        delta: i128,
        symbol: Option<String>,
        decimals: Option<u8>,
    },
    Object {
        object_id: Vec<u8>,
        standard: String,
        action: AssetAction,
    },
    Identity {
        identity_id: Vec<u8>,
        standard: String,
        action: AssetAction,
    },
    DaoDeposit {
        capacity: i64,
    },
    DaoWithdrawRequest {
        capacity: i64,
        deposit_block: i64,
    },
    DaoWithdrawComplete {
        capacity: i64,
        compensation: i64,
    },
}
```

**Step 8: Update DailyActivityStats**

Replace `nft_count` with `object_count` + `identity_count` (~line 923):

```rust
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DailyActivityStats {
    pub transfer_count: u32,
    pub dao_deposit_count: u32,
    pub dao_withdraw_request_count: u32,
    pub dao_withdraw_complete_count: u32,
    pub token_count: u32,
    pub object_count: u32,
    pub identity_count: u32,
    pub coinbase_count: u32,
    pub unique_address_count: u32,
    pub total_ckb_moved: u128,
}
```

**Step 9: Remove DobStandard, DobEntry, DobExtra, DobExtra, ClusterAggregate**

Delete the `DobStandard` enum (~lines 222-259), `DobExtra` enum (~lines 306-320), and `DobEntry` struct (~lines 322-340). `ClusterAggregate` (~line 443) stays — it's Spore-specific and still needed for CF_CLUSTER_AGG.

**Step 10: Rename NftDailyDelta → ObjectDailyDelta (if it exists)**

Search for `NftDailyDelta` in types.rs and rename.

**Step 11: Update all tests in types.rs**

Update tests that reference `DobStandard`, `DobEntry`, `DobExtra`, `NftStandard`, `NftEntry`, `NftExtra`, `AssetChange::Dob`, `AssetChange::Nft` to use the new names.

**Step 12: Run `cargo check -p ckbadger-store`**

Expected: Many errors in dependent crates (indexer, api) — that's fine. The store crate itself should compile.

**Step 13: Commit**

```
refactor(store): rename NFT→Object, remove DOB, add Identity types
```

---

### Task 2: Rename CF constants, add CF_IDENTITY_DATA

**Files:**

- Modify: `crates/ckbadger-store/src/store.rs`
- Modify: `crates/ckbadger-store/src/lib.rs`

**Step 1: Rename CF constants in store.rs**

At ~lines 288-306:

```rust
// Old:
pub const CF_NFT_DATA: &str = "nft_data";
pub const CF_NFT_BY_COLLECTION: &str = "nft_by_collection";
pub const CF_NFT_COLLECTION_AGG: &str = "nft_collection_agg";
pub const CF_NFT_COLLECTION_ACTIVITIES: &str = "nft_collection_activities";

// New:
pub const CF_OBJECT_DATA: &str = "object_data";
pub const CF_OBJECT_BY_COLLECTION: &str = "object_by_collection";
pub const CF_OBJECT_COLLECTION_AGG: &str = "object_collection_agg";
pub const CF_OBJECT_COLLECTION_ACTIVITIES: &str = "object_collection_activities";
pub const CF_IDENTITY_DATA: &str = "identity_data";
```

**Step 2: Update ALL_CFS, DOMAIN_CFS, APPEND_CFS arrays**

Replace old CF names with new ones. Add `CF_IDENTITY_DATA` to `ALL_CFS` and `DOMAIN_CFS`.

In `APPEND_CFS` (~line 409): replace `CF_NFT_COLLECTION_ACTIVITIES` with `CF_OBJECT_COLLECTION_ACTIVITIES`.

**Step 3: Add cf_handle accessor methods**

Rename existing accessors and add new one:

```rust
pub fn cf_object_data(&self) -> &ColumnFamily { ... }
pub fn cf_object_by_collection(&self) -> &ColumnFamily { ... }
pub fn cf_object_collection_agg(&self) -> &ColumnFamily { ... }
pub fn cf_object_collection_activities(&self) -> &ColumnFamily { ... }
pub fn cf_identity_data(&self) -> &ColumnFamily { ... }
```

Remove old `cf_nft_*` methods.

**Step 4: Update lib.rs exports**

Replace `CF_NFT_*` exports with `CF_OBJECT_*` and add `CF_IDENTITY_DATA`.

**Step 5: Run `cargo check -p ckbadger-store`**

**Step 6: Commit**

```
refactor(store): rename NFT CFs to Object, add CF_IDENTITY_DATA
```

---

### Task 3: Update store batch methods

**Files:**

- Modify: `crates/ckbadger-store/src/batch.rs`

**Step 1: Rename nft batch methods → object**

Rename all `put_nft*` / `delete_nft*` methods to `put_object*` / `delete_object*`. Update internal CF references from `cf_nft_*` to `cf_object_*`. Update type parameters from `NftEntry` to `ObjectEntry`, `NftCollectionAggregate` to `ObjectCollectionAggregate`, etc.

Key renames (~lines 588-775):

- `put_nft_hourly_transfer` → `put_object_hourly_transfer`
- `put_nft_daily_delta` → `put_object_daily_delta`
- `put_nft_type_index` → `put_object_type_index`
- `put_nft` → `put_object`
- `put_nft_by_collection` → `put_object_by_collection`
- `put_nft_collection_aggregate` → `put_object_collection_aggregate`
- `put_nft_collection_owner_count` → `put_object_collection_owner_count`
- `delete_nft_collection_owner` → `delete_object_collection_owner`
- `put_nft_collection_activity` → `put_object_collection_activity`

Type param changes:

- `&NftEntry` → `&ObjectEntry`
- `&NftCollectionAggregate` → `&ObjectCollectionAggregate`
- `&NftCollectionActivityEntry` → `&ObjectCollectionActivityEntry`
- `&NftDailyDelta` → `&ObjectDailyDelta`
- `&NftTypeIndex` → `&ObjectTypeIndex`

**Step 2: Add identity batch methods**

```rust
pub fn put_identity(&mut self, id: &[u8], entry: &IdentityEntry) {
    let value = bincode::serialize(entry).expect("serialize IdentityEntry");
    self.domain.put_cf(self.store.cf_identity_data(), id, &value);
}
```

**Step 3: Update `put_spore` to use ObjectEntry instead of DobEntry**

The `put_spore` method (~line 685) currently takes `&DobEntry`. It should keep its name (it writes to CF_SPORE_DATA which is unchanged) but the type changes. However — CF_SPORE_DATA stores Spore-specific rich data and ObjectEntry is a different struct.

**Important design decision:** CF_SPORE_DATA continues to store Spore-specific data. But the value type stored there was `DobEntry`. Now we need to decide:

- Option A: CF_SPORE_DATA stores `ObjectEntry` (same struct, just renamed)
- Option B: CF_SPORE_DATA keeps its own Spore-specific struct

Since `DobEntry` and `ObjectEntry` have the same shape, and Spore data includes media profiles via `ObjectExtra::Spore`, **Option A** is cleanest — `put_spore` takes `&ObjectEntry`.

**Step 4: Run `cargo check -p ckbadger-store`**

**Step 5: Commit**

```
refactor(store): rename batch NFT methods to Object, add identity batch
```

---

### Task 4: Update store read ops

**Files:**

- Rename: `crates/ckbadger-store/src/nft_ops.rs` → `crates/ckbadger-store/src/object_ops.rs`
- Create: `crates/ckbadger-store/src/identity_ops.rs`
- Modify: `crates/ckbadger-store/src/spore_ops.rs`
- Modify: `crates/ckbadger-store/src/dotbit_ops.rs`
- Modify: `crates/ckbadger-store/src/lib.rs` (module declarations)

**Step 1: Rename nft_ops.rs → object_ops.rs**

`mv crates/ckbadger-store/src/nft_ops.rs crates/ckbadger-store/src/object_ops.rs`

In the file, rename all:

- `get_nft` → `get_object`
- `get_nfts_batch` → `get_objects_batch`
- `list_nfts` → `list_objects`
- `NftEntry` → `ObjectEntry`
- `NftBatchEntry` → `ObjectBatchEntry`
- `get_nft_collection_aggregate` → `get_object_collection_aggregate`
- `list_nft_collection_aggregates` → `list_object_collection_aggregates`
- `NftCollectionAggregate` → `ObjectCollectionAggregate`
- `get_nft_collection_owner_count` → `get_object_collection_owner_count`
- `list_nft_collection_owner_counts` → `list_object_collection_owner_counts`
- `get_nft_type_index` → `get_object_type_index`
- `put_nft_type_index_direct` → `put_object_type_index_direct`
- `NftTypeIndex` → `ObjectTypeIndex`
- `get_nft_daily_delta` → `get_object_daily_delta`
- `put_nft_daily_delta` → `put_object_daily_delta`
- `list_nft_daily_deltas` → `list_object_daily_deltas`
- `NftDailyDelta` → `ObjectDailyDelta`
- `list_nft_ids_by_collection` → `list_object_ids_by_collection`
- `list_nft_collection_activities` → `list_object_collection_activities`
- `count_nft_collection_activities` → `count_object_collection_activities`
- `NftCollectionActivityEntry` → `ObjectCollectionActivityEntry`
- All `cf_nft_*` → `cf_object_*`

**Step 2: Create identity_ops.rs**

```rust
use crate::types::IdentityEntry;

type IdentityBatchEntry = (Vec<u8>, Option<IdentityEntry>);

impl crate::CkbadgerStore {
    pub fn get_identity(&self, id: &[u8]) -> anyhow::Result<Option<IdentityEntry>> {
        let cf = self.cf_identity_data();
        match self.db().get_cf(cf, id)? {
            Some(bytes) => Ok(Some(bincode::deserialize(&bytes)?)),
            None => Ok(None),
        }
    }

    pub fn get_identities_batch(
        &self,
        ids: &[Vec<u8>],
    ) -> anyhow::Result<Vec<IdentityBatchEntry>> {
        let cf = self.cf_identity_data();
        let keys: Vec<(&rocksdb::ColumnFamily, &[u8])> =
            ids.iter().map(|id| (cf, id.as_slice())).collect();
        let results = self.db().multi_get_cf(&keys);
        let mut out = Vec::with_capacity(ids.len());
        for (id, result) in ids.iter().zip(results) {
            let entry = match result? {
                Some(bytes) => Some(bincode::deserialize(&bytes)?),
                None => None,
            };
            out.push((id.clone(), entry));
        }
        Ok(out)
    }

    pub fn list_identities(
        &self,
        limit: usize,
    ) -> anyhow::Result<Vec<(Vec<u8>, IdentityEntry)>> {
        let cf = self.cf_identity_data();
        let iter = self.db().iterator_cf(cf, rocksdb::IteratorMode::Start);
        let mut out = Vec::new();
        for item in iter {
            let (key, value) = item?;
            let entry: IdentityEntry = bincode::deserialize(&value)?;
            out.push((key.to_vec(), entry));
            if out.len() >= limit {
                break;
            }
        }
        Ok(out)
    }
}
```

**Step 3: Update spore_ops.rs**

Replace all `DobEntry` references with `ObjectEntry`. The function names stay (`get_spore`, `list_spores`, etc.) since they operate on CF_SPORE_DATA.

**Step 4: Update dotbit_ops.rs**

This file contains outpoint-lookup operations — no type changes needed since it doesn't reference NftEntry/DobEntry directly. Verify and confirm.

**Step 5: Update module declarations in lib.rs**

Replace `mod nft_ops;` with `mod object_ops;`, add `mod identity_ops;`.

**Step 6: Run `cargo check -p ckbadger-store`**

**Step 7: Commit**

```
refactor(store): rename nft_ops→object_ops, add identity_ops, update spore_ops
```

---

## Phase 2: Indexer Layer

### Task 5: Update indexer spore writer

**Files:**

- Modify: `crates/indexer/src/db/writer/spore.rs`

**Step 1: Replace all DobStandard/DobEntry/DobExtra with ObjectStandard/ObjectEntry/ObjectExtra**

Key changes:

- Line 8 imports: `DobEntry, DobExtra, DobStandard` → `ObjectEntry, ObjectExtra, ObjectStandard`
- Line 24 cache type: `HashMap<Vec<u8>, Option<DobEntry>>` → `HashMap<Vec<u8>, Option<ObjectEntry>>`
- Line 36-43: `get_spore` return type `Option<DobEntry>` → `Option<ObjectEntry>`
- Lines 419-435: `DobEntry` construction for clusters → `ObjectEntry` with `ObjectStandard::SporeCluster`, `ObjectExtra::SporeCluster`
- Lines 541-561: `DobEntry` construction for spores → `ObjectEntry` with `ObjectStandard::Spore`, `ObjectExtra::Spore { ... }`

**Step 2: Handle did:ckb differently**

Lines 522-539: did:ckb was stored as DobEntry with DobStandard::DidCkb. Now it should create an `IdentityEntry` with `IdentityStandard::DidCkb` and write to CF_IDENTITY_DATA via `batch.put_identity()`.

Also need to ensure the spore writer's did:ckb path uses the new batch method and no longer writes to CF_SPORE_DATA for did:ckb entries.

**Step 3: Run `cargo check -p ckbadger-indexer`**

Expected: Still many errors from other writer modules, but spore.rs should be clean.

**Step 4: Commit**

```
refactor(indexer): update spore writer for Object+Identity types
```

---

### Task 6: Update indexer dotbit writer

**Files:**

- Modify: `crates/indexer/src/db/writer/dotbit.rs`

**Step 1: Replace NftStandard/NftEntry/NftExtra with IdentityStandard/IdentityEntry/IdentityExtra**

Key changes:

- Line 6-8 imports: Replace `NftStandard, NftEntry` with `IdentityStandard, IdentityEntry, IdentityExtra`
- Line 180 cache: `HashMap<Vec<u8>, Option<NftEntry>>` → `HashMap<Vec<u8>, Option<IdentityEntry>>`
- Line 188-199: `get_account()` return type → `Option<IdentityEntry>`
- Lines 395-412: `insert_dotbit_account()` creates `IdentityEntry` with `IdentityStandard::DotBit`, `IdentityExtra::DotBit { expired_at, registered_at, status }`

**Step 2: Update batch calls**

- `batch.put_nft(...)` → `batch.put_identity(...)`
- `batch.put_nft_by_collection(...)` → remove (identities don't have collections)
- `batch.put_nft_collection_aggregate(...)` → remove for dotbit
- `batch.put_nft_collection_activity(...)` → remove or convert (identity activity tracking TBD)

**Important:** .bit currently uses NftCollectionAggregate and NftCollectionActivityEntry for the implicit ".bit" collection. After refactor, decide if Identity needs its own collection aggregate or if the simpler flat list is sufficient. For now, **remove collection logic for identities** — they are flat lists without collection hierarchy.

**Step 3: Update store reads**

Change `store.get_nft(...)` calls to `store.get_identity(...)`.

**Step 4: Commit**

```
refactor(indexer): update dotbit writer for Identity types
```

---

### Task 7: Update indexer activities writer

**Files:**

- Modify: `crates/indexer/src/db/writer/activities.rs`

**Step 1: Update AssetKind and code_hash classification**

The `AssetKind` enum (~line 19) and the code hash lookup table (~lines 47-60) need updating. Current kinds: `Udt, Dao, SporeDid, Spore, Cluster, MnftToken, Dotbit`.

Split into category-aware classification:

- `Udt` → stays (Token)
- `Dao` → stays (DAO)
- `Spore`, `Cluster` → stays (Object)
- `MnftToken` → stays (Object)
- `SporeDid` → now Identity
- `Dotbit` → now Identity

**Step 2: Update emit_nft_changes function**

Rename to something like `emit_object_and_identity_changes` or split into two functions. The key change:

- For Spore/SporeCluster/mNFT: emit `AssetChange::Object { object_id, standard, action }`
- For DotBit/DidCkb: emit `AssetChange::Identity { identity_id, standard, action }`

Lines ~479-520: Replace `AssetChange::Dob { dob_id, standard, action }` and `AssetChange::Nft { nft_id, standard, action }` with the new variants based on AssetKind.

**Step 3: Update collection activity tracking**

Lines ~949-1010: Where `AssetChange::Nft` and `AssetChange::Dob` are matched for collection activity emission. Update to match `AssetChange::Object` for collection activities. Identity assets don't participate in collection activities.

**Step 4: Commit**

```
refactor(indexer): update activities writer for Object+Identity asset changes
```

---

### Task 8: Update indexer statistics writer

**Files:**

- Modify: `crates/indexer/src/db/writer/statistics.rs`

**Step 1: Update accumulate_activity_stats**

Line ~557: Replace `AssetChange::Dob { .. } | AssetChange::Nft { .. }` match arm:

```rust
AssetChange::Object { .. } => {
    stats.object_count += 1;
}
AssetChange::Identity { .. } => {
    stats.identity_count += 1;
}
```

**Step 2: Update update_daily_activity_stats merge**

Line ~592: Replace `nft_count` merge with `object_count` and `identity_count`.

**Step 3: Update tests**

Replace `AssetChange::Dob` / `AssetChange::Nft` in test data with `AssetChange::Object` / `AssetChange::Identity`.

**Step 4: Run `cargo check -p ckbadger-indexer`**

All indexer modules should compile now.

**Step 5: Commit**

```
refactor(indexer): update statistics for object_count + identity_count
```

---

## Phase 3: API Layer

### Task 9: Update API activity response types

**Files:**

- Modify: `crates/api/src/routes/activities.rs`

**Step 1: Update AssetChangeResponse enum**

Lines ~52-86: Replace `Dob` and `Nft` variants:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum AssetChangeResponse {
    #[serde(rename = "token")]
    Token { type_script_hash: String, delta: String, symbol: Option<String>, decimals: Option<u8> },
    #[serde(rename = "object")]
    Object { object_id: String, standard: String, action: String },
    #[serde(rename = "identity")]
    Identity { identity_id: String, standard: String, action: String },
    #[serde(rename = "daoDeposit")]
    DaoDeposit { capacity: String },
    #[serde(rename = "daoWithdrawRequest")]
    DaoWithdrawRequest { capacity: String, deposit_block: i64 },
    #[serde(rename = "daoWithdrawComplete")]
    DaoWithdrawComplete { capacity: String, compensation: String },
}
```

**Step 2: Update convert_asset_change function**

Lines ~124-173: Replace `AssetChange::Dob` and `AssetChange::Nft` match arms:

```rust
AssetChange::Object { object_id, standard, action } => {
    AssetChangeResponse::Object {
        object_id: format!("0x{}", hex::encode(object_id)),
        standard: standard.clone(),
        action: format_action(action),
    }
}
AssetChange::Identity { identity_id, standard, action } => {
    AssetChangeResponse::Identity {
        identity_id: format!("0x{}", hex::encode(identity_id)),
        standard: standard.clone(),
        action: format_action(action),
    }
}
```

**Step 3: Commit**

```
refactor(api): update activity response for Object+Identity types
```

---

### Task 10: Update API assets route

**Files:**

- Modify: `crates/api/src/routes/assets.rs`

**Step 1: Update AssetFilterType enum**

Line ~133: Add Identity variant:

```rust
pub enum AssetFilterType {
    Token,
    Object,   // was Nft
    Identity,  // new
}
```

Update the `from_str` / display logic to accept `"object"` and `"identity"`. Add backward compat: `"nft"` and `"dob"` both map to `Object`.

**Step 2: Update list_assets handler**

Update cache key references and filter logic. Replace `CACHE_KEY_ASSETS_NFT` with `CACHE_KEY_ASSETS_OBJECT` and add `CACHE_KEY_ASSETS_IDENTITY`.

**Step 3: Update parse_asset_cursor**

Line ~542: Replace `"nft"` with `"object"`, add `"identity"`.

**Step 4: Update all DobStandard/NftStandard references**

Throughout the file, replace `DobStandard::DidCkb` checks with `IdentityStandard::DidCkb`, replace `DobEntry` with `ObjectEntry` or `IdentityEntry` as appropriate.

**Step 5: Update tests**

**Step 6: Commit**

```
refactor(api): update assets route for Token/Object/Identity
```

---

### Task 11: Update API warmup cache

**Files:**

- Modify: `crates/api/src/warmup.rs`

**Step 1: Rename cache key constants**

```rust
pub const CACHE_KEY_ASSETS_TOKEN: &str = "assets:token";     // unchanged
pub const CACHE_KEY_ASSETS_OBJECT: &str = "assets:object";   // was "assets:nft"
pub const CACHE_KEY_ASSETS_IDENTITY: &str = "assets:identity"; // new
```

**Step 2: Update CachedAssetEntry**

Change `asset_type` field to use `"object"` / `"identity"` instead of `"nft"` / `"dob"`.

**Step 3: Update cache warming logic**

Add Identity cache warming: load all identities from CF_IDENTITY_DATA, build CachedAssetEntry for each.

**Step 4: Commit**

```
refactor(api): update warmup cache for Object+Identity
```

---

### Task 12: Update remaining API files

**Files:**

- Modify: `crates/api/src/utils/assets.rs`
- Modify: `crates/api/src/ws/broadcaster.rs`
- Modify: `crates/api/src/routes/search.rs`
- Modify: `crates/api/src/routes/spore.rs`

**Step 1: Update utils/assets.rs**

- Remove `DobStandard` import
- Rename `resolve_dob_collection_name` → `resolve_object_collection_name` (for Spore clusters)
- Rename `resolve_nft_collection_name` to handle mNFT collections under Object
- Update storage tier overrides: move .bit and did:ckb overrides to a separate identity function if needed
- Update tests

**Step 2: Update ws/broadcaster.rs**

Lines ~285-299: Replace `AssetChange::Dob` and `AssetChange::Nft` match arms with `AssetChange::Object` and `AssetChange::Identity`.

**Step 3: Update routes/search.rs**

Replace `DobEntry` references with `ObjectEntry`. Update search scope labels.

**Step 4: Update routes/spore.rs**

Replace all `DobEntry` / `DobExtra` / `DobStandard` with `ObjectEntry` / `ObjectExtra` / `ObjectStandard`. This is a large file — mostly mechanical renames.

**Step 5: Run `cargo check` (full workspace)**

All Rust code should compile.

**Step 6: Run `cargo test`**

Fix any test failures.

**Step 7: Commit**

```
refactor(api): update utils, websocket, search, spore routes
```

---

## Phase 4: Frontend Layer

### Task 13: Update frontend API types

**Files:**

- Modify: `frontend/lib/api.ts`
- Modify: `frontend/lib/format-asset.ts`
- Modify: `frontend/lib/nft-utils.ts`

**Step 1: Update AssetTransfer interface**

Line ~405: Change assetCategory union:

```typescript
assetCategory: 'token' | 'object' | 'identity' | 'dao';
```

**Step 2: Update ActivityAssetChange union type**

Line ~424-430: Replace `dob` and `nft` variants:

```typescript
type ActivityAssetChange =
  | {
      type: 'token';
      typeScriptHash: string;
      delta: string;
      symbol: string | null;
      decimals: number | null;
    }
  | { type: 'object'; objectId: string; standard: string; action: string }
  | { type: 'identity'; identityId: string; standard: string; action: string }
  | { type: 'daoDeposit'; capacity: string }
  | { type: 'daoWithdrawRequest'; capacity: string; depositBlock: number }
  | { type: 'daoWithdrawComplete'; capacity: string; compensation: string };
```

**Step 3: Update Asset interface**

Line ~596: Change assetType:

```typescript
assetType: 'token' | 'object' | 'identity';
```

**Step 4: Update AssetQueryParams**

Line ~627: Change type:

```typescript
type?: 'token' | 'object' | 'identity';
```

**Step 5: Update format-asset.ts**

Update `getAssetLabel`:

- Remove `'dob/0'` and `'dob/1'` cases (keep if DOB content type labels are still needed for Spore display)
- Add `'did_ckb'` case if missing

Update `getAssetBadgeVariant`:

- Remove `'dob'` case
- Add `'object'` → `'purple'`
- Add `'identity'` → `'teal'` (use a teal/cyan variant)

**Step 6: Update nft-utils.ts**

Rename identity-related utilities if they reference "nft" in misleading ways.

**Step 7: Commit**

```
refactor(frontend): update API types for Token/Object/Identity
```

---

### Task 14: Update frontend assets page

**Files:**

- Modify: `frontend/app/assets/assets-page-client.tsx`

**Step 1: Update AssetTab type**

```typescript
type AssetTab = 'token' | 'object' | 'identity';
```

**Step 2: Update backward compat normalization**

Line ~49: Map old URL values:

```typescript
if (value === 'dob' || value === 'nft') value = 'object';
```

**Step 3: Update standard filter options**

```typescript
const OBJECT_STANDARD_OPTIONS = ['spore', 'm-nft'];
const IDENTITY_STANDARD_OPTIONS = ['dotbit', 'did:ckb'];
```

**Step 4: Update tab rendering**

Three tabs: Tokens | Objects | Identities

**Step 5: Update storage tier filter visibility**

Storage tier filter shows for `object` tab (not identity — identities are always fully on-chain).

**Step 6: Run `pnpm type-check && pnpm lint`**

**Step 7: Commit**

```
refactor(frontend): update assets page for Token/Object/Identity tabs
```

---

### Task 15: Update frontend activity components

**Files:**

- Modify: `frontend/components/latest-activities.tsx`
- Modify: `frontend/app/address/[addr]/client-page.tsx`

**Step 1: Update latest-activities.tsx**

Line ~62: Replace `case 'dob':` with `case 'object':` and add `case 'identity':`.

**Step 2: Update address page category filter**

Line ~295: Replace `case 'dob':` with `case 'object':` and add `case 'identity':`.

**Step 3: Run `pnpm type-check && pnpm lint`**

**Step 4: Commit**

```
refactor(frontend): update activity components for Object+Identity
```

---

## Phase 5: Tests and Verification

### Task 16: Update Rust integration tests

**Files:**

- Modify: `crates/api/tests/api_integration.rs`
- Modify: `crates/indexer/tests/batch_nft_lookups.rs`
- Modify: `crates/indexer/tests/reorg_handling.rs` (if it references Dob/Nft types)

**Step 1: Update api_integration.rs**

Replace all `DobEntry, DobExtra, DobStandard` with `ObjectEntry, ObjectExtra, ObjectStandard`. Replace `NftEntry, NftExtra, NftStandard` similarly. Update assertions on API response `assetCategory` strings.

**Step 2: Update batch_nft_lookups.rs**

Rename to `batch_object_lookups.rs` if appropriate, or just update the type references.

**Step 3: Run `cargo test`**

**Step 4: Commit**

```
test: update integration tests for Object+Identity types
```

---

### Task 17: Update frontend tests

**Files:**

- Modify: `frontend/__tests__/lib/format-asset.test.ts`
- Modify: `frontend/__tests__/lib/dob-render.test.ts` (if badge variant tests reference "dob")
- Modify: `frontend/__tests__/components/identity-nft-item-detail.test.tsx`
- Modify: `frontend/__tests__/pages/address.test.tsx`
- Modify: `frontend/__tests__/pages/dotbit-item-detail.test.tsx`
- Modify: `frontend/__tests__/pages/did-item-detail.test.tsx`

**Step 1: Update format-asset tests**

Replace `'dob'` badge variant tests with `'object'` and `'identity'`.

**Step 2: Update component tests**

Update mock data that references `assetCategory: 'nft'` or `'dob'` to use `'object'` or `'identity'`.

**Step 3: Run `pnpm test`**

**Step 4: Commit**

```
test: update frontend tests for Object+Identity types
```

---

### Task 18: Full verification

**Step 1: Run full pre-commit checks**

```bash
cargo check && cargo clippy && cd frontend && pnpm type-check && pnpm lint
```

**Step 2: Run all tests**

```bash
cargo test && cd frontend && npx vitest run
```

**Step 3: Verify no stale references**

Search for any remaining `"dob"`, `"nft"` (as asset category, not in DOB rendering code), `DobStandard`, `DobEntry`, `NftStandard`, `NftEntry` references that should have been updated.

Exceptions (keep as-is):

- `dob-render.ts` and `media_source.rs` — DOB is a content format, not an asset category
- `dob-cookbook/` docs — reference material
- Content type strings like `"dob/0"`, `"dob/1"` — these are MIME-like format identifiers

**Step 4: Final commit if any fixes needed**

```
chore: fix remaining stale NFT/DOB references
```
