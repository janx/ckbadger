use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use std::time::Duration;

use ckbadger_indexer::db::writer::{
    ActivityRow, BatchData, BlockRow, CellInputRow, CellOutputRow, CellStateRow, TransactionRow,
    EMPTY_HASH, EMPTY_NONCE,
};
use ckbadger_indexer::parser::BlockParser;
use ckbadger_indexer::rpc::{
    BlockView, CellInput, CellOutput, HeaderView, OutPoint, Script, TransactionView,
};
use clickhouse::types::UInt256;

fn generate_mock_block(block_number: u64, tx_count: usize, outputs_per_tx: usize) -> BlockView {
    BlockView {
        header: HeaderView {
            version: "0x0".to_string(),
            compact_target: "0x1a08a97e".to_string(),
            timestamp: format!("0x{:x}", 1700000000000u64 + block_number * 10000),
            number: format!("0x{:x}", block_number),
            epoch: "0x708070e00028b".to_string(),
            parent_hash: format!("0x{}", "a".repeat(64)),
            transactions_root: format!("0x{}", "b".repeat(64)),
            proposals_hash: format!("0x{}", "c".repeat(64)),
            extra_hash: format!("0x{}", "d".repeat(64)),
            dao: format!("0x{}", "e".repeat(64)),
            nonce: format!("0x{}", "f".repeat(32)),
            hash: format!("0x{:064x}", block_number),
        },
        uncles: vec![],
        transactions: (0..tx_count)
            .map(|i| generate_mock_transaction(block_number, i, outputs_per_tx))
            .collect(),
        proposals: vec![format!("0x{}", "2".repeat(20)); 5],
    }
}

fn generate_mock_transaction(
    block_number: u64,
    tx_index: usize,
    output_count: usize,
) -> TransactionView {
    let hash = format!("0x{:016x}{:016x}{:032x}", block_number, tx_index, 0u128);
    TransactionView {
        hash,
        version: "0x0".to_string(),
        cell_deps: vec![],
        header_deps: vec![],
        inputs: (0..2)
            .map(|i| CellInput {
                since: "0x0".to_string(),
                previous_output: OutPoint {
                    tx_hash: format!("0x{}", "3".repeat(64)),
                    index: format!("0x{:x}", i),
                },
            })
            .collect(),
        outputs: (0..output_count)
            .map(|_| CellOutput {
                capacity: "0x174876e800".to_string(),
                lock: Script {
                    code_hash: format!("0x{}", "4".repeat(64)),
                    hash_type: "type".to_string(),
                    args: format!("0x{}", "5".repeat(40)),
                },
                type_: None,
            })
            .collect(),
        outputs_data: vec!["0x".to_string(); output_count],
        witnesses: vec![format!("0x{}", "6".repeat(130)); 2],
    }
}

fn generate_batch_data(
    block_count: usize,
    txs_per_block: usize,
    outputs_per_tx: usize,
) -> BatchData {
    let mut batch = BatchData::new();

    for block_num in 0..block_count {
        batch.blocks.push(BlockRow {
            number: block_num as u64,
            hash: [block_num as u8; 32],
            parent_hash: EMPTY_HASH,
            timestamp: 1700000000000 + (block_num as i64 * 10000),
            version: 0,
            compact_target: 0x1a08a97e,
            transactions_count: txs_per_block as u32,
            proposals_count: 5,
            uncles_count: 0,
            epoch_number: block_num as u64 / 1000,
            epoch_index: (block_num % 1000) as u32,
            epoch_length: 1000,
            dao: EMPTY_HASH,
            nonce: EMPTY_NONCE,
            extra_hash: EMPTY_HASH,
            extension: String::new(),
            proposals_hash: EMPTY_HASH,
            transactions_root: EMPTY_HASH,
            uncles_hash: EMPTY_HASH,
            miner_lock_hash: EMPTY_HASH,
            miner_message: String::new(),
            total_difficulty: UInt256::from_le_bytes([0u8; 32]),
            reward: 0,
        });

        for tx_idx in 0..txs_per_block {
            let tx_hash: [u8; 32] = {
                let mut h = [0u8; 32];
                h[0..8].copy_from_slice(&(block_num as u64).to_le_bytes());
                h[8..16].copy_from_slice(&(tx_idx as u64).to_le_bytes());
                h
            };

            batch.transactions.push(TransactionRow {
                hash: tx_hash,
                block_number: block_num as u64,
                block_hash: [block_num as u8; 32],
                tx_index: tx_idx as u32,
                version: 0,
                inputs_count: 2,
                outputs_count: outputs_per_tx as u16,
                witnesses_count: 2,
                cell_deps_count: 0,
                header_deps_count: 0,
                total_input_capacity: 200_000_000_000,
                total_output_capacity: 199_000_000_000,
                fee: 1_000_000_000,
                tx_size: 500,
                cycles: 1000000,
                is_cellbase: 0,
                timestamp: 1700000000000 + (block_num as i64 * 10000),
            });

            for input_idx in 0..2 {
                batch.cell_inputs.push(CellInputRow {
                    tx_hash,
                    tx_block_number: block_num as u64,
                    input_index: input_idx,
                    previous_tx_hash: EMPTY_HASH,
                    previous_output_index: 0,
                    since: 0,
                });
            }

            for output_idx in 0..outputs_per_tx {
                batch.cell_outputs.push(CellOutputRow {
                    tx_hash,
                    output_index: output_idx as u16,
                    block_number: block_num as u64,
                    block_hash: [block_num as u8; 32],
                    capacity: 100_000_000_000,
                    lock_code_hash: EMPTY_HASH,
                    lock_hash_type: 1,
                    lock_args: "0x".to_string() + &"5".repeat(40),
                    lock_script_hash: EMPTY_HASH,
                    type_code_hash: EMPTY_HASH,
                    type_hash_type: 0,
                    type_args: String::new(),
                    type_script_hash: EMPTY_HASH,
                    data_hash: EMPTY_HASH,
                    data_size: 0,
                    data: String::new(),
                });

                batch.cell_states.push(CellStateRow {
                    tx_hash,
                    output_index: output_idx as u16,
                    canon_version: 1,
                    is_present: 1,
                    is_live: 1,
                    consumed_by_tx: EMPTY_HASH,
                    consumed_at_block: 0,
                    consumed_at_index: 0,
                    capacity: 100_000_000_000,
                    lock_script_hash: EMPTY_HASH,
                    type_script_hash: EMPTY_HASH,
                    lock_code_hash: EMPTY_HASH,
                    type_code_hash: EMPTY_HASH,
                    data_size: 0,
                    created_at_block: block_num as u64,
                });
            }
        }
    }

    batch
}

