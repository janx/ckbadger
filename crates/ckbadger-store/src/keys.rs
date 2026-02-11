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

/// Encode an activity key: block_num(8B BE) + activity_idx(4B BE) = 12 bytes
pub fn encode_activity_key(block_num: i64, activity_idx: i32) -> [u8; 12] {
    let mut key = [0u8; 12];
    key[..8].copy_from_slice(&block_num.to_be_bytes());
    key[8..12].copy_from_slice(&activity_idx.to_be_bytes());
    key
}

/// Encode an activity-by-addr key:
/// lock_hash(32B) + block_num(8B BE) + idx(4B BE) = 44 bytes
pub fn encode_activity_by_addr_key(lock_hash: &[u8], block_num: i64, idx: i32) -> Vec<u8> {
    let mut key = Vec::with_capacity(44);
    key.extend_from_slice(&lock_hash[..32]);
    key.extend_from_slice(&block_num.to_be_bytes());
    key.extend_from_slice(&idx.to_be_bytes());
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
}
