# Fiber Channel Activities Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Detect Fiber payment channel lifecycle events (open, cooperative close, force close, settlement) from on-chain data and display them via the protocol action framework with dedicated channel pages.

**Architecture:** Fiber is a Layer 3 ProtocolDetector (same pattern as RgbppDetector). `FiberDetector` detects channel events from lock script patterns. A separate channel writer processes detected events to maintain `CF_FIBER_CHANNELS` state for dedicated pages. The existing protocol action framework handles activity storage, API serialization, frontend display, and filtering (`protocol:fiber`).

**Tech Stack:** Rust (indexer/store/api), RocksDB column families, Axum routes, React/TypeScript frontend with TanStack Query.

**Design doc:** `docs/plans/2026-03-14-fiber-channel-activities-design.md`

---

### Task 1: Fiber Parser — Code Hash Constants

**Files:**

- Create: `crates/indexer/src/parser/fiber.rs`
- Modify: `crates/indexer/src/parser/mod.rs`

**Step 1: Create fiber parser with code hash constants and args parsing**

Create `crates/indexer/src/parser/fiber.rs`. Follow the pattern from `crates/indexer/src/parser/dao.rs` (code hash constants + LazyLock bytes + detection functions).

Contents:

- Mainnet + testnet code hashes for funding-lock and commitment-lock
- `is_funding_lock(code_hash: &[u8]) -> bool`
- `is_commitment_lock(code_hash: &[u8]) -> bool`
- `all_fiber_lock_code_hashes() -> Vec<Vec<u8>>` (for PROTOCOL_ACTION_LOCKS)
- `parse_funding_lock_args(args: &[u8]) -> Option<FundingLockArgs>` — extracts pubkey_hash (20B)
- `parse_commitment_lock_args(args: &[u8]) -> Option<CommitmentLockArgs>` — extracts pubkey_hash(20B) + delay_epoch(8B LE) + version(8B BE) + settlement_hash(20B) + settlement_flag(1B)
- `FundingLockArgs` and `CommitmentLockArgs` structs
- Unit tests for all detection and parsing functions

Code hash values:

- Funding lock mainnet: `0xe45b1f8f21bff23137035a3ab751d75b36a981deec3e7820194b9c042967f4f1`
- Funding lock testnet: `0x6c67887fe201ee0c7853f1682c0b77c0e6214044c156c7558269390a8afa6d7c`
- Commitment lock mainnet: `0x2d45c4d3ed3e942f1945386ee82a5d1b7e4bb16d7fe1ab015421174ab747406c`
- Commitment lock testnet: `0x740dee83f87c6f309824d8fd3fbdd3c8380ee6fc9acc90b1a748438afcdf81d8`

**Step 2: Register module in parser/mod.rs**

Add `pub mod fiber;` to `crates/indexer/src/parser/mod.rs` (line ~4, after `pub mod dotbit;`).

**Step 3: Run tests**

Run: `cargo test -p ckbadger-indexer parser::fiber`
Expected: all tests pass

**Step 4: Commit**

```
feat(parser): add Fiber Network script identification and args parsing
```

---

### Task 2: FiberDetector — ProtocolDetector Implementation

**Files:**

- Create: `crates/indexer/src/db/writer/fiber_detector.rs`
- Modify: `crates/indexer/src/db/writer.rs` (module registration)

Follow the pattern from `crates/indexer/src/db/writer/rgbpp_detector.rs`.

**Step 1: Implement FiberDetector**

Create `crates/indexer/src/db/writer/fiber_detector.rs`:

```rust
use super::activities::{OwnerAccum, ProtocolDetector, TxView};
use crate::parser::fiber::{is_funding_lock, is_commitment_lock, parse_commitment_lock_args};
use ckbadger_store::types::ProtocolAction;
```

The detector:

1. Scans all input/output cells for funding-lock and commitment-lock code_hashes
2. Pattern-matches to determine event type:
   - Funding-lock output, no funding-lock input → `channel_open`
   - Funding-lock input, no commitment-lock output → `channel_close`
   - Funding-lock input + commitment-lock output → `force_close`
   - Commitment-lock input → `settlement`
3. Emits `ProtocolAction { protocol: "fiber", action: "...", metadata: {...} }` only for owners who are NOT the funding/commitment lock owner (i.e., the actual channel participants)
4. Metadata includes: capacity, fundingLockArgs or commitmentLockArgs, channelOutpoint/commitmentOutpoint where applicable, UDT info if present

