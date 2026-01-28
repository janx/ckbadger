use criterion::{black_box, criterion_group, criterion_main, Criterion};

use chrono::Utc;
use ckbadger_indexer::db::copy_cells::CopyCellsWriter;
use ckbadger_indexer::db::copy_format::BinaryCopyBuffer;
use ckbadger_indexer::db::copy_inputs::CopyInputsWriter;
use ckbadger_indexer::db::copy_live_cells::CopyLiveCellsWriter;
use ckbadger_indexer::db::copy_transactions::CopyTransactionsWriter;
use ckbadger_indexer::parser::cell::ParsedCell;
use ckbadger_indexer::parser::transaction::ParsedInput;

fn create_test_cell() -> ParsedCell {
    ParsedCell {
        capacity: 100_00000000,
        lock_code_hash: vec![0u8; 32],
        lock_hash_type: 0,
        lock_args: vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10],
        lock_script_hash: vec![0u8; 32],
        type_code_hash: Some(vec![1u8; 32]),
        type_hash_type: Some(1),
        type_args: Some(vec![2, 3, 4, 5]),
        type_script_hash: Some(vec![5u8; 32]),
        data_hash: vec![0u8; 32],
        data_size: 100,
        data: vec![0u8; 100],
    }
}

fn create_test_input() -> ParsedInput {
    ParsedInput {
        previous_tx_hash: vec![0u8; 32],
        previous_output_index: 0,
        since: 0,
    }
}

fn benchmark_copy_format_basic(c: &mut Criterion) {
    c.bench_function("copy_format_1000_i64", |b| {
        b.iter(|| {
            let mut buf = BinaryCopyBuffer::new(1);
            for i in 0..1000i64 {
                buf.start_row();
                buf.write_i64(black_box(i));
            }
            buf.finish()
        })
    });
}

fn benchmark_cells_writer(c: &mut Criterion) {
    let cell = create_test_cell();
    let tx_hash = vec![0u8; 32];

    c.bench_function("cells_writer_1000_cells", |b| {
        b.iter(|| {
            let mut writer = CopyCellsWriter::new();
            for i in 0..1000i16 {
                writer.add_cell(black_box(&tx_hash), i, &cell, 1000);
            }
            writer.finish()
        })
    });
}

fn benchmark_transactions_writer(c: &mut Criterion) {
    let tx_hash = vec![0u8; 32];
    let timestamp = Utc::now();

    c.bench_function("transactions_writer_1000_txs", |b| {
        b.iter(|| {
            let mut writer = CopyTransactionsWriter::new();
            for i in 0..1000i64 {
                writer.add_transaction(
                    black_box(&tx_hash),
                    i,
                    0,
                    0,
                    2,
                    3,
                    1,
                    2,
                    0,
                    100_00000000,
                    99_00000000,
                    1_00000000,
                    Some(500),
                    Some(1000000),
                    false,
                    timestamp,
                );
            }
            writer.finish()
        })
    });
}

fn benchmark_inputs_writer(c: &mut Criterion) {
    let input = create_test_input();
    let tx_hash = vec![0u8; 32];

    c.bench_function("inputs_writer_1000_inputs", |b| {
        b.iter(|| {
            let mut writer = CopyInputsWriter::new();
            for i in 0..1000i16 {
                writer.add_input(black_box(&tx_hash), 1000, i, &input);
            }
            writer.finish()
        })
    });
}

fn benchmark_live_cells_writer(c: &mut Criterion) {
    let cell = create_test_cell();
    let tx_hash = vec![0u8; 32];

    c.bench_function("live_cells_writer_1000_cells", |b| {
        b.iter(|| {
            let mut writer = CopyLiveCellsWriter::new();
            for i in 0..1000i16 {
                writer.add_live_cell(black_box(&tx_hash), i, &cell, 1000);
            }
            writer.finish()
        })
    });
}

fn benchmark_batch_sizes(c: &mut Criterion) {
    let cell = create_test_cell();
    let tx_hash = vec![0u8; 32];

    let mut group = c.benchmark_group("cells_batch_sizes");

    for size in [100, 500, 1000, 5000, 10000].iter() {
        group.bench_function(format!("{}_cells", size), |b| {
            b.iter(|| {
                let mut writer = CopyCellsWriter::new();
                for i in 0..*size as i16 {
                    writer.add_cell(black_box(&tx_hash), i, &cell, 1000);
                }
                writer.finish()
            })
        });
    }

    group.finish();
}

criterion_group!(
    benches,
    benchmark_copy_format_basic,
    benchmark_cells_writer,
    benchmark_transactions_writer,
    benchmark_inputs_writer,
    benchmark_live_cells_writer,
    benchmark_batch_sizes,
);
criterion_main!(benches);
