# xudt_compatible UDT Recognition + Protocol Grouping Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Make xudt_compatible tokens (RUSD, ccBTC, wCKB, USDI) show as token transfers instead of script calls, and group script calls by protocol (Stable++, Godwoken) in the frontend.

**Architecture:** Two layers. Layer 1: build.rs extracts `decoderType: "udt"` script code_hashes at compile time, activity builder loads them as `AssetKind::Udt`. Layer 2: `script-name-overrides.json` gains a `protocols` mapping, API enriches `ScriptCallResponse` with `protocolName`, frontend groups by protocol.

**Tech Stack:** Rust (build.rs, indexer, axum API), TypeScript/React (frontend), serde_json, TanStack Query.

**Design doc:** `docs/plans/2026-03-13-xudt-compatible-and-protocol-grouping-design.md`

---

## Task 1: build.rs — Extract UDT script code_hashes

**Files:**

- Modify: `crates/indexer/build.rs`

**Context:** `build.rs` already collects script labels into `bundled_script_labels.json`. We add a step that filters scripts with `decoderType: "udt"`, extracts all non-deprecated deployment code_hashes (both mainnet and testnet), excludes the 3 already-hardcoded ones (SUDT `0x5e7a36...`, XUDT data1 `0x50bd8d...`, XUDT type `0x25c29d...`), and writes the result as a JSON array of hex strings.

**Step 1: Add UDT script code_hash extraction to build.rs**

After the existing `// --- Script labels ---` section (line 42-54), add a new section that reuses the already-collected `script_entries`:

```rust
// --- UDT-compatible script code hashes ---
// Scripts with decoderType "udt" that aren't the 3 hardcoded UDT scripts.
// Used by the activity builder to classify xudt_compatible tokens as UDT.
let hardcoded_udt: std::collections::HashSet<&str> = [
    "0x5e7a36a77e68eecc013dfa2fe6a23f3b6c344b04005808694ae6dd45eea4cfd5",
    "0x50bd8d6680b8b9cf98b73f3c08faf8b2a21914311954118ad6609be6e78a1b95",
    "0x25c29dc317811a6f6f3985a7a9ebc4838bd388d19d0feeecf0bcd60f6c0975bb",
]
.into_iter()
.collect();

let mut extra_udt_code_hashes: Vec<String> = Vec::new();
for entry in &script_entries {
    let is_udt = entry
        .get("decoderType")
        .and_then(|v| v.as_str())
        .map(|s| s == "udt")
        .unwrap_or(false);
    if !is_udt {
        continue;
    }
    if let Some(deployments) = entry.get("deployments").and_then(|v| v.as_object()) {
        for network_deployments in deployments.values() {
            if let Some(deps) = network_deployments.as_array() {
                for dep in deps {
                    let deprecated = dep
                        .get("deprecated")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                    if deprecated {
                        continue;
                    }
                    if let Some(code_hash) = dep.get("codeHash").and_then(|v| v.as_str()) {
                        if !hardcoded_udt.contains(code_hash) {
                            extra_udt_code_hashes.push(code_hash.to_string());
                        }
                    }
                }
            }
        }
    }
}
extra_udt_code_hashes.sort();
extra_udt_code_hashes.dedup();
let udt_ch_json = serde_json::to_string(&extra_udt_code_hashes)
    .expect("failed to serialize UDT script code hashes");
fs::write(
    Path::new(&out_dir).join("bundled_udt_script_code_hashes.json"),
    udt_ch_json,
)
.expect("failed to write bundled_udt_script_code_hashes.json");
```

**Step 2: Verify build succeeds**

Run: `cargo check -p ckbadger-indexer`
Expected: compiles without errors.

**Step 3: Commit**

```
feat(indexer): extract xudt_compatible code_hashes at build time

build.rs now collects code_hashes from scripts with decoderType "udt"
(excluding the 3 hardcoded SUDT/XUDT) into bundled_udt_script_code_hashes.json.
```

---

## Task 2: Activity builder — Load bundled UDT code_hashes

**Files:**

- Modify: `crates/indexer/src/db/writer/activities.rs`

**Context:** `CodeHashes::new()` builds a HashMap of code_hash → AssetKind. Currently has 13 hardcoded entries. We add the bundled extra UDT code_hashes as `AssetKind::Udt`.

**Step 1: Write test for xudt_compatible classification**

Add to the `#[cfg(test)] mod tests` at the bottom of `activities.rs`. Use the Stable++ Asset mainnet code_hash as the test subject:

```rust
#[test]
fn test_xudt_compatible_code_hash_classified_as_udt() {
    use crate::rpc::parse_hex_to_bytes;
    let hashes = CodeHashes::new();

    // Stable++ Asset (mainnet) — xudt_compatible, decoderType "udt" in script labels
    let stablepp = parse_hex_to_bytes(
        "0x26a33e0815888a4a0614a0b7d09fa951e0993ff21e55905510104a0b1312032b",
    );
    assert_eq!(
        hashes.classify(&stablepp),
        Some(AssetKind::Udt),
        "Stable++ Asset should be classified as Udt"
    );

    // ccBTC Asset (mainnet)
    let ccbtc = parse_hex_to_bytes(
        "0x092c2c4a26ea475a8e860c29cf00502103add677705e2ccd8d6fe5af3caa5ae3",
    );
    assert_eq!(
        hashes.classify(&ccbtc),
        Some(AssetKind::Udt),
        "ccBTC Asset should be classified as Udt"
    );

    // Random unknown code_hash should still be None
    assert_eq!(hashes.classify(&[0x99; 32]), None);
}
```

**Step 2: Run test — verify it fails**

Run: `cargo test -p ckbadger-indexer test_xudt_compatible_code_hash_classified_as_udt`
Expected: FAIL — `left: None, right: Some(Udt)`.

**Step 3: Load bundled code_hashes in CodeHashes::new()**

At the top of the file (below existing imports), add the bundled data constant:

```rust
mod bundled_udt {
    pub const EXTRA_UDT_CODE_HASHES: &[u8] =
        include_bytes!(concat!(env!("OUT_DIR"), "/bundled_udt_script_code_hashes.json"));
}
```

In `CodeHashes::new()`, after the existing `let lookup = entries...collect();` (line 67-70), add:

```rust
let mut lookup: HashMap<Vec<u8>, AssetKind> = entries
    .iter()
    .map(|(hex, kind)| (parse_hex_to_bytes(hex), *kind))
    .collect();

// Extend with xudt_compatible scripts from bundled script labels (decoderType "udt").
let extra: Vec<String> =
    serde_json::from_slice(bundled_udt::EXTRA_UDT_CODE_HASHES)
        .expect("bundled UDT script code hashes JSON is invalid — build.rs bug");
for hex_str in &extra {
    let bytes = parse_hex_to_bytes(hex_str);
    lookup.entry(bytes).or_insert(AssetKind::Udt);
}

Self { lookup }
```

Note: this replaces the existing `let lookup = ...` binding — make it `let mut lookup`.

**Step 4: Run test — verify it passes**

Run: `cargo test -p ckbadger-indexer test_xudt_compatible_code_hash_classified_as_udt`
Expected: PASS.

**Step 5: Write integration test — xudt_compatible token produces AssetChange::Token**

Add test in the same test module. This mirrors the existing `test_unrecognized_type_script_produces_script_call` but uses a Stable++ Asset code_hash:

```rust
#[test]
fn test_xudt_compatible_produces_token_change_not_script_call() {
    use crate::rpc::parse_hex_to_bytes;

    let alice = 0xAA;
    let bob = 0xBB;

    // Stable++ Asset (mainnet) code_hash
    let stablepp_code_hash = parse_hex_to_bytes(
        "0x26a33e0815888a4a0614a0b7d09fa951e0993ff21e55905510104a0b1312032b",
    );
    let type_script_hash = vec![0x71; 32]; // dummy type_script_hash for RUSD
    let type_args = vec![0x36; 32]; // dummy args

    // Alice has 100 RUSD, sends to Bob
    let amount: u128 = 100_00000000;
    let mut data = amount.to_le_bytes().to_vec();
    data.extend_from_slice(&[0u8; 16]); // xUDT extra data padding

    let mut alice_input = make_input(alice, 200_00000000, 102_00000000);
    alice_input.type_code_hash = Some(stablepp_code_hash.clone());
    alice_input.type_script_hash = Some(type_script_hash.clone());
    alice_input.type_hash_type = Some(1);
    alice_input.type_args = Some(type_args.clone());
    alice_input.udt_amount = Some(amount);

    let mut bob_output = make_output(
        bob,
        200_00000000,
        Some(stablepp_code_hash),
        Some(type_script_hash),
        Some(type_args),
        data,
    );
    bob_output.type_hash_type = Some(1);

    let outputs = vec![bob_output];
    let tx = TxView {
        tx_hash: &[0x0B; 32],
        block_hash: &[0xBB; 32],
        tx_index: 1,
        block_number: 2000,
        timestamp: 1_700_000_000,
        is_cellbase: false,
        inputs: vec![alice_input],
        outputs: &outputs,
    };

    let activities = build_activities_for_block(&[tx], &HashMap::new());

    // Alice should have a Token asset change (negative delta), NOT a script_call
    let alice_act = activities
        .iter()
        .find(|(lh, _, _)| lh == &vec![alice; 32])
        .map(|(_, _, e)| e)
        .unwrap();
    assert!(
        alice_act.script_calls.is_none()
            || alice_act.script_calls.as_ref().unwrap().is_empty(),
        "xudt_compatible should not produce script_calls"
    );
    let has_token_change = alice_act
        .asset_changes
        .iter()
        .any(|c| matches!(c, AssetChange::Token { .. }));
    assert!(
        has_token_change,
        "xudt_compatible should produce Token asset change"
    );
}
```

