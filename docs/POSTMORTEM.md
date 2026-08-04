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

### STATS-007: Home hashRate overstated ~20% after every epoch difficulty step

**Date**: 2026-08-01

**Symptom**: `/statistics/network` `hashRate` read 87.98 PH/s while the exact
windowed value was 73.54 PH/s, right after an epoch difficulty increase; the
error decayed over the next ~600 blocks and recurred at every epoch boundary.

**Root Cause**: `hash_rate = tip_epoch_difficulty / avg_block_time` applied the
tip epoch's difficulty to a 600-gap window that still consisted mostly of
previous-epoch (lower-difficulty) blocks — mixing one epoch's difficulty with
another epoch's block rate.

**Fix**: Estimate from actual work in the window:
`Σ per-block compact_to_difficulty(compact_target) / window span seconds`,
excluding the oldest boundary block's work (it predates the span). The
displayed `difficulty` field stays tip-epoch difficulty.

**Lesson**: A rate derived from difficulty is only valid over blocks mined at
that difficulty. Any window crossing an epoch boundary must sum per-block
work, not scale a point difficulty.

---

### STATS-008: Asset-ecosystem capacity breakdown mixed units (DAO showed 161%)

**Date**: 2026-08-01

**Symptom**: `/statistics/asset-ecosystem` `capacityBreakdown` reported DAO at
161.01% and the categories summed to 162.6%; `other` was silently clamped to 0.

**Root Cause**: Numerators were full cell capacities (DAO `total_deposited`,
token/object owned capacity) but the denominator was the DAO snapshot's
`occupied_capacity` (knowledge size — occupied bytes only, where a DAO cell
contributes ~102 CKB, not its deposit). `other = clamp(total - categorized, 0)`
masked the structural violation as a "transient warmup skew".

**Fix**: Denominator is total live capacity `C − S` from the tip header's DAO
field (every issued-but-live shannon sits in a cell); response adds
`totalLiveCapacityCkb`; `other` is the exact remainder and a negative remainder
is a hard error naming all four numbers. `totalKnowledgeSizeCkb` remains as a
standalone stat.

**Lesson**: Every ratio needs numerator and denominator in the same unit, and a
`clamp(…, 0)` on a derived remainder is a bug mask, not robustness.

---

### STATS-009: recent-blocks silently truncated the 24h window at 10,000 blocks

**Date**: 2026-08-01

**Symptom**: `/statistics/recent-blocks` fetched `list_blocks_desc(None, 10000)`
then filtered by cutoff; 2026-07-30 (UTC+8) had 10,141 mainnet blocks in 24h,
so peak days silently lost the oldest ~24 minutes of the window.

**Root Cause**: A one-shot fetch sized by the ~8,640-block estimate treated an
estimate as a bound.

**Fix**: Cursor-paginate until the first block at or before the cutoff (or
store exhaustion), with a generous safety bound that returns an explicit error
instead of truncating.

**Lesson**: Never bound a time-window query by an estimated row count; page
until the boundary condition is actually seen, and make any safety cap loud.

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

| Area        | Pitfall                             | Prevention                                                                                  |
| ----------- | ----------------------------------- | ------------------------------------------------------------------------------------------- |
| CKB Scripts | Confusing code_hash vs script_hash  | code_hash = script type, script_hash = instance identity                                    |
| CKB Scripts | Hardcoded hashes                    | Verify against chain, reference RFC-0024                                                    |
| DAO         | Multi-phase tracking                | Map full lifecycle before implementing                                                      |
| DAO         | Compensation formula                | Follow RFC-0023 exactly, use free_capacity                                                  |
| DAO         | DAO field parsing                   | 32 bytes, 4 x u64 LE, check byte offsets                                                    |
| DAO         | APC calculation                     | Keep estimated and nominal models distinct; both use the persisted network genesis baseline |
| DAO         | Point-in-time aggregations          | Filter out withdrawn deposits for historical snapshots                                      |
| DAO         | Phase 2 withdrawal lookup           | Resolve the request outpoint through `dao_by_withdraw_tx`                                   |
| Supply      | Using total_issuance as circulating | Use exact `C - GenesisBaseline.burnt - S`                                                   |
| Supply      | Confusing issuance vs circulating   | Read `docs/DAO_CALCULATIONS.md` supply model                                                |
| Indexer     | Fields not in batch sync            | Ensure both real-time AND batch sync populate all fields                                    |
| Frontend    | Percentage double-multiply          | Establish API contract: ratio (0-1) or percent (0-100)                                      |
| Docker      | Missing files                       | Verify all runtime deps are COPY'd                                                          |
| Docker      | Network isolation                   | Use host network or proper bridging                                                         |
| Charts      | Incomplete data                     | Exclude current incomplete period                                                           |

---

## CKB-Specific Constants

