# ckbadger 数据库重构计划

> **Note (2026-02)**: This plan has been partially superseded by the "Zero-RPC Architecture" work on the `ckbdb-direct` branch. The `blocks` and `transactions` full tables described below still exist, but lightweight index tables (`blocks_index`, `transactions_index`) now handle most read queries. API detail endpoints read raw blockchain data directly from CKB's RocksDB instead of PostgreSQL, eliminating the need to store full block/transaction data for detail views. See `INDEXER_PIPELINE.md` and `PERFORMANCE_TUNING.md` for current architecture.

> 目标: 在单台PC上支持2-5x当前CKB主网数据量,实现所有页面 <200ms 响应

## 一、设计原则

1. **分区优先**: 大表从创建时就分区,细粒度分区(每500万区块≈1.5年)
2. **预计算余额**: 地址余额增量维护,不实时聚合
3. **游标分页**: 全面废弃OFFSET,使用Keyset分页
4. **类型优化**: capacity用NUMERIC,避免TEXT转换开销
5. **索引精简**: BRIN用于时序数据,部分索引用于热数据
6. **破坏性重建**: 完全重建数据库,不保留旧数据,不考虑回滚

---

## 二、新表结构设计

### 2.1 核心表 (分区)

#### `blocks` - 区块表

```sql
CREATE TABLE blocks (
    number BIGINT NOT NULL,
    hash BYTEA NOT NULL,
    parent_hash BYTEA NOT NULL,
    timestamp TIMESTAMPTZ NOT NULL,
    version INTEGER NOT NULL,
    compact_target BIGINT NOT NULL,
    transactions_count INTEGER NOT NULL DEFAULT 0,
    proposals_count INTEGER NOT NULL DEFAULT 0,
    uncles_count INTEGER NOT NULL DEFAULT 0,
    epoch_number BIGINT NOT NULL,
    epoch_index INTEGER NOT NULL,
    epoch_length INTEGER NOT NULL,
    dao BYTEA NOT NULL,  -- 32 bytes
    nonce BYTEA NOT NULL,
    extra_hash BYTEA NOT NULL,
    extension BYTEA,
    proposals_hash BYTEA NOT NULL,
    transactions_root BYTEA NOT NULL,
    uncles_hash BYTEA NOT NULL,
    miner_lock_hash BYTEA,
    miner_message BYTEA,
    total_difficulty NUMERIC(40,0) NOT NULL DEFAULT 0,
    reward NUMERIC(20,0),
    PRIMARY KEY (number)
) PARTITION BY RANGE (number);

-- 每500万区块一个分区 (约1.5年数据)
CREATE TABLE blocks_p00 PARTITION OF blocks FOR VALUES FROM (0) TO (5000000);
CREATE TABLE blocks_p01 PARTITION OF blocks FOR VALUES FROM (5000000) TO (10000000);
CREATE TABLE blocks_p02 PARTITION OF blocks FOR VALUES FROM (10000000) TO (15000000);
CREATE TABLE blocks_p03 PARTITION OF blocks FOR VALUES FROM (15000000) TO (20000000);
CREATE TABLE blocks_p04 PARTITION OF blocks FOR VALUES FROM (20000000) TO (25000000);
CREATE TABLE blocks_p05 PARTITION OF blocks FOR VALUES FROM (25000000) TO (30000000);
CREATE TABLE blocks_p06 PARTITION OF blocks FOR VALUES FROM (30000000) TO (35000000);
CREATE TABLE blocks_p07 PARTITION OF blocks FOR VALUES FROM (35000000) TO (40000000);
CREATE TABLE blocks_p08 PARTITION OF blocks FOR VALUES FROM (40000000) TO (45000000);
CREATE TABLE blocks_p09 PARTITION OF blocks FOR VALUES FROM (45000000) TO (50000000);
-- 预留到50M区块 (约15年)
```

#### `transactions` - 交易表

