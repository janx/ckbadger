# Identity Page Separation Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Separate identity collections (.bit, DID:CKB) from NFT/object collections — both in API endpoints and frontend pages — so identities have their own aggregation path, page template, and correct data reads.

**Architecture:** Create dedicated `identities.rs` API route module with endpoints under `/assets/identities/{collection_id}`. Create dedicated frontend route at `/identities/[collectionId]/` with its own client component. Fix holder counts bug (was reading wrong CF). Use canonical activity count for display consistency. Remove identity-specific branching from the shared NFT page.

**Tech Stack:** Rust/Axum (backend API), React/TypeScript (frontend), RocksDB (store reads)

---

## Root Cause Analysis: Activities Count Mismatch

**Finding:** The collection detail API returns `agg.activities_count` from the pre-computed `IdentityCollectionAggregate`, but the activities list API filters entries through `canonical_nft_collection_activity_locations()` which cross-checks each activity's tx_hash against the canonical tx index. These counts can diverge after reorgs (the reorg handler counts ALL CF entries, while the API list filters for canonical matches). Additionally, if the DB was synced before commit `c87e6a3` and not rebuilt, the aggregate values are stale.

**Fix:** The new identity detail endpoint uses `count_identity_collection_activities_canonical` (actual canonical count matching what the list shows) instead of the aggregate's `activities_count`. This ensures the tab header count always matches the displayed records.

## Bug Found: Holders Endpoint Uses Wrong CF

**Finding:** `collect_nft_collection_holder_counts()` in `assets.rs:2361` always calls `store.list_object_collection_owner_counts()` which reads `CF_STATS_OBJECT`. Identity collection holders are stored in `CF_STATS_SPORE` via `store.list_cluster_owner_counts()`. Result: identity holder listings return empty.

**Fix:** The new identity holders endpoint reads from the correct CF.

---

### Task 1: Create identity API route module with collection detail endpoint

**Files:**

- Create: `crates/api/src/routes/identities.rs`
- Modify: `crates/api/src/routes/mod.rs`
- Modify: `crates/api/src/routes/assets.rs` (make helpers pub(crate))

**Step 1: Create `identities.rs` with response types and detail endpoint**

```rust
// crates/api/src/routes/identities.rs
use axum::{
    extract::{Path, Query, State},
    routing::get,
    Json, Router,
};
use ckbadger_store::CkbadgerStore;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use crate::cache::InMemoryCache;
use crate::response::{ok, ApiError, ApiResult, CursorPaginatedResponse};
use crate::AppState;

use super::assets::{
    count_nft_collection_activities_cached, list_canonical_nft_collection_activities_page,
    NftCollectionActivityResponse, NftCollectionHolderResponse,
};

const DOTBIT_SENTINEL_COLLECTION: [u8; 32] = *b"dotbit_collection_______________";
const DID_CKB_SENTINEL_COLLECTION: [u8; 32] = *b"did_ckb_collection______________";

const IDENTITY_HOLDER_LIST_CACHE_TTL: Duration = Duration::from_secs(30);

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route(
            "/assets/identities/{collection_id}",
            get(get_identity_collection),
        )
        .route(
            "/assets/identities/{collection_id}/items",
            get(list_identity_collection_items),
        )
        .route(
            "/assets/identities/{collection_id}/holders",
            get(list_identity_collection_holders),
        )
        .route(
            "/assets/identities/{collection_id}/activities",
            get(list_identity_collection_activities),
        )
}

fn decode_identity_collection_id(
    raw: &str,
) -> Result<Vec<u8>, (axum::http::StatusCode, Json<ApiError>)> {
    let normalized = raw.to_ascii_lowercase();
    if normalized == "dotbit" || normalized == ".bit" {
        return Ok(DOTBIT_SENTINEL_COLLECTION.to_vec());
    }
    if normalized == "did:ckb" || normalized == "did_ckb" {
        return Ok(DID_CKB_SENTINEL_COLLECTION.to_vec());
    }
    let bytes = hex::decode(normalized.strip_prefix("0x").unwrap_or(&normalized))
        .map_err(|_| ApiError::bad_request("Invalid identity collection ID"))?;
    if bytes != DOTBIT_SENTINEL_COLLECTION && bytes != DID_CKB_SENTINEL_COLLECTION {
        return Err(ApiError::bad_request(
            "Not an identity collection. Use /assets/nfts/ for NFT collections.",
        ));
    }
    Ok(bytes)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IdentityCollectionDetailResponse {
    pub collection_id: String,
    pub standard: String,
    pub name: Option<String>,
    pub total_count: i64,
    pub live_count: i64,
    pub holders_count: i64,
    pub activities_count: i64,
}

async fn get_identity_collection(
    State(state): State<Arc<AppState>>,
    Path(collection_id): Path<String>,
) -> ApiResult<IdentityCollectionDetailResponse> {
    let collection_id_bytes = decode_identity_collection_id(&collection_id)?;

    let agg = state
        .store
        .get_identity_collection_aggregate(&collection_id_bytes)
        .map_err(|e| ApiError::internal(e.to_string()))?
        .ok_or_else(|| ApiError::not_found("Identity collection not found"))?;

    // Use canonical activity count for consistency with the list endpoint.
    let activities_count = count_nft_collection_activities_cached(
        state.store.as_ref(),
        state.store.as_ref(),
        &state.mem_cache,
        &collection_id_bytes,
    )
    .map_err(|e| ApiError::internal(e.to_string()))?;

    ok(IdentityCollectionDetailResponse {
        collection_id: format!("0x{}", hex::encode(&collection_id_bytes)),
        standard: agg.standard.asset_standard().to_string(),
        name: agg.name,
        total_count: agg.total_count,
        live_count: agg.live_count,
        holders_count: agg.holders_count,
        activities_count,
    })
}
```

