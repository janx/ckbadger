# ckbadger Postmortem - Lessons Learned

Historical fixes and their root causes. Reference this before implementing similar features.

---

## Category: NervosDAO

### DAO-001: Wrong DAO code_hash (0eeae17)

**Symptom**: No DAO deposits detected despite transactions existing on-chain.

**Root Cause**: Used incorrect mainnet DAO code_hash.

- Wrong: `0x82d76d1b...d56a12f184e06979`
- Correct: `0x82d76d1b...d3f81cf3e7e13f2e`

**Lesson**: Always verify hardcoded hashes against on-chain data (e.g., first known DAO tx `0x1fdfec93...`). Reference `docs/rfcs/rfcs/0024-ckb-genesis-script-list` for official genesis script hashes.

**Files**: `crates/indexer/src/parser/dao.rs`

---

### DAO-002: DAO cell detection using wrong hash type (bc85c0e)

**Symptom**: DAO cells not detected even with correct code_hash.

**Root Cause**: Was computing full script hash from type_script, then comparing to DAO code_hash. Should compare type_script's `code_hash` field directly.

```rust
// WRONG: Computing script hash
let type_hash = ScriptParser::compute_script_hash(type_script);
return Self::is_dao_type_script(&type_hash);

// CORRECT: Compare code_hash directly
let code_hash = parse_hex_to_bytes(&type_script.code_hash);
return Self::is_dao_code_hash(&code_hash);
```

**Lesson**: CKB script identification: `code_hash` identifies the script code, `script_hash` is the unique identity of a specific script instance (code_hash + hash_type + args). For detecting script types, compare `code_hash`, not `script_hash`.

**Reference**: `docs/rfcs/rfcs/0022-transaction-structure` - Script structure.

**Files**: `crates/indexer/src/parser/dao.rs`

---

### DAO-003: Withdrawal completion lookup by wrong field (0535f99)

**Symptom**: DAO withdrawals not being recorded as completed.

**Root Cause**: `complete_dao_withdrawal` was looking up deposits by the withdraw request cell's `tx_hash`, but `dao_deposits` tracks by original deposit `tx_hash`. Need to lookup by `withdraw_request_tx` column.

**The DAO withdrawal process**:

1. Deposit TX creates DAO cell → record in `dao_deposits(tx_hash=deposit_tx)`
2. Withdraw Request TX consumes deposit, creates request cell → update `dao_deposits.withdraw_request_tx`
3. Withdraw Completion TX consumes request cell → lookup by `withdraw_request_tx`, not by request cell's tx_hash

**Lesson**: Trace the full lifecycle of multi-phase protocols before implementing. DAO has 3 phases - each references the previous differently.

**Reference**: `docs/rfcs/rfcs/0023-dao-deposit-withdraw` - DAO lifecycle.

**Files**: `crates/indexer/src/db/writer.rs`

---

### DAO-004: Compensation calculation not following RFC-0023 (af92ac8)

**Symptom**: Incorrect compensation amounts displayed.

**Root Cause**: Multiple errors in compensation calculation:

1. Not extracting AR (accumulated rate) from block DAO field correctly (bytes 8-16, little-endian u64)
2. Using total capacity instead of free capacity (capacity - 102 CKB occupied)
3. Not fetching withdraw_request_block to get correct AR for phase 2

**Correct formula** (per RFC-0023):

```
free_capacity = capacity - 102_00000000 (occupied bytes)
compensation = free_capacity * (AR_withdraw / AR_deposit) - free_capacity
```

**DAO field structure** (32 bytes):

- Bytes 0-7: Total issuance (u64 LE)
- Bytes 8-15: Accumulated Rate (u64 LE) ← used for compensation
- Bytes 16-23: Secondary issuance accumulator
- Bytes 24-31: Occupied capacity accumulator

**Lesson**: Read the RFC thoroughly. CKB uses non-obvious encodings (LE u64 in fixed byte ranges). The 102 CKB occupied capacity is a protocol constant.

**Reference**: `docs/rfcs/rfcs/0023-dao-deposit-withdraw` - Section on compensation calculation.

**Files**: `crates/indexer/src/db/writer.rs`, `crates/api/src/routes/dao.rs`

---

### DAO-005: Circulating supply not populated (7b6ce8c)

**Symptom**: Deposit-to-circulation ratio chart showing zero.

**Root Cause**: `dao_daily_snapshots.total_issuance` was never populated. Need to extract total issuance from block DAO field (first 8 bytes, LE u64).

**Lesson**: When adding derived statistics, trace data lineage back to source. `total_issuance` requires parsing the DAO field from blocks.

**Files**: `crates/indexer/src/db/writer.rs`

---

### DAO-006: APC using wrong formula (716e355)

**Symptom**: DAO page showed ~9% APC instead of correct ~4.86%.

**Root Cause**: Was calculating APC from deposit/compensation ratio instead of AR (accumulated rate) growth.

```rust
// WRONG: Using deposit-based calculation
let apc = (total_compensation / total_deposit) * 100;

// CORRECT: Using AR growth rate
let ar_ratio = ar_current as f64 / ar_one_year_ago as f64;
let apc = (ar_ratio.powf(1.0 / years) - 1.0) * 100.0;
```

**Lesson**: APC measures the DAO's compensation rate over time. It's derived from how fast AR grows, not from summing deposits/compensations (which is affected by deposit timing and duration).

**Reference**: `docs/DAO_CALCULATIONS.md` - comprehensive calculation documentation added.

**Files**: `crates/api/src/routes/dao.rs`, `crates/indexer/src/db/writer.rs`

---

### DAO-007: Secondary issuance counting all deposits (5ce76af)

**Symptom**: Secondary issuance pie chart showed burnt at ~35% instead of correct ~65-70%.

**Root Cause**: The `total_deposit` query counted ALL deposits ever made, not just active deposits at each date. Withdrawn deposits were still being counted.

```sql
-- WRONG: Counts all deposits including withdrawn
SELECT SUM(capacity) FROM dao_deposits WHERE deposit_timestamp::date <= date

-- CORRECT: Only count active deposits (not yet withdrawn)
SELECT SUM(capacity) FROM dao_deposits
WHERE deposit_timestamp::date <= date
  AND (withdraw_timestamp IS NULL OR withdraw_timestamp::date > date)
```

**Lesson**: Point-in-time queries for cumulative statistics must filter out records that have been "removed" (withdrawn) by that point.

**Files**: `crates/indexer/src/db/writer.rs`

---

### DAO-008: Cell status check inverted in migration (829caf6)

**Symptom**: DAO withdrawal backfill migration didn't find expected cells.

**Root Cause**: Migration 009 used `c.status = 0` (live) but should have checked for consumed cells. In this schema, `status=1` means consumed.

**Lesson**: Schema semantics can change across versions. Always verify column value meanings before writing migrations, especially when backfilling data.

**Files**: `crates/indexer/src/db/writer.rs` (migration consolidated into `001_init.sql`)

---

### DAO-009: Phase 2 withdrawal not detected (e531c08)

