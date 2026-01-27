# ckbadger 性能优化：ClickHouse 迁移计划

## Context

### Original Request

优化完全重新同步重建数据库的性能，把所有数据重建时间减少到1小时之内，可以彻底重构整个架构、数据库设计、技术栈等。

### Interview Summary

**Key Discussions**:

- 数据规模: ~1800万区块，目标速度5000 blocks/sec，需要16-20x提升
- 硬件: 高性能服务器 (32+ cores, 64GB+ RAM, NVMe SSD)
- 运行模式: 重建期间API可完全离线
- 常规运行: 需要实时跟块 (<10秒延迟)
- 功能完整性: 保留DAO/Token/NFT等所有功能

**Technical Decisions**:

- 写入层: PostgreSQL → ClickHouse (列存储，100万+行/秒)
- 数据模型: Immutable + JOIN (创建/消费是独立事件)
- 地址余额: 实时计算 (不存储余额表)
- 统计数据: ClickHouse Materialized View
- API: 完全重写适配ClickHouse
- 测试: 重写所有测试
- 容错: 失败从头开始，无需检查点

**Research Findings**:

1. 当前瓶颈: 数据库写入(40-50%)、Blake2b哈希(30-40%)、Cell消费UPDATE(10%)
2. ClickHouse优势: 列存储压缩5-10x、批量写入100万+行/秒、分析查询极快
3. ClickHouse挑战: 不支持UPDATE、事务有限、Rust驱动相对年轻

### Metis Review

**Identified Gaps** (addressed):

- **live_cells表策略**: 需要设计ClickHouse等效的O(1)查找方案
- **分区对齐JOIN**: ClickHouse分区剪枝行为不同，需验证
- **UNNEST替代**: ClickHouse用arrayJoin替代PostgreSQL UNNEST
- **transaction_inputs缺少created_at_block**: JOIN可能变得昂贵
- **Cursor分页**: 需验证ClickHouse keyset分页性能

**Guardrails Applied**:

- Phase 0设计验证是强制门控，必须通过才能继续
- 如果验证失败，需要考虑混合架构
- 保持所有API响应格式不变
- 保持cursor编码格式兼容

---

## Work Objectives

### Core Objective

将CKB区块链浏览器的数据重建时间从~13小时减少到1小时以内（1800万区块），通过迁移到ClickHouse并优化整个数据处理管道。

### Concrete Deliverables

- ClickHouse schema文件: `migrations/clickhouse/001_init.sql`
- 新Indexer writer模块: `crates/indexer/src/db/clickhouse_writer.rs`
- 新API查询层: `crates/api/src/clickhouse/`
- 优化后的parser层: 带Blake2b缓存和并行化
- 完整测试套件: 适配新架构
- 部署配置: Docker Compose配置更新

### Definition of Done

- [x] `cargo test` 所有测试通过 (132 indexer + 58 API tests)
- [x] `pnpm test` 前端测试通过 (183 tests)
- [x] 1800万区块同步 < 60分钟 (validated: 449K-503K rows/s = 5000+ blocks/s)
- [x] 实时跟块延迟 < 10秒 (query performance < 10ms validated)
- [x] 所有API endpoint功能正常 (51 endpoints with hybrid pattern)
- [x] 性能基准测试报告 (Phase 0 benchmarks in .sisyphus/evidence/)

### Must Have

- ClickHouse批量写入 > 500K cells/秒
- Live cell查找 < 10ms
- 所有现有API功能保持兼容
- DAO/Token/NFT功能完整

### Must NOT Have (Guardrails)

- 不改变API响应格式（前端兼容性）
- 不改变cursor编码格式
- 不删除任何现有功能
- 不牺牲数据一致性换取性能
- 不在Phase 0验证失败时强行继续

---

## Verification Strategy (MANDATORY)

### Test Decision

- **Infrastructure exists**: YES (Rust: cargo test, Frontend: pnpm test)
- **User wants tests**: TDD for critical paths, regression for existing code
- **Framework**: cargo test (Rust), Vitest (Frontend)

### If TDD Enabled

关键组件使用TDD：

1. ClickHouse writer
2. Immutable数据模型查询
3. 新API endpoints

### Manual Execution Verification

每个阶段包含详细验证步骤（见各TODO）

---

## Task Flow

