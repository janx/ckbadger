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

| Term             | Definition                                       | At Genesis |
| ---------------- | ------------------------------------------------ | ---------- |
| `total_issuance` | dao field C - all issued CKB including burnt     | 33.6B      |
| `circulating`    | total_issuance - genesis_burnt - secondary_burnt | 25.2B      |
| `liquid`         | circulating - locked_in_dao                      | varies     |

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
const DAO_OCCUPIED_CAPACITY: u64 = 102 * SHANNON;   // 102 CKB minimum for DAO cell
const BLOCKS_PER_YEAR: i64 = 3_942_000;             // 365.25 * 24 * 60 * 60 / 8 seconds
const GENESIS_BURNT: u64 = 8_400_000_000 * SHANNON; // 8.4B CKB burnt at genesis
const SECONDARY_ISSUANCE_PER_YEAR: u64 = 1_344_000_000 * SHANNON; // 1.344B CKB/year
```

## 1. Individual Deposit Compensation

For a single DAO deposit, compensation is calculated using AR growth:

```
free_capacity = capacity - DAO_OCCUPIED_CAPACITY
compensation = free_capacity * (AR_withdraw / AR_deposit) - free_capacity
             = free_capacity * (AR_withdraw - AR_deposit) / AR_deposit
```

Where:

- `capacity`: Total CKB locked in the deposit (in shannons)
- `AR_deposit`: AR value at deposit block
- `AR_withdraw`: AR value at withdrawal block

**Implementation**: `crates/api/src/routes/dao.rs::calculate_compensation()`

## 2. Estimated APC (Annual Percentage Compensation)

APC represents the annualized return rate for DAO deposits at the current block height.

### Formula

```
APC = (secondary_issuance_per_year / circulating_supply) * 100
```

Where:

- `secondary_issuance_per_year` = 1.344B CKB (protocol constant)
- `circulating_supply` = `total_issuance` - `genesis_burnt` - `secondary_burnt`
- `total_issuance` = dao field bytes 0-7 (C)
- `genesis_burnt` = 8.4B CKB (never entered circulation)
- `secondary_burnt` = cumulative burnt from secondary issuance

### Why Use Circulating Supply (Not Total Issuance)

The 8.4B CKB burnt at genesis was issued but never entered circulation. Only the circulating supply (25.2B at genesis) actually participates in the economy and competes for secondary issuance. Using `total_issuance` (33.6B) would underestimate the true return rate.

### APC Over Time

| Period               | circulating | APC   |
| -------------------- | ----------- | ----- |
| Genesis (Year 0)     | ~25.2B      | ~5.3% |
| Year 4 (1st halving) | ~41B        | ~3.3% |
| Year 8 (2nd halving) | ~53B        | ~2.5% |
| Long term            | → ∞         | → 0%  |

APC decreases over time because:

- `secondary_issuance` is constant (1.344B/year)
- `circulating_supply` grows (primary + secondary issuance - burnt)

**Update frequency**: Calculated from latest snapshot on API read path.

**Implementation**:

- API calculation: `crates/api/src/routes/dao.rs::snapshot_estimated_apc()`
- Writer hook (currently no-op): `crates/indexer/src/db/writer/dao.rs::recalculate_dao_extended_statistics()`

## 3. Secondary Issuance Distribution

CKB's secondary issuance (~1.344 billion CKB/year) is distributed among three recipients:

### 3.1 Distribution Formula

For each block's secondary issuance:

```
1. miner_secondary = from RPC get_block_economic_state().miner_reward.secondary
2. non_miner_secondary = secondary_issuance - miner_secondary
3. free_capacity = total_issuance - occupied_capacity - dao_deposits
4. dao_compensation = non_miner_secondary * dao_deposits / (dao_deposits + free_capacity)
5. burnt = non_miner_secondary - dao_compensation
```

### 3.2 Distribution Ratios

The distribution depends on how CKB is allocated across the network:

| Recipient            | Formula                                              | Typical % |
| -------------------- | ---------------------------------------------------- | --------- |
| **Mining Reward**    | `occupied_capacity / total_issuance`                 | ~10-12%   |
| **DAO Compensation** | `non_miner * dao_deposits / (dao_deposits + liquid)` | ~15-20%   |
| **Burnt**            | `non_miner * liquid / (dao_deposits + liquid)`       | ~70-75%   |

Where:

- `liquid = total_issuance - occupied_capacity - dao_deposits` (CKB not locked in DAO or used for storage)

**Key insight**: Most secondary issuance is burnt because most CKB remains liquid (not locked in DAO).

### 3.3 Data Sources

- `secondary_issuance`: RPC `get_block_economic_state().issuance.secondary`
- `miner_secondary`: RPC `get_block_economic_state().miner_reward.secondary`
- `total_issuance`: DAO field bytes 0-7 (C)
- `occupied_capacity`: DAO field bytes 24-31 (U)
- `dao_deposits`: Sum of active deposits **at that block** (not current deposits!)

**CRITICAL**: When calculating historical secondary issuance breakdown, `dao_deposits` must be queried for that specific block number. Using current deposits will produce incorrect historical values. The indexer maintains point-in-time deposit tracking in RocksDB's `dao_deposits` column family.

**Update frequency**: Per processed block when close to tip; skipped while secondary-issuance bulk mode is active (`blocks_remaining > 1000`).

**Implementation**: `crates/indexer/src/sync/indexer.rs::update_secondary_issuance()`

## 4. Storage Schema (RocksDB)

DAO-related state is split across several CFs:

| CF / Data                     | Key                                                     | Value                  | Purpose                                                                                                                                          |
| ----------------------------- | ------------------------------------------------------- | ---------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------ |
| `dao_deposits`                | `tx_hash + output_index`                                | `DaoDepositCacheEntry` | Deposit lifecycle (active / withdraw requested / completed)                                                                                      |
| `dao_by_withdraw_tx`          | `withdraw_outpoint` (34B)                               | `deposit_outpoint_key` | Fast lookup on withdraw completion (keyed by withdraw request outpoint)                                                                          |
| `dao_by_block`                | `block_desc (8B BE) + outpoint (34B)`                   | empty                  | Newest-first global DAO deposit index                                                                                                            |
| `dao_by_lock_block`           | `lock_hash (32B) + block_desc (8B BE) + outpoint (34B)` | empty                  | Newest-first DAO deposit index scoped by lock hash                                                                                               |
| `dao_by_status_block`         | `status (2B BE) + block_desc (8B BE) + outpoint (34B)`  | empty                  | Newest-first DAO deposit index scoped by status                                                                                                  |
| `block_issuance`              | block number                                            | `SecondaryIssuance`    | Per-block secondary issuance split (miner / dao / treasury)                                                                                      |
| `stats` (DAO snapshot prefix) | date                                                    | `DaoDailySnapshot`     | Daily cumulative series (`total_issuance`, `secondary_pool`, `occupied_capacity`, `cum_miner_secondary`, `cum_dao_compensation`, `cum_treasury`) |

## 5. Update Triggers

| Statistic                    | Trigger                                                  | Function                                                  |
| ---------------------------- | -------------------------------------------------------- | --------------------------------------------------------- |
| Secondary issuance breakdown | For each processed block when `blocks_remaining <= 1000` | `update_secondary_issuance()`                             |
| DAO extended statistics hook | When crossing 1000-block boundaries in non-bulk mode     | `recalculate_dao_extended_statistics()` (currently no-op) |
| Daily snapshots              | Daily                                                    | `update_dao_daily_snapshot()`                             |

> **Note:** Estimated APC served by DAO APIs is derived from the latest `DaoDailySnapshot` + protocol constants, not from a periodically persisted `estimated_apc` field.

## 6. Charts Data

### Total Supply Chart

Shows stacked area of CKB distribution:

```
total_issuance = dao field C (includes genesis burnt)
total_burnt = GENESIS_BURNT + secondary_burnt
circulating = total_issuance - total_burnt
liquid = circulating - locked_in_dao
```

| Layer                | Calculation              | At Genesis |
| -------------------- | ------------------------ | ---------- |
| Circulating (liquid) | `circulating - locked`   | ~25.2B     |
| Locked in DAO        | `dao_deposits`           | ~0         |
| Burnt                | `8.4B + secondary_burnt` | 8.4B       |
| **Total**            | Sum of above             | 33.6B      |

**Implementation**: `crates/api/src/routes/statistics.rs::get_total_supply_chart()`

### Total Deposit Chart

- Source: `dao_daily_snapshots.total_deposit`
- Shows cumulative CKB locked in DAO over time

### Circulation Ratio Chart

Shows percentage of **circulating** CKB locked in DAO:

```
circulating = total_issuance - GENESIS_BURNT - secondary_burnt
ratio = dao_deposits / circulating * 100
```

**Note**: Uses real circulating supply (excluding burnt), not `total_issuance`.

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

**Correct**: `circulating = total_issuance - genesis_burnt - secondary_burnt` (25.2B at genesis)

The dao field's `total_issuance` (C) includes the 8.4B genesis burnt. For user-facing "circulating supply", subtract all burnt CKB.

### Using 33.6B as Genesis Supply for APC

**Wrong**: `GENESIS_SUPPLY = 33.6B` in nominal APC calculation

**Correct**: `GENESIS_SUPPLY = 25.2B` (actual circulating at genesis)

The 8.4B burnt never enters circulation and shouldn't be counted when calculating expected returns.

### Incorrect APC Formula

**Wrong**: `APC = (AR_current / AR_past)^(1/years) - 1` (comparing two blocks)

This requires historical data and is complex when sync is incomplete.

**Wrong**: `APC = secondary_issuance_per_year / total_issuance * 100`

Using `total_issuance` includes the 8.4B genesis burnt which never circulates.

**Correct**: `APC = secondary_issuance_per_year / circulating_supply * 100`

This only needs current block data and gives the instantaneous APC rate based on actual circulating supply.

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
