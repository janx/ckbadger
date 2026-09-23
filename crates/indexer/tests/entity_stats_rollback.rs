//! Task 1.2 — per-block entity stats rollback and replay-equivalence matrix.
//!
//! These tests drive the real `BatchWriter` daily writers and the real
//! `EntityStatsOverlay` hourly path, then roll back with the production
//! two-phase path (`rollback_via_undo_log` + `rollback_to_block_with_append_only_store`,
//! the exact order `BatchWriter::execute_reorg` uses) and compare the resulting
//! eight entity stats families byte-for-byte against a second, independently
//! built store that synced the surviving branch directly.
//!
//! Until Phase 2 lands, this file does not compile: `EntityDailyChanges`,
//! `EntityStatsOverlay` and the five per-block daily writer signatures do not
//! exist yet. That compile failure IS the red light for Task 1.2 — the current
//! writers take a flat `HashMap<(entity, date), (i128, i128)>` that has already
//! thrown the block identity away, so no rollback to the middle of a batch can
//! be expressed against them.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use ckbadger_indexer::db::{apply_hourly_increment, BatchWriter, EntityStatsOverlay};
use ckbadger_indexer::sync::types::{EntityDailyChanges, EntityDateKey, ScriptDailyKey};
use ckbadger_store::batch::StoreBatch;
use ckbadger_store::types::EpochStats;
use ckbadger_store::{keys, CachedBlockHeader, CkbadgerStore, LiveCellInfo};
use rocksdb::{Direction, IteratorMode};

// ---------------------------------------------------------------------------
// Fixture constants
// ---------------------------------------------------------------------------

/// 2026-09-22 09:00:00 UTC+8 — every block in a scenario lands on this day
/// unless it carries an explicit timestamp.
const TS_DAY: i64 = 1_790_038_800_000;
/// 2026-09-21 23:30:00 UTC+8 — the previous UTC+8 calendar day.
const TS_PREV_DAY: i64 = 1_790_004_600_000;
const DAY: u32 = 20_260_922;
const PREV_DAY: u32 = 20_260_921;

const CODE_A: [u8; 32] = [0x11; 32];
const CODE_B: [u8; 32] = [0x12; 32];
const TOKEN_X: [u8; 32] = [0x22; 32];
const TOKEN_Y: [u8; 32] = [0x23; 32];
const CLUSTER_ID: [u8; 32] = [0x33; 32];
const SPORE_ID: [u8; 32] = [0x44; 32];
const COLLECTION_ID: [u8; 32] = [0x55; 32];

fn hour_of(ts: i64) -> i64 {
    ts / 3_600_000
}

// ---------------------------------------------------------------------------
// Store fixtures (mirrors tests/reorg_handling.rs; no shared `common/` module
// exists for indexer integration tests)
// ---------------------------------------------------------------------------

fn setup_split_stores() -> (Arc<CkbadgerStore>, Arc<CkbadgerStore>) {
    let domain_dir = tempfile::tempdir().unwrap();
    let append_dir = tempfile::tempdir().unwrap();
    let domain = Arc::new(CkbadgerStore::open_domain(domain_dir.path()).unwrap());
    let append = Arc::new(CkbadgerStore::open_append_only(append_dir.path()).unwrap());
    std::mem::forget(domain_dir);
    std::mem::forget(append_dir);
    (domain, append)
}

fn make_header(block_num: i64, timestamp: i64) -> CachedBlockHeader {
    let mut hash = vec![0u8; 32];
    hash[0..8].copy_from_slice(&block_num.to_le_bytes());
    CachedBlockHeader {
        hash,
        parent_hash: vec![0u8; 32],
        timestamp,
        epoch_number: 11,
        epoch_index: (block_num % 1800) as i32,
        epoch_length: 1800,
        dao: vec![0u8; 32],
        transactions_count: 0,
        uncles_count: 0,
        proposals_count: 0,
        compact_target: 0,
        miner_lock_hash: None,
        cycles: None,
    }
}

/// Epoch rows consistent with `make_header`: rollback fails fast when the
/// boundary epoch row is missing, so fixtures must uphold the same invariant
/// the real write path does.
fn seed_epoch_rows<I: IntoIterator<Item = (i64, i64)>>(store: &CkbadgerStore, blocks: I) {
    let mut lo = i64::MAX;
    let mut hi = i64::MIN;
    let mut lo_ts = 0i64;
    for (block, ts) in blocks {
        if block < lo {
            lo = block;
            lo_ts = ts;
        }
        hi = hi.max(block);
    }
    if lo == i64::MAX {
        return;
    }
    let existing = store.get_epoch_stats(11).unwrap();
    let (start_block, start_ts) = match &existing {
        Some(row) => (row.start_block.min(lo), row.start_timestamp),
        None => (lo, chrono::DateTime::from_timestamp_millis(lo_ts).unwrap()),
    };
    let end_block = existing
        .as_ref()
        .and_then(|row| row.end_block)
        .unwrap_or(hi)
        .max(hi);
    store
        .put_epoch_stats(
            11,
            &EpochStats {
                epoch_number: 11,
                start_block,
                end_block: Some(end_block),
                blocks_count: (end_block - start_block + 1) as i32,
                length: 1800,
                start_timestamp: start_ts,
                end_timestamp: None,
                transactions_count: 0,
            },
        )
        .unwrap();
}