```sql
CREATE TABLE transactions (
    hash BYTEA NOT NULL,
    block_number BIGINT NOT NULL,
    tx_index INTEGER NOT NULL,
    version INTEGER NOT NULL,
    inputs_count SMALLINT NOT NULL DEFAULT 0,
    outputs_count SMALLINT NOT NULL DEFAULT 0,
    witnesses_count SMALLINT NOT NULL DEFAULT 0,
    cell_deps_count SMALLINT NOT NULL DEFAULT 0,
    header_deps_count SMALLINT NOT NULL DEFAULT 0,
    total_input_capacity NUMERIC(20,0) NOT NULL DEFAULT 0,
    total_output_capacity NUMERIC(20,0) NOT NULL DEFAULT 0,
    fee NUMERIC(20,0) NOT NULL DEFAULT 0,
    tx_size INTEGER,
    cycles BIGINT,
    is_cellbase BOOLEAN NOT NULL DEFAULT FALSE,
    timestamp TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (block_number, hash)
) PARTITION BY RANGE (block_number);

-- 每500万区块一个分区 (与blocks对齐)
CREATE TABLE transactions_p00 PARTITION OF transactions FOR VALUES FROM (0) TO (5000000);
CREATE TABLE transactions_p01 PARTITION OF transactions FOR VALUES FROM (5000000) TO (10000000);
CREATE TABLE transactions_p02 PARTITION OF transactions FOR VALUES FROM (10000000) TO (15000000);
CREATE TABLE transactions_p03 PARTITION OF transactions FOR VALUES FROM (15000000) TO (20000000);
CREATE TABLE transactions_p04 PARTITION OF transactions FOR VALUES FROM (20000000) TO (25000000);
CREATE TABLE transactions_p05 PARTITION OF transactions FOR VALUES FROM (25000000) TO (30000000);
CREATE TABLE transactions_p06 PARTITION OF transactions FOR VALUES FROM (30000000) TO (35000000);
CREATE TABLE transactions_p07 PARTITION OF transactions FOR VALUES FROM (35000000) TO (40000000);
CREATE TABLE transactions_p08 PARTITION OF transactions FOR VALUES FROM (40000000) TO (45000000);
CREATE TABLE transactions_p09 PARTITION OF transactions FOR VALUES FROM (45000000) TO (50000000);
```

#### `cells` - Cell/UTXO表 (最大表,重点优化)

```sql
CREATE TABLE cells (
    id BIGINT GENERATED ALWAYS AS IDENTITY,
    tx_hash BYTEA NOT NULL,
    output_index SMALLINT NOT NULL,
    capacity NUMERIC(20,0) NOT NULL,  -- shannon, 最大约 10^18

    -- Lock Script (必填)
    lock_code_hash BYTEA NOT NULL,
    lock_hash_type SMALLINT NOT NULL,
    lock_args BYTEA NOT NULL,
    lock_script_hash BYTEA NOT NULL,

    -- Type Script (可选)
    type_code_hash BYTEA,
    type_hash_type SMALLINT,
    type_args BYTEA,
    type_script_hash BYTEA,

    -- Data
    data_hash BYTEA NOT NULL,
    data_size INTEGER NOT NULL DEFAULT 0,

    -- Lifecycle
    status SMALLINT NOT NULL DEFAULT 0,  -- 0=live, 1=dead
    created_at_block BIGINT NOT NULL,
    consumed_at_block BIGINT,
    consumed_by_tx BYTEA,
    consumed_at_index SMALLINT,

    PRIMARY KEY (created_at_block, id),
    UNIQUE (created_at_block, tx_hash, output_index)
) PARTITION BY RANGE (created_at_block);

-- 每500万区块一个分区 (与blocks对齐)
CREATE TABLE cells_p00 PARTITION OF cells FOR VALUES FROM (0) TO (5000000);
CREATE TABLE cells_p01 PARTITION OF cells FOR VALUES FROM (5000000) TO (10000000);
CREATE TABLE cells_p02 PARTITION OF cells FOR VALUES FROM (10000000) TO (15000000);
CREATE TABLE cells_p03 PARTITION OF cells FOR VALUES FROM (15000000) TO (20000000);
CREATE TABLE cells_p04 PARTITION OF cells FOR VALUES FROM (20000000) TO (25000000);
CREATE TABLE cells_p05 PARTITION OF cells FOR VALUES FROM (25000000) TO (30000000);
CREATE TABLE cells_p06 PARTITION OF cells FOR VALUES FROM (30000000) TO (35000000);
CREATE TABLE cells_p07 PARTITION OF cells FOR VALUES FROM (35000000) TO (40000000);
CREATE TABLE cells_p08 PARTITION OF cells FOR VALUES FROM (40000000) TO (45000000);
CREATE TABLE cells_p09 PARTITION OF cells FOR VALUES FROM (45000000) TO (50000000);
```

#### `transaction_inputs` - 交易输入表