**Step 2: Register in mod.rs**

Add to `crates/api/src/routes/mod.rs`:

```rust
mod identities;
// ...
.merge(identities::routes())
```

**Step 3: Make shared helpers pub(crate) in assets.rs**

In `crates/api/src/routes/assets.rs`, change visibility of:

- `count_nft_collection_activities_cached` → already `pub(crate)` ✓
- `list_canonical_nft_collection_activities_page` → already `pub(crate)` ✓
- `NftCollectionActivityResponse` → make `pub(crate)`
- `NftCollectionHolderResponse` → make `pub(crate)`

**Step 4: Run `cargo check` to verify compilation**

Run: `cargo check -p ckbadger-api`
Expected: PASS

**Step 5: Commit**

```
feat(api): add identity collection detail endpoint

Dedicated /assets/identities/{collection_id} endpoint that reads
IdentityCollectionAggregate directly (no conversion to Object type).
Uses canonical activity count for consistency with the list endpoint.
```

---

### Task 2: Add identity collection holders endpoint (fixes wrong-CF bug)

**Files:**

- Modify: `crates/api/src/routes/identities.rs`

**Step 1: Add holders endpoint that reads from CF_STATS_SPORE**

Add to `identities.rs`:

```rust
#[derive(Debug, Deserialize)]
pub struct IdentityCollectionHoldersParams {
    #[serde(default = "default_limit")]
    limit: i64,
    cursor: Option<String>,
}

fn default_limit() -> i64 {
    20
}

fn collect_identity_holder_counts(
    store: &CkbadgerStore,
    collection_id_bytes: &[u8],
) -> Result<HashMap<Vec<u8>, i64>, (axum::http::StatusCode, Json<ApiError>)> {
    // Identity holders are in CF_STATS_SPORE, NOT CF_STATS_OBJECT.
    // The old shared endpoint had a bug: it always read CF_STATS_OBJECT.
    let rows = store
        .list_cluster_owner_counts(collection_id_bytes)
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let mut holder_counts: HashMap<Vec<u8>, i64> = HashMap::with_capacity(rows.len());
    for (lock_hash, count) in rows {
        if count <= 0 {
            continue;
        }
        holder_counts.insert(lock_hash, count);
    }
    Ok(holder_counts)
}

fn list_identity_holders_ranked(
    store: &CkbadgerStore,
    collection_id_bytes: &[u8],
) -> Result<Vec<(Vec<u8>, i64)>, (axum::http::StatusCode, Json<ApiError>)> {
    let mut holders: Vec<(Vec<u8>, i64)> =
        collect_identity_holder_counts(store, collection_id_bytes)?
            .into_iter()
            .collect();
    holders.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    Ok(holders)
}

fn list_identity_holders_ranked_cached(
    store: &CkbadgerStore,
    mem_cache: &InMemoryCache,
    collection_id_bytes: &[u8],
) -> Result<Vec<(Vec<u8>, i64)>, (axum::http::StatusCode, Json<ApiError>)> {
    let cache_key = format!(
        "assets:identity_collection_holders_ranked:0x{}",
        hex::encode(collection_id_bytes)
    );
    if let Some(cached) = mem_cache.get::<Vec<(Vec<u8>, i64)>>(&cache_key) {
        return Ok(cached);
    }
    let holders = list_identity_holders_ranked(store, collection_id_bytes)?;
    mem_cache.set(&cache_key, &holders, IDENTITY_HOLDER_LIST_CACHE_TTL);
    Ok(holders)
}

fn decode_identity_holders_cursor(
    raw: &str,
) -> Result<(i64, Vec<u8>), (axum::http::StatusCode, Json<ApiError>)> {
    let parts: Vec<&str> = raw.splitn(2, ':').collect();
    if parts.len() != 2 {
        return Err(ApiError::bad_request("Invalid holders cursor format"));
    }
    let count: i64 = parts[0]
        .parse()
        .map_err(|_| ApiError::bad_request("Invalid holders cursor count"))?;
    let lock_hash = hex::decode(parts[1].strip_prefix("0x").unwrap_or(parts[1]))
        .map_err(|_| ApiError::bad_request("Invalid holders cursor lock_hash"))?;
    Ok((count, lock_hash))
}

async fn list_identity_collection_holders(
    State(state): State<Arc<AppState>>,
    Path(collection_id): Path<String>,
    Query(params): Query<IdentityCollectionHoldersParams>,
) -> ApiResult<CursorPaginatedResponse<NftCollectionHolderResponse>> {
    let limit = params.limit.clamp(1, 100) as usize;
    let collection_id_bytes = decode_identity_collection_id(&collection_id)?;
    let cursor = params
        .cursor
        .as_deref()
        .map(decode_identity_holders_cursor)
        .transpose()?;

    let _agg = state
        .store
        .get_identity_collection_aggregate(&collection_id_bytes)
        .map_err(|e| ApiError::internal(e.to_string()))?
        .ok_or_else(|| ApiError::not_found("Identity collection not found"))?;

    let all_holders = list_identity_holders_ranked_cached(
        state.store.as_ref(),
        &state.mem_cache,
        &collection_id_bytes,
    )?;

    let total = all_holders.len() as i64;

    let start_idx = if let Some((cursor_count, cursor_lock)) = &cursor {
        all_holders
            .iter()
            .position(|(lh, c)| *c < *cursor_count || (*c == *cursor_count && *lh > *cursor_lock))
            .unwrap_or(all_holders.len())
    } else {
        0
    };

    let page_holders: Vec<&(Vec<u8>, i64)> =
        all_holders.iter().skip(start_idx).take(limit + 1).collect();
    let has_more = page_holders.len() > limit;
    let page: Vec<&(Vec<u8>, i64)> = page_holders.into_iter().take(limit).collect();

    let lock_hashes: Vec<&[u8]> = page.iter().map(|(lh, _)| lh.as_slice()).collect();
    let address_map = state
        .store
        .get_addresses_by_lock_hashes(&lock_hashes)
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let data: Vec<NftCollectionHolderResponse> = page
        .iter()
        .map(|(lock_hash, count)| {
            let address = address_map
                .get(lock_hash.as_slice())
                .and_then(|a| a.clone());
            NftCollectionHolderResponse {
                lock_script_hash: format!("0x{}", hex::encode(lock_hash)),
                address,
                item_count: *count,
            }
        })
        .collect();

    let next_cursor = if has_more {
        data.last().map(|row| {
            format!(
                "{}:{}",
                row.item_count,
                row.lock_script_hash.strip_prefix("0x").unwrap_or(&row.lock_script_hash)
            )
        })
    } else {
        None
    };

    ok(CursorPaginatedResponse::new(data, total, limit as i64, next_cursor))
}
```

