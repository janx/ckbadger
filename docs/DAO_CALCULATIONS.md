# DAO Calculations Specification

This document describes the calculation logic for Nervos DAO statistics displayed on the `/dao` page and DAO charts.

## References

- [RFC-0023: Deposit and Withdraw in Nervos DAO](./rfcs/rfcs/0023-dao-deposit-withdraw/0023-dao-deposit-withdraw.md)
- [RFC-0015: CKB Cryptoeconomics](./rfcs/rfcs/0015-ckb-cryptoeconomics/0015-ckb-cryptoeconomics.md)
- [Understanding the Nervos CKB Issuance Model](https://www.nervos.org/knowledge-base/understanding_nervos_ckb_issuance_model)

## CKB Supply Model

### Genesis Block Issuance

The genesis block issued **33.6 billion CKB**, but **8.4 billion (25%) was immediately burnt** and never entered circulation:

| Category                   | Amount    | Notes                                            |
| -------------------------- | --------- | ------------------------------------------------ |
| **Total Issued**           | 33.6B CKB | Recorded in dao field `total_issuance`           |
| **Genesis Burnt**          | 8.4B CKB  | Never circulated, but affects secondary issuance |
| **Circulating at Genesis** | 25.2B CKB | Actual tokens in circulation                     |

**Important**: The 8.4B burnt CKB is "issued but not circulating". It impacts secondary issuance distribution:

- 5.04B (60% of 8.4B) is hard-coded as "occupied" capacity → miners receive secondary issuance
- 3.36B (40% of 8.4B) is hard-coded as "liquid" → treasury receives secondary issuance (also burnt)

This ensures miners and treasury always receive a minimum portion of secondary issuance even if all circulating CKB were locked in DAO.

### Genesis Special Burn Cell

The 8.4B burnt CKB exists in a single cell in the genesis block:

| Field        | Value                                                                |
| ------------ | -------------------------------------------------------------------- |
| Transaction  | `0xe2fb199810d49a4d8beec56718ba2593b665db9d52299a0f9e6e75416d73ff5c` |
| Output Index | 6                                                                    |
| Capacity     | 8,400,000,000 CKB                                                    |
| Lock Args    | `0x62e907b15cbf27d5425399ebf6f0fb50ebb88f18` (Satoshi's pubkey hash) |
| Data Size    | 0 bytes                                                              |

The lock args is the pubkey hash from Bitcoin's genesis block coinbase, making this cell permanently unspendable. In the explorer, this cell is identified by:

1. Lock args matching Satoshi's pubkey hash
2. Created at block 0 (genesis)

The API returns `cellType: "genesis_special_burn"` and `virtualOccupiedCapacity: "504000000000000000"` (5.04B in shannons) for this cell.

### Supply Terminology

RFC-0023 calls DAO field `C` total issuance, but field `S` is the portion of
secondary issuance that is still unissued. Therefore user-facing circulation
must subtract the complete `S` pool, not only its treasury portion.

| Term                          | Definition                                             | At Genesis |
| ----------------------------- | ------------------------------------------------------ | ---------- |
| `total_issuance`              | DAO field `C` (genesis + primary + secondary schedule) | 33.6B      |
| `unissued_secondary`          | DAO field `S` (unmade DAO interest + treasury)         | ~0         |
| `secondary_treasury`          | `S - unmade_dao_interests`                             | ~0         |
| `protocol_circulating`        | `C - genesis_burnt - S`                                | ~25.2B     |
| `liquid`                      | `protocol_circulating - locked_in_dao`                 | varies     |
| `explorer_policy_locked`      | Explorer-labelled vesting and Bug Bounty balances      | varies     |
| `explorer_circulating_supply` | `protocol_circulating - explorer_policy_locked`        | varies     |

ckbadger exposes `protocol_circulating`, derived only from consensus DAO fields
and the chain-derived genesis burn. The official explorer applies an additional
off-chain policy classification: historical vesting allocations plus the live
balance of its labelled Bug Bounty address. Its separately published
`locked_capacity` is the exact amount needed to normalize that external metric:

```
protocol_circulating =
    explorer_circulating_supply + explorer_locked_capacity
```

### Issuance Schedule

| Type                   | Rate                                      | Hard Cap    |
| ---------------------- | ----------------------------------------- | ----------- |
| **Primary Issuance**   | 4.2B/year initially, halves every 4 years | 33.6B total |
| **Secondary Issuance** | 1.344B/year (constant)                    | No cap      |

## DAO Field Structure

Each block header contains a 32-byte `dao` field with 4 little-endian u64 values:

| Bytes | Field                 | Description                                                       |
| ----- | --------------------- | ----------------------------------------------------------------- |
| 0-7   | C (total_issuance)    | Cumulative issuance (genesis + primary + secondary) in shannons   |
| 8-15  | AR (accumulated_rate) | Accumulated Rate for compensation calculation (scaled by 10^16)   |
| 16-23 | S (secondary_pool)    | Total unissued secondary issuance pool (DAO + treasury unclaimed) |
| 24-31 | U (occupied_capacity) | Total occupied capacity (cell storage costs) in shannons          |

```rust
pub struct DaoField {
    pub total_issuance: u64,      // C - includes genesis burnt (33.6B at genesis)
    pub accumulated_rate: u64,    // AR - starts at 10^16
    pub secondary_pool: u64,      // S (unissued secondary pool)
    pub occupied_capacity: u64,   // U
}
```

### S Field Dynamics (RFC-0023)

`S` is a `u64` pool value and remains non-negative, but block-to-block delta `S_i - S_{i-1}` can be negative.

Per RFC-0023:

```
S_i = S_{i-1} - I_i + s_i - floor(s_i * U_{i-1} / C_{i-1})
```

Where:

- `s_i`: secondary issuance in block `i`
- `I_i`: total compensation of completed DAO withdrawals in block `i` (phase-2 completions)

So if `I_i` exceeds the block's net inflow to `S`, the delta is negative even though `S_i` itself is still a valid non-negative `u64`.

## Constants

```rust
const SHANNON: u64 = 100_000_000;                    // 1 CKB = 10^8 shannons
const DAO_OCCUPIED_CAPACITY: u64 = 102 * SHANNON;   // standard secp256k1 DAO cell
const BLOCKS_PER_YEAR: i64 = 3_942_000;             // 365.25 * 24 * 60 * 60 / 8 seconds
const GENESIS_BURNT: u64 = 8_400_000_000 * SHANNON; // 8.4B CKB burnt at genesis
const SECONDARY_ISSUANCE_PER_YEAR: u64 = 1_344_000_000 * SHANNON; // 1.344B CKB/year
```

## 1. Individual Deposit Compensation

For a single DAO deposit, compensation is calculated using AR growth:

```
free_capacity = capacity - occupied_capacity
compensation = free_capacity * (AR_withdraw / AR_deposit) - free_capacity
             = free_capacity * (AR_withdraw - AR_deposit) / AR_deposit
```

Where:

- `capacity`: Total CKB locked in the deposit (in shannons)
- `occupied_capacity`: Exact occupied capacity of that deposit cell, derived
  from its capacity field, lock script, type script, and data
- `AR_deposit`: AR value at deposit block
- `AR_withdraw`: AR value at the phase-1 withdraw-request block

The familiar 102 CKB value applies to a standard secp256k1 DAO cell. It is not
a protocol-wide constant for cells using different lock scripts. The indexer
therefore persists the original deposit cell's exact occupied capacity.

### Lifecycle aggregation

At observation block `B`, each deposit contributes through exactly one state:

| State at `B`             | Compensation AR                       | Aggregate bucket |
| ------------------------ | ------------------------------------- | ---------------- |
| Active deposit           | AR at block `B`                       | `active_unmade`  |
| Withdraw request pending | AR at request block                   | frozen unclaimed |
| Withdraw completed       | request AR (stored amount must match) | claimed          |

```
unclaimed_compensation = active_unmade + frozen_phase1_compensation
deposit_compensation = claimed_compensation + unclaimed_compensation
```

Phase-1 cells remain protocol-locked, but their compensation stops accruing at
the request AR. Phase-2 moves the same amount from unclaimed to claimed; it
does not change cumulative deposit compensation. Completed entries retain the
request AR so rollback to phase-1 can reproduce and validate the frozen amount.

**Implementation**:

- Arithmetic: `crates/common/src/dao.rs::calculate_dao_compensation_from_ar()`
- Historical lifecycle: `crates/ckbadger-store/src/dao_ops.rs::dao_compensation_for_entry_at()`
- Bulk daily timeline: `crates/indexer/src/sync/bulk_build/owners/dao.rs`

## 2. Estimated APC (Annual Percentage Compensation)

APC represents the annualized return rate for DAO deposits at the current block height.

### Formula (CKB Explorer-compatible model)

Uses continuous compounding with halving-aware alpha factor:

```
alpha = primary_issuance_per_epoch / secondary_issuance_per_epoch
sn = secondary_issuance over 1-year deposit window
C = theoretical cumulative total_issuance at deposit epoch

rate = ln(1 + (alpha + 1) * sn / C) / (alpha + 1)
APC = rate * 100  (truncated to 4 decimal places)
```

Where:

- `alpha` depends on the halving period (primary/secondary ratio per epoch)
- `sn` = secondary issuance over 2190 epochs (1 year)
- `C` = genesis issuance + cumulative primary + cumulative secondary
- If the deposit window spans a halving boundary, rates are compounded per segment

### Why This Model

This matches the CKB Explorer's `estimated_apc` calculation. The model accounts for:

1. **Primary issuance dilution** via the alpha factor (total issuance grows faster than secondary alone)
2. **Halving schedule** (primary issuance halves every 4 years, reducing alpha)
3. **Continuous compounding** (more accurate than simple division)

### APC Over Time

| Period               | alpha  | APC   |
| -------------------- | ------ | ----- |
| Genesis (Year 0)     | 3.125  | ~3.7% |
| Year 4 (1st halving) | 1.5625 | ~2.7% |
| Year 8 (2nd halving) | 0.7813 | ~2.0% |
| Long term            | → 0    | → 0%  |

APC decreases over time because cumulative `total_issuance` grows while secondary issuance is constant.

**Update frequency**: Calculated from tip block epoch info on API read path.

**Implementation**:

- Core calculation: `crates/common/src/dao.rs::calculate_estimated_apc()`
- API wrapper: `crates/api/src/routes/dao.rs::estimated_apc_from_store()`

## 3. Secondary Issuance Components

CKB's secondary issuance is reflected through DAO header fields and individual
deposit AR growth. These are related protocol quantities, but they are not
interchangeable calculation paths.

### 3.1 Miner secondary issuance

For block `N`, using header state at the end of block `N-1`:

```
non_miner_delta =
    (S_N - S_(N-1)) + claimed_compensation_in_block_N

miner_secondary_delta =
    non_miner_delta * U_(N-1) / (C_(N-1) - U_(N-1))
```

If `non_miner_delta` is a negative protocol correction, miner reward does not
decrease. Cumulative miner secondary issuance is the exact sum of the
non-negative per-block miner deltas.

### 3.2 DAO compensation

DAO compensation is **not** reconstructed by multiplying the aggregate
secondary-pool delta by aggregate DAO principal. That proportional shortcut is
not equivalent to the protocol because deposits have different AR start
points, per-deposit integer flooring, exact occupied capacities, and phase-1
freeze times.

The only DAO compensation path is the lifecycle calculation in section 1:

```
deposit_compensation = sum(exact per-deposit claimed or unclaimed compensation)
```

Bulk materialization advances deposit, request, and completion events in block
order. For each daily boundary it evaluates only the deposits still accruing
at that day's AR, while frozen and claimed totals advance through lifecycle
events.

### 3.3 Treasury

The explorer-compatible treasury series is derived directly from the on-chain
secondary pool and active unmade interest:

```
treasury = S - active_unmade
```

This formula deliberately uses only still-accruing status-0 compensation.
Phase-1 compensation is frozen and remains in `unclaimed_compensation`, but is
not part of `active_unmade`.

### 3.4 Data sources and implementation

- `C`, `S`, `U`, and `AR`: exact DAO fields from every block header
- `claimed_compensation_in_block`: exact phase-2 lifecycle transition
- Per-deposit capacity, exact occupied capacity, deposit AR, request AR, and
  lifecycle block numbers: domain-store `dao_deposits`
- Miner arithmetic:
  `crates/common/src/dao.rs::calculate_secondary_miner_delta()`
- Per-block miner accumulation:
  `crates/indexer/src/sync/dao_helpers.rs`
- Exact bulk compensation timeline:
  `crates/indexer/src/sync/bulk_build/owners/dao.rs`

## 4. Storage Schema (RocksDB)

DAO-related state is split across several CFs:

| CF / Data                     | Key                                                     | Value                  | Purpose                                                                                                                                          |
| ----------------------------- | ------------------------------------------------------- | ---------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------ |
| `dao_deposits`                | `tx_hash + output_index`                                | `DaoDepositCacheEntry` | Deposit lifecycle plus original capacity, exact occupied capacity, deposit/request ARs, and validated claimed compensation                       |
| `dao_by_withdraw_tx`          | `withdraw_outpoint` (34B)                               | `deposit_outpoint_key` | Fast lookup on withdraw completion (keyed by withdraw request outpoint)                                                                          |
| `dao_by_block`                | `block_desc (8B BE) + outpoint (34B)`                   | empty                  | Newest-first global DAO deposit index                                                                                                            |
| `dao_by_lock_block`           | `lock_hash (32B) + block_desc (8B BE) + outpoint (34B)` | empty                  | Newest-first DAO deposit index scoped by lock hash                                                                                               |
| `dao_by_status_block`         | `status (2B BE) + block_desc (8B BE) + outpoint (34B)`  | empty                  | Newest-first DAO deposit index scoped by status                                                                                                  |
| `stats` (DAO snapshot prefix) | date                                                    | `DaoDailySnapshot`     | Daily cumulative series (`total_issuance`, `secondary_pool`, `occupied_capacity`, `cum_miner_secondary`, `cum_dao_compensation`, `cum_treasury`) |

## 5. Update Triggers

| Statistic                       | Trigger                            | Function / owner                                                                      |
| ------------------------------- | ---------------------------------- | ------------------------------------------------------------------------------------- |
| Miner secondary issuance        | Every processed block              | `accumulate_secondary_issuance_deltas_from_csu()`                                     |
| Bulk daily deposit compensation | Bulk final materialization         | `DaoCompensationTimeline`                                                             |
| Latest deposit compensation     | After each live domain-store batch | `BatchWriter::refresh_latest_dao_statistics()`                                        |
| Reorg cutoff-date repair        | After partial-day rollback         | `CkbadgerStore::recompute_dao_daily_snapshot_for_date()` + `repair_cutoff_date_stats` |

> **Note:** Estimated APC served by DAO APIs is derived from the latest `DaoDailySnapshot` + protocol constants, not from a periodically persisted `estimated_apc` field.

## 6. Charts Data

### Total Supply Chart

Shows stacked area of CKB distribution:

```
total_issuance = DAO field C
unissued_secondary = DAO field S
secondary_treasury = S - unmade_dao_interests
total_burnt = GENESIS_BURNT + secondary_treasury
protocol_circulating = total_issuance - GENESIS_BURNT - unissued_secondary
liquid = protocol_circulating - locked_in_dao
```

| Layer                | Calculation                            | At Genesis |
| -------------------- | -------------------------------------- | ---------- |
| Circulating (liquid) | `protocol_circulating - locked_in_dao` | ~25.2B     |
| Locked in DAO        | active DAO deposit principal           | ~0         |
| Burnt                | `GENESIS_BURNT + secondary_treasury`   | 8.4B       |

Outstanding `unmade_dao_interests` are not circulating yet and are not
treasury/burnt. Consequently the three displayed layers sum to
`C - unmade_dao_interests`.

**Implementation**: `crates/api/src/routes/statistics.rs::get_total_supply_chart()`

### Total Deposit Chart

- Source: `dao_daily_snapshots.total_deposit`
- Shows cumulative CKB locked in DAO over time

### Circulation Ratio Chart

Shows percentage of **circulating** CKB locked in DAO:

```
protocol_circulating = C - GENESIS_BURNT - S
ratio = dao_deposits / protocol_circulating * 100
```

**Note**: Uses realized circulating supply, excluding both the genesis burn and
the complete unissued secondary pool.

**Implementation**: `crates/api/src/routes/dao.rs::get_circulation_ratio_chart()`

### Secondary Issuance Distribution Chart

Shows percentage allocation of secondary issuance:

```
mining_pct = occupied_capacity / total_issuance * 100
compensation_pct = dao_deposits / total_issuance * 100
burnt_pct = liquid / total_issuance * 100
```

**Note**: Uses `total_issuance` (including genesis burnt) because the 8.4B burnt affects the distribution formula at protocol level.

**Implementation**: `crates/api/src/routes/statistics.rs::get_secondary_issuance_chart()`

### Nominal APC Chart

Shows theoretical APC over time (0-20 years):

```
total_supply = GENESIS_CIRCULATING + primary_issued + secondary_issued
APC = (SECONDARY_ISSUANCE_PER_YEAR / total_supply) * 100
```

Where `GENESIS_CIRCULATING = 25.2B` (not 33.6B).

**Implementation**: `crates/api/src/routes/statistics.rs::calculate_nominal_apc()`

## 7. Common Pitfalls

### Confusing total_issuance with circulating

**Wrong**: Using `total_issuance` (33.6B at genesis) as circulating supply

**Wrong**: Subtracting only treasury/burnt secondary issuance. That leaves
unmade DAO interest in circulation before it has been claimed.

**Correct**: `protocol_circulating = C - genesis_burnt - S` (~25.2B at genesis)

DAO field `C` includes the 8.4B genesis burn and the scheduled secondary
issuance accumulated in `S`. RFC-0023 defines `S` as unissued, so both must be
removed from user-facing circulating supply.

### Comparing protocol circulation directly with explorer circulation

**Wrong**: Comparing ckbadger `C - genesis_burnt - S` directly with the
official explorer's `circulating_supply`, then widening the tolerance to hide a
stable offset.

**Correct**: Compare with
`explorer circulating_supply + explorer locked_capacity`. The explorer
subtracts policy-labelled balances that are not excluded by CKB consensus; its
`locked_capacity` series publishes that exact adjustment.

### Using 33.6B as Genesis Supply for APC

**Wrong**: `GENESIS_SUPPLY = 33.6B` in nominal APC calculation

**Correct**: `GENESIS_SUPPLY = 25.2B` (actual circulating at genesis)

The 8.4B burnt never enters circulation and shouldn't be counted when calculating expected returns.

### Incorrect APC Formula

**Wrong**: `APC = secondary_issuance_per_year / circulating_supply * 100` (simple division)

Ignores primary issuance dilution (alpha factor) and doesn't match the CKB Explorer.

**Wrong**: `APC = secondary_issuance_per_year / total_issuance * 100`

Closer but still doesn't account for the alpha factor or continuous compounding.

**Correct**: Use the continuous compounding model with `rate = ln(1 + (alpha+1) * sn / C) / (alpha+1)`. See formula above.

### Secondary Issuance Attribution

**Wrong**: Assuming most goes to miners

- Mining reward is proportional to occupied capacity (~10% of total issuance)

**Correct**: Most is burnt (~70%) because most CKB is liquid

- Only CKB locked in DAO earns compensation
- Free-floating CKB's share is burnt

### When to Use total_issuance vs circulating

| Use Case                               | Use `total_issuance` | Use `circulating` |
| -------------------------------------- | -------------------- | ----------------- |
| Secondary issuance % distribution      | ✓                    |                   |
| APC calculation                        |                      | ✓                 |
| User-facing "Total Supply"             |                      | ✓                 |
| Circulation ratio                      |                      | ✓                 |
| Total Supply Chart (circulating layer) |                      | ✓                 |

### DAO Compensation vs Secondary Allocation

These are different concepts:

- **DAO Compensation**: What depositors receive based on AR growth (individual deposits)
- **Secondary Allocation to DAO**: Portion of secondary issuance allocated to DAO pool (affects AR growth)

## 8. Common Knowledge Size

**Common Knowledge** is a core CKB concept referring to state verified by global consensus and accepted by all network participants. The set of all live cells represents the current common knowledge on CKB.

### Formula

```
knowledge_size = U - BURN_ADJUSTMENT
```

Where:

- `U` = DAO field bytes 24-31 (occupied capacity in shannons)
- `BURN_ADJUSTMENT` = 504,000,000,000,000,000 shannons (5.04B CKB)

### Why the Burn Adjustment?

The 8.4B CKB burnt at genesis is "issued but not circulating". Of this:

- **5.04B (60%)** is hard-coded as "occupied" capacity
- **3.36B (40%)** is hard-coded as "liquid"

The `U` field in the DAO includes this virtual 5.04B occupied capacity. Since the burn cell doesn't actually store any data, we subtract it to get the real Common Knowledge Size.

### Constants

```rust
const GENESIS_BURNT: u64 = 8_400_000_000 * SHANNON;     // 8.4B CKB
const BURN_OCCUPIED_RATIO: f64 = 0.6;                    // 60%
const BURN_ADJUSTMENT: i128 = 504_000_000_000_000_000;  // 5.04B CKB in shannons
// Derivation: 8_400_000_000 * 100_000_000 * 0.6 = 504_000_000_000_000_000
```

### What Occupied Capacity Includes

A cell's occupied capacity is NOT just `cell.data.len()`. It includes ALL storage requirements:

| Component      | Size                                  |
| -------------- | ------------------------------------- |
| Capacity field | 8 bytes                               |
| Lock script    | 32 (code_hash) + 1 (hash_type) + args |
| Type script    | 32 (code_hash) + 1 (hash_type) + args |
| Data           | Actual data bytes                     |

**IMPORTANT**: Do NOT confuse:

- `cell.data.len()` = Only the data field bytes
- `occupied_capacity` = Full storage cost (capacity + scripts + data)
- `U` field = Protocol-level cumulative occupied capacity

### Implementation

**Indexer** (`crates/indexer/src/db/writer/statistics.rs`):

```rust
pub fn calculate_knowledge_size(dao_field: &[u8]) -> Option<i128> {
    if dao_field.len() < 32 { return None; }
    let u_field = u64::from_le_bytes(dao_field[24..32].try_into().ok()?);
    Some(u_field as i128 - BURN_ADJUSTMENT)
}
```

**API** (`crates/api/src/routes/statistics.rs`):

- Endpoint: `GET /api/v1/charts/knowledge-size`
- Returns daily values in shannons as strings (for precision)
- Frontend converts to CKB for display

### Data Flow

1. Each block's DAO field is stored during indexing
2. `update_daily_statistics()` extracts U field from the last block of each day
3. Calculates `knowledge_size = U - 504000000000000000`
4. Stores in `DailyStats.knowledge_size` in the `stats` column family
5. API serves historical chart data

### Reference

The formula matches the official CKB Explorer implementation:

```ruby
# Official explorer (Ruby)
knowledge_size = dao.U - (BURN_QUOTA * 0.6)
```