```sql
CREATE TABLE transaction_inputs (
    id BIGINT GENERATED ALWAYS AS IDENTITY,
    tx_hash BYTEA NOT NULL,
    tx_block_number BIGINT NOT NULL,  -- 冗余,用于分区对齐
    input_index SMALLINT NOT NULL,
    previous_tx_hash BYTEA NOT NULL,
    previous_output_index SMALLINT NOT NULL,
    since NUMERIC(20,0) NOT NULL DEFAULT 0,

    PRIMARY KEY (tx_block_number, id),
    UNIQUE (tx_block_number, tx_hash, input_index)
) PARTITION BY RANGE (tx_block_number);

-- 每500万区块一个分区 (与blocks对齐)
CREATE TABLE transaction_inputs_p00 PARTITION OF transaction_inputs FOR VALUES FROM (0) TO (5000000);
CREATE TABLE transaction_inputs_p01 PARTITION OF transaction_inputs FOR VALUES FROM (5000000) TO (10000000);
CREATE TABLE transaction_inputs_p02 PARTITION OF transaction_inputs FOR VALUES FROM (10000000) TO (15000000);
CREATE TABLE transaction_inputs_p03 PARTITION OF transaction_inputs FOR VALUES FROM (15000000) TO (20000000);
CREATE TABLE transaction_inputs_p04 PARTITION OF transaction_inputs FOR VALUES FROM (20000000) TO (25000000);
CREATE TABLE transaction_inputs_p05 PARTITION OF transaction_inputs FOR VALUES FROM (25000000) TO (30000000);
CREATE TABLE transaction_inputs_p06 PARTITION OF transaction_inputs FOR VALUES FROM (30000000) TO (35000000);
CREATE TABLE transaction_inputs_p07 PARTITION OF transaction_inputs FOR VALUES FROM (35000000) TO (40000000);
CREATE TABLE transaction_inputs_p08 PARTITION OF transaction_inputs FOR VALUES FROM (40000000) TO (45000000);
CREATE TABLE transaction_inputs_p09 PARTITION OF transaction_inputs FOR VALUES FROM (45000000) TO (50000000);
```

### 2.2 预计算聚合表 (新增)

#### `address_balances` - 地址余额表 ⭐ 关键新增

```sql
CREATE TABLE address_balances (
    lock_script_hash BYTEA PRIMARY KEY,

    -- 余额 (增量维护)
    balance NUMERIC(20,0) NOT NULL DEFAULT 0,

    -- Cell计数
    live_cells_count INTEGER NOT NULL DEFAULT 0,
    total_cells_count BIGINT NOT NULL DEFAULT 0,

    -- 交易计数 (增量维护)
    transactions_count BIGINT NOT NULL DEFAULT 0,

    -- 时间线
    first_seen_block BIGINT,
    first_seen_tx BYTEA,
    last_activity_block BIGINT,
    last_activity_tx BYTEA,

    updated_at TIMESTAMPTZ DEFAULT NOW()
);

-- 富豪榜排序
CREATE INDEX idx_address_balances_balance ON address_balances(balance DESC)
    WHERE balance > 0;
-- 活跃地址
CREATE INDEX idx_address_balances_activity ON address_balances(last_activity_block DESC);
```

#### `address_transactions` - 地址交易关联表 ⭐ 关键新增