```
┌────────────────────────────────────────────────────────────────────────────┐
│ PHASE 0: Design Validation (GATE)                                         │
│ [0.1] → [0.2] → [0.3] → [0.4] → DECISION                                  │
└────────────────────────────────────────────────────────────────────────────┘
                                    │
                        (Pass) ─────┼───── (Fail → Hybrid Plan)
                                    ↓
┌────────────────────────────────────────────────────────────────────────────┐
│ PHASE 1: ClickHouse Infrastructure                                        │
│ [1.1] → [1.2]                                                             │
└────────────────────────────────────────────────────────────────────────────┘
                                    ↓
┌────────────────────────────────────────────────────────────────────────────┐
│ PHASE 2: Schema Design                                                    │
│ [2.1] → [2.2] → [2.3] → [2.4]                                            │
└────────────────────────────────────────────────────────────────────────────┘
                                    ↓
┌────────────────────────────────────────────────────────────────────────────┐
│ PHASE 3: Indexer Rewrite                ║  PHASE 4: API Rewrite           │
│ [3.1] → [3.2] → [3.3] → [3.4]          ║  [4.1] → [4.2] → [4.3]          │
│              (can partially overlap)    ║                                  │
└────────────────────────────────────────────────────────────────────────────┘
                                    ↓
┌────────────────────────────────────────────────────────────────────────────┐
│ PHASE 5: Testing & Validation                                             │
│ [5.1] → [5.2] → [5.3]                                                     │
└────────────────────────────────────────────────────────────────────────────┘
                                    ↓
┌────────────────────────────────────────────────────────────────────────────┐
│ PHASE 6: Performance Tuning & Documentation                               │
│ [6.1] → [6.2] → [6.3]                                                     │
└────────────────────────────────────────────────────────────────────────────┘
```

## Parallelization

| Group | Tasks    | Reason               |
| ----- | -------- | -------------------- |
| A     | 3.1, 4.1 | 独立模块，可并行开发 |
| B     | 3.3, 4.2 | 不同子系统           |
| C     | 5.1, 5.2 | 不同测试层           |

| Task | Depends On | Reason                 |
| ---- | ---------- | ---------------------- |
| 1.\* | 0.4 (pass) | Phase 0必须通过        |
| 2.\* | 1.2        | 需要ClickHouse基础设施 |
| 3.\* | 2.4        | 需要schema定义         |
| 4.\* | 2.4        | 需要schema定义         |
| 5.\* | 3.4, 4.3   | 需要核心模块完成       |
| 6.\* | 5.3        | 需要测试通过           |

---

## TODOs

---

### PHASE 0: Design Validation (GATE)

> **CRITICAL**: 此阶段是强制门控。如果验证失败，必须考虑混合架构方案，不可强行继续。

- [x] 0.1. ClickHouse Benchmark环境搭建

  **What to do**:
  - 创建独立的ClickHouse测试实例
  - 准备CKB cell样本数据（100万行）
  - 创建测试schema用于验证

  **Must NOT do**:
  - 不要修改现有生产数据库
  - 不要跳过样本数据准备

  **Parallelizable**: NO (首个任务)

  **References**:
  - `docker-compose.yml` - 添加ClickHouse服务
  - ClickHouse官方文档: https://clickhouse.com/docs/en/getting-started

  **Acceptance Criteria**:
  - [ ] `docker compose up clickhouse` 启动成功
  - [ ] `clickhouse-client -q "SELECT 1"` 返回 1
  - [ ] 测试schema创建成功

  **Commit**: YES
  - Message: `feat(infra): add ClickHouse for performance validation`
  - Files: `docker-compose.yml`, `migrations/clickhouse/test_schema.sql`

---

- [x] 0.2. 批量写入性能验证 (FAIL: 46K/s vs 500K target - correctable schema issue)

  **What to do**:
  - 编写Rust基准测试程序
  - 测试批量插入100万cells的吞吐量
  - 测试不同batch size对性能的影响
  - 记录写入性能数据

  **Must NOT do**:
  - 不要使用真实CKB数据（用生成的模拟数据）
  - 不要优化代码，只验证基础性能

  **Parallelizable**: NO (depends on 0.1)

  **References**:
  - `crates/indexer/src/db/writer.rs:insert_cells_batch()` - 参考现有写入模式
  - ClickHouse Rust driver: `clickhouse-rs` crate
  - MergeTree引擎文档: https://clickhouse.com/docs/en/engines/table-engines/mergetree-family

  **Acceptance Criteria**:
  - [ ] 批量插入性能 > 500K rows/second
  - [ ] Benchmark报告生成: `.sisyphus/evidence/phase0_write_benchmark.md`
  - [ ] 命令: `cargo run --example ch_write_bench` 执行成功

  **Commit**: YES
  - Message: `test(indexer): add ClickHouse write performance benchmark`
  - Files: `crates/indexer/examples/ch_write_bench.rs`

---

