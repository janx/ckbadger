## Task 4.2.3 - Cells.rs ClickHouse Migration

### Endpoints Migrated

Successfully migrated 3 cell-specific endpoints to ClickHouse hybrid pattern:

1. `GET /cells/live` - list_live_cells
2. `GET /cells/by-script` - list_cells_by_script
3. `GET /cells/{tx_hash}/{output_index}` - get_cell

### Key Implementation Patterns

#### Live Cells Query Pattern

Used LEFT ANTI JOIN to identify live cells:

```sql
SELECT c.* FROM cells c
LEFT ANTI JOIN cell_consumptions cc
  ON c.tx_hash = cc.tx_hash AND c.output_index = cc.output_index
WHERE [filters]
```

This pattern efficiently excludes consumed cells without requiring a status column.

#### Hybrid Architecture

- **ClickHouse**: Primary query execution for cells and cell_consumptions tables
- **PostgreSQL**:
  - Total count queries (using live_cells view and script_usage_stats)
  - DAO info lookups (dao_deposits table)
  - Code cell script lookups (known_scripts table)
  - UDT amounts (not yet in ClickHouse)

#### Filter Handling

- Lock script hash: Supports both CKB address and raw hash formats
- Type script hash: Direct hex comparison
- Type code hash: Direct hex comparison
- Combined filters: Multiple WHERE clauses with AND

#### Cell Detail Endpoint

Two-query approach:

1. Query cells table for cell data
2. Query cell_consumptions table to check if consumed
3. Fallback to PostgreSQL for:
   - DAO info (dao_deposits)
   - Code cell scripts (known_scripts)
   - Full cell data if truncated

### Response Format Preservation

All endpoints maintain exact same response format as PostgreSQL version:

- CellResponse: tx_hash, output_index, capacity, lock_script_hash, type_script_hash, type_code_hash, data_size, created_at_block, cell_type, virtual_occupied_capacity, udt_amount
- CellDetailResponse: Includes lock/type scripts, address, consumption status, dep_group info, code_cell_of, dao_info

### Known Limitations

- UDT amounts not yet in ClickHouse (returns None)
- Total counts still from PostgreSQL (could optimize with ClickHouse aggregates)
- Cell data may be truncated in ClickHouse (falls back to PostgreSQL cell_data table)

### Verification Results

- ✅ cargo build -p ckbadger-api: PASSED
- ✅ cargo clippy -p ckbadger-api: PASSED
- ✅ cargo test -p ckbadger-api: PASSED (57 tests)

### Address Endpoints Not Migrated

The following endpoints remain PostgreSQL-only (not cell-specific):

- `/addresses/top` - get_top_addresses
- `/addresses/active` - get_active_addresses
- `/addresses/{addr}` - get_address
- `/addresses/{addr}/transactions` - get_address_transactions
- `/addresses/{addr}/tokens` - get_address_tokens
- `/addresses/{addr}/asset-transfers` - get_address_asset_transfers

These involve complex joins with address_balances, token_balances, and address_transactions tables that are not yet in ClickHouse.