```sql
-- 用于地址页面的交易历史,避免复杂UNION查询
CREATE TABLE address_transactions (
    lock_script_hash BYTEA NOT NULL,
    tx_hash BYTEA NOT NULL,
    block_number BIGINT NOT NULL,
    tx_type SMALLINT NOT NULL,  -- 1=received, 2=sent, 3=both
    capacity_change NUMERIC(20,0) NOT NULL,  -- 正=收入, 负=支出
    timestamp TIMESTAMPTZ NOT NULL,

    PRIMARY KEY (lock_script_hash, block_number, tx_hash)
) PARTITION BY HASH (lock_script_hash);

-- 16个Hash分区 (均匀分布)
CREATE TABLE address_transactions_p00 PARTITION OF address_transactions FOR VALUES WITH (MODULUS 16, REMAINDER 0);
CREATE TABLE address_transactions_p01 PARTITION OF address_transactions FOR VALUES WITH (MODULUS 16, REMAINDER 1);
CREATE TABLE address_transactions_p02 PARTITION OF address_transactions FOR VALUES WITH (MODULUS 16, REMAINDER 2);
CREATE TABLE address_transactions_p03 PARTITION OF address_transactions FOR VALUES WITH (MODULUS 16, REMAINDER 3);
CREATE TABLE address_transactions_p04 PARTITION OF address_transactions FOR VALUES WITH (MODULUS 16, REMAINDER 4);
CREATE TABLE address_transactions_p05 PARTITION OF address_transactions FOR VALUES WITH (MODULUS 16, REMAINDER 5);
CREATE TABLE address_transactions_p06 PARTITION OF address_transactions FOR VALUES WITH (MODULUS 16, REMAINDER 6);
CREATE TABLE address_transactions_p07 PARTITION OF address_transactions FOR VALUES WITH (MODULUS 16, REMAINDER 7);
CREATE TABLE address_transactions_p08 PARTITION OF address_transactions FOR VALUES WITH (MODULUS 16, REMAINDER 8);
CREATE TABLE address_transactions_p09 PARTITION OF address_transactions FOR VALUES WITH (MODULUS 16, REMAINDER 9);
CREATE TABLE address_transactions_p10 PARTITION OF address_transactions FOR VALUES WITH (MODULUS 16, REMAINDER 10);
CREATE TABLE address_transactions_p11 PARTITION OF address_transactions FOR VALUES WITH (MODULUS 16, REMAINDER 11);
CREATE TABLE address_transactions_p12 PARTITION OF address_transactions FOR VALUES WITH (MODULUS 16, REMAINDER 12);
CREATE TABLE address_transactions_p13 PARTITION OF address_transactions FOR VALUES WITH (MODULUS 16, REMAINDER 13);
CREATE TABLE address_transactions_p14 PARTITION OF address_transactions FOR VALUES WITH (MODULUS 16, REMAINDER 14);
CREATE TABLE address_transactions_p15 PARTITION OF address_transactions FOR VALUES WITH (MODULUS 16, REMAINDER 15);
```

### 2.3 统计表 (优化)

#### `sync_status` - 同步状态

```sql
CREATE TABLE sync_status (
    id INTEGER PRIMARY KEY DEFAULT 1,
    tip_block_number BIGINT NOT NULL DEFAULT 0,
    tip_block_hash BYTEA,
    total_transactions BIGINT NOT NULL DEFAULT 0,
    total_cells BIGINT NOT NULL DEFAULT 0,
    total_live_cells BIGINT NOT NULL DEFAULT 0,
    total_addresses BIGINT NOT NULL DEFAULT 0,
    last_synced_at TIMESTAMPTZ DEFAULT NOW(),

    CONSTRAINT single_row CHECK (id = 1)
);

INSERT INTO sync_status (id) VALUES (1);
```

#### `daily_statistics` - 每日统计 (优化)

```sql
CREATE TABLE daily_statistics (
    date DATE PRIMARY KEY,

    -- 每日增量
    blocks_count INTEGER NOT NULL DEFAULT 0,
    transactions_count INTEGER NOT NULL DEFAULT 0,
    cells_created INTEGER NOT NULL DEFAULT 0,
    cells_consumed INTEGER NOT NULL DEFAULT 0,
    capacity_transferred NUMERIC(30,0) NOT NULL DEFAULT 0,

    -- 每日快照 (区块结束时的值)
    total_blocks BIGINT NOT NULL DEFAULT 0,
    total_transactions BIGINT NOT NULL DEFAULT 0,
    total_live_cells BIGINT NOT NULL DEFAULT 0,
    total_data_size BIGINT NOT NULL DEFAULT 0,

    -- 计算指标
    avg_block_time_ms INTEGER,
    avg_tx_per_block NUMERIC(10,2),
    new_addresses INTEGER NOT NULL DEFAULT 0,
    active_addresses INTEGER NOT NULL DEFAULT 0,

    created_at TIMESTAMPTZ DEFAULT NOW()
);
```

### 2.4 DAO相关表 (基本保持)

```sql
-- dao_deposits 保持现有结构,添加索引优化
CREATE TABLE dao_deposits (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    tx_hash BYTEA NOT NULL,
    output_index SMALLINT NOT NULL,
    lock_script_hash BYTEA NOT NULL,
    capacity NUMERIC(20,0) NOT NULL,

    deposit_block_number BIGINT NOT NULL,
    deposit_tx_hash BYTEA NOT NULL,
    deposit_timestamp TIMESTAMPTZ NOT NULL,
    deposit_ar NUMERIC(20,0) NOT NULL,  -- 新增: 存款时AR

    status SMALLINT NOT NULL DEFAULT 0,  -- 0=active, 1=requesting, 2=withdrawn

    withdraw_request_block BIGINT,
    withdraw_request_tx BYTEA,
    withdraw_request_timestamp TIMESTAMPTZ,
    withdraw_request_ar NUMERIC(20,0),  -- 新增: 请求时AR

    withdraw_block BIGINT,
    withdraw_tx BYTEA,
    withdraw_timestamp TIMESTAMPTZ,

    compensation NUMERIC(20,0),  -- 计算后存储

    UNIQUE(tx_hash, output_index)
);

CREATE INDEX idx_dao_deposits_lock ON dao_deposits(lock_script_hash);
CREATE INDEX idx_dao_deposits_status ON dao_deposits(status) WHERE status < 2;
CREATE INDEX idx_dao_deposits_block ON dao_deposits(deposit_block_number DESC);
```

