use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::RwLock;
use std::time::Instant;

use sqlx::PgPool;

/// Type alias for the database row returned when loading live cells
/// Fields: (tx_hash, output_index, created_at_block, capacity, lock_script_hash, lock_code_hash, lock_args, type_script_hash, type_code_hash, data_size)
type LiveCellRow = (
    Vec<u8>,
    i16,
    i64,
    i64,
    Vec<u8>,
    Vec<u8>,
    Vec<u8>,
    Option<Vec<u8>>,
    Option<Vec<u8>>,
    i32,
);

/// Information about a live cell stored in memory
#[derive(Debug, Clone)]
pub struct LiveCellInfo {
    pub capacity: i64,
    pub created_at_block: i64,
    pub lock_script_hash: Vec<u8>,
    pub lock_code_hash: Vec<u8>,
    pub lock_args: Vec<u8>,
    pub type_script_hash: Option<Vec<u8>>,
    pub type_code_hash: Option<Vec<u8>>,
    pub data_size: i32,
}

/// Record of a consumed cell for potential rollback
#[derive(Debug, Clone)]
pub struct ConsumedCellRecord {
    pub tx_hash: Vec<u8>,
    pub output_index: i16,
    pub info: LiveCellInfo,
    pub consumed_at_block: i64,
}

impl LiveCellInfo {
    /// Estimate memory size of this cell info in bytes
    pub fn memory_size(&self) -> usize {
        let mut size = 0;
        size += 8;
        size += 8;
        size += 24 + self.lock_script_hash.len();
        size += 24 + self.lock_code_hash.len();
        size += 24 + self.lock_args.len();
        size += 24 + self.type_script_hash.as_ref().map(|v| v.len()).unwrap_or(0);
        size += 24 + self.type_code_hash.as_ref().map(|v| v.len()).unwrap_or(0);
        size += 4;
        size
    }
}

/// In-memory storage for live cell state with O(1) operations
///
/// Uses a HashMap keyed by (tx_hash, output_index) for fast lookups.
/// Thread-safe via RwLock for concurrent read/write access.
pub struct LiveCellStore {
    cells: RwLock<HashMap<(Vec<u8>, i16), LiveCellInfo>>,
    consumed_history: RwLock<VecDeque<ConsumedCellRecord>>,
    max_memory_bytes: usize,
    max_history_blocks: i64,
    /// Cells inserted since last flush (for durability)
    dirty_inserts: RwLock<HashMap<(Vec<u8>, i16), LiveCellInfo>>,
    /// Cells removed since last flush (for durability)
    dirty_removals: RwLock<HashSet<(Vec<u8>, i16)>>,
}

impl LiveCellStore {
    /// Create a new LiveCellStore with specified memory limit
    pub fn new(max_memory_bytes: usize) -> Self {
        let cells = RwLock::new(HashMap::with_capacity(50_000_000));
        let consumed_history = RwLock::new(VecDeque::new());
        let dirty_inserts = RwLock::new(HashMap::new());
        let dirty_removals = RwLock::new(HashSet::new());
        Self {
            cells,
            consumed_history,
            max_memory_bytes,
            max_history_blocks: 36,
            dirty_inserts,
            dirty_removals,
        }
    }

    /// Create a new LiveCellStore with default 8GB memory limit
    pub fn with_default_limit() -> Self {
        Self::new(8 * 1024 * 1024 * 1024)
    }

    /// Insert a live cell into the store
    pub fn insert(&self, tx_hash: Vec<u8>, output_index: i16, info: LiveCellInfo) {
        let key = (tx_hash.clone(), output_index);
        {
            let mut cells = self.cells.write().unwrap();
            cells.insert(key.clone(), info.clone());
        }
        {
            let mut dirty_inserts = self.dirty_inserts.write().unwrap();
            dirty_inserts.insert(key, info);
        }
    }

    /// Get a live cell from the store
    pub fn get(&self, tx_hash: &[u8], output_index: i16) -> Option<LiveCellInfo> {
        let cells = self.cells.read().unwrap();
        cells.get(&(tx_hash.to_vec(), output_index)).cloned()
    }

