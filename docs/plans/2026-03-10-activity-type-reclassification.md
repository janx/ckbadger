# Activity Type Reclassification Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Reclassify activity types so Transfer means pure CKB (no type scripts), ScriptCall captures unrecognized type scripts, and Unknown is an unconditional fallback.

**Architecture:** Add `AssetChange::ScriptCall` variant and `has_type_script: bool` to `ActivityEntry`. In the builder, unrecognized type scripts emit ScriptCall instead of being silently dropped. In statistics, Transfer is a positive match (no type scripts), ScriptCall counts activities with unrecognized scripts, and Unknown is a bare `else` branch catching anything that slips through.

**Tech Stack:** Rust (bincode serialization, RocksDB), TypeScript/React (frontend charts/API types)

**Re-sync required:** Yes — ActivityEntry serialization format changes.

---

### Task 1: Add `ScriptCall` variant to `AssetChange` and `has_type_script` to `ActivityEntry`

**Files:**

- Modify: `crates/ckbadger-store/src/types.rs:886-914` (AssetChange enum)
- Modify: `crates/ckbadger-store/src/types.rs:857-872` (ActivityEntry struct)
- Modify: `crates/ckbadger-store/src/types.rs:934-958` (DailyActivityStats struct)

**Step 1: Add `ScriptCall` variant to `AssetChange`**

In `crates/ckbadger-store/src/types.rs`, add after the `DaoWithdrawComplete` variant (line ~914):

```rust
    ScriptCall {
        type_code_hash: Vec<u8>,
    },
```

**Step 2: Add `has_type_script` field to `ActivityEntry`**

In `crates/ckbadger-store/src/types.rs`, add after the `is_cellbase` field (line ~868):

```rust
    /// Whether any cell for this owner in the transaction had a type script.
    #[serde(default)]
    pub has_type_script: bool,
```

The `#[serde(default)]` ensures backward compatibility during development (old serialized entries deserialize with `false`).

**Step 3: Add `script_call_count` and `unknown_count` to `DailyActivityStats`**

In `crates/ckbadger-store/src/types.rs`, add after `identity_count` (line ~949):

```rust
    /// Activities involving unrecognized type scripts
    #[serde(default)]
    pub script_call_count: u32,
    /// Fallback — should always be 0; non-zero indicates a classification bug
    #[serde(default)]
    pub unknown_count: u32,
```

**Step 4: Run `cargo check -p ckbadger-store`**

Expected: Compilation errors in downstream crates (exhaustive match on AssetChange, ActivityEntry construction). This is expected — we fix them in subsequent tasks.

**Step 5: Commit**

```bash
git add crates/ckbadger-store/src/types.rs
git commit -m "feat(store): add ScriptCall variant, has_type_script flag, and new stats fields"
```

---

### Task 2: Update serialization tests in `types.rs`

**Files:**

- Modify: `crates/ckbadger-store/src/types.rs:1241-1366` (ActivityEntry tests)

**Step 1: Write test for ScriptCall roundtrip**

Add to the `tests` module in `crates/ckbadger-store/src/types.rs`, after the `test_activity_entry_empty_roundtrip` test:

```rust
    #[test]
    fn test_activity_entry_script_call_roundtrip() {
        let entry = ActivityEntry {
            tx_hash: vec![0x10; 32],
            block_hash: vec![0xF0; 32],
            block_number: 500,
            tx_index: 1,
            timestamp: 1_700_000_000,
            ckb_delta: -100_00000000,
            occupied_delta: 0,
            is_cellbase: false,
            has_type_script: true,
            asset_changes: vec![AssetChange::ScriptCall {
                type_code_hash: vec![0xDD; 32],
            }],
            peers: vec![vec![0xEE; 32]],
        };
        let bytes = bincode::serialize(&entry).unwrap();
        let decoded: ActivityEntry = bincode::deserialize(&bytes).unwrap();
        assert!(decoded.has_type_script);
        assert_eq!(decoded.asset_changes.len(), 1);
        match &decoded.asset_changes[0] {
            AssetChange::ScriptCall { type_code_hash } => {
                assert_eq!(type_code_hash, &vec![0xDD; 32]);
            }
            _ => panic!("expected ScriptCall variant"),
        }
    }
```