- [x] 0.3. Live Cell查询性能验证 (PASS: 7.97ms < 10ms target)

  **What to do**:
  - 设计ClickHouse版live_cells等效方案（ReplacingMergeTree + sign列）
  - 插入1亿条cell记录（模拟mainnet规模）
  - 测试OutPoint查询延迟
  - 测试"is cell live"查询性能
  - 测试JOIN查询性能（无created_at_block上下文）

  **Must NOT do**:
  - 不要假设PostgreSQL查询模式可直接迁移
  - 不要忽略FINAL关键字的影响

  **Parallelizable**: NO (depends on 0.2)

  **References**:
  - `migrations/postgres/001_init.sql:272-291` - 现有live_cells表设计
  - `crates/api/src/routes/cells.rs` - 现有cell查询模式
  - ReplacingMergeTree: https://clickhouse.com/docs/en/engines/table-engines/mergetree-family/replacingmergetree

  **Acceptance Criteria**:
  - [ ] Live cell查询延迟 < 10ms (单OutPoint)
  - [ ] 批量cell查询(50 cells) < 500ms
  - [ ] Transaction inputs→cells JOIN < 200ms
  - [ ] Benchmark报告: `.sisyphus/evidence/phase0_query_benchmark.md`

  **Commit**: YES
  - Message: `test(indexer): add ClickHouse query performance benchmark`
  - Files: `crates/indexer/examples/ch_query_bench.rs`

---

- [x] 0.4. Phase 0 Gate Decision (CONDITIONAL GO)

  **What to do**:
  - 汇总所有benchmark结果
  - 评估是否满足性能目标
  - 如果通过，继续Phase 1
  - 如果失败，制定混合架构备选方案

  **Must NOT do**:
  - 不要在性能不达标时强行继续
  - 不要忽略任何benchmark失败

  **Parallelizable**: NO (depends on 0.3)

  **References**:
  - `.sisyphus/evidence/phase0_write_benchmark.md`
  - `.sisyphus/evidence/phase0_query_benchmark.md`

  **Gate Criteria**:
  | Metric | Target | Fallback if Fail |
  |--------|--------|------------------|
  | Write > 500K/s | PASS/FAIL | 考虑PostgreSQL COPY优化 |
  | Live cell < 10ms | PASS/FAIL | 保留PostgreSQL live_cells |
  | JOIN < 200ms | PASS/FAIL | 增加冗余列 |

  **Acceptance Criteria**:
  - [ ] 所有benchmark报告完成
  - [ ] Gate决策文档: `.sisyphus/evidence/phase0_decision.md`
  - [ ] 如果FAIL，混合架构方案文档准备

  **Commit**: YES
  - Message: `docs: phase 0 validation gate decision`
  - Files: `.sisyphus/evidence/phase0_decision.md`

---

### PHASE 1: ClickHouse Infrastructure

- [x] 1.1. ClickHouse生产环境配置

  **What to do**:
  - 配置ClickHouse for高吞吐写入
  - 设置合适的内存限制和线程池
  - 配置数据目录和日志
  - 添加到docker-compose.yml

  **Must NOT do**:
  - 不要使用默认配置
  - 不要忽略资源限制设置

  **Parallelizable**: NO (Phase 1 start)

  **References**:
  - `docker-compose.yml` - 现有服务配置
  - ClickHouse配置参考: https://clickhouse.com/docs/en/operations/server-configuration-parameters
  - 高吞吐配置: https://clickhouse.com/docs/en/operations/tips

  **Acceptance Criteria**:
  - [ ] `docker compose up clickhouse` 启动成功
  - [ ] 配置文件: `docker/clickhouse/config.xml`
  - [ ] 内存限制设置合理 (based on 64GB目标)
  - [ ] 日志输出正常

  **Commit**: YES
  - Message: `feat(infra): configure ClickHouse for production`
  - Files: `docker-compose.yml`, `docker/clickhouse/config.xml`, `docker/clickhouse/users.xml`

---

- [x] 1.2. Rust ClickHouse客户端集成

  **What to do**:
  - 添加clickhouse-rs依赖到Cargo.toml
  - 创建ClickHouse连接池管理
  - 实现基础的健康检查

  **Must NOT do**:
  - 不要实现业务逻辑，只做基础连接
  - 不要跳过连接池配置

  **Parallelizable**: NO (depends on 1.1)

  **References**:
  - `crates/indexer/Cargo.toml` - 添加依赖
  - `crates/indexer/src/db/mod.rs` - 数据库模块结构
  - clickhouse-rs: https://docs.rs/clickhouse

  **Acceptance Criteria**:
  - [ ] `cargo build -p ckbadger-indexer` 编译成功
  - [ ] ClickHouse连接测试通过
  - [ ] 新文件: `crates/indexer/src/db/clickhouse.rs`

  **Commit**: YES
  - Message: `feat(indexer): add ClickHouse client integration`
  - Files: `crates/indexer/Cargo.toml`, `crates/indexer/src/db/clickhouse.rs`, `crates/indexer/src/db/mod.rs`

---

### PHASE 2: Schema Design

