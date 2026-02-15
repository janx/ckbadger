//! Key encoding for RocksDB column families.
//!
//! All numeric keys use big-endian encoding for correct lexicographic sort order.
//! Fixed-size fields with no delimiters needed.

/// Outpoint key: tx_hash(32B) + output_index(2B BE) = 34 bytes
pub const OUTPOINT_KEY_SIZE: usize = 34;

/// Block number key: 8 bytes big-endian i64
pub const BLOCK_NUM_KEY_SIZE: usize = 8;

pub fn encode_outpoint(tx_hash: &[u8], output_index: i16) -> [u8; OUTPOINT_KEY_SIZE] {
    let mut key = [0u8; OUTPOINT_KEY_SIZE];
    key[..32].copy_from_slice(&tx_hash[..32]);
    key[32..34].copy_from_slice(&output_index.to_be_bytes());
    key
}

pub fn decode_outpoint(key: &[u8]) -> (Vec<u8>, i16) {
    let tx_hash = key[..32].to_vec();
    let output_index = i16::from_be_bytes([key[32], key[33]]);
    (tx_hash, output_index)
}

pub fn encode_block_num(n: i64) -> [u8; BLOCK_NUM_KEY_SIZE] {
    n.to_be_bytes()
}

pub fn decode_block_num(key: &[u8]) -> i64 {
    i64::from_be_bytes(key[..8].try_into().unwrap_or([0; 8]))
}

pub fn encode_tx_idx(idx: i32) -> [u8; 4] {
    idx.to_be_bytes()
}

pub fn decode_tx_idx(key: &[u8]) -> i32 {
    i32::from_be_bytes(key[..4].try_into().unwrap_or([0; 4]))
}

/// Encode composite key from multiple parts concatenated.
pub fn encode_composite(parts: &[&[u8]]) -> Vec<u8> {
    let total: usize = parts.iter().map(|p| p.len()).sum();
    let mut key = Vec::with_capacity(total);
    for part in parts {
        key.extend_from_slice(part);
    }
    key
}

/// Encode a cell-by-lock/type index key:
/// lock_hash(32B) + block_num(8B BE) + outpoint(34B) = 74 bytes
pub fn encode_cell_index_key(
    script_hash: &[u8],
    block_num: i64,
    tx_hash: &[u8],
    output_index: i16,
) -> Vec<u8> {
    let mut key = Vec::with_capacity(74);
    key.extend_from_slice(&script_hash[..32]);
    key.extend_from_slice(&block_num.to_be_bytes());
    key.extend_from_slice(&tx_hash[..32]);
    key.extend_from_slice(&output_index.to_be_bytes());
    key
}

/// Encode an address-tx index key:
/// lock_hash(32B) + block_num(8B BE) + tx_idx(4B BE) = 44 bytes
pub fn encode_addr_tx_key(lock_hash: &[u8], block_num: i64, tx_idx: i32) -> Vec<u8> {
    let mut key = Vec::with_capacity(44);
    key.extend_from_slice(&lock_hash[..32]);
    key.extend_from_slice(&block_num.to_be_bytes());
    key.extend_from_slice(&tx_idx.to_be_bytes());
    key
}

/// Encode a token_holders key: type_hash(32B) + lock_hash(32B) = 64 bytes
pub fn encode_token_holder_key(type_hash: &[u8], lock_hash: &[u8]) -> [u8; 64] {
    let mut key = [0u8; 64];
    key[..32].copy_from_slice(&type_hash[..32]);
    key[32..64].copy_from_slice(&lock_hash[..32]);
    key
}

/// Encode task index key: status(1B) + priority_desc(2B BE) + id(16B) = 19 bytes
pub fn encode_task_index_key(status: u8, priority: i32, id: &uuid::Uuid) -> Vec<u8> {
    let priority_desc = (i32::MAX - priority) as u16;
    let mut key = Vec::with_capacity(19);
    key.push(status);
    key.extend_from_slice(&priority_desc.to_be_bytes());
    key.extend_from_slice(id.as_bytes());
    key
}

/// Stats key: prefix(1B) + variable key
pub fn encode_stats_key(prefix: u8, suffix: &[u8]) -> Vec<u8> {
    let mut key = Vec::with_capacity(1 + suffix.len());
    key.push(prefix);
    key.extend_from_slice(suffix);
    key
}

