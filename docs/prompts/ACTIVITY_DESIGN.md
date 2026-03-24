# Activity System Design

## Philosophy

Activities include both facts and interpretations. A CKB UTXO transaction is an atomic action bundle involving multiple parties. The activity system interprets each transaction from two complementary perspectives:

- **Per-participant**: what changed in each participant's personal balance sheet (Layer 1 + Layer 2)
- **Per-transaction**: what happened as a whole, across all participants (Layer 3)

## Three-Layer Analysis Model

Activity analysis decomposes into three layers. Each layer adds interpretation on top of the layers below. Layers are **composable, not mutually exclusive** — a single activity may have signals at all three layers simultaneously.

```
Layer 3: Protocol Action     TX level interpretations
                             - Cross-user behavior: "RGB++ leap", "UTXOSwap swap"
                             - Item action: "DAO deposit", "pet pat", ".bit update"
                             Present when a ProtocolDetector matches

Layer 2: Item Delta          User level interpretations, per-participant balance sheet
                             Token delta, Item arrived/departed
                             Only records position changes (delta != 0)
                             Present when participant gained or lost items

Layer 1: CKB Position        User level, changes of CKBytes, the raw material
                             ckb_delta, used_delta
                             Always present. Not a fallback.
```

### Layer 1: CKB Position (per-participant, always present)

Every participant has a CKB position change. It is computed by pure arithmetic on cell capacities:

```
ckb_delta  = sum(output_capacities) - sum(input_capacities)  for this participant
used_delta = change in occupied capacity
```

CKB delta is a **dimension**, not a classification. A DAO deposit has `ckb_delta = -102 CKB`. A token transfer may have `ckb_delta = -188 CKB` (capacity transferred with tokens). A coinbase has `ckb_delta = +1065 CKB`. The CKB position is the bedrock measurement that is always meaningful.

### Layer 2: Item Delta (per-participant balance sheet)

Layer 2 records the participant's **personal balance sheet** — what items were gained or lost. It follows double-entry bookkeeping principles: each participant independently records their own position changes.

All item types — fungible tokens, non-fungible objects, identities — are recorded uniformly. Each entry is an item identifier, a kind tag, and a signed delta. Fungible items carry precise amounts; non-fungible items carry +1 (arrived) or -1 (departed). The kind tag is extensible to future asset types without structural changes.

#### What Layer 2 records

Layer 2 records **only position changes** (delta != 0):

- Token received or sent (with precise amount)
- Object (Spore, mNFT, ...) arrived or departed
- Identity (.bit, did:ckb, ...) arrived or departed

#### What Layer 2 does NOT record

- **delta=0 interactions** — If a participant still holds the same item after the transaction (e.g., patting a pet, updating .bit records, requesting DAO withdrawal), their balance sheet didn't change. The interaction is an item action interpreted at Layer 3.

- **DAO operations** — A DAO cell is CKB in a different state (locked vs free), not a separate item in the portfolio. The CKB movement is captured at Layer 1; the protocol semantics belong at Layer 3.

- **Mint/Transfer/Burn classification** — Layer 2 records what happened to your holdings, not why. Classification is derived from the pattern of deltas across participants or provided by Layer 3 protocol actions:

  | Scenario             | Alice's L2    | Bob's L2             | Layer 3 narrative          |
  | -------------------- | ------------- | -------------------- | -------------------------- |
  | Transfer             | spore_123: -1 | spore_123: +1        | "Spore Transfer"           |
  | Mint                 | —             | spore_123: +1        | "Spore Mint"               |
  | Burn                 | spore_123: -1 | —                    | "Spore Burn"               |
  | Pat                  | —             | —                    | "Pet Pat"                  |
  | Future: 1-to-N split | obj_a: -1     | obj_b: +1, obj_c: +1 | defined by future detector |

- **Counterparty information** — Layer 2 is self-contained per-participant. "Who did I trade with" is a Layer 3 cross-user interpretation.

### Layer 3: Protocol Action (TX level interpretations)

Protocol actions are the highest-level interpretation. Layer 3 concerns about:

- **Cross-user behavior interpretation** — actions that span multiple participants:
  - "Alice and Bob completed a UTXOSwap trade"
  - "RGB++ leap from BTC to CKB"
  - "Fiber channel opened between Alice and Bob"

- **Item action interpretation** — actions performed on specific items:
  - "DAO deposit of 102 CKB"
  - "Pet #123 was patted"
  - ".bit xyz records updated"
  - "DAO withdrawal requested"

These two concerns are not mutually exclusive — a single protocol action may express both (e.g., "Spore transfer from Alice to Bob" is both a cross-user action and an item action).

A protocol action **explains** the lower-layer signals, it does not **replace** them:

```
Layer 3:  rgbpp:leap_to_ckb     <- explains WHY this pattern of changes happened
Layer 2:  Token +1,000 XUDT     <- explains WHAT item position changed
Layer 1:  +500 CKB              <- explains HOW MUCH CKB changed
```

