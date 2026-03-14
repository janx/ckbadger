# Stable++ Protocol Support Refactor Design

## Problem

The `TypeCallResponse.protocol_name` field and `PROTOCOL_INDEX` static were introduced before the protocol action framework existed. Now that Layer 3 `ProtocolAction` / `ProtocolDetector` is the canonical mechanism for protocol identification, `protocol_name` is a pre-framework workaround that creates conceptual confusion:

- Two independent protocol identity systems coexist (API-time `PROTOCOL_INDEX` vs indexer-time `ProtocolDetector`)
- `PROTOCOL_ACTION_LOCKS` and `lock_call_role()` in the API are also pre-framework — Fiber now has a proper `FiberDetector`
- Stable++ has no `ProtocolDetector`, so its protocol-level operations are invisible at Layer 3
- `script_name` already carries protocol context (e.g., "Stable++ Pool"), making `protocol_name` decoration low-value

## Scope

Three parts:

1. **Remove `protocol_name`** — delete `TypeCallResponse.protocol_name`, `PROTOCOL_INDEX`, `PROTOCOL_ACTION_LOCKS`, `lock_call_role()`, and `LockCallResponse.role`
2. **Clean up frontend** — remove `protocolName` from type call rendering, remove `role === 'protocol_action'` classification path
3. **Implement `StableppDetector`** — proper Layer 3 protocol action detection for Stable++ CDP operations

## Part 1: Cleanup

### Backend (`crates/api/src/routes/activities.rs`)

Remove:

- `TypeCallResponse.protocol_name: Option<String>` field
- `PROTOCOL_INDEX` static (LazyLock + JSON parse, lines 150-181)
- `protocol_name: PROTOCOL_INDEX.get(...)` in `convert_type_call()` (line 288)
- `PROTOCOL_ACTION_LOCKS` static (lines 305-315)
- `lock_call_role()` function (lines 439-445)
- `LockCallResponse.role` field (line 110)
- `role: role.to_string()` in `convert_lock_call()` (line 464)

### Frontend

- `lib/api.ts` — remove `protocolName?` from `ActivityTypeCall`, remove `role` from `ActivityLockCall`
- `lib/activity-classify.ts` — remove `role === 'protocol_action'` fallback classification path (lines 88-98)
- `components/activity-event-row.tsx` — `getTypeEventParts()`: replace `sc.protocolName` logic with `sc.scriptName || 'Type call'`
- `components/latest-activities.tsx` — `StreamItemTypeCall`: same, use `scriptName` directly
- Tests: update accordingly

### Config

`docs/script-name-overrides.json` `protocols` field **retained** — useful metadata for detector code_hash registration.

## Part 2: StableppDetector

### Protocol Background

Stable++ is a CDP (Collateralized Debt Position) protocol on CKB. Users deposit CKB/BTC collateral into vaults and mint RUSD (USD-pegged stablecoin). Uses an aggregator node + intent cell architecture. Contracts are closed-source.

UTXOSwap and Stable++ are independent projects from separate teams. They are not from the same organization.

### Scripts

| Script               | Role                   | Type | Code Hash (mainnet) |
| -------------------- | ---------------------- | ---- | ------------------- |
| Stable++ Asset       | UDT token (RUSD, wCKB) | type | `0x26a33e...`       |
| Stable++ Pool        | Protocol global state  | type | `0x266221...`       |
| Stable++ Intent Lock | User intent cell       | lock | `0x56fb63...`       |
| Stable++ Vault Lock  | Collateral vault       | lock | `0xff3520...`       |

Asset is already recognized as xudt_compatible UDT (handled at Layer 2). The other three are the detection targets.

### Code Hash Source

Hardcoded in `crates/indexer/src/parser/stablepp.rs`, consistent with RgbppDetector and FiberDetector patterns. Mainnet code hashes + testnet Asset code hash.

### Detection Logic

Detector receives `TxView`, `owner_lock_hash`, `accum`, `asset_changes`, `type_calls`, `lock_calls`.

**Step 1: Relevance check** — scan type_calls and lock_calls code_hashes for any Stable++ script. Return empty if none found.

**Step 2: Vault Lock lifecycle** — count Vault Lock cells in accum inputs vs outputs:

- `vault_in_inputs`: count of input cells with Vault Lock
- `vault_in_outputs`: count of output cells with Vault Lock

**Step 3: RUSD delta** — find Stable++ Asset token change in `asset_changes`:

- `rusd_delta > 0` → user receives RUSD (mint/borrow)
- `rusd_delta < 0` → user consumes RUSD (repay/close/redeem)

**Step 4: Action inference**

| vault_in_inputs | vault_in_outputs | rusd_delta | Action        |
| :-------------: | :--------------: | :--------: | ------------- |
|        0        |        >0        |     >0     | `open_vault`  |
|        0        |        >0        |     0      | `deposit`     |
|       >0        |        >0        |     >0     | `borrow`      |
|       >0        |        >0        |     <0     | `repay`       |
|       >0        |        >0        |     0      | `adjust`      |
|       >0        |        0         |     <0     | `close_vault` |
|       >0        |        0         |     0      | `liquidation` |
|        0        |        0         |     ≠0     | `redemption`  |
|     (other)     |     (other)      |   (any)    | `interaction` |