/// Stats key prefixes
pub mod stats_prefix {
    pub const DAILY: u8 = 0x01;
    pub const HOURLY: u8 = 0x02;
    pub const EPOCH: u8 = 0x03;
    pub const MINER: u8 = 0x04;
    pub const BLOCK_TIME_DIST: u8 = 0x05;
    pub const EPOCH_TIME_DIST: u8 = 0x06;
    pub const DAILY_BLOCK: u8 = 0x07;
    pub const DAO_DAILY_SNAPSHOT: u8 = 0x08;
    pub const TOKEN_TRANSFERS: u8 = 0x09;
    pub const TOKEN_HOURLY: u8 = 0x0A;
}

// Flat re-exports for convenience
pub const STATS_PREFIX_DAILY: u8 = stats_prefix::DAILY;
pub const STATS_PREFIX_HOURLY: u8 = stats_prefix::HOURLY;
pub const STATS_PREFIX_EPOCH: u8 = stats_prefix::EPOCH;
pub const STATS_PREFIX_MINER: u8 = stats_prefix::MINER;
pub const STATS_PREFIX_BLOCK_TIME_DIST: u8 = stats_prefix::BLOCK_TIME_DIST;
pub const STATS_PREFIX_EPOCH_TIME_DIST: u8 = stats_prefix::EPOCH_TIME_DIST;
pub const STATS_PREFIX_DAILY_BLOCK: u8 = stats_prefix::DAILY_BLOCK;
pub const STATS_PREFIX_DAO_DAILY_SNAPSHOT: u8 = stats_prefix::DAO_DAILY_SNAPSHOT;
pub const STATS_PREFIX_TOKEN_TRANSFERS: u8 = stats_prefix::TOKEN_TRANSFERS;
pub const STATS_PREFIX_TOKEN_HOURLY: u8 = stats_prefix::TOKEN_HOURLY;

/// Token transfers total count key: prefix(1B) + type_hash(32B) = 33 bytes
pub fn encode_token_transfers_key(type_hash: &[u8]) -> Vec<u8> {
    let mut key = Vec::with_capacity(33);
    key.push(STATS_PREFIX_TOKEN_TRANSFERS);
    key.extend_from_slice(&type_hash[..32]);
    key
}

/// Token hourly transfer count key: prefix(1B) + type_hash(32B) + hour_bucket(8B BE) = 41 bytes
/// hour_bucket = timestamp_ms / 3_600_000
pub fn encode_token_hourly_key(type_hash: &[u8], hour_bucket: i64) -> Vec<u8> {
    let mut key = Vec::with_capacity(41);
    key.push(STATS_PREFIX_TOKEN_HOURLY);
    key.extend_from_slice(&type_hash[..32]);
    key.extend_from_slice(&hour_bucket.to_be_bytes());
    key
}

/// Prefix for scanning all hourly buckets of a given token.
pub fn encode_token_hourly_prefix(type_hash: &[u8]) -> Vec<u8> {
    let mut key = Vec::with_capacity(33);
    key.push(STATS_PREFIX_TOKEN_HOURLY);
    key.extend_from_slice(&type_hash[..32]);
    key
}

/// Token transfer key: type_hash(32B) + block_num_desc(8B BE) + tx_idx(4B BE) = 44 bytes
/// Uses descending block_num so newest transfers come first in prefix scan.
pub fn encode_token_transfer_key(type_hash: &[u8], block_num: i64, tx_idx: i32) -> Vec<u8> {
    let block_desc = (i64::MAX - block_num).to_be_bytes();
    let mut key = Vec::with_capacity(44);
    key.extend_from_slice(&type_hash[..32]);
    key.extend_from_slice(&block_desc);
    key.extend_from_slice(&tx_idx.to_be_bytes());
    key
}

/// Decode block_num and tx_idx from a token transfer key.
pub fn decode_token_transfer_key(key: &[u8]) -> (i64, i32) {
    let block_desc = i64::from_be_bytes(key[32..40].try_into().unwrap());
    let block_num = i64::MAX - block_desc;
    let tx_idx = i32::from_be_bytes(key[40..44].try_into().unwrap());
    (block_num, tx_idx)
}

/// Spore-by-cluster key: cluster_id(32B) + spore_id(32B) = 64 bytes
pub fn encode_spore_by_cluster_key(cluster_id: &[u8], spore_id: &[u8]) -> [u8; 64] {
    let mut key = [0u8; 64];
    key[..32].copy_from_slice(&cluster_id[..32]);
    key[32..64].copy_from_slice(&spore_id[..32]);
    key
}