**Step 2: Update existing test constructors to include `has_type_script`**

In every `ActivityEntry { ... }` literal in the `tests` module (lines ~1245, ~1281, ~1349), add `has_type_script: false,` after `is_cellbase`. There are 3 existing tests.

In `test_activity_entry_all_asset_change_variants` (line ~1281), also add the ScriptCall variant to the `asset_changes` vec and update the `assert_eq!(decoded.asset_changes.len(), 6)` to `7`.

**Step 3: Run `cargo test -p ckbadger-store -- activity_entry`**

Expected: All pass.

**Step 4: Commit**

```bash
git add crates/ckbadger-store/src/types.rs
git commit -m "test(store): update ActivityEntry serialization tests for ScriptCall and has_type_script"
```

---

### Task 3: Update activity builder to emit ScriptCall and set `has_type_script`

**Files:**

- Modify: `crates/indexer/src/db/writer/activities.rs:126-157` (OwnerAccum struct)
- Modify: `crates/indexer/src/db/writer/activities.rs:162-332` (build_tx_activities)
- Modify: `crates/indexer/src/db/writer/activities.rs:334-387` (classify_input, None arm)
- Modify: `crates/indexer/src/db/writer/activities.rs:389-453` (classify_output, None arm)

**Step 1: Add fields to `OwnerAccum`**

In `crates/indexer/src/db/writer/activities.rs`, add to `OwnerAccum` struct after `involved_scripts` (line ~156):

```rust
    /// Whether any cell for this owner has a type script
    has_type_script: bool,
    /// Unrecognized type script code_hashes
    unrecognized_scripts: HashSet<Vec<u8>>,
```

**Step 2: Set `has_type_script = true` when type_code_hash is present**

In `classify_input` (line ~345), add as the first line of the function body:

```rust
    accum.has_type_script = true;
```

In `classify_output` (line ~398), add as the first line of the function body:

```rust
    accum.has_type_script = true;
```

**Step 3: Change `None` arms to accumulate unrecognized scripts**

In `classify_input` (line ~385), change:

```rust
        None => {}
```

to:

```rust
        None => {
            accum.unrecognized_scripts.insert(type_code_hash.to_vec());
        }
```

In `classify_output` (line ~451), change:

```rust
        None => {}
```

to:

```rust
        None => {
            accum.unrecognized_scripts.insert(type_code_hash.to_vec());
        }
```

**Step 4: Emit ScriptCall asset changes and set `has_type_script` on entry**

In `build_tx_activities`, after the `emit_identity_changes` call for did:ckb (line ~312), add:

```rust
        // Unrecognized type script calls → ScriptCall
        for code_hash in &accum.unrecognized_scripts {
            asset_changes.push(AssetChange::ScriptCall {
                type_code_hash: code_hash.clone(),
            });
        }
```

In the `ActivityEntry` construction (line ~314), add the `has_type_script` field:

```rust
        let entry = ActivityEntry {
            // ... existing fields ...
            has_type_script: accum.has_type_script,
            // ...
        };
```

**Step 5: Run `cargo check -p ckbadger-indexer`**

Expected: May still have compile errors in other files (statistics.rs, API routes). Activity builder should be clean.

**Step 6: Commit**

```bash
git add crates/indexer/src/db/writer/activities.rs
git commit -m "feat(indexer): emit ScriptCall for unrecognized type scripts and set has_type_script"
```

---

### Task 4: Update activity builder tests

**Files:**

- Modify: `crates/indexer/src/db/writer/activities.rs:548-1101` (tests module)

**Step 1: Update all existing `ActivityEntry` assertions/constructors**

In `make_input` helper (line ~578), no change needed (no ActivityEntry constructed).

In existing tests, the returned `ActivityEntry` now has `has_type_script`. Update assertions where relevant:

- `test_simple_ckb_transfer`: add `assert!(!alice_act.has_type_script);`
- `test_cellbase_reward`: add `assert!(!entry.has_type_script);`
- `test_udt_token_transfer`: add `assert!(alice_act.has_type_script);`
- `test_dao_withdraw_complete_is_classified_from_input_view_flag`: add `assert!(entry.has_type_script);`

