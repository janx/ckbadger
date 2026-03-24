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

- **Cross-user behaviors** — actions that span multiple participants:
  - "Alice and Bob completed a UTXOSwap trade"
  - "RGB++ leap from BTC to CKB"
  - "Fiber channel opened between Alice and Bob"

- **Item actions** — actions performed on specific items:
  - "DAO deposit of 102 CKB"
  - "Pet #123 was patted"
  - ".bit xyz records updated"
  - "DAO withdrawal requested"

A single protocol action may express both, e.g., "Spore transfer from Alice to Bob" is both a cross-user action and an item action.

A protocol action **explains** the lower-layer signals, it does NOT replace them:

```
Layer 3:  rgbpp:leap_to_ckb     <- explains WHY this pattern of changes happened
Layer 2:  Token +1,000 XUDT     <- explains WHAT item position changed
Layer 1:  +500 CKB              <- explains HOW MUCH CKB changed
```

All three layers are simultaneously true and should be simultaneously visible to the user.

#### Catch-All Entries

Unrecognized type scripts and non-standard lock scripts are recorded as catch-all entries at the TX level. As more protocol detectors land, some catch-all entries get promoted to named protocol actions. This is the natural growth path: unrecognized today, recognized tomorrow.

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
RGB++ Leap to CKB           +500 CKB, XUDT +1,000    <- L3 headline, L1+L2 detail
DAO Deposit                 -102 CKB                 <- L3 headline, L1 detail
CKB Transfer                -100.00 CKB              <- Layer 1 only
```

## Statistics

Daily activity stats classify each activity for aggregate counting. The classification is **per-layer**, not mutually exclusive:

- Layer 1: every non-coinbase activity contributes to `total_ckb_moved`
- Layer 2: item deltas contribute to `token_count`, `object_count`, etc.
- Layer 3: protocol actions contribute to `protocol_action_counts` (`"rgbpp:leap_to_ckb" -> 3`, `"dao:deposit" -> 5`)

A single activity may increment counters at multiple layers.