**Symptom**: DAO withdrawal completions (Phase 2) not being recorded.

**Root Cause**: `find_consumed_dao_deposits` was only matching by `tx_hash` (original deposit). Phase 2 consumes the **withdraw request cell**, not the original deposit. Need to also match `withdraw_request_tx` for `status=1` records.

```sql
-- WRONG: Only matches original deposits
WHERE tx_hash = $1 AND output_index = $2

-- CORRECT: Match both Phase 1 and Phase 2
WHERE (tx_hash = $1 AND output_index = $2)
   OR (withdraw_request_tx = $1 AND status = 1)
```

**DAO withdrawal lifecycle reminder**:

1. Phase 1 (Withdraw Request): Consumes original deposit cell → match by `tx_hash`
2. Phase 2 (Withdrawal Completion): Consumes withdraw request cell → match by `withdraw_request_tx`

**Lesson**: Multi-phase protocols require matching against different fields depending on which phase is being processed. The consumed cell in Phase 2 is the output of Phase 1, not the original input.

**Reference**: `docs/rfcs/rfcs/0023-dao-deposit-withdraw`

**Files**: `crates/indexer/src/db/writer.rs`

---

### DAO-010: APC calculated from AR growth instead of issuance formula (8d42894)

**Symptom**: Estimated APC showing ~2.9% instead of correct ~4.8%.

**Root Cause**: Was calculating APC from AR growth rate between blocks (which measures actual depositor returns). The correct "Estimated APC" should use the theoretical formula based on secondary issuance rate.

```rust
// WRONG: AR-based calculation (measures historical returns)
let ar_ratio = ar_current as f64 / ar_one_year_ago as f64;
let apc = (ar_ratio.powf(1.0 / years) - 1.0) * 100.0;

// CORRECT: Issuance-based formula (theoretical max rate)
const SECONDARY_ISSUANCE_PER_YEAR: u64 = 1_344_000_000_00000000;
let apc = (SECONDARY_ISSUANCE_PER_YEAR as f64 / total_issuance as f64) * 100.0;
```

**Key distinction**:

- **Estimated APC** = theoretical max if 100% deposited = `secondary_issuance / total_issuance`
- **Nominal APC** = historical based on AR growth = actual returns depositors received

**Lesson**: "APC" can mean different things. Clarify which metric is being displayed and use the correct formula. See `docs/DAO_CALCULATIONS.md` for full specification.

**Files**: `crates/indexer/src/db/writer.rs`, `crates/api/src/routes/dao.rs`

---

### DAO-011: Circulation ratio using total_issuance instead of real circulating (c14b29b)

**Symptom**: Deposit-to-circulation ratio showing ~17% instead of correct ~22%.

**Root Cause**: Was dividing deposits by `total_issuance` (33.6B+ at genesis). Should divide by actual `circulating` supply which excludes burnt CKB.

```rust
// WRONG: Uses total_issuance (includes burnt)
let ratio = deposit as f64 / total_issuance as f64;

// CORRECT: Subtract all burnt CKB
const GENESIS_BURNT: u128 = 8_400_000_000_00000000;
let total_burnt = GENESIS_BURNT + secondary_burnt;
let circulating = total_issuance.saturating_sub(total_burnt);
let ratio = deposit as f64 / circulating as f64;
```

**CKB supply model**:

- `total_issuance` (dao field C) = 33.6B genesis + all issuance since
- `genesis_burnt` = 8.4B (never circulated, but counted in total_issuance)
- `secondary_burnt` = cumulative burnt secondary issuance
- `circulating` = total_issuance - genesis_burnt - secondary_burnt

**Lesson**: Always subtract burnt CKB when calculating circulation-based ratios. The 8.4B genesis burnt is "issued but never circulated". See `docs/DAO_CALCULATIONS.md` for supply model.

**Reference**: `docs/DAO_CALCULATIONS.md` - CKB Supply Model section

**Files**: `crates/api/src/routes/dao.rs`, `crates/api/src/warmup.rs`

---

### DAO-012: Total Supply chart starting at 33.6B instead of 25.2B (8d42894)

**Symptom**: Total Supply chart showed 33.6B at genesis, misleading users.

**Root Cause**: Chart displayed `total_issuance` directly. Genesis `total_issuance` is 33.6B, but 8.4B was immediately burnt and never entered circulation.

**Correct display**:

- Genesis circulating = 25.2B (not 33.6B)
- Chart should show `total_issuance - genesis_burnt - secondary_burnt`

**Lesson**: For user-facing "supply" charts, always show circulating supply (minus burnt), not raw `total_issuance`. Users expect to see tokens that actually exist in circulation.

**Files**: `crates/api/src/warmup.rs`, `crates/api/src/routes/statistics.rs`

---

### DAO-013: Circulation ratio chart showing wrong values during batch sync

**Symptom**: Deposit-to-circulation ratio chart showing suspicious/incorrect historical values.

**Root Cause**: `update_dao_daily_snapshot` used a tip-level cumulative source for all dates, so historical days inherited the same value. During batch sync, this made every past snapshot read as if it were "today".

**Correct approach**:

1. Prefer previous day's snapshot as the cumulative base (historical continuity).
2. Only fall back to current aggregate source when no previous snapshot exists.

**Lesson**: When creating historical snapshots during batch sync, cumulative values must be derived from previous snapshots, not from the current aggregate table. The aggregate table reflects the tip, not historical state.

**Reference**: Similar to STATS-001 (cumulative values wrong for new days).

**Files**: `crates/indexer/src/db/writer.rs`, `crates/indexer/src/db/writer_v2.rs`

---

### DAO-014: Secondary issuance burnt percentage regression (fbda36a)

**Symptom**: Secondary issuance chart showing abnormally low burnt percentage (~35% instead of ~65-70%).

**Root Cause**: Commit `5ce76af` correctly fixed the issue with a simple time-window predicate:

- Include deposits created on or before the target day.
- Exclude deposits already withdrawn on or before that day.

But 8 minutes later, commit `fbda36a` replaced it with complex status-branch logic.

The complex logic is flawed because:

1. `status = 0` includes deposits unconditionally, ignoring their actual withdrawal state
2. `status = 1 AND withdraw_request_timestamp IS NULL` is a contradictory condition that shouldn't exist but may match edge cases
3. The logic relies on `status` field correctness, which is a separate concern

**Lesson**:

1. Don't "improve" a working fix without understanding why it worked
2. Simpler logic is often more correct - the withdrawal state is fully determined by `withdraw_timestamp`
3. Avoid adding dependencies on field correctness (status) when a simpler field (timestamp) suffices
4. Watch for commits that immediately follow fixes - they may inadvertently revert them

**Files**: `crates/indexer/src/db/writer.rs`

---

### DAO-015: Secondary issuance dao_compensation calculated with wrong deposit value

**Symptom**: Secondary issuance chart showed burnt at ~64% instead of correct ~74%.

**Root Cause**: The calculation used **current** `total_dao_deposits` for all historical blocks, instead of the deposit amount **at that block's time**.

