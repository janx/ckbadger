use anyhow::Result;
use clickhouse::{Client, Row};
use rand::Rng;
use serde::Serialize;
use std::time::{Duration, Instant};

const TOTAL_ROWS: usize = 1_000_000;
const BATCH_SIZES: &[usize] = &[1_000, 10_000, 50_000, 100_000];
const RUNS_PER_BATCH: usize = 3;

#[derive(Debug, Clone, Serialize, Row)]
struct CellRow {
    id: u64,
    tx_hash: Vec<u8>,
    output_index: u16,
    capacity: u64,
    lock_code_hash: Vec<u8>,
    lock_hash_type: u8,
    lock_args: String,
    lock_script_hash: Vec<u8>,
    type_code_hash: Option<Vec<u8>>,
    type_hash_type: Option<u8>,
    type_args: Option<String>,
    type_script_hash: Option<Vec<u8>>,
    data_hash: Vec<u8>,
    data_size: u32,
    data: Option<String>,
    status: u8,
    created_at_block: u64,
    consumed_at_block: Option<u64>,
    consumed_by_tx: Option<Vec<u8>>,
    consumed_at_index: Option<u16>,
}

fn generate_random_hash(rng: &mut impl Rng) -> Vec<u8> {
    let mut hash = [0u8; 32];
    rng.fill(&mut hash);
    hash.to_vec()
}

fn generate_random_lock_args(rng: &mut impl Rng) -> String {
    let mut bytes = [0u8; 20];
    rng.fill(&mut bytes);
    hex::encode(bytes)
}

fn generate_cell(rng: &mut impl Rng, id: u64) -> CellRow {
    let capacity = if rng.gen_bool(0.7) {
        rng.gen_range(61_00000000..200_00000000)
    } else if rng.gen_bool(0.8) {
        rng.gen_range(200_00000000..1000_00000000)
    } else {
        rng.gen_range(1000_00000000..10000_00000000)
    };

    let output_index = rng.gen_range(0..4);
    let created_at_block = rng.gen_range(0..18_000_000);
    let has_type_script = rng.gen_bool(0.3);
    let is_live = rng.gen_bool(0.7);

    let (status, consumed_at_block, consumed_by_tx, consumed_at_index) = if is_live {
        (0, None, None, None)
    } else {
        (
            1,
            Some(created_at_block + rng.gen_range(1..1000)),
            Some(generate_random_hash(rng)),
            Some(rng.gen_range(0..4)),
        )
    };

    let (type_code_hash, type_hash_type, type_args, type_script_hash) = if has_type_script {
        let type_hash = generate_random_hash(rng);
        (
            Some(type_hash.clone()),
            Some(1),
            Some(hex::encode(&type_hash)),
            Some(generate_random_hash(rng)),
        )
    } else {
        (None, None, None, None)
    };

    let data_size = if rng.gen_bool(0.8) {
        0
    } else {
        rng.gen_range(1..256)
    };

    let data = if data_size > 0 {
        let mut bytes = vec![0u8; data_size as usize];
        rng.fill(&mut bytes[..]);
        Some(hex::encode(bytes))
    } else {
        Some(String::new())
    };

    CellRow {
        id,
        tx_hash: generate_random_hash(rng),
        output_index,
        capacity,
        lock_code_hash: generate_random_hash(rng),
        lock_hash_type: 1,
        lock_args: generate_random_lock_args(rng),
        lock_script_hash: generate_random_hash(rng),
        type_code_hash,
        type_hash_type,
        type_args,
        type_script_hash,
        data_hash: generate_random_hash(rng),
        data_size,
        data,
        status,
        created_at_block,
        consumed_at_block,
        consumed_by_tx,
        consumed_at_index,
    }
}

async fn clear_table(client: &Client) -> Result<()> {
    client
        .query("TRUNCATE TABLE ckbadger_test.cells")
        .execute()
        .await?;
    Ok(())
}

async fn benchmark_batch_insert(
    client: &Client,
    batch_size: usize,
    total_rows: usize,
) -> Result<BenchmarkResult> {
    let mut rng = rand::thread_rng();
    let mut latencies = Vec::new();
    let mut total_rows_inserted = 0;

    let overall_start = Instant::now();

    let mut id_counter = 0u64;
    while total_rows_inserted < total_rows {
        let rows_to_insert = batch_size.min(total_rows - total_rows_inserted);
        let mut batch = Vec::with_capacity(rows_to_insert);

        for _ in 0..rows_to_insert {
            batch.push(generate_cell(&mut rng, id_counter));
            id_counter += 1;
        }

        let batch_start = Instant::now();
        let mut insert = client.insert("ckbadger_test.cells")?;
        for row in batch {
            insert.write(&row).await?;
        }
        insert.end().await?;
        let batch_duration = batch_start.elapsed();

        latencies.push(batch_duration);
        total_rows_inserted += rows_to_insert;
    }

    let total_duration = overall_start.elapsed();

    Ok(BenchmarkResult {
        batch_size,
        total_rows: total_rows_inserted,
        total_duration,
        latencies,
    })
}

#[allow(dead_code)]
#[derive(Debug)]
struct BenchmarkResult {
    batch_size: usize,
    total_rows: usize,
    total_duration: Duration,
    latencies: Vec<Duration>,
}