/// One append-only cell payload written before any fork, so every scenario can
/// assert `CF_CELLS` bytes are untouched by rollback.
fn seed_append_only_witness(append: &CkbadgerStore) -> (Vec<u8>, Vec<u8>) {
    let key = keys::encode_outpoint(&[0xAB; 32], 0).to_vec();
    let mut batch = StoreBatch::new(append);
    batch.put_cell_payload_by_outpoint(
        &[0xAB; 32],
        0,
        &LiveCellInfo {
            capacity: 10_000_000_000,
            lock_script_hash: vec![0xC1; 32],
            lock_code_hash: CODE_A.to_vec(),
            lock_hash_type: 1,
            lock_args: vec![0xC3; 20],
            type_script_hash: None,
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            data_size: 0,
            occupied_capacity: 6_100_000_000,
            udt_amount: None,
            data_hash: None,
        },
    );
    batch.commit().unwrap();
    let bytes = append.get_cf(append.cf_cells(), &key).unwrap().unwrap();
    (key, bytes)
}

// ---------------------------------------------------------------------------
// Per-block change description
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct BlockChanges {
    block: i64,
    timestamp: i64,
    script: Vec<(ScriptDailyKey, i128, i128)>,
    token: Vec<(EntityDateKey, i128, i128)>,
    cluster: Vec<(EntityDateKey, i128, i128)>,
    spore: Vec<(EntityDateKey, i128, i128)>,
    object: Vec<(EntityDateKey, i128, i128)>,
    hourly: Vec<(Vec<u8>, i64)>,
}

impl BlockChanges {
    fn new(block: i64) -> Self {
        Self {
            block,
            timestamp: TS_DAY + block * 1_000,
            script: Vec::new(),
            token: Vec::new(),
            cluster: Vec::new(),
            spore: Vec::new(),
            object: Vec::new(),
            hourly: Vec::new(),
        }
    }

    fn at(mut self, timestamp: i64) -> Self {
        self.timestamp = timestamp;
        self
    }

    fn token(mut self, id: [u8; 32], date: u32, cap: i128, know: i128) -> Self {
        self.token.push(((id.to_vec(), date), cap, know));
        self
    }

    fn script(mut self, code: [u8; 32], is_type: bool, date: u32, cap: i128, know: i128) -> Self {
        self.script
            .push(((code.to_vec(), 1u8, is_type, date), cap, know));
        self
    }

    fn cluster(mut self, id: [u8; 32], date: u32, cap: i128, know: i128) -> Self {
        self.cluster.push(((id.to_vec(), date), cap, know));
        self
    }

    fn spore(mut self, id: [u8; 32], date: u32, cap: i128, know: i128) -> Self {
        self.spore.push(((id.to_vec(), date), cap, know));
        self
    }

    fn object(mut self, id: [u8; 32], date: u32, cap: i128, know: i128) -> Self {
        self.object.push(((id.to_vec(), date), cap, know));
        self
    }

    fn token_hour(mut self, id: [u8; 32], ts: i64, by: i64) -> Self {
        self.hourly
            .push((keys::encode_token_hourly_key(&id, hour_of(ts)), by));
        self
    }

    fn spore_hour(mut self, id: [u8; 32], ts: i64, by: i64) -> Self {
        self.hourly
            .push((keys::encode_spore_hourly_key(&id, hour_of(ts)), by));
        self
    }

    fn object_hour(mut self, id: [u8; 32], ts: i64, by: i64) -> Self {
        self.hourly
            .push((keys::encode_object_hourly_key(&id, hour_of(ts)), by));
        self
    }
}

fn blk(n: i64) -> BlockChanges {
    BlockChanges::new(n)
}