This caused early blocks to use inflated deposit values (~22% of total issuance) instead of the actual lower values at that time (~5-10%), resulting in overestimated `dao_compensation` and underestimated `burnt`.

**Failed fix attempt**: Tried using DAO field `S` (secondary_pool) difference between blocks. But `S` is "total unissued secondary issuance" which equals `non_miner - claimed_compensation`, not `dao_compensation`. This made burnt nearly 0%.

**Correct fix**: Use deposits active at that specific block height:

- Include deposits created no later than the target block.
- Exclude deposits consumed before or at that target boundary.
- Then apply RFC-0015: `dao_compensation = non_miner * deposit / (C - U)`.

**Key insight**:

- `C - U` equals `deposit + liquid` (total capacity minus occupied)
- `dao_compensation / non_miner = deposit / (C - U)`
- `burnt / non_miner = liquid / (C - U)`

**Lesson**:

1. Point-in-time calculations need point-in-time data, not current state
2. DAO field `S` is NOT the same as `dao_compensation` - read RFC-0023 carefully
3. Always verify calculations against known correct values (e.g., explorer.nervos.org)

**Reference**: RFC-0015 (CKB Cryptoeconomics), RFC-0023 (DAO Deposit/Withdraw)

**Files**: `crates/indexer/src/sync/indexer.rs`, `crates/indexer/src/db/writer.rs`

---

### DAO-016: Secondary issuance off-by-one in deposit query

**Symptom**: Burnt percentage showing ~48% instead of correct ~72%. DAO compensation inflated.

**Root Cause**: `get_dao_deposits_at_block` had two off-by-one errors in the active-deposit boundary conditions.

Per RFC-0023, block N's secondary issuance distribution uses `U_{i-1}` and `C_{i-1}` (previous block's state). So for calculating block N's distribution, we need deposits active at end of block N-1.

Use state at end of `N-1`:

- Deposit side: use `< N` instead of `<= N` (exclude deposits created at block `N`).
- Withdraw side: use `>= N` instead of `> N` (include withdrawals that are still active at end of `N-1`).

**Impact**:

- `deposit_block_number <= N` includes deposits made at block N that weren't yet active at end of N-1 → inflates `dao_deposits`
- `withdraw_block > N` excludes deposits withdrawn at block N that WERE still active at end of N-1 → deflates `dao_deposits`
- Net effect: During deposit growth, more deposits than withdrawals → dao_deposits inflated → burnt percentage too low

**Lesson**: When protocol calculations use "previous block state", queries must use `<` not `<=` for creation events, and `>=` not `>` for consumption events.

**Reference**: RFC-0023 line 124: `S_i = S_{i-1} - I_i + s_i - floor(s_i * U_{i-1} / C_{i-1})`

**Files**: `crates/indexer/src/db/writer.rs`

---

### DAO-017: Withdrawing cells counted as new deposits

**Symptom**: Burnt percentage showing ~57% instead of ~72%. DAO compensation approximately 2x higher than expected.

**Root Cause**: `parse_deposits_from_cells` only checked `type_code_hash == DAO && data_size == 8`, but did NOT verify that `data == [0; 8]` (deposit) vs non-zero (withdrawing cell).

Per RFC-0023:

- Deposit cell: data = 8 bytes of zeros
- Withdrawing cell: data = 8 bytes containing deposit block number (little-endian u64)

```rust
// WRONG: Both deposit and withdrawing cells pass this check
if type_code_hash == DAO && cell.data_size == 8 { ... }

// CORRECT: Only deposit cells (data is all zeros)
if type_code_hash == DAO && cell.data_size == 8 && cell.data == [0u8; 8] { ... }
```

**Impact**: Each deposit was counted TWICE in `dao_deposits`:

1. When deposit cell created (correct)
2. When withdrawing cell created in Phase 1 (incorrect - this is the same deposit!)

This inflated `dao_deposits` by ~2x, causing DAO compensation to be ~2x higher and burnt percentage to be correspondingly lower.

**Note**: The correct function `parse_deposits` (using TransactionView) DID check `dao_cell.state != DaoState::Deposit`, but the batch sync used `parse_deposits_from_cells` which lacked this check.

**Files**: `crates/indexer/src/parser/dao.rs`

---

### DAO-018: Secondary issuance overcounting from CKB S-field protocol upgrade drops

**Symptom**: Cumulative secondary issuance values (mining_reward, deposit_compensation, treasury_amount) exceeded the on-chain S field by ~4.7% (~321M CKB). Verify checks X13-X15 failed against CKB Explorer.

**Root Cause**: The CKB DAO header's S field (cumulative non-miner secondary issuance) **physically decreases** at protocol upgrade boundaries. This was observed at 6+ points in the chain's history:

| Date       | Approximate S Drop |
| ---------- | ------------------ |
| 2020-07-01 | ~1.5M CKB          |
| 2021-07-29 | ~1.2M CKB          |
| 2021-09-20 | ~0.9M CKB          |
| 2021-12-03 | ~32M CKB           |
| 2022-10-17 | ~62M CKB           |
| 2024-10-17 | ~5.4M CKB          |

The code computed `s_delta = (S_today - S_prev).max(0)`, which:

1. Clamped the negative delta to 0 (discarded the decrease)
2. **Still updated `prev_secondary_pool` to the lower S value**
3. In the next batch, `s_delta = S_next - S_low` was inflated by the amount of the drop

This was particularly destructive at **batch boundaries**: if a batch ended mid-day after an S drop, the partial-day snapshot stored the low S value. The next batch then computed an inflated s_delta from that low base, permanently overcounting the cumulative.

```rust
// WRONG: discards S drop but still updates prev to the lower value
let s_delta = (secondary_pool - prev_secondary_pool).max(0);
// ... compute split ...
prev_secondary_pool = secondary_pool; // saves the too-low S!

// CORRECT: allow negative s_delta, absorb into treasury
let s_delta = secondary_pool - prev_secondary_pool;
if s_delta >= 0 {
    // normal split
} else {
    // protocol upgrade: absorb negative into treasury
    (0, 0, s_delta)
}
prev_secondary_pool = secondary_pool;
```

**Lesson**: CKB protocol upgrades can retroactively recalculate the DAO S field, causing it to decrease. Never assume on-chain cumulative fields are monotonically increasing. When computing deltas from cumulative values, handle negative deltas explicitly rather than clamping to zero.

**Files**:

- `crates/indexer/src/sync/indexer.rs` — inline snapshot computation (2 paths)
- `crates/ckbadger-store/src/stats_ops.rs` — rebuild path

**Tests**: `stats_ops::tests::test_dao_snapshot_negative_s_delta_protocol_upgrade`, `stats_ops::tests::test_dao_snapshot_negative_s_delta_batch_boundary`

---

### DAO-019: total_deposit and depositors_count 10% too high vs explorer

**Symptom**: Verify checks nervos_dao_total_deposit (+10.4%), nervos_dao_depositors_count (+8.3%), nervos_dao_average_deposit_time (+9.7%), nervos_dao_treasury_amount (-1.25%) all failing.

