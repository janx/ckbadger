# Issues & Gotchas - ClickHouse Migration

## Known Issues from Research

1. **ClickHouse不支持UPDATE** - 使用Immutable数据模型解决
2. **live_cells表O(1)查找** - 需要特殊设计 (Phase 0验证)
3. **transaction_inputs缺少created_at_block** - JOIN可能昂贵
4. **UNNEST替代** - 使用ClickHouse arrayJoin

---

(Implementation-specific issues will be recorded here)

---