/// Activity key: lock_hash(32B) + block_num_desc(8B BE) + tx_idx(4B BE) = 44 bytes
/// Uses descending block_num so newest activities come first in prefix scan.
pub fn encode_activity_key(lock_hash: &[u8], block_num: i64, tx_idx: i32) -> Vec<u8> {
    assert!(
        lock_hash.len() >= 32,
        "encode_activity_key: lock_hash must be >= 32 bytes, got {}",
        lock_hash.len()
    );
    let block_desc = (i64::MAX - block_num).to_be_bytes();
    let mut key = Vec::with_capacity(44);
    key.extend_from_slice(&lock_hash[..32]);
    key.extend_from_slice(&block_desc);
    key.extend_from_slice(&tx_idx.to_be_bytes());
    key
}

/// Decode block_num and tx_idx from an activity key.
pub fn decode_activity_key(key: &[u8]) -> (Vec<u8>, i64, i32) {
    let lock_hash = key[..32].to_vec();
    let block_desc = i64::from_be_bytes(key[32..40].try_into().unwrap());
    let block_num = i64::MAX - block_desc;
    let tx_idx = i32::from_be_bytes(key[40..44].try_into().unwrap());
    (lock_hash, block_num, tx_idx)
}

/// Address daily stats key: lock_hash(32B) + date(4B u32 YYYYMMDD BE) = 36 bytes
pub const ADDR_DAILY_STATS_KEY_SIZE: usize = 36;

pub fn encode_addr_daily_stats_key(
    lock_hash: &[u8],
    date_yyyymmdd: u32,
) -> [u8; ADDR_DAILY_STATS_KEY_SIZE] {
    let mut key = [0u8; ADDR_DAILY_STATS_KEY_SIZE];
    key[..32].copy_from_slice(&lock_hash[..32]);
    key[32..36].copy_from_slice(&date_yyyymmdd.to_be_bytes());
    key
}

pub fn decode_addr_daily_stats_key(key: &[u8]) -> (Vec<u8>, u32) {
    let lock_hash = key[..32].to_vec();
    let date = u32::from_be_bytes(key[32..36].try_into().unwrap());
    (lock_hash, date)
}

/// Convert a Unix timestamp in milliseconds to YYYYMMDD u32.
pub fn timestamp_ms_to_date(timestamp_ms: i64) -> u32 {
    let secs = timestamp_ms / 1000;
    let dt = chrono::DateTime::from_timestamp(secs, 0).unwrap_or_default();
    let date = dt.format("%Y%m%d").to_string();
    date.parse::<u32>().unwrap_or(0)
}

/// Sync meta keys
pub mod sync_meta_keys {
    pub const TIP_BLOCK: &[u8] = b"tip_block";
    pub const SYNC_STATUS: &[u8] = b"sync_status";
    pub const DEEP_FORK: &[u8] = b"deep_fork";
    pub const REORG_EVENTS: &[u8] = b"reorg_events";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_outpoint_roundtrip() {
        let tx_hash = [42u8; 32];
        let output_index: i16 = 7;
        let key = encode_outpoint(&tx_hash, output_index);
        let (decoded_hash, decoded_idx) = decode_outpoint(&key);
        assert_eq!(decoded_hash, tx_hash.to_vec());
        assert_eq!(decoded_idx, output_index);
    }

    #[test]
    fn test_block_num_sort_order() {
        let k1 = encode_block_num(100);
        let k2 = encode_block_num(200);
        let k3 = encode_block_num(300);
        assert!(k1 < k2);
        assert!(k2 < k3);
    }

    #[test]
    fn test_block_num_roundtrip() {
        for n in [0i64, 1, 100, 1_000_000, i64::MAX] {
            assert_eq!(decode_block_num(&encode_block_num(n)), n);
        }
    }

    #[test]
    fn test_composite_key() {
        let hash = [1u8; 32];
        let block = encode_block_num(42);
        let key = encode_composite(&[&hash, &block]);
        assert_eq!(key.len(), 40);
        assert_eq!(&key[..32], &hash);
        assert_eq!(decode_block_num(&key[32..40]), 42);
    }

    #[test]
    fn test_token_transfers_key_structure() {
        let type_hash = [0xABu8; 32];
        let key = encode_token_transfers_key(&type_hash);
        assert_eq!(key.len(), 33);
        assert_eq!(key[0], STATS_PREFIX_TOKEN_TRANSFERS);
        assert_eq!(&key[1..33], &type_hash);
    }

    #[test]
    fn test_token_hourly_key_structure() {
        let type_hash = [0xCDu8; 32];
        let hour_bucket: i64 = 482_000;
        let key = encode_token_hourly_key(&type_hash, hour_bucket);
        assert_eq!(key.len(), 41);
        assert_eq!(key[0], STATS_PREFIX_TOKEN_HOURLY);
        assert_eq!(&key[1..33], &type_hash);
        assert_eq!(
            i64::from_be_bytes(key[33..41].try_into().unwrap()),
            hour_bucket
        );
    }

