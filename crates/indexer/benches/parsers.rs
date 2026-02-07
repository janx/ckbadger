use criterion::{black_box, criterion_group, criterion_main, Criterion, Throughput};

use ckbadger_indexer::parser::BlockParser;
use ckbadger_indexer::rpc::{
    BlockView, CellInput, CellOutput, HeaderView, OutPoint, Script, TransactionView,
};

fn generate_mock_block(tx_count: usize) -> BlockView {
    BlockView {
        header: HeaderView {
            version: "0x0".to_string(),
            compact_target: "0x1a08a97e".to_string(),
            timestamp: "0x18d4e5a3b42".to_string(),
            number: "0xf4240".to_string(),
            epoch: "0x708070e00028b".to_string(),
            parent_hash: "0x".to_string() + &"a".repeat(64),
            transactions_root: "0x".to_string() + &"b".repeat(64),
            proposals_hash: "0x".to_string() + &"c".repeat(64),
            extra_hash: "0x".to_string() + &"d".repeat(64),
            dao: "0x".to_string() + &"e".repeat(64),
            nonce: "0x".to_string() + &"f".repeat(32),
            hash: "0x".to_string() + &"1".repeat(64),
        },
        uncles: vec![],
        transactions: (0..tx_count).map(generate_mock_transaction).collect(),
        proposals: vec!["0x".to_string() + &"2".repeat(20); 10],
    }
}

fn generate_mock_transaction(index: usize) -> TransactionView {
    let hash_suffix = format!("{:064x}", index);
    TransactionView {
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
        outputs: (0..3)
            .map(|_| CellOutput {
                capacity: "0x174876e800".to_string(),
                lock: Script {
                    code_hash: "0x".to_string() + &"4".repeat(64),
                    hash_type: "type".to_string(),
                    args: "0x".to_string() + &"5".repeat(40),
                },
                type_: None,
            })
            .collect(),
        outputs_data: vec!["0x".to_string(); 3],
        version: "0x0".to_string(),
        witnesses: vec!["0x".to_string() + &"6".repeat(130); 2],
        hash: format!("0x{}", hash_suffix),
    }
}

fn bench_block_parser(c: &mut Criterion) {
    let mut group = c.benchmark_group("block_parser");

    for tx_count in [1, 10, 50, 100, 500] {
        let block = generate_mock_block(tx_count);
        group.throughput(Throughput::Elements(1));

        group.bench_function(format!("parse_block_{}_txs", tx_count), |b| {
            b.iter(|| BlockParser::parse(black_box(&block)))
        });
    }

    group.finish();
}

fn bench_header_parser(c: &mut Criterion) {
    let block = generate_mock_block(1);
    let header = &block.header;

    let mut group = c.benchmark_group("header_parser");
    group.throughput(Throughput::Elements(1));

    group.bench_function("parse_header", |b| {
        b.iter(|| BlockParser::parse_header(black_box(header)))
    });

    group.finish();
}

fn bench_epoch_parsing(c: &mut Criterion) {
    let epoch_values = ["0x708070e00028b", "0x7080000000000", "0x70807ff00028b"];

    let mut group = c.benchmark_group("epoch_parsing");

    for (i, epoch) in epoch_values.iter().enumerate() {
        group.bench_function(format!("epoch_case_{}", i), |b| {
            b.iter(|| {
                let val = u64::from_str_radix(&epoch[2..], 16).unwrap();
                let _length = (val >> 40) & 0xFFFF;
                let _index = (val >> 24) & 0xFFFF;
                let _number = val & 0xFFFFFF;
            })
        });
    }

    group.finish();
}

fn bench_hex_parsing(c: &mut Criterion) {
    let hash_32 = "0x".to_owned() + &"ab".repeat(32);
    let data_256 = "0x".to_owned() + &"cd".repeat(256);

    let test_cases = [
        ("short", "0x1234"),
        ("hash_32", &hash_32),
        ("data_256", &data_256),
    ];

    let mut group = c.benchmark_group("hex_parsing");

    for (name, hex_str) in test_cases {
        group.bench_function(name, |b| {
            b.iter(|| {
                let s = hex_str.strip_prefix("0x").unwrap_or(hex_str);
                hex::decode(black_box(s)).ok()
            })
        });
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_block_parser,
    bench_header_parser,
    bench_epoch_parsing,
    bench_hex_parsing,
);
criterion_main!(benches);