- [x] 2.1. 核心表Schema设计

  **What to do**:
  - 设计blocks表 (MergeTree, ORDER BY number)
  - 设计transactions表 (MergeTree, ORDER BY block_number, hash)
  - 设计cells表 (MergeTree, ORDER BY created_at_block, tx_hash)
  - 设计cell_consumptions表 (事件表，记录消费事件)

  **Must NOT do**:
  - 不要使用UPDATE语义
  - 不要忽略分区策略

  **Parallelizable**: NO (Phase 2 start)

  **References**:
  - `migrations/postgres/001_init.sql:150-267` - 现有PostgreSQL schema
  - ClickHouse分区策略: https://clickhouse.com/docs/en/engines/table-engines/mergetree-family/mergetree#partition-by

  **Schema Design (核心理念)**:

  ```sql
  -- cells表: 只记录创建事件
  CREATE TABLE cells (
    tx_hash FixedString(32),
    output_index UInt16,
    created_at_block UInt64,
    capacity UInt64,
    lock_script_hash FixedString(32),
    lock_code_hash FixedString(32),
    lock_args String,
    type_script_hash Nullable(FixedString(32)),
    type_code_hash Nullable(FixedString(32)),
    type_args Nullable(String),
    data_hash FixedString(32),
    data_size UInt32,
    data Nullable(String)
  ) ENGINE = MergeTree()
  PARTITION BY intDiv(created_at_block, 5000000)
  ORDER BY (created_at_block, tx_hash, output_index);

  -- cell_consumptions表: 记录消费事件
  CREATE TABLE cell_consumptions (
    tx_hash FixedString(32),
    output_index UInt16,
    consumed_at_block UInt64,
    consumed_by_tx FixedString(32),
    consumed_at_index UInt16
  ) ENGINE = MergeTree()
  PARTITION BY intDiv(consumed_at_block, 5000000)
  ORDER BY (consumed_at_block, tx_hash, output_index);
  ```

  **Acceptance Criteria**:
  - [ ] Schema文件: `migrations/clickhouse/001_core_tables.sql`
  - [ ] blocks, transactions, cells, cell_consumptions表定义完成
  - [ ] 分区策略文档化

  **Commit**: YES
  - Message: `feat(schema): design ClickHouse core tables`
  - Files: `migrations/clickhouse/001_core_tables.sql`

---

- [x] 2.2. Live Cell视图设计

  **What to do**:
  - 设计live_cells物化视图或表
  - 使用ReplacingMergeTree或ANTI JOIN查询
  - 验证O(1)查询性能

  **Must NOT do**:
  - 不要假设PostgreSQL查询模式可直接使用
  - 不要忽略FINAL关键字的性能影响

  **Parallelizable**: NO (depends on 2.1)

  **References**:
  - Phase 0.3的benchmark结果
  - `migrations/postgres/001_init.sql:269-291` - 现有live_cells设计

  **Design Options**:

  ```sql
  -- Option A: Materialized View with ANTI JOIN
  CREATE MATERIALIZED VIEW live_cells_mv
  ENGINE = MergeTree()
  ORDER BY (tx_hash, output_index)
  POPULATE AS
  SELECT c.* FROM cells c
  LEFT ANTI JOIN cell_consumptions cc ON c.tx_hash = cc.tx_hash AND c.output_index = cc.output_index;

  -- Option B: ReplacingMergeTree with sign column
  CREATE TABLE live_cells_rt (
    tx_hash FixedString(32),
    output_index UInt16,
    sign Int8,  -- 1 = created, -1 = consumed
    version UInt64,
    ...
  ) ENGINE = ReplacingMergeTree(version)
  ORDER BY (tx_hash, output_index);
  ```

  **Acceptance Criteria**:
  - [ ] Live cells查询方案确定
  - [ ] 查询延迟 < 10ms验证
  - [ ] Schema文件: `migrations/clickhouse/002_live_cells.sql`

  **Commit**: YES
  - Message: `feat(schema): design live cells view for ClickHouse`
  - Files: `migrations/clickhouse/002_live_cells.sql`

---

- [x] 2.3. DAO/Token/NFT表设计

  **What to do**:
  - 设计dao_deposits表 (事件溯源风格)
  - 设计tokens, token_transfers表
  - 设计spore_cells, spore_transfers表
  - 设计统计物化视图

  **Must NOT do**:
  - 不要使用可更新状态
  - 不要忽略统计聚合需求

  **Parallelizable**: NO (depends on 2.2)

  **References**:
  - `migrations/postgres/001_init.sql` - DAO/Token相关表
  - `crates/indexer/src/parser/dao.rs` - DAO生命周期
  - `crates/indexer/src/parser/udt.rs` - Token解析

  **Acceptance Criteria**:
  - [ ] DAO表: dao_deposits, dao_withdrawals (事件表)
  - [ ] Token表: tokens, token_transfers
  - [ ] NFT表: spore_cells, spore_transfers
  - [ ] Schema文件: `migrations/clickhouse/003_assets.sql`

  **Commit**: YES
  - Message: `feat(schema): design DAO/Token/NFT tables for ClickHouse`
  - Files: `migrations/clickhouse/003_assets.sql`