All three layers are simultaneously true and should be simultaneously visible to the user.

#### DAO as Protocol Actions

NervosDAO operations are protocol actions (Layer 3), not balance sheet items (Layer 2). A DAO cell is CKB reclassified from free to locked — not a new asset acquired.

- **Deposit**: Layer 1 records ckb_delta = -102. Layer 3 explains "this is a DAO deposit."
- **Withdraw Request**: Layer 1 records ckb_delta = 0 (cell consumed and recreated, same owner). Layer 3 explains "withdrawal was requested." This is a delta=0 interaction — exactly the kind of item action that belongs at Layer 3, not Layer 2.
- **Withdraw Complete**: Layer 1 records ckb_delta = +102 + compensation. Layer 3 explains "DAO withdrawal completed with X compensation."

All three DAO lifecycle states live at the same layer, providing consistent treatment.

#### Catch-All Entries

Unrecognized type scripts and non-standard lock scripts are recorded as catch-all entries at the TX level. As more `ProtocolDetector` implementations land, some catch-all entries get promoted to named protocol actions. This is the natural growth path: unrecognized today, recognized tomorrow.

Protocol actions are detected by `ProtocolDetector` implementations (see `docs/plans/2026-03-14-protocol-action-framework-design.md`). Each detector receives the full transaction view, then returns zero or more protocol actions.

## Storage Design Principles

**TX-level data stored once.** Protocol actions, unrecognized script calls, and all cross-user interpretations are properties of the transaction, not of individual participants. Storing them once (at the TX level) instead of N times (per participant) eliminates the largest source of redundancy in the current activity storage.

**Per-participant data is minimal.** Each participant stores only: CKB position (Layer 1), item deltas (Layer 2), and a classification bitmask. Lock script metadata (code_hash, hash_type, args), display metadata (symbol, decimals, standard), counterparty lists, and script involvement lists are all derivable from existing stores and are not duplicated in the activity record.

**Classification bitmask bridges layers.** A per-participant bitmask is computed at write time from both Layer 2 and Layer 3 signals. This enables fast filtering (e.g., "show me DAO activities for this address") without deserializing the full activity record. The bitmask is set when a participant has relevant item deltas (Layer 2) **or** is involved in relevant protocol actions (Layer 3), providing uniform filtering regardless of which layer carries the detail.

**Item deltas are uniform.** All asset types share the same structure: an identifier, a kind tag, and a signed delta. This avoids per-type fields that would require schema changes when new asset types emerge. The kind tag is extensible; new item kinds (future NFT standards, new fungible token protocols) are added without structural changes to the activity record.

## Display Classification

The frontend needs a single "display type" for badge/icon/color selection. This is a **lossy projection** of the layered analysis:

```
1. protocol_actions present    -> 'protocolAction'
2. item_deltas present         -> item kind priority: token > object > identity
3. type_calls present          -> 'typeCall'
4. (none of the above)         -> 'ckbTransfer'
```

This projection picks a headline. It does NOT mean the other layers are absent or unimportant. The UI should render all non-empty layers, with the display type governing the headline treatment.

## Rendering: Show All Layers

Activity rendering should present all non-empty layers, not just the winner.

**Cross-user view (global feed, transaction detail):**

All participants visible, protocol actions provide the narrative:

```
RGB++ Leap to CKB                                   <- Layer 3
  Alice: -500 CKB, XUDT -1,000                      <- Layer 1 + 2
  Bob:   +500 CKB, XUDT +1,000                      <- Layer 1 + 2
```

```
DAO Deposit                                          <- Layer 3
  Alice: -102 CKB                                    <- Layer 1
```

```
Spore Transfer                                       <- Layer 3
  Alice: -0.001 CKB, Spore #abc departed             <- Layer 1 + 2
  Bob:   +0.001 CKB, Spore #abc arrived              <- Layer 1 + 2
```

**Per-participant view (address feed):**

Filtered to one participant, with TX-level context:

```
RGB++ Leap to CKB          +500 CKB, XUDT +1,000    <- L3 headline, L1+L2 detail
DAO Deposit                 -102 CKB                 <- L3 headline, L1 detail
CKB Transfer                -100.00 CKB              <- Layer 1 only
```

## Statistics

Daily activity stats classify each activity for aggregate counting. The classification is **per-layer**, not mutually exclusive:

- Layer 1: every non-coinbase activity contributes to `total_ckb_moved`
- Layer 2: item deltas contribute to `token_count`, `object_count`, etc.
- Layer 3: protocol actions contribute to `protocol_action_counts` (`"rgbpp:leap_to_ckb" -> 3`, `"dao:deposit" -> 5`)

A single activity may increment counters at multiple layers. For example, an RGB++ token leap increments both `protocol_action_counts["rgbpp:leap_to_ckb"]` AND `token_count`.

The mutually-exclusive `transfer_count` bucket is the Layer 1 degenerate case: activities with CKB delta but no Layer 2 or Layer 3 signals.