#[allow(dead_code)]
impl BenchmarkResult {
    fn throughput(&self) -> f64 {
        self.total_rows as f64 / self.total_duration.as_secs_f64()
    }

    fn latency_stats(&self) -> LatencyStats {
        let mut sorted = self.latencies.clone();
        sorted.sort();

        let min = sorted.first().copied().unwrap_or_default();
        let max = sorted.last().copied().unwrap_or_default();
        let mean = Duration::from_secs_f64(
            sorted.iter().map(|d| d.as_secs_f64()).sum::<f64>() / sorted.len() as f64,
        );

        let p50_idx = (sorted.len() as f64 * 0.50) as usize;
        let p95_idx = (sorted.len() as f64 * 0.95) as usize;
        let p99_idx = (sorted.len() as f64 * 0.99) as usize;

        let p50 = sorted.get(p50_idx).copied().unwrap_or_default();
        let p95 = sorted.get(p95_idx).copied().unwrap_or_default();
        let p99 = sorted.get(p99_idx).copied().unwrap_or_default();

        LatencyStats {
            min,
            max,
            mean,
            p50,
            p95,
            p99,
        }
    }
}

#[allow(dead_code)]
#[derive(Debug)]
struct LatencyStats {
    min: Duration,
    max: Duration,
    mean: Duration,
    p50: Duration,
    p95: Duration,
    p99: Duration,
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("=== ClickHouse Write Performance Benchmark ===\n");

    let client = Client::default()
        .with_url("http://localhost:8123")
        .with_database("ckbadger_test")
        .with_user("ckbadger")
        .with_password("changeme");

    println!("Testing connection...");
    let version: String = client
        .query("SELECT version()")
        .fetch_one::<String>()
        .await?;
    println!("Connected to ClickHouse version: {}\n", version);

    let mut all_results = Vec::new();

    for &batch_size in BATCH_SIZES {
        println!("--- Batch Size: {} rows ---", batch_size);

        let mut run_results = Vec::new();

        for run in 1..=RUNS_PER_BATCH {
            println!("  Run {}/{}...", run, RUNS_PER_BATCH);

            clear_table(&client).await?;

            let result = benchmark_batch_insert(&client, batch_size, TOTAL_ROWS).await?;

            println!("    Throughput: {:.0} rows/sec", result.throughput());
            println!(
                "    Total Duration: {:.2}s",
                result.total_duration.as_secs_f64()
            );

            run_results.push(result);
        }

        let avg_throughput =
            run_results.iter().map(|r| r.throughput()).sum::<f64>() / run_results.len() as f64;

        println!("  Average Throughput: {:.0} rows/sec\n", avg_throughput);

        all_results.push((batch_size, run_results));
    }

    println!("\n=== Summary ===\n");
    println!(
        "{:<15} {:<20} {:<15} {:<15} {:<15} {:<15} {:<15}",
        "Batch Size", "Throughput (rows/s)", "Min Latency", "Mean Latency", "P50", "P95", "P99"
    );
    println!("{}", "-".repeat(120));

    for (batch_size, results) in &all_results {
        let avg_throughput =
            results.iter().map(|r| r.throughput()).sum::<f64>() / results.len() as f64;

        let all_latencies: Vec<Duration> =
            results.iter().flat_map(|r| r.latencies.clone()).collect();

        let mut sorted = all_latencies.clone();
        sorted.sort();

        let min = sorted.first().copied().unwrap_or_default();
        let mean = Duration::from_secs_f64(
            sorted.iter().map(|d| d.as_secs_f64()).sum::<f64>() / sorted.len() as f64,
        );
        let p50_idx = (sorted.len() as f64 * 0.50) as usize;
        let p95_idx = (sorted.len() as f64 * 0.95) as usize;
        let p99_idx = (sorted.len() as f64 * 0.99) as usize;
        let p50 = sorted.get(p50_idx).copied().unwrap_or_default();
        let p95 = sorted.get(p95_idx).copied().unwrap_or_default();
        let p99 = sorted.get(p99_idx).copied().unwrap_or_default();

        println!(
            "{:<15} {:<20.0} {:<15.3} {:<15.3} {:<15.3} {:<15.3} {:<15.3}",
            batch_size,
            avg_throughput,
            min.as_secs_f64() * 1000.0,
            mean.as_secs_f64() * 1000.0,
            p50.as_secs_f64() * 1000.0,
            p95.as_secs_f64() * 1000.0,
            p99.as_secs_f64() * 1000.0,
        );
    }

    let best_throughput = all_results
        .iter()
        .map(|(_, results)| {
            results.iter().map(|r| r.throughput()).sum::<f64>() / results.len() as f64
        })
        .max_by(|a, b| a.partial_cmp(b).unwrap())
        .unwrap_or(0.0);

    println!("\n=== Gate Criterion Check ===");
    println!("Target: > 500,000 rows/second");
    println!("Best Achieved: {:.0} rows/second", best_throughput);

    if best_throughput > 500_000.0 {
        println!("✓ PASS - ClickHouse meets write performance requirements");
    } else {
        println!("✗ FAIL - ClickHouse does not meet write performance requirements");
    }

    Ok(())
}
