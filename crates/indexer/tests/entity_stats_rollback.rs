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
fn dump_entity_stats(domain: &CkbadgerStore) -> EntityStatsDump {
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

/// Byte-exact dump of the eight entity stats families, keyed by full stats key.
type EntityStatsDump = BTreeMap<Vec<u8>, Vec<u8>>;

/// The core driver: build the original branch, roll back to `fork_point`,
/// replay the new branch — and build the same surviving history directly in a
/// second pair of stores. Returns `(after_rollback_replay, direct)`.
fn run_scenario(
    original: &[Vec<BlockChanges>],
    fork_point: i64,
    new_branch: &[Vec<BlockChanges>],
) -> (EntityStatsDump, EntityStatsDump) {
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
        direct.get(keys::encode_token_daily_key(&TOKEN_Y, DAY).as_slice()),
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
    assert!(direct.contains_key(keys::encode_token_daily_key(&TOKEN_X, DAY).as_slice()));
    assert!(!direct.contains_key(keys::encode_token_daily_key(&TOKEN_Y, DAY).as_slice()));
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
    assert!(direct.contains_key(keys::encode_token_daily_key(&TOKEN_X, PREV_DAY).as_slice()));
    assert!(direct.contains_key(keys::encode_token_daily_key(&TOKEN_X, DAY).as_slice()));
}

#[tokio::test]
async fn depth_1_and_36_recover_exactly() {
    // Depth 1 and depth 36 are inside the shallow-fork window and must recover
    // exactly. The depth gate itself lives in the CALLER
    // (`sync/reorg.rs::handle_reorg`, `depth > DEEP_FORK_DEPTH`), not in
    // `execute_reorg`, and needs RPC to decide — it is covered by
    // `tests/reorg_handling.rs::test_deep_fork_flag`. Asserting it from here
    // could only produce a near-unfalsifiable `is_err() || changed` check.
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
        assert!(
            !direct.is_empty(),
            "depth {depth} fixture must produce rows"
        );
    }
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
    let push = |writer: &BatchWriter,
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
        rollback(&domain, &append, fork_point);
        // Replay resumes at the fork point's successor.
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

// ---------------------------------------------------------------------------
// Task 2.5 — coverage contract, bounded window, floor enforcement
// ---------------------------------------------------------------------------

/// A store with a tip but no contract was written by a build that deleted
/// entity stats buckets on rollback. There is no honest migration.
#[tokio::test]
async fn startup_fails_on_nonfresh_store_without_contract() {
    let (domain, _append) = setup_split_stores();

    // Fresh store: nothing to protect, the first commit writes the contract.
    ckbadger_indexer::entry::ensure_entity_stats_undo_contract_on_startup(&domain)
        .expect("a fresh store must be allowed to start");

    // Give it a tip and nothing else.
    let mut batch = StoreBatch::new(&domain);
    batch.put_block_header(500, &make_header(500, TS_DAY));
    batch.put_sync_meta(
        keys::sync_meta_keys::SYNC_STATUS,
        &bincode::serialize(&ckbadger_store::types::SyncStatus {
            tip_block_number: 500,
            tip_block_hash: make_header(500, TS_DAY).hash,
            ..Default::default()
        })
        .unwrap(),
    );
    batch.commit().unwrap();

    let err = ckbadger_indexer::entry::ensure_entity_stats_undo_contract_on_startup(&domain)
        .expect_err("a non-fresh store without a contract must refuse to start");
    assert!(
        ckbadger_indexer::lifecycle::is_rebuild_required(&err),
        "must be a rebuild-required error, got: {err:#}"
    );
    assert!(
        err.to_string().contains("entity stats undo contract"),
        "got: {err:#}"
    );

    // With the contract present it starts.
    domain
        .put_entity_stats_undo_contract(&ckbadger_store::types::EntityStatsUndoContract {
            version: ckbadger_store::types::ENTITY_STATS_UNDO_CONTRACT_VERSION,
            coverage_floor_block: 0,
            updated_at_block: 500,
        })
        .unwrap();
    ckbadger_indexer::entry::ensure_entity_stats_undo_contract_on_startup(&domain).unwrap();
}

#[tokio::test]
async fn prune_keeps_window_and_updates_floor() {
    let (domain, _append) = setup_split_stores();

    const ENTITY: u64 = 0x0004 << 48;
    const TX_CONTEXT: u64 = 0x0001 << 48;
    let mut batch = StoreBatch::new(&domain);
    for block in 1..=1_200i64 {
        batch.put_reorg_undo_log_by_block(
            block,
            ENTITY,
            &ckbadger_store::types::UndoLogEntry::KeyMutation {
                target_store: ckbadger_store::types::UndoLogStoreTarget::Domain,
                cf_name: ckbadger_store::CF_STATS_TOKEN.to_string(),
                key: keys::encode_token_daily_key(&TOKEN_X, DAY).to_vec(),
                previous_value: None,
            },
        );
        batch.put_reorg_undo_log_by_block(
            block,
            TX_CONTEXT,
            &ckbadger_store::types::UndoLogEntry::TxContext(ckbadger_store::types::UndoTxContext {
                tx_hash: vec![block as u8; 32],
                outputs_count: 1,
                inputs: vec![],
            }),
        );
    }
    batch.commit().unwrap();

    let mut batch = StoreBatch::new(&domain);
    let pruned =
        ckbadger_indexer::sync::stage_entity_stats_undo_retention(&domain, &mut batch, 1_200)
            .unwrap();
    batch.commit().unwrap();
    assert_eq!(
        pruned, 200,
        "blocks 1..=200 fall out of the 1000-block window"
    );

    let mut entity_blocks = Vec::new();
    let mut tx_context_entries = 0usize;
    let iter = domain.iterator_cf(domain.cf_reorg_undo_log_by_block(), IteratorMode::Start);
    for item in iter {
        let (key, _) = item.unwrap();
        let (block, seq) = keys::decode_reorg_undo_log_key(&key);
        if seq >> 48 == 0x0004 {
            entity_blocks.push(block);
        } else {
            tx_context_entries += 1;
        }
    }
    assert_eq!(entity_blocks.len(), 1_000);
    assert_eq!(*entity_blocks.iter().min().unwrap(), 201);
    assert_eq!(*entity_blocks.iter().max().unwrap(), 1_200);
    assert_eq!(
        tx_context_entries, 1_200,
        "retention must not touch the TxContext scope"
    );

    let contract = domain.get_entity_stats_undo_contract().unwrap().unwrap();
    assert_eq!(contract.coverage_floor_block, 200);
    assert_eq!(contract.updated_at_block, 1_200);

    // The floor never moves backwards, even if the tip does.
    let mut batch = StoreBatch::new(&domain);
    assert_eq!(
        ckbadger_indexer::sync::stage_entity_stats_undo_retention(&domain, &mut batch, 900)
            .unwrap(),
        0
    );
    batch.commit().unwrap();
    assert_eq!(
        domain
            .get_entity_stats_undo_contract()
            .unwrap()
            .unwrap()
            .coverage_floor_block,
        200
    );
}

#[tokio::test]
async fn reorg_below_floor_fails_fast() {
    let (domain, append) = setup_split_stores();
    let writer = BatchWriter::new(domain.clone(), append.clone());

    apply_commit(
        &writer,
        &domain,
        &[
            blk(1).token(TOKEN_X, DAY, 9_000_000_000, 6_100_000_000),
            blk(2).token(TOKEN_X, DAY, 3_000_000_000, 2_000_000_000),
        ],
    );
    domain
        .put_entity_stats_undo_contract(&ckbadger_store::types::EntityStatsUndoContract {
            version: ckbadger_store::types::ENTITY_STATS_UNDO_CONTRACT_VERSION,
            coverage_floor_block: 200,
            updated_at_block: 1_200,
        })
        .unwrap();

    let before = dump_entity_stats(&domain);
    assert!(!before.is_empty());

    let result = writer
        .execute_reorg(
            append.as_ref(),
            150,
            &[0x96; 32],
            1_200,
            &[0x11; 32],
            1_201,
            &[0x22; 32],
        )
        .await;
    let err = match result {
        Ok(_) => panic!("a fork point below the coverage floor must fail fast"),
        Err(err) => err,
    };
    assert!(
        ckbadger_indexer::lifecycle::is_rebuild_required(&err),
        "must be rebuild-required, got: {err:#}"
    );
    assert!(
        err.to_string().contains("coverage floor 200")
            && err.to_string().contains("reorg fork point 150"),
        "got: {err:#}"
    );
    assert_eq!(
        dump_entity_stats(&domain),
        before,
        "a refused reorg must not have modified any entity stats row"
    );
}

#[tokio::test]
async fn bulk_completion_writes_contract() {
    let (domain, _append) = setup_split_stores();
    assert!(domain.get_entity_stats_undo_contract().unwrap().is_none());

    // Bulk stops once it is within `bulk_sync_threshold` of the sampled chain
    // tip, so the block it actually WROTE (the handoff tip) is below that tip.
    // The floor is a statement about what exists in the store, so it has to be
    // the handoff tip; taking the chain tip would claim coverage over blocks
    // bulk never wrote and make every early live reorg demand a rebuild.
    const SAMPLED_CHAIN_TIP: u64 = 22_500_000;
    const HANDOFF_TIP: i64 = 22_499_000;
    ckbadger_indexer::sync::persist_bulk_sync_completion_status_for_test(
        &domain,
        SAMPLED_CHAIN_TIP,
        HANDOFF_TIP,
    )
    .unwrap();

    let contract = domain.get_entity_stats_undo_contract().unwrap().unwrap();
    assert_eq!(
        contract.version,
        ckbadger_store::types::ENTITY_STATS_UNDO_CONTRACT_VERSION
    );
    assert_eq!(
        contract.coverage_floor_block, HANDOFF_TIP,
        "bulk records no undo entries, so the last block it WROTE is the floor"
    );
    assert_eq!(contract.updated_at_block, HANDOFF_TIP);
    assert!(
        contract.coverage_floor_block
            <= SAMPLED_CHAIN_TIP as i64 - ckbadger_indexer::config::DEEP_FORK_DEPTH as i64,
        "the handoff must leave live at least DEEP_FORK_DEPTH blocks to build undo \
         coverage over before a legal shallow fork can land"
    );
    // And a store built that way now passes the startup gate.
    ckbadger_indexer::entry::ensure_entity_stats_undo_contract_on_startup(&domain).unwrap();
}

// ---------------------------------------------------------------------------
// Task 2.6 — hourly retention runs inside the writer
// ---------------------------------------------------------------------------

/// Seed `count` hourly buckets for one entity, one per hour ending at
/// `newest_hour`, and return their keys oldest-first.
fn seed_token_hourly(domain: &CkbadgerStore, newest_hour: i64, count: i64) -> Vec<Vec<u8>> {
    let mut batch = StoreBatch::new(domain);
    let mut keys = Vec::new();
    for i in (0..count).rev() {
        let key = keys::encode_token_hourly_key(&TOKEN_X, newest_hour - i);
        batch.put_stats(&key, &1i64.to_le_bytes());
        keys.push(key);
    }
    batch.commit().unwrap();
    keys
}

/// A chain that stopped producing blocks: the clock says "48h ago" but the
/// block at `tip - ENTITY_STATS_UNDO_RETAIN_BLOCKS` is far older, and every
/// hourly key an undo entry in that window could restore must survive.
#[tokio::test]
async fn retention_step_protects_undo_window() {
    let (domain, append) = setup_split_stores();
    let writer = BatchWriter::new(domain.clone(), append.clone());

    // Tip 5000; the undo-window floor block 4000 has a timestamp 200 hours old.
    let now_ms = TS_DAY;
    let now_hour = now_ms / 3_600_000;
    let floor_block_hour = now_hour - 200;
    {
        let mut batch = StoreBatch::new(&domain);
        batch.put_block_header(
            5_000 - ckbadger_indexer::sync::ENTITY_STATS_UNDO_RETAIN_BLOCKS,
            &make_header(4_000, floor_block_hour * 3_600_000),
        );
        batch.commit().unwrap();
    }

    let cutoff = writer
        .hourly_retention_cutoff_hour(now_ms, 5_000, i64::MIN)
        .unwrap();
    assert_eq!(
        cutoff, floor_block_hour,
        "the block-derived bound must win over the 48h clock bound when the chain stalls; \
         assuming 36 blocks is always under 48 hours is exactly what breaks here"
    );

    // Buckets between the block bound and the clock bound must survive.
    seed_token_hourly(&domain, now_hour, 1);
    let protected = keys::encode_token_hourly_key(&TOKEN_X, floor_block_hour + 1);
    let expired = keys::encode_token_hourly_key(&TOKEN_X, floor_block_hour - 1);
    {
        let mut batch = StoreBatch::new(&domain);
        batch.put_stats(&protected, &7i64.to_le_bytes());
        batch.put_stats(&expired, &7i64.to_le_bytes());
        batch.commit().unwrap();
    }

    let mut batch = StoreBatch::new(&domain);
    ckbadger_indexer::sync::stage_hourly_retention(&writer, &mut batch, 5_000, now_ms).unwrap();
    batch.commit().unwrap();

    assert!(
        domain.get_stats_key(&protected).unwrap().is_some(),
        "a bucket inside the undo window must survive even though it is older than 48h"
    );
    assert!(
        domain.get_stats_key(&expired).unwrap().is_none(),
        "a bucket older than both bounds must go"
    );
}

#[tokio::test]
async fn retention_state_committed_with_deletes() {
    let (domain, append) = setup_split_stores();
    let writer = BatchWriter::new(domain.clone(), append.clone());

    let now_ms = TS_DAY;
    let now_hour = now_ms / 3_600_000;
    let hourly = seed_token_hourly(&domain, now_hour, 200);

    // A canonical block at or below the tip with no header is store corruption,
    // not a reason to skip the round: without that header there is no honest
    // block bound, and answering "delete nothing" would hide the corruption
    // behind a sweep that quietly stopped working.
    let mut batch = StoreBatch::new(&domain);
    let err = ckbadger_indexer::sync::stage_hourly_retention(&writer, &mut batch, 5_000, now_ms)
        .expect_err("a missing undo-window header must fail the retention step");
    assert!(
        err.to_string()
            .contains("missing header for the entity-stats undo window block")
            && err.to_string().contains("block=4000")
            && err.to_string().contains("committed_tip=5000"),
        "got: {err:#}"
    );
    assert!(
        domain
            .get_hourly_retention_state(ckbadger_store::types::HourlyRetentionFamily::Token)
            .unwrap()
            .is_none(),
        "a failed step must leave no retention state behind"
    );
    assert!(domain.get_stats_key(&hourly[0]).unwrap().is_some());
    drop(batch); // simulate a failed commit

    assert!(domain
        .get_hourly_retention_state(ckbadger_store::types::HourlyRetentionFamily::Token)
        .unwrap()
        .is_none());
    assert!(
        domain.get_stats_key(&hourly[0]).unwrap().is_some(),
        "a dropped batch must delete nothing"
    );

    // Now with a header at the floor block, so there IS a bound, and commit.
    {
        let mut batch = StoreBatch::new(&domain);
        batch.put_block_header(
            5_000 - ckbadger_indexer::sync::ENTITY_STATS_UNDO_RETAIN_BLOCKS,
            &make_header(4_000, now_ms),
        );
        batch.commit().unwrap();
    }
    let mut batch = StoreBatch::new(&domain);
    ckbadger_indexer::sync::stage_hourly_retention(&writer, &mut batch, 5_000, now_ms).unwrap();
    batch.commit().unwrap();

    let state = domain
        .get_hourly_retention_state(ckbadger_store::types::HourlyRetentionFamily::Token)
        .unwrap()
        .unwrap();
    assert_eq!(
        state.policy_version,
        ckbadger_store::types::HOURLY_RETENTION_POLICY_VERSION
    );
    assert_eq!(state.executed_cutoff_hour, now_hour - 48);
    assert!(state.round_completed_at.is_some());
    assert!(state.cursor.is_none());

    // Everything older than the boundary is gone; the last 48 hours remain.
    for key in &hourly {
        let hour = i64::from_be_bytes(key[33..41].try_into().unwrap());
        let present = domain.get_stats_key(key).unwrap().is_some();
        assert_eq!(
            present,
            hour >= state.executed_cutoff_hour,
            "bucket at hour {hour} present={present} but boundary is {}",
            state.executed_cutoff_hour
        );
    }
}

#[tokio::test]
async fn retention_never_runs_in_bulk_mode() {
    let (domain, append) = setup_split_stores();
    let writer = BatchWriter::new(domain.clone(), append.clone());
    let now_ms = TS_DAY;
    let now_hour = now_ms / 3_600_000;
    let hourly = seed_token_hourly(&domain, now_hour - 500, 10);
    {
        let mut batch = StoreBatch::new(&domain);
        batch.put_block_header(
            5_000 - ckbadger_indexer::sync::ENTITY_STATS_UNDO_RETAIN_BLOCKS,
            &make_header(4_000, TS_DAY),
        );
        batch.commit().unwrap();
    }

    domain.set_bulk_sync_mode(true);
    let mut batch = StoreBatch::new(&domain);
    ckbadger_indexer::sync::stage_hourly_retention(&writer, &mut batch, 5_000, now_ms).unwrap();
    batch.commit().unwrap();
    assert!(domain
        .get_hourly_retention_state(ckbadger_store::types::HourlyRetentionFamily::Token)
        .unwrap()
        .is_none());
    for key in &hourly {
        assert!(domain.get_stats_key(key).unwrap().is_some());
    }
    domain.set_bulk_sync_mode(false);
}

#[tokio::test]
async fn clock_rollback_does_not_lower_executed_cutoff() {
    let (domain, append) = setup_split_stores();
    let writer = BatchWriter::new(domain.clone(), append.clone());

    let now_ms = TS_DAY;
    {
        let mut batch = StoreBatch::new(&domain);
        batch.put_block_header(
            5_000 - ckbadger_indexer::sync::ENTITY_STATS_UNDO_RETAIN_BLOCKS,
            &make_header(4_000, now_ms),
        );
        batch.commit().unwrap();
    }
    let forward = writer
        .hourly_retention_cutoff_hour(now_ms, 5_000, i64::MIN)
        .unwrap();

    // The clock jumps a week backwards; the boundary must not follow it, or the
    // store would claim to still hold hours it has already deleted.
    let rolled_back = writer
        .hourly_retention_cutoff_hour(now_ms - 7 * 24 * 3_600_000, 5_000, forward)
        .unwrap();
    assert_eq!(
        rolled_back, forward,
        "executed_cutoff_hour is monotonic; a backwards clock must not lower it"
    );
}

// ---------------------------------------------------------------------------
// Task 2.7 — interrupted rollback recovery
// ---------------------------------------------------------------------------

/// `execute_reorg` replays the undo log and THEN runs the domain rollback. A
/// crash between the two leaves the entity stats already restored and their
/// undo entries already consumed. The startup cleanup path must finish the same
/// rollback without undoing anything a second time.
#[tokio::test]
async fn crash_between_undo_replay_and_domain_rollback_is_recoverable() {
    let original = vec![vec![
        blk(1)
            .token(TOKEN_X, DAY, 9_000_000_000, 6_100_000_000)
            .script(CODE_A, false, DAY, 9_000_000_000, 6_100_000_000)
            .token_hour(TOKEN_X, TS_DAY, 2),
        blk(2)
            .token(TOKEN_X, DAY, 3_000_000_000, 2_000_000_000)
            .object(COLLECTION_ID, DAY, 5_000_000_000, 4_800_000_000),
        // Orphans from here.
        blk(3)
            .token(TOKEN_X, DAY, 100_000_000_000, 90_000_000_000)
            .token(TOKEN_Y, DAY, 6_000_000_000, 5_000_000_000)
            .token_hour(TOKEN_X, TS_DAY, 4),
        blk(4)
            .object(COLLECTION_ID, DAY, -5_000_000_000, -4_800_000_000)
            .spore(SPORE_ID, DAY, 1_000_000_000, 800_000_000),
    ]];

    // Branch A: apply everything, replay the undo log, then CRASH — no
    // `rollback_to_block`, no refreshes. Recovery runs the startup path.
    let (domain_a, append_a) = setup_split_stores();
    let writer_a = BatchWriter::new(domain_a.clone(), append_a.clone());
    let (cell_key, cell_bytes) = seed_append_only_witness(&append_a);
    for commit in &original {
        apply_commit(&writer_a, &domain_a, commit);
    }
    domain_a
        .set_genesis_baseline(&ckbadger_store::GenesisBaseline {
            total_issuance: 3_360_000_000_000_000_000,
            burnt: 840_000_000_000_000_000,
            virtual_occupied: 0,
        })
        .unwrap();
    // The startup cleanup path also re-derives the DAO singleton stats, which
    // need a daily snapshot to exist. Seed one on a day BEFORE the cutoff, the
    // way a real chain would have it; it is untouched by this rollback.
    domain_a
        .put_stats_key(
            &keys::encode_stats_key(
                keys::STATS_PREFIX_DAO_DAILY_SNAPSHOT,
                PREV_DAY.to_string().as_bytes(),
            ),
            &bincode::serialize(&ckbadger_store::types::DaoDailySnapshot {
                date: PREV_DAY.to_string(),
                total_deposited: 0,
                depositors_count: 0,
                new_deposits: 0,
                withdrawals: 0,
                compensation: 0,
                cumulative_deposit_amount: 0,
                total_issuance: 0,
                secondary_pool: 0,
                occupied_capacity: 0,
                cum_miner_secondary: 0,
                cum_dao_compensation: 0,
                cum_treasury: 0,
                unclaimed_compensation: 0,
                frozen_phase1_compensation: 0,
                cumulative_depositors: 0,
                daily_depositor_addresses: 0,
                protocol_deposited: None,
            })
            .unwrap(),
        )
        .unwrap();

    let undo = domain_a.rollback_via_undo_log(&append_a, 2).unwrap();
    assert!(
        undo.undo_entries_applied > 0,
        "the fixture must exercise the undo path"
    );
    let after_undo_only = dump_entity_stats(&domain_a);
    assert!(
        !domain_a.has_undo_log_entries_after(2).unwrap(),
        "the replayed entries are consumed before the crash"
    );

    // Recovery: the startup cleanup path with the same target.
    writer_a
        .init_sync_start_with_options(append_a.as_ref(), 2, false, true)
        .expect("startup cleanup must finish the interrupted rollback");

    assert_eq!(
        dump_entity_stats(&domain_a),
        after_undo_only,
        "recovery must not undo the already-applied restoration a second time"
    );
    assert_eq!(
        append_a.get_cf(append_a.cf_cells(), &cell_key).unwrap(),
        Some(cell_bytes),
        "append-only payload bytes must be unchanged by crash recovery"
    );

    // Branch B: the same surviving history, synced directly.
    let (domain_b, append_b) = setup_split_stores();
    let writer_b = BatchWriter::new(domain_b.clone(), append_b.clone());
    seed_append_only_witness(&append_b);
    let surviving: Vec<BlockChanges> = original[0]
        .iter()
        .filter(|b| b.block <= 2)
        .cloned()
        .collect();
    apply_commit(&writer_b, &domain_b, &surviving);

    assert_eq!(
        dump_entity_stats(&domain_a),
        dump_entity_stats(&domain_b),
        "a crash between undo replay and domain rollback must still land on the \
         same eight entity stats families as a node that never saw the orphans"
    );
}

/// If the domain rollback fails after the undo log was replayed, the restored
/// values must stay visible and the cleanup marker must stay set, so the next
/// start finishes the same rollback instead of publishing an inconsistent tip.
#[tokio::test]
async fn rollback_to_block_failure_leaves_undo_replay_visible_and_marker_set() {
    let (domain, append) = setup_split_stores();
    let writer = BatchWriter::new(domain.clone(), append.clone());

    apply_commit(
        &writer,
        &domain,
        &[
            blk(1).token(TOKEN_X, DAY, 9_000_000_000, 6_100_000_000),
            blk(2).token(TOKEN_X, DAY, 3_000_000_000, 2_000_000_000),
            blk(3).token(TOKEN_X, DAY, 100_000_000_000, 90_000_000_000),
        ],
    );
    let key = keys::encode_token_daily_key(&TOKEN_X, DAY).to_vec();
    let after_block_3 = read_stats(&domain, &key).unwrap();

    // Corrupt block 3's header so the domain rollback cannot decode it.
    domain
        .put_cf(
            domain.cf_block_headers(),
            &keys::encode_block_num(3),
            b"invalid-header-payload",
        )
        .unwrap();

    let undo = domain.rollback_via_undo_log(&append, 2).unwrap();
    assert!(undo.undo_entries_applied > 0);
    let restored = read_stats(&domain, &key).unwrap();
    assert_ne!(
        restored, after_block_3,
        "the undo replay must have rolled the row back to its end-of-block-2 value"
    );

    let err = domain
        .rollback_to_block_with_tx_contexts(2, Some(append.as_ref()), undo.tx_contexts)
        .expect_err("a corrupt header must fail the domain rollback");
    assert!(!err.to_string().is_empty());

    assert_eq!(
        read_stats(&domain, &key),
        Some(restored),
        "a failed domain rollback must leave the undo replay's result intact — the next \
         start finishes the same rollback, it does not redo it"
    );
    assert!(
        !domain.has_undo_log_entries_after(2).unwrap(),
        "consumed undo entries must not reappear and be applied twice"
    );
}

// ---------------------------------------------------------------------------
// Review M2 — every rollback entry point must honour the coverage floor
// ---------------------------------------------------------------------------

/// `execute_reorg` refuses a fork below the coverage floor, but the two startup
/// cleanup entry points did not. They matter more, not less: the startup path
/// resets to -1 whenever the tip header is missing, and since Task 2.4 the
/// cutoff sweep no longer deletes the eight entity families while the undo log
/// only covers the last 1000 blocks. Every entity row would therefore survive a
/// "full reset" that wipes blocks, cells and indexes, and the re-sync would add
/// the same deltas on top — a silent double count that
/// `find_first_invalid_token_daily_delta` cannot see, because the totals stay
/// positive.
#[tokio::test]
async fn startup_cleanup_below_coverage_floor_fails_fast() {
    let (domain, append) = setup_split_stores();
    let writer = BatchWriter::new(domain.clone(), append.clone());

    apply_commit(
        &writer,
        &domain,
        &[
            blk(1).token(TOKEN_X, DAY, 9_000_000_000, 6_100_000_000),
            blk(2).token(TOKEN_X, DAY, 3_000_000_000, 2_000_000_000),
        ],
    );
    domain
        .put_entity_stats_undo_contract(&ckbadger_store::types::EntityStatsUndoContract {
            version: ckbadger_store::types::ENTITY_STATS_UNDO_CONTRACT_VERSION,
            coverage_floor_block: 1_000,
            updated_at_block: 2_000,
        })
        .unwrap();

    // Partial data strictly after the startup tip, whose own header is missing:
    // the startup path takes the partial-data branch and resets to -1.
    let mut batch = StoreBatch::new(&domain);
    batch.put_tx_index(
        3_001,
        0,
        &ckbadger_store::types::TxIndexEntry {
            is_cellbase: true,
            timestamp: TS_DAY,
            inputs_count: 0,
            outputs_count: 1,
            fee: 0,
            tx_size: 128,
            cycles: None,
            semantic_tags: 0,
        },
    );
    batch.commit().unwrap();

    let before = dump_entity_stats(&domain);
    assert!(!before.is_empty());

    let err = writer
        .init_sync_start_with_options(append.as_ref(), 3_000, false, true)
        .expect_err("a cleanup below the coverage floor must fail fast");
    assert!(
        ckbadger_indexer::lifecycle::is_rebuild_required(&err),
        "must be rebuild-required, got: {err:#}"
    );
    assert!(
        err.to_string().contains("coverage floor 1000"),
        "got: {err:#}"
    );
    assert_eq!(
        dump_entity_stats(&domain),
        before,
        "a refused cleanup must not have modified any entity stats row"
    );
}

#[tokio::test]
async fn cleanup_batch_range_below_coverage_floor_fails_fast() {
    let (domain, append) = setup_split_stores();
    let writer = BatchWriter::new(domain.clone(), append.clone());

    apply_commit(
        &writer,
        &domain,
        &[blk(1).token(TOKEN_X, DAY, 9_000_000_000, 6_100_000_000)],
    );
    domain
        .put_entity_stats_undo_contract(&ckbadger_store::types::EntityStatsUndoContract {
            version: ckbadger_store::types::ENTITY_STATS_UNDO_CONTRACT_VERSION,
            coverage_floor_block: 500,
            updated_at_block: 1_500,
        })
        .unwrap();

    let before = dump_entity_stats(&domain);
    let err = writer
        .cleanup_batch_range(append.as_ref(), 100, 200)
        .expect_err("a batch-range cleanup below the coverage floor must fail fast");
    assert!(
        ckbadger_indexer::lifecycle::is_rebuild_required(&err),
        "must be rebuild-required, got: {err:#}"
    );
    assert!(
        err.to_string().contains("coverage floor 500"),
        "got: {err:#}"
    );
    assert_eq!(dump_entity_stats(&domain), before);
}

/// The guard must not fire on the ordinary case: a cleanup target at or above
/// the floor still runs, and a fresh store with no contract yet still runs.
#[tokio::test]
async fn cleanup_at_or_above_coverage_floor_still_runs() {
    let (domain, append) = setup_split_stores();
    let writer = BatchWriter::new(domain.clone(), append.clone());

    apply_commit(
        &writer,
        &domain,
        &[
            blk(1).token(TOKEN_X, DAY, 9_000_000_000, 6_100_000_000),
            blk(2).token(TOKEN_X, DAY, 3_000_000_000, 2_000_000_000),
        ],
    );
    // No contract yet (fresh store mid-first-batch): cleanup proceeds.
    writer
        .cleanup_batch_range(append.as_ref(), 2, 2)
        .expect("a store with no contract has nothing to protect");

    domain
        .put_entity_stats_undo_contract(&ckbadger_store::types::EntityStatsUndoContract {
            version: ckbadger_store::types::ENTITY_STATS_UNDO_CONTRACT_VERSION,
            coverage_floor_block: 1,
            updated_at_block: 2,
        })
        .unwrap();
    writer
        .cleanup_batch_range(append.as_ref(), 2, 2)
        .expect("a target exactly at the floor is inside coverage");
}

/// Review m5: a live reorg on a store with NO contract must refuse, not
/// silently proceed. Startup already rejects any non-fresh store without one,
/// so this is unreachable today — which is exactly why it should be stated
/// rather than left as an `if let Some(..)` that quietly does nothing.
#[tokio::test]
async fn reorg_without_a_contract_fails_fast() {
    let (domain, append) = setup_split_stores();
    let writer = BatchWriter::new(domain.clone(), append.clone());

    apply_commit(
        &writer,
        &domain,
        &[
            blk(1).token(TOKEN_X, DAY, 9_000_000_000, 6_100_000_000),
            blk(2).token(TOKEN_X, DAY, 3_000_000_000, 2_000_000_000),
        ],
    );
    assert!(domain.get_entity_stats_undo_contract().unwrap().is_none());

    let before = dump_entity_stats(&domain);
    let result = writer
        .execute_reorg(
            append.as_ref(),
            1,
            &[0x01; 32],
            2,
            &[0x02; 32],
            3,
            &[0x03; 32],
        )
        .await;
    let err = match result {
        Ok(_) => panic!("a reorg without a coverage contract must fail fast"),
        Err(err) => err,
    };
    assert!(
        ckbadger_indexer::lifecycle::is_rebuild_required(&err),
        "must be rebuild-required, got: {err:#}"
    );
    assert!(
        err.to_string()
            .contains("has no entity stats undo contract"),
        "got: {err:#}"
    );
    assert_eq!(dump_entity_stats(&domain), before);
}

// ---------------------------------------------------------------------------
// Task 5.1 — recovery is persisted, not merely in-process
// ---------------------------------------------------------------------------

/// Everything above runs against a store that stays open for the whole test, so
/// a rollback that only looked right because of an in-memory overlay, an
/// unflushed memtable or a batch-scoped cache would still pass. This one closes
/// the RocksDB handles and reopens the same directory before asserting.
///
/// The shape is the production symptom: a day of accumulation, a depth-1 fork
/// inside that day, then a consumption day on the surviving branch. If the
/// cutoff day's bucket were lost, the replayed consumption alone would drive
/// the running total negative and `find_first_invalid_token_daily_delta` — the
/// check `reconcile_token_daily_deltas_on_startup` runs at every indexer start
/// — would refuse to start the store, exactly as testnet did on 2026-09-15.
#[tokio::test]
async fn recovered_entity_stats_survive_close_and_reopen() {
    let domain_dir = tempfile::tempdir().unwrap();
    let append_dir = tempfile::tempdir().unwrap();

    let in_process = {
        let domain = Arc::new(CkbadgerStore::open_domain(domain_dir.path()).unwrap());
        let append = Arc::new(CkbadgerStore::open_append_only(append_dir.path()).unwrap());
        let writer = BatchWriter::new(domain.clone(), append.clone());

        // Blocks 1..=3, all on the previous UTC+8 day: the history the fork
        // must not touch.
        for n in 1..=3i64 {
            apply_commit(
                &writer,
                &domain,
                &[blk(n)
                    .at(TS_PREV_DAY + n * 1_000)
                    .token(TOKEN_X, PREV_DAY, 10_000_000_000, 6_100_000_000)
                    .token_hour(TOKEN_X, TS_PREV_DAY, 1)],
            );
        }
        // Orphan block 4, same day, moves TOKEN_X and creates TOKEN_Y.
        apply_commit(
            &writer,
            &domain,
            &[blk(4)
                .at(TS_PREV_DAY + 4_000)
                .token(TOKEN_X, PREV_DAY, 25_000_000_000, 13_000_000_000)
                .token(TOKEN_Y, PREV_DAY, 9_000_000_000, 4_200_000_000)
                .token_hour(TOKEN_X, TS_PREV_DAY, 1)],
        );

        rollback(&domain, &append, 3);

        // The surviving branch replays block 4 as a consumption on the next day.
        apply_commit(
            &writer,
            &domain,
            &[blk(4)
                .at(TS_DAY)
                .token(TOKEN_X, DAY, -20_000_000_000, -12_000_000_000)
                .token_hour(TOKEN_X, TS_DAY, 1)],
        );

        assert_eq!(
            domain
                .get_token_daily_delta(&TOKEN_X, PREV_DAY)
                .unwrap()
                .unwrap()
                .owned_capacity_delta,
            30_000_000_000,
            "the three pre-fork blocks, without the orphan's 25_000_000_000"
        );
        assert!(
            domain
                .find_first_invalid_token_daily_delta()
                .unwrap()
                .is_none(),
            "the startup validator must pass before the store is closed"
        );

        let dump = dump_entity_stats(&domain);
        drop(writer);
        dump
    };

    // Reopen the same directories with fresh RocksDB handles — no overlay, no
    // memtable from the writing process, nothing cached.
    let reopened = CkbadgerStore::open_domain(domain_dir.path()).unwrap();

    assert!(
        reopened
            .find_first_invalid_token_daily_delta()
            .unwrap()
            .is_none(),
        "`reconcile_token_daily_deltas_on_startup`'s check must pass on a \
         reopened store: recovery has to be on disk, not in the writer's overlay"
    );
    assert_eq!(
        dump_entity_stats(&reopened),
        in_process,
        "every entity stats row must be byte-identical after close and reopen"
    );
    assert_eq!(
        reopened
            .get_token_daily_delta(&TOKEN_X, PREV_DAY)
            .unwrap()
            .unwrap()
            .owned_capacity_delta,
        30_000_000_000
    );
    assert!(
        reopened
            .get_token_daily_delta(&TOKEN_Y, PREV_DAY)
            .unwrap()
            .is_none(),
        "the orphan-only row must still be absent after reopen"
    );
}