**Step 6: Run test — verify it passes**

Run: `cargo test -p ckbadger-indexer test_xudt_compatible_produces_token_change_not_script_call`
Expected: PASS.

**Step 7: Run all existing tests to check for regressions**

Run: `cargo test -p ckbadger-indexer`
Expected: all pass.

**Step 8: Commit**

```
feat(indexer): classify xudt_compatible scripts as UDT in activity builder

CodeHashes::new() now loads bundled decoderType "udt" code_hashes from
build.rs output. RUSD, ccBTC, wCKB, USDI transactions produce Token
asset changes instead of script_calls. Requires re-sync.
```

---

## Task 3: script-name-overrides.json — Add protocols mapping

**Files:**

- Modify: `docs/script-name-overrides.json`

**Step 1: Add protocols field**

Add the `protocols` field to the existing JSON. Include Stable++ (4 mainnet scripts) and Godwoken (7 mainnet scripts):

```json
"protocols": {
  "Stable++": [
    "0x26a33e0815888a4a0614a0b7d09fa951e0993ff21e55905510104a0b1312032b",
    "0x56fb632a13abdad7308d2e034baae1cb049e8e8ff23cc7c0b69449f617549733",
    "0x26622198b66240e437e323e0fecf1c26ba3c8c28a45f03ed3ebb9f7f2bdc0055",
    "0xff352022029a6ecf03e8a838b979a46e1231f05f9a3df9b4198f7eeb4afc2e67"
  ],
  "Godwoken": [
    "0x628b5f956b46ae27b50819a9ebab79ce5f957e6899ba0c75b8e142de2ed0dcd2",
    "0x000f87062a2fe9bb4a6cc475212ea11014b84deb32e0375ee51e6ec4a553e009",
    "0xff602581f07667eef54232cce850cbca2c418b3418611c132fca849d1edcd775",
    "0x096df264f38fff07f3acd318995abc2c71ae0e504036fe32bc38d5b6037364d4",
    "0xb619184ab9142c51b0ee75f4e24bcec3d077eefe513115bad68836d06738fd2c",
    "0xfef1d086d9f74d143c60bf03bd04bab29200dbf484c801c72774f2056d4c6718",
    "0x3714af858b8b82b2bb8f13d51f3cffede2dd8d352a6938334bb79e6b845e3658"
  ]
}
```

Also add testnet code_hashes for both protocols to the same arrays (the reverse index is network-agnostic).

**Step 2: Commit**

```
data: add protocol grouping to script-name-overrides.json

Maps Stable++ and Godwoken code_hashes to their protocol names.
Used by the API to enrich ScriptCallResponse with protocolName.
```

---

## Task 4: Backend — Parse protocols + enrich ScriptCallResponse

**Files:**

- Modify: `crates/indexer/src/label_import.rs` (ScriptNameOverrides struct)
- Modify: `crates/api/src/routes/activities.rs` (ScriptCallResponse + convert_script_call)

**Context:** The `ScriptNameOverrides` struct in `label_import.rs` (line 96-106) needs a new `protocols` field. The API's `convert_script_call` needs to look up the protocol name from a reverse index built from the bundled overrides.

**Step 1: Add `protocols` to ScriptNameOverrides**

In `crates/indexer/src/label_import.rs`, add to `ScriptNameOverrides`:

```rust
#[derive(Debug, Clone, Default, Deserialize)]
struct ScriptNameOverrides {
    #[serde(default)]
    pub overrides: std::collections::HashMap<String, String>,
    #[allow(dead_code)]
    #[serde(default)]
    pub nft_storage_tier_overrides: std::collections::HashMap<String, String>,
    #[serde(default)]
    pub deprecated: Vec<String>,
    #[serde(default)]
    pub protocols: std::collections::HashMap<String, Vec<String>>,
}
```

This is backward-compatible — `#[serde(default)]` means missing field → empty HashMap.

**Step 2: Add `protocol_name` to ScriptCallResponse**

In `crates/api/src/routes/activities.rs`, add the field:

```rust
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptCallResponse {
    pub type_code_hash: String,
    pub type_hash_type: String,
    pub type_args: String,
    pub script_hash: String,
    pub script_name: Option<String>,
    pub protocol_name: Option<String>,
}
```

**Step 3: Build reverse protocol index from bundled overrides**

In `crates/api/src/routes/activities.rs`, add a `LazyLock` that builds the reverse index:

```rust
use std::sync::LazyLock;

static PROTOCOL_INDEX: LazyLock<HashMap<Vec<u8>, String>> = LazyLock::new(|| {
    let overrides: serde_json::Value =
        serde_json::from_slice(include_bytes!(concat!(
            env!("OUT_DIR"),
            "/bundled_script_overrides.json"
        )))
        .unwrap_or_default();

    let mut index = HashMap::new();
    if let Some(protocols) = overrides.get("protocols").and_then(|v| v.as_object()) {
        for (protocol_name, code_hashes) in protocols {
            if let Some(hashes) = code_hashes.as_array() {
                for hash_val in hashes {
                    if let Some(hex_str) = hash_val.as_str() {
                        let hex = hex_str.strip_prefix("0x").unwrap_or(hex_str);
                        if let Ok(bytes) = hex::decode(hex) {
                            index.insert(bytes, protocol_name.clone());
                        }
                    }
                }
            }
        }
    }
    index
});
```

Note: The `bundled_script_overrides.json` is already generated by `build.rs` and already `include_bytes!`'d in `label_import.rs`. The API crate is a different crate, so it needs its own include. Check if the API crate has its own build.rs or if it can reference the indexer's bundled data. If the API crate cannot include the indexer's OUT_DIR file, use an alternative: read the file at runtime from the known repo path (same pattern as `assets.rs` line 52: `Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/script-name-overrides.json")`).

**Step 4: Populate protocol_name in convert_script_call**

In `convert_script_call()`, after computing `script_name`:

```rust
Ok(ScriptCallResponse {
    type_code_hash: format!("0x{}", hex::encode(&call.type_code_hash)),
    type_hash_type: hash_type.to_string(),
    type_args: format!("0x{}", hex::encode(&call.type_args)),
    script_hash: format!("0x{}", hex::encode(script_hash)),
    script_name: normalized_script_name(script_info),
    protocol_name: PROTOCOL_INDEX.get(&call.type_code_hash).cloned(),
})
```

**Step 5: Run cargo check**

Run: `cargo check -p ckbadger-api`
Expected: compiles.

**Step 6: Run all tests**

Run: `cargo test -p ckbadger-api && cargo test -p ckbadger-indexer`
Expected: all pass (existing tests for ScriptNameOverrides deserialization should still pass since `protocols` has `#[serde(default)]`).

**Step 7: Commit**

```
feat(api): enrich ScriptCallResponse with protocolName

Parses protocols mapping from script-name-overrides.json into a reverse
code_hash→protocol index. ScriptCallResponse now includes protocolName
for Stable++ and Godwoken script calls.
```

---

## Task 5: Frontend — Add protocolName to types + grouped rendering

**Files:**

- Modify: `frontend/lib/api.ts`
- Modify: `frontend/components/latest-activities.tsx`
- Modify: `frontend/app/address/[addr]/client-page.tsx`

**Step 1: Add protocolName to ActivityScriptCall type**

In `frontend/lib/api.ts`, update the `ActivityScriptCall` interface (line 445-451):

```typescript
interface ActivityScriptCall {
  typeCodeHash: string;
  typeHashType: string;
  typeArgs: string;
  scriptHash: string;
  scriptName?: string;
  protocolName?: string;
}
```

**Step 2: Update homepage StreamItemScriptCall**

In `frontend/components/latest-activities.tsx`, update the `StreamItemScriptCall` component. When `protocolName` is present, show it as prefix:

Replace the existing `StreamItemScriptCall` (lines 442-463) with:

```tsx
function StreamItemScriptCall({ classified }: { classified: ClassifiedActivity }) {
  const { activity, primaryScriptCall } = classified;
  const badge = getTypeBadge(classified);
  const protocolName = primaryScriptCall?.protocolName;

  return (
    <>
      <div className="flex items-center justify-between gap-2">
        <span className={cn('min-w-0 truncate font-mono text-xs', badge.colorClass)}>
          {badge.icon}{' '}
          {protocolName ? (
            <>
              <span className="text-amber">{protocolName}</span>
              <span className="text-text-dim"> · </span>
            </>
          ) : (
            'Script call '
          )}
          {primaryScriptCall ? <ScriptCallExpr sc={primaryScriptCall} /> : null}
        </span>
        <span className="text-text-dim shrink-0 font-mono text-[10px]">
          {formatTimeAgo(activity.timestamp)}
        </span>
      </div>
      <div className="flex items-center justify-between gap-2">
        <AddressLink address={activity.address} />
        <CkbDelta delta={activity.ckbDelta} />
      </div>
    </>
  );
}
```

**Step 3: Update address page ScriptCallBadge with protocol grouping**

In `frontend/app/address/[addr]/client-page.tsx`, find where `activity.scriptCalls.map` renders `ScriptCallBadge` (around line 877 and 941). Add a grouping helper and update the rendering.

Add a helper function near the top of the component or file:

```typescript
function groupScriptCallsByProtocol(calls: ActivityScriptCall[]) {
  const groups: { protocol: string | null; calls: ActivityScriptCall[] }[] = [];
  const protocolMap = new Map<string, ActivityScriptCall[]>();
  const ungrouped: ActivityScriptCall[] = [];

  for (const call of calls) {
    if (call.protocolName) {
      const existing = protocolMap.get(call.protocolName);
      if (existing) {
        existing.push(call);
      } else {
        protocolMap.set(call.protocolName, [call]);
      }
    } else {
      ungrouped.push(call);
    }
  }

  for (const [protocol, protocolCalls] of protocolMap) {
    groups.push({ protocol, calls: protocolCalls });
  }
  if (ungrouped.length > 0) {
    groups.push({ protocol: null, calls: ungrouped });
  }
  return groups;
}
```

Then replace the script calls rendering sections (both desktop ~line 877 and mobile ~line 941) to use grouped rendering:

```tsx
{
  activity.scriptCalls.length > 0 &&
    (() => {
      const groups = groupScriptCallsByProtocol(activity.scriptCalls);
      return (
        <div className="flex min-w-0 flex-col items-end gap-1">
          {groups.map((group, gi) => (
            <div key={gi} className="flex flex-col items-end gap-0.5">
              {group.protocol && (
                <span className="text-amber font-mono text-[10px] uppercase tracking-wider">
                  {group.protocol}
                </span>
              )}
              {group.calls.map((change, i) => (
                <ScriptCallBadge key={i} change={change} />
              ))}
            </div>
          ))}
        </div>
      );
    })();
}
```

**Step 4: Run type-check and lint**

Run: `cd frontend && pnpm type-check && pnpm lint`
Expected: no errors.

**Step 5: Run frontend tests**

Run: `cd frontend && npx vitest run`
Expected: all pass.

**Step 6: Commit**

```
feat(frontend): protocol-aware script call display

- ActivityScriptCall gains protocolName field
- Homepage: shows protocol name as prefix (e.g., "Stable++ · Pool(...)")
- Address page: groups script calls by protocol with header label
```

---

## Task 6: Frontend tests

**Files:**

- Modify: `frontend/__tests__/components/latest-activities.test.tsx`
- Modify: `frontend/__tests__/lib/activity-classify.test.ts`

**Step 1: Add test for protocol name rendering in latest-activities**

Add a test that verifies when a scriptCall has `protocolName: "Stable++"`, the rendered output contains "Stable++" text.

**Step 2: Add test for groupScriptCallsByProtocol helper**

Test: two calls with same protocol → one group; mixed → two groups; no protocol → single ungrouped.

**Step 3: Run frontend tests**

Run: `cd frontend && npx vitest run`
Expected: all pass.

**Step 4: Commit**

```
test(frontend): add tests for protocol-aware script call display
```

---

## Task 7: Full verification

**Step 1: Run full pre-commit checks**

Run: `cargo check && cargo clippy && cd frontend && pnpm type-check && pnpm lint`
Expected: all clean.

**Step 2: Run all tests**

Run: `cargo test && cd frontend && npx vitest run`
Expected: all pass.

**Step 3: Final commit if any fixups needed**
