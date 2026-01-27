# Problems - API ClickHouse Migration

## Unresolved Blockers

None yet - will be populated if blockers are encountered.

## Test Migration Blocker (Task 5.3)

**Date**: 2026-01-27

**Issue**: API integration tests still use PostgreSQL (sqlx::PgPool)

**Details**:

- File: `crates/api/tests/api_integration.rs`
- Size: 2700 lines
- sqlx::PgPool references: 65
- Compilation errors: 172

**Impact**:

- Cannot run `cargo test -p ckbadger-api` until tests are migrated
- Tests need to be updated to use ClickHouse client instead of sqlx

**Scope**:
This is a significant migration task that was not included in the original plan. The plan only covered migrating the 4 API route files (status.rs, spore.rs, forks.rs, assets.rs), not the test suite.

**Recommendation**:

1. Mark Task 5.3 as blocked
2. Create a separate plan for test migration
3. Consider this phase "complete" for the route files migration
4. Tests can be migrated in a follow-up task

**Workaround**:
The API route files are fully migrated and compile successfully. The application can run, but tests cannot be executed until they are also migrated.
