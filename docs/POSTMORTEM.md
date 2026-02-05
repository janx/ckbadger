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

**Root Cause**: `update_dao_daily_snapshot` fetched `cumulative_burnt` from `dao_statistics WHERE id = 1`, which contains the **current** cumulative value, not the historical value for that specific date. During batch sync, all historical snapshots received the same cumulative_burnt (the value at sync time).

```rust
// WRONG: Uses current cumulative, not historical
let secondary_issuance = sqlx::query_as::<_, (String, String, String)>(
    "SELECT cumulative_burnt, ... FROM dao_statistics WHERE id = 1"
)

// CORRECT: First check previous day's snapshot for historical continuity
let prev_snapshot = sqlx::query_as::<_, (String, String, String)>(
    "SELECT cumulative_burnt, ... FROM dao_daily_snapshots WHERE date = $1"
).bind(date - 1.day())

// Fall back to dao_statistics only if no previous snapshot
let secondary_issuance = prev_snapshot.unwrap_or_else(|| /* fetch from dao_statistics */);
```

**Lesson**: When creating historical snapshots during batch sync, cumulative values must be derived from previous snapshots, not from the current aggregate table. The aggregate table reflects the tip, not historical state.

**Reference**: Similar to STATS-001 (cumulative values wrong for new days).

**Files**: `crates/indexer/src/db/writer.rs`, `crates/indexer/src/db/writer_v2.rs`

---

### DAO-014: Secondary issuance burnt percentage regression (fbda36a)

**Symptom**: Secondary issuance chart showing abnormally low burnt percentage (~35% instead of ~65-70%).

**Root Cause**: Commit `5ce76af` correctly fixed the issue with simple logic:

```sql
WHERE deposit_timestamp::date <= $1
  AND (withdraw_timestamp IS NULL OR withdraw_timestamp::date > $1)
```

But 8 minutes later, commit `fbda36a` replaced it with complex status-based logic:

```sql
WHERE deposit_timestamp::date <= $1
  AND (
      status = 0
      OR (status = 1 AND (withdraw_request_timestamp IS NULL OR withdraw_request_timestamp::date > $1))
      OR (status = 2 AND withdraw_timestamp IS NOT NULL AND withdraw_timestamp::date > $1)
  )
```

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

**Root Cause**: The calculation used **current** `total_dao_deposits` (queried with `WHERE status = 0`) for all historical blocks, instead of the deposit amount **at that block's time**.

```rust
// WRONG: Uses current deposits for all blocks
let total_dao_deposits = sqlx::query("SELECT SUM(capacity) FROM dao_deposits WHERE status = 0");
let dao_compensation = non_miner * total_dao_deposits / (C - U);
```

This caused early blocks to use inflated deposit values (~22% of total issuance) instead of the actual lower values at that time (~5-10%), resulting in overestimated `dao_compensation` and underestimated `burnt`.

**Failed fix attempt**: Tried using DAO field `S` (secondary_pool) difference between blocks. But `S` is "total unissued secondary issuance" which equals `non_miner - claimed_compensation`, not `dao_compensation`. This made burnt nearly 0%.

**Correct fix**: Query deposits active at that specific block number:

```rust
// CORRECT: Query deposits at that block's point in time
let total_dao_deposits = sqlx::query(
    "SELECT SUM(capacity) FROM dao_deposits
     WHERE deposit_block_number <= $1
       AND (withdraw_block IS NULL OR withdraw_block > $1)"
).bind(block_number);

// RFC-0015 formula: dao_compensation = non_miner * deposit / (C - U)
let dao_compensation = non_miner * total_dao_deposits / (C - U);
let burnt = non_miner - dao_compensation;
```

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

**Root Cause**: `get_dao_deposits_at_block` had two off-by-one errors in the SQL query.

Per RFC-0023, block N's secondary issuance distribution uses `U_{i-1}` and `C_{i-1}` (previous block's state). So for calculating block N's distribution, we need deposits active at end of block N-1.

```sql
-- WRONG: Two off-by-one errors
WHERE deposit_block_number <= $1    -- includes deposits at block N (not yet active)
  AND (withdraw_block IS NULL OR withdraw_block > $1)  -- excludes withdrawals at block N (still active)

-- CORRECT: Use state at end of N-1
WHERE deposit_block_number < $1     -- deposited before block N
  AND (withdraw_block IS NULL OR withdraw_block >= $1)  -- not withdrawn before block N
```

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

