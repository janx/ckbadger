# Activity System Design

## Philosophy

Activities are **interpretations, not facts**. A simple form of activity is the interpretation of a per-owner position change in a single transaction: how much CKB moved, how much occupied capacity changed, which assets were affected, and who the counterparties were.

More sophisticated activity systems may interpret two owners' position changes in a single transaction as a single 'swap' activity rather than two separate activities. Since UTXO transactions are atomic action bundles involving multiple parties, the combination possibilities and thus possible interpretations are endless.

## Three-Layer Analysis Model

Activity analysis decomposes into three layers. Each layer adds interpretation on top of the layers below. Layers are **composable, not mutually exclusive** — a single activity may have signals at all three layers simultaneously.

```
Layer 3: Protocol Action     WHY — cross-script pattern interpretation
                             "RGB++ leap to CKB"
                             Present when a ProtocolDetector matches

Layer 2: Asset Change        WHAT — recognized asset mutations
                             Token delta, DAO deposit, Spore mint, .bit update
                             TypeCallEntry / LockCallEntry for unrecognized scripts
                             Present when type/lock scripts are involved

Layer 1: CKB Position        HOW MUCH — capacity arithmetic
                             ckb_delta, used_delta
                             Always present. Not a fallback.
```

Raw cells (InputCellView, ParsedCell, witnesses) are Layer 0 — the ground truth that all layers derive from, but not stored in activities. Available via transaction lookup.

### Layer 1: CKB Position (always present)

Every activity has a CKB position change. It is computed by pure arithmetic on cell capacities:

```
ckb_delta  = sum(output_capacities) - sum(input_capacities)  for this owner
used_delta = change in occupied capacity
```

CKB delta is a **dimension**, not a classification. A DAO deposit has `ckb_delta = -102 CKB`. A token transfer has `ckb_delta = -0.001 CKB` (fee). A coinbase has `ckb_delta = +1065 CKB`. The CKB position is the bedrock measurement that is always meaningful.

"CKB transfer" as a display type means: "the only interesting signal is CKB delta" — the degenerate case where Layers 2 and 3 are empty. It is not a separate activity type; it is the absence of higher-layer signals.

### Layer 2: Asset Change (when type/lock scripts are involved)

Asset changes are derived from recognizing type scripts and parsing cell data. They answer: **what assets were involved and what happened to them?**

Recognized assets become `AssetChange` variants:

- `Token` — xUDT/sUDT delta (fungible)
- `DaoDeposit` / `DaoWithdrawRequest` / `DaoWithdrawComplete` — NervosDAO lifecycle
- `Object` — Spore, mNFT mint/transfer/burn (non-fungible)
- `Identity` — .bit, did:ckb create/update/recycle (identity)

Unrecognized scripts become catch-all entries:

- `TypeCallEntry` — type scripts we cannot yet interpret
- `LockCallEntry` — non-standard lock scripts on outputs

Both recognized and unrecognized entries are the same conceptual layer — "what scripts were involved and what did they do" — differing only in whether we have a dedicated parser. As more parsers and `ProtocolDetector` implementations land, some catch-all entries get promoted: lock calls may become Layer 3 protocol actions; type calls may become recognized asset changes.

### Layer 3: Protocol Action (cross-layer patterns)

Protocol actions are the highest-level interpretation. They combine Layer 2 signals (asset changes, lock calls) with Layer 0 raw data (cell scripts, witnesses) to identify **protocol-level actions** that span multiple scripts or owners.

A protocol action **explains** the lower-layer signals, it does not **replace** them:

```
Layer 3:  rgbpp:leap_to_ckb     ← explains WHY this pattern of changes happened
Layer 2:  Token +1,000 XUDT     ← explains WHAT asset moved
Layer 1:  +500 CKB              ← explains HOW MUCH CKB changed
```

All three layers are simultaneously true and should be simultaneously visible to the user.

Protocol actions are detected by `ProtocolDetector` implementations (see `docs/plans/2026-03-14-protocol-action-framework-design.md`). Each detector receives ALL accumulated Layer 2 signals and the full transaction view, then returns zero or more `ProtocolAction` values.

## Display Classification

The frontend needs a single "display type" for badge/icon/color selection. This is a **lossy projection** of the layered analysis:

```
1. protocolActions.length > 0   → 'protocolAction'
2. assetChanges (DAO > token > object > identity)
3. typeCalls.length > 0         → 'typeCall'
4. (none of the above)          → 'ckbTransfer'
```

This projection picks a headline. It does NOT mean the other layers are absent or unimportant. The UI should render all non-empty layers, with the display type governing the headline treatment.

## Rendering: Show All Layers

Activity rendering should present all non-empty layers, not just the winner:

```
🔷 RGB++ · leap to ckb          btc:abc1...ef       ← Layer 3 headline
📦 XUDT Transfer                 +1,000 XUDT         ← Layer 2 detail
💰 CKB                           +500.00 CKB         ← Layer 1 detail
```

For a pure CKB transfer (Layers 2 and 3 empty):

```
💰 CKB Transfer                  -100.00 CKB         ← Layer 1 only
```

For a DAO deposit (Layer 3 empty):

```
🏦 DAO Deposit                   102.00 CKB          ← Layer 2 headline
💰 CKB                           -102.00 CKB         ← Layer 1 detail
```

## Statistics

Daily activity stats classify each activity for aggregate counting. The classification is **per-layer**, not mutually exclusive:

- Layer 1: every non-coinbase activity contributes to `total_ckb_moved`
- Layer 2: asset changes contribute to `token_count`, `dao_deposit_count`, `object_count`, etc.
- Layer 3: protocol actions contribute to `protocol_action_counts` (`"rgbpp:leap_to_ckb" -> 3`)

A single activity may increment counters at multiple layers. For example, an RGB++ token leap increments both `protocol_action_counts["rgbpp:leap_to_ckb"]` AND `token_count`.

The mutually-exclusive `transfer_count` bucket is the Layer 1 degenerate case: activities with CKB delta but no Layer 2 or Layer 3 signals.
