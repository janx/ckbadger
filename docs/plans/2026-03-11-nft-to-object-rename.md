# NFT → Object / Identity Rename — Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Rename all "NFT" terminology to "Object" (for spore/m-nft assets) and "Identity" (for dotbit/did_ckb assets) across backend API and frontend. M-NFT/mNFT stay as protocol standard names.

**Architecture:** Backend-first rename (Rust API routes + types), then frontend API layer, then routes/pages/components, then tests. Each task can be verified independently. Identity item endpoints move from assets.rs to identities.rs; all other changes are in-place renames.

**Tech Stack:** Rust (Axum 0.8), TypeScript (React 19, Vite, TanStack Query v5)

---

## Task 1: Backend — Rename spore.rs routes

**Files:**

- Modify: `crates/api/src/routes/spore.rs`

**Step 1: Rename route paths and handler names**

In the `routes()` function (line ~24), rename all `/spore/nfts` paths to `/spore/objects`:

```rust
// BEFORE:
.route("/spore/nfts", get(list_spores))
.route("/spore/nfts/{spore_id}", get(get_spore))
.route("/spore/nfts/{spore_id}/decode", get(decode_spore))
.route("/spore/nfts/{spore_id}/charts/occupation", get(get_spore_occupation_chart))

// AFTER:
.route("/spore/objects", get(list_spores))
.route("/spore/objects/{spore_id}", get(get_spore))
.route("/spore/objects/{spore_id}/decode", get(decode_spore))
.route("/spore/objects/{spore_id}/charts/occupation", get(get_spore_occupation_chart))
```

Handler function names (`list_spores`, `get_spore`, `decode_spore`, `get_spore_occupation_chart`) stay unchanged — they already use "spore" not "nft".

**Step 2: Verify**

Run: `cargo check -p ckbadger-api`
Expected: compiles clean

**Step 3: Commit**

```
feat(api): rename /spore/nfts routes to /spore/objects
```

---

## Task 2: Backend — Rename shared response types in assets.rs

**Files:**

- Modify: `crates/api/src/routes/assets.rs`

**Step 1: Rename response structs (find-and-replace, whole file)**

| Old name                        | New name                      |
| ------------------------------- | ----------------------------- |
| `NftCollectionItemResponse`     | `CollectionItemResponse`      |
| `NftCollectionHolderResponse`   | `CollectionHolderResponse`    |
| `NftCollectionActivityResponse` | `CollectionActivityResponse`  |
| `NftItemsParams`                | `ObjectItemsParams`           |
| `NftCollectionActivitiesParams` | `CollectionActivitiesParams`  |
| `NftCollectionHoldersParams`    | `CollectionHoldersParams`     |
| `MnftItemActivityResponse`      | stays (mNFT is protocol name) |
| `MnftItemDetailResponse`        | stays                         |
| `MnftItemActivitiesParams`      | stays                         |

Apply these renames across the entire file (struct definitions, all usages in handler return types and function bodies).

**Step 2: Update identities.rs imports**

`identities.rs` imports shared types from `assets.rs`. Update those imports to use new names:

```rust
// identities.rs — update use statement
use super::assets::{
    CollectionItemResponse,        // was NftCollectionItemResponse
    CollectionHolderResponse,      // was NftCollectionHolderResponse
    CollectionActivityResponse,    // was NftCollectionActivityResponse
    CollectionActivitiesParams,    // was NftCollectionActivitiesParams
    CollectionHoldersParams,       // was NftCollectionHoldersParams
};
```

**Step 3: Verify**

Run: `cargo check -p ckbadger-api`
Expected: compiles clean

**Step 4: Commit**

```
refactor(api): rename NftCollection* response types to Collection*
```

---

## Task 3: Backend — Rename assets.rs route paths and handlers

**Files:**

- Modify: `crates/api/src/routes/assets.rs`

**Step 1: Rename route paths in routes() function**

In the `routes()` function (line ~69), rename all `/assets/nfts` to `/assets/objects`. Also rename `{nft_id}` to `{object_id}` for object routes:

```rust
// BEFORE:
.route("/assets/nfts/items/{nft_id}", get(get_nft_item_detail))
.route("/assets/nfts/items/{nft_id}/activities", get(list_mnft_item_activities))
.route("/assets/nfts/{collection_id}", get(get_nft_collection))
.route("/assets/nfts/{collection_id}/items", get(list_nft_collection_items))
.route("/assets/nfts/{collection_id}/holders", get(list_nft_collection_holders))
.route("/assets/nfts/{collection_id}/activities", get(list_nft_collection_activities))
.route("/assets/nfts/{collection_id}/charts/occupation", get(get_nft_collection_occupation_chart))

// AFTER:
.route("/assets/objects/items/{object_id}", get(get_object_item_detail))
.route("/assets/objects/items/{object_id}/activities", get(list_mnft_item_activities))
.route("/assets/objects/{collection_id}", get(get_object_collection))
.route("/assets/objects/{collection_id}/items", get(list_object_collection_items))
.route("/assets/objects/{collection_id}/holders", get(list_object_collection_holders))
.route("/assets/objects/{collection_id}/activities", get(list_object_collection_activities))
.route("/assets/objects/{collection_id}/charts/occupation", get(get_object_collection_occupation_chart))
```

**Remove** these routes from assets.rs (they move to identities.rs in Task 4):