### 2.5 Token/Spore表 (基本保持)

```sql
-- tokens, token_balances, token_transfers 保持现有结构
-- spore_clusters, spore_cells, spore_content 保持现有结构
-- 添加必要索引优化
```

---

## 三、索引策略

### 3.1 BRIN索引 (时序数据)

```sql
-- blocks: 按number顺序插入
CREATE INDEX idx_blocks_number_brin ON blocks USING BRIN (number) WITH (pages_per_range = 128);
CREATE INDEX idx_blocks_timestamp_brin ON blocks USING BRIN (timestamp) WITH (pages_per_range = 128);

-- transactions: 按block_number顺序
CREATE INDEX idx_tx_block_brin ON transactions USING BRIN (block_number) WITH (pages_per_range = 128);

-- cells: 按created_at_block顺序
CREATE INDEX idx_cells_created_brin ON cells USING BRIN (created_at_block) WITH (pages_per_range = 128);
```

### 3.2 B-tree索引 (查找)

```sql
-- blocks
CREATE UNIQUE INDEX idx_blocks_hash ON blocks(hash);
CREATE INDEX idx_blocks_epoch ON blocks(epoch_number);
CREATE INDEX idx_blocks_miner ON blocks(miner_lock_hash) WHERE miner_lock_hash IS NOT NULL;

-- transactions
CREATE UNIQUE INDEX idx_tx_hash ON transactions(hash);
CREATE INDEX idx_tx_timestamp ON transactions(timestamp DESC);
-- 游标分页用
CREATE INDEX idx_tx_cursor ON transactions(block_number DESC, tx_index DESC);

-- cells
CREATE UNIQUE INDEX idx_cells_outpoint ON cells(tx_hash, output_index);
-- Live cells查询 (最重要)
CREATE INDEX idx_cells_lock_live ON cells(lock_script_hash, created_at_block DESC)
    WHERE status = 0;
CREATE INDEX idx_cells_type_live ON cells(type_script_hash, created_at_block DESC)
    WHERE status = 0 AND type_script_hash IS NOT NULL;
-- 消费查询
CREATE INDEX idx_cells_consumed_by ON cells(consumed_by_tx)
    WHERE consumed_by_tx IS NOT NULL;

-- transaction_inputs
CREATE INDEX idx_inputs_previous ON transaction_inputs(previous_tx_hash, previous_output_index);
CREATE INDEX idx_inputs_tx ON transaction_inputs(tx_hash);
```

### 3.3 覆盖索引 (避免回表)

```sql
-- 交易列表页常用字段
CREATE INDEX idx_tx_list_covering ON transactions(block_number DESC, tx_index DESC)
    INCLUDE (hash, inputs_count, outputs_count, fee, is_cellbase, timestamp);

-- Cell列表页常用字段
CREATE INDEX idx_cells_list_covering ON cells(lock_script_hash, created_at_block DESC)
    INCLUDE (tx_hash, output_index, capacity, type_script_hash, data_size)
    WHERE status = 0;
```

---

## 四、API层改动清单

### 4.1 分页改为游标模式

| 端点                     | 当前           | 改为                                   |
| ------------------------ | -------------- | -------------------------------------- |
| `/transactions`          | `page + limit` | `cursor_block + cursor_index + limit`  |
| `/cells/live`            | `page + limit` | `cursor_block + cursor_id + limit`     |
| `/dao/deposits`          | `page + limit` | `cursor_block + cursor_id + limit`     |
| `/tokens`                | `page + limit` | `cursor_id + limit`                    |
| `/tokens/{id}/holders`   | `page + limit` | `cursor_balance + cursor_hash + limit` |
| `/tokens/{id}/transfers` | `page + limit` | `cursor_block + cursor_id + limit`     |

### 4.2 地址端点重构