/// Apply one committed batch of blocks exactly the way the writer will: a
/// single `StoreBatch`, a single `EntityStatsOverlay`, a single shared
/// `undo_seq_by_block` map, per-block undo pre-images and one final write per
/// touched key.
fn apply_commit(writer: &BatchWriter, domain: &CkbadgerStore, blocks: &[BlockChanges]) {
    let mut batch = StoreBatch::new(domain);
    let mut overlay = EntityStatsOverlay::new();
    let mut undo_seq: HashMap<i64, u64> = HashMap::new();

    let mut script = EntityDailyChanges::<ScriptDailyKey>::new();
    let mut token = EntityDailyChanges::<EntityDateKey>::new();
    let mut cluster = EntityDailyChanges::<EntityDateKey>::new();
    let mut spore = EntityDailyChanges::<EntityDateKey>::new();
    let mut object = EntityDailyChanges::<EntityDateKey>::new();

    for b in blocks {
        for (k, cap, know) in &b.script {
            script.add(b.block, k.clone(), *cap, *know).unwrap();
        }
        for (k, cap, know) in &b.token {
            token.add(b.block, k.clone(), *cap, *know).unwrap();
        }
        for (k, cap, know) in &b.cluster {
            cluster.add(b.block, k.clone(), *cap, *know).unwrap();
        }
        for (k, cap, know) in &b.spore {
            spore.add(b.block, k.clone(), *cap, *know).unwrap();
        }
        for (k, cap, know) in &b.object {
            object.add(b.block, k.clone(), *cap, *know).unwrap();
        }
    }

    writer
        .update_script_daily_deltas_batch(&script, &mut overlay, &mut undo_seq, &mut batch)
        .unwrap();
    writer
        .update_token_daily_deltas_batch(&token, &mut overlay, &mut undo_seq, &mut batch)
        .unwrap();
    writer
        .update_cluster_daily_deltas_batch(&cluster, &mut overlay, &mut undo_seq, &mut batch)
        .unwrap();
    writer
        .update_spore_daily_deltas_batch(&spore, &mut overlay, &mut undo_seq, &mut batch)
        .unwrap();
    writer
        .update_object_daily_deltas_batch(&object, &mut overlay, &mut undo_seq, &mut batch)
        .unwrap();

    // The four hourly write points route through the same overlay; driving them
    // directly here keeps this matrix independent of each protocol's cell
    // fixtures while exercising the identical undo/overlay contract.
    for b in blocks {
        for (key, by) in &b.hourly {
            let prev = overlay.current(domain, key).unwrap();
            let next = apply_hourly_increment(prev.as_deref(), *by, &|| {
                format!("test hourly key=0x{}", hex::encode(key))
            })
            .unwrap();
            overlay
                .mutate(domain, &mut batch, &mut undo_seq, b.block, key, next)
                .unwrap();
        }
    }

    overlay.stage_final(&mut batch).unwrap();
    for b in blocks {
        batch.put_block_header(b.block, &make_header(b.block, b.timestamp));
    }
    batch.commit().unwrap();
    seed_epoch_rows(domain, blocks.iter().map(|b| (b.block, b.timestamp)));
}

/// Byte-exact dump of the eight entity stats families.
fn dump_entity_stats(domain: &CkbadgerStore) -> BTreeMap<Vec<u8>, Vec<u8>> {
    let families: [(&rocksdb::ColumnFamily, u8); 8] = [
        (domain.cf_stats_script(), keys::STATS_PREFIX_SCRIPT_DAILY),
        (domain.cf_stats_token(), keys::STATS_PREFIX_TOKEN_DAILY),
        (domain.cf_stats_spore(), keys::STATS_PREFIX_CLUSTER_DAILY),
        (domain.cf_stats_spore(), keys::STATS_PREFIX_SPORE_DAILY),
        (domain.cf_stats_mnft(), keys::STATS_PREFIX_OBJECT_DAILY),
        (domain.cf_stats_token(), keys::STATS_PREFIX_TOKEN_HOURLY),
        (domain.cf_stats_spore(), keys::STATS_PREFIX_SPORE_HOURLY),
        (domain.cf_stats_mnft(), keys::STATS_PREFIX_OBJECT_HOURLY),
    ];
    let mut out = BTreeMap::new();
    for (cf, prefix) in families {
        let iter = domain.iterator_cf(cf, IteratorMode::From(&[prefix], Direction::Forward));
        for item in iter {
            let (key, value) = item.unwrap();
            if key.first() != Some(&prefix) {
                break;
            }
            out.insert(key.to_vec(), value.to_vec());
        }
    }
    out
}

fn read_stats(domain: &CkbadgerStore, key: &[u8]) -> Option<Vec<u8>> {
    domain.get_stats_key(key).unwrap()
}

/// Production two-phase rollback, in `execute_reorg`'s order.
fn rollback(domain: &CkbadgerStore, append: &CkbadgerStore, fork_point: i64) {
    let undo = domain.rollback_via_undo_log(append, fork_point).unwrap();
    domain
        .rollback_to_block_with_tx_contexts(fork_point, Some(append), undo.tx_contexts)
        .unwrap();
}

