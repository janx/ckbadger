use anyhow::Result;
use clickhouse::Client;
use rand::Rng;
use std::time::Instant;

const TOTAL_CELLS: usize = 1_000_000; // Use existing 1M sample data
const LIVE_CELL_RATIO: f64 = 0.7; // 70% live, 30% consumed
const BATCH_SIZE: usize = 50_000;

// Test query patterns
const SINGLE_OUTPOINT_QUERIES: usize = 100;
const BATCH_OUTPOINT_SIZE: usize = 50;
const BATCH_OUTPOINT_QUERIES: usize = 20;
const ADDRESS_BALANCE_QUERIES: usize = 50;
const JOIN_QUERIES: usize = 20;

#[derive(Debug)]
struct QueryResult {
    query_type: String,
    description: String,
    min_ms: f64,
    max_ms: f64,
    mean_ms: f64,
    p50_ms: f64,
    p95_ms: f64,
    p99_ms: f64,
    rows_returned: u64,
    used_final: bool,
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

async fn populate_live_cells(client: &Client) -> Result<Vec<(Vec<u8>, u16)>> {
    println!(
        "Populating live_cells_rmt table with {} cells...",
        TOTAL_CELLS
    );

    let mut rng = rand::thread_rng();
    let mut outpoints = Vec::new();
    let mut version = 0u64;

    // Clear existing data
    client
        .query("TRUNCATE TABLE ckbadger_test.live_cells_rmt")
        .execute()
        .await?;
    client
        .query("TRUNCATE TABLE ckbadger_test.live_cells_by_lock")
        .execute()
        .await?;
    client
        .query("TRUNCATE TABLE ckbadger_test.live_cells_by_type")
        .execute()
        .await?;

    let mut inserted = 0;
    while inserted < TOTAL_CELLS {
        let batch_size = BATCH_SIZE.min(TOTAL_CELLS - inserted);
        let mut values = Vec::new();
        let mut lock_values = Vec::new();
        let mut type_values = Vec::new();

        for _ in 0..batch_size {
            let tx_hash = generate_random_hash(&mut rng);
            let output_index: u16 = rng.gen_range(0..4);
            let capacity: u64 = rng.gen_range(61_00000000..1000_00000000);
            let lock_script_hash = generate_random_hash(&mut rng);
            let lock_code_hash = generate_random_hash(&mut rng);
            let lock_args = generate_random_lock_args(&mut rng);
            let created_at_block: u64 = rng.gen_range(0..18_000_000);
            let data_size: u32 = if rng.gen_bool(0.8) {
                0
            } else {
                rng.gen_range(1..256)
            };

            let has_type_script = rng.gen_bool(0.3);
            let (type_script_hash, type_code_hash) = if has_type_script {
                (
                    Some(generate_random_hash(&mut rng)),
                    Some(generate_random_hash(&mut rng)),
                )
            } else {
                (None, None)
            };

            let is_live = rng.gen_bool(LIVE_CELL_RATIO);
            let sign: i8 = if is_live { 1 } else { -1 };

            if is_live {
                outpoints.push((tx_hash.clone(), output_index));
            }

            version += 1;

            // Main table
            let tx_hash_hex = hex::encode(&tx_hash);
            let lock_script_hash_hex = hex::encode(&lock_script_hash);
            let lock_code_hash_hex = hex::encode(&lock_code_hash);
            let type_script_hash_hex = type_script_hash.as_ref().map(hex::encode);
            let type_code_hash_hex = type_code_hash.as_ref().map(hex::encode);

            values.push(format!(
                "(unhex('{}'), {}, {}, unhex('{}'), unhex('{}'), '{}', {}, {}, {}, {}, {}, {})",
                tx_hash_hex,
                output_index,
                capacity,
                lock_script_hash_hex,
                lock_code_hash_hex,
                lock_args,
                if type_script_hash_hex.is_some() {
                    format!("unhex('{}')", type_script_hash_hex.as_ref().unwrap())
                } else {
                    "NULL".to_string()
                },
                if type_code_hash_hex.is_some() {
                    format!("unhex('{}')", type_code_hash_hex.as_ref().unwrap())
                } else {
                    "NULL".to_string()
                },
                data_size,
                created_at_block,
                sign,
                version
            ));

            // Lock index
            lock_values.push(format!(
                "(unhex('{}'), unhex('{}'), {}, {}, {}, {}, {}, {})",
                lock_script_hash_hex,
                tx_hash_hex,
                output_index,
                capacity,
                if type_script_hash_hex.is_some() {
                    format!("unhex('{}')", type_script_hash_hex.as_ref().unwrap())
                } else {
                    "NULL".to_string()
                },
                created_at_block,
                sign,
                version
            ));

            // Type index (only if has type script)
            if let Some(ref type_hash_hex) = type_script_hash_hex {
                type_values.push(format!(
                    "(unhex('{}'), unhex('{}'), {}, {}, unhex('{}'), {}, {}, {})",
                    type_hash_hex,
                    tx_hash_hex,
                    output_index,
                    capacity,
                    lock_script_hash_hex,
                    created_at_block,
                    sign,
                    version
                ));
            }
        }

        // Insert batch
        let query = format!(
            "INSERT INTO ckbadger_test.live_cells_rmt VALUES {}",
            values.join(",")
        );
        client.query(&query).execute().await?;

        let lock_query = format!(
            "INSERT INTO ckbadger_test.live_cells_by_lock VALUES {}",
            lock_values.join(",")
        );
        client.query(&lock_query).execute().await?;

        if !type_values.is_empty() {
            let type_query = format!(
                "INSERT INTO ckbadger_test.live_cells_by_type VALUES {}",
                type_values.join(",")
            );
            client.query(&type_query).execute().await?;
        }

        inserted += batch_size;
        if inserted % 100_000 == 0 {
            println!("  Inserted {} cells...", inserted);
        }
    }

    println!(
        "✓ Populated {} cells ({} live)",
        TOTAL_CELLS,
        outpoints.len()
    );
    Ok(outpoints)
}

async fn benchmark_single_outpoint_query(
    client: &Client,
    outpoints: &[(Vec<u8>, u16)],
    use_final: bool,
) -> Result<QueryResult> {
    let mut rng = rand::thread_rng();
    let mut latencies = Vec::new();
    let mut total_rows = 0u64;

    let final_clause = if use_final { "FINAL" } else { "" };

    for _ in 0..SINGLE_OUTPOINT_QUERIES {
        let (tx_hash, output_index) = &outpoints[rng.gen_range(0..outpoints.len())];
        let tx_hash_hex = hex::encode(tx_hash);

        let query = format!(
            "SELECT count() FROM ckbadger_test.live_cells_rmt {} WHERE tx_hash = unhex('{}') AND output_index = {} AND sign = 1",
            final_clause, tx_hash_hex, output_index
        );

        let start = Instant::now();
        let count: u64 = client.query(&query).fetch_one().await?;
        let duration = start.elapsed();

        latencies.push(duration.as_secs_f64() * 1000.0);
        total_rows += count;
    }

    Ok(calculate_stats(
        "single_outpoint",
        &format!(
            "Single OutPoint lookup (tx_hash + output_index){}",
            if use_final { " with FINAL" } else { "" }
        ),
        latencies,
        total_rows,
        use_final,
    ))
}

async fn benchmark_batch_outpoint_query(
    client: &Client,
    outpoints: &[(Vec<u8>, u16)],
    use_final: bool,
) -> Result<QueryResult> {
    let mut rng = rand::thread_rng();
    let mut latencies = Vec::new();
    let mut total_rows = 0u64;

    let final_clause = if use_final { "FINAL" } else { "" };

    for _ in 0..BATCH_OUTPOINT_QUERIES {
        let batch: Vec<_> = (0..BATCH_OUTPOINT_SIZE)
            .map(|_| {
                let (tx_hash, output_index) = &outpoints[rng.gen_range(0..outpoints.len())];
                (hex::encode(tx_hash), *output_index)
            })
            .collect();

        let conditions: Vec<String> = batch
            .iter()
            .map(|(tx_hash_hex, output_index)| {
                format!(
                    "(tx_hash = unhex('{}') AND output_index = {})",
                    tx_hash_hex, output_index
                )
            })
            .collect();

        let query = format!(
            "SELECT count() FROM ckbadger_test.live_cells_rmt {} WHERE ({}) AND sign = 1",
            final_clause,
            conditions.join(" OR ")
        );

        let start = Instant::now();
        let count: u64 = client.query(&query).fetch_one().await?;
        let duration = start.elapsed();

        latencies.push(duration.as_secs_f64() * 1000.0);
        total_rows += count;
    }

    Ok(calculate_stats(
        "batch_outpoint",
        &format!(
            "Batch OutPoint lookup ({} cells){}",
            BATCH_OUTPOINT_SIZE,
            if use_final { " with FINAL" } else { "" }
        ),
        latencies,
        total_rows,
        use_final,
    ))
}

async fn benchmark_address_balance_query(client: &Client, use_final: bool) -> Result<QueryResult> {
    let mut rng = rand::thread_rng();
    let mut latencies = Vec::new();
    let mut total_rows = 0u64;

    let final_clause = if use_final { "FINAL" } else { "" };

    // Get some random lock_script_hashes
    #[derive(Debug, clickhouse::Row, serde::Deserialize)]
    struct LockHash {
        lock_script_hash: String,
    }

    let lock_hashes: Vec<String> = client
        .query("SELECT hex(lock_script_hash) as lock_script_hash FROM ckbadger_test.live_cells_by_lock GROUP BY lock_script_hash LIMIT 1000")
        .fetch_all::<LockHash>()
        .await?
        .into_iter()
        .map(|h| h.lock_script_hash)
        .collect();

    if lock_hashes.is_empty() {
        return Ok(QueryResult {
            query_type: "address_balance".to_string(),
            description: "Address balance query (SKIPPED - no data)".to_string(),
            min_ms: 0.0,
            max_ms: 0.0,
            mean_ms: 0.0,
            p50_ms: 0.0,
            p95_ms: 0.0,
            p99_ms: 0.0,
            rows_returned: 0,
            used_final: use_final,
        });
    }

    for _ in 0..ADDRESS_BALANCE_QUERIES {
        let lock_hash = &lock_hashes[rng.gen_range(0..lock_hashes.len())];

        let query = format!(
            "SELECT sum(capacity) as total_capacity, count() as cell_count FROM ckbadger_test.live_cells_by_lock {} WHERE lock_script_hash = unhex('{}') AND sign = 1",
            final_clause, lock_hash
        );

        let start = Instant::now();
        let result: Vec<(u64, u64)> = client.query(&query).fetch_all().await?;
        let duration = start.elapsed();

        latencies.push(duration.as_secs_f64() * 1000.0);
        total_rows += result.len() as u64;
    }

    Ok(calculate_stats(
        "address_balance",
        &format!(
            "Address balance query (sum + count){}",
            if use_final { " with FINAL" } else { "" }
        ),
        latencies,
        total_rows,
        use_final,
    ))
}

async fn benchmark_join_query(
    client: &Client,
    outpoints: &[(Vec<u8>, u16)],
) -> Result<QueryResult> {
    println!("Populating transaction_inputs for JOIN test...");

    // Create some transaction inputs that reference our live cells
    let mut rng = rand::thread_rng();
    let mut values = Vec::new();

    for _i in 0..10_000 {
        let tx_hash = generate_random_hash(&mut rng);
        let input_index: u16 = rng.gen_range(0..4);
        let (prev_tx_hash, prev_output_index) = &outpoints[rng.gen_range(0..outpoints.len())];
        let block_number: u64 = rng.gen_range(0..18_000_000);

        values.push(format!(
            "(unhex('{}'), {}, unhex('{}'), {}, {})",
            hex::encode(&tx_hash),
            input_index,
            hex::encode(prev_tx_hash),
            prev_output_index,
            block_number
        ));

        if values.len() >= 1000 {
            let query = format!(
                "INSERT INTO ckbadger_test.transaction_inputs VALUES {}",
                values.join(",")
            );
            client.query(&query).execute().await?;
            values.clear();
        }
    }

    if !values.is_empty() {
        let query = format!(
            "INSERT INTO ckbadger_test.transaction_inputs VALUES {}",
            values.join(",")
        );
        client.query(&query).execute().await?;
    }

    println!("✓ Populated transaction_inputs");

    // Now benchmark JOIN queries
    let mut latencies = Vec::new();
    let mut total_rows = 0u64;

    // Get some random transaction hashes
    #[derive(Debug, clickhouse::Row, serde::Deserialize)]
    struct TxHash {
        tx_hash: String,
    }

    let tx_hashes: Vec<String> = client
        .query("SELECT hex(tx_hash) as tx_hash FROM ckbadger_test.transaction_inputs GROUP BY tx_hash LIMIT 100")
        .fetch_all::<TxHash>()
        .await?
        .into_iter()
        .map(|h| h.tx_hash)
        .collect();

    for _ in 0..JOIN_QUERIES {
        let tx_hash_hex = &tx_hashes[rng.gen_range(0..tx_hashes.len())];

        // Simulate: SELECT cells.* FROM transaction_inputs JOIN live_cells ON ...
        let query = format!(
            "SELECT count() FROM ckbadger_test.transaction_inputs ti 
             JOIN (SELECT * FROM ckbadger_test.live_cells_rmt FINAL WHERE sign = 1) lc 
             ON ti.previous_tx_hash = lc.tx_hash AND ti.previous_output_index = lc.output_index
             WHERE ti.tx_hash = unhex('{}')",
            tx_hash_hex
        );

        let start = Instant::now();
        let count: u64 = client
            .query(&query.replace("lc.*", "count()"))
            .fetch_one()
            .await?;
        let duration = start.elapsed();

        latencies.push(duration.as_secs_f64() * 1000.0);
        total_rows += count;
    }

    Ok(calculate_stats(
        "join_query",
        "JOIN transaction_inputs → live_cells (with FINAL)",
        latencies,
        total_rows,
        true,
    ))
}

fn calculate_stats(
    query_type: &str,
    description: &str,
    mut latencies: Vec<f64>,
    rows_returned: u64,
    used_final: bool,
) -> QueryResult {
    latencies.sort_by(|a, b| a.partial_cmp(b).unwrap());

    let min_ms = latencies.first().copied().unwrap_or(0.0);
    let max_ms = latencies.last().copied().unwrap_or(0.0);
    let mean_ms = latencies.iter().sum::<f64>() / latencies.len() as f64;

    let p50_idx = (latencies.len() as f64 * 0.50) as usize;
    let p95_idx = (latencies.len() as f64 * 0.95) as usize;
    let p99_idx = (latencies.len() as f64 * 0.99) as usize;

    let p50_ms = latencies.get(p50_idx).copied().unwrap_or(0.0);
    let p95_ms = latencies.get(p95_idx).copied().unwrap_or(0.0);
    let p99_ms = latencies.get(p99_idx).copied().unwrap_or(0.0);

    QueryResult {
        query_type: query_type.to_string(),
        description: description.to_string(),
        min_ms,
        max_ms,
        mean_ms,
        p50_ms,
        p95_ms,
        p99_ms,
        rows_returned,
        used_final,
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("=== ClickHouse Live Cell Query Performance Benchmark ===\n");

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

    // Populate test data
    let outpoints = populate_live_cells(&client).await?;

    println!("\n=== Running Query Benchmarks ===\n");

    let mut results = Vec::new();

    // Test 1: Single OutPoint lookup (without FINAL)
    println!("1. Single OutPoint lookup (without FINAL)...");
    results.push(benchmark_single_outpoint_query(&client, &outpoints, false).await?);

    // Test 2: Single OutPoint lookup (with FINAL)
    println!("2. Single OutPoint lookup (with FINAL)...");
    results.push(benchmark_single_outpoint_query(&client, &outpoints, true).await?);

    // Test 3: Batch OutPoint lookup (without FINAL)
    println!("3. Batch OutPoint lookup (without FINAL)...");
    results.push(benchmark_batch_outpoint_query(&client, &outpoints, false).await?);

    // Test 4: Batch OutPoint lookup (with FINAL)
    println!("4. Batch OutPoint lookup (with FINAL)...");
    results.push(benchmark_batch_outpoint_query(&client, &outpoints, true).await?);

    // Test 5: Address balance query (without FINAL)
    println!("5. Address balance query (without FINAL)...");
    results.push(benchmark_address_balance_query(&client, false).await?);

    // Test 6: Address balance query (with FINAL)
    println!("6. Address balance query (with FINAL)...");
    results.push(benchmark_address_balance_query(&client, true).await?);

    // Test 7: JOIN query
    println!("7. JOIN transaction_inputs → live_cells...");
    results.push(benchmark_join_query(&client, &outpoints).await?);

    // Print results
    println!("\n=== Query Performance Results ===\n");
    println!(
        "{:<40} {:<10} {:<10} {:<10} {:<10} {:<10} {:<10}",
        "Query Type", "Min (ms)", "Mean (ms)", "P50 (ms)", "P95 (ms)", "P99 (ms)", "Max (ms)"
    );
    println!("{}", "-".repeat(100));

    for result in &results {
        println!(
            "{:<40} {:<10.2} {:<10.2} {:<10.2} {:<10.2} {:<10.2} {:<10.2}",
            result.description,
            result.min_ms,
            result.mean_ms,
            result.p50_ms,
            result.p95_ms,
            result.p99_ms,
            result.max_ms
        );
    }

    // Gate criterion check
    println!("\n=== Phase 0 Gate Criterion Check ===\n");

    let single_outpoint_result = results
        .iter()
        .find(|r| r.query_type == "single_outpoint" && r.used_final)
        .unwrap();

    let batch_outpoint_result = results
        .iter()
        .find(|r| r.query_type == "batch_outpoint" && r.used_final)
        .unwrap();

    let join_result = results
        .iter()
        .find(|r| r.query_type == "join_query")
        .unwrap();

    println!("Criterion 1: Single OutPoint query < 10ms");
    println!(
        "  Result: P50 = {:.2}ms, P95 = {:.2}ms, P99 = {:.2}ms",
        single_outpoint_result.p50_ms, single_outpoint_result.p95_ms, single_outpoint_result.p99_ms
    );
    let criterion1_pass = single_outpoint_result.p95_ms < 10.0;
    println!(
        "  Status: {}",
        if criterion1_pass {
            "✓ PASS"
        } else {
            "✗ FAIL"
        }
    );

    println!("\nCriterion 2: Batch OutPoint query (50 cells) < 500ms");
    println!(
        "  Result: P50 = {:.2}ms, P95 = {:.2}ms, P99 = {:.2}ms",
        batch_outpoint_result.p50_ms, batch_outpoint_result.p95_ms, batch_outpoint_result.p99_ms
    );
    let criterion2_pass = batch_outpoint_result.p95_ms < 500.0;
    println!(
        "  Status: {}",
        if criterion2_pass {
            "✓ PASS"
        } else {
            "✗ FAIL"
        }
    );

    println!("\nCriterion 3: JOIN query < 200ms");
    println!(
        "  Result: P50 = {:.2}ms, P95 = {:.2}ms, P99 = {:.2}ms",
        join_result.p50_ms, join_result.p95_ms, join_result.p99_ms
    );
    let criterion3_pass = join_result.p95_ms < 200.0;
    println!(
        "  Status: {}",
        if criterion3_pass {
            "✓ PASS"
        } else {
            "✗ FAIL"
        }
    );

    println!("\n=== Overall Gate Decision ===");
    if criterion1_pass && criterion2_pass && criterion3_pass {
        println!("✓ PASS - ClickHouse meets all query performance requirements");
    } else {
        println!("✗ FAIL - ClickHouse does not meet query performance requirements");
        if !criterion1_pass {
            println!("  - Single OutPoint query too slow");
        }
        if !criterion2_pass {
            println!("  - Batch OutPoint query too slow");
        }
        if !criterion3_pass {
            println!("  - JOIN query too slow");
        }
    }

    // Save results to database
    for result in &results {
        let query = format!(
            "INSERT INTO ckbadger_test.query_benchmark_results (test_name, query_type, query_description, rows_returned, duration_ms, used_final) VALUES ('phase0_query_benchmark', '{}', '{}', {}, {}, {})",
            result.query_type,
            result.description,
            result.rows_returned,
            result.mean_ms,
            if result.used_final { 1 } else { 0 }
        );
        client.query(&query).execute().await?;
    }

    println!("\n✓ Benchmark results saved to query_benchmark_results table");

    Ok(())
}