```rust
// 当前: 实时聚合 (慢)
async fn get_address(...) {
    let balance = sqlx::query("SELECT SUM(capacity) FROM cells WHERE lock_script_hash = $1 AND status = 0");
    let tx_count = sqlx::query("SELECT COUNT(DISTINCT tx_hash) FROM ... UNION ...");
}

// 改为: 查预计算表 (快)
async fn get_address(...) {
    let stats = sqlx::query_as::<_, AddressStats>(
        "SELECT balance, live_cells_count, transactions_count, first_seen_block, last_activity_block
         FROM address_balances WHERE lock_script_hash = $1"
    ).fetch_optional(&state.pool).await?;
}
```

### 4.3 Graph端点修复N+1

```rust
// 当前: 循环查询 (慢)
for (prev_tx_hash, prev_idx) in inputs {
    let cell = sqlx::query("SELECT capacity FROM cells WHERE tx_hash = $1 AND output_index = $2");
}

// 改为: 批量查询 (快)
let cells = sqlx::query_as::<_, CellInfo>(r#"
    SELECT c.tx_hash, c.output_index, c.capacity
    FROM cells c
    JOIN UNNEST($1::bytea[], $2::smallint[]) AS t(tx_hash, output_index)
      ON c.tx_hash = t.tx_hash AND c.output_index = t.output_index
"#)
.bind(&tx_hashes)
.bind(&output_indices)
.fetch_all(&state.pool).await?;
```

### 4.4 新增端点

```rust
// 地址交易历史 (使用预计算表)
GET /addresses/{addr}/transactions?cursor_block=&cursor_tx=&limit=20

// 富豪榜
GET /addresses/top?limit=100

// 活跃地址
GET /addresses/active?days=7&limit=100
```

---

## 五、Indexer层改动清单

### 5.1 新增: 地址余额维护

```rust
// crates/indexer/src/db/writer.rs

impl BatchWriter {
    /// 处理Cell创建 - 更新address_balances
    async fn on_cell_created(&self, cell: &ParsedCell) -> Result<()> {
        sqlx::query(r#"
            INSERT INTO address_balances (
                lock_script_hash, balance, live_cells_count, total_cells_count,
                transactions_count, first_seen_block, first_seen_tx,
                last_activity_block, last_activity_tx
            ) VALUES ($1, $2, 1, 1, 1, $3, $4, $3, $4)
            ON CONFLICT (lock_script_hash) DO UPDATE SET
                balance = address_balances.balance + EXCLUDED.balance,
                live_cells_count = address_balances.live_cells_count + 1,
                total_cells_count = address_balances.total_cells_count + 1,
                last_activity_block = EXCLUDED.last_activity_block,
                last_activity_tx = EXCLUDED.last_activity_tx,
                updated_at = NOW()
        "#)
        .bind(&cell.lock_script_hash)
        .bind(&cell.capacity)
        .bind(cell.block_number)
        .bind(&cell.tx_hash)
        .execute(&self.pool).await?;

        Ok(())
    }

    /// 处理Cell消费 - 更新address_balances
    async fn on_cell_consumed(&self, cell: &ConsumedCell) -> Result<()> {
        sqlx::query(r#"
            UPDATE address_balances SET
                balance = balance - $2,
                live_cells_count = live_cells_count - 1,
                last_activity_block = $3,
                last_activity_tx = $4,
                updated_at = NOW()
            WHERE lock_script_hash = $1
        "#)
        .bind(&cell.lock_script_hash)
        .bind(&cell.capacity)
        .bind(cell.consumed_at_block)
        .bind(&cell.consumed_by_tx)
        .execute(&self.pool).await?;

        Ok(())
    }
}
```

### 5.2 新增: 地址交易关联维护

```rust
impl BatchWriter {
    /// 处理交易 - 记录地址交易关联
    async fn record_address_transactions(&self, tx: &ParsedTx) -> Result<()> {
        // 收集所有涉及的地址及其capacity变化
        let mut address_changes: HashMap<Vec<u8>, i128> = HashMap::new();

        // 输入 (减少)
        for input in &tx.inputs {
            if let Some(cell) = &input.previous_cell {
                *address_changes.entry(cell.lock_script_hash.clone()).or_default() -= cell.capacity as i128;
            }
        }

        // 输出 (增加)
        for output in &tx.outputs {
            *address_changes.entry(output.lock_script_hash.clone()).or_default() += output.capacity as i128;
        }

        // 批量插入
        for (lock_hash, change) in address_changes {
            let tx_type = match change.cmp(&0) {
                Ordering::Greater => 1, // received
                Ordering::Less => 2,    // sent
                Ordering::Equal => 3,   // both (internal transfer)
            };

            sqlx::query(r#"
                INSERT INTO address_transactions
                    (lock_script_hash, tx_hash, block_number, tx_type, capacity_change, timestamp)
                VALUES ($1, $2, $3, $4, $5, $6)
                ON CONFLICT DO NOTHING
            "#)
            .bind(&lock_hash)
            .bind(&tx.hash)
            .bind(tx.block_number)
            .bind(tx_type)
            .bind(change)
            .bind(tx.timestamp)
            .execute(&self.pool).await?;
        }

        Ok(())
    }
}
```