/// The core driver: build the original branch, roll back to `fork_point`,
/// replay the new branch — and build the same surviving history directly in a
/// second pair of stores. Returns `(after_rollback_replay, direct)`.
fn run_scenario(
    original: &[Vec<BlockChanges>],
    fork_point: i64,
    new_branch: &[Vec<BlockChanges>],
) -> (BTreeMap<Vec<u8>, Vec<u8>>, BTreeMap<Vec<u8>, Vec<u8>>) {
    // Branch A: full original history, then rollback, then replay.
    let (domain_a, append_a) = setup_split_stores();
    let writer_a = BatchWriter::new(domain_a.clone(), append_a.clone());
    let (cell_key, cell_bytes) = seed_append_only_witness(&append_a);
    for commit in original {
        apply_commit(&writer_a, &domain_a, commit);
    }
    rollback(&domain_a, &append_a, fork_point);
    for commit in new_branch {
        apply_commit(&writer_a, &domain_a, commit);
    }
    assert_eq!(
        append_a.get_cf(append_a.cf_cells(), &cell_key).unwrap(),
        Some(cell_bytes),
        "append-only CF_CELLS payload bytes must be unchanged by rollback"
    );

    // Branch B: only the surviving prefix of the original history, then the new
    // branch — i.e. what a node that never saw the orphans would have written.
    let (domain_b, append_b) = setup_split_stores();
    let writer_b = BatchWriter::new(domain_b.clone(), append_b.clone());
    seed_append_only_witness(&append_b);
    for commit in original {
        let surviving: Vec<BlockChanges> = commit
            .iter()
            .filter(|b| b.block <= fork_point)
            .cloned()
            .collect();
        if !surviving.is_empty() {
            apply_commit(&writer_b, &domain_b, &surviving);
        }
    }
    for commit in new_branch {
        apply_commit(&writer_b, &domain_b, commit);
    }

    (dump_entity_stats(&domain_a), dump_entity_stats(&domain_b))
}

// ---------------------------------------------------------------------------
// Matrix
// ---------------------------------------------------------------------------

#[tokio::test]
async fn token_daily_untouched_by_orphan_survives() {
    let (domain, append) = setup_split_stores();
    let writer = BatchWriter::new(domain.clone(), append.clone());
    let (cell_key, cell_bytes) = seed_append_only_witness(&append);

    for n in 1..=3i64 {
        apply_commit(
            &writer,
            &domain,
            &[blk(n).token(TOKEN_X, DAY, 10_000_000_000, 6_100_000_000)],
        );
    }
    // Block 4 is the orphan and carries no TOKEN_X contribution at all.
    apply_commit(
        &writer,
        &domain,
        &[blk(4).token(TOKEN_Y, DAY, 5_000_000_000, 4_200_000_000)],
    );

    let key = keys::encode_token_daily_key(&TOKEN_X, DAY).to_vec();
    let before = read_stats(&domain, &key);
    assert!(before.is_some());

    rollback(&domain, &append, 3);

    assert_eq!(
        read_stats(&domain, &key),
        before,
        "a day bucket the orphan never touched must survive byte-for-byte"
    );
    assert_eq!(
        read_stats(&domain, &keys::encode_token_daily_key(&TOKEN_Y, DAY)),
        None,
        "the orphan's own row must be gone"
    );
    assert_eq!(
        append.get_cf(append.cf_cells(), &cell_key).unwrap(),
        Some(cell_bytes)
    );
}

#[tokio::test]
async fn token_daily_orphan_only_row_removed() {
    let (domain, append) = setup_split_stores();
    let writer = BatchWriter::new(domain.clone(), append.clone());

    apply_commit(
        &writer,
        &domain,
        &[blk(1).token(TOKEN_Y, DAY, 1_000_000_000, 900_000_000)],
    );
    apply_commit(
        &writer,
        &domain,
        &[blk(2).token(TOKEN_X, DAY, 7_000_000_000, 6_100_000_000)],
    );

    rollback(&domain, &append, 1);

    assert_eq!(
        read_stats(&domain, &keys::encode_token_daily_key(&TOKEN_X, DAY)),
        None,
        "a row only the orphan ever created must go back to not existing"
    );
    assert!(read_stats(&domain, &keys::encode_token_daily_key(&TOKEN_Y, DAY)).is_some());
}

#[tokio::test]
async fn consume_in_orphan_restores_capacity_and_occupied() {
    let (domain, append) = setup_split_stores();
    let writer = BatchWriter::new(domain.clone(), append.clone());

    apply_commit(
        &writer,
        &domain,
        &[blk(2).token(TOKEN_X, DAY, 12_000_000_000, 6_100_000_000)],
    );
    let key = keys::encode_token_daily_key(&TOKEN_X, DAY).to_vec();
    let before = read_stats(&domain, &key).unwrap();

    // Orphan block 3 consumes the cell block 2 created: both fields go negative.
    apply_commit(
        &writer,
        &domain,
        &[blk(3).token(TOKEN_X, DAY, -12_000_000_000, -6_100_000_000)],
    );
    assert_eq!(
        read_stats(&domain, &key),
        None,
        "net-zero collapses the row, which is exactly the state rollback must undo"
    );

    rollback(&domain, &append, 2);

    assert_eq!(
        read_stats(&domain, &key),
        Some(before),
        "both capacity and occupied capacity must come back, not just one field"
    );
}

#[tokio::test]
async fn same_block_create_then_consume_nets_zero_row() {
    // Block 4 creates and consumes inside itself: net zero, no row either way.
    let original = vec![
        vec![blk(1).token(TOKEN_X, DAY, 9_000_000_000, 6_100_000_000)],
        vec![blk(2)
            .token(TOKEN_Y, DAY, 4_000_000_000, 3_000_000_000)
            .token(TOKEN_Y, DAY, -4_000_000_000, -3_000_000_000)],
    ];
    let (replayed, direct) = run_scenario(&original, 2, &[]);
    assert_eq!(replayed, direct);
    assert_eq!(
        direct.get(&keys::encode_token_daily_key(&TOKEN_Y, DAY).to_vec()),
        None,
        "a create+consume inside one block leaves no row"
    );
}