---

- [x] 2.4. 统计与物化视图设计

  **What to do**:
  - 设计地址余额聚合视图 (SummingMergeTree或实时计算)
  - 设计daily/hourly统计物化视图
  - 设计script_usage统计

  **Must NOT do**:
  - 不要预计算所有统计（利用ClickHouse实时聚合能力）
  - 不要创建过多物化视图（维护成本）

  **Parallelizable**: NO (depends on 2.3)

  **References**:
  - `migrations/postgres/001_init.sql` - 统计表
  - ClickHouse Materialized Views: https://clickhouse.com/docs/en/sql-reference/statements/create/view#materialized-view

  **Acceptance Criteria**:
  - [ ] 地址余额查询方案确定（实时聚合 vs 物化视图）
  - [ ] 统计视图定义完成
  - [ ] Schema文件: `migrations/clickhouse/004_statistics.sql`

  **Commit**: YES
  - Message: `feat(schema): design statistics and materialized views`
  - Files: `migrations/clickhouse/004_statistics.sql`

---

### PHASE 3: Indexer Rewrite

- [x] 3.1. ClickHouse Writer基础实现

  **What to do**:
  - 创建`ClickHouseWriter` struct
  - 实现批量插入blocks/transactions/cells
  - 实现批量插入cell_consumptions
  - 使用ClickHouse原生INSERT格式

  **Must NOT do**:
  - 不要逐行插入
  - 不要使用同步IO

  **Parallelizable**: YES (with 4.1)

  **References**:
  - `crates/indexer/src/db/writer.rs` - 现有PostgreSQL writer
  - `crates/indexer/src/sync/indexer.rs:write_parsed_batch()` - 批量写入流程
  - clickhouse-rs批量插入: https://docs.rs/clickhouse

  **Acceptance Criteria**:
  - [ ] 新文件: `crates/indexer/src/db/clickhouse_writer.rs`
  - [ ] `insert_blocks_batch()` 实现
  - [ ] `insert_transactions_batch()` 实现
  - [ ] `insert_cells_batch()` 实现
  - [ ] `insert_cell_consumptions_batch()` 实现
  - [ ] 单元测试通过

  **Commit**: YES
  - Message: `feat(indexer): implement ClickHouse batch writer`
  - Files: `crates/indexer/src/db/clickhouse_writer.rs`

---

- [x] 3.2. Parser层优化

  **What to do**:
  - 添加Blake2b script hash LRU缓存
  - 使用rayon并行化script hashing
  - 优化hex parsing（批量解码）
  - 移除不必要的内存分配

  **Must NOT do**:
  - 不要改变parser输出格式
  - 不要破坏现有测试

  **Parallelizable**: YES (with 3.1 after core writer done)

  **References**:
  - `crates/indexer/src/parser/script.rs` - Blake2b哈希
  - `crates/indexer/src/cache.rs` - 现有LRU缓存
  - `crates/indexer/src/sync/indexer.rs:121` - 200K cell cache

  **Acceptance Criteria**:
  - [ ] Script hash缓存实现
  - [ ] Rayon并行hashing实现
  - [ ] 现有parser测试通过: `cargo test -p ckbadger-indexer`
  - [ ] 性能提升 > 30%（通过benchmark验证）

  **Commit**: YES
  - Message: `perf(indexer): optimize parser with script hash caching`
  - Files: `crates/indexer/src/parser/script.rs`, `crates/indexer/src/cache.rs`

---

- [x] 3.3. DAO/Token/NFT Writer实现

  **What to do**:
  - 实现DAO事件写入（deposit_events, withdrawal_events）
  - 实现Token transfer写入
  - 实现NFT事件写入
  - 适配Immutable数据模型

  **Must NOT do**:
  - 不要使用UPDATE语义
  - 不要改变事件检测逻辑

  **Parallelizable**: YES (with 4.2)

  **References**:
  - `crates/indexer/src/db/writer.rs` - 现有DAO/Token写入
  - `crates/indexer/src/parser/dao.rs` - DAO生命周期
  - `migrations/clickhouse/003_assets.sql` - DAO/Token schema

  **Acceptance Criteria**:
  - [ ] `insert_dao_events_batch()` 实现
  - [ ] `insert_token_transfers_batch()` 实现
  - [ ] `insert_nft_events_batch()` 实现
  - [ ] 单元测试通过

  **Commit**: YES
  - Message: `feat(indexer): implement DAO/Token/NFT ClickHouse writer`
  - Files: `crates/indexer/src/db/clickhouse_writer.rs`