### 5.3 优化: 批量写入

```rust
impl BatchWriter {
    /// 批量插入Cells - 使用UNNEST
    async fn insert_cells_batch(&self, cells: &[ParsedCell]) -> Result<()> {
        if cells.is_empty() { return Ok(()); }

        let tx_hashes: Vec<&[u8]> = cells.iter().map(|c| c.tx_hash.as_slice()).collect();
        let output_indices: Vec<i16> = cells.iter().map(|c| c.output_index as i16).collect();
        let capacities: Vec<i64> = cells.iter().map(|c| c.capacity as i64).collect();
        // ... 其他字段

        sqlx::query(r#"
            INSERT INTO cells (
                tx_hash, output_index, capacity,
                lock_code_hash, lock_hash_type, lock_args, lock_script_hash,
                type_code_hash, type_hash_type, type_args, type_script_hash,
                data_hash, data_size, status, created_at_block
            )
            SELECT * FROM UNNEST(
                $1::bytea[], $2::smallint[], $3::numeric[],
                $4::bytea[], $5::smallint[], $6::bytea[], $7::bytea[],
                $8::bytea[], $9::smallint[], $10::bytea[], $11::bytea[],
                $12::bytea[], $13::int[], $14::smallint[], $15::bigint[]
            )
        "#)
        .bind(&tx_hashes)
        .bind(&output_indices)
        .bind(&capacities)
        // ... 绑定其他参数
        .execute(&self.pool).await?;

        Ok(())
    }
}
```

### 5.4 优化: sync_status计数器维护

```rust
impl BatchWriter {
    /// 处理完一个区块后更新计数器
    async fn update_sync_status(&self, block: &ParsedBlock) -> Result<()> {
        sqlx::query(r#"
            UPDATE sync_status SET
                tip_block_number = $1,
                tip_block_hash = $2,
                total_transactions = total_transactions + $3,
                total_cells = total_cells + $4,
                total_live_cells = total_live_cells + $5 - $6,
                last_synced_at = NOW()
            WHERE id = 1
        "#)
        .bind(block.number)
        .bind(&block.hash)
        .bind(block.transactions.len() as i64)
        .bind(block.cells_created as i64)
        .bind(block.cells_created as i64)
        .bind(block.cells_consumed as i64)
        .execute(&self.pool).await?;

        Ok(())
    }
}
```

---

## 六、Migration脚本清单

完全重建,使用单一migration:

```
migrations/postgres/
  001_schema_v2.sql          # 所有表定义 + 分区 + 索引
```

### 001_schema_v2.sql 结构