For channel_open, use `ckbadger_store::keys::encode_outpoint(tx_hash, output_index)` to create the channelOutpoint in metadata.

For channel_close / force_close, the consumed funding cell's lock_args identifies the channel (since InputCellView doesn't carry its outpoint).

**Step 2: Register module**

In `crates/indexer/src/db/writer.rs`, add:

```rust
pub(crate) mod fiber_detector;
```

**Step 3: Register detector in sync pipeline**

In `crates/indexer/src/sync/batch.rs`, find where `protocol_detectors` is constructed (two locations: ~line 1676 for bulk sync, ~line 4097 for live sync). Add `FiberDetector` alongside `RgbppDetector`:

```rust
let protocol_detectors: Vec<Box<dyn ProtocolDetector>> = vec![
    Box::new(RgbppDetector::new(self.config.is_mainnet())),
    Box::new(FiberDetector::new(self.config.is_mainnet())),
];
```

**Step 4: Write tests**

Add tests in `fiber_detector.rs` `#[cfg(test)]` module. Test channel_open, channel_close, force_close, settlement patterns. Follow the test patterns from rgbpp_detector.rs.

**Step 5: Run tests**

Run: `cargo test -p ckbadger-indexer fiber_detector`
Expected: pass

**Step 6: Commit**

```
feat(indexer): implement FiberDetector for protocol action framework
```

---

### Task 3: API — Lock Call Enrichment

**Files:**

- Modify: `crates/api/src/routes/activities.rs`

**Step 1: Add Fiber code hashes to PROTOCOL_ACTION_LOCKS**

Replace the empty `PROTOCOL_ACTION_LOCKS` (line ~296-299) with Fiber code hashes:

```rust
static PROTOCOL_ACTION_LOCKS: LazyLock<HashSet<Vec<u8>>> = LazyLock::new(|| {
    let hashes: &[&str] = &[
        // Fiber funding lock (mainnet, testnet)
        "0xe45b1f8f21bff23137035a3ab751d75b36a981deec3e7820194b9c042967f4f1",
        "0x6c67887fe201ee0c7853f1682c0b77c0e6214044c156c7558269390a8afa6d7c",
        // Fiber commitment lock (mainnet, testnet)
        "0x2d45c4d3ed3e942f1945386ee82a5d1b7e4bb16d7fe1ab015421174ab747406c",
        "0x740dee83f87c6f309824d8fd3fbdd3c8380ee6fc9acc90b1a748438afcdf81d8",
    ];
    hashes.iter().map(|h| parse_hex_code_hash(h)).collect()
});
```

**Step 2: Add Fiber lock args decoders**

Add `decode_fiber_funding_lock_args` and `decode_fiber_commitment_lock_args` functions, and register them in `LOCK_ARGS_DECODERS` (after BTC time lock entries, ~line 332). Follow the pattern from `decode_rgbpp_lock_args`.

Funding decoder returns: `{ "protocol": "fiber", "action": "funding", "pubkeyHash": "0x..." }`
Commitment decoder returns: `{ "protocol": "fiber", "action": "commitment", "pubkeyHash": "0x...", "delayEpoch": N, "version": N, "settlementHash": "0x...", "settlementFlag": N }`

**Step 3: Add tests**

Add tests for the decoders in the existing `#[cfg(test)]` module.

**Step 4: Run tests**

Run: `cargo test -p ckbadger-api`
Expected: pass

**Step 5: Commit**

```
feat(api): add Fiber lock args decoders and protocol_action classification
```

---

### Task 4: Store Infrastructure — FiberChannel Types, CFs, Keys, Ops, Batch

**Files:**

- Modify: `crates/ckbadger-store/src/types.rs`
- Modify: `crates/ckbadger-store/src/store.rs`
- Modify: `crates/ckbadger-store/src/keys.rs`
- Modify: `crates/ckbadger-store/src/batch.rs`
- Create: `crates/ckbadger-store/src/fiber_ops.rs`
- Modify: `crates/ckbadger-store/src/lib.rs` (register fiber_ops module)

**Step 1: Add FiberChannel types**

In `crates/ckbadger-store/src/types.rs`, add after the `AssetAction` enum:

```rust
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum FiberChannelState { Open, CooperativelyClosed, ForceClosed, Settled }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FiberChannel { /* fields from design doc */ }
```

**Step 2: Add 3 CF constants + arrays + getters**

In `crates/ckbadger-store/src/store.rs`:

- Add `CF_FIBER_CHANNELS`, `CF_FIBER_CHANNEL_BY_COMMITMENT`, `CF_ADDR_FIBER_CHANNELS` constants (after line ~315)
- Add to `ALL_CFS` and `DOMAIN_CFS` arrays
- Add `cf_fiber_channels()`, `cf_fiber_channel_by_commitment()`, `cf_addr_fiber_channels()` getter methods

**Step 3: Add key encoding**

In `crates/ckbadger-store/src/keys.rs`, add:

- `encode_fiber_channel_id(funding_tx_hash, output_index) -> Vec<u8>` (blake2b hash)
- `encode_outpoint(tx_hash, output_index) -> Vec<u8>` (raw 36B)
- `decode_outpoint(data) -> (Vec<u8>, u32)`
- `encode_addr_fiber_channel_key(lock_hash, channel_id) -> Vec<u8>` (64B)
- `decode_addr_fiber_channel_key(key) -> (&[u8], &[u8])`

**Step 4: Add batch write/delete methods**

In `crates/ckbadger-store/src/batch.rs`, add:

- `put_fiber_channel(channel_id, channel)`
- `put_fiber_channel_by_commitment(hash, channel_id)`
- `put_addr_fiber_channel(lock_hash, channel_id)`
- `delete_fiber_channel(channel_id)`
- `delete_fiber_channel_by_commitment(hash)`
- `delete_addr_fiber_channel(lock_hash, channel_id)`

**Step 5: Create fiber_ops.rs**

Create `crates/ckbadger-store/src/fiber_ops.rs` with read methods:

- `get_fiber_channel(channel_id) -> Option<FiberChannel>`
- `get_fiber_channel_id_by_commitment(hash) -> Option<Vec<u8>>`
- `list_fiber_channels(limit, cursor, state_filter) -> Vec<(Vec<u8>, FiberChannel)>`
- `list_addr_fiber_channels(lock_hash, limit) -> Vec<(Vec<u8>, FiberChannel)>`

Register module in `lib.rs`.

**Step 6: Run check**

Run: `cargo check -p ckbadger-store`
Expected: pass

**Step 7: Commit**

```
feat(store): add Fiber channel CFs, types, key encoding, batch writes, and read ops
```

---

### Task 5: Channel Writer — Lifecycle State Updates

**Files:**

- Create: `crates/indexer/src/db/writer/fiber.rs`
- Modify: `crates/indexer/src/db/writer.rs` (module registration)
- Modify: `crates/indexer/src/sync/batch.rs` (pipeline integration)

**Step 1: Create fiber channel writer**

Create `crates/indexer/src/db/writer/fiber.rs`. The writer processes `TxActivityBundle`s and updates channel state based on protocol_actions with `protocol == "fiber"`:

- `channel_open` → read metadata (channelOutpoint, capacity, fundingLockArgs), compute channel_id via `keys::encode_fiber_channel_id`, insert FiberChannel with state Open, insert addr_fiber_channel for each participant
- `channel_close` → look up channel by fundingLockArgs, update to CooperativelyClosed
- `force_close` → look up channel by fundingLockArgs, update to ForceClosed, insert commitment index
- `settlement` → look up channel via CF_FIBER_CHANNEL_BY_COMMITMENT, update to Settled

Extract participant lock_hashes from the bundle's owners list (all owners except the funding/commitment lock owner).

**Step 2: Register module + integrate into pipeline**

In `crates/indexer/src/db/writer.rs`, add `pub(crate) mod fiber;`.

In `crates/indexer/src/sync/batch.rs`, after activity bundles are committed, call the fiber channel writer to process each bundle.

**Step 3: Run check**

Run: `cargo check -p ckbadger-indexer`
Expected: pass

**Step 4: Commit**

```
feat(indexer): add Fiber channel writer — lifecycle state tracking in CF_FIBER_CHANNELS
```

---

### Task 6: Reorg Handling — Fiber Channel Rollback

**Files:**

- Modify: `crates/ckbadger-store/src/reorg_ops.rs`

**Step 1: Add Fiber channel cleanup to rollback**

Follow the pattern from activity rollback (line ~1237 in reorg_ops.rs). Add sections to clean up all three Fiber CFs for blocks > rollback_to.

Simplest approach: iterate CF_FIBER_CHANNELS, delete channels with `open_block > rollback_to`. For channels modified (closed/settled) during rolled-back blocks but opened earlier, revert their state or rely on reindex.

**Step 2: Run check**

Run: `cargo check -p ckbadger-store`
Expected: pass

**Step 3: Commit**

