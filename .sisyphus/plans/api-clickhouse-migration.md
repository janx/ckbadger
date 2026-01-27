# API Routes ClickHouse Migration Plan

## Context

### Original Request

将 API 层剩余的 4 个文件从 PostgreSQL 迁移到 ClickHouse，完成完整的 ClickHouse-only 迁移。

### Current State

- **Errors**: 71 compilation errors
- **Files**: 4 files still using PostgreSQL (sqlx)
  - `assets.rs`: 24 errors
  - `forks.rs`: 20 errors
  - `spore.rs`: 20 errors
  - `status.rs`: 6 errors

### Migration Pattern

**From (PostgreSQL)**:

```rust
let row = sqlx::query_as::<_, SomeRow>("SELECT ... FROM table WHERE id = $1")
    .bind(id)
    .fetch_one(&state.pool)
    .await?;
```

**To (ClickHouse)**:

```rust
#[derive(clickhouse::Row, serde::Deserialize)]
struct SomeRow { ... }

let query = format!("SELECT ... FROM table WHERE id = {}", id);
let row = state.clickhouse.client()
    .query(&query)
    .fetch_one::<SomeRow>()
    .await?;
```

---

## TODOs

### Phase 1: Migrate status.rs (6 errors - simplest)

- [x] 1.1. Fix SyncStatusRow struct

  **What to do**:
  - The Row struct already has `#[derive(clickhouse::Row, serde::Deserialize)]`
  - Fix the query to use ClickHouse client instead of sqlx

  **Files**: `crates/api/src/routes/status.rs`

---

- [x] 1.2. Migrate get_system_status function

  **What to do**:
  - Replace `sqlx::query_as` with ClickHouse query pattern
  - Use `state.clickhouse.client()` instead of `state.pool`
  - Adapt SQL syntax for ClickHouse (no `$1` placeholders)

  **Files**: `crates/api/src/routes/status.rs`

---

- [x] 1.3. Migrate missing_cycles query

  **What to do**:
  - Convert the missing cycles count query to ClickHouse

  **Files**: `crates/api/src/routes/status.rs`

---

- [x] 1.4. Migrate recent_fixes query

  **What to do**:
  - Convert the recent fixes query to ClickHouse

  **Files**: `crates/api/src/routes/status.rs`

---

### Phase 2: Migrate spore.rs (20 errors)

- [x] 2.1. Migrate list_clusters function

  **What to do**:
  - Convert COUNT query to ClickHouse
  - Convert cluster listing query to ClickHouse
  - Update Row types with clickhouse::Row derive

  **Files**: `crates/api/src/routes/spore.rs`

---

- [x] 2.2. Migrate get_cluster function

  **What to do**:
  - Convert single cluster lookup to ClickHouse

  **Files**: `crates/api/src/routes/spore.rs`

---

- [x] 2.3. Migrate get_spores_by_cluster function

  **What to do**:
  - Convert spores listing query to ClickHouse

  **Files**: `crates/api/src/routes/spore.rs`

---

- [x] 2.4. Migrate list_spores function

  **What to do**:
  - Convert all spores listing query to ClickHouse

  **Files**: `crates/api/src/routes/spore.rs`

---

- [x] 2.5. Migrate get_spore function

  **What to do**:
  - Convert single spore lookup to ClickHouse

  **Files**: `crates/api/src/routes/spore.rs`

---

- [x] 2.6. Migrate get_spores_by_owner function

  **What to do**:
  - Convert owner's spores query to ClickHouse

  **Files**: `crates/api/src/routes/spore.rs`

---

### Phase 3: Migrate forks.rs (20 errors)

- [x] 3.1. Migrate list_forks function

  **What to do**:
  - Convert total count query to ClickHouse
  - Convert reorg events listing to ClickHouse

  **Files**: `crates/api/src/routes/forks.rs`

---

- [x] 3.2. Migrate get_fork function

  **What to do**:
  - Convert single reorg event lookup to ClickHouse

  **Files**: `crates/api/src/routes/forks.rs`

---

- [x] 3.3. Migrate get_orphaned_blocks function

  **What to do**:
  - Convert orphaned blocks query to ClickHouse

  **Files**: `crates/api/src/routes/forks.rs`