**Root Cause**: CKB explorer subtracts deposits from `total_deposit` at phase-1 withdrawal (when deposit cell is consumed by withdraw request), not at phase-2 completion. Our code counted both status=0 (active deposit) and status=1 (withdraw request pending) in `total_deposited` and `total_depositors`. The ~81T shannons difference was pending withdrawal capacity.

Confirmed from explorer source: `process_withdraw_dao_events!` does `total_deposit -= withdraw_amount` at phase-1. The `depositor` scope is `DaoEvent.where(event_type: "deposit_to_dao", consumed_transaction_id: nil)` — only unconsumed deposits.

**Fix**: Changed `refresh_latest_dao_statistics` and API accumulator to only count status=0 for `total_deposited`, `total_depositors`, `average_deposit_days`, and `unclaimed_compensation`. Status=1 reported separately as `pending_withdrawal_capacity`.

**Lesson**: When comparing against an external reference, verify their exact definition — "deposited in DAO" can mean "unconsumed deposit cells only" (explorer) or "all locked CKB including pending withdrawals" (protocol-level). Read the reference implementation's source code.

**Files**: `crates/indexer/src/db/writer/statistics.rs`, `crates/api/src/routes/dao.rs`

---

### DAO-020: cumulative depositors 17% below explorer (known acceptable)

**Symptom**: Verify check explorer_total_depositors_count showing ours=66,009 vs explorer=79,276 (16.7% deviation).

**Root Cause**: Explorer uses incremental accumulation: `yesterday_total + daily_new_active_depositors`. The daily count includes returning depositors (withdrew all deposits, then deposited again), causing the cumulative total to re-count them. Our `cumulative_depositors` uses an `ever_deposited` HashSet that tracks each address exactly once (first deposit only). Our value is more accurate.

**Decision**: Accepted as known discrepancy. Widened X26 tolerance to 20%. Our definition is protocol-correct; the explorer's incremental formula has an inherent overcount.

**Files**: `crates/indexer/src/verify/explorer.rs`

---

## Category: Statistics & Charts

### STATS-001: Cumulative values wrong for new days (19ee513)

**Symptom**: `daily_statistics.cumulative_cells` and `cumulative_data_size` showed incorrect values, sometimes negative or resetting.

**Root Cause**: INSERT path was using only today's delta instead of `prev_cumulative + delta`. The update path was correct, but first-write path was wrong.

**Lesson**: Cumulative/rolling values need special handling on INSERT vs UPDATE. Test both paths - first record of the day (INSERT) vs subsequent records (UPDATE).

**Files**: `crates/indexer/src/db/writer.rs`

---

### STATS-002: Activity charts returning empty (f261854)

**Symptom**: Transaction and cell activity charts showed no data.

**Root Cause**: Charts were querying `daily_statistics` table which wasn't being populated by indexer. Changed to query directly from source tables (`transactions`, `cells`).

**Lesson**: Don't rely on denormalized/aggregated tables unless you're certain they're populated. When in doubt, query source tables directly (with appropriate aggregation).

**Files**: `crates/api/src/routes/statistics.rs`

---

### STATS-003: Incomplete latest day shown in charts (e4f4344)

**Symptom**: Charts showed misleading drop on latest day (partial data).

**Root Cause**: Daily stats for current day are incomplete (day not finished). Showing partial data misrepresents trends.

**Fix**: Exclude latest day from daily aggregation queries:

```sql
WHERE date < (SELECT MAX(date) FROM daily_block_stats)
```

**Lesson**: Time-series charts with daily aggregation should exclude incomplete periods.

**Files**: `crates/api/src/routes/statistics.rs`

---

### STATS-004: Chart edge data points not selectable (6c079c5)

**Symptom**: Mouse hover near chart edges didn't select first/last data points.

**Root Cause**: When mouse X was outside chart bounds (negative or beyond width), binary search returned invalid indices.

**Files**: `frontend/components/ui/line-chart.tsx`

---

### STATS-005: Percentage values displayed 100x too large (a487190)

**Symptom**: Charts showing percentages like 1120% instead of 11.2%.

**Root Cause**: API returned values already in percentage form (e.g., `11.20` for 11.20%), but frontend's `formatValue` multiplied by 100 again.

```typescript
// WRONG: Double multiplication
formatValue: (v) => `${(v * 100).toFixed(2)}%`;

// CORRECT: Value is already in percentage
formatValue: (v) => `${v.toFixed(2)}%`;
```

**Lesson**: Establish a contract between API and frontend for percentage values. Document whether values are 0-1 (ratio) or 0-100 (percentage). Consistency prevents this class of bugs.

**Files**: `frontend/components/ui/line-chart.tsx`

---

### STATS-006: Statistics fields not populated during batch sync (41c8786, 5e0cf52, e753200)

**Symptom**: Multiple charts showing all zeros (On-Chain Data Size, Epoch Time Distribution, Average Block Time).

**Root Cause**: `indexer_v2` batch sync mode wasn't populating these fields in `daily_statistics`. Fields were only updated in real-time sync mode.

**Affected fields**:

- `total_data_size` - cumulative on-chain data
- `epoch_time_distribution` - epoch duration histogram
- `avg_block_time_ms` - daily average block time

**Lesson**: When adding statistics fields, ensure BOTH sync modes populate them:

1. **Batch sync** (`indexer_v2.rs`) - historical data catchup
2. **Real-time sync** (`writer.rs`) - live block processing

Test by wiping database and re-syncing from genesis.

**Files**: `crates/indexer/src/db/writer_v2.rs`, `crates/indexer/src/sync/indexer_v2.rs`

---

## Category: Docker & Build

### BUILD-001: Rust version compatibility (0ebb92c, 3d5e392, 2dc515e)

**Timeline**:

1. `async-graphql v7` requires Rust 1.85+ → upgraded Dockerfiles
2. `home` crate version incompatible with 1.85 → downgraded `home` crate
3. `async-graphql` later required edition 2024 (Rust 1.89+) → removed GraphQL temporarily

**Lesson**:

- Pin critical dependency versions in production
- Check MSRV (minimum supported Rust version) of dependencies before upgrading
- Have a rollback plan for major dependency changes

**Files**: `docker/Dockerfile.api`, `docker/Dockerfile.indexer`, `crates/api/Cargo.toml`

---

### BUILD-002: Frontend Docker standalone output path (05f5a42, 195d72f)

**Symptom**: Frontend Docker build failing or missing files.

**Root Cause**: Next.js standalone output path varies based on working directory structure. When building in monorepo, standalone output is at `.next/standalone/frontend/`, not `.next/standalone/`.

**Lesson**: When using Next.js standalone in monorepo, check actual output structure. The standalone folder mirrors the build context structure.

**Files**: `docker/Dockerfile.frontend`

---

### BUILD-004: Next.js typedRoutes breaking Docker (88072b9)

**Symptom**: Docker build failing with TypeScript errors.