    /// Remove a live cell from the store (when consumed)
    pub fn remove(&self, tx_hash: &[u8], output_index: i16) -> Option<LiveCellInfo> {
        let key = (tx_hash.to_vec(), output_index);
        let result = {
            let mut cells = self.cells.write().unwrap();
            cells.remove(&key)
        };
        if result.is_some() {
            let mut dirty_inserts = self.dirty_inserts.write().unwrap();
            dirty_inserts.remove(&key);

            let mut dirty_removals = self.dirty_removals.write().unwrap();
            dirty_removals.insert(key);
        }
        result
    }

    /// Record a cell consumption for potential rollback
    pub fn record_consumption(
        &self,
        tx_hash: Vec<u8>,
        output_index: i16,
        info: LiveCellInfo,
        consumed_at_block: i64,
    ) {
        let record = ConsumedCellRecord {
            tx_hash,
            output_index,
            info,
            consumed_at_block,
        };

        let mut history = self.consumed_history.write().unwrap();
        history.push_back(record);

        while let Some(oldest) = history.front() {
            if consumed_at_block - oldest.consumed_at_block > self.max_history_blocks {
                history.pop_front();
            } else {
                break;
            }
        }
    }

    /// Get multiple cells in a single operation
    pub fn get_batch(&self, outpoints: &[(&[u8], i16)]) -> HashMap<(Vec<u8>, i16), LiveCellInfo> {
        let cells = self.cells.read().unwrap();
        let mut result = HashMap::with_capacity(outpoints.len());

        for (tx_hash, output_index) in outpoints {
            if let Some(info) = cells.get(&(tx_hash.to_vec(), *output_index)) {
                result.insert((tx_hash.to_vec(), *output_index), info.clone());
            }
        }

        result
    }

    /// Get the number of live cells in the store
    pub fn len(&self) -> usize {
        let cells = self.cells.read().unwrap();
        cells.len()
    }

    /// Check if the store is empty
    pub fn is_empty(&self) -> bool {
        let cells = self.cells.read().unwrap();
        cells.is_empty()
    }

    /// Estimate current memory usage in bytes
    pub fn memory_usage(&self) -> usize {
        let cells = self.cells.read().unwrap();
        let mut total = 0;

        total += cells.capacity() * 48;

        for ((tx_hash, _), info) in cells.iter() {
            total += 24 + tx_hash.len();
            total += 2;
            total += info.memory_size();
        }

        total
    }

    /// Clear all cells from the store
    pub fn clear(&self) {
        let mut cells = self.cells.write().unwrap();
        cells.clear();
    }

    /// Get the maximum memory limit
    pub fn max_memory_bytes(&self) -> usize {
        self.max_memory_bytes
    }

    /// Check if memory pressure is detected (usage >= limit)
    pub fn is_memory_pressure(&self) -> bool {
        self.memory_usage() >= self.max_memory_bytes
    }

    /// Get memory usage as a percentage of the limit (0-100+)
    pub fn memory_usage_percent(&self) -> f64 {
        let usage = self.memory_usage() as f64;
        let limit = self.max_memory_bytes as f64;
        (usage / limit) * 100.0
    }

    /// Check memory and log warnings/errors if pressure detected
    /// Returns true if critical pressure (>= 90%), false otherwise
    pub fn check_memory_and_warn(&self) -> bool {
        let percent = self.memory_usage_percent();

        if percent >= 90.0 {
            tracing::error!(
                "CRITICAL: LiveCellStore memory pressure at {:.1}% ({} / {} bytes)",
                percent,
                self.memory_usage(),
                self.max_memory_bytes
            );
            true
        } else if percent >= 75.0 {
            tracing::warn!(
                "LiveCellStore memory pressure at {:.1}% ({} / {} bytes)",
                percent,
                self.memory_usage(),
                self.max_memory_bytes
            );
            false
        } else {
            false
        }
    }

    /// Get the memory limit in bytes
    pub fn memory_limit(&self) -> usize {
        self.max_memory_bytes
    }

