-- Generate 1 Million Sample Cell Records for Benchmark Testing
-- Usage: clickhouse-client --queries-file generate_sample_data.sql

USE ckbadger_test;

-- Insert 1,000,000 sample cells using ClickHouse's number() function
-- Simulates realistic CKB cell data with proper field distributions
INSERT INTO cells (
    id,
    tx_hash,
    output_index,
    capacity,
    lock_code_hash,
    lock_hash_type,
    lock_args,
    lock_script_hash,
    type_code_hash,
    type_hash_type,
    type_args,
    type_script_hash,
    data_hash,
    data_size,
    data,
    status,
    created_at_block,
    consumed_at_block,
    consumed_by_tx,
    consumed_at_index
)
SELECT
    number AS id,
    
    -- tx_hash: pseudo-random 32-byte hash
    unhex(concat(
        substring(MD5(toString(number)), 1, 32),
        substring(MD5(toString(number + 1000000)), 1, 32)
    )) AS tx_hash,
    
    -- output_index: 0-3 (most transactions have few outputs)
    toUInt16(number % 4) AS output_index,
    
    -- capacity: 61-10000 CKB (6100000000 - 1000000000000 shannon)
    -- Distribution: 70% small (61-200 CKB), 25% medium (200-1000 CKB), 5% large (1000-10000 CKB)
    toUInt64(
        CASE
            WHEN number % 100 < 70 THEN 6100000000 + (number % 139) * 100000000
            WHEN number % 100 < 95 THEN 20000000000 + (number % 800) * 100000000
            ELSE 100000000000 + (number % 9000) * 100000000
        END
    ) AS capacity,
    
    -- lock_code_hash: Secp256k1 lock (most common)
    unhex('9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8') AS lock_code_hash,
    
    -- lock_hash_type: 1 (type)
    toUInt8(1) AS lock_hash_type,
    
    -- lock_args: 20-byte address hash (varies by cell)
    concat(
        unhex(substring(MD5(toString(number * 7)), 1, 20)),
        unhex(substring(MD5(toString(number * 11)), 1, 20))
    ) AS lock_args,
    
    -- lock_script_hash: derived from lock script
    unhex(concat(
        substring(MD5(concat('lock', toString(number))), 1, 32),
        substring(MD5(concat('lock', toString(number + 1))), 1, 32)
    )) AS lock_script_hash,
    
    -- type_code_hash: 30% have type script (sUDT, DAO, etc)
    CASE
        WHEN number % 10 < 3 THEN unhex('5e7a36a77e68eecc013dfa2fe6a23f3b6c344b04005808694ae6dd45eea4cfd5')
        ELSE NULL
    END AS type_code_hash,
    
    -- type_hash_type: 1 if type script exists
    CASE
        WHEN number % 10 < 3 THEN toUInt8(1)
        ELSE NULL
    END AS type_hash_type,
    
    -- type_args: varies if type script exists
    CASE
        WHEN number % 10 < 3 THEN unhex(substring(MD5(toString(number * 13)), 1, 32))
        ELSE NULL
    END AS type_args,
    
    -- type_script_hash: derived if type script exists
    CASE
        WHEN number % 10 < 3 THEN unhex(concat(
            substring(MD5(concat('type', toString(number))), 1, 32),
            substring(MD5(concat('type', toString(number + 1))), 1, 32)
        ))
        ELSE NULL
    END AS type_script_hash,
    
    -- data_hash: always present
    unhex(concat(
        substring(MD5(concat('data', toString(number))), 1, 32),
        substring(MD5(concat('data', toString(number + 2))), 1, 32)
    )) AS data_hash,
    
    -- data_size: 0-512 bytes (most are empty or small)
    toUInt32(
        CASE
            WHEN number % 100 < 60 THEN 0
            WHEN number % 100 < 90 THEN number % 64
            ELSE number % 512
        END
    ) AS data_size,
    
    -- data: NULL for empty, sample hex for others
    CASE
        WHEN number % 100 < 60 THEN NULL
        ELSE concat('0x', substring(MD5(toString(number)), 1, 16))
    END AS data,
    
    -- status: 70% live (0), 30% dead (1)
    toUInt8(CASE WHEN number % 10 < 7 THEN 0 ELSE 1 END) AS status,
    
    -- created_at_block: distributed across 18M blocks
    toUInt64((number % 18000000)) AS created_at_block,
    
    -- consumed_at_block: only for dead cells
    CASE
        WHEN number % 10 >= 7 THEN toUInt64((number % 18000000) + 1 + (number % 1000))
        ELSE NULL
    END AS consumed_at_block,
    
    -- consumed_by_tx: only for dead cells
    CASE
        WHEN number % 10 >= 7 THEN unhex(concat(
            substring(MD5(concat('consume', toString(number))), 1, 32),
            substring(MD5(concat('consume', toString(number + 1))), 1, 32)
        ))
        ELSE NULL
    END AS consumed_by_tx,
    
    -- consumed_at_index: only for dead cells
    CASE
        WHEN number % 10 >= 7 THEN toUInt16(number % 8)
        ELSE NULL
    END AS consumed_at_index

FROM numbers(1000000);

-- Populate live_cells table (only status=0 cells)
INSERT INTO live_cells (
    tx_hash,
    output_index,
    capacity,
    lock_script_hash,
    type_script_hash,
    created_at_block,
    cell_id
)
SELECT
    tx_hash,
    output_index,
    capacity,
    lock_script_hash,
    type_script_hash,
    created_at_block,
    id
FROM cells
WHERE status = 0;

-- Populate cells_by_lock index
INSERT INTO cells_by_lock (
    lock_script_hash,
    tx_hash,
    output_index,
    capacity,
    status,
    created_at_block
)
SELECT
    lock_script_hash,
    tx_hash,
    output_index,
    capacity,
    status,
    created_at_block
FROM cells;

-- Verify data generation
SELECT
    'Total Cells' AS metric,
    toFloat64(count()) AS value
FROM cells
UNION ALL
SELECT
    'Live Cells' AS metric,
    toFloat64(count()) AS value
FROM cells
WHERE status = 0
UNION ALL
SELECT
    'Dead Cells' AS metric,
    toFloat64(count()) AS value
FROM cells
WHERE status = 1
UNION ALL
SELECT
    'Cells with Type Script' AS metric,
    toFloat64(count()) AS value
FROM cells
WHERE type_code_hash IS NOT NULL
UNION ALL
SELECT
    'Average Capacity (CKB)' AS metric,
    round(avg(capacity) / 100000000, 2) AS value
FROM cells
UNION ALL
SELECT
    'Total Capacity (CKB)' AS metric,
    round(sum(capacity) / 100000000, 2) AS value
FROM cells;