## Category: Statistics & Charts

### STATS-001: Cumulative values wrong for new days (19ee513)

**Symptom**: `daily_statistics.cumulative_cells` and `cumulative_data_size` showed incorrect values, sometimes negative or resetting.

**Root Cause**: INSERT path was using only today's delta instead of `prev_cumulative + delta`. The ON CONFLICT UPDATE path was correct, but INSERT was wrong.

```sql
-- WRONG: INSERT with just delta
VALUES ($1, ..., $3 - $4, $5 - $6)

-- CORRECT: Fetch previous cumulative, add delta
SELECT cumulative_cells, cumulative_data_size
FROM daily_statistics WHERE date < $1 ORDER BY date DESC LIMIT 1;
-- Then INSERT with prev + delta
```

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

### STATS-007: Common Knowledge Size only accumulated, never decremented

**Symptom**: "Common Knowledge Size" chart shows monotonically increasing values (~1.7GB), even though cells are constantly being consumed. The official CKB explorer shows ~156MB. This means `cells.consumed_at_block` was almost never being set (only 0.03% of cells had it populated).

**Initial Analysis**: Thought `data_size_consumed` wasn't being calculated in statistics. But the code was updated correctly to use `net_data_size = data_size_added - data_size_consumed`.

**Actual Root Cause**: During bulk sync, cell lookup in `get_cells_info_batch()` silently failed for most cells:

1. Check RocksDB `live_cell_store` → miss (not populated during bulk sync startup)
2. Check `consumed_cells` cache → miss (recently consumed only)
3. Fallback to `live_cells` table → **only 4,836 records!** (not populated in bulk sync mode)
4. **Missing**: No fallback to `cells` table

Since lookups failed, `consume_cells_batch()` never received consumption data, so `cells.consumed_at_block` was never set (except for a tiny fraction).

**Fix** (Feb 2026):

1. Added fallback to `cells` table in `get_cells_info_batch()`:

   ```rust
   // STATS-007 fix: Fallback to cells table for bulk sync mode
   let fallback_rows = sqlx::query_as(
       "SELECT ... FROM cells c WHERE c.status = 0 ..."
   )
   ```

2. Created `consumed_at_backfill` task to backfill historical data:

   ```sql
   UPDATE cells c SET
       status = 1,
       consumed_at_block = ti.tx_block_number,
       consumed_by_tx = ti.tx_hash
   FROM transaction_inputs ti
   WHERE c.tx_hash = ti.previous_tx_hash
     AND c.output_index = ti.previous_output_index
     AND c.consumed_at_block IS NULL
   ```

3. After backfill: run `statistics_rebuild` task to recalculate `daily_statistics.total_data_size`

**Lesson**: Multi-stage lookups with fallbacks must be tested with realistic data. Silent failures (returning empty results instead of errors) mask critical bugs. Add logging when fallback queries return significant results.

**Files**: `crates/indexer/src/db/writer/cells.rs`, `crates/task-runner/src/executor/consumed.rs`

---

## Category: Database & SQL

### DB-001: Foreign key constraint violations (17fefa5)

**Symptom**: Indexer failing with FK constraint errors during block processing.

**Root Cause**: Insert order violated FK constraints. Was inserting inputs (which reference transactions) before inserting the transaction itself.

**Correct order**:

1. Insert transaction (parent)
2. Insert outputs/cells
3. Insert inputs and consume cells (after tx exists)

**Lesson**: When tables have FK relationships, always insert parent records before children. Map out the dependency graph before implementing.

**Files**: `crates/indexer/src/sync/indexer.rs`

---

### DB-002: NUMERIC to float8 type mismatch (17fefa5)

**Symptom**: SQL query error on statistics endpoint.

**Root Cause**: PostgreSQL `AVG()` on timestamps returns `NUMERIC`, but Rust was expecting `f64`. Need explicit cast.

```sql
-- Wrong
SELECT AVG(EXTRACT(EPOCH FROM ...))

-- Correct
SELECT AVG(EXTRACT(EPOCH FROM ...))::float8
```

**Lesson**: PostgreSQL aggregate functions may return unexpected types. Always check return types and cast explicitly.

**Files**: `crates/api/src/routes/statistics.rs`

---

### DB-003: SQL parameter type casting for numeric (cf6b6e0)

**Symptom**: SQL errors when accumulating DAO statistics.

