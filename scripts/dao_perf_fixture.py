from pathlib import Path
import sys

if len(sys.argv) != 3 or sys.argv[2] not in {"current", "baseline"}:
    raise SystemExit("usage: dao_perf_fixture.py STATISTICS_RS current|baseline")

path = Path(sys.argv[1])
current = sys.argv[2] == "current"
source = path.read_text()
assert source.endswith("}\n")
if "test_dao_perf_nonempty_fixture" in source:
    raise SystemExit("performance fixture already present")
fixture = """
    // Temporary synthetic performance fixture. The committed report records
    // this source and command; this benchmark-only test is removed afterward.
    #[test]
    fn test_dao_perf_nonempty_fixture() {
        use std::time::Instant;
        const CAPACITY: i64 = 200_00000000;
        const OCCUPIED: i64 = 102_00000000;
        let n: usize = std::env::var("DAO_PERF_ENTRIES").unwrap_or_else(|_| "512".into()).parse().unwrap();
        let runs: usize = std::env::var("DAO_PERF_ITERS").unwrap_or_else(|_| "12".into()).parse().unwrap();
        let mixed = std::env::var("DAO_PERF_MIXED").as_deref() == Ok("1");
        assert!(n > 0 && n < 100_000);
        assert!(runs > 0);
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_domain(dir.path()).unwrap());
        seed_test_genesis_baseline(&store);
        seed_dao_free_fixture(&store, &[]);
        let mut batch = StoreBatch::new(&store);
        let mut live = 0usize;
        let mut deposited = 0usize;
        let mut unclaimed = 0usize;
        let mut claimed = 0usize;
        for i in 0..n {
            let mut tx_hash = [0xa0u8; 32];
            tx_hash[..8].copy_from_slice(&(i as u64).to_le_bytes());
            let status = if mixed { (i % 4).min(2) as i16 } else { 0 };
            let mut entry = dao_free_test_entry(CAPACITY, OCCUPIED, status, (status >= 1).then_some(OCCUPIED));
            entry.deposit_ar = 1;
            entry.lock_script_hash[..8].copy_from_slice(&((i % 128) as u64).to_le_bytes());
            if status >= 1 { entry.withdraw_request_tx = Some(tx_hash.to_vec()); }
            if status == 2 {
                entry.withdraw_tx = Some(tx_hash.to_vec());
                entry.compensation = Some(CAPACITY - OCCUPIED);
            }
            if status < 2 { live += 1; unclaimed += 1; }
            if status == 0 { deposited += 1; }
            if status == 2 { claimed += 1; }
            batch.put_dao_deposit(&keys::encode_outpoint(&tx_hash, 0), &entry);
        }
        batch.commit().unwrap();
        let writer = BatchWriter::new(store.clone(), store.clone());
        __EXACT_SEED__
        let mut refresh_ms = Vec::new();
        for i in 0..runs + 3 {
            let start = Instant::now();
            writer.refresh_latest_dao_statistics().unwrap();
            if i >= 3 { refresh_ms.push(start.elapsed().as_secs_f64() * 1000.0); }
        }
        let latest = store.get_latest_dao_statistics().unwrap().unwrap();
        assert_eq!(latest.active_deposits as usize, live);
        assert_eq!(latest.total_deposited, i128::from(CAPACITY) * deposited as i128);
        assert_eq!(latest.dao_capacity, i128::from(CAPACITY) * live as i128);
        assert_eq!(latest.dao_free_capacity, i128::from(CAPACITY - OCCUPIED) * live as i128);
        assert_eq!(latest.unclaimed_compensation, i128::from(CAPACITY - OCCUPIED) * unclaimed as i128);
        assert_eq!(latest.total_compensation_paid, i128::from(CAPACITY - OCCUPIED) * claimed as i128);
        let mut snapshot_ms = Vec::new();
        for i in 0..runs + 3 {
            let mut input = placeholder_snapshot_input();
            let start = Instant::now();
            writer.apply_exact_completed_dao_snapshot(
                DaoSnapshotBoundary {
                    date: chrono::NaiveDate::from_ymd_opt(2023, 11, 15).unwrap(),
                    end_block: 100,
                },
                2,
                i128::from(DAO_FREE_FIXTURE_S),
                &HashMap::new(),
                &HashMap::new(),
                &mut input,
            ).unwrap();
            if i >= 3 { snapshot_ms.push(start.elapsed().as_secs_f64() * 1000.0); }
            assert_eq!(input.unclaimed_compensation, i128::from(CAPACITY - OCCUPIED) * unclaimed as i128);
            assert_eq!(input.total_compensation, i128::from(CAPACITY - OCCUPIED) * claimed as i128);
            assert_eq!(input.cum_dao_compensation, i128::from(CAPACITY - OCCUPIED) * n as i128);
        }
        let mut batch_ms = Vec::new();
        let date = chrono::NaiveDate::from_ymd_opt(2023, 11, 15).unwrap();
        for i in 0..runs + 3 {
            let mut input = placeholder_snapshot_input();
            let mut write_batch = StoreBatch::new(&store);
            let start = Instant::now();
            writer.apply_exact_completed_dao_snapshot(
                DaoSnapshotBoundary { date, end_block: 100 },
                2,
                i128::from(DAO_FREE_FIXTURE_S),
                &HashMap::new(),
                &HashMap::new(),
                &mut input,
            ).unwrap();
            writer.update_dao_daily_snapshot(date, &input, &mut write_batch).unwrap();
            write_batch.commit().unwrap();
            if i >= 3 { batch_ms.push(start.elapsed().as_secs_f64() * 1000.0); }
            let persisted = store.get_latest_dao_daily_snapshot().unwrap().unwrap();
            assert_eq!(persisted.compensation, i128::from(CAPACITY - OCCUPIED) * claimed as i128);
            assert_eq!(persisted.unclaimed_compensation, i128::from(CAPACITY - OCCUPIED) * unclaimed as i128);
            assert_eq!(persisted.cum_dao_compensation, i128::from(CAPACITY - OCCUPIED) * n as i128);
        }
        eprintln!("DAO_PERF entries={n} mixed={mixed} runs={runs} capacity={} free={} unclaimed={} claimed={} refresh_ms={refresh_ms:?} snapshot_ms={snapshot_ms:?} batch_ms={batch_ms:?}", latest.dao_capacity, latest.dao_free_capacity, latest.unclaimed_compensation, latest.total_compensation_paid);
    }
"""
fixture = fixture.replace("__EXACT_SEED__", "seed_exact_tip_daily_partition(&store);" if current else "")
anchor = "\n}\n\n#[cfg(test)]\nmod epoch_stats_tests {"
assert anchor in source
source = source.replace(anchor, fixture + anchor, 1)
path.write_text(source)
