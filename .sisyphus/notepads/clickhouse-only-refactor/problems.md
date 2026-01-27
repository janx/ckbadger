## Phase 3 Blocker: Indexer ClickHouse Writer Implementation Incomplete

### Issue

Tasks 3.1-3.5 attempted to remove PostgreSQL from the indexer and make it ClickHouse-only. However, the ClickHouseWriter implementation is incomplete and has stub methods with incorrect signatures.

### Current State

- ✅ Config simplified (DatabaseBackend enum removed)
- ✅ Main.rs simplified (PostgreSQL connection removed)
- ✅ db/ module restructured (clickhouse_writer.rs → writer.rs)
- ✅ sqlx dependency removed from Cargo.toml
- ✅ integrity module deleted (PostgreSQL-specific)
- ❌ **ClickHouseWriter has 60+ compilation errors** - stub methods don't match sync/indexer.rs expectations

### Root Cause

The ClickHouseWriter was created as a stub with generic methods like:

```rust
pub async fn insert_dao_deposit<T>(&self, _tx_hash: &[u8], _output_index: i16, _capacity: i64, _block_number: i64) -> Result<()> {
    Ok(()) // Stub
}
```

But sync/indexer.rs calls it with different signatures:

```rust
writer.insert_dao_deposit(deposit, parsed.number, parsed.timestamp, ar)
// Expects: (&ParsedDaoDeposit, i64, DateTime<Utc>, u64)
// Got: (&[u8], i16, i64, i64)
```

### Impact

- Indexer crate does NOT compile
- Cannot run indexer with ClickHouse backend
- Phase 3 is BLOCKED

### Options

**Option A**: Implement full ClickHouseWriter (LARGE task)

- Implement all 50+ methods properly
- Convert ParsedBlock/ParsedTransaction to ClickHouse Row types
- Test each method
- Estimated effort: 8-16 hours

**Option B**: Revert Phase 3 changes

- Keep PostgreSQL in indexer for now
- Focus on completing other phases (tests, docs, cleanup)
- Revisit indexer ClickHouse migration later

**Option C**: Hybrid approach

- Keep current changes (config simplified, integrity removed)
- Add back PostgreSQL writer alongside ClickHouse writer
- Allow both backends to coexist temporarily

### Recommendation

**Choose Option B** - Revert Phase 3 changes for now.

Reasoning:

- The API is already ClickHouse-only (Phase 2 complete)
- The indexer ClickHouse migration is a MUCH larger task than anticipated
- We can complete Phases 4-6 (tests, docs, cleanup) without blocking on this
- The indexer can continue using PostgreSQL while the API uses ClickHouse

### Next Steps

1. Revert Phase 3 changes: `git restore crates/indexer/`
2. Mark Phase 3 as "DEFERRED" in the plan
3. Continue with Phase 4 (Update Tests)
4. Complete Phases 5-6 (Cleanup & Documentation)
5. Revisit Phase 3 as a separate, dedicated effort

### Date

2026-01-27