**Root Cause**: `typedRoutes: true` in next.config requires generated route types that aren't available in Docker build context.

**Lesson**: Features that require build-time code generation may not work in isolated Docker builds. Test Docker builds early.

**Files**: `frontend/next.config.ts`

---

### BUILD-005: Host CKB node connectivity (17fefa5)

**Symptom**: Indexer in Docker can't connect to host's CKB node.

**Root Cause**: Container network isolation. Using Docker network with service name didn't work for host-running CKB node.

**Fix**: Use `network_mode: host` for indexer container when CKB runs on host.

**Lesson**: When services need to communicate with host processes, consider `network_mode: host` or proper network bridge configuration.

**Files**: `docker-compose.yml`

---

## Quick Reference: Common Pitfalls

| Area        | Pitfall                             | Prevention                                               |
| ----------- | ----------------------------------- | -------------------------------------------------------- |
| CKB Scripts | Confusing code_hash vs script_hash  | code_hash = script type, script_hash = instance identity |
| CKB Scripts | Hardcoded hashes                    | Verify against chain, reference RFC-0024                 |
| DAO         | Multi-phase tracking                | Map full lifecycle before implementing                   |
| DAO         | Compensation formula                | Follow RFC-0023 exactly, use free_capacity               |
| DAO         | DAO field parsing                   | 32 bytes, 4 x u64 LE, check byte offsets                 |
| DAO         | APC calculation                     | Estimated = issuance/supply; Nominal = AR growth         |
| DAO         | Point-in-time aggregations          | Filter out withdrawn deposits for historical snapshots   |
| DAO         | Phase 2 withdrawal lookup           | Match by `withdraw_request_tx`, not `tx_hash`            |
| Supply      | Using total_issuance as circulating | Subtract 8.4B genesis burnt + secondary burnt            |
| Supply      | Confusing issuance vs circulating   | Read `docs/DAO_CALCULATIONS.md` supply model             |
| Indexer     | Fields not in batch sync            | Ensure both real-time AND batch sync populate all fields |
| Frontend    | Percentage double-multiply          | Establish API contract: ratio (0-1) or percent (0-100)   |
| Docker      | Missing files                       | Verify all runtime deps are COPY'd                       |
| Docker      | Network isolation                   | Use host network or proper bridging                      |
| Charts      | Incomplete data                     | Exclude current incomplete period                        |

---

## CKB-Specific Constants

```rust
// DAO
const DAO_CODE_HASH: &str = "0x82d76d1b75fe2fd9a27dfbaa65a039221a380d76c926f378d3f81cf3e7e13f2e";
const DAO_OCCUPIED_CAPACITY: u64 = 102_00000000; // 102 CKB in shannons

// Supply model (in shannons)
const GENESIS_BURNT: u64 = 8_400_000_000_00000000;          // 8.4B CKB burnt at genesis
const SECONDARY_ISSUANCE_PER_YEAR: u64 = 1_344_000_000_00000000; // 1.344B CKB/year

// DAO field extraction (32 bytes total)
fn extract_total_issuance(dao: &[u8]) -> u64 { u64::from_le_bytes(dao[0..8]) }
fn extract_ar(dao: &[u8]) -> u64 { u64::from_le_bytes(dao[8..16]) }

// Compensation formula
let free = capacity - DAO_OCCUPIED_CAPACITY;
let compensation = free * ar_withdraw / ar_deposit - free;

// Circulating supply (NOT same as total_issuance!)
let circulating = total_issuance - GENESIS_BURNT - secondary_burnt;

// APC formulas
let estimated_apc = (SECONDARY_ISSUANCE_PER_YEAR as f64 / total_issuance as f64) * 100.0;
let nominal_apc = ((ar_current as f64 / ar_past as f64).powf(1.0 / years) - 1.0) * 100.0;
```

---

## Category: Testing

### TEST-001: Pre-commit hooks blocking on pre-existing warnings (a55df69)

**Symptom**: Git commits rejected by pre-commit hook due to clippy warnings.

**Root Cause**: Husky pre-commit hook runs `cargo clippy` with `-D warnings` (deny all warnings). Existing code had ~80 warnings (type complexity, manual clamp, unused variables) that predated the hook setup.

**Workaround**: Use `git commit --no-verify` for commits that don't introduce new warnings.

**Proper Fix**: Clean up existing clippy warnings in a separate PR, then enable strict mode.

**Lesson**: When adding CI/CD enforcement, ensure existing code passes first. Otherwise you create friction for unrelated changes.

**Files**: `.husky/pre-commit`, `.github/workflows/ci.yml`

---

### TEST-002: Vitest globals not recognized in test files (a55df69)

**Symptom**: TypeScript errors for `describe`, `it`, `expect` in test files.

**Root Cause**: Vitest globals need to be declared in `tsconfig.json` types array.

```json
// tsconfig.json
{
  "compilerOptions": {
    "types": ["vitest/globals", "@testing-library/jest-dom"]
  }
}
```

**Lesson**: When adding test frameworks, update TypeScript configuration to include their type definitions.

**Files**: `frontend/tsconfig.json`, `frontend/vitest.config.mts`

---

### TEST-003: MSW handlers not intercepting API calls (a55df69)

**Symptom**: Tests making real network requests instead of using mocked responses.

**Root Cause**: MSW server not started before tests. Need setup file that starts server in `beforeAll`.

```typescript
// __tests__/setup.ts
import { server } from './msw/server';

beforeAll(() => server.listen({ onUnhandledRequest: 'error' }));
afterEach(() => server.resetHandlers());
afterAll(() => server.close());
```

**Lesson**: MSW requires explicit lifecycle management. The `onUnhandledRequest: 'error'` option catches missing handlers early.

**Files**: `frontend/__tests__/setup.ts`, `frontend/__tests__/msw/server.ts`

---

### TEST-005: Background tasks interfering with tests (a55df69)

**Symptom**: API integration tests hanging or timing out.

**Root Cause**: `AppConfig` was starting background tasks (warmup cache, WebSocket broadcaster) during tests. These tasks blocked or competed with test execution.

**Fix**: Add `start_background_tasks: bool` to `AppConfig`, default to `true` in production, `false` in tests.

```rust
pub struct AppConfig {
    pub start_background_tasks: bool,  // false in tests
}
```

**Lesson**: Test environments need different behavior than production. Use configuration flags to disable async background processes in tests.

**Files**: `crates/api/src/lib.rs`, `crates/api/src/main.rs`

---

## Category: Indexer

### IDX-001: sync_status updated before DAO/UDT processing complete

**Symptom**: DAO deposits, UDT transfers, or token statistics missing after indexer restart following a crash.

**Root Cause**: In `sync_blocks_batch()`, `flush_batch_stats()` (which updates `sync_status.tip_block_number`) was called BEFORE DAO and UDT processing completed. If the indexer crashed after sync_status update but before DAO/UDT writes, the batch would not be reprocessed on restart.