**Root Cause**: String parameters being used in numeric operations need explicit `::numeric` cast.

**Lesson**: When passing string representations of numbers to PostgreSQL for arithmetic, cast them explicitly.

**Files**: `crates/indexer/src/db/writer.rs`

---

### DB-004: CREATE INDEX CONCURRENTLY in SQLx migrations

**Symptom**: Migration fails with `CREATE INDEX CONCURRENTLY cannot run inside a transaction block`.

**Root Cause**: SQLx runs migrations inside transactions. PostgreSQL's `CREATE INDEX CONCURRENTLY` cannot run inside a transaction because it uses a special two-phase locking protocol.

```sql
-- WRONG: Fails in SQLx migration
CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_foo ON bar(col);

-- CORRECT: Use regular CREATE INDEX
CREATE INDEX IF NOT EXISTS idx_foo ON bar(col);
```

**Lesson**: Don't use `CONCURRENTLY` in SQLx migrations. If you need non-blocking index creation on a large production table, run it manually outside the migration system.

---

### DB-005: NUMERIC column to String type mismatch (c686e83)

**Symptom**: Cell page and graph endpoints returning 500 errors.

**Root Cause**: Capacity column is `NUMERIC` in PostgreSQL but Rust struct expects `String`. SQLx doesn't auto-convert.

```sql
-- WRONG: Returns NUMERIC, Rust expects String
SELECT capacity FROM cells WHERE ...

-- CORRECT: Cast to TEXT
SELECT capacity::TEXT FROM cells WHERE ...
```

**Lesson**: When database columns store large numbers as NUMERIC (for precision) but API returns them as strings (for JSON compatibility), explicitly cast in SQL. This is common for blockchain amounts that exceed JavaScript's Number.MAX_SAFE_INTEGER.

**Files**: `crates/api/src/routes/cells.rs`, `crates/api/src/routes/graph.rs`, `crates/api/src/routes/search.rs`

---

### DB-006: N+1 query in DAO statistics (52992a4)

**Symptom**: Slow DAO statistics calculation during indexing.

**Root Cause**: `recalculate_dao_extended_statistics` was fetching block DAO fields individually for each deposit to calculate compensation.

```rust
// WRONG: N+1 queries
for deposit in deposits {
    let ar_deposit = fetch_block_dao(deposit.block_number);
    let ar_withdraw = fetch_block_dao(deposit.withdraw_block_number);
}

// CORRECT: Single JOIN query
SELECT d.*, b1.dao as deposit_dao, b2.dao as withdraw_dao
FROM dao_deposits d
JOIN blocks b1 ON d.deposit_block_number = b1.number
LEFT JOIN blocks b2 ON d.withdraw_request_block_number = b2.number
```

**Lesson**: Batch operations on related data. Use JOINs to fetch associated records in one query rather than iterating.

**Files**: `crates/indexer/src/db/writer.rs`

---

### DB-007: Querying empty table instead of cells.data (096a7d5)

**Symptom**: Token page holders list and total supply showing empty/zero.

**Root Cause**: Token queries referenced `cell_data` table which was empty. The actual cell data is stored in `cells.data` column.

```sql
-- WRONG: Querying separate table (empty)
SELECT cd.data FROM cell_data cd WHERE cd.tx_hash = c.tx_hash

-- CORRECT: Query cells table directly
SELECT c.data FROM cells c WHERE c.status = 0 AND ...
```

**Lesson**: When refactoring schema (e.g., moving data from separate table to column), update ALL queries that reference the old structure. Search codebase for all usages.

**Files**: `crates/api/src/routes/tokens.rs`

---

### DB-008: UDT amount parsing in SQL (096a7d5)

**Symptom**: Token total supply showing incorrect values.

**Root Cause**: UDT amounts are stored as little-endian u128 in first 16 bytes of cell data. PostgreSQL doesn't have u128, so need to parse manually.

```sql
-- Parse little-endian u128 from cell data (first 8 bytes for u64)
SELECT SUM(
    get_byte(c.data, 0)::bigint
    + get_byte(c.data, 1)::bigint * 256
    + get_byte(c.data, 2)::bigint * 65536
    + get_byte(c.data, 3)::bigint * 16777216
    + get_byte(c.data, 4)::bigint * 4294967296
    + get_byte(c.data, 5)::bigint * 1099511627776
    + get_byte(c.data, 6)::bigint * 281474976710656
    + get_byte(c.data, 7)::bigint * 72057594037927936
) FROM cells c WHERE ...
```