**Step 2: Write test for unrecognized type script producing ScriptCall**

Add to the tests module:

```rust
    #[test]
    fn test_unrecognized_type_script_produces_script_call() {
        let alice = 0xAA;
        let bob = 0xBB;
        let unknown_code_hash = vec![0xFF; 32];

        let mut alice_input = make_input(alice, 200_00000000, 61_00000000);
        alice_input.type_code_hash = Some(unknown_code_hash.clone());
        alice_input.type_script_hash = Some(vec![0xDD; 32]);

        let outputs = vec![make_output(
            bob,
            200_00000000,
            Some(unknown_code_hash.clone()),
            Some(vec![0xDD; 32]),
            Some(vec![0xEE; 20]),
            vec![],
        )];

        let tx = TxView {
            tx_hash: &[0x0A; 32],
            block_hash: &[0xAA; 32],
            tx_index: 1,
            block_number: 1000,
            timestamp: 1_700_000_000,
            is_cellbase: false,
            inputs: vec![alice_input],
            outputs: &outputs,
        };

        let activities = build_activities_for_block(&[tx], &HashMap::new());

        let alice_act = activities
            .iter()
            .find(|(lh, _, _)| lh == &vec![alice; 32])
            .map(|(_, _, e)| e)
            .unwrap();
        assert!(alice_act.has_type_script);
        let script_call = alice_act
            .asset_changes
            .iter()
            .find(|c| matches!(c, AssetChange::ScriptCall { .. }))
            .expect("should have ScriptCall for unrecognized type script");
        match script_call {
            AssetChange::ScriptCall { type_code_hash } => {
                assert_eq!(type_code_hash, &vec![0xFF; 32]);
            }
            _ => unreachable!(),
        }

        let bob_act = activities
            .iter()
            .find(|(lh, _, _)| lh == &vec![bob; 32])
            .map(|(_, _, e)| e)
            .unwrap();
        assert!(bob_act.has_type_script);
        assert!(bob_act
            .asset_changes
            .iter()
            .any(|c| matches!(c, AssetChange::ScriptCall { .. })));
    }

    #[test]
    fn test_pure_ckb_transfer_has_no_type_script() {
        let alice = 0xAA;
        let bob = 0xBB;

        let outputs = vec![
            make_output(bob, 100_00000000, None, None, None, vec![]),
            make_output(alice, 200_00000000, None, None, None, vec![]),
        ];

        let tx = TxView {
            tx_hash: &[0x0B; 32],
            block_hash: &[0xAB; 32],
            tx_index: 1,
            block_number: 1000,
            timestamp: 1_700_000_000,
            is_cellbase: false,
            inputs: vec![make_input(alice, 300_00000000, 61_00000000)],
            outputs: &outputs,
        };

        let activities = build_activities_for_block(&[tx], &HashMap::new());
        for (_, _, entry) in &activities {
            assert!(!entry.has_type_script);
            assert!(entry.asset_changes.is_empty());
        }
    }

    #[test]
    fn test_mixed_known_and_unknown_scripts_in_same_tx() {
        let alice = 0xAA;
        let sudt_code_hash = crate::rpc::parse_hex_to_bytes(crate::parser::udt::SUDT_CODE_HASH);
        let unknown_code_hash = vec![0xFF; 32];
        let type_script_hash = vec![0xDD; 32];

        let mut udt_input = make_input(alice, 200_00000000, 61_00000000);
        udt_input.type_code_hash = Some(sudt_code_hash.clone());
        udt_input.type_script_hash = Some(type_script_hash.clone());
        udt_input.data = 5000u128.to_le_bytes().to_vec();

        let outputs = vec![
            make_output(
                alice,
                100_00000000,
                Some(sudt_code_hash),
                Some(type_script_hash),
                Some(vec![0xEE; 20]),
                3000u128.to_le_bytes().to_vec(),
            ),
            make_output(
                alice,
                100_00000000,
                Some(unknown_code_hash.clone()),
                Some(vec![0xCC; 32]),
                Some(vec![0xEE; 20]),
                vec![],
            ),
        ];

        let tx = TxView {
            tx_hash: &[0x0C; 32],
            block_hash: &[0xAC; 32],
            tx_index: 1,
            block_number: 1000,
            timestamp: 1_700_000_000,
            is_cellbase: false,
            inputs: vec![udt_input],
            outputs: &outputs,
        };

        let activities = build_activities_for_block(&[tx], &HashMap::new());
        let (_, _, entry) = &activities[0];
        assert!(entry.has_type_script);
        assert!(entry.asset_changes.iter().any(|c| matches!(c, AssetChange::Token { .. })));
        assert!(entry.asset_changes.iter().any(|c| matches!(c, AssetChange::ScriptCall { .. })));
    }
```

