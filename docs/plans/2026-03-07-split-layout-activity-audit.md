# Split Layout Activity-Adjacent Audit Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Fix remaining split-layout helper misuse in activity-adjacent NFT live outpoint resolution without changing store boundaries.

**Architecture:** Keep canonical NFT/account state in the domain store and validate live outpoints through explicit payload-store cell reads. Activity append-only readers stay unchanged because their runtime callers already use the append-only store correctly.

**Tech Stack:** Rust, RocksDB, Axum, ckbadger-store dual-store layout

---

### Task 1: Lock the regression with tests

**Files:**

- Modify: `crates/ckbadger-store/src/dotbit_ops.rs`
- Modify: `crates/ckbadger-store/src/mnft_ops.rs`
- Modify: `crates/api/src/routes/assets.rs`

**Step 1: Write the failing test**

Add regression tests proving split domain/append-only layout fails when live outpoint helpers use unified `get_cell()`.

**Step 2: Run test to verify it fails**

Run:
`cargo test -p ckbadger-store live_.*outpoint -- --nocapture`

Expected:
failing split-layout tests or compile failure for missing split-aware API.

**Step 3: Write minimal implementation**

Add payload-store-aware helper variants and route callers through them.

**Step 4: Run test to verify it passes**

Run:
`cargo test -p ckbadger-store live_.*outpoint -- --nocapture`
`cargo test -p ckbadger-api nft_collection_items -- --nocapture`

Expected:
all new regression tests pass.

### Task 2: Verify no activity CF caller regression

**Files:**

- Modify: `crates/api/src/routes/assets.rs`

**Step 1: Add focused API regression**

Cover `.bit`/`mNFT` item listing/detail paths under split layout.

**Step 2: Run test to verify it fails**

Run:
`cargo test -p ckbadger-api nft_collection_items -- --nocapture`

**Step 3: Keep implementation minimal**

Only change callers that currently pass domain store into unified helpers.

**Step 4: Run test to verify it passes**

Run:
`cargo test -p ckbadger-api nft_collection_items -- --nocapture`