**Lesson**: Binary data parsing in SQL is possible but verbose. Consider whether to parse in SQL (single query) or application (cleaner code, multiple queries). For aggregates across many rows, SQL parsing avoids N+1.

**Files**: `crates/api/src/routes/tokens.rs`

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

### BUILD-003: Missing migrations in Docker (bdb7e17)

**Symptom**: Indexer Docker container failing - can't find migrations.

**Root Cause**: Forgot to COPY migrations directory in Dockerfile.

**Lesson**: Always verify all runtime dependencies are included in Docker builds. Create a checklist for each service's required files.

**Files**: `docker/Dockerfile.indexer`

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
| SQL         | Cumulative values                   | Test INSERT and UPDATE paths separately                  |
| SQL         | Type mismatches                     | Cast explicitly: `::float8`, `::numeric`, `::TEXT`       |
| SQL         | NUMERIC to String                   | Blockchain amounts need `::TEXT` cast for API responses  |
| SQL         | FK violations                       | Insert parents before children                           |
| SQL         | CONCURRENTLY in migrations          | Don't use; SQLx runs in transactions                     |
| SQL         | N+1 queries                         | Use JOINs to batch-fetch related records                 |
| SQL         | Querying wrong table                | Verify table/column names after schema refactors         |
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

### TEST-004: sqlx::test macro requires MIGRATOR constant (a55df69)

**Symptom**: `#[sqlx::test]` macro fails with "MIGRATOR not found".

**Root Cause**: sqlx's test macro expects a `MIGRATOR` constant for automatic database setup.

```rust
// crates/api/src/lib.rs
pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("../migrations/postgres");
```

**Lesson**: sqlx integration tests with `#[sqlx::test]` require explicit migrator setup. The macro path is relative to the crate root.

**Files**: `crates/api/src/lib.rs`, `crates/api/Cargo.toml`

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

### PERF-001: Slow cell consumption due to partition scan (2026-01-24)

**Symptom**: Sync speed dropped from 2000+ to ~100 blocks/sec as database grew. `consume_cells_batch` taking 3-4 seconds.

**Root Cause**: The `cells` table is partitioned by `created_at_block` (10 partitions, 5M blocks each), but the UPDATE query used a JOIN with UNNEST which prevented PostgreSQL from pruning partitions.

```sql
-- SLOW: PostgreSQL scans ALL 10 partitions
UPDATE cells SET status = 1 ...
FROM (SELECT * FROM UNNEST(...)) AS u
WHERE cells.tx_hash = u.tx_hash
  AND cells.created_at_block = u.created_at_block  -- Can't prune: value from joined data
```

**Solution**: Two-part fix:

1. **`live_cells` table**: Non-partitioned lookup table for O(1) OutPoint resolution
   - Contains only live cells (~1.7M vs 55M total)
   - Primary key on `(tx_hash, output_index)` for fast lookups
   - Maintained in sync: INSERT on cell creation, DELETE on consumption

2. **Partition-aware UPDATE**: Group consumptions by partition and add explicit bounds
   ```sql
   -- FAST: PostgreSQL prunes to single partition
   UPDATE cells SET status = 1 ...
   WHERE cells.created_at_block >= $7  -- Partition lower bound
     AND cells.created_at_block < $8   -- Partition upper bound
   ```

**Performance Impact**:
| Metric | Before | After |
|--------|--------|-------|
| Sync speed | ~100 blocks/sec | ~290 blocks/sec |
| UPDATE cells avg | 702ms | 24ms |
| OutPoint lookup | 3.9s (scan 10 partitions) | 0.1ms (PK lookup) |

**Key Insight**: When using partitioned tables, ensure hot-path queries include partition key bounds in WHERE clause, not just in JOIN conditions.

**Files**:

- `migrations/postgres/001_init.sql` - `live_cells` table
- `crates/indexer/src/db/writer.rs` - `consume_cells_batch`, `get_cells_info_batch`
- `crates/indexer/tests/live_cells.rs` - 8 tests including cross-partition

---

### PERF-002: API live cells queries scanning partitioned table (2026-01-24)

**Symptom**: `/api/v1/cells/live` endpoint slow (~255ms) when filtering by `lock_script_hash`.