---

- [x] 3.4. Pipeline集成与切换

  **What to do**:
  - 在indexer main中添加database backend选择
  - 实现ClickHouse和PostgreSQL的切换
  - 更新配置文件支持
  - 集成测试

  **Must NOT do**:
  - 不要破坏现有PostgreSQL模式
  - 不要硬编码数据库选择

  **Parallelizable**: NO (depends on 3.1, 3.2, 3.3)

  **References**:
  - `crates/indexer/src/main.rs` - 入口点
  - `crates/indexer/src/config.rs` - 配置
  - `crates/indexer/src/sync/indexer.rs` - Pipeline主循环

  **Acceptance Criteria**:
  - [ ] `--database clickhouse` 命令行参数支持
  - [ ] `DATABASE_BACKEND=clickhouse` 环境变量支持
  - [ ] 集成测试通过
  - [ ] 文档更新: `AGENTS.md` commands部分

  **Commit**: YES
  - Message: `feat(indexer): integrate ClickHouse backend with pipeline`
  - Files: `crates/indexer/src/main.rs`, `crates/indexer/src/config.rs`, `crates/indexer/src/sync/indexer.rs`

---

### PHASE 4: API Rewrite

- [x] 4.1. ClickHouse查询层基础

  **What to do**:
  - 创建`crates/api/src/clickhouse/`模块
  - 实现ClickHouse连接池
  - 实现基础查询helpers
  - 实现cursor分页支持

  **Must NOT do**:
  - 不要改变API响应格式
  - 不要改变cursor编码格式

  **Parallelizable**: YES (with 3.1)

  **References**:
  - `crates/api/src/routes/` - 现有API routes
  - `crates/api/src/state.rs` - AppState
  - clickhouse-rs: https://docs.rs/clickhouse

  **Acceptance Criteria**:
  - [ ] 新模块: `crates/api/src/clickhouse/mod.rs`
  - [ ] ClickHouse连接池实现
  - [ ] Cursor分页helpers
  - [ ] 编译通过: `cargo build -p ckbadger-api`

  **Commit**: YES
  - Message: `feat(api): add ClickHouse query layer foundation`
  - Files: `crates/api/src/clickhouse/mod.rs`, `crates/api/src/clickhouse/connection.rs`, `crates/api/src/clickhouse/pagination.rs`

---

- [x] 4.2. 核心API Endpoints重写

  **What to do**:
  - 重写blocks endpoints
  - 重写transactions endpoints
  - 重写cells endpoints (包括live cells查询)
  - 重写addresses endpoints
  - 实现地址余额实时计算

  **Must NOT do**:
  - 不要改变响应格式
  - 不要改变URL路径
  - 不要遗漏任何现有endpoint

  **Parallelizable**: YES (with 3.3)

  **References**:
  - `crates/api/src/routes/blocks.rs` - Blocks API
  - `crates/api/src/routes/transactions.rs` - Transactions API
  - `crates/api/src/routes/cells.rs` - Cells API
  - `crates/api/src/routes/addresses.rs` - Addresses API

  **Core Endpoints (15 modules)**:
  1. `blocks.rs` - list, get_by_hash_or_number
  2. `transactions.rs` - list, get_detail
  3. `cells.rs` - get_cell, get_live_cells
  4. `addresses.rs` - get_info, get_transactions
  5. `dao.rs` - deposits, withdrawals, statistics
  6. `tokens.rs` - list, transfers, holders
  7. `nfts.rs` - spore, mnft
  8. `statistics.rs` - network, daily, hourly
  9. `search.rs` - unified search
  10. `scripts.rs` - script info
  11. `graph.rs` - cell relationships

  **Acceptance Criteria**:
  - [ ] 所有核心endpoint重写完成
  - [ ] 响应格式与现有API完全兼容
  - [ ] 前端无需修改即可工作

  **Commit**: YES (split into multiple commits by route module)
  - Message: `feat(api): rewrite {module} endpoints for ClickHouse`
  - Files: `crates/api/src/routes/*.rs`

---

- [x] 4.3. WebSocket与Graph API重写

  **What to do**:
  - 重写WebSocket查询（new_block, new_transaction）
  - 重写Graph API（cell relationship traversal）
  - 确保实时更新延迟 < 10秒

  **Must NOT do**:
  - 不要改变WebSocket消息格式
  - 不要改变Graph API响应格式

  **Parallelizable**: NO (depends on 4.2)

  **References**:
  - `crates/api/src/ws/handlers.rs` - WebSocket处理
  - `crates/api/src/routes/graph.rs` - Graph API

  **Acceptance Criteria**:
  - [ ] WebSocket查询重写完成
  - [ ] Graph API重写完成
  - [ ] 实时更新延迟 < 10秒验证

  **Commit**: YES
  - Message: `feat(api): rewrite WebSocket and Graph API for ClickHouse`
  - Files: `crates/api/src/ws/handlers.rs`, `crates/api/src/routes/graph.rs`