**Step 2: Run `cargo check`**

Run: `cargo check -p ckbadger-api`
Expected: PASS

**Step 3: Commit**

```
fix(api): identity holders reads correct CF (CF_STATS_SPORE)

The shared NFT holders endpoint always read CF_STATS_OBJECT, returning
empty results for identity collections. The new dedicated endpoint
reads CF_STATS_SPORE via list_cluster_owner_counts().
```

---

### Task 3: Add identity collection activities and items endpoints

**Files:**

- Modify: `crates/api/src/routes/identities.rs`
- Modify: `crates/api/src/routes/assets.rs` (make more helpers pub(crate))

**Step 1: Make item-related helpers pub(crate) in assets.rs**

Change visibility of these in `assets.rs`:

- `NftCollectionItemResponse` → `pub(crate)`
- `fetch_dotbit_collection_entries_by_ids` → `pub(crate)`
- `fetch_did_collection_entries_by_ids` → `pub(crate)`
- `normalize_nft_items_search` → `pub(crate)`
- `normalize_nft_items_status` → `pub(crate)`
- `nft_item_matches_status` → `pub(crate)`
- `nft_item_matches_search` → `pub(crate)`
- `NftItemsParams` → `pub(crate)`
- `NftCollectionActivitiesParams` → `pub(crate)`
- `decode_activity_cursor` → make `pub(crate)` (if not already)
- `normalize_nft_activity_action_filter` → make `pub(crate)` (if not already)

