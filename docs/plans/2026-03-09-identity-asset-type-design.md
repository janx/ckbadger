# Identity Asset Type — Design

**Date**: 2026-03-09
**Status**: Approved

## Problem

.bit and did:ckb are identity protocols awkwardly classified under NFT/DOB categories. The DOB category is a historical artifact that splits Spore from mNFT unnecessarily. This refactor introduces Identity as a first-class asset type and collapses DOB into a unified Object category.

## New Asset Type Model

| Category     | Standards                                             | API string   |
| ------------ | ----------------------------------------------------- | ------------ |
| **Token**    | sUDT, xUDT                                            | `"token"`    |
| **Object**   | Spore, SporeCluster, MnftIssuer, MnftClass, MnftToken | `"object"`   |
| **Identity** | DotBit, DidCkb                                        | `"identity"` |
| **DAO**      | NervosDAO                                             | `"dao"`      |

## Rust Type Changes

### Remove entirely

- `DobStandard` enum
- `DobEntry` struct
- `DobExtra` enum
- `AssetChange::Dob` variant

### Rename NFT -> Object

- `NftStandard` -> `ObjectStandard` (add `Spore`, `SporeCluster` variants)
- `NftExtra` -> `ObjectExtra` (add `Spore`, `SporeCluster`, `DidCkb` variants from DobExtra)
- `NftEntry` -> `ObjectEntry`
- `NftCollectionAggregate` -> `ObjectCollectionAggregate`
- `NftCollectionActivityEntry` -> `ObjectCollectionActivityEntry`
- `AssetChange::Nft` -> `AssetChange::Object`

### New Identity types

```rust
pub enum IdentityStandard {
    DotBit,
    DidCkb,
}

pub enum IdentityExtra {
    DotBit {
        expired_at: Option<u64>,
        registered_at: Option<u64>,
        status: Option<u8>,
    },
    DidCkb,
}

pub struct IdentityEntry {
    pub standard: IdentityStandard,
    pub owner_lock_hash: Option<Vec<u8>>,
    pub name: Option<String>,
    pub is_live: bool,
    pub created_at_block: i64,
    pub created_at_tx: Vec<u8>,
    pub extra: IdentityExtra,
}

// In AssetChange enum:
Identity {
    identity_id: Vec<u8>,
    standard: String,
    action: AssetAction,
}
```

## Column Family Changes

### Rename (requires DB rebuild)

| Old                            | New                               |
| ------------------------------ | --------------------------------- |
| `CF_NFT_DATA`                  | `CF_OBJECT_DATA`                  |
| `CF_NFT_BY_COLLECTION`         | `CF_OBJECT_BY_COLLECTION`         |
| `CF_NFT_COLLECTION_AGG`        | `CF_OBJECT_COLLECTION_AGG`        |
| `CF_NFT_COLLECTION_ACTIVITIES` | `CF_OBJECT_COLLECTION_ACTIVITIES` |

### New

- `CF_IDENTITY_DATA` — stores `IdentityEntry` keyed by identity ID

### Unchanged

- `CF_SPORE_DATA`, `CF_SPORE_BY_CLUSTER`, `CF_CLUSTER_AGG` — Spore-specific rich data (media profiles, content types) stays in dedicated CFs
- All token CFs, DAO CFs, activity CFs

## DailyActivityStats

```rust
pub struct DailyActivityStats {
    pub transfer_count: u32,
    pub dao_deposit_count: u32,
    pub dao_withdraw_request_count: u32,
    pub dao_withdraw_complete_count: u32,
    pub token_count: u32,
    pub object_count: u32,      // was nft_count
    pub identity_count: u32,    // new
    pub coinbase_count: u32,
    pub unique_address_count: u32,
    pub total_ckb_moved: u128,
}
```

## API Changes

- `assetCategory` union: `'token' | 'object' | 'identity' | 'dao'` (was `'token' | 'dob' | 'nft' | 'dao'`)
- Asset list `?type=` parameter: `token | object | identity`
- Old `?type=nft` and `?type=dob` URLs normalize to `?type=object` for backward compat

## Frontend Changes

- Assets page tabs: **Tokens** | **Objects** | **Identities**
- `AssetTab = 'token' | 'object' | 'identity'`
- Badge colors: amber (token), purple (object), teal (identity), gray (dao)
- Standard filter options per tab:
  - Token: `['xudt', 'sudt']`
  - Object: `['spore', 'm-nft']`
  - Identity: `['dotbit', 'did:ckb']`

## Unchanged

- DOB rendering engine (`dob-render.ts`, `media_source.rs`) — DOB is a content format inside Spore, not an asset category
- Spore parser, dotbit parser — internal logic unchanged, only classification labels change
- DAO — completely unchanged

## DB Rebuild

Required. CF renames + new CF. Delete RocksDB and re-sync from genesis.