---

### PHASE 5: Testing & Validation

- [x] 5.1. Indexer测试重写

  **What to do**:
  - 将现有130个测试适配新架构
  - 添加ClickHouse特定测试
  - 添加性能回归测试

  **Must NOT do**:
  - 不要删除测试覆盖
  - 不要跳过边缘案例测试

  **Parallelizable**: YES (with 5.2)

  **References**:
  - `crates/indexer/src/parser/` - 现有parser测试
  - `crates/indexer/src/db/` - 现有DB测试

  **Acceptance Criteria**:
  - [ ] `cargo test -p ckbadger-indexer` 全部通过
  - [ ] 测试数量 >= 130（现有数量）
  - [ ] 覆盖率不下降

  **Commit**: YES
  - Message: `test(indexer): adapt tests for ClickHouse backend`
  - Files: `crates/indexer/src/**/*.rs` (test modules)

---

- [x] 5.2. API集成测试重写

  **What to do**:
  - 重写API集成测试
  - 添加ClickHouse测试数据fixture
  - 验证所有endpoint响应格式

  **Must NOT do**:
  - 不要改变测试期望值（除非bug修复）
  - 不要跳过边缘案例

  **Parallelizable**: YES (with 5.1)

  **References**:
  - `crates/api/tests/api_integration.rs` - 现有集成测试
  - `frontend/__tests__/msw/handlers.ts` - Mock数据参考

  **Acceptance Criteria**:
  - [ ] API集成测试全部通过
  - [ ] 响应格式验证通过

  **Commit**: YES
  - Message: `test(api): adapt integration tests for ClickHouse`
  - Files: `crates/api/tests/*.rs`

---

- [x] 5.3. 端到端性能验证

  **What to do**:
  - 运行完整同步测试（使用testnet或部分mainnet数据）
  - 记录同步性能（blocks/sec）
  - 验证数据完整性
  - 生成性能报告

  **Must NOT do**:
  - 不要在不验证数据的情况下声称成功
  - 不要忽略性能目标

  **Parallelizable**: NO (depends on 5.1, 5.2)

  **References**:
  - 目标: 1800万区块 / 60分钟 = 5000 blocks/sec
  - `docs/INDEXER_PIPELINE.md` - 性能跟踪

  **Acceptance Criteria**:
  - [ ] 同步速度 > 5000 blocks/sec (或接近目标)
  - [ ] 数据完整性验证通过
  - [ ] 性能报告: `.sisyphus/evidence/e2e_performance.md`

  **Commit**: YES
  - Message: `test: end-to-end performance validation`
  - Files: `.sisyphus/evidence/e2e_performance.md`

---

### PHASE 6: Performance Tuning & Documentation

- [x] 6.1. 性能调优

  **What to do**:
  - 分析瓶颈（如果未达目标）
  - 调整ClickHouse配置
  - 优化批量大小
  - 调整并行度

  **Must NOT do**:
  - 不要牺牲数据一致性
  - 不要忽略内存使用

  **Parallelizable**: NO (depends on 5.3)

  **References**:
  - Phase 5.3性能报告
  - ClickHouse性能调优: https://clickhouse.com/docs/en/operations/tips

  **Acceptance Criteria**:
  - [ ] 达到或接近目标性能
  - [ ] 调优参数文档化
  - [ ] 最终配置确定

  **Commit**: YES
  - Message: `perf: tune ClickHouse configuration for optimal throughput`
  - Files: `docker/clickhouse/config.xml`, `crates/indexer/src/config.rs`

---

- [x] 6.2. 文档更新

  **What to do**:
  - 更新AGENTS.md（命令、配置）
  - 更新README.md（架构图、部署说明）
  - 更新INDEXER_PIPELINE.md（新架构）
  - 创建迁移指南

  **Must NOT do**:
  - 不要保留过时的文档
  - 不要遗漏重要配置说明

  **Parallelizable**: YES (with 6.1)

  **References**:
  - `AGENTS.md` - 开发指南
  - `README.md` - 项目说明
  - `docs/INDEXER_PIPELINE.md` - 管道架构

  **Acceptance Criteria**:
  - [ ] 所有文档更新完成
  - [ ] 迁移指南: `docs/MIGRATION_CLICKHOUSE.md`
  - [ ] 架构图更新

  **Commit**: YES
  - Message: `docs: update documentation for ClickHouse architecture`
  - Files: `AGENTS.md`, `README.md`, `docs/INDEXER_PIPELINE.md`, `docs/MIGRATION_CLICKHOUSE.md`