    /// Rollback the store to a specific block number
    ///
    /// Returns (removed_count, restored_count)
    pub fn rollback_to_block(&self, rollback_to: i64) -> (usize, usize) {
        let mut removed = 0;
        let mut restored = 0;

        {
            let mut cells = self.cells.write().unwrap();
            let to_remove: Vec<_> = cells
                .iter()
                .filter(|(_, info)| info.created_at_block > rollback_to)
                .map(|(k, _)| k.clone())
                .collect();
            for key in to_remove {
                cells.remove(&key);
                removed += 1;
            }
        }

        {
            let history = self.consumed_history.read().unwrap();
            let to_restore: Vec<_> = history
                .iter()
                .filter(|r| r.consumed_at_block > rollback_to)
                .cloned()
                .collect();
            drop(history);

            for record in to_restore {
                self.insert(record.tx_hash, record.output_index, record.info);
                restored += 1;
            }
        }

        {
            let mut history = self.consumed_history.write().unwrap();
            history.retain(|r| r.consumed_at_block <= rollback_to);
        }

        (removed, restored)
    }

    /// Get cells created since a specific block number
    pub fn cells_created_since(&self, block_number: i64) -> Vec<(Vec<u8>, i16, LiveCellInfo)> {
        let cells = self.cells.read().unwrap();
        cells
            .iter()
            .filter(|(_, info)| info.created_at_block > block_number)
            .map(|((tx_hash, output_index), info)| (tx_hash.clone(), *output_index, info.clone()))
            .collect()
    }

    /// Flush dirty cells to database for durability
    ///
    /// Returns (insert_count, removal_count)
    pub async fn flush_to_db(&self, pool: &PgPool) -> anyhow::Result<(usize, usize)> {
        let (inserts, removals) = {
            let mut dirty_inserts = self.dirty_inserts.write().unwrap();
            let mut dirty_removals = self.dirty_removals.write().unwrap();
            let inserts = std::mem::take(&mut *dirty_inserts);
            let removals = std::mem::take(&mut *dirty_removals);
            (inserts, removals)
        };

        let insert_count = inserts.len();
        let removal_count = removals.len();

        if insert_count == 0 && removal_count == 0 {
            return Ok((0, 0));
        }

        if insert_count > 0 {
            let mut query_builder = sqlx::QueryBuilder::new(
                "INSERT INTO live_cells (tx_hash, output_index, created_at_block, capacity, lock_script_hash, lock_code_hash, lock_args, type_script_hash, type_code_hash, data_size) "
            );

            query_builder.push_values(inserts.iter(), |mut b, ((tx_hash, output_index), info)| {
                b.push_bind(tx_hash)
                    .push_bind(output_index)
                    .push_bind(info.created_at_block)
                    .push_bind(info.capacity)
                    .push_bind(&info.lock_script_hash)
                    .push_bind(&info.lock_code_hash)
                    .push_bind(&info.lock_args)
                    .push_bind(&info.type_script_hash)
                    .push_bind(&info.type_code_hash)
                    .push_bind(info.data_size);
            });

            query_builder.push(" ON CONFLICT (tx_hash, output_index) DO NOTHING");
            query_builder.build().execute(pool).await?;
        }

        if removal_count > 0 {
            let batch_size = 1000;
            let removal_vec: Vec<_> = removals.into_iter().collect();

            for chunk in removal_vec.chunks(batch_size) {
                let mut query_builder = sqlx::QueryBuilder::new("DELETE FROM live_cells WHERE ");

                let mut first = true;
                for (tx_hash, output_index) in chunk {
                    if !first {
                        query_builder.push(" OR ");
                    }
                    first = false;
                    query_builder.push("(tx_hash = ");
                    query_builder.push_bind(tx_hash);
                    query_builder.push(" AND output_index = ");
                    query_builder.push_bind(output_index);
                    query_builder.push(")");
                }

                query_builder.build().execute(pool).await?;
            }
        }

        tracing::info!(
            "LiveCellStore flush: {} inserts, {} removals",
            insert_count,
            removal_count
        );

        Ok((insert_count, removal_count))
    }