    #[test]
    fn test_token_hourly_key_sort_order() {
        let type_hash = [0x01u8; 32];
        let k1 = encode_token_hourly_key(&type_hash, 100);
        let k2 = encode_token_hourly_key(&type_hash, 200);
        let k3 = encode_token_hourly_key(&type_hash, 300);
        assert!(k1 < k2);
        assert!(k2 < k3);
    }

    #[test]
    fn test_token_hourly_prefix_is_prefix_of_full_key() {
        let type_hash = [0x42u8; 32];
        let prefix = encode_token_hourly_prefix(&type_hash);
        let full_key = encode_token_hourly_key(&type_hash, 999);
        assert_eq!(prefix.len(), 33);
        assert!(full_key.starts_with(&prefix));
    }

    // ---- Activity key ----

    #[test]
    fn test_activity_key_roundtrip() {
        let lock_hash = [0xAAu8; 32];
        for (block, idx) in [
            (0i64, 0i32),
            (1, 0),
            (100, 5),
            (1_000_000, 42),
            (i64::MAX, 0),
        ] {
            let key = encode_activity_key(&lock_hash, block, idx);
            assert_eq!(key.len(), 44);
            let (decoded_hash, decoded_block, decoded_idx) = decode_activity_key(&key);
            assert_eq!(decoded_hash, lock_hash.to_vec());
            assert_eq!(decoded_block, block);
            assert_eq!(decoded_idx, idx);
        }
    }

    #[test]
    fn test_activity_key_descending_sort_order() {
        let lock_hash = [0xBBu8; 32];
        let k1 = encode_activity_key(&lock_hash, 300, 0);
        let k2 = encode_activity_key(&lock_hash, 200, 0);
        let k3 = encode_activity_key(&lock_hash, 100, 0);
        // Higher block_num should produce SMALLER key (descending)
        assert!(k1 < k2);
        assert!(k2 < k3);
    }

    #[test]
    fn test_activity_key_prefix_is_lock_hash() {
        let lock_hash = [0xCCu8; 32];
        let key = encode_activity_key(&lock_hash, 500, 3);
        assert!(key.starts_with(&lock_hash));
    }

    #[test]
    fn test_activity_key_different_locks_differ() {
        let lock_a = [0x01u8; 32];
        let lock_b = [0x02u8; 32];
        assert_ne!(
            encode_activity_key(&lock_a, 100, 0),
            encode_activity_key(&lock_b, 100, 0)
        );
    }

    // ---- Address daily stats key ----

    #[test]
    fn test_addr_daily_stats_key_roundtrip() {
        let lock_hash = [0xAAu8; 32];
        for date in [20240101u32, 20241231, 20250615, 99991231] {
            let key = encode_addr_daily_stats_key(&lock_hash, date);
            assert_eq!(key.len(), ADDR_DAILY_STATS_KEY_SIZE);
            let (decoded_hash, decoded_date) = decode_addr_daily_stats_key(&key);
            assert_eq!(decoded_hash, lock_hash.to_vec());
            assert_eq!(decoded_date, date);
        }
    }

    #[test]
    fn test_addr_daily_stats_key_sort_order() {
        let lock_hash = [0xBBu8; 32];
        let k1 = encode_addr_daily_stats_key(&lock_hash, 20240101);
        let k2 = encode_addr_daily_stats_key(&lock_hash, 20240601);
        let k3 = encode_addr_daily_stats_key(&lock_hash, 20241231);
        // Ascending date order for range scans
        assert!(k1 < k2);
        assert!(k2 < k3);
    }

    #[test]
    fn test_addr_daily_stats_key_prefix_is_lock_hash() {
        let lock_hash = [0xCCu8; 32];
        let key = encode_addr_daily_stats_key(&lock_hash, 20240101);
        assert!(key.starts_with(&lock_hash));
    }

    #[test]
    fn test_timestamp_ms_to_date() {
        // 2024-01-15 00:00:00 UTC = 1705276800000 ms
        assert_eq!(timestamp_ms_to_date(1705276800000), 20240115);
        // 2025-06-15 12:30:00 UTC = 1750000200000 ms
        assert_eq!(timestamp_ms_to_date(1750000200000), 20250615);
    }

    #[test]
    fn test_different_tokens_produce_different_keys() {
        let hash_a = [0x01u8; 32];
        let hash_b = [0x02u8; 32];
        assert_ne!(
            encode_token_transfers_key(&hash_a),
            encode_token_transfers_key(&hash_b)
        );
        assert_ne!(
            encode_token_hourly_key(&hash_a, 100),
            encode_token_hourly_key(&hash_b, 100)
        );
    }
}