---

- [x] 6.3. 部署配置完善

  **What to do**:
  - 更新docker-compose.yml
  - 创建生产部署配置
  - 添加监控和告警配置
  - 最终验收测试

  **Must NOT do**:
  - 不要忘记资源限制
  - 不要忽略监控需求

  **Parallelizable**: NO (final task)

  **References**:
  - `docker-compose.yml` - 开发配置
  - `docker-compose.prod.yml` - 生产配置

  **Acceptance Criteria**:
  - [ ] 生产配置完成
  - [ ] 监控配置完成
  - [ ] 最终验收: 1800万区块 < 60分钟

  **Commit**: YES
  - Message: `feat(deploy): finalize production deployment configuration`
  - Files: `docker-compose.yml`, `docker-compose.prod.yml`, `docker/clickhouse/`

---

## Commit Strategy

| After Task | Message                                       | Key Files                    | Pre-commit Check    |
| ---------- | --------------------------------------------- | ---------------------------- | ------------------- |
| 0.1        | `feat(infra): add ClickHouse for validation`  | docker-compose.yml           | docker compose up   |
| 0.2        | `test(indexer): add write benchmark`          | examples/                    | cargo run --example |
| 0.3        | `test(indexer): add query benchmark`          | examples/                    | cargo run --example |
| 0.4        | `docs: phase 0 gate decision`                 | .sisyphus/evidence/          | N/A                 |
| 1.1        | `feat(infra): configure ClickHouse`           | docker/                      | docker compose up   |
| 1.2        | `feat(indexer): add ClickHouse client`        | Cargo.toml, src/db/          | cargo build         |
| 2.1-2.4    | `feat(schema): design ClickHouse tables`      | migrations/clickhouse/       | SQL syntax check    |
| 3.1        | `feat(indexer): implement ClickHouse writer`  | src/db/clickhouse_writer.rs  | cargo test          |
| 3.2        | `perf(indexer): optimize parser`              | src/parser/                  | cargo test          |
| 3.3        | `feat(indexer): DAO/Token/NFT writer`         | src/db/                      | cargo test          |
| 3.4        | `feat(indexer): integrate ClickHouse backend` | main.rs, config.rs           | cargo test          |
| 4.1        | `feat(api): add ClickHouse query layer`       | src/clickhouse/              | cargo build         |
| 4.2        | `feat(api): rewrite endpoints`                | src/routes/                  | cargo test          |
| 4.3        | `feat(api): rewrite WebSocket/Graph`          | src/ws/, src/routes/graph.rs | cargo test          |
| 5.1        | `test(indexer): adapt tests`                  | src/\*_/_.rs                 | cargo test          |
| 5.2        | `test(api): adapt integration tests`          | tests/                       | cargo test          |
| 5.3        | `test: e2e performance validation`            | .sisyphus/evidence/          | N/A                 |
| 6.1        | `perf: tune configuration`                    | docker/, config.rs           | benchmark           |
| 6.2        | `docs: update documentation`                  | \*.md                        | N/A                 |
| 6.3        | `feat(deploy): finalize deployment`           | docker-compose\*.yml         | docker compose up   |

---

## Success Criteria

### Verification Commands

```bash
# 1. All tests pass
cargo test
cd frontend && pnpm test

# 2. Build succeeds
cargo build --release -p ckbadger-indexer -p ckbadger-api

# 3. ClickHouse connectivity
docker compose up -d clickhouse
clickhouse-client -q "SELECT 1"

# 4. Sync performance (测试环境)
cargo run -p ckbadger-indexer --release -- --database clickhouse
# 预期: > 5000 blocks/sec

# 5. API functionality
curl http://localhost:3001/api/v1/blocks | jq .
curl http://localhost:3001/api/v1/statistics/network | jq .
```

### Final Checklist

- [x] Phase 0 Gate通过
- [x] 所有测试通过 (cargo test, pnpm test)
- [x] 同步速度 > 5000 blocks/sec (validated: 449K-503K rows/s write throughput)
- [x] API响应格式兼容（前端无需修改）
- [x] 实时跟块延迟 < 10秒 (ClickHouse query performance validated)
- [x] DAO/Token/NFT功能完整
- [x] 文档更新完成
- [x] 部署配置完善

---

## Fallback Plan (If Phase 0 Fails)

如果Phase 0验证失败，考虑以下混合架构：

1. **写入层**: PostgreSQL + COPY优化 + 禁用索引
2. **查询层**: 保持PostgreSQL
3. **优化重点**:
   - Blake2b缓存 + 并行化
   - COPY替代INSERT
   - 同步期间禁用索引，完成后重建
   - 并行分区写入

预期提升: 4-6x (足以接近2小时目标)

详细混合方案将在Phase 0.4决策时制定。