**Root Cause**: API queried `cells` table with `status = 0` filter, which scanned all 10 partitions even for simple COUNT queries.

**Solution**:

1. Added `lock_args` column to `live_cells` table (needed for API response)
2. Changed API queries to use `live_cells` instead of `cells WHERE status = 0`

```rust
// Before: Scans 10 partitions
"SELECT COUNT(*) FROM cells WHERE lock_script_hash = $1 AND status = 0"

// After: Single table scan
"SELECT COUNT(*) FROM live_cells WHERE lock_script_hash = $1"
```

**Performance Impact**:
| Query | Before | After |
|-------|--------|-------|
| COUNT by lock_script_hash | 255ms | 15ms |
| Paginated live cells | ~300ms | ~15ms |

**Files**:

- `migrations/postgres/001_init.sql` - Added `lock_args` to `live_cells`
- `crates/indexer/src/db/writer.rs` - Write `lock_args` on insert/reorg
- `crates/api/src/routes/cells.rs` - Query `live_cells` for live cell endpoints

---

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

### IDX-006: live_cells partition indexes failed to rebuild (2026-01-30)

**Symptom**: After bulk sync completed and index rebuild triggered, 5 indexes on `live_cells` table failed with error "cannot create index on partitioned table concurrently".

```
Failed indexes: idx_live_cells_lock, idx_live_cells_lock_code, idx_live_cells_type,
                idx_live_cells_type_code, idx_live_cells_block
```

**Root Cause**: Two bugs in `indexes.rs`:

1. **Wrong partition type**: `live_cells` indexes were marked as `is_partitioned: false`, but `live_cells` is a HASH-partitioned table with 16 partitions. The code tried to create indexes directly on the parent table with `CREATE INDEX CONCURRENTLY`, which PostgreSQL doesn't support for partitioned tables.

2. **Missing partition suffixes**: Even if marked as partitioned, the `PARTITION_SUFFIXES` constant only had 10 entries (`_p00` to `_p09`) for RANGE-partitioned tables, but `live_cells` has 16 HASH partitions (`_p00` to `_p15`).

```rust
// WRONG: live_cells marked as non-partitioned
DeferrableIndex {
    name: "idx_live_cells_lock",
    table: "live_cells",
    is_partitioned: false,  // BUG: Should be partitioned!
    ...
}

// WRONG: Only 10 suffixes, but live_cells has 16 partitions
const PARTITION_SUFFIXES: &[&str] = &["_p00", ..., "_p09"];
```

**Fix**: Introduced `PartitionType` enum to distinguish partition schemes:

```rust
enum PartitionType {
    None,   // Not partitioned
    Range,  // 10 partitions (_p00 to _p09) - blocks, cells, transactions, etc.
    Hash,   // 16 partitions (_p00 to _p15) - live_cells
}

const RANGE_PARTITION_SUFFIXES: &[&str] = &["_p00", ..., "_p09"];
const HASH_PARTITION_SUFFIXES: &[&str] = &["_p00", ..., "_p15"];
```

**Test Coverage Added**:

- `test_hash_partitioned_indexes` - Verifies live_cells indexes use Hash type
- `test_partition_suffix_counts` - Verifies 10 RANGE and 16 HASH suffixes
- `test_live_cells_indexes_are_hash_partitioned` - Explicit check for live_cells

**Lesson**: When a table uses a different partition scheme than others, it needs explicit handling. PostgreSQL `CREATE INDEX CONCURRENTLY` cannot be used on partitioned parent tables - indexes must be created on each partition individually.

**Files**:

- `crates/indexer/src/db/indexes.rs` - Added PartitionType enum, fixed live_cells indexes
- `migrations/postgres/001_init.sql` - Documents live_cells HASH partitioning (16 partitions)

---

### IDX-007: Task-runner dao_daily_snapshots rebuild missing cumulative fields (2026-02-01)

**Symptom**: After database rebuild and running `statistics_rebuild` task, DAO charts, Total Supply chart, and Secondary Issuance chart showed incorrect or empty data.

- Total Supply Chart: `burnt` layer only showed genesis burnt (8.4B), missing secondary burnt
- Secondary Issuance Chart: Completely empty (no data points)
- DAO Circulation Ratio: Potentially incorrect values

**Root Cause**: The task-runner's `rebuild_dao_daily_snapshots` function in `crates/task-runner/src/executor/statistics.rs` was implemented incorrectly compared to the indexer's version in `crates/indexer/src/db/writer/statistics.rs`:

1. **Missing cumulative fields**: Did not query `block_secondary_issuance` table, leaving `cumulative_burnt`, `cumulative_mining_reward`, `cumulative_deposit_compensation` as NULL
2. **Wrong field names**: Used `total_deposited`, `active_deposits`, `unique_depositors` instead of schema fields `total_deposit`, `depositors_count`
3. **Missing `dao_data`**: Did not store the raw DAO bytes
4. **Wrong deposit query**: Used `withdraw_request_tx IS NULL` instead of timestamp-based logic

```rust
// WRONG: Task-runner implementation was completely different from indexer
// - Did not query block_secondary_issuance for cumulative values
// - Used wrong column names
// - Used block-number based deposit logic instead of timestamp-based

// CORRECT: Must match indexer's update_dao_daily_snapshot() exactly
let secondary_issuance = sqlx::query_as::<_, (String, String, String)>(
    r#"
    SELECT
        COALESCE(SUM(burnt), 0)::text,
        COALESCE(SUM(miner_secondary), 0)::text,
        COALESCE(SUM(dao_compensation), 0)::text
    FROM block_secondary_issuance
    WHERE block_timestamp::date <= $1
    "#,
)
.bind(date)
.fetch_one(pool)
.await?;
```

**Impact**: Charts relying on `dao_daily_snapshots` displayed incorrect data:

| Chart              | API Query              | Effect of NULL cumulative fields            |
| ------------------ | ---------------------- | ------------------------------------------- |
| Total Supply       | `cumulative_burnt`     | Only genesis 8.4B shown as burnt            |
| Secondary Issuance | `cumulative_*` columns | WHERE clause filters all rows → empty chart |

**Lesson**: When two codepaths (indexer sync vs task-runner rebuild) populate the same table, they MUST produce identical output. Add integration tests that verify the rebuild produces the same schema/values as incremental updates.

**Test Coverage Added**: `crates/task-runner/tests/dao_daily_snapshot.rs` - 5 tests verifying cumulative fields, deposit totals, and DAO field parsing.

**Files**:

- `crates/task-runner/src/executor/statistics.rs` - Fixed `update_dao_daily_snapshot()`
- `crates/task-runner/src/lib.rs` - Added lib.rs to export modules for testing
- `crates/task-runner/tests/dao_daily_snapshot.rs` - New integration tests

---

### PERF-003: UNIQUE constraints not dropped on partition tables (2026-02-05)

**Symptom**: Bulk sync performance degraded ~37% (5,700 → 3,600 blocks/sec). Logs showed warnings:

```
WARN Failed to drop constraint cells_p00_created_at_block_tx_hash_output_index_key:
  cannot drop inherited constraint "cells_p00_created_at_block_tx_hash_output_index_key"
```

**Root Cause**: `drop_deferrable_constraints()` tried to drop constraints on partition tables (e.g., `cells_p00`), but PostgreSQL partition constraints are inherited from the parent table and cannot be dropped directly on children.

```rust
// WRONG: Drop on partition tables
for suffix in RANGE_PARTITION_SUFFIXES {
    let table_name = format!("{}{}", constraint.table, suffix); // cells_p00
    drop_constraint_if_exists(&table_name, &constraint_name)    // Fails!
}

// CORRECT: Drop on parent table
let constraint_name = format!("{}_{}", constraint.table, constraint.name);
drop_constraint_if_exists(constraint.table, &constraint_name)  // cells
// PostgreSQL cascades to all partitions automatically
```

**Key Insight**: PostgreSQL partition inheritance is asymmetric:

- **DROP**: Must target parent table (cascades to children)
- **ADD**: Can target individual partitions (task-runner rebuild does this correctly)

**Performance Impact**:

| Metric        | Before (bug)  | After (fix) |
| ------------- | ------------- | ----------- |
| DB write time | 3-6s/10K      | ~1.5s/10K   |
| Sync rate     | 3,600/sec     | 6,000+/sec  |
| Slow INSERTs  | 2.3s (2 rows) | <100ms      |

**Test Coverage Added**: `test_constraint_drop_uses_parent_table_name` in `crates/indexer/src/db/indexes.rs`

**Files**: `crates/indexer/src/db/indexes.rs` - `drop_deferrable_constraints()`

---

_Last updated: 2026-02-05_