    /// Rebuild the store from the database during startup recovery
    ///
    /// Loads all live cells from the database in batches to avoid memory spikes.
    /// Logs progress every 1M cells and total rebuild time at completion.
    pub async fn rebuild_from_db(&self, pool: &PgPool) -> anyhow::Result<()> {
        let start_time = Instant::now();
        let batch_size: i64 = 100_000;
        let mut offset: i64 = 0;
        let mut total_loaded: i64 = 0;

        tracing::info!("Starting LiveCellStore rebuild from database");

        // Clear existing data before rebuild
        self.clear();

        loop {
            let rows: Vec<LiveCellRow> = sqlx::query_as(
                r#"SELECT tx_hash, output_index, created_at_block, capacity,
                          lock_script_hash, lock_code_hash, lock_args,
                          type_script_hash, type_code_hash, data_size
                   FROM live_cells
                   ORDER BY tx_hash, output_index
                   LIMIT $1 OFFSET $2"#,
            )
            .bind(batch_size)
            .bind(offset)
            .fetch_all(pool)
            .await?;

            if rows.is_empty() {
                break;
            }

            for (
                tx_hash,
                output_index,
                created_at_block,
                capacity,
                lock_script_hash,
                lock_code_hash,
                lock_args,
                type_script_hash,
                type_code_hash,
                data_size,
            ) in rows
            {
                let info = LiveCellInfo {
                    capacity,
                    created_at_block,
                    lock_script_hash,
                    lock_code_hash,
                    lock_args,
                    type_script_hash,
                    type_code_hash,
                    data_size,
                };
                self.insert(tx_hash, output_index, info);
            }

            total_loaded += batch_size;

            // Log progress every 1M cells
            if total_loaded % 1_000_000 == 0 {
                let memory_usage = self.memory_usage();
                let memory_percent = self.memory_usage_percent();
                tracing::info!(
                    "LiveCellStore rebuild progress: {} cells loaded, memory: {:.1}% ({} MB)",
                    total_loaded,
                    memory_percent,
                    memory_usage / (1024 * 1024)
                );
            }

            offset += batch_size;
        }

        let elapsed = start_time.elapsed();
        let final_count = self.len();
        let final_memory = self.memory_usage();
        let final_memory_percent = self.memory_usage_percent();

        tracing::info!(
            "LiveCellStore rebuild completed in {:.2}s: {} cells loaded, memory: {:.1}% ({} MB)",
            elapsed.as_secs_f64(),
            final_count,
            final_memory_percent,
            final_memory / (1024 * 1024)
        );

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_cell_info() -> LiveCellInfo {
        LiveCellInfo {
            capacity: 10000000000,
            created_at_block: 12345,
            lock_script_hash: vec![1u8; 32],
            lock_code_hash: vec![2u8; 32],
            lock_args: vec![3u8; 20],
            type_script_hash: Some(vec![4u8; 32]),
            type_code_hash: Some(vec![5u8; 32]),
            data_size: 100,
        }
    }

    #[test]
    fn test_new_store() {
        let store = LiveCellStore::new(1024 * 1024 * 1024);
        assert_eq!(store.len(), 0);
        assert!(store.is_empty());
        assert_eq!(store.max_memory_bytes(), 1024 * 1024 * 1024);
    }

    #[test]
    fn test_with_default_limit() {
        let store = LiveCellStore::with_default_limit();
        assert_eq!(store.max_memory_bytes(), 8 * 1024 * 1024 * 1024);
    }

    #[test]
    fn test_insert_and_get() {
        let store = LiveCellStore::new(1024 * 1024 * 1024);
        let tx_hash = vec![0xabu8; 32];
        let output_index = 0;
        let info = create_test_cell_info();

        store.insert(tx_hash.clone(), output_index, info.clone());

        let retrieved = store.get(&tx_hash, output_index);
        assert!(retrieved.is_some());
        let retrieved = retrieved.unwrap();
        assert_eq!(retrieved.capacity, info.capacity);
        assert_eq!(retrieved.created_at_block, info.created_at_block);
        assert_eq!(retrieved.lock_script_hash, info.lock_script_hash);
        assert_eq!(store.len(), 1);
    }

    #[test]
    fn test_remove() {
        let store = LiveCellStore::new(1024 * 1024 * 1024);
        let tx_hash = vec![0xabu8; 32];
        let output_index = 0;
        let info = create_test_cell_info();

        store.insert(tx_hash.clone(), output_index, info.clone());
        assert_eq!(store.len(), 1);

        let removed = store.remove(&tx_hash, output_index);
        assert!(removed.is_some());
        assert_eq!(store.len(), 0);

        let retrieved = store.get(&tx_hash, output_index);
        assert!(retrieved.is_none());
    }

    #[test]
    fn test_get_batch() {
        let store = LiveCellStore::new(1024 * 1024 * 1024);

        let tx_hash1 = vec![0x11u8; 32];
        let tx_hash2 = vec![0x22u8; 32];
        let tx_hash3 = vec![0x33u8; 32];
        let tx_hash_nonexistent = vec![0xffu8; 32];

        store.insert(tx_hash1.clone(), 0, create_test_cell_info());
        store.insert(tx_hash2.clone(), 1, create_test_cell_info());
        store.insert(tx_hash3.clone(), 2, create_test_cell_info());

        let outpoints = vec![
            (tx_hash1.as_slice(), 0),
            (tx_hash2.as_slice(), 1),
            (tx_hash_nonexistent.as_slice(), 99),
        ];

        let result = store.get_batch(&outpoints);
        assert_eq!(result.len(), 2);
        assert!(result.contains_key(&(tx_hash1.clone(), 0)));
        assert!(result.contains_key(&(tx_hash2.clone(), 1)));
        assert!(!result.contains_key(&(tx_hash_nonexistent, 99)));
    }

    #[test]
    fn test_memory_usage() {
        let store = LiveCellStore::new(1024 * 1024 * 1024);
        let initial_usage = store.memory_usage();

        let tx_hash = vec![0xabu8; 32];
        let info = create_test_cell_info();
        store.insert(tx_hash.clone(), 0, info);

        let after_insert = store.memory_usage();
        assert!(after_insert > initial_usage);
    }

    #[test]
    fn test_clear() {
        let store = LiveCellStore::new(1024 * 1024 * 1024);

        for i in 0..10 {
            let tx_hash = vec![i as u8; 32];
            store.insert(tx_hash, i as i16, create_test_cell_info());
        }

        assert_eq!(store.len(), 10);

        store.clear();
        assert_eq!(store.len(), 0);
        assert!(store.is_empty());
    }

    #[test]
    fn test_live_cell_info_memory_size() {
        let info = LiveCellInfo {
            capacity: 10000000000,
            created_at_block: 12345,
            lock_script_hash: vec![1u8; 32],
            lock_code_hash: vec![2u8; 32],
            lock_args: vec![3u8; 20],
            type_script_hash: Some(vec![4u8; 32]),
            type_code_hash: Some(vec![5u8; 32]),
            data_size: 100,
        };

        let size = info.memory_size();
        assert!(size > 200 && size < 400);
    }

    #[test]
    fn test_thread_safety() {
        use std::sync::Arc;
        use std::thread;

        let store = Arc::new(LiveCellStore::new(1024 * 1024 * 1024));
        let mut handles = vec![];

        for i in 0..10 {
            let store_clone = Arc::clone(&store);
            let handle = thread::spawn(move || {
                for j in 0..100 {
                    let tx_hash = vec![i as u8; 32];
                    let info = create_test_cell_info();
                    store_clone.insert(tx_hash, j as i16, info);
                }
            });
            handles.push(handle);
        }

        for i in 0..10 {
            let store_clone = Arc::clone(&store);
            let handle = thread::spawn(move || {
                for j in 0..100 {
                    let tx_hash = vec![i as u8; 32];
                    let _ = store_clone.get(&tx_hash, j as i16);
                }
            });
            handles.push(handle);
        }

        for handle in handles {
            handle.join().unwrap();
        }

        assert_eq!(store.len(), 1000);
    }

    #[test]
    fn test_overwrite_existing_cell() {
        let store = LiveCellStore::new(1024 * 1024 * 1024);
        let tx_hash = vec![0xabu8; 32];
        let output_index = 0;

        let info1 = LiveCellInfo {
            capacity: 100,
            created_at_block: 1,
            lock_script_hash: vec![1u8; 32],
            lock_code_hash: vec![2u8; 32],
            lock_args: vec![3u8; 20],
            type_script_hash: None,
            type_code_hash: None,
            data_size: 10,
        };

        let info2 = LiveCellInfo {
            capacity: 200,
            created_at_block: 2,
            lock_script_hash: vec![4u8; 32],
            lock_code_hash: vec![5u8; 32],
            lock_args: vec![6u8; 20],
            type_script_hash: Some(vec![7u8; 32]),
            type_code_hash: Some(vec![8u8; 32]),
            data_size: 20,
        };

        store.insert(tx_hash.clone(), output_index, info1);
        assert_eq!(store.len(), 1);

        store.insert(tx_hash.clone(), output_index, info2.clone());
        assert_eq!(store.len(), 1);

        let retrieved = store.get(&tx_hash, output_index).unwrap();
        assert_eq!(retrieved.capacity, 200);
        assert_eq!(retrieved.created_at_block, 2);
    }

    #[test]
    fn test_is_memory_pressure() {
        let store = LiveCellStore::new(10 * 1024 * 1024 * 1024);
        assert!(!store.is_memory_pressure());

        let tx_hash = vec![0xabu8; 32];
        let info = create_test_cell_info();
        store.insert(tx_hash.clone(), 0, info);

        assert!(!store.is_memory_pressure());
    }

    #[test]
    fn test_memory_usage_percent() {
        let store = LiveCellStore::new(10 * 1024 * 1024 * 1024);
        let percent = store.memory_usage_percent();
        assert!(percent >= 0.0);

        let tx_hash = vec![0xabu8; 32];
        let info = create_test_cell_info();
        store.insert(tx_hash, 0, info);

        let percent_after = store.memory_usage_percent();
        assert!(percent_after > percent);
    }

    #[test]
    fn test_memory_limit_getter() {
        let limit = 2 * 1024 * 1024 * 1024;
        let store = LiveCellStore::new(limit);
        assert_eq!(store.memory_limit(), limit);
        assert_eq!(store.max_memory_bytes(), limit);
    }

    #[test]
    fn test_check_memory_and_warn() {
        let store = LiveCellStore::new(10 * 1024 * 1024 * 1024);
        let is_critical = store.check_memory_and_warn();
        assert!(!is_critical);
    }

    #[test]
    fn test_record_consumption() {
        let store = LiveCellStore::new(1024 * 1024 * 1024);
        let tx_hash = vec![0xabu8; 32];
        let output_index = 0;
        let info = create_test_cell_info();

        store.record_consumption(tx_hash.clone(), output_index, info.clone(), 100);

        let history = store.consumed_history.read().unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].tx_hash, tx_hash);
        assert_eq!(history[0].output_index, output_index);
        assert_eq!(history[0].consumed_at_block, 100);
    }

    #[test]
    fn test_record_consumption_prunes_old_history() {
        let store = LiveCellStore::new(1024 * 1024 * 1024);

        for i in 0..50 {
            let tx_hash = vec![i as u8; 32];
            let info = create_test_cell_info();
            store.record_consumption(tx_hash, 0, info, i as i64);
        }

        let history = store.consumed_history.read().unwrap();
        assert!(history.len() <= 37);

        if let Some(oldest) = history.front() {
            assert!(49 - oldest.consumed_at_block <= 36);
        }
    }

    #[test]
    fn test_rollback_to_block_removes_new_cells() {
        let store = LiveCellStore::new(1024 * 1024 * 1024);

        let tx1 = vec![0x11u8; 32];
        let tx2 = vec![0x22u8; 32];
        let tx3 = vec![0x33u8; 32];

        let mut info1 = create_test_cell_info();
        info1.created_at_block = 99;
        let mut info2 = create_test_cell_info();
        info2.created_at_block = 100;
        let mut info3 = create_test_cell_info();
        info3.created_at_block = 101;

        store.insert(tx1.clone(), 0, info1);
        store.insert(tx2.clone(), 0, info2);
        store.insert(tx3.clone(), 0, info3);

        assert_eq!(store.len(), 3);

        let (removed, restored) = store.rollback_to_block(100);

        assert_eq!(removed, 1);
        assert_eq!(restored, 0);
        assert_eq!(store.len(), 2);

        assert!(store.get(&tx1, 0).is_some());
        assert!(store.get(&tx2, 0).is_some());
        assert!(store.get(&tx3, 0).is_none());
    }

    #[test]
    fn test_rollback_to_block_restores_consumed_cells() {
        let store = LiveCellStore::new(1024 * 1024 * 1024);

        let tx1 = vec![0x11u8; 32];
        let tx2 = vec![0x22u8; 32];

        let mut info1 = create_test_cell_info();
        info1.created_at_block = 99;
        let mut info2 = create_test_cell_info();
        info2.created_at_block = 99;

        store.record_consumption(tx1.clone(), 0, info1.clone(), 100);
        store.record_consumption(tx2.clone(), 0, info2.clone(), 101);

        assert_eq!(store.len(), 0);

        let (removed, restored) = store.rollback_to_block(100);

        assert_eq!(removed, 0);
        assert_eq!(restored, 1);
        assert_eq!(store.len(), 1);

        assert!(store.get(&tx1, 0).is_none());
        assert!(store.get(&tx2, 0).is_some());
    }

    #[test]
    fn test_rollback_to_block_combined() {
        let store = LiveCellStore::new(1024 * 1024 * 1024);

        let tx1 = vec![0x11u8; 32];
        let tx2 = vec![0x22u8; 32];
        let tx3 = vec![0x33u8; 32];
        let tx4 = vec![0x44u8; 32];

        let mut info1 = create_test_cell_info();
        info1.created_at_block = 99;
        let mut info2 = create_test_cell_info();
        info2.created_at_block = 100;
        let mut info3 = create_test_cell_info();
        info3.created_at_block = 101;
        let mut info4 = create_test_cell_info();
        info4.created_at_block = 99;

        store.insert(tx1.clone(), 0, info1);
        store.insert(tx2.clone(), 0, info2);
        store.insert(tx3.clone(), 0, info3);

        store.record_consumption(tx4.clone(), 0, info4.clone(), 101);

        assert_eq!(store.len(), 3);

        let (removed, restored) = store.rollback_to_block(100);

        assert_eq!(removed, 1);
        assert_eq!(restored, 1);
        assert_eq!(store.len(), 3);

        assert!(store.get(&tx1, 0).is_some());
        assert!(store.get(&tx2, 0).is_some());
        assert!(store.get(&tx3, 0).is_none());
        assert!(store.get(&tx4, 0).is_some());
    }

    #[test]
    fn test_rollback_to_block_cleans_history() {
        let store = LiveCellStore::new(1024 * 1024 * 1024);

        let tx1 = vec![0x11u8; 32];
        let tx2 = vec![0x22u8; 32];
        let tx3 = vec![0x33u8; 32];

        let info = create_test_cell_info();

        store.record_consumption(tx1.clone(), 0, info.clone(), 99);
        store.record_consumption(tx2.clone(), 0, info.clone(), 100);
        store.record_consumption(tx3.clone(), 0, info.clone(), 101);

        {
            let history = store.consumed_history.read().unwrap();
            assert_eq!(history.len(), 3);
        }

        store.rollback_to_block(100);

        {
            let history = store.consumed_history.read().unwrap();
            assert_eq!(history.len(), 2);
            assert!(history.iter().all(|r| r.consumed_at_block <= 100));
        }
    }

    #[test]
    fn test_cells_created_since() {
        let store = LiveCellStore::new(1024 * 1024 * 1024);

        let tx1 = vec![0x11u8; 32];
        let tx2 = vec![0x22u8; 32];
        let tx3 = vec![0x33u8; 32];

        let mut info1 = create_test_cell_info();
        info1.created_at_block = 99;
        let mut info2 = create_test_cell_info();
        info2.created_at_block = 100;
        let mut info3 = create_test_cell_info();
        info3.created_at_block = 101;

        store.insert(tx1.clone(), 0, info1);
        store.insert(tx2.clone(), 0, info2);
        store.insert(tx3.clone(), 0, info3);

        let cells = store.cells_created_since(100);
        assert_eq!(cells.len(), 1);
        assert_eq!(cells[0].0, tx3);
        assert_eq!(cells[0].1, 0);
        assert_eq!(cells[0].2.created_at_block, 101);
    }

    #[test]
    fn test_rollback_to_block_no_changes() {
        let store = LiveCellStore::new(1024 * 1024 * 1024);

        let tx1 = vec![0x11u8; 32];
        let mut info1 = create_test_cell_info();
        info1.created_at_block = 99;

        store.insert(tx1.clone(), 0, info1);

        let (removed, restored) = store.rollback_to_block(100);

        assert_eq!(removed, 0);
        assert_eq!(restored, 0);
        assert_eq!(store.len(), 1);
    }

    #[tokio::test]
    async fn test_rebuild_from_db() {
        use sqlx::postgres::PgPoolOptions;

        // This test requires a database connection
        // Skip if DATABASE_URL is not set
        let database_url = match std::env::var("DATABASE_URL") {
            Ok(url) => url,
            Err(_) => {
                eprintln!("Skipping test_rebuild_from_db: DATABASE_URL not set");
                return;
            }
        };

        let pool = match PgPoolOptions::new()
            .max_connections(1)
            .connect(&database_url)
            .await
        {
            Ok(p) => p,
            Err(_) => {
                eprintln!("Skipping test_rebuild_from_db: Could not connect to database");
                return;
            }
        };

        let store = LiveCellStore::new(1024 * 1024 * 1024);

        // Insert test data into live_cells table
        let tx_hash = vec![0xabu8; 32];
        let output_index: i16 = 0;
        let created_at_block: i64 = 12345;
        let capacity: i64 = 10000000000;
        let lock_script_hash = vec![1u8; 32];
        let lock_code_hash = vec![2u8; 32];
        let lock_args = vec![3u8; 20];
        let type_script_hash: Option<Vec<u8>> = Some(vec![4u8; 32]);
        let type_code_hash: Option<Vec<u8>> = Some(vec![5u8; 32]);
        let data_size: i32 = 100;

        // Clean up any existing test data
        let _ = sqlx::query("DELETE FROM live_cells WHERE tx_hash = $1 AND output_index = $2")
            .bind(&tx_hash)
            .bind(output_index)
            .execute(&pool)
            .await;

        // Insert test cell
        sqlx::query(
            r#"INSERT INTO live_cells (tx_hash, output_index, created_at_block, capacity,
                                       lock_script_hash, lock_code_hash, lock_args,
                                       type_script_hash, type_code_hash, data_size)
               VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
               ON CONFLICT (tx_hash, output_index) DO NOTHING"#,
        )
        .bind(&tx_hash)
        .bind(output_index)
        .bind(created_at_block)
        .bind(capacity)
        .bind(&lock_script_hash)
        .bind(&lock_code_hash)
        .bind(&lock_args)
        .bind(&type_script_hash)
        .bind(&type_code_hash)
        .bind(data_size)
        .execute(&pool)
        .await
        .expect("Failed to insert test cell");

        // Rebuild from database
        store
            .rebuild_from_db(&pool)
            .await
            .expect("Failed to rebuild from database");

        // Verify the cell was loaded
        assert_eq!(store.len(), 1);
        let retrieved = store.get(&tx_hash, output_index);
        assert!(retrieved.is_some());

        let cell = retrieved.unwrap();
        assert_eq!(cell.capacity, capacity);
        assert_eq!(cell.created_at_block, created_at_block);
        assert_eq!(cell.lock_script_hash, lock_script_hash);
        assert_eq!(cell.lock_code_hash, lock_code_hash);
        assert_eq!(cell.lock_args, lock_args);
        assert_eq!(cell.type_script_hash, type_script_hash);
        assert_eq!(cell.type_code_hash, type_code_hash);
        assert_eq!(cell.data_size, data_size);

        // Clean up
        let _ = sqlx::query("DELETE FROM live_cells WHERE tx_hash = $1 AND output_index = $2")
            .bind(&tx_hash)
            .bind(output_index)
            .execute(&pool)
            .await;
    }
}
