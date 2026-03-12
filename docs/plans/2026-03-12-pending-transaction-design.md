# Pending Transaction Search And Detail Design

## Goal

- Support searching mempool transactions that are not yet committed on chain.
- Let `/tx/{hash}` render a pending/proposed transaction detail page with all known data and explicit `pending...` placeholders for unknown chain-derived fields.
- Auto-refresh a pending transaction detail page so it transitions to the committed view once the transaction lands on chain.

## Principle Alignment

- CKB Native: treat mempool state as a real part of the transaction lifecycle instead of hiding it behind a chain-only explorer model.
- Local First: reuse direct CKB RPC and existing API read paths; no new persistent DB writes or backfill flows.
- Agent Friendly: keep a single transaction detail route and a single calculation path for committed vs pending reads.

## Context

- Search already supports chain-backed transaction hashes through `crates/api/src/routes/search.rs` and `frontend/components/search-bar.tsx`.
- Transaction detail currently assumes a committed transaction and returns `404` when the hash is not yet indexed in RocksDB.
- The API already exposes mempool aggregate data via `crates/api/src/routes/mempool.rs`.
- CKB `get_transaction` can return the full transaction body plus `tx_status` for `pending` and `proposed` transactions, including `time_added_to_pool`, `fee`, and `cycles`.

## Requirements

- Search by pending/proposed transaction hash from the global search bar.
- Route pending/proposed matches to the existing `/tx/{hash}` page.
- Keep the page structure for pending transactions:
  - `Overview`
  - `Inputs/Outputs`
  - `Scripts`
  - `Cell Deps`
  - `Graph`
  - `Witness`
- Render unknown chain-derived values as literal `pending...` with a marquee-style text effect.
- Refresh pending detail data automatically until the transaction becomes committed.
- Do not add any RocksDB write path for this feature.

## API Approach

- Keep `/api/v1/transactions/{hash}/detail` as the single transaction detail entry point.
- Extend the response so it can represent both committed and mempool transactions.
- Lookup order:
  - committed/indexed transaction in RocksDB first
  - CKB RPC `get_transaction` second
  - `404` only if neither source knows the hash
- Add transaction status fields:
  - `status: 'committed' | 'pending' | 'proposed'`
  - `pendingSince: string | null`
- For pending/proposed responses:
  - return known transaction body data from RPC
  - leave committed-only fields nullable instead of fabricating values

## Data Model Rules

- Known for pending/proposed:
  - `hash`
  - `status`
  - `pendingSince`
  - `fee`
  - `txSize`
  - `cycles`
  - `isCellbase`
  - `inputsCount`
  - `outputsCount`
  - `inputs`
  - `outputs`
  - `witnesses`
  - `witnessesAvailable`
- Nullable for pending/proposed because chain placement is unknown:
  - `blockNumber`
  - `blockHash`
  - `index`
  - `confirmations`
  - `timestamp`
  - `feeRate` when exact UI semantics depend on committed response shape and cannot be guaranteed identically
  - `inputsCapacity`
  - `outputsCapacity`
  - `inputsUsedCapacity`
  - `outputsUsedCapacity`
- Committed transactions keep existing semantics.
- The detail endpoint remains the single owner of transaction detail shaping; frontend does not reconstruct pending detail from multiple APIs.

## Search Behavior

- Keep `/api/v1/search` as the single search endpoint.
- For exact 32-byte hash queries:
  - return committed transaction result if indexed
  - if not indexed, query CKB RPC for mempool status
  - if `pending` or `proposed`, return a transaction result pointing to `/tx/{hash}`
- Pending labels must explicitly communicate status:
  - `Pending Transaction`
  - `Proposed Transaction`
- Committed search results still win when a transaction exists in both committed storage and mempool-related reads.

## Frontend Behavior

- Keep the existing `/tx/[hash]` page and `api.getTransactionDetail(hash)` call.
- Make the transaction detail types pending-aware by allowing committed-only fields to be nullable.
- Pending/proposed page behavior:
  - show a `Pending` or `Proposed` badge instead of confirmations
  - render `pending...` placeholders for unknown fields in `Overview`
  - keep `Inputs/Outputs`, `Scripts`, and `Witness` functional with known transaction data
  - keep `Cell Deps`, `Graph`, and lifecycle-related UI sections mounted, but render `pending...` placeholders inside them
  - do not synthesize graph or lifecycle data client-side
- Introduce a small reusable pending placeholder component so the marquee treatment stays consistent.

## Auto Refresh

- While `status` is `pending` or `proposed`, the detail query polls every 3 seconds.
- Once the response becomes `committed`:
  - stop polling
  - allow committed-only dependent queries (`graph`, `cell-deps`, `lifecycle`) to run normally
  - replace pending placeholders in place without route changes
- If a previously pending transaction disappears from RPC and is still not indexed, show a direct error state instead of pretending it is still pending.

## Error Handling

- `/transactions/{hash}/detail`
  - committed: normal response
  - pending/proposed: normal response with mempool status
  - unknown everywhere: `404`
- `/transactions/{hash}/cell-deps`, `/transactions/{hash}/lifecycle`, and graph transaction reads
  - committed: current behavior
  - pending/proposed: explicit non-success response that the frontend maps to `pending...`
- Avoid fallback calculation chains. If committed-only data is unknown, surface that state directly.

## Testing

- Rust API tests:
  - search returns pending/proposed transaction results
  - detail returns mempool-backed response for pending/proposed transactions
  - committed lookup has priority over mempool fallback
  - pending graph/lifecycle/cell-deps requests return the expected explicit error
- Frontend tests:
  - search bar shows and navigates to pending transaction results
  - transaction detail renders pending badge and marquee placeholder text
  - pending-only sections preserve structure with placeholders
  - detail page transitions from pending to committed after a refetch

## Risks

- The existing detail page assumes several numeric fields are always present; broadening the type must be done carefully to avoid accidental `null` arithmetic in committed views.
- Polling needs clear stop conditions so a committed page does not keep re-fetching unnecessarily.
- Pending-only error states for graph/lifecycle/cell-deps must be explicit enough that the frontend can distinguish them from real failures.