#[tokio::test]
async fn multi_block_batch_rollback_to_middle() {
    // One commit spanning blocks 1..=5, all writing the same key. Rolling back
    // to 3 must land on the value as of the end of block 3 — which is only
    // expressible if the batch kept per-block identity.
    let commit = vec![
        blk(1).token(TOKEN_X, DAY, 1_000_000_000, 100_000_000),
        blk(2).token(TOKEN_X, DAY, 2_000_000_000, 200_000_000),
        blk(3).token(TOKEN_X, DAY, 4_000_000_000, 400_000_000),
        blk(4).token(TOKEN_X, DAY, 8_000_000_000, 800_000_000),
        blk(5).token(TOKEN_X, DAY, 16_000_000_000, 1_600_000_000),
    ];
    let (domain, append) = setup_split_stores();
    let writer = BatchWriter::new(domain.clone(), append.clone());
    apply_commit(&writer, &domain, &commit);

    rollback(&domain, &append, 3);

    let key = keys::encode_token_daily_key(&TOKEN_X, DAY).to_vec();
    let expected = {
        let (d2, a2) = setup_split_stores();
        let w2 = BatchWriter::new(d2.clone(), a2.clone());
        apply_commit(&w2, &d2, &commit[..3]);
        read_stats(&d2, &key).unwrap()
    };
    assert_eq!(
        read_stats(&domain, &key),
        Some(expected),
        "rolling back into the middle of a batch must yield the end-of-block-3 value"
    );
}

#[tokio::test]
async fn sign_cancel_row_disappears_then_reappears() {
    // Block 3 zeroes the row (deleting it), block 4 recreates it. Rolling back
    // to 2 must restore `Some(bytes)`; rolling back to 3 must restore `None`.
    let commits = vec![
        vec![blk(1).token(TOKEN_X, DAY, 5_000_000_000, 500_000_000)],
        vec![blk(2).token(TOKEN_X, DAY, 3_000_000_000, 300_000_000)],
        vec![blk(3).token(TOKEN_X, DAY, -8_000_000_000, -800_000_000)],
        vec![blk(4).token(TOKEN_X, DAY, 2_000_000_000, 200_000_000)],
    ];
    let key = keys::encode_token_daily_key(&TOKEN_X, DAY).to_vec();

    let (domain, append) = setup_split_stores();
    let writer = BatchWriter::new(domain.clone(), append.clone());
    for c in &commits[..2] {
        apply_commit(&writer, &domain, c);
    }
    let after_block_2 = read_stats(&domain, &key).unwrap();
    for c in &commits[2..] {
        apply_commit(&writer, &domain, c);
    }

    rollback(&domain, &append, 3);
    assert_eq!(
        read_stats(&domain, &key),
        None,
        "rolling back to the block that zeroed the row must restore its absence"
    );

    let (domain2, append2) = setup_split_stores();
    let writer2 = BatchWriter::new(domain2.clone(), append2.clone());
    for c in &commits {
        apply_commit(&writer2, &domain2, c);
    }
    rollback(&domain2, &append2, 2);
    assert_eq!(
        read_stats(&domain2, &key),
        Some(after_block_2),
        "rolling back past the zeroing block must restore the exact earlier bytes"
    );
}

#[tokio::test]
async fn multi_entity_same_day_isolation() {
    let original = vec![
        vec![blk(1)
            .token(TOKEN_X, DAY, 9_000_000_000, 6_100_000_000)
            .script(CODE_A, false, DAY, 9_000_000_000, 6_100_000_000)
            .cluster(CLUSTER_ID, DAY, 2_000_000_000, 1_500_000_000)
            .spore(SPORE_ID, DAY, 2_000_000_000, 1_500_000_000)
            .object(COLLECTION_ID, DAY, 3_000_000_000, 2_800_000_000)],
        vec![blk(2)
            .token(TOKEN_Y, DAY, 4_000_000_000, 3_000_000_000)
            .script(CODE_B, true, DAY, 4_000_000_000, 3_000_000_000)],
    ];
    let (replayed, direct) = run_scenario(&original, 1, &[]);
    assert_eq!(
        replayed, direct,
        "entities the orphan never touched must be bit-identical"
    );
    assert!(direct.contains_key(&keys::encode_token_daily_key(&TOKEN_X, DAY).to_vec()));
    assert!(!direct.contains_key(&keys::encode_token_daily_key(&TOKEN_Y, DAY).to_vec()));
}