```sql
-- ============================================
-- ckbadger Database Schema v2.0
-- Optimized for 2-5x CKB mainnet data volume
-- Partition size: 5M blocks (~1.5 years)
-- ============================================

-- 1. Sync Status
CREATE TABLE sync_status (...);

-- 2. Core Tables (Partitioned by 5M blocks)
CREATE TABLE blocks (...) PARTITION BY RANGE (number);
CREATE TABLE blocks_p00 PARTITION OF blocks FOR VALUES FROM (0) TO (5000000);
CREATE TABLE blocks_p01 PARTITION OF blocks FOR VALUES FROM (5000000) TO (10000000);
-- ... up to p09 (50M blocks, ~15 years)

CREATE TABLE transactions (...) PARTITION BY RANGE (block_number);
-- 10 partitions, aligned with blocks

CREATE TABLE cells (...) PARTITION BY RANGE (created_at_block);
-- 10 partitions, aligned with blocks

CREATE TABLE transaction_inputs (...) PARTITION BY RANGE (tx_block_number);
-- 10 partitions, aligned with blocks

-- 3. Pre-computed Aggregation Tables
CREATE TABLE address_balances (...);
CREATE TABLE address_transactions (...) PARTITION BY HASH (lock_script_hash);
-- partitions...

-- 4. Statistics Tables
CREATE TABLE daily_statistics (...);
CREATE TABLE epoch_statistics (...);
CREATE TABLE daily_block_stats (...);
CREATE TABLE block_time_distribution (...);
CREATE TABLE epoch_time_distribution (...);
CREATE TABLE miner_statistics (...);

-- 5. DAO Tables
CREATE TABLE dao_deposits (...);
CREATE TABLE dao_statistics (...);
CREATE TABLE dao_daily_snapshots (...);

-- 6. Token Tables
CREATE TABLE tokens (...);
CREATE TABLE token_balances (...);
CREATE TABLE token_transfers (...);

-- 7. Spore Tables
CREATE TABLE spore_clusters (...);
CREATE TABLE spore_cells (...);
CREATE TABLE spore_content (...);

-- 8. Cell Data (Separate for large blobs)
CREATE TABLE cell_data (...);

-- 9. Known Scripts
CREATE TABLE known_scripts (...);
INSERT INTO known_scripts ...;

-- 10. BRIN Indexes
CREATE INDEX idx_blocks_number_brin ...;
CREATE INDEX idx_tx_block_brin ...;
CREATE INDEX idx_cells_created_brin ...;

-- 11. B-tree Indexes
CREATE UNIQUE INDEX idx_blocks_hash ...;
CREATE UNIQUE INDEX idx_tx_hash ...;
CREATE UNIQUE INDEX idx_cells_outpoint ...;
CREATE INDEX idx_cells_lock_live ...;
-- ... all other indexes

-- 12. Initialize sync_status
INSERT INTO sync_status (id) VALUES (1);
INSERT INTO dao_statistics (id) VALUES (1);
```

---

## 七、实施步骤

### Phase 1: 准备 (1天)

- [ ] 编写 `001_schema_v2.sql` migration
- [ ] 更新 `crates/common` 中的类型定义 (capacity: u64 -> i64 for sqlx)
- [ ] 配置 PostgreSQL (shared_buffers, work_mem等)

### Phase 2: Indexer重构 (3-5天)

- [ ] 重构 `BatchWriter` 支持新表结构
- [ ] 实现 `address_balances` 增量维护
- [ ] 实现 `address_transactions` 记录
- [ ] 更新 `sync_status` 计数器逻辑
- [ ] 更新 `daily_statistics` 计算逻辑
- [ ] 测试: 处理100个区块验证数据完整性

### Phase 3: API重构 (2-3天)

- [ ] 改造所有分页端点为游标模式
- [ ] 重构 `/addresses/{addr}` 使用预计算表
- [ ] 修复 `graph.rs` N+1查询
- [ ] 添加新端点 (`/addresses/top`, `/addresses/{addr}/transactions`)
- [ ] 更新前端API client适配新分页

### Phase 4: 前端适配 (1-2天)

- [ ] 更新分页组件支持游标
- [ ] 适配新的地址交易历史端点
- [ ] 测试所有页面

### Phase 5: 全量同步 (数天-数周)

- [ ] 清空数据库,运行新migration
- [ ] 启动indexer开始同步
- [ ] 监控性能指标
- [ ] 调整PostgreSQL配置

### Phase 6: 验证与调优

- [ ] 验证所有API响应时间
- [ ] 验证数据准确性
- [ ] 根据实际查询模式调整索引
- [ ] 文档更新

---

## 八、预期效果

### 响应时间 (P99)

| 页面             | 当前估计 | 目标       |
| ---------------- | -------- | ---------- |
| 首页             | 200ms    | <50ms      |
| 区块列表         | 100ms    | <30ms      |
| 交易列表         | 150ms    | <50ms      |
| 交易详情         | 300ms    | <100ms     |
| **地址页面**     | 500ms-2s | **<80ms**  |
| **地址交易历史** | 1s+      | **<100ms** |
| 图表页面         | 1-3s     | <300ms     |
| Cell图谱         | 2s+      | <150ms     |

### 存储效率

| 表           | 当前索引开销 | 优化后                 |
| ------------ | ------------ | ---------------------- |
| cells        | ~40% of data | ~25% (BRIN + 部分索引) |
| transactions | ~30%         | ~20%                   |

### 扩展性

- 支持 5亿+ cells
- 支持 2亿+ transactions
- 单机PC (16GB RAM, SSD) 可运行

---

_Last updated: 2025-01-13 (partially superseded by Zero-RPC Architecture, 2026-02)_