```rust
// DAO
const DAO_CODE_HASH: &str = "0x82d76d1b75fe2fd9a27dfbaa65a039221a380d76c926f378d3f81cf3e7e13f2e";
const SECONDARY_ISSUANCE_PER_YEAR: u64 = 1_344_000_000_00000000; // 1.344B CKB/year

// DAO field extraction (32 bytes total)
fn extract_total_issuance(dao: &[u8]) -> u64 { u64::from_le_bytes(dao[0..8]) }
fn extract_ar(dao: &[u8]) -> u64 { u64::from_le_bytes(dao[8..16]) }

// Compensation formula: 102 CKB is only the standard secp DAO-cell case.
// Persist and use the deposit cell's exact occupied capacity.
let free = capacity - exact_occupied_capacity;
let compensation = free * ar_withdraw / ar_deposit - free;

// GenesisBaseline is derived from block 0 and persisted per network.
let baseline = store.get_genesis_baseline()?.expect("required invariant");

// Protocol circulating supply (NOT the same as total_issuance).
let circulating = total_issuance - baseline.burnt - secondary_pool_s;

// Estimated APC: explorer-compatible continuous-compounding model,
// seeded with baseline.total_issuance.
let estimated_apc = calculate_estimated_apc(
    epoch_number,
    epoch_index,
    epoch_length,
    baseline.total_issuance,
);

// Nominal chart: separate theoretical supply curve, seeded with exact genesis circulation.
let nominal_genesis_supply = baseline.total_issuance - baseline.burnt;
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

**Follow-up**: DAO-025 records why this proportional aggregate split remained
insufficient even after its free-capacity correction.

---

### DAO-023: Circulating supply included unissued DAO interest

**Date**: 2026-07-24

**Symptom**: Mainnet `explorer_circulating_supply` failed by about 1.69%
(roughly 833.1M CKB), while the burnt-supply comparison still passed.
`explorer_knowledge_size` also reported a one-day 232 CKB transition mismatch.

**Root cause**:

1. API circulation paths subtracted genesis burn and treasury
   (`S - unmade_dao_interests`) from DAO `C`. This left
   `unmade_dao_interests` in circulation even though RFC-0023 defines all of
   `S` as unissued secondary issuance.
2. The same supply formula was duplicated across statistics, DAO ratio, and
   chart handlers, including a fallback to `cum_treasury`.
3. The knowledge-size verifier compared ckbadger's DAO-U-derived
   `knowledge_size` with the explorer's independently indexed
   `occupied_capacity`. The latter carried a historical live-cell projection
   correction. The explorer's own DAO-U-derived `knowledge_size` matched
   ckbadger exactly.

**Fix**:

- Added one fail-fast API supply calculation:
  `circulating = C - genesis_burnt - S`,
  `treasury = S - unmade_dao_interests`, and
  `liquid = circulating - active_dao_principal`.
- Routed the total-supply chart, network hero metric, circulation-ratio chart,
  knowledge utilization, and treasury chart through that calculation.
- Removed the legacy `cum_treasury` fallback.
- Changed X5 to compare against explorer `knowledge_size`, preserving exact
  daily DAO-U transition validation.

**Tests Added/Updated**:

- Verifier HTTP regression proving X5 requests `knowledge_size`.
- Shared supply arithmetic and invariant tests.
- DAO, statistics, and network-response circulation regressions.

**Re-sync required**: No. The persisted DAO `C`, `S`, and unmade-interest
snapshots are correct; this was an API read-path and verifier-source bug.

---

### DAO-024: Explorer circulation check compared different policy scopes

**Date**: 2026-07-24

**Symptom**: After DAO-023 corrected the protocol supply formula,
`explorer_circulating_supply` still failed for all 30 days by about 0.2094%,
or 103,000,770 CKB. The new API values proved that the larger 833.1M CKB error
was fixed.

**Root cause**:

1. ckbadger circulation is chain-native: `C - genesis_burnt - S`.
2. The official explorer additionally subtracts policy-classified capacity:
   historical vesting allocations and the live balance of its labelled Bug
   Bounty address. After the vesting schedules ended, the latter accounted for
   the complete residual difference.
3. X11 directly compared these different definitions. Its 0.2% tolerance had
   previously hidden the semantic mismatch until the Bug Bounty balance crossed
   that relative threshold.

**Fix**:

- X11 now fetches the explorer's separately published `locked_capacity` and
  exactly adds it back to `circulating_supply` before comparison.
- Kept the ckbadger API on its CKB-native calculation instead of importing an
  off-chain address label into the supply definition.
- Limited normalization to X11's requested 30-day window. Explorer has old
  BigDecimal rows with sub-shannon fractions outside that window.
- Added an exact decimal parser that accepts integer-valued forms such as
  `4918812917022796752.0` but rejects non-zero sub-shannon fractions.
- Replaced floating-point/filtering stacked-series summation with checked exact
  shannon arithmetic, and made the 0.2% decision an exact `i128` ratio check.
- Did not widen the verification tolerance.

**Tests Added**:

- HTTP regression proving X11 requests both explorer series, ignores unrelated
  legacy fractional rows, accepts current integer-valued decimal rows, and
  restores policy locked capacity before comparison.

**Re-sync required**: No. This was a verifier semantic-normalization bug; no
stored chain or derived data changed.

---

### DAO-025: Aggregate secondary split was not exact DAO compensation

**Date**: 2026-07-24

**Symptom**: After DAO-022's free-capacity correction and a rebuild, testnet
daily `deposit_compensation` remained about 13.96% above the official explorer;
the latest NervosDAO check was about 14.03% high.

**Root cause**:

1. Historical `cum_dao_compensation` was still reconstructed by multiplying
   each block's aggregate secondary-pool delta by aggregate DAO principal.
   That is not equivalent to RFC-0023's per-deposit AR calculation. Deposits
   have different AR start values and integer flooring, and phase-1 requests
   freeze at their request AR.
2. DAO-022 subtracted a fixed 102 CKB occupied capacity per deposit. That value
   describes a standard secp256k1 DAO cell, not every possible lock script.
   Compensation must use the original cell's exact occupied capacity.
3. The testnet explorer's daily `deposit_compensation` series has a historical
   constant baseline gap. Full chain replay showed that recent daily changes
   match the exact lifecycle result, while absolute levels retain the old gap.
4. The explorer NervosDAO response mixes a daily-statistics
   `deposit_compensation` field with live `claimed_compensation` and
   `unclaimed_compensation` fields. Its live components sum to the comparable
   current value.

**Fix**:

- Persist the original DAO deposit cell's exact occupied capacity in the
  domain-store lifecycle entry.
- Make exact per-deposit lifecycle accounting the single compensation path:
  active deposits use the observation AR, phase-1 deposits use the frozen
  request AR, and completed deposits contribute their stored claimed amount.
- Retain request AR after phase-2 and validate stored claimed compensation
  against the exact request-AR calculation, so a rollback to phase-1 remains
  computable and corrupted lifecycle entries fail immediately.
- Materialize bulk daily snapshots through an event-ordered timeline that
  advances deposits, requests, and completions exactly. Live latest statistics
  and reorg repair use the same lifecycle arithmetic.
- Restrict aggregate `C/S/U` arithmetic to miner secondary issuance. Treasury
  remains the direct `S - active_unmade` derivation.
- Compare testnet daily compensation changes with the existing 0.2% tolerance,
  preserving the explorer's constant historical offset without hiding bad
  transitions.
- Compare the latest value with explorer
  `claimed_compensation + unclaimed_compensation`, using checked exact integer
  arithmetic.

**Tests Added/Updated**:

- Actual occupied-capacity compensation regression.
- Active → frozen phase-1 → claimed phase-2 lifecycle regressions in store and
  bulk materialization.
- Phase-2 rollback regression proving request AR survives and frozen
  compensation is reproduced exactly.
- Bulk event-timeline historical reconstruction regression.
- Exact rational daily-delta verifier tests for constant baseline and bad
  transition cases.
- Latest Explorer live-component sum regression.
- DAO rollback and API fixtures updated for the new persisted schema.

**Re-sync required**: Yes. `DaoDepositCacheEntry` now contains exact occupied
capacity, and historical DAO daily snapshots were written by the invalid
aggregate calculation. Purge the chain stores and sync from genesis; do not
backfill or patch the old aggregates.

---

### DAO-026: Cross-day live batch left completed compensation snapshot stale

**Date**: 2026-07-25

**Symptom**: Mainnet `explorer_deposit_compensation` failed only for the most
recent completed day. The 2026-07-23→2026-07-24 change was short by about
7,836.9 CKB. Reading the domain store showed a persisted July 24 cumulative
value of `155184037017868168`; exact lifecycle recomputation at that day's last
block produced `155184820689222572`.

**Root cause**:

1. Live snapshot construction shares the atomic domain batch with DAO lifecycle
   mutations, so compensation fields are intentionally staged from the
   pre-commit lifecycle state.
2. After the domain batch committed, `refresh_latest_dao_statistics` replaced
   the staged fields only in the lexicographically latest daily snapshot.
3. When one live batch crossed the UTC+8 day boundary, the batch wrote both the
   just-completed date and the new current date. The post-commit refresh fixed
   only the new date, permanently leaving the completed date at the preceding
   batch's cumulative compensation.

**Fix**:

- Carry every completed date and its last block from the live batch into the
  post-commit DAO refresh.
- Validate each boundary against the canonical block header and the first block
  of the following date, then run the one exact per-deposit lifecycle
  calculation at that boundary's AR.
- Materialize completed-date and live-tip compensation fields together in one
  domain-store DAO statistics batch.
- Keep bulk materialization and append-only storage unchanged.

**Tests Added**:

- Cross-day regression proving the completed snapshot is evaluated at its own
  final block/AR while the current snapshot is evaluated at the live tip.
- Boundary-selection and missing-end-block fail-fast tests.

**Re-sync required**: Yes. A completed daily snapshot already written by the
old live path remains incorrect. Fix the indexer, purge the chain stores, and
sync from genesis; do not add a repair/backfill workflow.

---

### DAO-027: Inflation chart rejected legitimate testnet blockless days

**Date**: 2026-07-31

**Symptom**: Testnet `chart_inflation_rate_sane` failed because
`/charts/inflation-rate` returned 500 for a DAO snapshot jump from 2020-05-12
to 2020-05-22.

**Root cause**:

1. The realized-inflation read path assumed persisted DAO snapshot dates were
   a dense calendar series and classified every missing date as corruption.
2. DAO snapshots are intentionally materialized only for dates containing
   blocks. Testnet block 0 is dated 2020-05-12, while consecutive block 1 is
   dated 2020-05-22, so the intervening nine complete dates have no blocks and
   correctly have no persisted snapshots.
3. The API regression fixture fabricated all 366 daily rows and therefore did
   not exercise the real testnet genesis-to-first-block timestamp gap.

**Fix**:

- Validate every observed snapshot gap against canonical block headers.
- If the first canonical block after a gap belongs to the next snapshot date,
  carry the previous state through each blockless day exactly for the
  trailing-365-calendar-day calculation.
- If any canonical block belongs to a missing snapshot date, retain the
  fail-fast 500 with the missing date and first affected block.
- Keep the densification on the API read path; no RocksDB rows are synthesized
  or written by the API.

**Tests Added**:

- Real testnet block-0/block-1 timestamps prove blockless days are filled and
  produce continuous trailing-year chart points.
- A block-bearing missing date still fails with canonical block context.

**Re-sync required**: No. The sparse persisted snapshots are correct; purging
and replaying the same chain would reproduce the same legitimate date gap.

---

## Category: API Read Path

### API-001: Lazy cycles executed historical scripts on the wrong VM version

**Date**: 2026-08-01

**Symptom**: `/transactions/{hash}/cycles` returned 1,644,449 for an epoch-264
secp transfer whose consensus-true count is 1,709,221. Every pre-Meepo
transaction with a `hash_type: "type"` script group was affected.

**Root Cause**: The lazy path shells out to `ckb-debugger` without any epoch
context; the debugger defaults to `--script-version 2`, so `hash_type: "type"`
groups always ran on the newest VM. Consensus pins VM selection to the commit
block's epoch (RFC0032 → VM1, RFC0049 → VM2).

**Fix**: `calculate_cycles` now requires a committed tx, derives the epoch from
the commit header, maps it to a script version using activation epochs fetched
from the node's `get_consensus` (`rfc "0032"` / `"0049"`; absent or null = never
activated — no hardcoded per-network tables), and passes `--script-version` for
every group. `--script-version` is a consensus _ceiling_: `data*` groups stay
pinned by their hash_type, so one per-tx value reproduces consensus exactly.
Already-persisted wrong values heal on re-sync (bulk sync stores node-reported
cycles only when present; the lazy path recomputes the rest with fixed logic).

**Lesson**: Replaying historical execution requires the historical rule set.
Any re-execution tool must be pinned to the consensus parameters of the block
being replayed — and "matches the official explorer" is not validation for
pre-Mirana history: the explorer's own cycles for that era are VM1 replays,
wrong the same way.

---

### API-002: Spore cursor pagination irrecoverably skipped same-block groups

**Date**: 2026-08-01

**Symptom**: Walking `/spore/objects` to exhaustion returned 31,975 of 37,291
live spores (15.6% missing). All 5,806 missing ids shared `createdAtBlock` with
a page-boundary cursor.

**Root Cause**: The cursor was `created_at_block` alone with a strict
`created_at_block < cursor` resume, so any entries of the cursor's block not
consumed by the previous page were skipped forever. Blocks hold up to 181
spores while the page limit caps at 100, so some blocks could never be fully
listed. The same design existed in the clusters list, per-cluster spores, and
per-owner spores paths.

**Fix**: Composite cursor `{block}:{0x-id}` over an explicit total order
`(created_at_block DESC, id ASC)`, one strict parser shared by all four
endpoints (malformed or legacy numeric cursors → 400).

**Lesson**: A pagination cursor must name a unique position in a total order.
If the sort key isn't unique, the cursor needs a tiebreaker — and the same
cursor bug rarely lives in only one endpoint.

---

### API-003: Cluster cells leaked into the spore objects list as dead links

**Date**: 2026-08-01

**Symptom**: All 490 live cluster cells appeared as rows in `/spore/objects`
(and per-owner lists) with empty contentType; their `/spore/objects/{id}`
detail returned 404.

**Root Cause**: The spore store CF holds spores and clusters together;
`SporeCache::build` filtered clusters out of `by_cluster`/`name_index` but not
out of `live_indices`/`by_owner`, while the detail handler rejects clusters.

**Fix**: Exclude cluster entries from both list indexes; clusters remain served
by their own list/detail endpoints.

**Lesson**: When one CF stores two kinds, every index built over it must state
which kind it serves; a filter applied to some indexes and not others is a
latent inconsistency.

---

### API-004: Cluster sporesCount mixed ever-minted and live semantics

**Date**: 2026-08-01

**Symptom**: `/spore/clusters/{id}` showed `sporesCount: 97` beside live-based
holders (10), items (10), and composition (10) for a cluster with 87 melted
spores; 197 of 490 clusters were affected, including one showing 20 spores
with 0 items and 0 holders.

**Root Cause**: The detail and list paths read `agg.total_count` (ever minted,
including melted) for a field displayed among live-based figures; the list
additionally consumed it through a cached field named `transfers_count`.

**Fix**: All spore-count fields on the spore surfaces read `agg.live_count`
(single path shared by list and detail); cluster existence is judged by store
presence so fully-melted clusters still resolve. The ever-minted total remains
available as `totalCount` on the `/assets` surface, which was already correct.

**Lesson**: When a response mixes counters, every counter must state its
population (live vs ever). A cached field whose name doesn't match its content
(`transfers_count` = mint total) will eventually be consumed as its name.

---

### API-005: One name, two quantities — "Knowledge Size" on the hero vs the chart

**Date**: 2026-08-02

**Symptom**: `/statistics/network` reported `knowledgeSize` 519,967,746,700,000,000
shannon — bit-identical to the node's raw DAO header `U` — while
`/charts/knowledge-size`, which the homepage tile links to, plotted
159,659,096 CKB for the same moment. The same named concept differed by 32.6×.
`/statistics/asset-ecosystem`'s `totalKnowledgeSizeCkb` had the identical defect.
The frontend compounded it: `ckbytes-card` computed
`free = circulating − knowledge − dao` from the raw-`U` value, misallocating
~5.04B CKB in the circulation bar, and hid the impossible result behind a
`Math.max(0, …)` clamp.

**Root Cause**: Both handlers passed `DaoDailySnapshot.occupied_capacity`
(documented as the DAO header `U`) straight through, without subtracting the
network's `GenesisBaseline.virtual_occupied`. The chart series applies that
subtraction at write time, so two surfaces carried two quantities under one
name. The adjacent `circulating_supply` in the very same handler already read
`genesis_baseline()` correctly, which is what made the divergence invisible.

**Fix**: One `common_knowledge_size(snapshot, virtual_occupied)` helper shared
by `/statistics/network`, `/statistics/asset-ecosystem` and the chart path; a
result below zero is a hard error naming the date, `U` and `virtual_occupied`.
The frontend clamp is replaced by a labelled allocation-error box.

**Lesson**: A derived quantity needs one function, not one formula repeated at
each call site — repetition is how two call sites end up implementing two
different definitions of the same word. When a UI clamps a value to keep a bar
renderable, the clamp is hiding the arithmetic that proves the value wrong.

---

### API-006: Silent default-empty plus a full-scan fallback hid a post-reorg gap

**Date**: 2026-08-02

**Symptom**: For ~35-40 seconds after every reorg, `/dao/top-depositors`
returned HTTP 200 with an empty leaderboard. Captured live on testnet across
three reorgs in one evening.

**Root Cause**: Rollback deleted the DAO singleton rows, and the indexer
re-derived them only after that batch committed — a real ~6s window with no
row. The handler masked the absence with
`unwrap_or_else(|| DaoTopDepositors { depositors: vec![], .. })`, then cached
that empty response for the full 30s TTL. `/dao/statistics` hid the same state
behind a silent full-scan recompute — a fallback chain that had additionally
_drifted_ from the indexer's own treasury/compensation formulas, so the two
paths would have disagreed had anyone compared them.

**Fix**: Rollback no longer deletes the singletons (they are tip-scoped rows
the indexer rewrites wholesale right after every rollback, every batch commit,
and unconditionally at startup, so deleting them bought nothing); the read path
fails fast when a singleton is genuinely missing at a synced tip and reports
`initializing` before the first block; absent/failed states are never cached;
the `/dao/statistics` recompute fallback is deleted.

**Lesson**: A silent default-empty turns a transient write-path gap into a
plausible-looking answer, and a fallback recompute keeps it invisible while
quietly drifting from the path it shadows. Both are the forbidden pattern; the
gap itself is the bug to close.

---

### API-007: Proposal scans ignored uncle proposal zones

**Date**: 2026-08-02

**Symptom**: `/transactions/{hash}/lifecycle` returned `proposedIn: null` and
`commitmentDistance: null` for consensus-valid committed transactions, so the
tx page silently dropped the "Proposed" step of two-step commitment. Measured
at 33 of 58,687 committed txs (~0.05-0.07%) in uncle-bearing eras.

**Root Cause**: The `[commit−10, commit−2]` window scan read only
`block.data().proposals()` for each main-chain block and never
`block.uncles()[i].proposals()` — even though CKB consensus counts uncle
proposal zones and the uncle data already travelled inside the very block
objects the loop had fetched. `/graph/proposals/{block_number}` had the same
defect.

**Fix**: Both scans walk embedded uncles. `proposedIn` reports the
**containing main-chain block** (the block whose proposal zone the uncle
contributes to, and the block the commitment window is measured against), so
`commitmentDistance` stays consensus-meaningful; uncle identity is surfaced as
an explicit additional field.

**Lesson**: Consensus rules that admit two sources for one fact (main
proposals ∪ uncle proposals) need both sources read, and the derived field
must stay anchored to the entity the rule is measured against — reporting the
uncle's own number would have produced a distance corresponding to no rule.

---

### API-008: Address resolution that depended on having a live cell

**Date**: 2026-08-02

**Symptom**: `/addresses/{lock_hash}` returned `address: null` with no
`lockScript` for any fully-spent address (completed DAO withdrawers, emptied
wallets) despite showing a non-zero transaction count, while
`/dao/deposits?status=2` resolved the very same locks to full addresses.
Separately, `/tokens/{type_hash}/holders` returned `address: null` for every
holder.

**Root Cause**: Two divergent resolution paths. The address handler derived the
lock script from _one live cell_ instead of `get_lock_script`, so it degraded
to null exactly when an address had no live cells. The holders handler simply
hardcoded `address: None` while the module's own `resolve_lock_addresses` —
used by the sibling transfers/activities handlers — sat unused.

**Fix**: Both handlers use the stored lock script and the shared resolver;
`CF_LOCK_SCRIPTS` is written by both sync paths from the same fields the old
code read off the cell and is never deleted, so the resolution is exact and
outlives the cells. The `_ => "data"` hash-type fallback is replaced by a
fail-fast conversion.

**Lesson**: Deriving a durable fact from a transient artifact (a live cell)
gives an answer that disappears with the artifact. When one module already has
a resolver, a second call site that returns null is not a missing feature — it
is a second path that will drift.

---

### API-009: Failed script replays were harvested as authoritative cycle counts

**Date**: 2026-08-02

**Symptom**: Every Nervos DAO phase-1 and phase-2 transaction served wrong
cycles as `status: done`. Example: phase-2
`0x6fa94cb21df82144505c5a9e5d3197e431ea0296a09c55a3e83e669f9ac01ab9` served
3,374,403 against a consensus-true 3,380,228. Genesis transactions served
15,511 for a run that had in fact failed.

**Root Cause**: Two composed defects. The mock transaction carried no header
association, so the DAO type script's `load_header(source=Input)` hit
ItemMissing and the group aborted after ~8k cycles. And the runner never
checked the child exit code or the `Run result:` line, so the aborted group's
partial count was summed into the total and persisted as a completed value —
which no later request would recompute.

**Fix**: `MockInput`/`MockCellDep` carry a required committing-block hash
(an unresolvable one is an invariant violation, not a `None`), and a group is
accepted only when `Run result: 0` _and_ exit code 0; anything else is an
error, which the existing worker persists as the `-1` failed marker.

**Lesson**: An external verifier's exit status is part of its answer. Parsing
its stdout for a number while ignoring "this run failed" converts an error into
a fact — and persisting that fact as `done` makes it permanent. Note the fix
does not heal stored values: they clear on re-sync.

---

### API-010: Three more unvalidated block numbers aborted the whole process

**Date**: 2026-08-02

**Symptom**: `GET /api/v1/blocks?limit=2&cursor=0` dropped the connection and
took the entire mainnet API down — every endpoint, every client — until the
supervisor restarted it (~40-60s). Verified live: `?cursor=1` answered 200,
`?cursor=0` made the listening PID vanish. Two sibling vectors were found by
sweeping the class: `/dao/calculator?deposit_block=-1` and
`/transactions?block_number=-1`.

**Root Cause**: An unvalidated negative reaches `keys::encode_block_num`, whose
`assert!(n >= 0)` is a correct internal invariant — but the release profile
sets `panic = "abort"` and the API has no catch-panic layer, so the assert
kills the process instead of the request. `/blocks` computed `cursor - 1`
(so `cursor = 0` produced `-1`, and `i64::MIN` wrapped to `i64::MAX` in
release, silently serving the newest page); `/dao/calculator` only compared
`withdraw < deposit`, which `-1` passes; `/transactions` did call
`validate_block_number` — 21 lines _after_ it had already looked the header up.

**Why a green test did not protect `/transactions`**: its regression test
asserted 400 and passed for months. Under the test profile a panic unwinds,
and the handler swallowed the result twice (`get_block_header(..).ok()` inside
a `spawn_blocking` whose `JoinError` was `.unwrap_or(0)`-ed), so control
reached the later validation and returned a reassuring 400 — while the release
binary was already dead. The fix therefore validates first _and_ deletes the
swallowing, so an ordering regression now surfaces as a 500 rather than a
passing test.

**Fix**: One validating parser at the boundary for the `/blocks` cursor
(returning the resolved scan start, so no caller can hold a value that could
go below genesis), `validate_block_number` hoisted above every store access in
the other two handlers, and the silent guards that laundered the panic removed.
The `keys.rs` assert is deliberately untouched.

**Lesson**: An internal `assert!` plus `panic = "abort"` makes every missing
boundary check a remote denial of service, so the boundary sweep has to be
exhaustive by construction rather than by memory — this is the third round in
which this family resurfaced, each time in the one shape the previous sweep's
tests did not cover (path params, then hash lengths, now bare integer query
params). And a test that asserts on a response cannot prove a panic did not
happen: if the code under test swallows failures, the test profile will hide
the crash the release profile takes.

---

### API-011: A checksum variant accepted, then echoed back as canonical

**Date**: 2026-08-03

**Symptom**: `/addresses/{addr}` answered 200 with full balance and cell counts
for an address whose checksum is invalid under RFC-0021 (the burn payload
carrying a legacy Bech32 checksum where the 0x00 full format mandates
Bech32m), and returned that invalid string back to the caller in the response's
`address` field. The official explorer 404s the same string. Separately,
an all-uppercase address — legal per the bech32 case rules — fell through to
the hex-hash branch and produced a 400 complaining about a 32-byte hex hash.

**Root Cause**: `bech32::decode` is checksum-variant agnostic, so the parser
verified only that the payload's format byte was 0x00, never that the encoding
that carried it was the one the format requires. The response then preferred
the caller's raw input over re-encoding the resolved lock script, so a rejected
string became a published one.

**Fix**: Decode explicitly under Bech32m for the full format, rejecting the
legacy variant with an error naming the reason; accept either case as the spec
allows; and always render `address` from the resolved lock script on the
serving network, never from the input.

**Lesson**: Echoing input into a response field makes the API an authority on
a string it never validated. The canonical form has to be derived from the
decoded value, which incidentally makes the wrong-checksum bug impossible to
reintroduce quietly — the derived form simply would not match.

### API-012: Two endpoints, one fact, one of them silent

**Date**: 2026-08-03

**Symptom**: `/blocks/{id}/proposals` reported `committedTxHash: null` for all
1,500 proposals of block 11988763, of which 1,410 were in fact committed within
the commit window, while `/graph/proposals/{block_number}` resolved those very
commitments for the same block.

**Root Cause**: The proposals handler hardcoded the field to `None` pending "a
dedicated proposal-to-tx reverse index", but the sibling graph endpoint already
answered the question by scanning the commit window. Consumers could not tell
"never committed" from "not computed".

**Fix**: The window scan was extracted into one helper used by both endpoints,
with the graph endpoint's matching semantics pinned by a test first so the
extraction provably changed nothing there. A proposal with no match in the
available window stays null, which is now an honest answer rather than a
placeholder.

**Lesson**: A field that is always null is indistinguishable from data, and
the justification for it ("no index exists") went stale the moment a sibling
handler computed the same thing a different way. Documented omissions need a
periodic check that they are still omissions.

### API-013: An empty array where there was no answer

**Date**: 2026-08-03

**Symptom**: `/transactions/{hash}/cell-deps` returned 200 with an empty array
both when the transaction did not exist and when the CKB store backing the
lookup was unavailable — the same response a transaction with genuinely no
cell deps would produce.

**Root Cause**: The handler mapped both failure paths to `ok(vec![])`.

**Fix**: A missing transaction is a 404 and an unavailable data source is a
500 naming what is missing; a reader that lags behind a node that reports the
transaction as committed also fails loudly rather than reporting absence.

**Lesson**: The empty collection is a legitimate answer for one question only —
"what are this transaction's cell deps" — so using it for "I could not tell"
destroys the distinction at exactly the point a client would have retried.

## Category: Ecosystem Protocol Detection

### PROTO-001: One payload layout applied to every UTXOSwap intent type

**Date**: 2026-08-03

**Symptom**: Roughly 23% of mainnet UTXOSwap protocol metadata (and 28% of
testnet) recorded amounts around 1.7e38 — add-liquidity and remove-liquidity
actions showed `amountIn: 170141183460469231731687303715884144673` where the
transaction had moved 9,969,978 shannons.

**Root Cause**: `parse_intent_args` had one catch-all branch that applied the
swap layout (`args[57]` index, `args[58..74]` and `args[74..90]` as u128s) to
every intent type that was not CreatePool. On chain the payload is per-type:
AddLiquidity is 121 bytes of four u128s, RemoveLiquidity 105 bytes of three,
and only the two swap types match the assumed shape. Reading a u128 one byte
late swallows the next field's low byte as its own high byte, which is why
every wrong value was `2^127 + (true_value >> 8)`.

**Fix**: Per-type decoding in a single decoder hoisted into `ckbadger-common`
and shared by the indexer parser and the API's live lock-args display, which
had carried its own copy of the same bug. Field names now describe what each
type actually holds; an unknown type or a length mismatch records a typed
`Unparsed` marker instead of borrowing another type's layout.

**Lesson**: A catch-all match arm over a protocol's message types is a claim
that every unlisted variant shares one layout — a claim no one verified here.
The field identities were confirmed against what the transactions actually
produce (the intent cell's own capacity and paired UDT amount), not inferred
from plausible-looking byte patterns.

### PROTO-002: Per-participant inference presented as transaction-level fact

**Date**: 2026-08-03

**Symptom**: One Stable++ transaction was labeled `borrow`, `adjust` and
`repay` at once; every one of the 68 vault closures in mainnet history also
carried `liquidation`; and about 89.5% of transactions labeled `redemption`
were ordinary RUSD transfers or DEX swaps that never touched a vault.

**Root Cause**: The detector ran once per participating owner and derived the
action from that owner's own balance deltas, so a transaction with several
participants emitted several mutually exclusive labels for one on-chain event.
The truth table also mapped any nonzero RUSD delta without a vault to
"redemption", and an `input_capacity == 0` early return skipped pure-receiver
owners, which is exactly the borrower in a borrow.

**Fix**: Classification is computed once per transaction from transaction-level
facts (vault cells in and out, intent cells consumed, RUSD supply delta). A
transaction that touches no vault, pool, or intent cell now emits no Stable++
action at all. `liquidation` was removed rather than repaired: all 68 closures
consume an intent belonging to the closing vault's own owner, so no chain
discriminator for a forced liquidation exists, and a label that fires on every
closure carries no information. `redemption` now requires RUSD actually
destroyed against a consumed intent, which currently fires zero times.

**Lesson**: When the thing being described is a property of the transaction,
computing it from one participant's view guarantees contradictions the moment
a second participant exists. And an emitted label must have a chain fact that
distinguishes it — inventing "liquidation" from the shape of a normal close
made the data actively misleading rather than merely incomplete.

### PROTO-003: A protocol pipeline nothing could reach

**Date**: 2026-08-03

**Symptom**: 421 live testnet and 32 live mainnet did:ckb identity cells were
absent from every surface — `/assets/identities/did_ckb` returned 404, item
detail 404, search empty — while the store schema, writer paths, aggregates
and API routes for exactly that data all existed and compiled.

**Root Cause**: The script registry mapped the `did-ckb` metadata slug to
`ProtocolScript::DidCkb`, but every detection site tested
`ProtocolScript::SporeDid`, a legacy variant no slug maps to. The predicate
was therefore permanently false and the entire downstream pipeline — insert,
consume, aggregate emit, live sentinels, writer paths — was unreachable code.

**Fix**: A dedicated `DidCkbParser` resolving the real registry variant, wired
symmetrically through the live and bulk paths. Wiring it exposed a second
defect the dormant code had hidden: 31 of the 421 cells carry 20-byte type
args, and both the outpoint reverse index and the forward map assumed exactly
32 bytes, so the reverse index key became variable-width (ids stay verbatim;
scans filter on exact key length and exact id bytes) and the forward map
stopped silently returning `None` for shorter ids.

**Lesson**: Dead code does not merely fail to run, it fails to be tested
against reality — this pipeline had never met a real cell, so its fixed-width
assumption survived unchallenged until classification was switched on. A
protocol is only integrated when something asserts its cells are indexed;
until then, shipped routes and schema are evidence of intent, not of function.

### PROTO-004: Curated labels attached to a hash the chain never resolves

**Date**: 2026-08-03

**Symptom**: The Fiber Funding and Commitment lock families reported zero
cells against 5,021 live funding cells on testnet, and `/scripts/lookup` for
the RGB++ BTC-testnet3 deployment answered with the signet deployment's
numbers (680 cells instead of 12,486) including signet's code cell.

**Root Cause**: Two independent faults that compound. In the metadata, ten
entries set a version's identity to the canonical reference hash — a type
script hash, which can never equal a code cell's bytecode data hash — while
the usage rollup attributes a reference to a version by reading the live code
cell's actual data hash, so the labeled version received nothing. In the API,
a version carried a single `associated_code_hash` slot used to redirect stats
lookups, but one bytecode is deployed under several independent references, so
the last label written (signet) answered for all of them.

**Fix**: The single-slot redirect is deleted rather than re-keyed, since no
one slot can express a one-to-many relation and the per-reference rollup
already holds the answer. Label import now validates a declared version
against the shapes a version hash can actually take, naming the offending TOML
and skipping the attachment instead of silently zeroing a family. The ten
metadata entries were corrected against code cells read from the node.

**Lesson**: Curated metadata is an input that can be wrong, so the code that
consumes it needs the same fail-loud posture as any other untrusted input —
here a placeholder silently produced a plausible zero, which reads as "this
script is unused" rather than "this label never matched anything".

### PROTO-005: Endianness copied from a fixture instead of the contract

**Date**: 2026-08-03

**Symptom**: mNFT item #1 displayed as token index 16777216 across all 5,209
classes; the official explorer showed 0, 1, 2 for the same items.

**Root Cause**: The parser read the 4-byte token index little-endian while the
official contract's `parse_type_args_id` reads it big-endian. The parser's own
test fixtures constructed args with `to_le_bytes`, so the tests agreed with
the bug and would have kept agreeing forever.

**Fix**: Big-endian in both parse paths, with the fixtures rebuilt from the
contract's definition, in the bulk-build tests as well.

**Lesson**: A test that builds its input with the same assumption as the code
under test asserts only self-consistency. For a format defined by someone
else, the fixture has to come from the external definition — the contract
source or bytes captured from the chain — or it is not a test of correctness.

### PROTO-006: An unsupported encoding recorded as a permanent failure

**Date**: 2026-08-03

**Symptom**: 24 of 40 sampled testnet DOB clusters failed to decode entirely
("failed to extract DNA hex from spore content"), and the failures were
persisted as deterministic so nothing would ever retry them.

**Root Cause**: The DOB spec defines a raw-binary DNA form (content byte zero
is `0x00`, DNA is the remaining bytes); the extractor only handled the text
and JSON forms, running the content through a lossy UTF-8 conversion first.
A related dispatch defect chose the protocol version from the spore's
`content_type` with a silent `unwrap_or(0)` default, where the reference
implementation dispatches on the cluster's declared `dob.ver`.

**Fix**: One shared extractor that handles all three content forms, and
cluster `dob.ver` as the dispatch authority with an undeclared version now a
typed failure rather than a silent version 0. Three read-path
re-implementations of decoder rules (range width, segment modulo, option
selector precedence) were also brought to the reference behavior; one existing
unit test had encoded the nonconforming precedence and was replaced.

**Lesson**: Classifying a decode failure as deterministic is a statement that
the input, not the decoder, is at fault — so it must not be reachable by an
input shape the decoder simply does not implement, or the classification turns
a missing feature into permanent data loss that a rebuild alone cannot heal.

---

### PROTO-007: A protocol wired into the live classifier only

**Date**: 2026-08-04

**Symptom**: Every did:ckb identity was missing — 421 live cells on testnet
(390 with 32-byte args, 31 with 20-byte), 113 on mainnet — after a full
from-genesis re-sync on the binary that had just fixed the did:ckb pipeline
(PROTO-003). The collection, every item id, and search all returned 404 while
the underlying cells were indexed and readable through `/cells/{tx}/{i}`.

**Root Cause**: `code_hash_to_semantic_tag` in `bulk_build/binary_facts.rs`
matched the protocol registry with a trailing `_ => CellSemanticTag::Plain`
arm and had no `ProtocolScript::DidCkb` case, so bulk build classified every
did:ckb cell as a plain cell and never produced protocol facts for it. Both
live-sync classifiers in `sync/pipeline.rs` did handle it. A from-genesis
re-sync runs essentially all of history through the bulk path, so the fix that
shipped was inert in the only mode that mattered.

**Fix**: Added the missing arm and removed the catch-all — the match is now
exhaustive over `ProtocolScript`, so a new variant fails to compile instead of
silently degrading to `Plain`. Added a cross-path parity test that classifies
every registered code hash through both the bulk and live classifiers and
requires them to agree; it reproduced the outage as the sole divergence.

**Lesson**: A wildcard arm over a protocol enum turns "unhandled" into
"ordinary". Where two code paths must classify the same thing, per-protocol
unit tests prove nothing unless one test drives both paths with the same
input — the did:ckb unit tests were green throughout, because they only ever
exercised the live classifier.

---

### DAO-028: The same shannons in two buckets of one partition

**Date**: 2026-08-04

**Symptom**: `/dao/statistics` (`miningReward + depositCompensation + burnt`)
and the Secondary Issuance chart (`mining + compensation + burnt`) exceeded the
protocol's total secondary issuance by 77,489,937.99 CKB on mainnet and
87,894.98 CKB on testnet. The protocol identity `mining + claimed ==
Σsecondary − S_tip + S_genesis` held at zero shannons, so the miner and claimed
series were exact and the entire excess sat in the treasury/compensation split.

**Root Cause**: `unmade_dao_interests` was written as
`DaoCompensationBreakdown::active_unmade` — the status-0 share only — and
treasury was derived as `S − unmade_dao_interests`. But RFC-0023 removes a
deposit's interest from `S` only when the phase-2 completion transaction
subtracts `withdrawed_interests`, so interest already frozen on a phase-1
withdraw-request cell is still inside `S`. That frozen amount was therefore
left in treasury while `cum_dao_compensation` (= claimed + unclaimed) already
counted it. Four sites derived treasury independently (live writer, bulk
reducer, store recompute, API read path), so the definition could drift.

**Fix**: One derivation, `dao_treasury_split(secondary_pool, unclaimed)` in
`ckbadger-store`, used by all four sites via
`DaoCompensationBreakdown::treasury`. The duplicate
`DaoDailySnapshot::unmade_dao_interests` field and the redundant
`compute_unmade_dao_interests` scan were deleted, leaving `unclaimed_compensation`
as the single stored quantity. `active_unmade` survives only as a diagnostic
and is documented as never valid for the treasury split. An API-statistics
fixture that had encoded the overlap as intended behavior was corrected.

**Lesson**: When a breakdown is presented as a partition, assert that its parts
reconstruct the whole — `treasury + unclaimed == S` is a one-line invariant
that would have failed the moment the wrong summand was chosen. Two stored
fields that differ only in a subtle qualifier are an invitation to pick the
wrong one.

---

### PROTO-008: A capacity heuristic gating on-chain evidence

**Date**: 2026-08-04

**Symptom**: Over 437 sampled mainnet UTXOSwap transactions, 6 of 228 on-chain
intent cells produced no `*_submitted` action and 8 of 228 consumed intent
cells produced no `*_settled` action.

**Root Cause**: Two independent defects. (1) The submitted branch was wrapped in
`if ckb_delta < 0`, requiring the intent owner's net capacity delta to be
negative — but an intent cell's capacity can be funded by anyone, and a
batching transaction can return more capacity than the submitter spent. Mainnet
tx `0x8fa2c828…` submits two intents and the owner of the dropped one ended
`+8,799,994,175` shannons. (2) All detector output was deduplicated by
`(protocol, action, metadata)` to collapse the copies a tx-level detector emits
once per participating owner. UTXOSwap is not tx-level: it emits one action per
intent cell, and mainnet tx `0xffedf80f…` settles five cells owned by five
different addresses whose payloads are byte-identical — five events reported as
one.

**Fix**: Deleted the capacity gate; attribution now rests solely on the owner
prefix the intent records on chain, exactly as the settled branch already did.
Added `ProtocolDetector::emits_tx_level_actions`, defaulting to the tx-level
behavior, so only detectors that genuinely summarise one event per transaction
are deduplicated across owners; UTXOSwap declares per-cell emission and joins
the per-cell DAO actions that were already exempt.

**Lesson**: Deriving a fact from a capacity heuristic when the chain states it
directly is guaranteed to be wrong at the margins. And a deduplication key must
identify the _event_: `(protocol, action, metadata)` cannot tell "one event seen
by two owners" from "two events that happen to look alike".

---

### API-014: A size that could not reproduce its own fee rate

**Date**: 2026-08-04

**Symptom**: `/transactions/{hash}/detail` served `txSize: 981` and
`feeRate: 2105` for mainnet tx `0xdf615176…`, but `981` cannot produce `2105` —
`2074 * 1000 / 981` is `2114`. The official explorer reported `bytes: 985` for
the same transaction.

**Root Cause**: Fee rates divided by the protocol's serialized size in block
(molecule size + the 4-byte offset slot the transaction occupies in the block's
transactions table), while the `txSize` / `size` fields served the bare molecule
size. The divergence was deliberate and documented, but the frontend recomputes
`fee / txSize` in three places, so the UI displayed a fee rate that disagreed
with the API's own field for the same transaction, and both sizes disagreed with
the node and explorer.

**Fix**: One convention everywhere the API serves a size — `txSize`, the block
`fee-stats.totalSize`, and the fee-rate denominator all use the serialized size
in block, converted once at the response boundary (the store still holds
molecule sizes). Two tests that pinned the old split convention were updated,
and the detail test now asserts the fields reproduce each other.

**Lesson**: If two fields in one response are related by arithmetic, a client
will do that arithmetic. Serving them on different conventions makes the
response internally false no matter how well the difference is documented.

---

### API-015: A deterministic value served as null

**Date**: 2026-08-04

**Symptom**: `/dao/deposits?status=1` returned `compensation: null` for all
1,660 mainnet and 1,062 testnet pending withdraw requests, even though those
rows' compensation totalled the 77.49M CKB that DAO-028 traced through the
aggregates.

**Root Cause**: The row mapper served the stored `entry.compensation`, which is
only populated at phase-2 completion. A phase-1 request has already frozen its
compensation at the request AR, and the same process derives it for every
aggregate through `dao_frozen_request_compensation` — the listing simply never
asked.

**Fix**: The listing derives compensation through that same helper as soon as a
withdraw request exists, so the listing and `/dao/statistics` cannot disagree.

**Lesson**: `null` should mean "no answer exists", not "this code path did not
compute the answer that another path in the same process already has".

---

_Last updated: 2026-08-04_