#[tokio::test]
async fn utc8_day_and_hour_boundary() {
    // Blocks 1-2 are on the previous UTC+8 day; blocks 3-4 cross into DAY, and
    // block 4's timestamp walks back a day (CKB timestamps are not monotonic).
    let original = vec![vec![
        blk(1)
            .at(TS_PREV_DAY)
            .token(TOKEN_X, PREV_DAY, 1_000_000_000, 100_000_000)
            .token_hour(TOKEN_X, TS_PREV_DAY, 1),
        blk(2)
            .at(TS_DAY)
            .token(TOKEN_X, DAY, 2_000_000_000, 200_000_000)
            .token_hour(TOKEN_X, TS_DAY, 1),
        blk(3)
            .at(TS_DAY + 3_600_000)
            .token(TOKEN_X, DAY, 4_000_000_000, 400_000_000)
            .token_hour(TOKEN_X, TS_DAY + 3_600_000, 1),
        blk(4)
            .at(TS_PREV_DAY + 1_000)
            .token(TOKEN_X, PREV_DAY, 8_000_000_000, 800_000_000)
            .token_hour(TOKEN_X, TS_PREV_DAY, 1),
    ]];
    let (replayed, direct) = run_scenario(&original, 2, &[]);
    assert_eq!(
        replayed, direct,
        "recovery must follow the keys actually written, not assume dates rise with block height"
    );
    assert!(direct.contains_key(&keys::encode_token_daily_key(&TOKEN_X, PREV_DAY).to_vec()));
    assert!(direct.contains_key(&keys::encode_token_daily_key(&TOKEN_X, DAY).to_vec()));
}

#[tokio::test]
async fn depth_1_and_36_ok_depth_37_deep_fork() {
    // Depth 1 and depth 36 are inside the shallow-fork window and must recover
    // exactly; depth 37 is past DEEP_FORK_DEPTH and keeps its existing
    // stop-and-alert semantics, which this matrix must not weaken.
    for depth in [1i64, 36] {
        let original: Vec<Vec<BlockChanges>> = (1..=40i64)
            .map(|n| vec![blk(n).token(TOKEN_X, DAY, 1_000_000_000 * n as i128, 1_000_000)])
            .collect();
        let fork_point = 40 - depth;
        let new_branch: Vec<Vec<BlockChanges>> = ((fork_point + 1)..=40)
            .map(|n| vec![blk(n).token(TOKEN_Y, DAY, 7_000_000_000, 700_000)])
            .collect();
        let (replayed, direct) = run_scenario(&original, fork_point, &new_branch);
        assert_eq!(replayed, direct, "depth {depth} must recover exactly");
    }

    let (domain, append) = setup_split_stores();
    let writer = BatchWriter::new(domain.clone(), append.clone());
    for n in 1..=40i64 {
        apply_commit(
            &writer,
            &domain,
            &[blk(n).token(TOKEN_X, DAY, 1_000_000_000, 1_000_000)],
        );
    }
    let before = dump_entity_stats(&domain);
    let result = writer
        .execute_reorg(
            append.as_ref(),
            3,
            &[0x03; 32],
            40,
            &[0x28; 32],
            41,
            &[0x29; 32],
        )
        .await;
    assert!(
        result.is_err() || dump_entity_stats(&domain) != before,
        "a depth-37 fork must not be silently absorbed by the shallow path"
    );
}

#[tokio::test]
async fn rollback_then_replay_equals_direct() {
    let original = vec![
        vec![
            blk(1)
                .token(TOKEN_X, DAY, 9_000_000_000, 6_100_000_000)
                .script(CODE_A, false, DAY, 9_000_000_000, 6_100_000_000)
                .token_hour(TOKEN_X, TS_DAY, 2),
            blk(2)
                .token(TOKEN_X, DAY, 3_000_000_000, 2_000_000_000)
                .cluster(CLUSTER_ID, DAY, 1_000_000_000, 800_000_000)
                .spore(SPORE_ID, DAY, 1_000_000_000, 800_000_000)
                .spore_hour(CLUSTER_ID, TS_DAY, 1),
        ],
        vec![
            blk(3)
                .token(TOKEN_X, DAY, -2_000_000_000, -1_500_000_000)
                .object(COLLECTION_ID, DAY, 5_000_000_000, 4_800_000_000)
                .object_hour(COLLECTION_ID, TS_DAY, 3),
            // Orphans from here.
            blk(4)
                .token(TOKEN_X, DAY, 100_000_000_000, 90_000_000_000)
                .token(TOKEN_Y, DAY, 6_000_000_000, 5_000_000_000)
                .script(CODE_B, true, DAY, 6_000_000_000, 5_000_000_000)
                .token_hour(TOKEN_X, TS_DAY, 4)
                .object_hour(COLLECTION_ID, TS_DAY, 2),
            blk(5)
                .cluster(CLUSTER_ID, DAY, -1_000_000_000, -800_000_000)
                .spore(SPORE_ID, DAY, -1_000_000_000, -800_000_000)
                .spore_hour(CLUSTER_ID, TS_DAY, 5),
        ],
    ];
    let new_branch = vec![vec![
        blk(4)
            .token(TOKEN_X, DAY, 1_500_000_000, 1_200_000_000)
            .token_hour(TOKEN_X, TS_DAY, 1),
        blk(5)
            .object(COLLECTION_ID, DAY, 700_000_000, 600_000_000)
            .object_hour(COLLECTION_ID, TS_DAY, 1),
    ]];

    let (replayed, direct) = run_scenario(&original, 3, &new_branch);
    assert_eq!(
        replayed, direct,
        "rollback + replay must equal direct sync for all eight entity families"
    );
    assert!(!direct.is_empty(), "fixture must produce rows to compare");
}