```rust
// DELETE from routes():
.route("/assets/nfts/dotbit/items/{nft_id}", get(get_dotbit_item_detail))
.route("/assets/nfts/did/items/{nft_id}", get(get_did_ckb_item_detail))
.route("/assets/nfts/dotbit/items/{nft_id}/activities", get(list_dotbit_item_activities))
.route("/assets/nfts/did/items/{nft_id}/activities", get(list_did_ckb_item_activities))
```

**Step 2: Rename handler functions**

| Old function name                     | New function name                        |
| ------------------------------------- | ---------------------------------------- |
| `get_nft_item_detail`                 | `get_object_item_detail`                 |
| `get_nft_collection`                  | `get_object_collection`                  |
| `list_nft_collection_items`           | `list_object_collection_items`           |
| `list_nft_collection_holders`         | `list_object_collection_holders`         |
| `list_nft_collection_activities`      | `list_object_collection_activities`      |
| `get_nft_collection_occupation_chart` | `get_object_collection_occupation_chart` |
| `list_mnft_item_activities`           | stays                                    |

In `get_object_item_detail`, rename the `Path` extractor param from `nft_id` to `object_id`.
In `list_mnft_item_activities`, rename the `Path` extractor param from `nft_id` to `object_id`.

Also rename any internal helper like `decode_nft_item_id` → `decode_object_item_id` and `normalize_nft_activity_action_filter` → `normalize_activity_action_filter` (used by both object and identity — identities.rs will need pub access).

**Step 3: Make moved helpers pub(super)**

The dotbit/did handlers will move to identities.rs. Ensure any shared helpers (`decode_object_item_id`, `normalize_activity_action_filter`) are `pub(super)` so identities.rs can use them:

```rust
pub(super) fn decode_object_item_id(nft_id: &str) -> Result<Vec<u8>, ApiError> { ... }
pub(super) fn normalize_activity_action_filter(action: Option<&str>) -> Result<Option<String>, ApiError> { ... }
```

**Step 4: Verify**

Run: `cargo check -p ckbadger-api`
Expected: may show errors for removed dotbit/did handlers (expected — Task 4 adds them to identities.rs)

**Step 5: Commit** (after Task 4 is also complete)

---

## Task 4: Backend — Move dotbit/did item endpoints to identities.rs

**Files:**

- Modify: `crates/api/src/routes/identities.rs`
- Modify: `crates/api/src/routes/assets.rs` (delete moved handlers)

**Step 1: Add new routes to identities.rs routes() function**

```rust
pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        // Existing collection routes:
        .route("/assets/identities/{collection_id}", get(get_identity_collection))
        .route("/assets/identities/{collection_id}/holders", get(list_identity_collection_holders))
        .route("/assets/identities/{collection_id}/activities", get(list_identity_collection_activities))
        .route("/assets/identities/{collection_id}/items", get(list_identity_collection_items))
        // NEW item-level routes:
        .route("/assets/identities/dotbit/items/{identity_id}", get(get_dotbit_item_detail))
        .route("/assets/identities/dotbit/items/{identity_id}/activities", get(list_dotbit_item_activities))
        .route("/assets/identities/did/items/{identity_id}", get(get_did_ckb_item_detail))
        .route("/assets/identities/did/items/{identity_id}/activities", get(list_did_ckb_item_activities))
}
```

**Step 2: Move handler functions from assets.rs to identities.rs**

Move these 4 functions:

- `get_dotbit_item_detail` (was at assets.rs:2547)
- `list_dotbit_item_activities` (was at assets.rs:1681)
- `get_did_ckb_item_detail` (was at assets.rs:2619)
- `list_did_ckb_item_activities` (was at assets.rs:1717)

In each moved function, rename `Path(nft_id)` → `Path(identity_id)` and update the variable name throughout the function body.

Import shared helpers from assets:

```rust
use super::assets::{
    CollectionItemResponse,
    decode_object_item_id,           // renamed from decode_nft_item_id
    normalize_activity_action_filter, // renamed from normalize_nft_activity_action_filter
    MnftItemActivityResponse,        // reuse for identity activities (same shape)
    MnftItemActivitiesParams,        // reuse
};
```

**Step 3: Delete the 4 moved handler functions from assets.rs**

Also delete any now-unused imports in assets.rs.

**Step 4: Verify**

Run: `cargo check -p ckbadger-api && cargo clippy -p ckbadger-api`
Expected: compiles clean, no warnings

**Step 5: Commit**

```
feat(api): rename /assets/nfts → /assets/objects, move identity item endpoints to identities.rs
```

---

## Task 5: Backend — Run full cargo check + clippy

**Step 1: Full workspace check**

Run: `cargo check && cargo clippy`
Expected: clean

**Step 2: Run tests**

Run: `cargo test -p ckbadger-api`
Expected: tests pass (integration tests may not exist for these specific routes)

**Step 3: Commit if any fixups needed**

---

## Task 6: Frontend — Rename API types in lib/api.ts

**Files:**

- Modify: `frontend/lib/api.ts`

**Step 1: Rename type definitions (find-and-replace across file)**

| Old                             | New                          |
| ------------------------------- | ---------------------------- |
| `NftCollection` (interface)     | `ObjectCollection`           |
| `NftCollectionItem`             | `CollectionItem`             |
| `NftCollectionHolder`           | `CollectionHolder`           |
| `NftCollectionActivity`         | `CollectionActivity`         |
| `NftItemStatusFilter`           | `ItemStatusFilter`           |
| `NftCollectionItemsParams`      | `CollectionItemsParams`      |
| `NftCollectionHoldersParams`    | `CollectionHoldersParams`    |
| `NftCollectionActivitiesParams` | `CollectionActivitiesParams` |