**Step 2: Add activities endpoint to identities.rs**

```rust
async fn list_identity_collection_activities(
    State(state): State<Arc<AppState>>,
    Path(collection_id): Path<String>,
    Query(params): Query<NftCollectionActivitiesParams>,
) -> ApiResult<CursorPaginatedResponse<NftCollectionActivityResponse>> {
    let limit = params.limit.clamp(1, 100);
    let collection_id_bytes = decode_identity_collection_id(&collection_id)?;
    let cursor = params
        .cursor
        .as_deref()
        .map(super::assets::decode_activity_cursor)
        .transpose()?;
    let action_filter =
        super::assets::normalize_nft_activity_action_filter(params.action.as_deref())?;

    let _agg = state
        .store
        .get_identity_collection_aggregate(&collection_id_bytes)
        .map_err(|e| ApiError::internal(e.to_string()))?
        .ok_or_else(|| ApiError::not_found("Identity collection not found"))?;

    let results = list_canonical_nft_collection_activities_page(
        state.store.as_ref(),
        state.store.as_ref(),
        &collection_id_bytes,
        (limit as usize) + 1,
        cursor,
        action_filter.as_deref(),
    )
    .map_err(|e| ApiError::internal(e.to_string()))?;

    let has_more = results.len() as i64 > limit;
    let page: Vec<NftCollectionActivityResponse> = results
        .into_iter()
        .take(limit as usize)
        .map(|(block_number, tx_index, entry)| {
            let actions: Vec<String> = entry
                .actions
                .iter()
                .map(|a| match a {
                    ckbadger_store::AssetAction::Mint => "mint".to_string(),
                    ckbadger_store::AssetAction::Transfer => "transfer".to_string(),
                    ckbadger_store::AssetAction::Burn => "burn".to_string(),
                    ckbadger_store::AssetAction::Recycle => "recycle".to_string(),
                    ckbadger_store::AssetAction::Renew => "renew".to_string(),
                    ckbadger_store::AssetAction::Update => "update".to_string(),
                })
                .collect();
            NftCollectionActivityResponse {
                tx_hash: format!("0x{}", hex::encode(&entry.tx_hash)),
                block_number,
                tx_index,
                timestamp: entry.timestamp_ms.to_string(),
                actions,
            }
        })
        .collect();

    let next_cursor = if has_more {
        page.last()
            .map(|row| format!("{}:{}", row.block_number, row.tx_index))
    } else {
        None
    };

    ok(CursorPaginatedResponse::without_total(
        page,
        limit,
        next_cursor,
    ))
}
```

