# Fiber Channel Activities Design

## Goal

Detect and display all Fiber payment channel lifecycle events (open, cooperative close, force close, settlement) with full channel tracking, dedicated pages, and activity feed integration.

## Principle Alignment

- **CKB Native**: Fiber is the CKB-native payment channel network (like Lightning on Bitcoin). Making its on-chain footprint visible and interpretable is core to CKB exploration.
- **Local First**: All data derived from on-chain cell patterns — no external Fiber node API required.
- **Agent Friendly**: Structured API endpoints with clear channel lifecycle semantics.

## Context

Fiber Network is a Lightning-compatible Layer 2 payment channel network on CKB. It uses two on-chain scripts:

- **funding-lock**: 2-of-2 multisig lock for channel funding cells. Args = blake160 hash of aggregated public key (20 bytes).
- **commitment-lock**: DARIC protocol lock for force-close commitment cells. Args = pubkey_hash(20B) + delay_epoch(8B) + version(8B) + settlement_hash(20B) + settlement_flag(1B) = 57 bytes.

Code hashes:

| Script          | Mainnet                                                              | Testnet                                                              |
| --------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- |
| funding-lock    | `0xe45b1f8f21bff23137035a3ab751d75b36a981deec3e7820194b9c042967f4f1` | `0x6c67887fe201ee0c7853f1682c0b77c0e6214044c156c7558269390a8afa6d7c` |
| commitment-lock | `0x2d45c4d3ed3e942f1945386ee82a5d1b7e4bb16d7fe1ab015421174ab747406c` | `0x740dee83f87c6f309824d8fd3fbdd3c8380ee6fc9acc90b1a748438afcdf81d8` |

## Three-Layer Integration

Fiber fits naturally into the three-layer activity analysis model (`docs/prompts/ACTIVITY_DESIGN.md`):

```
Layer 3: ProtocolAction     "fiber:channel_open" — WHY CKB moved
                             FiberDetector implements ProtocolDetector trait

Layer 2: LockCallEntry       funding-lock/commitment-lock seen on cells
                             Decoded args via LOCK_ARGS_DECODERS (existing framework)

Layer 1: CKB Position        ckb_delta = -145 CKB (capacity locked into channel)
                             Always present, always computed
```

All three layers are simultaneously true:

```
Layer 3:  fiber:channel_open   ← explains WHY this CKB movement happened
Layer 2:  lock_call funding-lock (pubkeyHash: 0x...)  ← explains WHAT script was involved
Layer 1:  -145.00 CKB         ← explains HOW MUCH CKB changed
```

**Key architectural decision:** Fiber is a **Layer 3 ProtocolDetector**, not Layer 2 AssetChange variants. This means:

- No new `AssetChange` enum variants needed
- No custom activity filter needed (`protocol:fiber` already works)
- No custom API response types needed (`ProtocolActionResponse` already works)
- No custom frontend classification needed (`protocolAction` displayType already works)
- Detection via `FiberDetector` following the same pattern as `RgbppDetector`

## Detection Logic

### FiberDetector (Layer 3 — ProtocolDetector)

`FiberDetector` implements the `ProtocolDetector` trait. It scans all input/output cells for funding-lock and commitment-lock code_hashes, then pattern-matches to determine the channel lifecycle event:

| On-chain Pattern                                       | ProtocolAction        |
| ------------------------------------------------------ | --------------------- |
| Funding-lock output created, no funding-lock input     | `fiber:channel_open`  |
| Funding-lock input consumed, no commitment-lock output | `fiber:channel_close` |
| Funding-lock input consumed + commitment-lock output   | `fiber:force_close`   |
| Commitment-lock input consumed                         | `fiber:settlement`    |

### Metadata

Each `ProtocolAction` carries structured metadata:

```json
// channel_open
{ "channelOutpoint": "0x...", "capacity": "14500000000", "fundingLockArgs": "0x..." }

// channel_close
{ "fundingLockArgs": "0x...", "capacity": "14500000000" }

// force_close
{ "fundingLockArgs": "0x...", "capacity": "14500000000",
  "commitmentOutpoint": "0x...", "delayEpoch": 1800 }

// settlement
{ "commitmentLockArgs": "0x...", "capacity": "14500000000" }
```

For UDT channels, metadata also includes `udtTypeHash` and `udtAmount`.

### Lock Call Enrichment (Layer 2)

The existing lock-based analysis framework already captures funding-lock and commitment-lock as `LockCallEntry`. We enrich these:

1. Add Fiber code hashes to `PROTOCOL_ACTION_LOCKS` → role becomes `"protocol_action"`.
2. Add `decode_funding_lock_args` and `decode_commitment_lock_args` to `LOCK_ARGS_DECODERS`.
3. Frontend: when a `protocolAction` with protocol `"fiber"` exists, the corresponding `lockCall` rows are deduplicated (same pattern as RGB++).

### Detection Pipeline

```
1. Process inputs  → OwnerAccum (existing)
2. Process outputs → OwnerAccum (existing)
3. Detect lock calls → LockCallEntry (existing, captures funding/commitment locks)
4. Emit asset changes → AssetChange (existing, captures UDT deltas if present)
5. Emit type/lock calls → TypeCallEntry/LockCallEntry (existing)
6. >>> Run FiberDetector (new) <<<  → ProtocolAction
7. Build OwnerActivityDelta with all fields (existing)
```

Channel ID = `blake2b(funding_outpoint)` — deterministic, unique, linkable across lifecycle.

## Channel State Storage

For the dedicated channel pages, a separate storage layer tracks channel lifecycle state. This is independent of the activity system — activities provide per-transaction interpretations; channel state provides the aggregate view.

### New Column Families (domain store)

**CF_FIBER_CHANNELS** — channel lifecycle state:

```
Key:   channel_id (32 bytes) = blake2b(funding_outpoint)
Value: FiberChannel {
    funding_tx_hash: Vec<u8>,
    funding_output_index: u32,
    state: FiberChannelState,  // Open | CooperativelyClosed | ForceClosed | Settled
    capacity: u64,
    udt_type_hash: Option<Vec<u8>>,
    udt_amount: Option<u128>,
    open_block: i64,
    open_timestamp: i64,
    close_tx_hash: Option<Vec<u8>>,
    close_block: Option<i64>,
    close_timestamp: Option<i64>,
    commitment_tx_hash: Option<Vec<u8>>,
    commitment_output_index: Option<u32>,
    delay_epoch: Option<u64>,
    settlement_tx_hash: Option<Vec<u8>>,
    settlement_block: Option<i64>,
    settlement_timestamp: Option<i64>,
    participants: Vec<Vec<u8>>,
    funding_lock_args: Vec<u8>,
}
```

**CF_FIBER_CHANNEL_BY_COMMITMENT** — commitment outpoint → channel lookup:

```
Key:   blake2b(commitment_outpoint)
Value: channel_id (32 bytes)
```

**CF_ADDR_FIBER_CHANNELS** — per-address channel index:

```
Key:   participant_lock_hash(32) + channel_id(32)
Value: empty
```

### Channel State Writer

The Fiber channel writer processes `TxActivityBundle`s after they're built. It scans each owner's `protocol_actions` for `protocol == "fiber"` and applies state transitions:

- `channel_open` → insert new `FiberChannel` with state `Open`
- `channel_close` → update to `CooperativelyClosed`
- `force_close` → update to `ForceClosed`, insert commitment index
- `settlement` → lookup via commitment index, update to `Settled`

Participant lock_hashes are extracted from the bundle's owners list (excluding the funding/commitment lock owner).

All three CFs are domain store (mutable). Reorg handling: direct delete on rollback.

## API

### Activity Changes (zero new code)

Fiber activities are already handled by the existing protocol action framework:

- Filter: `protocol:fiber` — already supported
- Response: `ProtocolActionResponse { protocol, action, metadata }` — already serialized
- No new `AssetChangeResponse` variants needed

### New Endpoints

```
GET /fiber/channels                    — list all channels, cursor-paginated
GET /fiber/channels/{channel_id}       — single channel with lifecycle timeline
GET /addresses/{addr}/fiber/channels   — channels for an address
GET /fiber/stats                       — aggregate stats
```

### FiberChannelResponse

```json
{
  "channelId": "0x...",
  "state": "open | cooperativelyClosed | forceClosed | settled",
  "capacity": "145000000000",
  "udtTypeHash": null,
  "udtAmount": null,
  "participants": ["ckb1...", "ckb1..."],
  "fundingTxHash": "0x...",
  "fundingOutputIndex": 0,
  "openBlock": 12345678,
  "openTimestamp": "1710000000",
  "closeTxHash": null,
  "closeBlock": null,
  "closeTimestamp": null,
  "commitmentTxHash": null,
  "delayEpoch": null,
  "settlementTxHash": null,
  "settlementBlock": null,
  "settlementTimestamp": null
}
```