Update all usages within api.ts (function signatures, return types, export statements).

**Step 2: Rename API functions and update endpoint URLs**

| Old function                      | New function                         | Old URL                                      | New URL                                            |
| --------------------------------- | ------------------------------------ | -------------------------------------------- | -------------------------------------------------- |
| `getSporeNfts`                    | `getSporeObjects`                    | `/spore/nfts`                                | `/spore/objects`                                   |
| `getSporeNft`                     | `getSporeObject`                     | `/spore/nfts/${id}`                          | `/spore/objects/${id}`                             |
| `getSporeNftDecoded`              | `getSporeObjectDecoded`              | `/spore/nfts/${id}/decode`                   | `/spore/objects/${id}/decode`                      |
| `getSporeNftOccupationChart`      | `getSporeObjectOccupationChart`      | `/spore/nfts/${id}/charts/occupation`        | `/spore/objects/${id}/charts/occupation`           |
| `getNftCollection`                | `getObjectCollection`                | `/assets/nfts/${id}`                         | `/assets/objects/${id}`                            |
| `getNftCollectionOccupationChart` | `getObjectCollectionOccupationChart` | `/assets/nfts/${id}/charts/occupation`       | `/assets/objects/${id}/charts/occupation`          |
| `getNftCollectionItems`           | `getObjectCollectionItems`           | `/assets/nfts/${id}/items`                   | `/assets/objects/${id}/items`                      |
| `getNftCollectionHolders`         | `getObjectCollectionHolders`         | `/assets/nfts/${id}/holders`                 | `/assets/objects/${id}/holders`                    |
| `getNftCollectionActivities`      | `getObjectCollectionActivities`      | `/assets/nfts/${id}/activities`              | `/assets/objects/${id}/activities`                 |
| `getMnftItemDetail`               | stays                                | `/assets/nfts/items/${id}`                   | `/assets/objects/items/${id}`                      |
| `getMnftItemActivities`           | stays                                | `/assets/nfts/items/${id}/activities`        | `/assets/objects/items/${id}/activities`           |
| `getDotbitItemDetail`             | stays                                | `/assets/nfts/dotbit/items/${id}`            | `/assets/identities/dotbit/items/${id}`            |
| `getDotbitItemActivities`         | stays                                | `/assets/nfts/dotbit/items/${id}/activities` | `/assets/identities/dotbit/items/${id}/activities` |
| `getDidCkbItemDetail`             | stays                                | `/assets/nfts/did/items/${id}`               | `/assets/identities/did/items/${id}`               |
| `getDidCkbItemActivities`         | stays                                | `/assets/nfts/did/items/${id}/activities`    | `/assets/identities/did/items/${id}/activities`    |

**Step 3: Remove `normalizeNftAssetId` calls**

In the renamed `getObjectCollection`, `getObjectCollectionItems`, etc., remove the `normalizeNftAssetId(collectionId)` call. Pass `collectionId` directly — object collection IDs are hex, no alias normalization needed.

Remove the import of `normalizeNftAssetId` from `@/lib/nft-collections`.

**Step 4: Update `getIdentityCollectionItems`, `getIdentityCollectionHolders`, `getIdentityCollectionActivities` return types**

These already exist and return `NftCollectionItem`, `NftCollectionHolder`, `NftCollectionActivity` — update to use the new names: `CollectionItem`, `CollectionHolder`, `CollectionActivity`.

**Step 5: Verify**

Run: `cd frontend && pnpm type-check`
Expected: type errors in consumers (pages/components still use old names) — that's expected, will fix in later tasks.

**Step 6: Commit**

```
feat(frontend): rename NFT API types and functions to Object/Collection
```

---

## Task 7: Frontend — Delete nft-collections.ts, rename nft-utils.ts

**Files:**

- Delete: `frontend/lib/nft-collections.ts`
- Rename: `frontend/lib/nft-utils.ts` → `frontend/lib/asset-utils.ts`
- Modify: `frontend/lib/asset-utils.ts` (update comments)

**Step 1: Delete nft-collections.ts**

Delete `frontend/lib/nft-collections.ts`. All its exports (`DOTBIT_COLLECTION_ID`, `DID_CKB_COLLECTION_ID`, `isDotbitAlias`, `isDidCkbAlias`, `normalizeNftAssetId`, `toNftDetailSlug`) are no longer needed:

- `normalizeNftAssetId` was removed from api.ts (Task 6)
- `toNftDetailSlug` was used by `getNftDetailHref` (replaced in Task 8)
- `isDotbitAlias`/`isDidCkbAlias` — check if `identities/[collectionId]/client-page.tsx` uses these; if so, inline them there

**Step 2: Rename nft-utils.ts to asset-utils.ts**

```bash
git mv frontend/lib/nft-utils.ts frontend/lib/asset-utils.ts
```

Update the doc comment at top:

```typescript
/**
 * Shared utility functions for asset detail pages (Objects + Identities).
 */
```

Rename `normalizeNftId` → `normalizeAssetId` throughout the file.

**Step 3: Update detail-routes.ts**

Replace `getNftDetailHref` with two functions:

```typescript
// frontend/lib/detail-routes.ts — remove old import, add new functions

export function getObjectDetailHref(assetId: string): string {
  return `/objects/${encodeURIComponent(assetId)}`;
}

export function getIdentityItemDetailHref(standard: string, identityId: string): string {
  if (standard === 'dotbit') return `/identities/dotbit/${encodeURIComponent(identityId)}`;
  if (standard === 'did_ckb' || standard === 'did:ckb')
    return `/identities/did/${encodeURIComponent(identityId)}`;
  // Fallback for mnft items under objects
  return `/objects/mnft/${encodeURIComponent(identityId)}`;
}
```

