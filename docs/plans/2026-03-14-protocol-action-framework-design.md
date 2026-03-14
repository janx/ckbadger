# Protocol Action Framework Design

## Problem

The activity system captures two layers of information:

- **Type script layer** (`AssetChange`): asset mutations (UDT delta, DAO deposit, Spore mint)
- **Lock script layer** (`LockCallEntry`): non-standard locks appearing on outputs

Missing is a **higher-level interpretation layer** that combines these signals to identify protocol-level actions. For example, an RGB++ "leap to CKB" involves a lock transition (rgbpp lock -> standard lock) on a cell carrying a typed asset (xUDT), which spans both layers. Other protocols (UTXOSwap, Fiber) will have similar cross-layer patterns.

## Design

### Layer Architecture

```
Layer 3 (new):   ProtocolAction      "RGB++ leap to CKB carrying 1000 XUDT"
                                     Composed from one or more Layer 2 signals

Layer 2 (exists): AssetChange        UDT delta, DAO deposit, Spore mint...
                  TypeCallEntry      Unrecognized type script invocations
                  LockCallEntry      Non-standard locks on outputs

Layer 1 (exists): InputCellView      Raw input cell: lock, type, data, capacity
                  ParsedCell         Raw output cell
                  witnesses          Raw witness hex strings
```

### Data Model

```rust
/// A protocol-level action detected by analyzing cross-layer signals.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolAction {
    /// Protocol identifier: "rgbpp", "utxoswap", "fiber", etc.
    pub protocol: String,
    /// Action name: "leap_to_ckb", "leap_to_btc", "transfer", etc.
    pub action: String,
    /// Protocol-specific decoded metadata (BTC TXID, channel ID, etc.)
    pub metadata: serde_json::Value,
}
```

New field on `OwnerActivityDelta` and `ActivityEntry`:

```rust
#[serde(default)]
pub protocol_actions: Vec<ProtocolAction>,
```

### ProtocolDetector Trait

Each protocol registers a detector that receives ALL accumulated Layer 2 signals for an owner and the full transaction view:

```rust
trait ProtocolDetector {
    fn protocol_name(&self) -> &str;

    fn detect(
        &self,
        tx: &TxView<'_>,
        owner_lock_hash: &[u8],
        accum: &OwnerAccum,
        asset_changes: &[AssetChange],
        type_calls: &[TypeCallEntry],
        lock_calls: &[LockCallEntry],
    ) -> Vec<ProtocolAction>;
}
```

Invocation point in `build_tx_activity_bundle` -- after ALL Layer 2 signals are generated:

```
1. Process inputs -> accum
2. Process outputs -> accum
3. Detect lock calls -> lock_calls
4. Emit asset changes -> asset_changes
5. Emit type calls -> type_calls
6. >>> Run protocol detectors <<<
7. Build OwnerActivityDelta with all fields
```

### TxView Extension

`TxView` gains a `witnesses` field for detectors that need raw witness access:

```rust
pub struct TxView<'a> {
    // ... existing fields ...
    pub witnesses: &'a [String],
}
```

`TxData` already carries `witnesses: Vec<String>` from RPC. Just thread `&td.witnesses` through to `TxView` construction.

## RGB++ Detector (First Implementation)

### Lock Transition Detection

`RgbppDetector` identifies RGB++ actions by comparing lock scripts across inputs and outputs for cells sharing the same type_script identity `(type_code_hash, type_args)`:

| Input lock            | Output lock                | Action            |
| --------------------- | -------------------------- | ----------------- |
| rgbpp                 | rgbpp (different BTC UTXO) | `transfer`        |
| rgbpp / btc_time_lock | standard CKB lock          | `leap_to_ckb`     |
| standard CKB lock     | rgbpp                      | `leap_to_btc`     |
| rgbpp                 | btc_time_lock              | `btc_time_locked` |
| (no matching input)   | rgbpp                      | `receive`         |

### Metadata

All data comes from lock args (already parsed by `RgbppParser`). No witness parsing needed -- RGB++ commitment is implicit through isomorphic binding (lock args reference BTC UTXO), not a parseable witness structure.

```json
{
  "btcTxid": "9993846c...ec06",
  "outIndex": 2,
  "carriedAsset": "token:XUDT"
}
```

### Code Hashes

RGB++ lock and BTC time lock code hashes are already defined in `crates/indexer/src/parser/rgbpp.rs` for mainnet, testnet3, and signet.

## API Layer

### Response Type

```rust
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProtocolActionResponse {
    pub protocol: String,
    pub action: String,
    pub metadata: serde_json::Value,
}
```

New field on `ActivityResponse` and `GlobalActivityResponse`:

```rust
pub protocol_actions: Vec<ProtocolActionResponse>,
```

Direct passthrough from store -- already JSON-friendly.

### Filter

New generic filter format: `protocol:{name}`

- `protocol:rgbpp` -- matches activities with any `protocol_actions` where `protocol == "rgbpp"`
- Future: `protocol:utxoswap`, `protocol:fiber`, etc.

Implementation in `activity_ops.rs`:

```rust
filter if filter.starts_with("protocol:") => {
    let name = &filter["protocol:".len()..];
    entry.protocol_actions.iter().any(|a| a.protocol == name)
}
```

API validation allows `protocol:*` prefix in addition to existing fixed filters.

### Daily Stats

```rust
/// Per-protocol action counts: "rgbpp:leap_to_ckb" -> 3
#[serde(default)]
pub protocol_action_counts: HashMap<String, u32>,
```

Key format `"{protocol}:{action}"`. Generic -- new protocols don't need new fields.

## Frontend

### TypeScript Types

```typescript
interface ActivityProtocolAction {
  protocol: string;
  action: string;
  metadata: Record<string, unknown>;
}

interface Activity {
  // ... existing fields ...
  protocolActions: ActivityProtocolAction[];
}
```

### Display: Parallel Event Rows

Protocol actions render as the FIRST event rows in `ActivityEventGroup`, above asset changes:

```
protocolActions  -> RGB++ leap to ckb     btc:abc1...ef (+1000 XUDT)
assetChanges     -> XUDT Transfer         +1,000 XUDT
ckbDelta         -> CKB Transfer          +500 CKB
```

### Classification Priority

```typescript
classifyActivity(activity):
  1. protocolActions.length > 0  -> 'protocolAction'
  2. assetChanges (DAO > token > object > identity)
  3. lock calls with role=protocol_action (legacy)
  4. type calls
  5. fallback: ckbTransfer
```

### Lock Call Deduplication

When a `protocolAction` with protocol P exists, hide `lockCall` event rows whose `decoded.protocol == P` to avoid redundant display. The underlying `LockCallEntry` data is preserved -- only the frontend rendering is deduplicated.

## Requires Reindex

Yes. Serialization format changes (`protocol_actions` field added to `OwnerActivityDelta` / `ActivityEntry`).