```
feat(store): add Fiber channel cleanup to reorg rollback
```

---

### Task 7: API — Fiber Channel Endpoints

**Files:**

- Create: `crates/api/src/routes/fiber.rs`
- Modify: `crates/api/src/routes/mod.rs`

**Step 1: Create fiber routes module**

Create `crates/api/src/routes/fiber.rs` with four endpoints:

- `GET /fiber/channels` — paginated list with optional `?state=` filter
- `GET /fiber/channels/{channel_id}` — single channel with timeline
- `GET /addresses/{addr}/fiber/channels` — channels for address
- `GET /fiber/stats` — aggregate stats

Follow the pattern from `crates/api/src/routes/dao.rs`. Use `FiberChannelResponse` and `FiberChannelDetailResponse` (with timeline). Convert participant lock_hashes to CKB addresses using `script_to_address`.

**Step 2: Register routes**

In `crates/api/src/routes/mod.rs`, add `mod fiber;` and `.merge(fiber::routes())`.

**Step 3: Run check + tests**

Run: `cargo check -p ckbadger-api`
Expected: pass

**Step 4: Commit**

```
feat(api): add Fiber channel endpoints — list, detail, by-address, stats
```

---

### Task 8: Frontend — Protocol Action Rendering for Fiber

**Files:**

- Modify: `frontend/lib/api.ts` (add channel API types + methods)
- Modify: `frontend/components/latest-activities.tsx` (Fiber-specific label/metadata formatting)
- Modify: `frontend/components/activity-event-row.tsx` (if separate from latest-activities)

**Step 1: Add Fiber channel API types and methods**

In `frontend/lib/api.ts`, add:

- `FiberChannel` interface (matching API response)
- `FiberChannelDetail` interface (with timeline)
- `getFiberChannels(params)`, `getFiberChannel(id)`, `getAddressFiberChannels(addr)`, `getFiberStats()` functions

**Step 2: Add Fiber protocol action rendering**

In the protocol action row renderer (latest-activities.tsx), add Fiber-specific formatting:

- `fiber:channel_open` → "Fiber Channel Open" + capacity from metadata
- `fiber:channel_close` → "Fiber Channel Close"
- `fiber:force_close` → "Fiber Force Close" + delay epoch
- `fiber:settlement` → "Fiber Settlement"

Follow the existing pattern for RGB++ action label mapping.

**Step 3: Add tests**

Add tests for Fiber protocol action rendering.

**Step 4: Run tests**

Run: `cd frontend && npx vitest run`
Expected: pass

**Step 5: Commit**

```
feat(frontend): add Fiber protocol action rendering and channel API types
```

---

### Task 9: Frontend — Fiber Channel Pages

**Files:**

- Create: `frontend/app/fiber/channels/page.tsx`
- Create: `frontend/app/fiber/channels/client-page.tsx`
- Create: `frontend/app/fiber/channels/[id]/page.tsx`
- Create: `frontend/app/fiber/channels/[id]/client-page.tsx`

**Step 1: Channel list page**

Stats bar + filter tabs + paginated table. Follow the page.tsx + client-page.tsx pattern from `frontend/app/address/[addr]/`.

**Step 2: Channel detail page**

Channel header + participants + lifecycle timeline. Show events with tx links and timestamps.

**Step 3: Add to navigation**

Add Fiber Channels link to site navigation.

**Step 4: Run type-check + lint**

Run: `cd frontend && pnpm type-check && pnpm lint`
Expected: pass

**Step 5: Commit**

```
feat(frontend): add Fiber channel list and detail pages
```

---

### Task 10: Frontend — Address Page Fiber Tab

**Files:**

- Modify: `frontend/app/address/[addr]/client-page.tsx`

**Step 1: Add Fiber Channels tab**

New tab using `/addresses/{addr}/fiber/channels` API.

**Step 2: Run type-check**

Run: `cd frontend && pnpm type-check`
Expected: pass

**Step 3: Commit**

```
feat(frontend): add Fiber Channels tab to address page
```

---

### Task 11: Pre-commit Checks + Final Verification

**Step 1: Run full pre-commit**

```bash
cargo check && cargo clippy && cd frontend && pnpm type-check && pnpm lint
```

**Step 2: Run all tests**

```bash
cargo test && cd frontend && npx vitest run
```

**Step 3: Verify no regressions**

Ensure existing activity and protocol action tests still pass.

**Step 4: Final commit (if any fixups needed)**

```
chore: fix lint and test issues from Fiber channel implementation
```