**Step 3: Add items endpoint to identities.rs**

This is the most complex endpoint — it must handle both .bit and did:ckb items with search/status filtering. Reuse helpers from `assets.rs`.

```rust
async fn list_identity_collection_items(
    State(state): State<Arc<AppState>>,
    Path(collection_id): Path<String>,
    Query(params): Query<NftItemsParams>,
) -> ApiResult<CursorPaginatedResponse<NftCollectionItemResponse>> {
    let limit = params.limit.clamp(1, 100);
    let collection_id_bytes = decode_identity_collection_id(&collection_id)?;
    let search = super::assets::normalize_nft_items_search(params.search.as_deref());
    let status_filter = super::assets::normalize_nft_items_status(params.status.as_deref())?;
    let cursor_bytes = params
        .cursor
        .as_deref()
        .map(|c| {
            hex::decode(c.strip_prefix("0x").unwrap_or(c))
                .map_err(|_| ApiError::bad_request("Invalid cursor"))
        })
        .transpose()?;

    let _agg = state
        .store
        .get_identity_collection_aggregate(&collection_id_bytes)
        .map_err(|e| ApiError::internal(e.to_string()))?
        .ok_or_else(|| ApiError::not_found("Identity collection not found"))?;

    // Delegate to the existing item listing logic in assets.rs.
    // The existing list_nft_collection_items already branches correctly
    // for identity items. We call it via the shared handler.
    // (This is handled by re-calling the same inner logic.)

    // For now, forward to the existing endpoint logic.
    // The items listing is already correct in assets.rs (reads CF_IDENTITY_DATA).
    // We just need to wire the route.
    super::assets::list_identity_items_inner(
        state.store.as_ref(),
        state.append_only_store.as_ref(),
        &collection_id_bytes,
        limit,
        cursor_bytes,
        search.as_deref(),
        status_filter,
    )
}
```

Note: Extract the item listing logic from the existing `list_nft_collection_items` handler in `assets.rs` into a `pub(crate) fn list_identity_items_inner(...)` function. The existing handler already correctly branches for identity items — we just need to expose the inner logic.

**Step 4: Run `cargo check`**

Run: `cargo check -p ckbadger-api`
Expected: PASS

**Step 5: Commit**

```
feat(api): add identity collection activities and items endpoints

Complete set of /assets/identities/{collection_id}/* endpoints.
Activities reuse canonical-filtered listing. Items reuse existing
identity item listing logic from assets.rs.
```

---

### Task 4: Add backend integration tests

**Files:**

- Modify: `crates/api/tests/api_integration.rs`

**Step 1: Add test for identity collection detail endpoint**

```rust
#[tokio::test]
async fn test_identity_collection_detail_returns_identity_aggregate() {
    // Setup: create domain store with identity aggregate for dotbit sentinel
    // Write a few identity collection activities
    // Call GET /assets/identities/dotbit
    // Assert: response has correct standard, counts, and activities_count
    //         matches the actual number of canonical activities
}
```

**Step 2: Add test for identity holders reading correct CF**

```rust
#[tokio::test]
async fn test_identity_collection_holders_reads_stats_spore() {
    // Setup: create domain store with cluster_owner entries in CF_STATS_SPORE
    //        for the dotbit sentinel collection
    // Call GET /assets/identities/dotbit/holders
    // Assert: holders are returned (not empty)
    // Verify: calling /assets/nfts/dotbit/holders would return empty (the bug)
}
```

**Step 3: Add test for identity activities**

```rust
#[tokio::test]
async fn test_identity_collection_activities_endpoint() {
    // Setup: write identity collection activities and canonical txs
    // Call GET /assets/identities/dotbit/activities
    // Assert: activities returned with correct format
}
```