**Step 3: Run `cargo test -p ckbadger-indexer -- activities::tests`**

Expected: All pass.

**Step 4: Commit**

```bash
git add crates/indexer/src/db/writer/activities.rs
git commit -m "test(indexer): add tests for ScriptCall, has_type_script, and mixed script scenarios"
```

---

### Task 5: Update statistics accumulation

**Files:**

- Modify: `crates/indexer/src/db/writer/statistics.rs:523-590` (accumulate_activity_stats)
- Modify: `crates/indexer/src/db/writer/statistics.rs:592-632` (update_daily_activity_stats merge)

**Step 1: Update `accumulate_activity_stats` classification logic**

In `crates/indexer/src/db/writer/statistics.rs`, replace lines ~546-589 with:

```rust
        // Check asset changes for specific types
        let mut has_dao = false;
        let mut has_token = false;
        let mut has_object = false;
        let mut has_identity = false;
        let mut has_script_call = false;

        for change in &entry.asset_changes {
            match change {
                AssetChange::DaoDeposit { .. } => {
                    stats.dao_deposit_count += 1;
                    has_dao = true;
                }
                AssetChange::DaoWithdrawRequest { .. } => {
                    stats.dao_withdraw_request_count += 1;
                    has_dao = true;
                }
                AssetChange::DaoWithdrawComplete { .. } => {
                    stats.dao_withdraw_complete_count += 1;
                    has_dao = true;
                }
                AssetChange::Token { .. } => {
                    has_token = true;
                }
                AssetChange::Object { .. } => {
                    has_object = true;
                }
                AssetChange::Identity { .. } => {
                    has_identity = true;
                }
                AssetChange::ScriptCall { .. } => {
                    has_script_call = true;
                }
            }
        }

        if has_token {
            stats.token_count += 1;
        }
        if has_object {
            stats.object_count += 1;
        }
        if has_identity {
            stats.identity_count += 1;
        }
        if has_script_call {
            stats.script_call_count += 1;
        }

        // Exclusive activity-level classification
        let matched = has_dao || has_token || has_object || has_identity || has_script_call;
        if matched {
            // Already counted in specific categories above
        } else if !entry.has_type_script {
            stats.transfer_count += 1; // Pure CKB transfer: positive match
        } else {
            stats.unknown_count += 1; // Fallback: no conditions, just else
        }
```

**Step 2: Update `update_daily_activity_stats` merge to include new fields**

In the merge block (line ~603-619), add after `e.identity_count += ...`:

```rust
                e.script_call_count += accumulated.script_call_count;
                e.unknown_count += accumulated.unknown_count;
```

**Step 3: Run `cargo check -p ckbadger-indexer`**

Expected: Compile errors in test helpers that construct `ActivityEntry` without `has_type_script`. Fix in next step.

**Step 4: Commit**

```bash
git add crates/indexer/src/db/writer/statistics.rs
git commit -m "feat(indexer): reclassify Transfer as pure CKB, add ScriptCall and Unknown stats"
```

---

### Task 6: Update statistics tests

**Files:**

- Modify: `crates/indexer/src/db/writer/statistics.rs:1514-1707` (activity_stats_tests module)

**Step 1: Update `make_entry` helper**

In `crates/indexer/src/db/writer/statistics.rs`, change the `make_entry` function (line ~1522) to accept `has_type_script`:

```rust
    fn make_entry(
        ckb_delta: i128,
        is_cellbase: bool,
        has_type_script: bool,
        changes: Vec<AssetChange>,
    ) -> ActivityEntry {
        ActivityEntry {
            tx_hash: vec![0; 32],
            block_hash: vec![0; 32],
            block_number: 100,
            tx_index: 0,
            timestamp: 1700000000000,
            ckb_delta,
            occupied_delta: 0,
            is_cellbase,
            has_type_script,
            asset_changes: changes,
            peers: vec![],
        }
    }
```

**Step 2: Update all existing test call sites**

Every `make_entry(...)` call needs the new `has_type_script` parameter inserted after `is_cellbase`:

- `test_coinbase_classified_correctly`: `make_entry(500_00000000, true, false, vec![])` — coinbase has no type scripts
- `test_plain_transfer_classified_correctly`: `make_entry(-100_00000000, false, false, vec![])` — pure CKB, no type scripts
- `test_dao_deposit_classified_correctly`: `make_entry(0, false, true, vec![AssetChange::DaoDeposit { ... }])` — DAO is a type script
- `test_dao_withdraw_request_classified_correctly`: same pattern, `has_type_script: true`
- `test_dao_withdraw_complete_classified_correctly`: same pattern, `has_type_script: true`
- `test_token_transfer_classified_correctly`: `has_type_script: true`
- `test_object_classified_correctly`: `has_type_script: true`
- `test_identity_classified_correctly`: `has_type_script: true`
- `test_mixed_asset_changes_classified_correctly`: `has_type_script: true`
- `test_multiple_activities_accumulate`: transfers get `false`, coinbase gets `false`
- `test_negative_delta_uses_absolute_value`: `has_type_script: false`
- `test_script_counts_accumulated`: first entry `true` (has DAO), second `false` (plain transfer)

**Step 3: Add new tests for ScriptCall and Unknown classification**

```rust
    #[test]
    fn test_script_call_classified_correctly() {
        let mut stats = DailyActivityStats::default();
        let scripts = vec![vec![0xFF; 32]];
        let entry = make_entry(
            -50_00000000,
            false,
            true,
            vec![AssetChange::ScriptCall {
                type_code_hash: vec![0xFF; 32],
            }],
        );
        BatchWriter::accumulate_activity_stats(&entry, &scripts, &mut stats);
        assert_eq!(stats.script_call_count, 1);
        assert_eq!(stats.transfer_count, 0);
        assert_eq!(stats.unknown_count, 0);
    }

    #[test]
    fn test_unknown_is_unconditional_fallback() {
        let mut stats = DailyActivityStats::default();
        let scripts = vec![vec![0x11; 32]];
        // has_type_script=true but no asset changes — this is the Unknown case
        let entry = make_entry(0, false, true, vec![]);
        BatchWriter::accumulate_activity_stats(&entry, &scripts, &mut stats);
        assert_eq!(stats.unknown_count, 1);
        assert_eq!(stats.transfer_count, 0);
        assert_eq!(stats.script_call_count, 0);
    }

    #[test]
    fn test_transfer_requires_no_type_script() {
        let mut stats = DailyActivityStats::default();
        let scripts = vec![vec![0x11; 32]];
        // Pure CKB: no type scripts, no asset changes
        let entry = make_entry(-100_00000000, false, false, vec![]);
        BatchWriter::accumulate_activity_stats(&entry, &scripts, &mut stats);
        assert_eq!(stats.transfer_count, 1);
        assert_eq!(stats.unknown_count, 0);
        assert_eq!(stats.script_call_count, 0);
    }
```

**Step 4: Run `cargo test -p ckbadger-indexer -- activity_stats_tests`**

Expected: All pass.

**Step 5: Commit**

```bash
git add crates/indexer/src/db/writer/statistics.rs
git commit -m "test(indexer): update and add stats tests for Transfer/ScriptCall/Unknown classification"
```

---

### Task 7: Update activity filter in store

**Files:**

- Modify: `crates/ckbadger-store/src/activity_ops.rs:109-131` (matches_activity_filter)

**Step 1: Update `matches_activity_filter` for new types**

The `"ckb"` filter currently matches `asset_changes.is_empty()`. This needs updating to also consider `has_type_script`:

```rust
    fn matches_activity_filter(entry: &ActivityEntry, filter: Option<&str>) -> bool {
        match filter {
            None | Some("all") => true,
            Some("ckb") => entry.asset_changes.is_empty() && !entry.has_type_script,
            Some("token") => entry
                .asset_changes
                .iter()
                .any(|c| matches!(c, AssetChange::Token { .. })),
            Some("object") | Some("nft") => entry
                .asset_changes
                .iter()
                .any(|c| matches!(c, AssetChange::Object { .. })),
            Some("dao") => entry.asset_changes.iter().any(|c| {
                matches!(
                    c,
                    AssetChange::DaoDeposit { .. }
                        | AssetChange::DaoWithdrawRequest { .. }
                        | AssetChange::DaoWithdrawComplete { .. }
                )
            }),
            Some("script_call") => entry
                .asset_changes
                .iter()
                .any(|c| matches!(c, AssetChange::ScriptCall { .. })),
            Some(_) => false,
        }
    }
```

**Step 2: Update `validate_activity_filter` in API**

In `crates/api/src/routes/activities.rs:179-187`, add `"script_call"` to valid filters:

```rust
fn validate_activity_filter(filter: Option<&str>) -> Result<(), ApiRouteError> {
    if let Some(value) = filter {
        if !matches!(value, "all" | "ckb" | "token" | "nft" | "dao" | "script_call") {
            return Err(ApiError::bad_request(format!(
                "invalid activity filter '{}'; expected one of: all, ckb, token, nft, dao, script_call",
                value
            )));
        }
    }
    Ok(())
}
```

**Step 3: Update test helpers in `activity_ops.rs` tests**

In `crates/ckbadger-store/src/activity_ops.rs`, update `make_activity_with_hash` (line ~140) and `make_activity` to include `has_type_script: false`.

**Step 4: Run `cargo test -p ckbadger-store -- activity_ops`**

Expected: All pass.

**Step 5: Commit**

```bash
git add crates/ckbadger-store/src/activity_ops.rs crates/api/src/routes/activities.rs
git commit -m "feat(store,api): update activity filter for ckb purity check and add script_call filter"
```

---

### Task 8: Update API response types for ScriptCall

**Files:**

- Modify: `crates/api/src/routes/activities.rs:52-86` (AssetChangeResponse enum)
- Modify: `crates/api/src/routes/activities.rs:124-173` (convert_asset_change function)

**Step 1: Add ScriptCall to `AssetChangeResponse`**

In `crates/api/src/routes/activities.rs`, add to the `AssetChangeResponse` enum after `DaoWithdrawComplete`:

```rust
    #[serde(rename = "scriptCall", rename_all = "camelCase")]
    ScriptCall {
        type_code_hash: String,
    },
```

**Step 2: Add ScriptCall arm to `convert_asset_change`**

In `crates/api/src/routes/activities.rs`, add after the `DaoWithdrawComplete` arm:

```rust
        AssetChange::ScriptCall { type_code_hash } => AssetChangeResponse::ScriptCall {
            type_code_hash: format!("0x{}", hex::encode(type_code_hash)),
        },
```

**Step 3: Run `cargo check -p ckbadger-api`**

Expected: Pass. All AssetChange match arms are now exhaustive.

**Step 4: Commit**

```bash
git add crates/api/src/routes/activities.rs
git commit -m "feat(api): add scriptCall to activity response serialization"
```

---

### Task 9: Update API daily activity stats response

**Files:**

- Modify: `crates/api/src/routes/statistics.rs:3128-3141` (DailyActivityStatsResponse)
- Modify: `crates/api/src/routes/statistics.rs:3231-3244` (response mapping)

**Step 1: Add new fields to `DailyActivityStatsResponse`**

In `crates/api/src/routes/statistics.rs`, add after `identity_count`:

```rust
    pub script_call_count: u32,
    pub unknown_count: u32,
```

**Step 2: Update the response mapping**

In the `DailyActivityStatsResponse { ... }` construction (line ~3231), add:

```rust
                script_call_count: s.script_call_count,
                unknown_count: s.unknown_count,
```

**Step 3: Run `cargo check -p ckbadger-api`**

Expected: Pass.

**Step 4: Commit**