### Metadata

```json
{
  "hasIntent": true,
  "vaultCount": 1
}
```

No cell data parsing (closed-source contracts). Only externally observable signals.

### Owner Perspective

Detector runs independently per owner. Users see their RUSD/CKB deltas; protocol addresses see the inverse. Owners not holding any Stable++ scripts exit at Step 1.

### Registration

Added to both bulk and live sync detector lists in `crates/indexer/src/sync/batch.rs`:

```rust
Box::new(StableppDetector::new(self.config.is_mainnet())),
```

## Part 3: Frontend Rendering

### Type Call Display (post-cleanup)

Before: `⚙ Stable++ · Pool(0xab12...cd34)` (via protocolName)
After: `⚙ Stable++ Pool(0xab12...cd34)` (via scriptName)

`getTypeEventParts()` simplified: `badge = scriptName || "Type call"`.

### Protocol Action Display

Uses existing `StreamItemProtocolAction` + `getProtocolActionEventParts` rendering path. Add action label mapping:

```typescript
const STABLEPP_ACTION_LABELS: Record<string, string> = {
  open_vault: 'Open Vault',
  borrow: 'Borrow',
  repay: 'Repay',
  close_vault: 'Close Vault',
  deposit: 'Deposit',
  adjust: 'Adjust Vault',
  liquidation: 'Liquidation',
  redemption: 'Redemption',
  interaction: 'Interaction',
};
```

Rendered as three-layer display:

```
⚡ stablepp · Open Vault
   RUSD Transfer    +500.00 RUSD
   CKB              -1,000.00 CKB
```

### Lock/Type Call Dedup

No new dedup logic for Stable++. Existing lock_call dedup (via `decoded.protocol`) continues working for rgbpp/fiber. Stable++ lock/type calls render as Layer 2 detail alongside Layer 3 headline — consistent with the activity design philosophy of showing all non-empty layers.

## Tests

### Rust Unit Tests (`stablepp_detector.rs`)

- `test_stablepp_detector_protocol_name` — returns `"stablepp"`
- `test_no_stablepp_scripts_returns_empty` — no Stable++ scripts → empty
- `test_open_vault` — Vault only in outputs + RUSD mint → `open_vault`
- `test_borrow` — Vault in inputs+outputs + RUSD mint → `borrow`
- `test_repay` — Vault in inputs+outputs + RUSD burn → `repay`
- `test_close_vault` — Vault only in inputs + RUSD burn → `close_vault`
- `test_liquidation` — Vault only in inputs + no RUSD delta → `liquidation`
- `test_fallback_interaction` — unmatched pattern → `interaction`
- `test_intent_lock_metadata` — Intent Lock present → `hasIntent: true`

### Frontend Test Updates

- `activity-event-row.test.tsx` — remove `protocolName` tests, verify `scriptName` display
- `latest-activities.test.tsx` — same
- `activity-classify.test.ts` — remove `role === 'protocol_action'` tests

## Reindex

Not required. `LockCallResponse.role` is API-computed (not stored). `protocol_actions` field already exists in `ActivityEntry`. StableppDetector takes effect on new blocks; historical data with empty `protocol_actions` is normal. Full reindex only needed if historical Stable++ protocol actions are desired.

## Files Changed

| File                                                | Change                                                                                              |
| --------------------------------------------------- | --------------------------------------------------------------------------------------------------- |
| `crates/api/src/routes/activities.rs`               | Remove `protocol_name`, `PROTOCOL_INDEX`, `PROTOCOL_ACTION_LOCKS`, `lock_call_role()`, `role` field |
| `crates/indexer/src/parser/stablepp.rs`             | New: code hash constants                                                                            |
| `crates/indexer/src/parser/mod.rs`                  | Add `pub mod stablepp`                                                                              |
| `crates/indexer/src/db/writer/stablepp_detector.rs` | New: `StableppDetector` impl                                                                        |
| `crates/indexer/src/db/writer/mod.rs`               | Add `pub mod stablepp_detector`                                                                     |
| `crates/indexer/src/sync/batch.rs`                  | Register StableppDetector in both bulk/live detector lists                                          |
| `frontend/lib/api.ts`                               | Remove `protocolName` from `ActivityTypeCall`, `role` from `ActivityLockCall`                       |
| `frontend/lib/activity-classify.ts`                 | Remove `role === 'protocol_action'` path                                                            |
| `frontend/components/activity-event-row.tsx`        | Use `scriptName` for type calls, add stablepp action labels                                         |
| `frontend/components/latest-activities.tsx`         | Use `scriptName` for type calls, add stablepp action labels                                         |
| `frontend/__tests__/components/*.test.tsx`          | Update tests                                                                                        |
| `frontend/__tests__/lib/activity-classify.test.ts`  | Update tests                                                                                        |
| `docs/ACTIVITY_SYSTEM.md`                           | Update Protocol Grouping section, file reference                                                    |
