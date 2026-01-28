use std::collections::HashMap;
use std::sync::RwLock;

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
    max_memory_bytes: usize,
}

impl LiveCellStore {
    /// Create a new LiveCellStore with specified memory limit
    pub fn new(max_memory_bytes: usize) -> Self {
        let cells = RwLock::new(HashMap::with_capacity(50_000_000));
        Self {
            cells,
            max_memory_bytes,
        }
    }

    /// Create a new LiveCellStore with default 8GB memory limit
    pub fn with_default_limit() -> Self {
        Self::new(8 * 1024 * 1024 * 1024)
    }

    /// Insert a live cell into the store
    pub fn insert(&self, tx_hash: Vec<u8>, output_index: i16, info: LiveCellInfo) {
        let mut cells = self.cells.write().unwrap();
        cells.insert((tx_hash, output_index), info);
    }

    /// Get a live cell from the store
    pub fn get(&self, tx_hash: &[u8], output_index: i16) -> Option<LiveCellInfo> {
        let cells = self.cells.read().unwrap();
        cells.get(&(tx_hash.to_vec(), output_index)).cloned()
    }

    /// Remove a live cell from the store (when consumed)
    pub fn remove(&self, tx_hash: &[u8], output_index: i16) -> Option<LiveCellInfo> {
        let mut cells = self.cells.write().unwrap();
        cells.remove(&(tx_hash.to_vec(), output_index))
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
}
