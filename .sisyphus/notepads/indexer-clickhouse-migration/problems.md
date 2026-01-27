# Unresolved Problems - ClickHouse Migration

## Blockers

(Currently no unresolved blockers)

---

(Any blocking issues will be recorded here immediately when encountered)

---

## Task 3.4: Known Blockers for Full ClickHouse Implementation

### Critical Missing Piece: Data Conversion Logic

**Problem**: The indexer pipeline uses `ParsedBlock`, `ParsedTransaction`, `ParsedCell` types from the parser module, but ClickHouse writer expects `BlockRow`, `TransactionRow`, `CellRow` types.

**Current State**:

- Parser outputs: `crate::parser::block::ParsedBlock`, `crate::parser::transaction::ParsedTransaction`, etc.
- ClickHouse writer expects: `BlockRow`, `TransactionRow`, `CellRow` (defined in `db/clickhouse_writer.rs`)
- No conversion functions exist between these types

**Impact**:

- ClickHouse backend stub exits with error message
- Cannot actually write data to ClickHouse until conversion is implemented

**What's Needed**:

1. Conversion functions in a new module (e.g., `db/clickhouse_converter.rs`):

   ```rust
   pub fn parsed_block_to_row(parsed: &ParsedBlock) -> BlockRow { ... }
   pub fn parsed_transaction_to_row(parsed: &ParsedTransaction, block_number: i64) -> TransactionRow { ... }
   pub fn parsed_cell_to_row(parsed: &ParsedCell, tx_hash: &[u8], output_index: i16) -> CellRow { ... }
   ```

2. Integration in main.rs ClickHouse branch:
   ```rust
   let blocks: Vec<BlockRow> = parsed_blocks.iter()
       .map(|pb| clickhouse_converter::parsed_block_to_row(pb))
       .collect();
   ch_writer.insert_blocks_batch(blocks).await?;
   ```

**Complexity**: Medium

- Field mapping is straightforward (mostly 1:1)
- Some fields need format conversion (hex strings → Vec<u8>)
- Need to handle optional fields carefully

**Recommendation**: Create separate task for conversion logic implementation.

### Secondary Issue: Pipeline Architecture Differences

**Problem**: PostgreSQL indexer uses `Indexer::new()` which encapsulates the entire sync loop. ClickHouse needs similar but with different writer.

**Options**:

1. **Duplicate sync logic** (current stub approach): Separate code paths in main.rs
2. **Generic Indexer**: Make Indexer generic over writer type (complex refactor)
3. **Trait-based writer**: Define `DatabaseWriter` trait, implement for both (cleaner but more work)

**Current Decision**: Option 1 (duplicate) for now, can refactor later if needed.

**Rationale**:

- Fastest path to working implementation
- PostgreSQL code unchanged (no regression risk)
- Can optimize later once ClickHouse is proven

## Task 5.3 - E2E Performance Validation (BLOCKED)

**Status**: Blocked - requires actual ClickHouse deployment and data sync

**What's needed**:

1. ClickHouse server running with production config
2. Full or partial blockchain data sync (testnet or mainnet)
3. Performance measurement tools
4. Data integrity verification

**Why blocked**:

- No ClickHouse deployment available in current environment
- Requires actual blockchain data (not available in test environment)
- Performance testing needs real workload, not unit tests

**Recommendation**:

- Skip for now, mark as TODO for production deployment
- Can be done after deployment to staging/production
- Performance benchmarks from Phase 0 already validate target (449K-503K rows/s)

**Alternative**:

- Document expected performance based on Phase 0 benchmarks
- Create performance testing guide for deployment team
