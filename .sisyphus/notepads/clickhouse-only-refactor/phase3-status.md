# Phase 3 Status: Indexer ClickHouse Migration

## Completed Tasks: 1/5

### ✅ Task 3.1: Simplify indexer config (COMPLETE)

- Removed `DatabaseBackend` enum
- Removed `database_url` field (PostgreSQL)
- Made `clickhouse_url` required (not Optional)
- Removed `--database` CLI argument
- Removed `DATABASE_BACKEND` env var
- Indexer compiles successfully

## Blocked Tasks: 4/5

### ⏸️ Task 3.2: Simplify indexer main.rs

**Status**: BLOCKED by incomplete ClickHouseWriter  
**Current State**: main.rs exits with TODO message  
**Blocker**: Requires working ClickHouseWriter with all methods implemented

### ⏸️ Task 3.3: Remove PostgreSQL writer module

**Status**: BLOCKED by incomplete ClickHouseWriter  
**Attempted**: Renaming clickhouse_writer.rs → writer.rs breaks compilation  
**Blocker**: sync/indexer.rs imports BatchWriter, ReorgResult, SecondaryIssuanceBreakdown from old writer  
**Required**: ClickHouseWriter must implement these types

### ⏸️ Task 3.4: Remove sqlx from indexer dependencies

**Status**: BLOCKED by tasks 3.2-3.3  
**Blocker**: Cannot remove sqlx until PostgreSQL writer is fully replaced

### ⏸️ Task 3.5: Update sync module for ClickHouse-only

**Status**: BLOCKED by tasks 3.2-3.4  
**Blocker**: sync/indexer.rs uses PostgreSQL writer extensively

## Root Cause: Incomplete ClickHouseWriter

The ClickHouseWriter (`crates/indexer/src/db/clickhouse_writer.rs`) is a stub with only basic structure. It needs:

1. **Missing Types** (from PostgreSQL writer):
   - `BatchWriter` trait/struct
   - `ReorgResult` struct
   - `SecondaryIssuanceBreakdown` struct

2. **Missing Methods** (50+ methods):
   - `insert_block()`
   - `insert_transaction()`
   - `insert_cell()`
   - `insert_dao_deposit()`
   - `insert_udt_info()`
   - ... and 45+ more

3. **Missing Conversion Logic**:
   - `ParsedBlock` → ClickHouse `BlockRow`
   - `ParsedTransaction` → ClickHouse `TransactionRow`
   - `ParsedCell` → ClickHouse `CellRow`
   - etc.

## Estimated Effort

**8-16 hours** of focused development work:

- 2-4 hours: Implement missing types and trait definitions
- 4-8 hours: Implement 50+ writer methods
- 2-4 hours: Implement conversion logic and test

## Recommendation

**Defer Phase 3 tasks 3.2-3.5** until ClickHouseWriter is fully implemented as a dedicated effort.

Current state is stable:

- ✅ Indexer compiles
- ✅ Config is ClickHouse-only
- ✅ No breaking changes
- ⏸️ Indexer exits with clear TODO message

## Date

2026-01-27
