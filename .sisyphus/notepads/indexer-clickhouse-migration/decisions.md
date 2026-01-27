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