fn bench_batch_data_generation(c: &mut Criterion) {
    let mut group = c.benchmark_group("batch_data_generation");
    group.measurement_time(Duration::from_secs(10));

    for (blocks, txs, outputs) in [(100, 5, 3), (500, 5, 3), (1000, 5, 3), (1000, 10, 5)] {
        let total_txs = blocks * txs;
        group.throughput(Throughput::Elements(blocks as u64));

        group.bench_function(
            BenchmarkId::new(format!("{}blk_{}tx_{}out", blocks, txs, outputs), total_txs),
            |b| {
                b.iter(|| {
                    generate_batch_data(black_box(blocks), black_box(txs), black_box(outputs))
                })
            },
        );
    }

    group.finish();
}

fn bench_batch_data_serialization(c: &mut Criterion) {
    let batch_small = generate_batch_data(100, 5, 3);
    let batch_medium = generate_batch_data(500, 5, 3);
    let batch_large = generate_batch_data(1000, 10, 5);

    let mut group = c.benchmark_group("batch_serialization");
    group.measurement_time(Duration::from_secs(10));

    group.bench_function("small_100blk", |b| {
        b.iter(|| serde_json::to_string(black_box(&batch_small.blocks)))
    });

    group.bench_function("medium_500blk", |b| {
        b.iter(|| serde_json::to_string(black_box(&batch_medium.blocks)))
    });

    group.bench_function("large_1000blk", |b| {
        b.iter(|| serde_json::to_string(black_box(&batch_large.blocks)))
    });

    group.bench_function("transactions_10k", |b| {
        b.iter(|| serde_json::to_string(black_box(&batch_large.transactions)))
    });

    group.bench_function("cell_outputs_50k", |b| {
        b.iter(|| serde_json::to_string(black_box(&batch_large.cell_outputs)))
    });

    group.finish();
}

fn bench_block_parsing_pipeline(c: &mut Criterion) {
    let mut group = c.benchmark_group("block_parsing_pipeline");
    group.measurement_time(Duration::from_secs(15));

    for (tx_count, outputs_per_tx) in [(5, 3), (20, 5), (100, 3)] {
        let blocks: Vec<_> = (0..100)
            .map(|i| generate_mock_block(i, tx_count, outputs_per_tx))
            .collect();

        group.throughput(Throughput::Elements(100));

        group.bench_function(
            BenchmarkId::new(
                "parse_100_blocks",
                format!("{}tx_{}out", tx_count, outputs_per_tx),
            ),
            |b| {
                b.iter(|| {
                    for block in &blocks {
                        let _ = BlockParser::parse(black_box(block));
                    }
                })
            },
        );
    }

    group.finish();
}

fn bench_batch_memory_footprint(c: &mut Criterion) {
    let mut group = c.benchmark_group("batch_memory");

    for (blocks, txs, outputs) in [(100, 5, 3), (1000, 5, 3), (10000, 5, 3)] {
        group.bench_function(
            BenchmarkId::new("total_rows", format!("{}blk", blocks)),
            |b| {
                b.iter(|| {
                    let batch = generate_batch_data(blocks, txs, outputs);
                    black_box(batch.total_rows())
                })
            },
        );
    }

    group.finish();
}

fn bench_row_construction(c: &mut Criterion) {
    let mut group = c.benchmark_group("row_construction");

    group.bench_function("block_row", |b| {
        b.iter(|| BlockRow {
            number: 1000000,
            hash: [1u8; 32],
            parent_hash: [2u8; 32],
            timestamp: 1700000000000,
            version: 0,
            compact_target: 0x1a08a97e,
            transactions_count: 100,
            proposals_count: 10,
            uncles_count: 0,
            epoch_number: 1000,
            epoch_index: 500,
            epoch_length: 1000,
            dao: [3u8; 32],
            nonce: [4u8; 16],
            extra_hash: [5u8; 32],
            extension: String::new(),
            proposals_hash: [6u8; 32],
            transactions_root: [7u8; 32],
            uncles_hash: [8u8; 32],
            miner_lock_hash: [9u8; 32],
            miner_message: String::new(),
            total_difficulty: UInt256::from_le_bytes([0u8; 32]),
            reward: 1000000000,
        })
    });

    group.bench_function("transaction_row", |b| b.iter(TransactionRow::default));

    group.bench_function("cell_output_row", |b| b.iter(CellOutputRow::default));

    group.bench_function("cell_state_row", |b| b.iter(CellStateRow::default));

    group.bench_function("activity_row", |b| b.iter(ActivityRow::default));

    group.finish();
}

criterion_group!(
    benches,
    bench_batch_data_generation,
    bench_batch_data_serialization,
    bench_block_parsing_pipeline,
    bench_batch_memory_footprint,
    bench_row_construction,
);
criterion_main!(benches);