```bash
git add crates/api/src/routes/statistics.rs
git commit -m "feat(api): expose scriptCallCount and unknownCount in daily activity stats"
```

---

### Task 10: Fix remaining compile errors across crates

**Files:**

- Search all crates for `ActivityEntry {` construction sites that need `has_type_script`

**Step 1: Find all `ActivityEntry` construction sites**

Run: `cargo check 2>&1 | head -60` to find remaining errors.

Common locations that will need `has_type_script: false` or appropriate value:

- `crates/api/src/routes/activities.rs:429` (synthetic entry for latest activities)
- Any test helpers across crates
- `crates/indexer/tests/reorg_handling.rs` if it constructs ActivityEntry

**Step 2: Fix each site by adding `has_type_script` field**

For test/synthetic entries that represent pure CKB transfers, use `has_type_script: false`.
For entries with asset changes involving type scripts, use `has_type_script: true`.

**Step 3: Run `cargo check`**

Expected: Full project compiles cleanly.

**Step 4: Run `cargo test --lib`**

Expected: All unit tests pass.

**Step 5: Commit**

```bash
git add -A
git commit -m "fix: add has_type_script to all remaining ActivityEntry construction sites"
```

---

### Task 11: Update frontend types and charts

**Files:**

- Modify: `frontend/lib/api.ts:463-476` (DailyActivityStats interface)
- Modify: `frontend/components/activity-breakdown.tsx:17-38` (chart data builder)
- Modify: `frontend/lib/api.ts:2076-2118` (chart API methods)

**Step 1: Add new fields to `DailyActivityStats` TypeScript interface**

In `frontend/lib/api.ts`, add after `identityCount`:

```typescript
scriptCallCount: number;
unknownCount: number;
```

**Step 2: Update `buildChartData` in `activity-breakdown.tsx`**

Add ScriptCall to the chart data array (line ~28):

```typescript
    { label: 'Script Call', value: stats.scriptCallCount, color: ACTIVITY_COLORS['Script Call'] },
```

Add the color to `ACTIVITY_COLORS`:

```typescript
  'Script Call': '#f97316',
```

**Step 3: Update `getActivityVolumeChart` total**

In `frontend/lib/api.ts`, add `s.scriptCallCount` to the total sum (line ~2082):

```typescript
        value: String(
          s.transferCount +
            s.daoDepositCount +
            s.daoWithdrawRequestCount +
            s.daoWithdrawCompleteCount +
            s.tokenCount +
            s.objectCount +
            s.identityCount +
            s.scriptCallCount
        ),
```

**Step 4: Update `getActivityTypeBreakdownChart` values**

In `frontend/lib/api.ts`, add to the values object (line ~2101):

```typescript
          scriptCall: String(s.scriptCallCount),
```

**Step 5: Run `cd frontend && pnpm type-check && pnpm lint`**

Expected: Pass.

**Step 6: Commit**

```bash
git add frontend/lib/api.ts frontend/components/activity-breakdown.tsx
git commit -m "feat(frontend): add scriptCallCount and unknownCount to activity stats and charts"
```

---

### Task 12: Update frontend activity filter (if applicable)

**Files:**

- Modify: `frontend/app/address/[addr]/client-page.tsx` (activity filter UI)

**Step 1: Check if the address page has a filter dropdown**

Read the file to see if there's a filter select with options like "ckb", "token", "nft", "dao".

**Step 2: Add "script_call" option if filter UI exists**

Add `script_call` as a filter option with label "Script Call".

**Step 3: Run `cd frontend && pnpm type-check`**

Expected: Pass.

**Step 4: Commit**

```bash
git add frontend/app/address/[addr]/client-page.tsx
git commit -m "feat(frontend): add script_call filter option to address activity view"
```

---

### Task 13: Final verification

**Step 1: Run full Rust test suite**

Run: `cargo test`

Expected: All tests pass.

**Step 2: Run frontend checks**

Run: `cd frontend && pnpm type-check && pnpm lint`

Expected: Pass.

**Step 3: Run clippy**

Run: `cargo clippy`

Expected: No new warnings.

**Step 4: Commit any remaining fixes**

If any fixes were needed, commit them.