Remove the `import { toNftDetailSlug } from '@/lib/nft-collections'` line.

**Step 4: Update search-routing.ts**

```typescript
// Line 15-16: BEFORE
if (bodyLower === 'did:ckb' || bodyLower === 'did_ckb') {
  return '/nfts/did:ckb';
}

// AFTER
if (bodyLower === 'did:ckb' || bodyLower === 'did_ckb') {
  return '/identities/did:ckb';
}

// Line 51-53: BEFORE
if (intent.prefix === 'spore') {
  const hash = normalizeHash32(body);
  return hash ? `/nfts/${hash}` : null;
}

// AFTER
if (intent.prefix === 'spore') {
  const hash = normalizeHash32(body);
  return hash ? `/objects/${hash}` : null;
}
```

**Step 5: Commit**

```
refactor(frontend): delete nft-collections.ts, rename nft-utils → asset-utils, update detail-routes + search-routing
```

---

## Task 8: Frontend — Move and rename page files

**Files:**

- Move: `frontend/app/nfts/` → split into `frontend/app/objects/` and additions to `frontend/app/identities/`

**Step 1: Create directory structure**

```bash
mkdir -p frontend/app/objects/[sporeId]
mkdir -p frontend/app/objects/mnft/[objectId]
mkdir -p frontend/app/identities/dotbit/[identityId]
mkdir -p frontend/app/identities/did/[identityId]
```

**Step 2: Move and rename spore detail pages**

```bash
git mv frontend/app/nfts/[sporeId]/page.tsx frontend/app/objects/[sporeId]/page.tsx
git mv frontend/app/nfts/[sporeId]/client-page.tsx frontend/app/objects/[sporeId]/client-page.tsx
```

In `client-page.tsx`: update all internal `/nfts/` hrefs to use `/objects/` or `/identities/` as appropriate. Update API calls from `getSporeNft` → `getSporeObject`, `getNftCollection` → `getObjectCollection`, etc. Update type names. Replace `normalizeNftId` imports with `normalizeAssetId` from `@/lib/asset-utils`. Replace all "NFT" UI text with "Object" (except M-NFT/mNFT).

**Step 3: Move and rename mnft item detail pages**

```bash
git mv frontend/app/nfts/mnft/[nftId]/page.tsx frontend/app/objects/mnft/[objectId]/page.tsx
git mv frontend/app/nfts/mnft/[nftId]/client-page.tsx frontend/app/objects/mnft/[objectId]/client-page.tsx
```

In both files: rename `nftId` prop/param to `objectId`. Update imports, href links, API calls.

**Step 4: Move and rename dotbit item detail pages**

```bash
git mv frontend/app/nfts/dotbit/[nftId]/page.tsx frontend/app/identities/dotbit/[identityId]/page.tsx
git mv frontend/app/nfts/dotbit/[nftId]/client-page.tsx frontend/app/identities/dotbit/[identityId]/client-page.tsx
```

In `client-page.tsx`: rename `nftId` → `identityId`, update interface name `DotbitItemDetailPageProps` → keep or rename to `DotbitItemDetailProps`, update `IdentityNftItemDetail` → `IdentityItemDetail` (component renamed in Task 10).

**Step 5: Move and rename did item detail pages**

```bash
git mv frontend/app/nfts/did/[nftId]/page.tsx frontend/app/identities/did/[identityId]/page.tsx
git mv frontend/app/nfts/did/[nftId]/client-page.tsx frontend/app/identities/did/[identityId]/client-page.tsx
```

Same updates as Step 4 but for did:ckb.

**Step 6: Create objects redirect page**

Create `frontend/app/objects/page.tsx`:

```typescript
import { redirect } from '@/src/navigation';

export default function ObjectsPage() {
  redirect('/assets?type=object');
}
```

**Step 7: Delete old nfts directory**

```bash
git rm -r frontend/app/nfts/
```

**Step 8: Commit**

```
feat(frontend): move NFT pages to /objects and /identities routes
```

---

## Task 9: Frontend — Update router.tsx

**Files:**

- Modify: `frontend/src/routes/router.tsx`

**Step 1: Update lazy route imports and param mappings**

```typescript
// BEFORE (lines 128-151):
const SporeDetailRoute = lazyParamPage(
  () => import('@/app/nfts/[sporeId]/client-page'),
  (params) => ({ sporeId: params.sporeId ?? '' })
);
const MnftItemDetailRoute = lazyParamPage(
  () => import('@/app/nfts/mnft/[nftId]/client-page'),
  (params) => ({ nftId: params.nftId ?? '' })
);
const DotbitItemDetailRoute = lazyParamPage(
  () => import('@/app/nfts/dotbit/[nftId]/client-page'),
  (params) => ({ nftId: params.nftId ?? '' })
);
const DidCkbItemDetailRoute = lazyParamPage(
  () => import('@/app/nfts/did/[nftId]/client-page'),
  (params) => ({ nftId: params.nftId ?? '' })
);

// AFTER:
const SporeDetailRoute = lazyParamPage(
  () => import('@/app/objects/[sporeId]/client-page'),
  (params) => ({ sporeId: params.sporeId ?? '' })
);
const MnftItemDetailRoute = lazyParamPage(
  () => import('@/app/objects/mnft/[objectId]/client-page'),
  (params) => ({ objectId: params.objectId ?? '' })
);
const DotbitItemDetailRoute = lazyParamPage(
  () => import('@/app/identities/dotbit/[identityId]/client-page'),
  (params) => ({ identityId: params.identityId ?? '' })
);
const DidCkbItemDetailRoute = lazyParamPage(
  () => import('@/app/identities/did/[identityId]/client-page'),
  (params) => ({ identityId: params.identityId ?? '' })
);
```