#[tokio::test]
async fn two_consecutive_reorgs() {
    let (domain, append) = setup_split_stores();
    let writer = BatchWriter::new(domain.clone(), append.clone());
    for n in 1..=5i64 {
        apply_commit(
            &writer,
            &domain,
            &[blk(n).token(TOKEN_X, DAY, 1_000_000_000, 100_000_000)],
        );
    }
    rollback(&domain, &append, 3);
    for n in 4..=6i64 {
        apply_commit(
            &writer,
            &domain,
            &[blk(n).token(TOKEN_X, DAY, 2_000_000_000, 200_000_000)],
        );
    }
    rollback(&domain, &append, 4);

    let (d2, a2) = setup_split_stores();
    let w2 = BatchWriter::new(d2.clone(), a2.clone());
    for n in 1..=3i64 {
        apply_commit(
            &w2,
            &d2,
            &[blk(n).token(TOKEN_X, DAY, 1_000_000_000, 100_000_000)],
        );
    }
    apply_commit(
        &w2,
        &d2,
        &[blk(4).token(TOKEN_X, DAY, 2_000_000_000, 200_000_000)],
    );

    assert_eq!(
        dump_entity_stats(&domain),
        dump_entity_stats(&d2),
        "two consecutive reorgs must not double-undo, double-count, or collide on undo seq"
    );
    assert!(
        !domain.has_undo_log_entries_after(4).unwrap(),
        "every replayed undo entry must be consumed"
    );
}

#[tokio::test]
async fn append_only_bytes_unchanged() {
    let original = vec![vec![
        blk(1).token(TOKEN_X, DAY, 9_000_000_000, 6_100_000_000),
        blk(2).token(TOKEN_X, DAY, 3_000_000_000, 2_000_000_000),
    ]];
    // `run_scenario` asserts CF_CELLS bytes internally; this test pins that the
    // assertion is actually exercised on a scenario with a real rollback.
    let (replayed, direct) = run_scenario(&original, 1, &[]);
    assert_eq!(replayed, direct);
}

#[tokio::test]
async fn hourly_token_spore_object_symmetry() {
    for which in 0..3u8 {
        let key = match which {
            0 => keys::encode_token_hourly_key(&TOKEN_X, hour_of(TS_DAY)),
            1 => keys::encode_spore_hourly_key(&CLUSTER_ID, hour_of(TS_DAY)),
            _ => keys::encode_object_hourly_key(&COLLECTION_ID, hour_of(TS_DAY)),
        };
        let bump = |b: BlockChanges, by: i64| match which {
            0 => b.token_hour(TOKEN_X, TS_DAY, by),
            1 => b.spore_hour(CLUSTER_ID, TS_DAY, by),
            _ => b.object_hour(COLLECTION_ID, TS_DAY, by),
        };

        // (a) untouched by the orphan → preserved
        let (domain, append) = setup_split_stores();
        let writer = BatchWriter::new(domain.clone(), append.clone());
        apply_commit(&writer, &domain, &[bump(blk(1), 3)]);
        let before = read_stats(&domain, &key).unwrap();
        apply_commit(
            &writer,
            &domain,
            &[blk(2).token(TOKEN_Y, DAY, 1_000_000_000, 100_000_000)],
        );
        rollback(&domain, &append, 1);
        assert_eq!(
            read_stats(&domain, &key),
            Some(before.clone()),
            "hourly family {which}: untouched bucket must survive"
        );

        // (b) created only by the orphan → removed
        let (domain, append) = setup_split_stores();
        let writer = BatchWriter::new(domain.clone(), append.clone());
        apply_commit(
            &writer,
            &domain,
            &[blk(1).token(TOKEN_Y, DAY, 1_000_000_000, 100_000_000)],
        );
        apply_commit(&writer, &domain, &[bump(blk(2), 2)]);
        rollback(&domain, &append, 1);
        assert_eq!(
            read_stats(&domain, &key),
            None,
            "hourly family {which}: orphan-created bucket must be removed"
        );

        // (c) incremented by the orphan → exact previous count
        let (domain, append) = setup_split_stores();
        let writer = BatchWriter::new(domain.clone(), append.clone());
        apply_commit(&writer, &domain, &[bump(blk(1), 3)]);
        let before = read_stats(&domain, &key).unwrap();
        apply_commit(&writer, &domain, &[bump(blk(2), 7)]);
        rollback(&domain, &append, 1);
        assert_eq!(
            read_stats(&domain, &key),
            Some(before),
            "hourly family {which}: bucket must return to the pre-orphan count"
        );

        // (d) multi-block batch rolled back to the middle
        let (domain, append) = setup_split_stores();
        let writer = BatchWriter::new(domain.clone(), append.clone());
        apply_commit(
            &writer,
            &domain,
            &[bump(blk(1), 1), bump(blk(2), 1), bump(blk(3), 1)],
        );
        rollback(&domain, &append, 2);
        assert_eq!(
            read_stats(&domain, &key),
            Some(2i64.to_le_bytes().to_vec()),
            "hourly family {which}: must land on the end-of-block-2 count"
        );
    }
}