Channel detail adds timeline:

```json
{
  "...channelFields",
  "timeline": [
    { "event": "open", "txHash": "0x...", "block": 12345678, "timestamp": "..." },
    { "event": "forceClose", "txHash": "0x...", "block": 12346000, "timestamp": "..." },
    { "event": "settlement", "txHash": "0x...", "block": 12346500, "timestamp": "..." }
  ]
}
```

List pagination: cursor-based using `channel_id`, sorted by `open_block` descending. Optional `?state=open` filter.

## Frontend

### Activity Feed Integration (mostly zero new code)

Fiber activities render as `protocolAction` displayType — the same treatment as RGB++. The existing rendering pipeline shows:

```
⚡ Fiber · channel open              capacity: 145 CKB     ← Layer 3 headline
📦 lock_call funding-lock             pubkeyHash: 0x...      ← Layer 2 (hidden if dedup'd)
💰 CKB                                -145.00 CKB            ← Layer 1 detail
```

The only frontend change needed for activity display: protocol-specific formatting in the protocol action row renderer (action label mapping, metadata formatting) — same pattern as existing RGB++ rendering.

### New Pages

**`/fiber/channels`** — Channel list:

- Table: Channel ID, State (badge), Capacity, Participants (2 addresses), Opened, Closed
- Filter tabs: All | Open | Closed | Force-Closed
- Stats bar: total channels, capacity locked, active count

**`/fiber/channels/{channel_id}`** — Channel detail:

- Header: channel ID, state badge, capacity
- Participants section with address links
- Lifecycle timeline (vertical): Open → Close/ForceClose → Settlement
- Force-close: delay epoch with timelock status (active/expired)

**`/addresses/{addr}`** — Existing address page:

- New "Fiber Channels" tab listing channels where address is a participant

### Force-Close UX

Force-close and settlement appear as two separate linked events. In the activity feed they are separate rows sharing the same channel ID (via metadata). In the channel detail timeline they are connected with a visual line, showing the timelock countdown.

## Scope

### Files Changed

| Area             | Files                                                  | Purpose                                        |
| ---------------- | ------------------------------------------------------ | ---------------------------------------------- |
| Parser           | `crates/indexer/src/parser/fiber.rs` (new)             | Code hash constants, args parsing              |
| Store types      | `crates/ckbadger-store/src/types.rs`                   | FiberChannel struct (no AssetChange changes)   |
| Store CFs        | `crates/ckbadger-store/src/store.rs`                   | 3 new CFs                                      |
| Store ops        | `crates/ckbadger-store/src/fiber_ops.rs` (new)         | Channel CRUD, queries                          |
| Store batch      | `crates/ckbadger-store/src/batch.rs`                   | Channel write methods                          |
| Detector         | `crates/indexer/src/db/writer/fiber_detector.rs` (new) | FiberDetector (ProtocolDetector impl)          |
| Channel writer   | `crates/indexer/src/db/writer/fiber.rs` (new)          | Channel lifecycle state updates                |
| Pipeline         | `crates/indexer/src/sync/batch.rs`                     | Register FiberDetector + channel writer        |
| API activities   | `crates/api/src/routes/activities.rs`                  | Lock args decoders, PROTOCOL_ACTION_LOCKS      |
| API routes       | `crates/api/src/routes/fiber.rs` (new)                 | 4 new endpoints                                |
| Frontend         | `frontend/components/latest-activities.tsx`            | Fiber protocol action label/metadata rendering |
| Frontend pages   | `frontend/app/fiber/` (new)                            | Channel list + detail pages                    |
| Frontend address | `frontend/app/address/[addr]/client-page.tsx`          | Fiber Channels tab                             |

### What's NOT needed (framework provides)

- No new AssetChange variants
- No new AssetChangeResponse variants
- No new activity filter code (`protocol:fiber` works)
- No new frontend classification logic (`protocolAction` displayType works)
- No custom activity stats code (`protocol_action_counts` auto-increments)

### Storage Impact

- 3 new domain CFs (CF_FIBER_CHANNELS, CF_FIBER_CHANNEL_BY_COMMITMENT, CF_ADDR_FIBER_CHANNELS)
- No AssetChange serialization changes
- Requires reindex (new detector produces new ProtocolAction values)

### Store Boundary

- All 3 new CFs are **domain store** (mutable canonical view)
- No changes to append-only store (CF_CELLS)
- Write paths: indexer only (via StoreBatch)
- API: read-only access