**Step 2: Update route path definitions**

```typescript
// BEFORE (lines 361-376):
{ path: 'nfts/:sporeId', element: <SporeDetailRoute /> },
{ path: 'nfts/mnft/:nftId', element: <MnftItemDetailRoute /> },
{ path: 'nfts/dotbit/:nftId', element: <DotbitItemDetailRoute /> },
{ path: 'nfts/did/:nftId', element: <DidCkbItemDetailRoute /> },

// AFTER:
{ path: 'objects/:sporeId', element: <SporeDetailRoute /> },
{ path: 'objects/mnft/:objectId', element: <MnftItemDetailRoute /> },
{ path: 'identities/dotbit/:identityId', element: <DotbitItemDetailRoute /> },
{ path: 'identities/did/:identityId', element: <DidCkbItemDetailRoute /> },
```

**Step 3: Commit**

```
feat(frontend): update router.tsx for /objects and /identities routes
```

---

## Task 10: Frontend — Rename and split components/nft/

**Files:**

- Move: `frontend/components/nft/nft-activity-card.tsx` → `frontend/components/object/object-activity-card.tsx`
- Move: `frontend/components/nft/nft-collection-stat-cards.tsx` → `frontend/components/object/object-collection-stat-cards.tsx`
- Move: `frontend/components/nft/identity-nft-item-detail.tsx` → `frontend/components/identity/identity-item-detail.tsx`
- Create: `frontend/components/identity/identity-activity-card.tsx`
- Delete: `frontend/components/nft/` directory

**Step 1: Create directories**

```bash
mkdir -p frontend/components/object
mkdir -p frontend/components/identity
```

**Step 2: Move and rename object-activity-card**

```bash
git mv frontend/components/nft/nft-activity-card.tsx frontend/components/object/object-activity-card.tsx
```

Rename in the file:

- `NftActivityCardProps` → `ObjectActivityCardProps`
- `NftActivityCard` → `ObjectActivityCard`

**Step 3: Move and rename object-collection-stat-cards**

```bash
git mv frontend/components/nft/nft-collection-stat-cards.tsx frontend/components/object/object-collection-stat-cards.tsx
```

Rename in the file:

- `NftCollectionStatCardsProps` → `ObjectCollectionStatCardsProps`
- `NftCollectionStatCards` → `ObjectCollectionStatCards`
- Default `totalLabel = 'Total NFTs'` → `totalLabel = 'Total Objects'`
- Update import: `from '@/lib/nft-utils'` → `from '@/lib/asset-utils'`

**Step 4: Move and rename identity-item-detail**

```bash
git mv frontend/components/nft/identity-nft-item-detail.tsx frontend/components/identity/identity-item-detail.tsx
```

Rename in the file:

- `IdentityNftItemDetailConfig` → `IdentityItemDetailConfig`
- `IdentityNftItemDetail` → `IdentityItemDetail`
- Update imports:
  - `from '@/components/nft/nft-activity-card'` → will use new identity activity card (Step 5)
  - `from '@/lib/nft-utils'` → `from '@/lib/asset-utils'`
  - `normalizeNftId` → `normalizeAssetId`
  - `NftCollectionItem` → `CollectionItem`
- Rename `nftId` prop to `identityId` in the Props interface and throughout the component

**Step 5: Create identity-activity-card**

Create `frontend/components/identity/identity-activity-card.tsx` — fork from object-activity-card.tsx with identity-optimized naming:

```typescript
import Link from '@/components/ui/link';
import { HexDisplay } from '@/components/ui/hex-display';
import { Badge } from '@/components/ui/page-header';
import { formatNumber } from '@/lib/utils';

interface IdentityActivityCardProps {
  txHash: string;
  blockNumber: number;
  txIndex?: number;
  timestamp?: string;
  actions: string[];
  normalizeAction?: (action: string) => string;
  badgeActions?: boolean;
}

function actionBadgeVariant(action: string): 'green' | 'red' | 'blue' | 'neutral' {
  if (action === 'mint') return 'green';
  if (action === 'burn' || action === 'recycle') return 'red';
  if (action === 'renew') return 'blue';
  return 'neutral';
}

export function IdentityActivityCard({
  txHash,
  blockNumber,
  txIndex,
  timestamp,
  actions,
  normalizeAction,
  badgeActions = false,
}: IdentityActivityCardProps) {
  const displayActions = normalizeAction ? actions.map(normalizeAction) : actions;

  return (
    <div className="border-base-border bg-base-surface/40 space-y-2 rounded border p-3">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <div className="text-text-dim font-mono text-xs">
          Block{' '}
          <Link href={`/blocks/${blockNumber}`} className="text-gold hover:underline">
            #{formatNumber(blockNumber)}
          </Link>
          {txIndex !== undefined && (
            <>
              <span className="text-text-dim mx-1">•</span>
              Tx Index {txIndex}
            </>
          )}
        </div>
        {badgeActions ? (
          <div className="flex flex-wrap gap-1.5">
            {actions.map((action) => (
              <Badge
                key={`${txHash}-${txIndex ?? 0}-${action}`}
                variant={actionBadgeVariant(action)}
              >
                {action}
              </Badge>
            ))}
          </div>
        ) : (
          <div className="text-text font-mono text-xs">{displayActions.join(', ')}</div>
        )}
      </div>
      <Link href={`/tx/${txHash}`} className="text-text block font-mono text-xs hover:underline">
        <HexDisplay value={txHash} size="sm" startChars={14} endChars={10} />
      </Link>
      {timestamp && <div className="text-text-dim font-mono text-xs">Timestamp: {timestamp}</div>}
    </div>
  );
}
```

