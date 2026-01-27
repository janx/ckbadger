# Decisions - API ClickHouse Migration

## [2026-01-27] Migration Strategy

**Decision**: Migrate files in order of increasing complexity

- Phase 1: status.rs (6 errors - simplest)
- Phase 2: spore.rs (20 errors)
- Phase 3: forks.rs (20 errors)
- Phase 4: assets.rs (24 errors - most complex)

**Rationale**: Build confidence with simple migrations first, learn patterns before tackling complex ones.

## [2026-01-27] Notepad System

**Decision**: Track learnings and issues in notepad files
**Path**: `.sisyphus/notepads/api-clickhouse-migration/`
**Purpose**: Share knowledge across task delegations (subagents are stateless)
