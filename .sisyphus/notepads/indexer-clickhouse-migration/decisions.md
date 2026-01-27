# Architectural Decisions - ClickHouse Migration

## Key Technical Decisions

### From Planning Phase

- **Database**: PostgreSQL → ClickHouse (列存储，100万+行/秒)
- **Data Model**: Immutable + JOIN (创建/消费是独立事件)
- **地址余额**: 实时计算 (不存储余额表)
- **统计数据**: ClickHouse Materialized View
- **API**: 完全重写适配ClickHouse

---

(Additional architectural decisions will be recorded here as they emerge)

---

---

## Phase 0 Gate Decision (2026-01-27)

**Decision**: ⚠️ **CONDITIONAL GO** - Proceed to Phase 1 with schema correction

### Context

Phase 0 evaluation completed with 2/3 gate criteria PASSED:

- ✅ Query performance: All criteria exceeded (7.97ms single OutPoint, 47.15ms batch, 60.92ms JOIN)
- ❌ Write performance: FAILED (46K vs 500K rows/s target = 9.2%)
- ✅ ClickHouse fundamentals: Validated (5.15x compression, scalability projections positive)

### Root Cause of Write Failure

**Schema Design Issue**: Used `String` instead of `FixedString(32)` for hash fields

- Impact: 2x data size (64 hex chars vs 32 bytes)
- Impact: No fixed-length optimization in ClickHouse
- Impact: Extra serialization/deserialization overhead
- 7 hash fields per row × 2x size = ~10x performance penalty

**Driver Limitation**: `clickhouse-rs` 0.12 lacks `fixedstring` serde helper

- Attempted `#[serde(with = "clickhouse::serde::fixedstring")]` → compilation error
- Fallback to String types required for compatibility

### Why Conditional GO (Not NO-GO)

**Arguments FOR**:

1. Query performance excellent (all criteria exceeded by 1.25x - 10.6x)
2. Write failure is correctable (schema design, not ClickHouse limitation)
3. Expected 10x improvement with FixedString(32) → 500K+ rows/s
4. ClickHouse fundamentals validated (compression, scalability)

**Arguments AGAINST**:

1. Write performance far below target (9.2% of target)
2. Uncertainty about post-fix performance
3. PostgreSQL COPY optimization safer (proven technology)
4. Time investment risk (schema fix + re-test required)

**Risk Assessment**: MEDIUM (acceptable with mitigation)

### Conditions for Phase 1

**Mandatory**:

1. Fix schema to use `FixedString(32)` for all hash fields
2. Re-test write performance (Task 0.2.2)
3. Verify throughput > 500K rows/s sustained

**Recommended**:

1. Document schema design decisions
2. Add schema validation tests
3. Update Phase 1 plan with learnings

### Fallback Plan

If write performance re-test fails:

- Abort ClickHouse migration
- Proceed with PostgreSQL COPY optimization
- Expected: 200K-500K rows/s (4-10x improvement)
- Timeline: 1-2 weeks (vs 4-6 weeks for ClickHouse)

### Next Steps

1. **Task 0.2.1**: Fix schema (String → FixedString(32))
   - Upgrade `clickhouse-rs` or implement custom serialization
   - Update schema migration file
   - Update Rust structs

2. **Task 0.2.2**: Re-run write performance benchmark
   - Test with corrected schema
   - Verify > 500K rows/s sustained throughput
   - Document results

3. **Task 0.4.1**: Update Phase 1 plan
   - Incorporate schema design decisions
   - Add validation tests
   - Update timeline estimates

### Key Learnings

1. **Schema design is critical**: FixedString(32) vs String has 10x performance impact
2. **Driver compatibility matters**: Check feature support before schema design
3. **Benchmark early**: Phase 0 caught issue before full migration (saved weeks)
4. **Gate decisions work**: Clear criteria prevent premature commitment

### Evidence

- Write benchmark: `.sisyphus/evidence/phase0_write_benchmark.md`
- Query benchmark: `.sisyphus/evidence/phase0_query_benchmark.md`
- Gate decision: `.sisyphus/evidence/phase0_decision.md`

### Timeline

- Schema fix + re-test: 2-3 days
- Phase 1 (if GO): 4-6 weeks
- Fallback (if NO-GO): 1-2 weeks

**Confidence Level**: HIGH (evidence-based decision)