**Step 6: Update identity-item-detail to use IdentityActivityCard**

In `frontend/components/identity/identity-item-detail.tsx`, change:

```typescript
// BEFORE:
import { NftActivityCard } from '@/components/nft/nft-activity-card';
// AFTER:
import { IdentityActivityCard } from '@/components/identity/identity-activity-card';
```

And replace `<NftActivityCard` with `<IdentityActivityCard` in the JSX.

**Step 7: Delete old components/nft/ directory**

```bash
git rm -r frontend/components/nft/
```

**Step 8: Update all imports across the codebase**

Search for `from '@/components/nft/` and update:

- `'@/components/nft/nft-activity-card'` → `'@/components/object/object-activity-card'`
- `'@/components/nft/nft-collection-stat-cards'` → `'@/components/object/object-collection-stat-cards'`
- `'@/components/nft/identity-nft-item-detail'` → `'@/components/identity/identity-item-detail'`

**Step 9: Commit**

```
refactor(frontend): split components/nft into components/object + components/identity
```

---

## Task 11: Frontend — Update identity collection page links

**Files:**

- Modify: `frontend/app/identities/[collectionId]/client-page.tsx`

**Step 1: Update item detail link prefix**

At line ~71:

```typescript
// BEFORE:
const itemDetailPrefix = isDotbit ? '/nfts/dotbit' : '/nfts/did';
// AFTER:
const itemDetailPrefix = isDotbit ? '/identities/dotbit' : '/identities/did';
```

**Step 2: Update any NftCollectionItem type references**

Replace `NftCollectionItem` → `CollectionItem`, `NftCollectionHolder` → `CollectionHolder`, etc.

**Step 3: Commit**

```
fix(frontend): update identity collection page to use /identities/ item links
```

---

## Task 12: Frontend — UI text replacements in page components

**Files:**

- Modify: `frontend/app/objects/[sporeId]/client-page.tsx` (was nfts/[sporeId])
- Modify: `frontend/app/objects/mnft/[objectId]/client-page.tsx` (was nfts/mnft/[nftId])
- Modify: `frontend/app/clusters/[clusterId]/client-page.tsx`
- Modify: `frontend/components/charts/chart-calculation-descriptions.ts`
- Modify: `frontend/components/latest-activities.tsx`
- Modify: `frontend/lib/format-asset.ts`
- Modify: `frontend/app/assets/assets-page-client.tsx`
- Modify: `frontend/app/address/[addr]/client-page.tsx`

**Step 1: objects/[sporeId]/client-page.tsx** — replace all NFT display text:

| Find                         | Replace                                      |
| ---------------------------- | -------------------------------------------- |
| `← Back to NFTs`             | `← Back to Objects`                          |
| `NFTs (`                     | `Objects (`                                  |
| `NFT Collection`             | `Object Collection`                          |
| `Loading NFTs...`            | `Loading Objects...`                         |
| `Failed to load NFTs`        | `Failed to load Objects`                     |
| `No NFTs in this collection` | `No Objects in this collection`              |
| `totalLabel="NFTs"`          | `totalLabel="Objects"`                       |
| `this NFT collection`        | `this Object collection`                     |
| `this NFT.`                  | `this Object.`                               |
| `/assets?type=nft`           | `/assets?type=object`                        |
| `/nfts/` hrefs               | `/objects/` or `/identities/` as appropriate |

Also update all API function calls and type names per Task 6 renames, and component imports per Task 10 renames.

**Step 2: objects/mnft/[objectId]/client-page.tsx** — update:

- `Back to NFTs` → `Back to Objects`
- `/assets?type=nft` → `/assets?type=object`
- `/nfts/${detail.class.classId}` → `/objects/${detail.class.classId}`
- Rename `nftId` references to `objectId`

**Step 3: clusters/[clusterId]/client-page.tsx**:

- `← Back to NFTs` → `← Back to Objects`
- `NFTs (` tab label → `Objects (`
- Tab value `'nfts'` stays if it's a URL param (check), or rename to `'objects'`
- `/nfts/${sporeId}` links → `/objects/${sporeId}`

**Step 4: chart-calculation-descriptions.ts**:

- `'Ranks token and NFT collection assets by utilization in live state.'` → `'Ranks token and Object collection assets by utilization in live state.'`

**Step 5: latest-activities.tsx**:

- Already shows `'M-NFT'` for m-nft standard — this stays. Check if "Spore" label needs "Object" treatment.

**Step 6: format-asset.ts**:

- `return 'M-NFT'` stays (protocol name)

**Step 7: assets-page-client.tsx**:

- `return 'm-NFT'` stays (protocol name)
- Check for any `/nfts/` href links → update to `/objects/` or `/identities/`

**Step 8: address/[addr]/client-page.tsx**:

- `return 'M-NFT'` stays (protocol name)

**Step 9: Commit**

```
feat(frontend): replace NFT display text with Object across UI
```

---

## Task 13: Frontend — AI/LLM integration updates

**Files:**

