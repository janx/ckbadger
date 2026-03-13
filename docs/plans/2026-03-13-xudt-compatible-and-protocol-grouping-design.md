# Design: xudt_compatible UDT Recognition + Protocol Grouping

## Problem

Stable++ Asset, ccBTC Asset, wCKB Asset, and USDI Asset are `xudt_compatible` scripts — their cell data layout is identical to xUDT (u128 amount in first 16 bytes). But the activity builder only recognizes 3 hardcoded UDT code_hashes (SUDT, XUDT data1, XUDT type). These tokens are classified as `scriptCall` instead of `token`, making RUSD transfers show as "Script call Stable++ Asset(0x360c...)" instead of "RUSD Transfer".

Additionally, when a tx involves multiple scripts from the same protocol (e.g., Stable++ Pool + Vault Lock + Intent Lock), the frontend shows them as unrelated script calls with no protocol context.

## Layer 1: Recognize xudt_compatible as UDT

### Compile time: build.rs

`build.rs` already bundles script labels. Add a step: extract all deployment code_hashes from scripts with `decoderType: "udt"`, excluding the 3 already-hardcoded (SUDT, XUDT data1, XUDT type) and deprecated deployments. Write to `bundled_udt_script_code_hashes.json`:

```json
[
  "0x26a33e0815888a4a0614a0b7d09fa951e0993ff21e55905510104a0b1312032b",
  "0x092c2c4a26ea475a8e860c29cf00502103add677705e2ccd8d6fe5af3caa5ae3",
  "0x42a0b2aacc836c0fc2bbd421a9020de42b8411584190f30be547fdf54214acc3",
  "0xbfa35a9c38a676682b65ade8f02be164d48632281477e36f8dc2f41f79e56bfc",
  "0x1142755a044bf2ee358cba9f2da187ce928c91cd4dc8692ded0337efa677d21a"
]
```

### Runtime: Activity builder CodeHashes::new()

`include_bytes!` loads the bundled file. Parse and append these code_hashes to the `AssetKind::Udt` lookup map. The `OnceLock` initialization remains synchronous with no DB dependency.

### Effect

RUSD, ccBTC, wCKB, USDI transactions become `token` activities, displayed as "RUSD Transfer" on the frontend.

### Why compile-time, not runtime

Label import runs as a background async task after startup. The activity builder's `CodeHashes` is a `OnceLock` initialized eagerly on first activity processing. Bundling at compile time ensures the data is available synchronously without architectural changes.

## Layer 2: Protocol Grouping

### Data source: script-name-overrides.json

New `protocols` field mapping protocol name to code_hash list:

```json
{
  "protocols": {
    "Stable++": [
      "0x26a33e0815888a4a0614a0b7d09fa951e0993ff21e55905510104a0b1312032b",
      "0x56fb632a13abdad7308d2e034baae1cb049e8e8ff23cc7c0b69449f617549733",
      "0x26622198b66240e437e323e0fecf1c26ba3c8c28a45f03ed3ebb9f7f2bdc0055",
      "0xff352022029a6ecf03e8a838b979a46e1231f05f9a3df9b4198f7eeb4afc2e67"
    ],
    "Godwoken": ["...list of godwoken code_hashes..."]
  }
}
```

### Backend: API enrichment

`ScriptNameOverrides` struct gains `protocols: HashMap<String, Vec<String>>`.

Build a reverse index `code_hash → protocol_name`. `convert_script_call()` populates:

```rust
pub struct ScriptCallResponse {
    // ...existing fields...
    pub protocol_name: Option<String>,
}
```

Loaded from bundled overrides via `LazyLock<HashMap<Vec<u8>, String>>`. Available at API startup, no async dependency.

### Frontend: Grouped rendering

`ActivityScriptCall` type gains `protocolName?: string`.

**Homepage latest-activities** (`StreamItemScriptCall`): when `protocolName` exists, badge shows protocol name:

```
⚙ Stable++ · Pool(0xab12...cd34)
ckb1q...addr    -500.00 CKB
```

**Address page** (`ScriptCallBadge`): script calls with the same protocolName are grouped:

```
Stable++
  Pool        | type
  Vault Lock  | type
```

Script calls without `protocolName` render as before.

## Edge Cases

1. **Mixed protocols in one tx** — group by protocol, ungrouped calls render individually
2. **UDT + script call coexistence** — Layer 1 makes Stable++ Asset a token change, but Pool/Vault remain as script calls. Activity classified as `token` (higher priority); script calls still rendered in `scriptCalls` array
3. **Adding new xudt_compatible script** — add script + UDT entry in token-labels with `decoderType: "udt"`, rebuild
4. **Adding new protocol** — add entry in `script-name-overrides.json` `protocols`, rebuild

## Testing

| Layer | Test                                                                             | Type                       |
| ----- | -------------------------------------------------------------------------------- | -------------------------- |
| 1     | build.rs output contains Stable++ Asset code_hash                                | build verification         |
| 1     | `CodeHashes::classify()` returns `AssetKind::Udt` for xudt_compatible code_hash  | unit test                  |
| 1     | Tx with Stable++ Asset type script produces `AssetChange::Token` not script_call | activity builder unit test |
| 2     | `ScriptCallResponse` includes `protocolName` field                               | API integration test       |
| 2     | Frontend groups script calls by protocolName                                     | frontend unit test         |

## Re-sync

Layer 1 changes activity classification at write time — requires re-sync. Layer 2 is API/frontend display only — no re-sync needed.

## Files Changed

| File                                          | Change                                                           |
| --------------------------------------------- | ---------------------------------------------------------------- |
| `crates/indexer/build.rs`                     | Extract UDT script code_hashes to bundled JSON                   |
| `crates/indexer/src/db/writer/activities.rs`  | Load bundled UDT code_hashes in `CodeHashes::new()`              |
| `docs/script-name-overrides.json`             | Add `protocols` field                                            |
| `crates/indexer/src/label_import.rs`          | Parse `protocols` in `ScriptNameOverrides`                       |
| `crates/api/src/routes/activities.rs`         | Add `protocol_name` to `ScriptCallResponse`, build reverse index |
| `crates/api/src/utils/assets.rs`              | Parse `protocols` in `ScriptNameOverridesDoc`                    |
| `frontend/lib/api.ts`                         | Add `protocolName` to `ActivityScriptCall`                       |
| `frontend/components/latest-activities.tsx`   | Protocol-aware script call badge                                 |
| `frontend/app/address/[addr]/client-page.tsx` | Group script calls by protocol                                   |