// ---------------------------------------------------------------------------
// Task 1.4 — the fixed-shape iCKB three-day gap
// ---------------------------------------------------------------------------

/// Reproduces the shape proved on testnet iCKB: three consecutive UTC+8 days
/// each carrying main-chain contributions and each hit by a depth-1 fork whose
/// orphan block contains nothing for this token, followed by a day of
/// consumption. The three days' rows must equal the exact whole-day capacity
/// increments, the running total must never go negative, and the store's own
/// startup validator must find nothing wrong.
#[tokio::test]
async fn ickb_three_day_gap_shape_is_preserved() {
    const DAY_1: u32 = 20_260_912;
    const DAY_2: u32 = 20_260_913;
    const DAY_3: u32 = 20_260_914;
    const DAY_4: u32 = 20_260_915;
    // 2026-09-12 10:00 UTC+8, then one day apart.
    const TS_1: i64 = 1_789_005_600_000;
    const DAY_MS: i64 = 86_400_000;

    // Per-day whole-day increments (the "correct" column of the incident table).
    const INC_1: i128 = 29_349_109_967_584;
    const INC_2: i128 = 131_195_364_199_711;
    const INC_3: i128 = 143_234_681_921_302;
    // Day 3's contribution is split around its fork point, as it was on testnet.
    const INC_3_BEFORE_FORK: i128 = 71_607_501_066_642;
    const INC_3_AFTER_FORK: i128 = INC_3 - INC_3_BEFORE_FORK;

    let (domain, append) = setup_split_stores();
    let writer = BatchWriter::new(domain.clone(), append.clone());

    let mut block = 1i64;
    let mut push = |writer: &BatchWriter,
                    domain: &CkbadgerStore,
                    block: &mut i64,
                    ts: i64,
                    date: u32,
                    cap: i128| {
        apply_commit(
            writer,
            domain,
            &[blk(*block).at(ts).token(TOKEN_X, date, cap, cap / 10)],
        );
        *block += 1;
    };

    for (day_index, (date, inc)) in [(DAY_1, INC_1), (DAY_2, INC_2)].iter().enumerate() {
        let ts = TS_1 + day_index as i64 * DAY_MS;
        push(&writer, &domain, &mut block, ts, *date, *inc);
        // depth-1 fork: the orphan block carries no iCKB change at all
        let fork_point = block - 1;
        apply_commit(
            &writer,
            &domain,
            &[blk(block)
                .at(ts + 1_000)
                .token(TOKEN_Y, *date, 1_000_000_000, 100_000_000)],
        );
        block += 1;
        rollback(&domain, &append, fork_point);
        block = fork_point + 1;
    }

    // Day 3: contributions before AND after the fork point.
    let ts3 = TS_1 + 2 * DAY_MS;
    push(&writer, &domain, &mut block, ts3, DAY_3, INC_3_BEFORE_FORK);
    let fork_point = block - 1;
    apply_commit(
        &writer,
        &domain,
        &[blk(block)
            .at(ts3 + 1_000)
            .token(TOKEN_Y, DAY_3, 1_000_000_000, 100_000_000)],
    );
    block += 1;
    rollback(&domain, &append, fork_point);
    block = fork_point + 1;
    push(
        &writer,
        &domain,
        &mut block,
        ts3 + 2_000,
        DAY_3,
        INC_3_AFTER_FORK,
    );

    // Day 4: a consumption day.
    let ts4 = TS_1 + 3 * DAY_MS;
    push(
        &writer,
        &domain,
        &mut block,
        ts4,
        DAY_4,
        -14_019_999_853_193,
    );

    let row = |date: u32| -> i128 {
        domain
            .get_token_daily_delta(&TOKEN_X, date)
            .unwrap()
            .map(|d| d.owned_capacity_delta)
            .unwrap_or(0)
    };
    assert_eq!(row(DAY_1), INC_1, "20260912 whole-day increment");
    assert_eq!(row(DAY_2), INC_2, "20260913 whole-day increment");
    assert_eq!(row(DAY_3), INC_3, "20260914 whole-day increment");

    let mut running = 0i128;
    for date in [DAY_1, DAY_2, DAY_3, DAY_4] {
        running += row(date);
        assert!(
            running >= 0,
            "running total went negative at {date}: {running} — the 2026-09-15 \
             `-14,019,999,853,193` that made the testnet store unstartable"
        );
    }
    assert_eq!(
        running,
        INC_1 + INC_2 + INC_3 - 14_019_999_853_193,
        "the surviving total must be the full three-day sum minus the consumption"
    );
    assert!(
        domain
            .find_first_invalid_token_daily_delta()
            .unwrap()
            .is_none(),
        "the startup validator must find no negative running total"
    );
}