- Modify: `frontend/lib/ai/capabilities.ts`
- Modify: `frontend/lib/ai/raw-route.ts`
- Modify: `frontend/lib/ai/markdown-route.ts`
- Modify: `frontend/lib/ai/raw-renderer.ts`
- Modify: `frontend/lib/ai/markdown-renderer.ts`
- Modify: `frontend/public/llms.txt`
- Modify: `frontend/public/llms-full.txt`

**Step 1: capabilities.ts** — update RAW_ROUTE_PROFILES:

```typescript
const RAW_ROUTE_PROFILES: Record<string, readonly string[]> = {
  '/blocks/{id}': ['default'],
  '/cell/{outpoint}': ['default'],
  '/identities/dotbit/{identityId}': ['default'],
  '/identities/did/{identityId}': ['default'],
  '/objects/mnft/{objectId}': ['default'],
  '/tx/{hash}': ['default', 'debugger'],
};
```

**Step 2: raw-route.ts** — update type, patterns, and parser:

```typescript
export type ParsedRawPage =
  | { kind: 'block_detail'; pathname: string; id: string }
  | { kind: 'cell_detail'; pathname: string; outpoint: string }
  | { kind: 'dotbit_item_detail'; pathname: string; identityId: string }
  | { kind: 'did_ckb_item_detail'; pathname: string; identityId: string }
  | { kind: 'mnft_item_detail'; pathname: string; objectId: string }
  | { kind: 'tx_detail'; pathname: string; hash: string }
  | { kind: 'unknown'; pathname: string };

export const RAW_ROUTE_PATTERNS = [
  '/blocks/{id}',
  '/cell/{outpoint}',
  '/identities/dotbit/{identityId}',
  '/identities/did/{identityId}',
  '/objects/mnft/{objectId}',
  '/tx/{hash}',
] as const;
```

Update regex patterns in `parseRawSourcePath`:

- `\/nfts\/dotbit\/` → `\/identities\/dotbit\/` (field: `identityId`)
- `\/nfts\/did\/` → `\/identities\/did\/` (field: `identityId`)
- `\/nfts\/mnft\/` → `\/objects\/mnft\/` (field: `objectId`)

**Step 3: markdown-route.ts** — update type, patterns, and parser:

In `ParsedMarkdownPage` type:

- `'nfts_list'` → `'objects_list'`
- `'nft_detail'` → `'object_detail'`
- `nftId` fields → `identityId` for dotbit/did, `objectId` for mnft

In `MARKDOWN_ROUTE_PATTERNS`:

- `'/nfts'` → `'/objects'`
- `'/nfts/{sporeId}'` → `'/objects/{sporeId}'`
- `'/nfts/dotbit/{nftId}'` → `'/identities/dotbit/{identityId}'`
- `'/nfts/did/{nftId}'` → `'/identities/did/{identityId}'`
- `'/nfts/mnft/{nftId}'` → `'/objects/mnft/{objectId}'`

In `parseMarkdownSourcePath`:

- `normalized === '/nfts'` → `normalized === '/objects'` (kind: `'objects_list'`)
- `\/nfts\/dotbit\/` → `\/identities\/dotbit\/`
- `\/nfts\/did\/` → `\/identities\/did\/`
- `\/nfts\/mnft\/` → `\/objects\/mnft\/`
- `\/nfts\/` (generic) → `\/objects\/`

**Step 4: raw-renderer.ts and markdown-renderer.ts**

Update case statements that handle `dotbit_item_detail`, `did_ckb_item_detail`, `mnft_item_detail` to use new field names (`identityId` instead of `nftId`, `objectId` instead of `nftId`).

In markdown-renderer.ts update display text:

- `'# NFTs'` → `'# Objects'`
- `'# NFT ${...}'` → `'# Object ${...}'`

**Step 5: llms.txt and llms-full.txt**

Replace all `/nfts/` route references with `/objects/` or `/identities/` as appropriate. Replace `{nftId}` with `{objectId}` or `{identityId}`.

**Step 6: Commit**

```
feat(frontend): update AI/LLM integration for Object/Identity routes
```

---

## Task 14: Frontend — Update search bar and command palette

**Files:**

- Modify: `frontend/components/command-palette.tsx` (if exists)
- Modify: `frontend/components/search-bar.tsx` (if exists)

**Step 1: command-palette.tsx**

Find the assets entry with `keywords: ['asset', 'token', 'nft']` and update to `keywords: ['asset', 'token', 'object']`.

**Step 2: search-bar.tsx**

Find `case 'nft':` and update to `case 'object':` (if this still maps to search behavior).

**Step 3: Commit**

```
feat(frontend): update search/command palette for Object terminology
```

---

## Task 15: Frontend — Update and rename test files

**Files:**