**Step 4: Run tests**

Run: `cargo test -p ckbadger-api test_identity_collection`
Expected: PASS

**Step 5: Commit**

```
test(api): add identity collection endpoint integration tests
```

---

### Task 5: Create frontend identity collection page

**Files:**

- Create: `frontend/app/identities/[collectionId]/page.tsx`
- Create: `frontend/app/identities/[collectionId]/client-page.tsx`

**Step 1: Create route wrapper**

```tsx
// frontend/app/identities/[collectionId]/page.tsx
import IdentityCollectionPage from './client-page';

interface Props {
  params: Promise<{ collectionId: string }>;
}

export default async function Page({ params }: Props) {
  const { collectionId } = await params;
  return <IdentityCollectionPage collectionId={collectionId} />;
}
```

**Step 2: Create client page component**

```tsx
// frontend/app/identities/[collectionId]/client-page.tsx
'use client';
```

This component should:

- Fetch identity collection detail from new `/assets/identities/{id}` endpoint
- Display stat cards with `totalLabel="Total Identities"`
- Three tabs: Activities, Identities (not "NFTs"), Holders
- For .bit items: show name, ID, status (Live/Recycled), expiry, cell link, owner link
- For did:ckb items: show name, ID, status (Live/Recycled), cell link, owner link
- Cursor-paginated lists for all three tabs
- Search + status filter for the Identities tab
- Reuse existing components: `NftActivityCard`, `NftCollectionStatCards` (with custom label), `CursorPagination`, `TerminalPanel`, etc.
- Link items to existing detail pages: `/nfts/dotbit/{nftId}` or `/nfts/did/{nftId}`

Key differences from the NFT page:

