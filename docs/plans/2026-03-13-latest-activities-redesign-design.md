# Latest Activities Redesign — Mixed Event Stream

## Goal

Replace the current grouped-by-tx activity display on the homepage with a mixed event stream where each activity type gets a distinct visual treatment. The stream should communicate "what's happening on CKB right now" as a real-time pulse.

## Principle Alignment

- **CKB Native**: Different visual treatments for DAO, tokens, objects, identities, and script calls make CKB-specific concepts tangible
- **Local First**: No backend changes; classification logic is frontend-only
- **Agent Friendly**: Pure classification function, no complex grouping logic

## Design Decisions

### Stream Item Granularity

Each stream item = one `OwnerActivityDelta` (one owner's position change in one transaction). No grouping by tx_hash. Cellbase excluded. Two participants in the same tx appear as two separate stream entries.

### Type Classification (Priority Order)

| Priority | Type                  | Condition                         | Color  |
| -------- | --------------------- | --------------------------------- | ------ |
| 1        | DAO Deposit           | Has `daoDeposit` asset change     | Gold   |
| 2        | DAO Withdraw Request  | Has `daoWithdrawRequest`          | Gold   |
| 3        | DAO Withdraw Complete | Has `daoWithdrawComplete`         | Green  |
| 4        | Token Transfer        | Has `token` asset change          | Pink   |
| 5        | Object Action         | Has `object` asset change         | Purple |
| 6        | Identity Action       | Has `identity` asset change       | Blue   |
| 7        | Script Call           | Has `scriptCalls` entries         | Orange |
| 8        | CKB Transfer          | No asset changes, no script calls | Jade   |

First match wins. Classification is a pure function in frontend.

### Type-Specific Layouts

Each type gets a tailored 2-3 line layout with consistent structure:

- Line 1: type badge + action label (left), time ago (right)
- Line 2: address (left), primary value/amount (right)
- Line 3 (optional): secondary info (compensation, CKB delta for tokens, script args)

**CKB Transfer:**

```
arrow CKB Transfer                               3s ago
  ckb1q...3f2e                            -500.00 CKB
```

**DAO Deposit:**

```
diamond DAO Deposit                               5s ago
  ckb1q...a1b2                         +10,000.00 CKB locked
```

**DAO Withdraw Complete:**

```
diamond DAO Withdraw Complete                     8s ago
  ckb1q...c3d4           +10,142.00 CKB  (+142 compensation)
```

**Token Transfer:**

```
circle SEAL Transfer                             12s ago
  ckb1q...e5f6                         +1,200 SEAL
                                          -102.00 CKB
```

**Object (Spore/mNFT):**

```
hexagon Spore Mint                               15s ago
  ckb1q...7g8h                      0xabc12345...
```

**Identity (.bit / did:ckb):**

```
star .bit Renew                                  20s ago
  ckb1q...9i0j                      0xdef67890...
```

**Script Call:**

```
gear Script: Omnilock                            25s ago
  ckb1q...k1l2        type1 . args 0x3a...
                                          +300.00 CKB
```

### Stream Behavior

- **Data source**: `GET /activities/latest?limit=32`, same `GlobalActivity[]` type
- **Poll**: Every 10s (or WebSocket push)
- **New item detection**: Diff by composite key `txHash + address`
- **Animation**: New items slide down from top (~300ms CSS transition), subtle jade glow fades over ~2s
- **No exit animation**: Items that fall off the bottom simply disappear
- **Display**: ~20 visible items, internal scroll if needed

### Interaction

- Click action label / tx area -> `/tx/{txHash}`
- Click address -> `/address/{address}`
- Click script name -> script detail page
- Click token symbol -> `/tokens/{typeScriptHash}`
- Click object ID -> `/objects/{objectId}` or `/objects/mnft/{objectId}`
- Hover address -> full address tooltip
- Hover token delta -> full decimal amount tooltip
- Hover script call -> full code_hash + args tooltip
- No expand/collapse — each item is self-contained

## Scope

### Files Changed

| File                                        | Change                                  |
| ------------------------------------------- | --------------------------------------- |
| `frontend/components/latest-activities.tsx` | Rewrite with type-specific renderers    |
| `frontend/lib/latest-activity-groups.ts`    | Delete (no longer grouping by tx)       |
| `frontend/lib/activity-classify.ts`         | New: `classifyActivity()` pure function |
| `frontend/components/home-content.tsx`      | Minor grid/height adjustment if needed  |

### Files NOT Changed

- Backend (API, store, indexer) — no changes
- `frontend/lib/api.ts` — same types, same endpoint
- `frontend/components/activity-card.tsx` — 24h stats panel unchanged

### Tests

- Unit tests for `classifyActivity()` covering all 8 types + priority ordering
- Update `latest-activities.test.tsx` for new rendering
- Delete tests for `latest-activity-groups.ts`

## Validation

- No storage/schema impact
- No re-sync required
- Frontend only: `pnpm type-check && pnpm lint && pnpm test`