```rust
// WRONG order:
flush_batch_stats(&batch_stats).await?;  // Marks batch complete
// ... then DAO processing
// ... then UDT processing
// If crash here, data lost forever

// CORRECT order:
// ... DAO processing
// ... UDT processing
flush_batch_stats(&batch_stats).await?;  // Marks batch complete LAST
```

**Lesson**: In batch processing, the "completion marker" (sync_status update) must be the LAST operation. All data writes must complete before marking the batch as processed. This ensures crash recovery will reprocess incomplete batches.

**Files**: `crates/indexer/src/sync/indexer.rs`

---

### IDX-002: Reorg did not rollback token statistics

**Symptom**: After a chain reorganization, token `total_supply`, `holders_count`, `transfers_count`, and `token_balances` were inconsistent with actual on-chain state.

**Root Cause**: `execute_reorg()` deleted `token_transfers` but did not:

1. Reverse the `total_supply` changes from mints/burns
2. Recalculate `token_balances` from remaining transfers
3. Update `holders_count` and `transfers_count`

**Fix**: Added `rollback_token_statistics()` that:

1. Identifies affected tokens
2. Reverses supply changes (subtracts mints, adds back burns)
3. Decrements transfer counts
4. Rebuilds balances from remaining transfers
5. Recalculates holder counts

**Lesson**: When implementing rollback logic, trace ALL side effects of the operations being rolled back. Token transfers affect 4 tables: `token_transfers`, `tokens` (supply, counts), `token_balances`.

**Files**: `crates/indexer/src/db/writer.rs`

---

### IDX-003: DAO daily snapshots only updated for last date in batch

**Symptom**: `dao_daily_snapshots` table missing rows for historical dates. Charts using this table (`/charts/total-supply`, `/charts/secondary-issuance`, `/dao/charts/*`) showed gaps or incorrect data after batch sync.

**Root Cause**: In `flush_batch_stats()`, when a batch spanned multiple days, only the **last** date's snapshot was created:

```rust
// WRONG: Only updates the max date
if let Some(last_date) = stats.dao_snapshot_dates.iter().max() {
    self.writer.update_dao_daily_snapshot(*last_date).await?;
}

// CORRECT: Update ALL dates in chronological order
let mut snapshot_dates: Vec<_> = stats.dao_snapshot_dates.iter().collect();
snapshot_dates.sort();
for date in snapshot_dates {
    self.writer.update_dao_daily_snapshot(*date).await?;
}
```

**Impact**: During initial sync (or any batch spanning multiple days), intermediate dates never got `dao_daily_snapshots` rows. Charts showed incomplete data series.

**Lesson**: When batch operations accumulate data for multiple time periods, ensure ALL periods are flushed, not just the latest. The chronological order matters for cumulative calculations.

**Files**: `crates/indexer/src/sync/indexer.rs`

---

### IDX-004: Pipeline batch mismatch due to fetcher/writer race condition

**Symptom**: Repeated warnings during sync:

```
WARN Pipeline batch mismatch: expected 4086800, got 4086700. Draining stale batches.
INFO Drained 2 stale batches from pipeline
```

**Root Cause**: The pipeline fetcher task re-read `db_tip` from database every iteration to calculate `start_block`. But the writer task advances `db_tip` after processing each batch. When fetcher queued multiple batches (up to `pipeline_buffer=2`) before writer processed them:

1. Fetcher reads db_tip=4086699 → queues batch 4086700-4086799
2. Fetcher reads db_tip=4086699 (stale!) → queues batch 4086700-4086799 AGAIN
3. Writer processes first batch → db_tip becomes 4086799
4. Writer receives second batch (4086700) but expects 4086800 → **mismatch**

```rust
// WRONG: Re-read db_tip every iteration
loop {
    let (db_tip, _) = repo.get_sync_tip().await?;
    let start_block = (db_tip + 1) as u64;
    // ... fetch and send
}

// CORRECT: Track next_block locally
let mut next_block: Option<u64> = None;
loop {
    let start_block = match next_block {
        Some(nb) => nb,
        None => {
            let (db_tip, db_tip_hash) = repo.get_sync_tip().await?;
            // ... calculate from db_tip
        }
    };
    // ... fetch and send
    next_block = Some(end_block + 1);  // Advance locally
}
```

**Impact**: Wasted RPC calls fetching duplicate blocks. Constant warning spam in logs. Slightly reduced sync throughput due to draining stale batches.

**Lesson**: In producer/consumer pipelines, the producer should track its own state rather than re-querying shared state that the consumer modifies. Re-read shared state only on error recovery or explicit resync signals.

**Files**: `crates/indexer/src/sync/indexer.rs`

---

### IDX-005: Pipeline stuck after reorg - fetcher not notified (2026-01-30)

**Symptom**: After a chain reorganization, the indexer becomes stuck with continuous warnings:

```
INFO Reorg completed: fork_point=18499538, depth=1, orphaned_blocks=1, orphaned_txs=6
INFO Reorg handled, draining stale batches
INFO Drained 9 stale batches from pipeline
WARN Pipeline batch mismatch: expected 18499539, got 18499550. Draining stale batches.
WARN Pipeline batch mismatch: expected 18499539, got 18499551. Draining stale batches.
# ... repeats indefinitely, sync speed drops to 0
```

**Root Cause**: The IDX-004 fix introduced `next_block` local state tracking in the fetcher task. However, when a reorg occurs:

1. Writer detects reorg, rolls back DB to fork_point (18499538)
2. Writer drains stale batches from parse channel
3. **Fetcher is unaware** - its `next_block` still holds the old value (e.g., 18499550)
4. Fetcher continues sending batches starting from wrong block
5. Writer keeps rejecting with mismatch, draining, but fetcher keeps sending wrong blocks
6. **Deadlock**: Writer expects 18499539, fetcher sends 18499550+, forever

The periodic db_tip re-check (every 1000 blocks) was not triggered because the fetcher was already past that point.

**Solution**: Added `reorg_notify_flag: Arc<AtomicBool>` for cross-task communication:

```rust
// Writer: Signal fetcher after reorg/mismatch
if start_block != expected_start {
    self.reorg_notify_flag.store(true, Ordering::SeqCst);
    Self::drain_channel(&mut parse_rx).await;
    continue;
}

// Fetcher: Check flag and reset state
if reorg_notify.swap(false, Ordering::SeqCst) {
    info!("Fetcher received reorg notification, resetting next_block");
    next_block = None;  // Force re-query from DB
}
```

**Impact**: Indexer completely stuck after any reorg. Required manual container restart to recover.

**Lesson**: When one task in a pipeline modifies shared state that affects another task's assumptions, explicit notification is required. The "drain stale batches" pattern only clears the channel buffer, but doesn't reset the producer's internal state.

**Related**: IDX-004 (introduced `next_block` tracking to fix duplicate fetches)

**Files**: `crates/indexer/src/sync/indexer.rs`

---

## Category: Performance

### IDX-005: UDT burn/send-all transactions not tracked (2026-01-26)

**Symptom**: Address page showed incorrect asset holdings:

1. `token_balances` showed balance even after all tokens sent away
2. `address_asset_transfers` missing "out" direction records for send transactions
3. Transaction history showed "Received +X Token" but not "Sent -X Token"

**Root Cause**: The UDT processing loop in both `sync_blocks_batch` (pipeline) and `sync_block_optimized` (sequential) skipped transactions that had **no UDT outputs**:

```rust
// WRONG: Skip txs without UDT outputs
let output_udts = UdtParser::parse_udt_cells(tx);
if output_udts.is_empty() {
    continue;  // BUG: Skips burns and send-all transactions!
}
```

When user A sends ALL their tokens to user B:

- Transaction has UDT **inputs** (A's tokens being consumed)
- Transaction has UDT **outputs** (B receiving tokens)
- But from A's perspective, A has no UDT outputs
- The early `continue` caused the entire transaction to be skipped for UDT processing

**Impact**:

1. `token_balances` never decremented for sender
2. `udt_cells` never marked as consumed
3. `address_asset_transfers` missing "direction: out" record
4. `token_transfers` record created but only from output perspective

**Fix**: Restructured UDT processing to:

1. Collect ALL input outpoints from ALL non-cellbase transactions
2. Query `udt_cells` table to identify which inputs are UDT cells
3. Include transactions that have either UDT outputs OR UDT inputs

```rust
// CORRECT: Check both outputs AND inputs
let has_udt_outputs = !tx_info.output_udts.is_empty();
let has_udt_inputs = tx_info.input_outpoints.iter().any(|(tx_hash, idx)| {
    input_udt_info.contains_key(&(tx_hash.clone(), *idx))
        || batch_udt_cells.contains_key(&(tx_hash.clone(), *idx))
});

if has_udt_outputs || has_udt_inputs {
    udt_tx_contexts.push(/* ... */);
}
```

**Test Coverage Added**:

- `parser/udt.rs`: `test_parse_transfers_burn_no_output`, `test_parse_transfers_to_different_address`
- `tests/asset_transfers.rs`: `test_token_balance_decremented_on_burn`, `test_asset_transfer_records_both_directions`

**Lesson**: UTXO-like models require tracking BOTH inputs (consumption) AND outputs (creation). When optimizing by skipping "uninteresting" transactions, verify the skip condition accounts for all relevant cases.

**Files**: `crates/indexer/src/sync/indexer.rs` (both pipeline and sequential modes)

---

## Category: Task Runner (Legacy)

> Historical context: this category describes a legacy task-runner + deferred-flag
> architecture that has been removed. Current bulk sync design is single-shot and
> fail-fast, with required derived data written inline on the canonical sync path
> (no defer + refill/rebuild workflow).

### TR-001 (Legacy): Circular dependency in bulk sync detection deadlocks all rebuild tasks

**Symptom**: After bulk sync completed, `index_rebuild`, `cells_status_rebuild`, and all other rebuild tasks remained in `pending` state indefinitely. Task-runner repeatedly claimed and deferred `index_rebuild` in a hot loop (~1000 times/second).

**Root Cause (legacy design)**: `is_bulk_sync_active()` checked `indexes_deferred || address_balances_deferred || token_deferred` flags in `sync_status`. But these flags are cleared BY the rebuild tasks themselves, creating a circular dependency:

1. `index_rebuild` requires `is_bulk_sync_active() = false` to run
2. `is_bulk_sync_active()` returns `true` because `indexes_deferred = true`
3. Only `index_rebuild` completion sets `indexes_deferred = false`
4. Deadlock: task can never start

**Fix (at that time)**: Changed `is_bulk_sync_active()` to check actual block sync progress via `MAX(timestamp)` from the `blocks` table. If the latest block is within 1 hour of current time, bulk sync is considered complete. This is independent of any deferred flags.

**Current Status**: The deferred flags and task-runner rebuild flow referenced here are removed from current code.

**Lesson**: Never use the output of a task as a precondition for that same task to start. Bulk sync detection should use an orthogonal signal (block recency) rather than rebuild state flags.

**Test Coverage Added**: 5 tests in `crates/task-runner/tests/bulk_sync_protection.rs` - no blocks, old blocks, recent blocks, boundary, empty table.

**Files**: `crates/task-runner/src/db.rs` - `is_bulk_sync_active()`

---

### TR-002 (Legacy): Task-runner hot loop when deferring tasks (no backoff)

**Symptom**: Task-runner consumed 100% CPU and generated millions of log lines per minute, all showing the same task being claimed and deferred.

**Root Cause (legacy design)**: `run_once()` returned `Ok(true)` after deferring a task. The `run_continuous()` loop only sleeps when `Ok(false)` (no task found) or `Err`. So the loop immediately re-claimed the same task with zero delay.

```rust
// Before: claimed + deferred → Ok(true) → no sleep → instant re-claim
self.db.defer_task(task.id, &reason).await?;
return Ok(true);  // BUG: triggers immediate retry
```

**Fix (at that time)**: Changed defer path to return `Ok(false)`, triggering the 5-second poll interval sleep before retrying.

**Lesson**: In poll loops, any path that doesn't make progress must trigger backoff. "Claimed but deferred" is not progress.

**Test Coverage Added**: `test_run_once_returns_false_when_task_deferred` and `test_run_once_returns_true_when_task_executes` in `crates/task-runner/tests/bulk_sync_protection.rs`.

**Files**: `crates/task-runner/src/executor/mod.rs` - `run_once()`

---

## Category: Numeric Safety

### IDX-006: Shannons `i64`/unchecked cast caused overflow risk and pipeline false-idle hang (2026-02-22)

**Symptom**: Bulk sync stalled with repeated idle warnings and no forward progress, while some numeric fields had potential wraparound risk under large values.

**Root Cause**:

1. **Unchecked narrowing conversions (`u64 as i64`)** on sync-critical parsing path:
   - transaction `since`
   - block hex parsing helpers
   - tx `cycles` parsing
     These could silently wrap when input exceeds `i64::MAX`.

2. **Daily shannons deltas stored as `i64`** (`ScriptDailyDelta` / `TokenDailyDelta` / `ClusterDailyDelta` / `SporeDailyDelta` / `NftDailyDelta`), leaving insufficient headroom for long-term cumulative scale.

3. **Daily/Hourly transfer and occupied-capacity aggregates stored as `i64`**
   (`DailyStats.capacity_transferred`, `DailyStats.occupied_capacity_*`, `HourlyStats.capacity_transferred`), which can exceed `i64` under sustained high churn.

4. **Pipeline idle timeout path** could keep waiting even when parser/fetcher task had already exited, creating a false "idle" loop instead of hard failure.

**Fix**:

- Replaced unchecked casts with fail-fast checked conversion (`try_from`) and explicit context in panic/error messages.
- Promoted daily shannons delta value types from `i64` to `i128` across store types, indexer writer/update paths, sync aggregation maps, and API accumulation path.
- Promoted daily/hourly shannons aggregate fields to `i128` and applied checked-add on batch merge paths to fail fast on impossible arithmetic overflow.
- Added checked-add overflow guards when applying daily deltas in writer batch updates.
- On pipeline idle timeout, detect parser/fetcher completion and return error immediately instead of waiting indefinitely.
- Added progress-stall logging (rate-limited) with pipeline stage timings + queue fill ratios to speed up root-cause diagnosis when sync appears alive but block height does not advance.

**Tests Added/Updated**:

- `parser/cell.rs`: overflow panic test for capacity parsing.
- `parser/transaction.rs`: overflow panic test for `since`.
- `parser/block.rs`: overflow panic tests for `parse_hex_i64` / `parse_hex_i32`.
- `indexer/tests/pipeline_consistency.rs`: idle-timeout abort regression test.
- `indexer/tests/daily_statistics.rs`: regression coverage for daily/hourly shannons values above `i64::MAX`.
- `indexer/src/main.rs` unit tests: stall-warning policy helper coverage.
- writer unit tests updated for `i128` daily delta behavior.

**Lesson**:

- Any on-chain amount in shannons should default to `i128` in stored/aggregated forms.
- Never use `as` for narrowing integer conversion on correctness-critical paths.
- Timeout handlers must differentiate "slow" from "producer already dead"; dead producers should fail fast.

**Files**: `crates/indexer/src/parser/cell.rs`, `crates/indexer/src/parser/transaction.rs`, `crates/indexer/src/parser/block.rs`, `crates/indexer/src/sync/indexer.rs`, `crates/indexer/src/db/writer/addresses.rs`, `crates/indexer/src/db/writer/udt.rs`, `crates/indexer/src/db/writer/spore.rs`, `crates/indexer/src/db/writer/mnft.rs`, `crates/ckbadger-store/src/types.rs`, `crates/api/src/utils/assets.rs`, `crates/api/src/routes/scripts.rs`, `crates/api/src/routes/statistics.rs`

---

## Category: NervosDAO

### DAO-021: Partial-day reorg corrupts cutoff-date snapshots and daily_block_stats uncles

**Date**: 2026-04-10
**Affected**: `dao_daily_snapshots` (all fields), `daily_block_stats.total_uncles`

**Symptom**: On mainnet with multiple tip reorgs per day, the `explorer_total_deposit`,
`explorer_daily_deposit`, and `explorer_uncle_rate` verify checks all failed.
Inspection showed `dao_daily_snapshots.total_deposited` / `cumulative_deposit_amount`
frozen at the previous day's value with tiny bumps corresponding to only the
last handful of rolled-back-and-replayed blocks, and `daily_block_stats.total_uncles`
over-counted by 1 per rolled-back block that had contained an uncle.

**Root cause**:

1. `CachedBlockHeader` did not store `uncles_count`, so the reorg rollback path
   had no way to decrement `total_uncles` — the existing code decremented only
   `block_count` with a comment dismissing the error as "negligible for shallow
   reorgs of 1-2 blocks out of ~720/day", not realizing daily block count is
   ~7500 and that errors compound across many reorgs.
2. `repair_cutoff_date_stats` had no case for `STATS_PREFIX_DAO_DAILY_SNAPSHOT`,
   so `should_delete_stats_for_replay` deleted the cutoff-date DAO snapshot
   unconditionally. When live sync replayed the rolled-back blocks, it loaded
   `latest_snapshot` (now the previous day's snapshot) and added only the
   replay-block deltas, silently dropping all contributions from the
   non-rolled-back portion of the cutoff date.

**Fix**:

1. Added `uncles_count: i32` to `CachedBlockHeader` and populated it in both
   the live-sync writer (`crates/indexer/src/db/writer/chain.rs`) and the
   bulk-build writer (`crates/indexer/src/sync/bulk_build/mod.rs`).
2. Extended `RollbackStatsDeltas` with `date_uncles` and populated it during
   the block header deletion loop.
3. Extended `repair_cutoff_date_stats`' `STATS_PREFIX_DAILY_BLOCK` case to
   subtract `total_uncles` with a fail-fast underflow check (both the
   `checked_sub` overflow guard AND an explicit `< 0` bail, because
   `i32::checked_sub` only catches arithmetic overflow, not "went negative").
4. Added `CkbadgerStore::recompute_dao_daily_snapshot_for_date` that scans
   the authoritative `dao_deposits` CF (one pass, grouping by
   deposit/phase1/phase2 block) and walks block headers forward to recompute
   a single date's snapshot from scratch, including all cumulative secondary
   issuance splits.
5. Added a new reorg stage `recompute_dao_daily_snapshots` that runs after
   `delete_stats_from_cutoff` and invokes the recompute for every date from
   `fork_point_date` through `cutoff_date`.
6. Added integration tests in `crates/indexer/tests/dao_daily_snapshot.rs`
   covering partial-day, cross-day, and phase-1-in-rollback-range cases.

**Re-sync required**: Yes. The `uncles_count` field is new and reads as `0`
for pre-existing rows via `#[serde(default)]`, so the uncles repair would
undercount if any rolled-back block has a pre-fix header. Per CLAUDE.md Sync
Bug Policy, the correct procedure is: land the fix, run `ckbadger purge`,
re-sync from genesis.

---

### DAO-022: Testnet DAO compensation diverges from protocol free-capacity accounting

**Date**: 2026-07-23

**Symptom**: Testnet verification reported cumulative DAO compensation up to
14.23% above the official explorer, with smaller daily-series drift around the
same aggregate.

**Root cause**:

1. The secondary-issuance split used each DAO cell's full capacity. RFC-0023
   compensation accrues only to free capacity, so the mandatory 102 CKB
   occupied capacity of every live DAO cell was incorrectly treated as
   interest-bearing.
2. The split paired the pre-block deposit state with block N's C/U values.
   Protocol block N uses C/U at the end of block N-1, so all three inputs did
   not describe the same point in chain history.
3. Negative S-field corrections at protocol boundaries were skipped while the
   baseline advanced. The following rebound was then counted again, breaking
   exact telescoping across the boundary (a regression of DAO-018).

**Fix**:

- Added one shared exact split in `ckbadger-common` and routed live sync, bulk
  build, and reorg snapshot recomputation through it.
- Track protocol DAO free capacity as `capacity - 102 CKB` per live DAO cell,
  while retaining full capacity for user-facing locked-capacity statistics.
- Carry the complete previous `(C, S, U)` header state and use previous C/U
  with the pre-block free-capacity state.
- Preserve negative S corrections entirely in treasury so miner and DAO
  compensation remain monotonic while `DAO + treasury` telescopes exactly.

**Tests Added/Updated**:

- Shared free-capacity, signed-delta, positive-split, and negative-S tests.
- Live/bulk DAO lifecycle coverage for the 102 CKB occupied-capacity exclusion.
- Regression coverage proving block N uses block N-1 C/U, including reorg
  snapshot recomputation.

**Re-sync required**: Yes. Historical DAO snapshots were written by the wrong
calculation path; fix the writer, purge the chain stores, and re-sync from
genesis rather than backfilling or patching persisted aggregates.

---

_Last updated: 2026-07-23_