- No media preview / DoB rendering / cluster info
- No occupation chart (identity collections don't have meaningful capacity occupation)
- No storage profile
- Tab says "Identities" not "NFTs"
- Simpler layout — collection info + tabs only

**Step 3: Run type-check**

Run: `cd frontend && pnpm type-check`
Expected: PASS

**Step 4: Commit**

```
feat(frontend): add dedicated identity collection page

New route at /identities/[collectionId]/ with identity-specific
page template. Tabs: Activities, Identities, Holders.
```

---

### Task 6: Add frontend API methods for identity endpoints

**Files:**

- Modify: `frontend/lib/api.ts`

**Step 1: Add TypeScript types and API methods**

```typescript
// New identity collection type (no capacity/storage fields)
interface IdentityCollection {
  collectionId: string;
  standard: string;
  name: string | null;
  totalCount: number;
  liveCount: number;
  holdersCount: number;
  activitiesCount: number;
}

// API methods
getIdentityCollection: (collectionId: string): Promise<IdentityCollection> =>
  fetchJson(`/assets/identities/${normalizeNftAssetId(collectionId)}`),

getIdentityCollectionItems: (
  collectionId: string,
  params?: NftCollectionItemsParams
): Promise<CursorPaginatedResponse<NftCollectionItem>> =>
  fetchJson(`/assets/identities/${normalizeNftAssetId(collectionId)}/items`, params),

getIdentityCollectionHolders: (
  collectionId: string,
  params?: CursorQueryParams
): Promise<CursorPaginatedResponse<NftCollectionHolder>> =>
  fetchJson(`/assets/identities/${normalizeNftAssetId(collectionId)}/holders`, params),

getIdentityCollectionActivities: (
  collectionId: string,
  params?: NftCollectionActivitiesParams
): Promise<CursorPaginatedResponse<NftCollectionActivity>> =>
  fetchJson(`/assets/identities/${normalizeNftAssetId(collectionId)}/activities`, params),
```

**Step 2: Run type-check**

Run: `cd frontend && pnpm type-check`
Expected: PASS

**Step 3: Commit**

```
feat(frontend): add identity collection API methods
```

---

### Task 7: Update identity item detail pages to link back to identity collection

**Files:**

- Modify: `frontend/components/nft/identity-nft-item-detail.tsx`
- Modify: `frontend/app/nfts/did/[nftId]/client-page.tsx`
- Modify: `frontend/app/nfts/dotbit/[nftId]/client-page.tsx`

**Step 1: Update back links to point to `/identities/` routes**

In `identity-nft-item-detail.tsx`, change the "Back to NFTs" link:

```tsx
// OLD:
<Link href="/assets?type=nft">Back to NFTs</Link>

// NEW:
<Link href="/assets?type=nft">Back to Identities</Link>
```

Update the config `backHref` values:

- `.bit`: `'/identities/dotbit'`
- `did:ckb`: `'/identities/did:ckb'`

**Step 2: Run type-check**

Run: `cd frontend && pnpm type-check`
Expected: PASS

**Step 3: Commit**

```
fix(frontend): update identity item back links to /identities/ routes
```

---

### Task 8: Remove identity branching from NFT collection page

**Files:**

- Modify: `frontend/app/nfts/[sporeId]/client-page.tsx`

**Step 1: Redirect identity aliases to the new route**

At the top of the `SporeDetailPage` component, after detecting identity aliases, redirect:

```tsx
const isDotbitCollection = isDotbitAlias(rawAssetId);
const isDidCkbCollection = isDidCkbAlias(rawAssetId);

useEffect(() => {
  if (isDotbitCollection) {
    router.replace('/identities/dotbit');
  } else if (isDidCkbCollection) {
    router.replace('/identities/did:ckb');
  }
}, [isDotbitCollection, isDidCkbCollection, router]);

if (isDotbitCollection || isDidCkbCollection) {
  return null; // redirecting
}
```

**Step 2: Remove identity-specific code from the NFT page**

After the redirect is in place, remove:

- `isDotbitCollectionView`, `isDidCkbCollectionView` variables and all branching
- `supportsCollectionFilters` (always false now)
- `.bit` item rendering blocks in the NFTs tab
- `collectionSearchLabel`, `collectionInactiveStatusLabel` identity cases

This cleanup can be done incrementally — the redirect ensures users never see the NFT page for identity collections.

**Step 3: Run type-check and lint**

Run: `cd frontend && pnpm type-check && pnpm lint`
Expected: PASS

**Step 4: Commit**

```
refactor(frontend): redirect identity aliases from NFT page to /identities/

Identity collections now use their dedicated page template.
The NFT page no longer handles identity-specific branching.
```

---

### Task 9: Update frontend tests

**Files:**

- Create: `frontend/__tests__/pages/identity-collection.test.tsx`
- Modify: `frontend/__tests__/pages/nft-detail.test.tsx`

**Step 1: Add identity collection page tests**

Test cases:

- Renders identity collection detail with correct stat labels ("Total Identities")
- Activities tab shows correct count and list
- Identities tab shows items with search/filter
- Holders tab shows holders
- Items link to correct detail pages

**Step 2: Update NFT detail tests to verify redirect for identity aliases**

Remove or update tests that test identity collection rendering within the NFT page. Add test that identity aliases redirect to `/identities/`.

**Step 3: Run tests**

Run: `cd frontend && npx vitest run`
Expected: PASS

**Step 4: Commit**

```
test(frontend): add identity collection page tests, update NFT page tests
```

---

### Task 10: Final verification

**Step 1: Run full backend check**

Run: `cargo check && cargo clippy && cargo test`
Expected: PASS

**Step 2: Run full frontend check**

Run: `cd frontend && pnpm type-check && pnpm lint && npx vitest run`
Expected: PASS

**Step 3: Verify the fixes address both reported issues**

1. **Activities count**: The new `/assets/identities/{id}` endpoint returns canonical activities count (matches list), not the aggregate count.
2. **NFTs tab renamed**: The identity page has an "Identities" tab, not "NFTs".
3. **Bonus fix**: Identity holders now read from the correct CF.

**Step 4: Commit any remaining fixes**

---

## Re-sync Note

After deploying these changes, a DB re-sync from genesis is recommended to ensure identity collection aggregates (total_count, live_count, holders_count) are accurate. The activities_count in the aggregate is no longer used for display (replaced by canonical count), but other aggregate fields benefit from a clean rebuild.