---

- [x] 3.4. Migrate get_fork_stats function

  **What to do**:
  - Convert fork statistics query to ClickHouse

  **Files**: `crates/api/src/routes/forks.rs`

---

- [x] 3.5. Migrate get_recent_reorg function

  **What to do**:
  - Convert recent reorg lookup to ClickHouse

  **Files**: `crates/api/src/routes/forks.rs`

---

- [x] 3.6. Migrate resolve_deep_fork function

  **What to do**:
  - Convert deep fork detection query to ClickHouse
  - Convert resolution UPDATE to ClickHouse ALTER TABLE UPDATE

  **Files**: `crates/api/src/routes/forks.rs`

---

### Phase 4: Migrate assets.rs (24 errors - most complex)

- [x] 4.1. Migrate fetch_assets function - tokens query

  **What to do**:
  - Convert tokens COUNT and listing queries to ClickHouse
  - Handle search pattern conversion (LIKE syntax)

  **Files**: `crates/api/src/routes/assets.rs`

---

- [x] 4.2. Migrate fetch_assets function - spore clusters query

  **What to do**:
  - Convert spore_clusters COUNT and listing queries to ClickHouse

  **Files**: `crates/api/src/routes/assets.rs`

---

- [x] 4.3. Migrate fetch_assets function - mnft classes query

  **What to do**:
  - Convert mnft_classes COUNT and listing queries to ClickHouse

  **Files**: `crates/api/src/routes/assets.rs`

---

### Phase 5: Verification

- [ ] 5.1. Run cargo check on API crate

  **What to do**:

  ```bash
  cargo check -p ckbadger-api
  ```

  **Expected**: 0 errors

---

- [ ] 5.2. Run cargo check on full workspace

  **What to do**:

  ```bash
  cargo check
  ```

  **Expected**: 0 errors

---

- [ ] 5.3. Run API tests

  **What to do**:

  ```bash
  cargo test -p ckbadger-api
  ```

  **Expected**: All tests pass

---

## Commit Strategy

| Phase | Message                                         | Files     |
| ----- | ----------------------------------------------- | --------- |
| 1     | `feat(api): migrate status.rs to ClickHouse`    | status.rs |
| 2     | `feat(api): migrate spore.rs to ClickHouse`     | spore.rs  |
| 3     | `feat(api): migrate forks.rs to ClickHouse`     | forks.rs  |
| 4     | `feat(api): migrate assets.rs to ClickHouse`    | assets.rs |
| 5     | `feat(api): complete ClickHouse-only migration` | -         |

---

## Success Criteria

### Verification Commands

```bash
# Must pass
cargo check -p ckbadger-api
cargo check

# Should pass
cargo test -p ckbadger-api
```

### Final Checklist

- [ ] All 71 compilation errors resolved
- [ ] All 4 files migrated to ClickHouse
- [ ] No sqlx imports remain in these files
- [ ] All queries use state.clickhouse.client()
- [ ] Tests pass

---

## Technical Notes

### SQL Syntax Differences

| PostgreSQL             | ClickHouse                    |
| ---------------------- | ----------------------------- |
| `$1, $2, $3`           | Format string or `.bind()`    |
| `LOWER(x) LIKE $1`     | `lower(x) LIKE 'pattern'`     |
| `COALESCE(x, default)` | `COALESCE(x, default)` (same) |
| `true/false`           | `1/0` or `true/false`         |
| `LIMIT $1 OFFSET $2`   | `LIMIT N OFFSET M`            |

### ClickHouse Query Pattern

```rust
// For queries with parameters
let query = format!(
    "SELECT ... FROM table WHERE id = unhex('{}') LIMIT {}",
    hex::encode(&id),
    limit
);
let rows = state.clickhouse.client()
    .query(&query)
    .fetch_all::<RowType>()
    .await?;

// For queries returning hex strings
#[derive(clickhouse::Row, serde::Deserialize)]
struct RowType {
    #[serde(with = "hex_bytes")]  // if needed
    hash: String,  // Use String, decode with hex::decode
    count: u64,
}
```

### Error Handling

```rust
.map_err(|e| ApiError::internal(e.to_string()))?
```

This remains the same pattern.
