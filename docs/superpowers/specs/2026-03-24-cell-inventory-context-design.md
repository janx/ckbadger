# Cell Inventory Context Card

Show an inline summary of the inventory item a cell represents, directly on the cell detail page.

## Problem

The cell detail page decodes cell data (Spore, m-NFT, UDT, .bit, cluster) but never links to or summarizes the corresponding inventory item. Users see raw decoded segments but must manually navigate to the item's detail page to understand what the cell *is*.

## Design

### Approach: Frontend Joins

The cell detail page already detects item type via `dataAnalysis.deterministic.kind` and exposes the type script args. A new hook fetches the matching item detail from existing API endpoints. No backend changes required.

**Why not backend enrichment?** The `get_cell()` handler is already ~180 lines. Embedding inventory lookups would mix domain concerns. Frontend joins also let TanStack Query cache item data independently, so navigating to the full item page afterward hits warm cache.

### Placement

Between the type script section and the cell data section. The logical flow becomes: scripts (what the cell uses) -> inventory context (what the cell *is*) -> raw data (the bytes).

### Detection & ID Extraction

The `useInventoryContext(cell)` hook inspects the cell and returns `{ itemType, itemId }` or `null`.

**Primary path: match `deterministic.kind`** (values from `crates/api/src/routes/cells.rs`):

| `deterministic.kind` | Item type | ID source | API endpoint |
|---|---|---|---|
| `"spore_cell"` | spore | `cell.type.args` | `api.getSporeObject(id)` |
| `"spore_cluster_cell"` | cluster | `cell.type.args` | `api.getSporeCluster(id)` |
| `"mnft_token_cell"` | mnft_token | `cell.type.args` | `api.getMnftItemDetail(id)` |
| `"mnft_class_cell"` | mnft_class | `cell.type.args` | `api.getObjectCollection(id)` |
| `"mnft_issuer_cell"` | mnft_issuer | `cell.type.args` | `api.getObjectCollection(id)` |
| `"udt_amount"` | udt | `cell.typeScriptHash` | `api.getToken(typeHash)` |
| `"dotbit_account"` | dotbit | `cell.type.args` | `api.getDotbitItemDetail(nftId)` (returns `CollectionItem`) |

**Fallback path: type script code_hash match.** DID CKB cells have no deterministic decode in the analysis pipeline. If `deterministic` is absent, the hook checks `cell.type?.codeHash` (from the nested `ScriptResponse` object) against the DID CKB script code_hash (`0x079bb8c1dfb249f60d932f4b1a60fa5cb2a36af3653ac09464f262e2f3f682a9`, hash_type = type). If matched:

| Match | Item type | ID source | API endpoint |
|---|---|---|---|
| DID CKB code_hash | did_ckb | `cell.type.args` | `api.getDidCkbItemDetail(nftId)` (returns `CollectionItem`) |

If neither path matches, no card is shown. DAO cells (`dao_deposit_cell`, `dao_withdraw_request_cell`) are excluded — they already have `daoInfo`.

**Note on `CollectionItem` shape:** Both `.bit` and `did:ckb` endpoints return `CollectionItem { nftId, name, standard, ownerLockHash, isLive, createdAtBlock, expiredAt, txHash, outputIndex }`. The card fields map: `name` -> account/identity name, `ownerLockHash` -> owner, `expiredAt` -> expiry (`.bit` only; `did:ckb` may be null).

### Component Architecture

One new file: `frontend/components/cell/inventory-context.tsx`.

Contains:
- `useInventoryContext(cell)` hook — type detection and ID extraction
- `InventoryContextSection` — entry point, renders nothing if no item detected, otherwise fetches and renders the matching card
- Per-type card components: `SporeItemCard`, `ClusterItemCard`, `MnftTokenItemCard`, `MnftClassItemCard`, `MnftIssuerItemCard`, `UdtItemCard`, `DotbitItemCard`, `DidCkbItemCard`

Each card is small (20-40 lines). All live in the same file since they share the same pattern: field display inside a `TerminalPanel` with a "View details" link.

### Card Content Per Type

| Type | Fields |
|---|---|
| Spore/DOB | Content preview (image/text), cluster link (via `clusterId`; display name requires secondary `getSporeCluster` fetch or show truncated ID), owner address, content type, composition tier (from `mediaProfile.tier`) |
| Cluster | Name, description, item count, holders count |
| m-NFT Token | Class name (linked), issuer name, characteristics, owner |
| m-NFT Class | Name, issuer name + link (from `issuerDetail.name` and `classDetail.issuerId` on the same `ObjectCollection` response), total/issued count, description |
| m-NFT Issuer | Name, class count, set count. No "View details" link (no issuer page exists) |
| UDT | Icon, name, symbol, formatted amount (using cell's `udtAmount` + token's `decimals`), total supply, holders |
| .bit | Account name, owner, expiry |
| did:ckb | Identity name, owner |

### Link Targets

| Item Type | Route |
|---|---|
| Spore/DOB | `/objects/{sporeId}` |
| Cluster | `/clusters/{clusterId}` |
| m-NFT Token | `/objects/mnft/{nftId}` |
| m-NFT Class | `/classes/{classId}` |
| m-NFT Issuer | (none) |
| UDT | `/tokens/{typeScriptHash}` |
| .bit | `/identities/dotbit/{accountId}` |
| did:ckb | `/identities/did/{identityId}` |

### Data Flow & Caching

```
cell loads (useQuery ['cell', txHash, outputIndex])
  -> cell available
    -> useInventoryContext(cell) derives { itemType, itemId }
      -> useQuery(['inventory', itemType, itemId], fetchFn, { enabled: !!itemId })
        -> render card
```

- Dependent query: only fires after cell data is available
- Cache key `['inventory', itemType, itemId]` is independent of the cell query
- On fetch failure, the section hides silently — cell page core data is unaffected
- Loading state: subtle skeleton inside the TerminalPanel

### UDT Amount Formatting

The cell provides `udtAmount` (raw u128 string). The token response provides `decimals`. The card formats: `udtAmount / 10^decimals` with the token symbol, using existing formatting utilities.

## Files Changed

| File | Change |
|---|---|
| `frontend/components/cell/inventory-context.tsx` | New: hook + section + 8 card components |
| `frontend/app/cell/[outpoint]/client-page.tsx` | Import and render `InventoryContextSection` between scripts and data |
| `frontend/__tests__/components/cell/inventory-context.test.tsx` | New: 10 test cases |
| `frontend/__tests__/msw/handlers.ts` | Add mock handlers for inventory API endpoints |

## Testing

| Test | Verifies |
|---|---|
| No type script -> no render | Graceful no-op |
| Unrecognized deterministic kind -> no render | Unknown types handled |
| Spore card with content preview and cluster link | Spore detection + display |
| UDT card with formatted amount | Amount formatting with decimals |
| m-NFT token card with class/issuer info | Nested data display |
| Cluster card with item count | Cluster detection |
| .bit card with account name | Identity detection |
| Loading skeleton while fetch pending | Loading state |
| Silent hide on fetch error | Error resilience |
| View details links to correct route per type | Navigation correctness |

## Out of Scope

- Backend changes to `CellDetailResponse`
- New API endpoints
- DOB trait rendering (complex; the Spore detail page handles this)
- m-NFT issuer detail page
- Reverse link from item pages back to cell page
