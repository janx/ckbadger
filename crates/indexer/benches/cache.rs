use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};

use ckbadger_indexer::cache::cell_cache::CellInfoCache;
use ckbadger_indexer::db::live_cell_storage::LiveCellInfo;

fn generate_mock_cell_info() -> LiveCellInfo {
    LiveCellInfo {
        capacity: 10_000_000_000,
        created_at_block: 1_000_000,
        lock_script_hash: vec![0u8; 32],
        lock_code_hash: vec![0u8; 32],
        lock_args: vec![0u8; 20],
        type_script_hash: None,
        type_code_hash: None,
        data_size: 0,
    }
}

fn bench_cache_insert(c: &mut Criterion) {
    let mut group = c.benchmark_group("cache_insert");

    for capacity in [10_000, 100_000, 1_000_000] {
        let cache = CellInfoCache::new(capacity);

        group.bench_function(BenchmarkId::new("capacity", capacity), |b| {
            let mut i = 0u64;
            b.iter(|| {
                let tx_hash = i.to_le_bytes().to_vec();
                let cell_info = generate_mock_cell_info();
                cache.insert(black_box(tx_hash), black_box(0), black_box(cell_info));
                i = i.wrapping_add(1);
            })
        });
    }

    group.finish();
}

fn bench_cache_get_hit(c: &mut Criterion) {
    let cache = CellInfoCache::new(100_000);

    for i in 0..50_000u64 {
        let tx_hash = i.to_le_bytes().to_vec();
        let cell_info = generate_mock_cell_info();
        cache.insert(tx_hash, 0, cell_info);
    }

    let mut group = c.benchmark_group("cache_get");
    group.throughput(Throughput::Elements(1));

    group.bench_function("hit", |b| {
        let mut i = 0u64;
        b.iter(|| {
            let tx_hash = (i % 50_000).to_le_bytes().to_vec();
            let _result = cache.get(black_box(&tx_hash), black_box(0));
            i = i.wrapping_add(1);
        })
    });

    group.finish();
}

fn bench_cache_get_miss(c: &mut Criterion) {
    let cache = CellInfoCache::new(100_000);

    for i in 0..50_000u64 {
        let tx_hash = i.to_le_bytes().to_vec();
        let cell_info = generate_mock_cell_info();
        cache.insert(tx_hash, 0, cell_info);
    }

    let mut group = c.benchmark_group("cache_get");
    group.throughput(Throughput::Elements(1));

    group.bench_function("miss", |b| {
        let mut i = 100_000u64;
        b.iter(|| {
            let tx_hash = i.to_le_bytes().to_vec();
            let _result = cache.get(black_box(&tx_hash), black_box(0));
            i = i.wrapping_add(1);
        })
    });

    group.finish();
}

fn bench_cache_get_batch(c: &mut Criterion) {
    let cache = CellInfoCache::new(1_000_000);

    for i in 0..500_000u64 {
        let tx_hash = i.to_le_bytes().to_vec();
        let cell_info = generate_mock_cell_info();
        cache.insert(tx_hash, 0, cell_info);
    }

    let mut group = c.benchmark_group("cache_get_batch");

    for batch_size in [10, 50, 100, 500] {
        let batch_data: Vec<Vec<u8>> = (0..batch_size)
            .map(|i| {
                if i < batch_size * 8 / 10 {
                    (i as u64 % 500_000).to_le_bytes().to_vec()
                } else {
                    (1_000_000u64 + i as u64).to_le_bytes().to_vec()
                }
            })
            .collect();

        group.throughput(Throughput::Elements(batch_size as u64));

        group.bench_function(BenchmarkId::new("size", batch_size), |b| {
            b.iter(|| {
                let batch_refs: Vec<(&[u8], i16)> =
                    batch_data.iter().map(|h| (h.as_slice(), 0i16)).collect();
                cache.get_batch(black_box(&batch_refs))
            })
        });
    }

    group.finish();
}

fn bench_cache_eviction(c: &mut Criterion) {
    let capacity = 10_000;
    let cache = CellInfoCache::new(capacity);

    for i in 0..capacity as u64 {
        let tx_hash = i.to_le_bytes().to_vec();
        let cell_info = generate_mock_cell_info();
        cache.insert(tx_hash, 0, cell_info);
    }

    let mut group = c.benchmark_group("cache_eviction");
    group.throughput(Throughput::Elements(1));

    group.bench_function("insert_with_eviction", |b| {
        let mut i = capacity as u64;
        b.iter(|| {
            let tx_hash = i.to_le_bytes().to_vec();
            let cell_info = generate_mock_cell_info();
            cache.insert(black_box(tx_hash), black_box(0), black_box(cell_info));
            i = i.wrapping_add(1);
        })
    });

    group.finish();
}

fn bench_cache_stats(c: &mut Criterion) {
    let cache = CellInfoCache::new(100_000);

    for i in 0..10_000u64 {
        let tx_hash = i.to_le_bytes().to_vec();
        let cell_info = generate_mock_cell_info();
        cache.insert(tx_hash.clone(), 0, cell_info);
        let _ = cache.get(&tx_hash, 0);
    }

    let mut group = c.benchmark_group("cache_stats");

    group.bench_function("get_stats", |b| b.iter(|| cache.stats()));

    group.bench_function("len", |b| b.iter(|| cache.len()));

    group.finish();
}

criterion_group!(
    benches,
    bench_cache_insert,
    bench_cache_get_hit,
    bench_cache_get_miss,
    bench_cache_get_batch,
    bench_cache_eviction,
    bench_cache_stats,
);
criterion_main!(benches);