- Delete: `frontend/__tests__/lib/nft-collections.test.ts`
- Rename: `frontend/__tests__/lib/nft-utils.test.ts` → `frontend/__tests__/lib/asset-utils.test.ts`
- Rename: `frontend/__tests__/pages/nft-detail.test.tsx` → `frontend/__tests__/pages/object-detail.test.tsx`
- Rename: `frontend/__tests__/pages/nfts-page.test.ts` → `frontend/__tests__/pages/objects-page.test.ts`
- Rename: `frontend/__tests__/components/nft-activity-card.test.tsx` → `frontend/__tests__/components/object-activity-card.test.tsx`
- Rename: `frontend/__tests__/components/nft-collection-stat-cards.test.tsx` → `frontend/__tests__/components/object-collection-stat-cards.test.tsx`
- Rename: `frontend/__tests__/components/identity-nft-item-detail.test.tsx` → `frontend/__tests__/components/identity-item-detail.test.tsx`
- Modify: `frontend/__tests__/pages/cluster.test.tsx`
- Modify: `frontend/__tests__/pages/mnft-item-detail.test.tsx`
- Modify: `frontend/__tests__/pages/assets.test.tsx`
- Modify: `frontend/__tests__/pages/identity-collection.test.tsx`
- Modify: `frontend/__tests__/lib/api.test.ts`
- Modify: `frontend/__tests__/lib/raw-route.test.ts`
- Modify: `frontend/__tests__/lib/markdown-route.test.ts`
- Modify: `frontend/__tests__/lib/raw-renderer.test.ts`
- Modify: `frontend/__tests__/lib/markdown-renderer.test.ts`
- Modify: `frontend/__tests__/lib/search-routing.test.ts`
- Modify: `frontend/__tests__/lib/capabilities.test.ts`
- Modify: `frontend/__tests__/lib/format-asset.test.ts`
- Modify: `frontend/__tests__/lib/tooling-config.test.ts`
- Modify: `frontend/__tests__/pages/most-utilized-assets.test.tsx`

**Step 1: Delete and rename test files**

```bash
git rm frontend/__tests__/lib/nft-collections.test.ts
git mv frontend/__tests__/lib/nft-utils.test.ts frontend/__tests__/lib/asset-utils.test.ts
git mv frontend/__tests__/pages/nft-detail.test.tsx frontend/__tests__/pages/object-detail.test.tsx
git mv frontend/__tests__/pages/nfts-page.test.ts frontend/__tests__/pages/objects-page.test.ts
git mv frontend/__tests__/components/nft-activity-card.test.tsx frontend/__tests__/components/object-activity-card.test.tsx
git mv frontend/__tests__/components/nft-collection-stat-cards.test.tsx frontend/__tests__/components/object-collection-stat-cards.test.tsx
git mv frontend/__tests__/components/identity-nft-item-detail.test.tsx frontend/__tests__/components/identity-item-detail.test.tsx
```

**Step 2: Update all renamed test files**

In each renamed test file, update:

- Import paths (`@/components/nft/` → `@/components/object/` or `@/components/identity/`)
- Component names (`NftActivityCard` → `ObjectActivityCard`, etc.)
- API function mocks (`getSporeNft` → `getSporeObject`, etc.)
- Type names (`NftCollection` → `ObjectCollection`, etc.)
- URL assertions (`/nfts/` → `/objects/` or `/identities/`)
- Button label assertions (`/^NFTs \(/` → `/^Objects \(/`)
- Text assertions (`'Total NFTs'` → `'Total Objects'`)

**Step 3: Update modified test files**

For each non-renamed test file listed above, apply the same updates: mock names, API function names, URL patterns, UI text assertions.

Key patterns to find-and-replace in tests:

- `getSporeNft` → `getSporeObject`
- `getSporeNftDecoded` → `getSporeObjectDecoded`
- `getSporeNftOccupationChart` → `getSporeObjectOccupationChart`
- `getNftCollection` → `getObjectCollection`
- `getNftCollectionItems` → `getObjectCollectionItems`
- `getNftCollectionHolders` → `getObjectCollectionHolders`
- `getNftCollectionActivities` → `getObjectCollectionActivities`
- `getNftCollectionOccupationChart` → `getObjectCollectionOccupationChart`
- `/nfts/` in URLs → `/objects/` or `/identities/`
- `type=nft` in query params → `type=object` (except backward compat tests)
- `'NFTs'` in UI assertions → `'Objects'`
- `nftId` in route params → `objectId` or `identityId`
- `'components/nft/'` in tooling-config paths → `'components/object/'` and `'components/identity/'`
- `'app/nfts/'` in tooling-config paths → `'app/objects/'` and `'app/identities/'`

**Step 4: Verify**

Run: `cd frontend && npx vitest run`
Expected: all tests pass

**Step 5: Commit**

```
test(frontend): update all tests for NFT → Object/Identity rename
```

---

## Task 16: Frontend — Type check and lint

**Step 1: Type check**

Run: `cd frontend && pnpm type-check`
Expected: clean

**Step 2: Lint**

Run: `cd frontend && pnpm lint`
Expected: clean (may need to fix import order)

**Step 3: Format**

Run: `pnpm format`

**Step 4: Fix any issues and commit**

```
chore(frontend): fix type-check and lint issues from rename
```

---

## Task 17: Full verification

**Step 1: Backend tests**

Run: `cargo test`
Expected: all pass

**Step 2: Frontend tests**

Run: `cd frontend && npx vitest run`
Expected: all pass

**Step 3: Grep for stray NFT references**

Run in frontend (excluding M-NFT/mNFT and backward compat):

```bash
grep -rn 'NFT' frontend/lib/ frontend/app/ frontend/components/ frontend/src/ \
  --include='*.ts' --include='*.tsx' \
  | grep -v 'M-NFT\|mNFT\|MNFT\|m-nft\|mnft\|m_nft\|Mnft' \
  | grep -v '__tests__' \
  | grep -v 'node_modules'
```

Run in backend:

```bash
grep -rn '/nfts' crates/api/src/routes/ | grep -v '// '
```

Expected: no remaining `/nfts` paths or generic "NFT" text (only M-NFT/mNFT protocol names).

**Step 4: Commit cleanup if needed**

**Step 5: Final commit**

```
feat: complete NFT → Object/Identity rename across backend and frontend
```
