# DAO Calculations Specification

This document describes the calculation logic for Nervos DAO statistics displayed on the `/dao` page and DAO charts.

## References

- [RFC-0023: Deposit and Withdraw in Nervos DAO](https://github.com/nervosnetwork/rfcs/blob/master/rfcs/0023-dao-deposit-withdraw/0023-dao-deposit-withdraw.md)
- [RFC-0015: CKB Cryptoeconomics](https://github.com/nervosnetwork/rfcs/blob/master/rfcs/0015-ckb-cryptoeconomics/0015-ckb-cryptoeconomics.md)
- [Understanding the Nervos CKB Issuance Model](https://www.nervos.org/knowledge-base/understanding_nervos_ckb_issuance_model)

## CKB Supply Model

### Genesis Block Issuance

Mainnet's familiar rounded genesis figures are **33.6 billion CKB issued**, with **8.4 billion
(25%) immediately burnt** and never entering circulation:

| Category                   | Amount     | Notes                                           |
| -------------------------- | ---------- | ----------------------------------------------- |
| **Total Issued**           | ≈33.6B CKB | Exact value is block-0 DAO field `C`            |
| **Genesis Burnt**          | 8.4B CKB   | Derived from matching block-0 burn-policy cells |
| **Circulating at Genesis** | ≈25.2B CKB | `baseline.total_issuance - baseline.burnt`      |

These numbers are examples, not calculation constants. At indexer startup, ckbadger derives and
persists one per-network `GenesisBaseline`:

```rust
GenesisBaseline {
    total_issuance,  // exact block-0 DAO C field
    burnt,           // sum of block-0 cells matching the network burn policy
    virtual_occupied // burnt × the policy's exact occupied ratio
}
```

Mainnet and testnet currently declare the Satoshi burn-cell policy with a 6/10 occupied ratio;
unknown networks have no burn adjustment until a policy is declared. The amount is always
derived from chain cells rather than copied from the rounded mainnet example.

This ensures miners and treasury always receive a minimum portion of secondary issuance even if all circulating CKB were locked in DAO.

### Mainnet Genesis Special Burn Cell

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

The API returns `cellType: "genesis_special_burn"` for a policy-matched cell. Its
`virtualOccupiedCapacity` comes from `GenesisBaseline.virtual_occupied` (5.04B CKB for the
mainnet example), not from a response-layer constant.

### Supply Terminology

RFC-0023 calls DAO field `C` total issuance, but field `S` is the portion of
secondary issuance that is still unissued. Therefore user-facing circulation
must subtract the complete `S` pool, not only its treasury portion.

| Term                          | Definition                                             | Mainnet genesis (rounded) |
| ----------------------------- | ------------------------------------------------------ | ------------------------- |
| `total_issuance`              | DAO field `C` (genesis + primary + secondary schedule) | 33.6B                     |
| `unissued_secondary`          | DAO field `S` (unmade DAO interest + treasury)         | ~0                        |
| `secondary_treasury`          | `S - unclaimed_compensation`                           | ~0                        |
| `protocol_circulating`        | `C - GenesisBaseline.burnt - S`                        | ~25.2B                    |
| `liquid`                      | `protocol_circulating - locked_in_dao`                 | varies                    |
| `explorer_policy_locked`      | Explorer-labelled vesting and Bug Bounty balances      | varies                    |
| `explorer_circulating_supply` | `protocol_circulating - explorer_policy_locked`        | varies                    |

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
    pub total_issuance: u64,      // C - includes genesis burnt (≈33.6B on mainnet)
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
const SHANNON: u64 = 100_000_000;                   // 1 CKB = 10^8 shannons
const SECONDARY_ISSUANCE_PER_YEAR: u64 = 1_344_000_000 * SHANNON; // 1.344B CKB/year
const EPOCHS_PER_YEAR: f64 = 2_190.0;
const EPOCHS_PER_HALVING: f64 = 8_760.0;
```

There is deliberately no global `GENESIS_BURNT` calculation constant. Likewise, 102 CKB is only
the occupied capacity of a standard secp256k1 DAO cell; compensation uses the exact occupied
capacity of the cell RFC-0023 counts, persisted per deposit.

## 1. Individual Deposit Compensation

For a single DAO deposit, compensation is calculated using AR growth:

```
free_capacity = capacity - counted_occupied_capacity
compensation = free_capacity * (AR_withdraw / AR_deposit) - free_capacity
             = free_capacity * (AR_withdraw - AR_deposit) / AR_deposit
```

Where:

- `capacity`: Total CKB locked in the deposit (in shannons)
- `counted_occupied_capacity`: Exact occupied capacity of the cell RFC-0023
  counts (see below), derived from its capacity field, lock script, type
  script, and data
- `AR_deposit`: AR value at deposit block
- `AR_withdraw`: AR value at the phase-1 withdraw-request block

### Which cell's occupied capacity is counted

RFC-0023 derives `counted_capacity` from the **withdrawing cell** — the phase-1
request cell — not from the original deposit cell. The DAO type script does not
enforce lock preservation, so a withdraw request may carry a different lock and
therefore a different occupied capacity than the deposit it consumed.

| Deposit state              | Occupied capacity used                                         |
| -------------------------- | -------------------------------------------------------------- |
| Status 0 — still accruing  | The deposit cell's own occupied capacity (no request cell yet) |
| Status 1 — phase-1 frozen  | The **withdraw-request cell's** occupied capacity              |
| Status 2 — phase-2 claimed | The **withdraw-request cell's** occupied capacity              |

`DaoDepositCacheEntry` therefore persists both: `occupied_capacity` at deposit
and `withdraw_request_occupied_capacity` at phase-1. A status ≥ 1 entry missing
the request value fails loudly; it never falls back to the deposit cell.

The familiar 102 CKB value applies to a standard secp256k1 DAO cell. It is not
a protocol-wide constant for cells using different lock scripts — the observed
divergent shape on mainnet is a 33-byte-args lock at deposit withdrawing into a
standard 20-byte-args secp lock, a 13 CKB difference.

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
- `C` = exact `GenesisBaseline.total_issuance` + theoretical cumulative primary + secondary
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
- API wrapper: `crates/api/src/routes/dao.rs::estimated_apc_from_state()`

## 3. Secondary Issuance Components

CKB's secondary issuance is reflected through DAO header fields and individual
deposit AR growth. These are related protocol quantities, but they are not
interchangeable calculation paths.

### 3.1 Miner secondary issuance

Block `i`'s miner share is the protocol's own direct split (RFC-0023), using
the DAO header state of the PARENT block:

```
s_i    = per-block secondary issuance from the epoch schedule
miner_i = floor(s_i * U_(i-1) / C_(i-1))
```

`s_i` derives from the block header's packed epoch field plus the consensus
`secondary_epoch_reward`: the epoch reward is divided evenly over the epoch and
the division remainder is distributed as +1 shannon to the first
`secondary_epoch_reward % epoch_length` blocks — the same rule CKB applies to
the primary epoch reward. `secondary_epoch_reward` is fetched from
`get_consensus` at indexer startup, persisted in sync meta, and re-verified on
every restart so a network mismatch surfaces immediately.

This is the value the node reports as `miner_reward.secondary` in
`get_block_economic_state`. Cumulative miner secondary issuance is the exact
sum of these per-block values.

**Never reconstruct it** from the secondary-pool delta plus claimed
compensation (`(S_N - S_(N-1)) + claimed`, split by `U/(C-U)`). That
reconstruction couples the mining series to ckbadger's own DAO claim
recognition and carries an inherent flooring drift; it broke the on-chain
identity `cum_miner + cum_claimed = cum_secondary - (S_tip - S_0)`. Claimed
compensation feeds only the compensation aggregates.

Genesis carries its own primary+secondary share inside the genesis DAO `C`
field and has no parent to split against — the node defines no miner reward for
block 0 — so block 0 only seeds the parent C/U state. A missing parent C/U or
an invalid C/U relationship fails with block and date context; there is no
zero-clamp.

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

Live batches stage daily snapshots in the atomic domain write, then perform the
same exact lifecycle calculation after the deposit mutations are committed. If
a batch crosses a UTC+8 day boundary, the post-commit materialization finalizes
every completed date at its own last block and AR before refreshing the current
tip date. A completed day must never retain the pre-commit value carried from
the preceding batch.

### 3.3 Treasury

The treasury series is derived directly from the on-chain secondary pool and
the total unmade DAO interest:

```
treasury = S - unclaimed_compensation
```

`S` shrinks only when a phase-2 completion transaction subtracts
`withdrawed_interests`, so at any observation block it still holds the interest
owed to live deposits **and** the interest already frozen on phase-1
withdraw-request cells. Both are in `unclaimed_compensation`, so both must come
out of treasury.

Subtracting only `active_unmade` (the status-0 share) leaves the phase-1 frozen
amount inside treasury while `cum_dao_compensation = claimed + unclaimed`
already counts it — the same shannons in two buckets of one partition, which
overstated mainnet's secondary-issuance breakdown by 77,489,937.99 CKB
(see POSTMORTEM DAO-028). `active_unmade` is a diagnostic split only and is
never a valid treasury summand.

The partition invariant, which every write path must preserve:

```
treasury + unclaimed_compensation == S
cum_miner_secondary + cum_dao_compensation + treasury == Σ secondary issuance
```

`dao_treasury_split(secondary_pool, unclaimed)` in `ckbadger-store` is the sole
implementation, shared by the live writer, the bulk reducer, the store
recompute path, and the API read path.

### 3.4 Data sources and implementation

- `C`, `S`, `U`, and `AR`: exact DAO fields from every block header
- `claimed_compensation_in_block`: exact phase-2 lifecycle transition
- Per-deposit capacity, deposit and withdraw-request occupied capacities,
  deposit AR, request AR, and lifecycle block numbers: domain-store
  `dao_deposits`
- Per-block secondary schedule:
  `crates/common/src/dao.rs::secondary_block_issuance()`
- Miner arithmetic:
  `crates/common/src/dao.rs::calculate_miner_secondary_issuance()`
- Per-block miner accumulation:
  `crates/indexer/src/sync/dao_helpers.rs::accumulate_miner_secondary_for_block()`
  (applied by all three twins: live sync `sync/batch.rs`, bulk build
  `bulk_build/owners/dao.rs`, and the reorg snapshot recompute
  `ckbadger-store/src/stats_ops.rs`)
- Exact bulk compensation timeline:
  `crates/indexer/src/sync/bulk_build/owners/dao.rs`
- Exact live post-commit materialization:
  `crates/indexer/src/db/writer/statistics.rs`

## 4. Storage Schema (RocksDB)

DAO-related state is split across several CFs:

| CF / Data             | Key                                                     | Value                            | Purpose                                                                                                                                             |
| --------------------- | ------------------------------------------------------- | -------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------- |
| `dao_deposits`        | `tx_hash + output_index`                                | `DaoDepositCacheEntry`           | Deposit lifecycle plus original capacity, deposit and withdraw-request occupied capacities, deposit/request ARs, and validated claimed compensation |
| `dao_by_withdraw_tx`  | `withdraw_outpoint` (34B)                               | `deposit_outpoint_key`           | Fast lookup on withdraw completion (keyed by withdraw request outpoint)                                                                             |
| `dao_by_block`        | `block_desc (8B BE) + outpoint (34B)`                   | empty                            | Newest-first global DAO deposit index                                                                                                               |
| `dao_by_lock_block`   | `lock_hash (32B) + block_desc (8B BE) + outpoint (34B)` | empty                            | Newest-first DAO deposit index scoped by lock hash                                                                                                  |
| `dao_by_status_block` | `status (2B BE) + block_desc (8B BE) + outpoint (34B)`  | empty                            | Newest-first DAO deposit index scoped by status                                                                                                     |
| `stats_dao`           | date / fixed summary keys                               | `DaoDailySnapshot` and summaries | Daily cumulative series (`total_issuance`, `secondary_pool`, `occupied_capacity`, `cum_miner_secondary`, `cum_dao_compensation`, `cum_treasury`)    |

## 5. Update Triggers

| Statistic                       | Trigger                            | Function / owner                                                                                                                                    |
| ------------------------------- | ---------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------- |
| Miner secondary issuance        | Every processed block              | `accumulate_miner_secondary_for_block()`                                                                                                            |
| Bulk daily deposit compensation | Bulk final materialization         | `DaoCompensationTimeline`                                                                                                                           |
| Live daily deposit compensation | After each live domain-store batch | `BatchWriter::refresh_latest_dao_statistics()` (tip day + DAO singletons only; completed days are materialized exact inside their own atomic batch) |
| Reorg cutoff-date repair        | After partial-day rollback         | `CkbadgerStore::recompute_dao_daily_snapshot_for_date()` + `repair_cutoff_date_stats`                                                               |

> **Note:** Estimated APC served by DAO APIs is derived from the latest cached header's epoch,
> protocol issuance schedule, and `GenesisBaseline.total_issuance`; it is not read from a
> periodically persisted APC value.

## 6. Charts Data

### Total Supply Chart

Shows stacked area of CKB distribution:

```
total_issuance = DAO field C
unissued_secondary = DAO field S
secondary_treasury = S - unclaimed_compensation
total_burnt = GenesisBaseline.burnt + secondary_treasury
protocol_circulating =
    total_issuance - GenesisBaseline.burnt - unissued_secondary
liquid = protocol_circulating - locked_in_dao
```

| Layer                | Calculation                                  | At Genesis |
| -------------------- | -------------------------------------------- | ---------- |
| Circulating (liquid) | `protocol_circulating - locked_in_dao`       | ~25.2B     |
| Locked in DAO        | active DAO deposit principal                 | ~0         |
| Burnt                | `GenesisBaseline.burnt + secondary_treasury` | 8.4B       |

Outstanding `unclaimed_compensation` (interest accruing on live deposits plus
interest frozen on phase-1 withdraw-request cells) is not circulating yet and is
not treasury/burnt. Consequently the three displayed layers sum to
`C - unclaimed_compensation`.

**Implementation**: `crates/api/src/routes/statistics.rs::get_total_supply_chart()`

### Total Deposit Chart

- Source: `DaoDailySnapshot.total_deposited`
- Shows cumulative CKB locked in DAO over time

### Circulation Ratio Chart

Shows percentage of **circulating** CKB locked in DAO:

```
protocol_circulating = C - GenesisBaseline.burnt - S
ratio = dao_deposits / protocol_circulating * 100
```

**Note**: Uses realized circulating supply, excluding both the genesis burn and
the complete unissued secondary pool.

**Implementation**: `crates/api/src/routes/dao.rs::get_circulation_ratio_chart()`

### Secondary Issuance Chart

Shows the exact cumulative secondary-issuance amounts materialized in each DAO daily snapshot:

```
compensation = cum_dao_compensation
mining = cum_miner_secondary
burnt = secondary_pool - unclaimed_compensation
protocolTotalShannons = cum_miner_secondary + secondary_pool + claimed_compensation
```

The API converts each shannon value to whole CKB for the stacked-area series. It does not
reconstruct these amounts from capacity percentages: miner issuance is the sum of exact
per-block deltas, DAO compensation follows individual deposit lifecycles, and burnt/treasury is
the exact residual of `S`. `protocolTotalShannons` remains in shannons and is not a displayed
stack; it is the independent protocol total used to normalize external partitions without
deriving the expected result from ckbadger's treasury value.

**Implementation**: `crates/api/src/routes/statistics.rs::get_secondary_issuance_chart()`

### Nominal APC Chart

Shows theoretical APC over time (0-20 years):

```
genesis_circulating =
    GenesisBaseline.total_issuance - GenesisBaseline.burnt
total_supply = genesis_circulating + primary_issued + secondary_issued
APC = (SECONDARY_ISSUANCE_PER_YEAR / total_supply) * 100
```

For mainnet, `genesis_circulating` is approximately 25.2B CKB. The API derives its exact value
from the persisted baseline for the active network.

**Implementation**: `crates/api/src/routes/statistics.rs::calculate_nominal_apc()`

### Realized Inflation Chart

Shows the exact trailing-365-complete-day issuance rate derived from DAO daily
snapshots. It is a historical chain measurement, not a future issuance
projection:

```
nominal_issuance =
    C_today - C_365_days_ago

cumulative_secondary =
    cum_miner_secondary + secondary_pool + cumulative_claimed_compensation

secondary_issuance =
    cumulative_secondary_today - cumulative_secondary_365_days_ago

primary_issuance =
    nominal_issuance - secondary_issuance

nominal_inflation =
    nominal_issuance / C_365_days_ago

real_inflation =
    primary_issuance / C_365_days_ago
```

Rates are calculated with checked integer arithmetic and truncated only when
formatted to four decimal percentage places. The current incomplete UTC+8 day
is excluded. The persisted DAO series is sparse because snapshot dates are
driven by blocks, so a complete calendar day with no blocks may have no row.
The API first verifies such a gap against canonical block headers and then
carries the preceding end-of-block state forward exactly; this is not
interpolation because chain state did not change. If a canonical block exists
on a date whose DAO snapshot is missing, or if a cumulative value decreases,
the request fails with the affected date and block context.

`secondary_pool` is DAO header `S`, the non-miner secondary issuance that has
not yet been claimed. Adding cumulative claimed compensation restores the
amount that left `S`; adding `cum_miner_secondary` then gives total secondary
issuance. The corrected secondary-issuance partition also sums to that total,
but this independent `miner + S + claimed` identity deliberately avoids using
`cum_treasury`, so it can verify the treasury split instead of assuming it.

**Implementation**:
`crates/api/src/routes/statistics.rs::build_inflation_rate_response()`

## 7. Common Pitfalls

### Confusing total_issuance with circulating

**Wrong**: Using DAO `total_issuance` as circulating supply.

**Wrong**: Subtracting only treasury/burnt secondary issuance. That leaves
unmade DAO interest in circulation before it has been claimed.

**Correct**: `protocol_circulating = C - GenesisBaseline.burnt - S`
(approximately 25.2B at mainnet genesis)

DAO field `C` includes the network's genesis burn and the scheduled secondary
issuance accumulated in `S`. RFC-0023 defines `S` as unissued, so both must be
removed from user-facing circulating supply.

### Comparing protocol circulation directly with explorer circulation

**Wrong**: Comparing ckbadger `C - GenesisBaseline.burnt - S` directly with the
official explorer's `circulating_supply`, then widening the tolerance to hide a
stable offset.

**Correct**: Compare with
`explorer circulating_supply + explorer locked_capacity`. The explorer
subtracts policy-labelled balances that are not excluded by CKB consensus; its
`locked_capacity` series publishes that exact adjustment.

### Hardcoding Rounded Mainnet Genesis Supply

**Wrong**: using either `33.6B` or `25.2B` as a cross-network literal.

**Correct**:

- Estimated APC seeds its theoretical issuance model with exact
  `GenesisBaseline.total_issuance`.
- The nominal APC chart seeds its supply curve with exact
  `GenesisBaseline.total_issuance - GenesisBaseline.burnt`.

### Incorrect APC Formula

**Wrong for estimated APC**:
`secondary_issuance_per_year / circulating_supply * 100`.

Ignores primary issuance dilution (alpha factor) and doesn't match the CKB Explorer.

**Wrong**: `APC = secondary_issuance_per_year / total_issuance * 100`

Closer but still doesn't account for the alpha factor or continuous compounding.

**Correct for estimated APC**: Use the continuous compounding model with
`rate = ln(1 + (alpha+1) * sn / C) / (alpha+1)`. See formula above.

The nominal APC chart is a separate theoretical visualization and intentionally uses simple
secondary issuance divided by its modeled supply curve. Do not substitute it for the DAO
statistics `estimated_apc`.

### Secondary Issuance Attribution

**Wrong**: Assuming most goes to miners

- Mining reward is proportional to occupied capacity (~10% of total issuance)

**Correct**: Most is burnt (~70%) because most CKB is liquid

- Only CKB locked in DAO earns compensation
- Free-floating CKB's share is burnt

### Choosing the Correct Supply Base

| Use Case                                        | Base                                                                |
| ----------------------------------------------- | ------------------------------------------------------------------- |
| Secondary issuance percentage distribution      | DAO field `C`                                                       |
| Estimated APC                                   | Theoretical issuance seeded by `GenesisBaseline.total_issuance`     |
| Nominal APC chart                               | Modeled supply seeded by `baseline.total_issuance - baseline.burnt` |
| Protocol circulating supply / circulation ratio | `C - baseline.burnt - S`                                            |
| Total Supply Chart circulating layer            | Protocol circulation minus DAO-locked principal                     |

### DAO Compensation vs Secondary Allocation

These are different concepts:

- **DAO Compensation**: What depositors receive based on AR growth (individual deposits)
- **Secondary Allocation to DAO**: Portion of secondary issuance allocated to DAO pool (affects AR growth)

## 8. Common Knowledge Size

**Common Knowledge** is a core CKB concept referring to state verified by global consensus and accepted by all network participants. The set of all live cells represents the current common knowledge on CKB.

### Formula

```
knowledge_size = U - GenesisBaseline.virtual_occupied
```

Where:

- `U` = DAO field bytes 24-31 (occupied capacity in shannons)
- `GenesisBaseline.virtual_occupied` = chain-derived burnt capacity multiplied by the active
  network's exact burn-policy ratio

### Why the Burn Adjustment?

For mainnet, the 8.4B CKB burnt at genesis is "issued but not circulating". Of this:

- **5.04B (60%)** is treated by the declared burn policy as virtual occupied capacity
- **3.36B (40%)** remains on the liquid side of the protocol allocation model

The `U` field includes this virtual occupied capacity. Since the burn cell does not represent
real stored common knowledge, ckbadger subtracts the persisted per-network value.

### Constants

```rust
let baseline = store.get_genesis_baseline()?.expect("required invariant");
// mainnet example:
// baseline.burnt            = 840_000_000_000_000_000 shannons
// baseline.virtual_occupied = baseline.burnt * 6 / 10
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
pub fn calculate_knowledge_size(
    dao_field: &[u8],
    virtual_occupied: i128,
) -> Option<i128> {
    if dao_field.len() < 32 { return None; }
    let u_field = u64::from_le_bytes(dao_field[24..32].try_into().ok()?);
    Some(u_field as i128 - virtual_occupied)
}
```

**API** (`crates/api/src/routes/statistics.rs`):

- Endpoint: `GET /api/v1/charts/knowledge-size`
- Returns daily values in shannons as strings (for precision)
- Frontend converts to CKB for display

### Data Flow

1. Each block's DAO field is stored during indexing
2. `update_daily_statistics()` extracts U field from the last block of each day
3. Reads the required `GenesisBaseline.virtual_occupied`
4. Calculates `knowledge_size = U - baseline.virtual_occupied`
5. Stores the result in the daily chain-statistics record
6. API serves historical chart data

### Reference

The formula matches the official CKB Explorer implementation:

```ruby
# Official explorer (Ruby)
knowledge_size = dao.U - (BURN_QUOTA * 0.6)
```
