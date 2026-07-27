//! Key encoding for RocksDB column families.
//!
//! All numeric keys use big-endian encoding for correct lexicographic sort order.
//! Fixed-size fields with no delimiters needed.

use ckbadger_common::TokenBalance;

/// Outpoint key: tx_hash(32B) + output_index(2B BE) = 34 bytes
pub const OUTPOINT_KEY_SIZE: usize = 34;
pub const SCRIPT_VERSION_BY_LABEL_LEN_SIZE: usize = 2;
pub const SCRIPT_VERSION_HASH_SIZE: usize = 32;
pub const SCRIPT_REFERENCE_KEY_SIZE: usize = 33;

/// Block number key: 8 bytes big-endian i64
pub const BLOCK_NUM_KEY_SIZE: usize = 8;
/// Block outpoint key: block_number(8B BE) + outpoint(34B) = 42 bytes
pub const BLOCK_OUTPOINT_KEY_SIZE: usize = BLOCK_NUM_KEY_SIZE + OUTPOINT_KEY_SIZE;
/// Reorg undo-log key: block_number(8B BE) + seq(8B BE) = 16 bytes
pub const REORG_UNDO_LOG_KEY_SIZE: usize = 16;

pub fn encode_outpoint(tx_hash: &[u8], output_index: i16) -> [u8; OUTPOINT_KEY_SIZE] {
    assert!(
        output_index >= 0,
        "encode_outpoint: expected non-negative output_index, got {}",
        output_index
    );
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

pub fn encode_script_version_by_label_key(label_key: &str, version_hash: &[u8]) -> Vec<u8> {
    assert!(
        version_hash.len() >= SCRIPT_VERSION_HASH_SIZE,
        "encode_script_version_by_label_key: version_hash must be >= 32 bytes, got {}",
        version_hash.len()
    );
    let label_bytes = label_key.as_bytes();
    let label_len = u16::try_from(label_bytes.len()).expect("label_key length exceeds u16::MAX");
    let mut key = Vec::with_capacity(
        SCRIPT_VERSION_BY_LABEL_LEN_SIZE + label_bytes.len() + SCRIPT_VERSION_HASH_SIZE,
    );
    key.extend_from_slice(&label_len.to_be_bytes());
    key.extend_from_slice(label_bytes);
    key.extend_from_slice(&version_hash[..SCRIPT_VERSION_HASH_SIZE]);
    key
}

pub fn encode_script_version_by_label_prefix(label_key: &str) -> Vec<u8> {
    let label_bytes = label_key.as_bytes();
    let label_len = u16::try_from(label_bytes.len()).expect("label_key length exceeds u16::MAX");
    let mut prefix = Vec::with_capacity(SCRIPT_VERSION_BY_LABEL_LEN_SIZE + label_bytes.len());
    prefix.extend_from_slice(&label_len.to_be_bytes());
    prefix.extend_from_slice(label_bytes);
    prefix
}

pub fn decode_script_version_by_label_key(key: &[u8]) -> (String, Vec<u8>) {
    assert!(
        key.len() >= SCRIPT_VERSION_BY_LABEL_LEN_SIZE + SCRIPT_VERSION_HASH_SIZE,
        "decode_script_version_by_label_key: expected at least {} bytes, got {}",
        SCRIPT_VERSION_BY_LABEL_LEN_SIZE + SCRIPT_VERSION_HASH_SIZE,
        key.len()
    );
    let label_len = u16::from_be_bytes([key[0], key[1]]) as usize;
    let expected_len = SCRIPT_VERSION_BY_LABEL_LEN_SIZE + label_len + SCRIPT_VERSION_HASH_SIZE;
    assert!(
        key.len() == expected_len,
        "decode_script_version_by_label_key: expected {} bytes from label_len {}, got {}",
        expected_len,
        label_len,
        key.len()
    );
    let label =
        String::from_utf8(key[2..2 + label_len].to_vec()).expect("label key must be valid UTF-8");
    let version_hash = key[2 + label_len..expected_len].to_vec();
    (label, version_hash)
}

pub fn encode_script_version_by_family_key(family_id: &str, version_hash: &[u8]) -> Vec<u8> {
    assert!(
        version_hash.len() == SCRIPT_VERSION_HASH_SIZE,
        "encode_script_version_by_family_key: version_hash must be exactly 32 bytes, got {}",
        version_hash.len()
    );
    let family_bytes = family_id.as_bytes();
    let family_len = u16::try_from(family_bytes.len()).expect("family_id length exceeds u16::MAX");
    let mut key = Vec::with_capacity(
        SCRIPT_VERSION_BY_LABEL_LEN_SIZE + family_bytes.len() + SCRIPT_VERSION_HASH_SIZE,
    );
    key.extend_from_slice(&family_len.to_be_bytes());
    key.extend_from_slice(family_bytes);
    key.extend_from_slice(&version_hash[..SCRIPT_VERSION_HASH_SIZE]);
    key
}

pub fn encode_script_version_by_family_prefix(family_id: &str) -> Vec<u8> {
    let family_bytes = family_id.as_bytes();
    let family_len = u16::try_from(family_bytes.len()).expect("family_id length exceeds u16::MAX");
    let mut prefix = Vec::with_capacity(SCRIPT_VERSION_BY_LABEL_LEN_SIZE + family_bytes.len());
    prefix.extend_from_slice(&family_len.to_be_bytes());
    prefix.extend_from_slice(family_bytes);
    prefix
}

pub fn decode_script_version_by_family_key(key: &[u8]) -> (String, Vec<u8>) {
    assert!(
        key.len() >= SCRIPT_VERSION_BY_LABEL_LEN_SIZE + SCRIPT_VERSION_HASH_SIZE,
        "decode_script_version_by_family_key: expected at least {} bytes, got {}",
        SCRIPT_VERSION_BY_LABEL_LEN_SIZE + SCRIPT_VERSION_HASH_SIZE,
        key.len()
    );
    let family_len = u16::from_be_bytes([key[0], key[1]]) as usize;
    let expected_len = SCRIPT_VERSION_BY_LABEL_LEN_SIZE + family_len + SCRIPT_VERSION_HASH_SIZE;
    assert!(
        key.len() == expected_len,
        "decode_script_version_by_family_key: expected {} bytes from family_len {}, got {}",
        expected_len,
        family_len,
        key.len()
    );
    let family_id =
        String::from_utf8(key[2..2 + family_len].to_vec()).expect("family_id must be valid UTF-8");
    let version_hash = key[2 + family_len..expected_len].to_vec();
    (family_id, version_hash)
}

pub fn encode_script_reference_key(
    hash_type: u8,
    reference_hash: &[u8],
) -> [u8; SCRIPT_REFERENCE_KEY_SIZE] {
    assert!(
        reference_hash.len() == SCRIPT_VERSION_HASH_SIZE,
        "encode_script_reference_key: reference_hash must be exactly 32 bytes, got {}",
        reference_hash.len()
    );
    let mut key = [0u8; SCRIPT_REFERENCE_KEY_SIZE];
    key[0] = hash_type;
    key[1..].copy_from_slice(&reference_hash[..SCRIPT_VERSION_HASH_SIZE]);
    key
}

pub fn decode_script_reference_key(key: &[u8]) -> (u8, Vec<u8>) {
    assert!(
        key.len() == SCRIPT_REFERENCE_KEY_SIZE,
        "decode_script_reference_key: expected {} bytes, got {}",
        SCRIPT_REFERENCE_KEY_SIZE,
        key.len()
    );
    (key[0], key[1..].to_vec())
}

pub fn encode_block_num(n: i64) -> [u8; BLOCK_NUM_KEY_SIZE] {
    assert!(
        n >= 0,
        "encode_block_num: expected non-negative block_num, got {}",
        n
    );
    n.to_be_bytes()
}

pub fn decode_block_num(key: &[u8]) -> i64 {
    assert!(
        key.len() >= 8,
        "decode_block_num: expected at least 8 bytes, got {}",
        key.len()
    );
    i64::from_be_bytes(
        key[..8]
            .try_into()
            .expect("decode_block_num: slice length checked"),
    )
}

pub fn encode_block_outpoint_key(
    block_num: i64,
    tx_hash: &[u8],
    output_index: i16,
) -> [u8; BLOCK_OUTPOINT_KEY_SIZE] {
    let mut key = [0u8; BLOCK_OUTPOINT_KEY_SIZE];
    key[..8].copy_from_slice(&block_num.to_be_bytes());
    key[8..42].copy_from_slice(&encode_outpoint(tx_hash, output_index));
    key
}

pub fn decode_block_outpoint_key(key: &[u8]) -> (i64, Vec<u8>, i16) {
    let block_num = decode_block_num(&key[..8]);
    let (tx_hash, output_index) = decode_outpoint(&key[8..42]);
    (block_num, tx_hash, output_index)
}

pub fn encode_tx_idx(idx: i32) -> [u8; 4] {
    idx.to_be_bytes()
}

pub fn decode_tx_idx(key: &[u8]) -> i32 {
    assert!(
        key.len() >= 4,
        "decode_tx_idx: expected at least 4 bytes, got {}",
        key.len()
    );
    i32::from_be_bytes(
        key[..4]
            .try_into()
            .expect("decode_tx_idx: slice length checked"),
    )
}

pub fn encode_reorg_undo_log_key(block_num: i64, seq: u64) -> [u8; REORG_UNDO_LOG_KEY_SIZE] {
    let mut key = [0u8; REORG_UNDO_LOG_KEY_SIZE];
    key[..8].copy_from_slice(&block_num.to_be_bytes());
    key[8..16].copy_from_slice(&seq.to_be_bytes());
    key
}

pub fn decode_reorg_undo_log_key(key: &[u8]) -> (i64, u64) {
    assert!(
        key.len() == REORG_UNDO_LOG_KEY_SIZE,
        "decode_reorg_undo_log_key: expected {} bytes, got {}",
        REORG_UNDO_LOG_KEY_SIZE,
        key.len()
    );
    let block_num = i64::from_be_bytes(key[..8].try_into().unwrap());
    let seq = u64::from_be_bytes(key[8..16].try_into().unwrap());
    (block_num, seq)
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

fn encode_desc_block_num(block_num: i64) -> [u8; 8] {
    assert!(
        block_num >= 0,
        "encode_desc_block_num: expected non-negative block_num, got {}",
        block_num
    );
    (i64::MAX - block_num).to_be_bytes()
}

fn decode_desc_block_num(bytes: &[u8]) -> i64 {
    assert!(
        bytes.len() == 8,
        "decode_desc_block_num: expected 8 bytes, got {}",
        bytes.len()
    );
    i64::MAX - i64::from_be_bytes(bytes.try_into().unwrap())
}

fn encode_desc_tx_idx(tx_idx: i32) -> [u8; 4] {
    assert!(
        tx_idx >= 0,
        "encode_desc_tx_idx: expected non-negative tx_idx, got {}",
        tx_idx
    );
    (i32::MAX - tx_idx).to_be_bytes()
}

fn decode_desc_tx_idx(bytes: &[u8]) -> i32 {
    assert!(
        bytes.len() == 4,
        "decode_desc_tx_idx: expected 4 bytes, got {}",
        bytes.len()
    );
    i32::MAX - i32::from_be_bytes(bytes.try_into().unwrap())
}

fn encode_desc_token_balance(value: &TokenBalance) -> [u8; TokenBalance::ENCODED_LEN] {
    let mut bytes = value.to_be_bytes();
    for byte in &mut bytes {
        *byte = !*byte;
    }
    bytes
}

fn decode_desc_token_balance(bytes: &[u8]) -> TokenBalance {
    assert!(
        bytes.len() == TokenBalance::ENCODED_LEN,
        "decode_desc_token_balance: expected {} bytes, got {}",
        TokenBalance::ENCODED_LEN,
        bytes.len()
    );
    let mut ascending = [0u8; TokenBalance::ENCODED_LEN];
    for (output, input) in ascending.iter_mut().zip(bytes) {
        *output = !*input;
    }
    TokenBalance::from_be_bytes(&ascending)
        .expect("fixed-width complemented token balance must decode")
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

/// Address-tx key: lock_hash(32B) + block_num_desc(8B BE) + tx_idx_desc(4B BE) + tx_hash(32B)
pub const ADDR_TX_KEY_SIZE: usize = 76;

pub fn encode_addr_tx_key(
    lock_hash: &[u8],
    block_num: i64,
    tx_idx: i32,
    tx_hash: &[u8],
) -> Vec<u8> {
    assert!(
        lock_hash.len() >= 32,
        "encode_addr_tx_key: lock_hash must be >= 32 bytes, got {}",
        lock_hash.len()
    );
    assert!(
        tx_hash.len() >= 32,
        "encode_addr_tx_key: tx_hash must be >= 32 bytes, got {}",
        tx_hash.len()
    );
    let mut key = Vec::with_capacity(ADDR_TX_KEY_SIZE);
    key.extend_from_slice(&lock_hash[..32]);
    key.extend_from_slice(&encode_desc_block_num(block_num));
    key.extend_from_slice(&encode_desc_tx_idx(tx_idx));
    key.extend_from_slice(&tx_hash[..32]);
    key
}

pub fn encode_addr_tx_seek_after_key(lock_hash: &[u8], block_num: i64, tx_idx: i32) -> Vec<u8> {
    assert!(
        lock_hash.len() >= 32,
        "encode_addr_tx_seek_after_key: lock_hash must be >= 32 bytes, got {}",
        lock_hash.len()
    );
    let mut key = Vec::with_capacity(ADDR_TX_KEY_SIZE);
    key.extend_from_slice(&lock_hash[..32]);
    key.extend_from_slice(&encode_desc_block_num(block_num));
    key.extend_from_slice(&encode_desc_tx_idx(tx_idx));
    key.extend_from_slice(&[0xFF; 32]);
    key
}

pub fn decode_addr_tx_key(key: &[u8]) -> (Vec<u8>, i64, i32, Vec<u8>) {
    assert!(
        key.len() == ADDR_TX_KEY_SIZE,
        "decode_addr_tx_key: expected {} bytes, got {}",
        ADDR_TX_KEY_SIZE,
        key.len()
    );
    let lock_hash = key[..32].to_vec();
    let block_num = decode_desc_block_num(&key[32..40]);
    let tx_idx = decode_desc_tx_idx(&key[40..44]);
    let tx_hash = key[44..76].to_vec();
    (lock_hash, block_num, tx_idx, tx_hash)
}

/// Encode a token_holders key: type_hash(32B) + lock_hash(32B) = 64 bytes
pub fn encode_token_holder_key(type_hash: &[u8], lock_hash: &[u8]) -> [u8; 64] {
    let mut key = [0u8; 64];
    key[..32].copy_from_slice(&type_hash[..32]);
    key[32..64].copy_from_slice(&lock_hash[..32]);
    key
}

/// Token holder ranked index key:
/// type_hash(32B) + balance_desc(32B BE) + lock_hash(32B) = 96 bytes
pub const TOKEN_HOLDER_BALANCE_KEY_SIZE: usize = 96;

pub fn encode_token_holder_balance_key(
    type_hash: &[u8],
    balance: &TokenBalance,
    lock_hash: &[u8],
) -> [u8; TOKEN_HOLDER_BALANCE_KEY_SIZE] {
    assert!(
        type_hash.len() >= 32,
        "encode_token_holder_balance_key: type_hash must be >= 32 bytes, got {}",
        type_hash.len()
    );
    assert!(
        lock_hash.len() >= 32,
        "encode_token_holder_balance_key: lock_hash must be >= 32 bytes, got {}",
        lock_hash.len()
    );
    let mut key = [0u8; TOKEN_HOLDER_BALANCE_KEY_SIZE];
    key[..32].copy_from_slice(&type_hash[..32]);
    key[32..64].copy_from_slice(&encode_desc_token_balance(balance));
    key[64..96].copy_from_slice(&lock_hash[..32]);
    key
}

pub fn encode_token_holder_balance_seek_after_key(
    type_hash: &[u8],
    balance: &TokenBalance,
    lock_hash: &[u8],
) -> Vec<u8> {
    let mut key = Vec::with_capacity(TOKEN_HOLDER_BALANCE_KEY_SIZE + 1);
    key.extend_from_slice(&encode_token_holder_balance_key(
        type_hash, balance, lock_hash,
    ));
    key.push(0xFF);
    key
}

pub fn decode_token_holder_balance_key(key: &[u8]) -> (Vec<u8>, TokenBalance, Vec<u8>) {
    assert!(
        key.len() == TOKEN_HOLDER_BALANCE_KEY_SIZE,
        "decode_token_holder_balance_key: expected {} bytes, got {}",
        TOKEN_HOLDER_BALANCE_KEY_SIZE,
        key.len()
    );
    let balance = decode_desc_token_balance(&key[32..64]);
    (key[..32].to_vec(), balance, key[64..96].to_vec())
}

/// Address-token ranked index key:
/// lock_hash(32B) + balance_desc(32B BE) + type_hash(32B) = 96 bytes
pub const ADDR_TOKEN_BALANCE_KEY_SIZE: usize = 96;

pub fn encode_addr_token_balance_key(
    lock_hash: &[u8],
    balance: &TokenBalance,
    type_hash: &[u8],
) -> [u8; ADDR_TOKEN_BALANCE_KEY_SIZE] {
    assert!(
        lock_hash.len() >= 32,
        "encode_addr_token_balance_key: lock_hash must be >= 32 bytes, got {}",
        lock_hash.len()
    );
    assert!(
        type_hash.len() >= 32,
        "encode_addr_token_balance_key: type_hash must be >= 32 bytes, got {}",
        type_hash.len()
    );
    let mut key = [0u8; ADDR_TOKEN_BALANCE_KEY_SIZE];
    key[..32].copy_from_slice(&lock_hash[..32]);
    key[32..64].copy_from_slice(&encode_desc_token_balance(balance));
    key[64..96].copy_from_slice(&type_hash[..32]);
    key
}

pub fn encode_addr_token_balance_seek_after_key(
    lock_hash: &[u8],
    balance: &TokenBalance,
    type_hash: &[u8],
) -> Vec<u8> {
    let mut key = Vec::with_capacity(ADDR_TOKEN_BALANCE_KEY_SIZE + 1);
    key.extend_from_slice(&encode_addr_token_balance_key(
        lock_hash, balance, type_hash,
    ));
    key.push(0xFF);
    key
}

pub fn decode_addr_token_balance_key(key: &[u8]) -> (Vec<u8>, TokenBalance, Vec<u8>) {
    assert!(
        key.len() == ADDR_TOKEN_BALANCE_KEY_SIZE,
        "decode_addr_token_balance_key: expected {} bytes, got {}",
        ADDR_TOKEN_BALANCE_KEY_SIZE,
        key.len()
    );
    let balance = decode_desc_token_balance(&key[32..64]);
    (key[..32].to_vec(), balance, key[64..96].to_vec())
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
    pub const HODL_WAVE: u8 = 0x0B;
    pub const CLUSTER_OWNER: u8 = 0x0C;
    pub const SPORE_HOURLY: u8 = 0x0D;
    pub const OBJECT_HOURLY: u8 = 0x0E;
    pub const SCRIPT_DAILY: u8 = 0x0F;
    pub const TOKEN_DAILY: u8 = 0x10;
    pub const CLUSTER_DAILY: u8 = 0x11;
    pub const SPORE_DAILY: u8 = 0x12;
    pub const SPORE_OUTPOINT: u8 = 0x13;
    pub const SPORE_TYPE_INDEX: u8 = 0x14;
    pub const OBJECT_DAILY: u8 = 0x15;
    pub const OBJECT_TYPE_INDEX: u8 = 0x16;
    pub const MNFT_CLASS_OUTPOINT: u8 = 0x17;
    pub const MNFT_TOKEN_OUTPOINT: u8 = 0x18;
    pub const DOTBIT_ACCOUNT_OUTPOINT: u8 = 0x19;
    pub const SPORE_OUTPOINT_BY_ID: u8 = 0x1A;
    pub const DAO_LATEST_STATS: u8 = 0x1B;
    pub const OBJECT_COLLECTION_OWNER: u8 = 0x1C;
    pub const ACTIVITY_DAILY: u8 = 0x1D;
    pub const ACTIVITY_HOURLY: u8 = 0x1E;
    pub const DOTBIT_OUTPOINT_BY_ACCOUNT_ID: u8 = 0x1F;
    pub const DAO_TOP_DEPOSITORS: u8 = 0x20;
    pub const CELL_DISTRIBUTION: u8 = 0x21;
    pub const ADDR_COHORT: u8 = 0x22;
    /// Persistent address set for daily unique address dedup across batches.
    /// Value: sorted, concatenated [u8; 32] lock hashes.
    pub const ACTIVITY_DAILY_ADDR_SET: u8 = 0x23;
    /// Persistent address set for hourly unique address dedup across batches.
    pub const ACTIVITY_HOURLY_ADDR_SET: u8 = 0x24;
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
pub const STATS_PREFIX_HODL_WAVE: u8 = stats_prefix::HODL_WAVE;
pub const STATS_PREFIX_CLUSTER_OWNER: u8 = stats_prefix::CLUSTER_OWNER;
pub const STATS_PREFIX_SPORE_HOURLY: u8 = stats_prefix::SPORE_HOURLY;
pub const STATS_PREFIX_OBJECT_HOURLY: u8 = stats_prefix::OBJECT_HOURLY;
pub const STATS_PREFIX_SCRIPT_DAILY: u8 = stats_prefix::SCRIPT_DAILY;
pub const STATS_PREFIX_TOKEN_DAILY: u8 = stats_prefix::TOKEN_DAILY;
pub const STATS_PREFIX_CLUSTER_DAILY: u8 = stats_prefix::CLUSTER_DAILY;
pub const STATS_PREFIX_SPORE_DAILY: u8 = stats_prefix::SPORE_DAILY;
pub const STATS_PREFIX_SPORE_OUTPOINT: u8 = stats_prefix::SPORE_OUTPOINT;
pub const STATS_PREFIX_SPORE_TYPE_INDEX: u8 = stats_prefix::SPORE_TYPE_INDEX;
pub const STATS_PREFIX_OBJECT_DAILY: u8 = stats_prefix::OBJECT_DAILY;
pub const STATS_PREFIX_OBJECT_TYPE_INDEX: u8 = stats_prefix::OBJECT_TYPE_INDEX;
pub const STATS_PREFIX_MNFT_CLASS_OUTPOINT: u8 = stats_prefix::MNFT_CLASS_OUTPOINT;
pub const STATS_PREFIX_MNFT_TOKEN_OUTPOINT: u8 = stats_prefix::MNFT_TOKEN_OUTPOINT;
pub const STATS_PREFIX_DOTBIT_ACCOUNT_OUTPOINT: u8 = stats_prefix::DOTBIT_ACCOUNT_OUTPOINT;
pub const STATS_PREFIX_SPORE_OUTPOINT_BY_ID: u8 = stats_prefix::SPORE_OUTPOINT_BY_ID;
pub const STATS_PREFIX_DAO_LATEST_STATS: u8 = stats_prefix::DAO_LATEST_STATS;
pub const STATS_PREFIX_OBJECT_COLLECTION_OWNER: u8 = stats_prefix::OBJECT_COLLECTION_OWNER;
pub const STATS_PREFIX_ACTIVITY_DAILY: u8 = stats_prefix::ACTIVITY_DAILY;
pub const STATS_PREFIX_ACTIVITY_HOURLY: u8 = stats_prefix::ACTIVITY_HOURLY;
pub const STATS_PREFIX_DOTBIT_OUTPOINT_BY_ACCOUNT_ID: u8 =
    stats_prefix::DOTBIT_OUTPOINT_BY_ACCOUNT_ID;
pub const STATS_PREFIX_DAO_TOP_DEPOSITORS: u8 = stats_prefix::DAO_TOP_DEPOSITORS;
pub const STATS_PREFIX_CELL_DISTRIBUTION: u8 = stats_prefix::CELL_DISTRIBUTION;
pub const STATS_PREFIX_ADDR_COHORT: u8 = stats_prefix::ADDR_COHORT;
pub const STATS_PREFIX_ACTIVITY_DAILY_ADDR_SET: u8 = stats_prefix::ACTIVITY_DAILY_ADDR_SET;
pub const STATS_PREFIX_ACTIVITY_HOURLY_ADDR_SET: u8 = stats_prefix::ACTIVITY_HOURLY_ADDR_SET;

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

/// Token daily stats key: prefix(1B) + type_hash(32B) + date(4B YYYYMMDD BE)
pub const TOKEN_DAILY_KEY_SIZE: usize = 37;

pub fn encode_token_daily_key(type_hash: &[u8], date_yyyymmdd: u32) -> [u8; TOKEN_DAILY_KEY_SIZE] {
    let mut key = [0u8; TOKEN_DAILY_KEY_SIZE];
    key[0] = STATS_PREFIX_TOKEN_DAILY;
    key[1..33].copy_from_slice(&type_hash[..32]);
    key[33..37].copy_from_slice(&date_yyyymmdd.to_be_bytes());
    key
}

/// Prefix for scanning all token daily entries.
pub fn encode_token_daily_prefix(type_hash: &[u8]) -> [u8; 33] {
    let mut prefix = [0u8; 33];
    prefix[0] = STATS_PREFIX_TOKEN_DAILY;
    prefix[1..33].copy_from_slice(&type_hash[..32]);
    prefix
}

pub fn decode_token_daily_key(key: &[u8]) -> (Vec<u8>, u32) {
    let type_hash = key[1..33].to_vec();
    let date = u32::from_be_bytes(key[33..37].try_into().unwrap());
    (type_hash, date)
}

/// Cluster daily stats key: prefix(1B) + cluster_id(32B) + date(4B YYYYMMDD BE)
pub const CLUSTER_DAILY_KEY_SIZE: usize = 37;

pub fn encode_cluster_daily_key(
    cluster_id: &[u8],
    date_yyyymmdd: u32,
) -> [u8; CLUSTER_DAILY_KEY_SIZE] {
    let mut key = [0u8; CLUSTER_DAILY_KEY_SIZE];
    key[0] = STATS_PREFIX_CLUSTER_DAILY;
    key[1..33].copy_from_slice(&cluster_id[..32]);
    key[33..37].copy_from_slice(&date_yyyymmdd.to_be_bytes());
    key
}

pub fn encode_cluster_daily_prefix(cluster_id: &[u8]) -> [u8; 33] {
    let mut prefix = [0u8; 33];
    prefix[0] = STATS_PREFIX_CLUSTER_DAILY;
    prefix[1..33].copy_from_slice(&cluster_id[..32]);
    prefix
}

pub fn decode_cluster_daily_key(key: &[u8]) -> (Vec<u8>, u32) {
    let cluster_id = key[1..33].to_vec();
    let date = u32::from_be_bytes(key[33..37].try_into().unwrap());
    (cluster_id, date)
}

/// Spore daily stats key: prefix(1B) + spore_id(32B) + date(4B YYYYMMDD BE)
pub const SPORE_DAILY_KEY_SIZE: usize = 37;

pub fn encode_spore_daily_key(spore_id: &[u8], date_yyyymmdd: u32) -> [u8; SPORE_DAILY_KEY_SIZE] {
    let mut key = [0u8; SPORE_DAILY_KEY_SIZE];
    key[0] = STATS_PREFIX_SPORE_DAILY;
    key[1..33].copy_from_slice(&spore_id[..32]);
    key[33..37].copy_from_slice(&date_yyyymmdd.to_be_bytes());
    key
}

pub fn encode_spore_daily_prefix(spore_id: &[u8]) -> [u8; 33] {
    let mut prefix = [0u8; 33];
    prefix[0] = STATS_PREFIX_SPORE_DAILY;
    prefix[1..33].copy_from_slice(&spore_id[..32]);
    prefix
}

pub fn decode_spore_daily_key(key: &[u8]) -> (Vec<u8>, u32) {
    let spore_id = key[1..33].to_vec();
    let date = u32::from_be_bytes(key[33..37].try_into().unwrap());
    (spore_id, date)
}

/// Spore outpoint lookup key: prefix(1B) + outpoint(34B)
pub const SPORE_OUTPOINT_KEY_SIZE: usize = 35;

pub fn encode_spore_outpoint_key(
    tx_hash: &[u8],
    output_index: i16,
) -> [u8; SPORE_OUTPOINT_KEY_SIZE] {
    let mut key = [0u8; SPORE_OUTPOINT_KEY_SIZE];
    key[0] = STATS_PREFIX_SPORE_OUTPOINT;
    key[1..35].copy_from_slice(&encode_outpoint(tx_hash, output_index));
    key
}

pub fn decode_spore_outpoint_key(key: &[u8]) -> (Vec<u8>, i16) {
    decode_outpoint(&key[1..35])
}

/// Spore outpoint reverse index key: prefix(1B) + spore_id(32B) + outpoint(34B)
pub const SPORE_OUTPOINT_BY_ID_KEY_SIZE: usize = 67;

/// Prefix for scanning all outpoints of a given spore: prefix(1B) + spore_id(32B)
pub const SPORE_OUTPOINT_BY_ID_PREFIX_SIZE: usize = 33;

pub fn encode_spore_outpoint_by_id_key(
    spore_id: &[u8],
    tx_hash: &[u8],
    output_index: i16,
) -> [u8; SPORE_OUTPOINT_BY_ID_KEY_SIZE] {
    let mut key = [0u8; SPORE_OUTPOINT_BY_ID_KEY_SIZE];
    key[0] = STATS_PREFIX_SPORE_OUTPOINT_BY_ID;
    key[1..33].copy_from_slice(&spore_id[..32]);
    key[33..67].copy_from_slice(&encode_outpoint(tx_hash, output_index));
    key
}

pub fn encode_spore_outpoint_by_id_prefix(
    spore_id: &[u8],
) -> [u8; SPORE_OUTPOINT_BY_ID_PREFIX_SIZE] {
    let mut prefix = [0u8; SPORE_OUTPOINT_BY_ID_PREFIX_SIZE];
    prefix[0] = STATS_PREFIX_SPORE_OUTPOINT_BY_ID;
    prefix[1..33].copy_from_slice(&spore_id[..32]);
    prefix
}

pub fn decode_spore_outpoint_by_id_key(key: &[u8]) -> (Vec<u8>, i16) {
    decode_outpoint(&key[33..67])
}

/// mNFT class outpoint lookup key: prefix(1B) + outpoint(34B)
pub const MNFT_CLASS_OUTPOINT_KEY_SIZE: usize = 35;

pub fn encode_mnft_class_outpoint_key(
    tx_hash: &[u8],
    output_index: i16,
) -> [u8; MNFT_CLASS_OUTPOINT_KEY_SIZE] {
    let mut key = [0u8; MNFT_CLASS_OUTPOINT_KEY_SIZE];
    key[0] = STATS_PREFIX_MNFT_CLASS_OUTPOINT;
    key[1..35].copy_from_slice(&encode_outpoint(tx_hash, output_index));
    key
}

/// mNFT token outpoint lookup key: prefix(1B) + outpoint(34B)
pub const MNFT_TOKEN_OUTPOINT_KEY_SIZE: usize = 35;

pub fn encode_mnft_token_outpoint_key(
    tx_hash: &[u8],
    output_index: i16,
) -> [u8; MNFT_TOKEN_OUTPOINT_KEY_SIZE] {
    let mut key = [0u8; MNFT_TOKEN_OUTPOINT_KEY_SIZE];
    key[0] = STATS_PREFIX_MNFT_TOKEN_OUTPOINT;
    key[1..35].copy_from_slice(&encode_outpoint(tx_hash, output_index));
    key
}

/// .bit account outpoint lookup key: prefix(1B) + outpoint(34B)
pub const DOTBIT_ACCOUNT_OUTPOINT_KEY_SIZE: usize = 35;

pub fn encode_dotbit_account_outpoint_key(
    tx_hash: &[u8],
    output_index: i16,
) -> [u8; DOTBIT_ACCOUNT_OUTPOINT_KEY_SIZE] {
    let mut key = [0u8; DOTBIT_ACCOUNT_OUTPOINT_KEY_SIZE];
    key[0] = STATS_PREFIX_DOTBIT_ACCOUNT_OUTPOINT;
    key[1..35].copy_from_slice(&encode_outpoint(tx_hash, output_index));
    key
}

pub fn decode_dotbit_account_outpoint_key(key: &[u8]) -> (Vec<u8>, i16) {
    decode_outpoint(&key[1..35])
}

/// .bit outpoint reverse index key: prefix(1B) + account_id(20B) + outpoint(34B)
pub const DOTBIT_OUTPOINT_BY_ACCOUNT_ID_KEY_SIZE: usize = 55;

/// Prefix for scanning all outpoints of a given .bit account: prefix(1B) + account_id(20B)
pub const DOTBIT_OUTPOINT_BY_ACCOUNT_ID_PREFIX_SIZE: usize = 21;

pub fn encode_dotbit_outpoint_by_account_id_key(
    account_id: &[u8],
    tx_hash: &[u8],
    output_index: i16,
) -> [u8; DOTBIT_OUTPOINT_BY_ACCOUNT_ID_KEY_SIZE] {
    let mut key = [0u8; DOTBIT_OUTPOINT_BY_ACCOUNT_ID_KEY_SIZE];
    key[0] = STATS_PREFIX_DOTBIT_OUTPOINT_BY_ACCOUNT_ID;
    key[1..21].copy_from_slice(&account_id[..20]);
    key[21..55].copy_from_slice(&encode_outpoint(tx_hash, output_index));
    key
}

pub fn encode_dotbit_outpoint_by_account_id_prefix(
    account_id: &[u8],
) -> [u8; DOTBIT_OUTPOINT_BY_ACCOUNT_ID_PREFIX_SIZE] {
    let mut prefix = [0u8; DOTBIT_OUTPOINT_BY_ACCOUNT_ID_PREFIX_SIZE];
    prefix[0] = STATS_PREFIX_DOTBIT_OUTPOINT_BY_ACCOUNT_ID;
    prefix[1..21].copy_from_slice(&account_id[..20]);
    prefix
}

pub fn decode_dotbit_outpoint_by_account_id_key(key: &[u8]) -> (Vec<u8>, i16) {
    decode_outpoint(&key[21..55])
}

/// Spore type-script index key: prefix(1B) + type_script_hash(32B)
pub const SPORE_TYPE_INDEX_KEY_SIZE: usize = 33;

pub fn encode_spore_type_index_key(type_script_hash: &[u8]) -> [u8; SPORE_TYPE_INDEX_KEY_SIZE] {
    let mut key = [0u8; SPORE_TYPE_INDEX_KEY_SIZE];
    key[0] = STATS_PREFIX_SPORE_TYPE_INDEX;
    key[1..33].copy_from_slice(&type_script_hash[..32]);
    key
}

/// Object collection daily stats key: prefix(1B) + collection_id(32B padded) + date(4B YYYYMMDD BE)
pub const OBJECT_DAILY_KEY_SIZE: usize = 37;

pub fn encode_object_daily_key(
    collection_id: &[u8],
    date_yyyymmdd: u32,
) -> [u8; OBJECT_DAILY_KEY_SIZE] {
    let mut key = [0u8; OBJECT_DAILY_KEY_SIZE];
    key[0] = STATS_PREFIX_OBJECT_DAILY;
    key[1..33].copy_from_slice(&pad_id_32(collection_id));
    key[33..37].copy_from_slice(&date_yyyymmdd.to_be_bytes());
    key
}

pub fn encode_object_daily_prefix(collection_id: &[u8]) -> [u8; 33] {
    let mut prefix = [0u8; 33];
    prefix[0] = STATS_PREFIX_OBJECT_DAILY;
    prefix[1..33].copy_from_slice(&pad_id_32(collection_id));
    prefix
}

pub const OBJECT_COLLECTION_OWNER_KEY_SIZE: usize = 65;

pub fn encode_object_collection_owner_key(
    collection_id: &[u8],
    lock_hash: &[u8],
) -> [u8; OBJECT_COLLECTION_OWNER_KEY_SIZE] {
    let mut key = [0u8; OBJECT_COLLECTION_OWNER_KEY_SIZE];
    key[0] = STATS_PREFIX_OBJECT_COLLECTION_OWNER;
    key[1..33].copy_from_slice(&pad_id_32(collection_id));
    key[33..65].copy_from_slice(&pad_id_32(lock_hash));
    key
}

pub fn encode_object_collection_owner_prefix(collection_id: &[u8]) -> [u8; 33] {
    let mut prefix = [0u8; 33];
    prefix[0] = STATS_PREFIX_OBJECT_COLLECTION_OWNER;
    prefix[1..33].copy_from_slice(&pad_id_32(collection_id));
    prefix
}

pub fn decode_object_daily_key(key: &[u8]) -> (Vec<u8>, u32) {
    let collection_id = key[1..33].to_vec();
    let date = u32::from_be_bytes(key[33..37].try_into().unwrap());
    (collection_id, date)
}

/// Object type-script index key: prefix(1B) + type_script_hash(32B)
pub const OBJECT_TYPE_INDEX_KEY_SIZE: usize = 33;

pub fn encode_object_type_index_key(type_script_hash: &[u8]) -> [u8; OBJECT_TYPE_INDEX_KEY_SIZE] {
    let mut key = [0u8; OBJECT_TYPE_INDEX_KEY_SIZE];
    key[0] = STATS_PREFIX_OBJECT_TYPE_INDEX;
    key[1..33].copy_from_slice(&type_script_hash[..32]);
    key
}

/// Object-by-collection secondary index key: collection_id(32B padded) + object_id(variable).
pub fn encode_object_by_collection_key(collection_id: &[u8], object_id: &[u8]) -> Vec<u8> {
    let mut key = Vec::with_capacity(32 + object_id.len());
    key.extend_from_slice(&pad_id_32(collection_id));
    key.extend_from_slice(object_id);
    key
}

/// Prefix for scanning all objects in a collection.
pub fn encode_object_by_collection_prefix(collection_id: &[u8]) -> [u8; 32] {
    pad_id_32(collection_id)
}

pub fn decode_object_by_collection_key(key: &[u8]) -> Option<(Vec<u8>, Vec<u8>)> {
    if key.len() < 32 {
        return None;
    }
    Some((key[..32].to_vec(), key[32..].to_vec()))
}

// ---- Identity-by-collection secondary index ----

/// Identity-by-collection secondary index key: collection_id(32B padded) + identity_id(variable).
pub fn encode_identity_by_collection_key(collection_id: &[u8], identity_id: &[u8]) -> Vec<u8> {
    let mut key = Vec::with_capacity(32 + identity_id.len());
    key.extend_from_slice(&pad_id_32(collection_id));
    key.extend_from_slice(identity_id);
    key
}

/// Prefix for scanning all identities in a collection.
pub fn encode_identity_by_collection_prefix(collection_id: &[u8]) -> [u8; 32] {
    pad_id_32(collection_id)
}

// ---- Identity owner counts (CF_STATS_IDENTITY) ----

/// Identity owner key: collection_id(32B) + lock_hash(32B) = 64 bytes.
/// Stored in CF_STATS_IDENTITY. Value is i64 LE (live identity count for this owner).
pub const IDENTITY_OWNER_KEY_SIZE: usize = 64;

pub fn encode_identity_owner_key(
    collection_id: &[u8],
    lock_hash: &[u8],
) -> [u8; IDENTITY_OWNER_KEY_SIZE] {
    let mut key = [0u8; IDENTITY_OWNER_KEY_SIZE];
    key[..32].copy_from_slice(&pad_id_32(collection_id));
    key[32..64].copy_from_slice(&lock_hash[..32]);
    key
}

pub fn encode_identity_owner_prefix(collection_id: &[u8]) -> [u8; 32] {
    pad_id_32(collection_id)
}

/// Zero-pad an ID to exactly 32 bytes. IDs shorter than 32 bytes (e.g. mNFT class_id = 24B)
/// are right-padded with zeros. Panics if the ID exceeds 32 bytes to prevent silent key
/// collisions from truncation.
fn pad_id_32(id: &[u8]) -> [u8; 32] {
    assert!(
        id.len() <= 32,
        "pad_id_32: ID exceeds 32 bytes (got {}), which would cause key collisions from truncation",
        id.len()
    );
    let mut buf = [0u8; 32];
    buf[..id.len()].copy_from_slice(id);
    buf
}

/// Spore (DOB) hourly transfer count key: prefix(1B) + cluster_id(32B) + hour_bucket(8B BE) = 41 bytes
pub fn encode_spore_hourly_key(cluster_id: &[u8], hour_bucket: i64) -> Vec<u8> {
    let mut key = Vec::with_capacity(41);
    key.push(STATS_PREFIX_SPORE_HOURLY);
    key.extend_from_slice(&pad_id_32(cluster_id));
    key.extend_from_slice(&hour_bucket.to_be_bytes());
    key
}

/// Prefix for scanning all hourly buckets of a given spore cluster.
pub fn encode_spore_hourly_prefix(cluster_id: &[u8]) -> Vec<u8> {
    let mut key = Vec::with_capacity(33);
    key.push(STATS_PREFIX_SPORE_HOURLY);
    key.extend_from_slice(&pad_id_32(cluster_id));
    key
}

/// Object hourly transfer count key: prefix(1B) + collection_id(32B) + hour_bucket(8B BE) = 41 bytes
pub fn encode_object_hourly_key(collection_id: &[u8], hour_bucket: i64) -> Vec<u8> {
    let mut key = Vec::with_capacity(41);
    key.push(STATS_PREFIX_OBJECT_HOURLY);
    key.extend_from_slice(&pad_id_32(collection_id));
    key.extend_from_slice(&hour_bucket.to_be_bytes());
    key
}

/// Prefix for scanning all hourly buckets of a given object collection.
pub fn encode_object_hourly_prefix(collection_id: &[u8]) -> Vec<u8> {
    let mut key = Vec::with_capacity(33);
    key.push(STATS_PREFIX_OBJECT_HOURLY);
    key.extend_from_slice(&pad_id_32(collection_id));
    key
}

/// Script daily stats key:
/// prefix(1B) + code_hash(32B) + script_kind(1B, 0=lock/1=type) + date(4B YYYYMMDD BE)
pub const SCRIPT_DAILY_KEY_SIZE: usize = 38;

pub fn encode_script_daily_key(code_hash: &[u8], is_type: bool, date_yyyymmdd: u32) -> [u8; 38] {
    let mut key = [0u8; SCRIPT_DAILY_KEY_SIZE];
    key[0] = STATS_PREFIX_SCRIPT_DAILY;
    key[1..33].copy_from_slice(&code_hash[..32]);
    key[33] = if is_type { 1 } else { 0 };
    key[34..38].copy_from_slice(&date_yyyymmdd.to_be_bytes());
    key
}

/// Prefix for scanning a script daily timeline by deployment and kind.
pub fn encode_script_daily_prefix(code_hash: &[u8], is_type: bool) -> [u8; 34] {
    let mut prefix = [0u8; 34];
    prefix[0] = STATS_PREFIX_SCRIPT_DAILY;
    prefix[1..33].copy_from_slice(&code_hash[..32]);
    prefix[33] = if is_type { 1 } else { 0 };
    prefix
}

pub fn decode_script_daily_key(key: &[u8]) -> (Vec<u8>, bool, u32) {
    let code_hash = key[1..33].to_vec();
    let is_type = key[33] == 1;
    let date = u32::from_be_bytes(key[34..38].try_into().unwrap());
    (code_hash, is_type, date)
}

/// Token transfer key: type_hash(32B) + block_num_desc(8B BE) + tx_idx_desc(4B BE) = 44 bytes
/// Uses descending block_num and tx_idx so newest transfers come first in prefix scan.
pub fn encode_token_transfer_key(type_hash: &[u8], block_num: i64, tx_idx: i32) -> Vec<u8> {
    let mut key = Vec::with_capacity(44);
    key.extend_from_slice(&type_hash[..32]);
    key.extend_from_slice(&encode_desc_block_num(block_num));
    key.extend_from_slice(&encode_desc_tx_idx(tx_idx));
    key
}

/// Decode block_num and tx_idx from a token transfer key.
pub fn decode_token_transfer_key(key: &[u8]) -> (i64, i32) {
    let block_desc = i64::from_be_bytes(key[32..40].try_into().unwrap());
    let block_num = i64::MAX - block_desc;
    let tx_idx_desc = i32::from_be_bytes(key[40..44].try_into().unwrap());
    let tx_idx = i32::MAX - tx_idx_desc;
    (block_num, tx_idx)
}

/// Cluster owner key: prefix(1B) + cluster_id(32B) + lock_hash(32B) = 65 bytes
/// Stored in the stats_spore CF. Value is i64 LE (live spore count for this owner).
pub const CLUSTER_OWNER_KEY_SIZE: usize = 65;

pub fn encode_cluster_owner_key(
    cluster_id: &[u8],
    lock_hash: &[u8],
) -> [u8; CLUSTER_OWNER_KEY_SIZE] {
    let mut key = [0u8; CLUSTER_OWNER_KEY_SIZE];
    key[0] = STATS_PREFIX_CLUSTER_OWNER;
    key[1..33].copy_from_slice(&cluster_id[..32]);
    key[33..65].copy_from_slice(&lock_hash[..32]);
    key
}

/// Prefix for scanning all owners of a given cluster.
pub fn encode_cluster_owner_prefix(cluster_id: &[u8]) -> [u8; 33] {
    let mut prefix = [0u8; 33];
    prefix[0] = STATS_PREFIX_CLUSTER_OWNER;
    prefix[1..33].copy_from_slice(&cluster_id[..32]);
    prefix
}

/// Spore-by-cluster key: cluster_id(32B) + spore_id(32B) = 64 bytes
pub fn encode_spore_by_cluster_key(cluster_id: &[u8], spore_id: &[u8]) -> [u8; 64] {
    let mut key = [0u8; 64];
    key[..32].copy_from_slice(&cluster_id[..32]);
    key[32..64].copy_from_slice(&spore_id[..32]);
    key
}

/// TxActions key: block_num_desc(8B BE) + tx_idx_desc(4B BE) + tx_hash(32B)
pub const TX_ACTIONS_KEY_SIZE: usize = 44;

pub fn encode_tx_actions_key(block_num: i64, tx_idx: i32, tx_hash: &[u8]) -> Vec<u8> {
    assert!(
        tx_hash.len() >= 32,
        "encode_tx_actions_key: tx_hash must be >= 32 bytes, got {}",
        tx_hash.len()
    );
    let mut key = Vec::with_capacity(TX_ACTIONS_KEY_SIZE);
    key.extend_from_slice(&encode_desc_block_num(block_num));
    key.extend_from_slice(&encode_desc_tx_idx(tx_idx));
    key.extend_from_slice(&tx_hash[..32]);
    key
}

pub fn encode_tx_actions_seek_after_key(block_num: i64, tx_idx: i32) -> Vec<u8> {
    let mut key = Vec::with_capacity(TX_ACTIONS_KEY_SIZE);
    key.extend_from_slice(&encode_desc_block_num(block_num));
    key.extend_from_slice(&encode_desc_tx_idx(tx_idx));
    key.extend_from_slice(&[0xFF; 32]);
    key
}

pub fn decode_tx_actions_key(key: &[u8]) -> (i64, i32, Vec<u8>) {
    assert!(
        key.len() == TX_ACTIONS_KEY_SIZE,
        "decode_tx_actions_key: expected {} bytes, got {}",
        TX_ACTIONS_KEY_SIZE,
        key.len()
    );
    let block_num = decode_desc_block_num(&key[0..8]);
    let tx_idx = decode_desc_tx_idx(&key[8..12]);
    let tx_hash = key[12..44].to_vec();
    (block_num, tx_idx, tx_hash)
}

/// DAO-by-block index key: deposit_block_desc(8B BE) + outpoint(34B) = 42 bytes
pub const DAO_BY_BLOCK_KEY_SIZE: usize = 42;
/// DAO-by-lock index key: lock_hash(32B) + deposit_block_desc(8B BE) + outpoint(34B) = 74 bytes
pub const DAO_BY_LOCK_BLOCK_KEY_SIZE: usize = 74;
/// DAO-by-status index key: status(2B BE) + deposit_block_desc(8B BE) + outpoint(34B) = 44 bytes
pub const DAO_BY_STATUS_BLOCK_KEY_SIZE: usize = 44;

pub fn encode_dao_by_block_key(
    deposit_block: i64,
    outpoint_key: &[u8],
) -> [u8; DAO_BY_BLOCK_KEY_SIZE] {
    assert!(
        outpoint_key.len() == OUTPOINT_KEY_SIZE,
        "encode_dao_by_block_key: expected outpoint {} bytes, got {}",
        OUTPOINT_KEY_SIZE,
        outpoint_key.len()
    );
    let mut key = [0u8; DAO_BY_BLOCK_KEY_SIZE];
    key[..8].copy_from_slice(&encode_desc_block_num(deposit_block));
    key[8..42].copy_from_slice(outpoint_key);
    key
}

pub fn decode_dao_by_block_key(key: &[u8]) -> (i64, Vec<u8>, i16) {
    assert!(
        key.len() == DAO_BY_BLOCK_KEY_SIZE,
        "decode_dao_by_block_key: expected {} bytes, got {}",
        DAO_BY_BLOCK_KEY_SIZE,
        key.len()
    );
    let block_desc = i64::from_be_bytes(key[..8].try_into().unwrap());
    let block_num = i64::MAX - block_desc;
    let (tx_hash, output_index) = decode_outpoint(&key[8..42]);
    (block_num, tx_hash, output_index)
}

pub fn encode_dao_by_lock_block_key(
    lock_hash: &[u8],
    deposit_block: i64,
    outpoint_key: &[u8],
) -> [u8; DAO_BY_LOCK_BLOCK_KEY_SIZE] {
    assert!(
        lock_hash.len() == 32,
        "encode_dao_by_lock_block_key: expected lock_hash 32 bytes, got {}",
        lock_hash.len()
    );
    assert!(
        outpoint_key.len() == OUTPOINT_KEY_SIZE,
        "encode_dao_by_lock_block_key: expected outpoint {} bytes, got {}",
        OUTPOINT_KEY_SIZE,
        outpoint_key.len()
    );
    let mut key = [0u8; DAO_BY_LOCK_BLOCK_KEY_SIZE];
    key[..32].copy_from_slice(lock_hash);
    key[32..40].copy_from_slice(&encode_desc_block_num(deposit_block));
    key[40..74].copy_from_slice(outpoint_key);
    key
}

pub fn decode_dao_by_lock_block_key(key: &[u8]) -> (Vec<u8>, i64, Vec<u8>, i16) {
    assert!(
        key.len() == DAO_BY_LOCK_BLOCK_KEY_SIZE,
        "decode_dao_by_lock_block_key: expected {} bytes, got {}",
        DAO_BY_LOCK_BLOCK_KEY_SIZE,
        key.len()
    );
    let lock_hash = key[..32].to_vec();
    let block_desc = i64::from_be_bytes(key[32..40].try_into().unwrap());
    let block_num = i64::MAX - block_desc;
    let (tx_hash, output_index) = decode_outpoint(&key[40..74]);
    (lock_hash, block_num, tx_hash, output_index)
}

pub fn encode_dao_by_lock_prefix(lock_hash: &[u8]) -> [u8; 32] {
    assert!(
        lock_hash.len() == 32,
        "encode_dao_by_lock_prefix: expected lock_hash 32 bytes, got {}",
        lock_hash.len()
    );
    let mut prefix = [0u8; 32];
    prefix.copy_from_slice(lock_hash);
    prefix
}

pub fn encode_dao_by_status_block_key(
    status: i16,
    deposit_block: i64,
    outpoint_key: &[u8],
) -> [u8; DAO_BY_STATUS_BLOCK_KEY_SIZE] {
    assert!(
        outpoint_key.len() == OUTPOINT_KEY_SIZE,
        "encode_dao_by_status_block_key: expected outpoint {} bytes, got {}",
        OUTPOINT_KEY_SIZE,
        outpoint_key.len()
    );
    let mut key = [0u8; DAO_BY_STATUS_BLOCK_KEY_SIZE];
    key[..2].copy_from_slice(&status.to_be_bytes());
    key[2..10].copy_from_slice(&encode_desc_block_num(deposit_block));
    key[10..44].copy_from_slice(outpoint_key);
    key
}

pub fn decode_dao_by_status_block_key(key: &[u8]) -> (i16, i64, Vec<u8>, i16) {
    assert!(
        key.len() == DAO_BY_STATUS_BLOCK_KEY_SIZE,
        "decode_dao_by_status_block_key: expected {} bytes, got {}",
        DAO_BY_STATUS_BLOCK_KEY_SIZE,
        key.len()
    );
    let status = i16::from_be_bytes(key[..2].try_into().unwrap());
    let block_desc = i64::from_be_bytes(key[2..10].try_into().unwrap());
    let block_num = i64::MAX - block_desc;
    let (tx_hash, output_index) = decode_outpoint(&key[10..44]);
    (status, block_num, tx_hash, output_index)
}

pub fn encode_dao_by_status_prefix(status: i16) -> [u8; 2] {
    status.to_be_bytes()
}

/// Convert a Unix timestamp in milliseconds to YYYYMMDD u32 (UTC+8 day boundary).
pub fn timestamp_ms_to_date(timestamp_ms: i64) -> u32 {
    let date = ckbadger_common::block_date_from_ms(timestamp_ms);
    let s = date.format("%Y%m%d").to_string();
    s.parse::<u32>()
        .expect("timestamp_ms_to_date: formatted date must parse into u32")
}

/// Object collection activity key:
/// collection_id(32B padded) + block_num_desc(8B BE) + tx_idx_desc(4B BE) + block_hash(32B) + tx_hash(32B)
/// Uses descending block_num and tx_idx so newest activities come first in prefix scan.
pub const OBJECT_COLLECTION_ACTIVITY_KEY_SIZE: usize = 108;

pub fn encode_object_collection_activity_key(
    collection_id: &[u8],
    block_num: i64,
    tx_idx: i32,
    block_hash: &[u8],
    tx_hash: &[u8],
) -> [u8; OBJECT_COLLECTION_ACTIVITY_KEY_SIZE] {
    assert!(
        block_hash.len() >= 32,
        "encode_object_collection_activity_key: block_hash must be >= 32 bytes, got {}",
        block_hash.len()
    );
    assert!(
        tx_hash.len() >= 32,
        "encode_object_collection_activity_key: tx_hash must be >= 32 bytes, got {}",
        tx_hash.len()
    );
    let mut key = [0u8; OBJECT_COLLECTION_ACTIVITY_KEY_SIZE];
    key[..32].copy_from_slice(&pad_id_32(collection_id));
    key[32..40].copy_from_slice(&encode_desc_block_num(block_num));
    key[40..44].copy_from_slice(&encode_desc_tx_idx(tx_idx));
    key[44..76].copy_from_slice(&block_hash[..32]);
    key[76..108].copy_from_slice(&tx_hash[..32]);
    key
}

pub fn encode_object_collection_activity_prefix(collection_id: &[u8]) -> [u8; 32] {
    pad_id_32(collection_id)
}

pub fn encode_object_collection_activity_seek_after_key(
    collection_id: &[u8],
    block_num: i64,
    tx_idx: i32,
) -> [u8; OBJECT_COLLECTION_ACTIVITY_KEY_SIZE] {
    let mut key = [0xFFu8; OBJECT_COLLECTION_ACTIVITY_KEY_SIZE];
    key[..32].copy_from_slice(&pad_id_32(collection_id));
    key[32..40].copy_from_slice(&encode_desc_block_num(block_num));
    key[40..44].copy_from_slice(&encode_desc_tx_idx(tx_idx));
    key
}

pub fn decode_object_collection_activity_key(key: &[u8]) -> ([u8; 32], i64, i32, Vec<u8>, Vec<u8>) {
    assert!(
        key.len() == OBJECT_COLLECTION_ACTIVITY_KEY_SIZE,
        "decode_object_collection_activity_key: expected {} bytes, got {}",
        OBJECT_COLLECTION_ACTIVITY_KEY_SIZE,
        key.len()
    );
    let mut collection_id = [0u8; 32];
    collection_id.copy_from_slice(&key[..32]);
    let block_num = decode_desc_block_num(&key[32..40]);
    let tx_idx = decode_desc_tx_idx(&key[40..44]);
    let block_hash = key[44..76].to_vec();
    let tx_hash = key[76..108].to_vec();
    (collection_id, block_num, tx_idx, block_hash, tx_hash)
}

/// Sync meta keys
pub mod sync_meta_keys {
    pub const TIP_BLOCK: &[u8] = b"tip_block";
    pub const SYNC_STATUS: &[u8] = b"sync_status";
    pub const RUNTIME_STATUS: &[u8] = b"runtime_status";
    pub const ROLLBACK_CLEANUP_IN_PROGRESS: &[u8] = b"rollback_cleanup_in_progress";
    pub const REORG_LATEST_EVENT: &[u8] = b"reorg_latest_event";
    pub const DEEP_FORK: &[u8] = b"deep_fork";
    pub const REORG_EVENTS: &[u8] = b"reorg_events";
    pub const HODL_TRACKER: &[u8] = b"hodl_tracker";
    pub const CELL_DIST_TRACKER: &[u8] = b"cell_dist_tracker";
    pub const SYNC_PROGRESS: &[u8] = b"sync_progress";
    pub const MEMORY_STATS: &[u8] = b"memory_stats";
    pub const BULK_BATCH_IN_PROGRESS: &[u8] = b"bulk_batch_in_progress";
    pub const BULK_BUILD_SESSION_IN_PROGRESS: &[u8] = b"bulk_build_session_in_progress";
    pub const BACKGROUND_TASKS: &[u8] = b"background_tasks";
    /// Chain-network tag ("mainnet"/"testnet") the DB was first synced for.
    pub const NETWORK_IDENTITY: &[u8] = b"network_identity";
    /// Genesis economic baseline (bincode `GenesisBaseline`), derived at block 0.
    pub const GENESIS_BASELINE: &[u8] = b"genesis_baseline";
}

// -- Fiber Channels --

pub const FIBER_CHANNEL_KEY_SIZE: usize = 32;

pub fn encode_fiber_channel_id(funding_tx_hash: &[u8], output_index: u32) -> Vec<u8> {
    use ckb_hash::new_blake2b;
    let mut hasher = new_blake2b();
    hasher.update(funding_tx_hash);
    hasher.update(&output_index.to_le_bytes());
    let mut hash = vec![0u8; 32];
    hasher.finalize(&mut hash);
    hash
}

pub const FIBER_OUTPOINT_SIZE: usize = 36;

pub fn encode_fiber_outpoint(tx_hash: &[u8], output_index: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity(FIBER_OUTPOINT_SIZE);
    out.extend_from_slice(tx_hash);
    out.extend_from_slice(&output_index.to_le_bytes());
    out
}

pub fn decode_fiber_outpoint(data: &[u8]) -> (Vec<u8>, u32) {
    assert!(
        data.len() >= FIBER_OUTPOINT_SIZE,
        "decode_fiber_outpoint: expected at least {} bytes, got {}",
        FIBER_OUTPOINT_SIZE,
        data.len()
    );
    let tx_hash = data[0..32].to_vec();
    let output_index = u32::from_le_bytes(data[32..36].try_into().unwrap());
    (tx_hash, output_index)
}

pub const ADDR_FIBER_CHANNEL_KEY_SIZE: usize = 64;

pub fn encode_addr_fiber_channel_key(lock_hash: &[u8], channel_id: &[u8]) -> Vec<u8> {
    assert!(
        lock_hash.len() >= 32,
        "encode_addr_fiber_channel_key: lock_hash must be >= 32 bytes, got {}",
        lock_hash.len()
    );
    assert!(
        channel_id.len() >= 32,
        "encode_addr_fiber_channel_key: channel_id must be >= 32 bytes, got {}",
        channel_id.len()
    );
    let mut key = Vec::with_capacity(ADDR_FIBER_CHANNEL_KEY_SIZE);
    key.extend_from_slice(&lock_hash[..32]);
    key.extend_from_slice(&channel_id[..32]);
    key
}

pub fn decode_addr_fiber_channel_key(key: &[u8]) -> (&[u8], &[u8]) {
    assert!(
        key.len() >= ADDR_FIBER_CHANNEL_KEY_SIZE,
        "decode_addr_fiber_channel_key: expected at least {} bytes, got {}",
        ADDR_FIBER_CHANNEL_KEY_SIZE,
        key.len()
    );
    (&key[0..32], &key[32..64])
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
    fn test_dao_by_block_key_roundtrip() {
        let outpoint = encode_outpoint(&[0xAA; 32], 3);
        let key = encode_dao_by_block_key(123, &outpoint);
        assert_eq!(key.len(), DAO_BY_BLOCK_KEY_SIZE);
        let (block, tx_hash, output_index) = decode_dao_by_block_key(&key);
        assert_eq!(block, 123);
        assert_eq!(tx_hash, vec![0xAA; 32]);
        assert_eq!(output_index, 3);
    }

    #[test]
    fn test_dao_by_lock_block_key_roundtrip() {
        let outpoint = encode_outpoint(&[0xBB; 32], 5);
        let key = encode_dao_by_lock_block_key(&[0x11; 32], 999, &outpoint);
        assert_eq!(key.len(), DAO_BY_LOCK_BLOCK_KEY_SIZE);
        let (lock_hash, block, tx_hash, output_index) = decode_dao_by_lock_block_key(&key);
        assert_eq!(lock_hash, vec![0x11; 32]);
        assert_eq!(block, 999);
        assert_eq!(tx_hash, vec![0xBB; 32]);
        assert_eq!(output_index, 5);
    }

    #[test]
    fn test_dao_by_status_block_key_roundtrip() {
        let outpoint = encode_outpoint(&[0xCC; 32], 1);
        let key = encode_dao_by_status_block_key(2, 456, &outpoint);
        assert_eq!(key.len(), DAO_BY_STATUS_BLOCK_KEY_SIZE);
        let (status, block, tx_hash, output_index) = decode_dao_by_status_block_key(&key);
        assert_eq!(status, 2);
        assert_eq!(block, 456);
        assert_eq!(tx_hash, vec![0xCC; 32]);
        assert_eq!(output_index, 1);
    }

    #[test]
    fn test_dao_index_prefix_helpers() {
        assert_eq!(encode_dao_by_status_prefix(1), 1i16.to_be_bytes());
        assert_eq!(encode_dao_by_lock_prefix(&[0x22; 32]), [0x22; 32]);
    }

    #[test]
    fn test_script_version_by_label_key_roundtrip() {
        let version_hash = [0xCD; 32];
        let key = encode_script_version_by_label_key("Default Lock", &version_hash);
        let prefix = encode_script_version_by_label_prefix("Default Lock");
        let (label, decoded_hash) = decode_script_version_by_label_key(&key);

        assert_eq!(label, "Default Lock");
        assert_eq!(decoded_hash, version_hash);
        assert!(key.starts_with(&prefix));
    }

    #[test]
    fn test_script_version_by_family_key_roundtrip() {
        let version_hash = [0x56; 32];
        let key = encode_script_version_by_family_key("family/default-lock", &version_hash);
        let prefix = encode_script_version_by_family_prefix("family/default-lock");
        let (family_id, decoded_hash) = decode_script_version_by_family_key(&key);

        assert_eq!(family_id, "family/default-lock");
        assert_eq!(decoded_hash, version_hash);
        assert!(key.starts_with(&prefix));
    }

    #[test]
    #[should_panic(
        expected = "encode_script_version_by_family_key: version_hash must be exactly 32 bytes"
    )]
    fn test_script_version_by_family_key_panics_on_oversized_hash() {
        let oversized = [0x56; 33];
        let _ = encode_script_version_by_family_key("family/default-lock", &oversized);
    }

    #[test]
    #[should_panic(
        expected = "encode_script_version_by_family_key: version_hash must be exactly 32 bytes"
    )]
    fn test_script_version_by_family_key_panics_on_undersized_hash() {
        let undersized = [0x56; 31];
        let _ = encode_script_version_by_family_key("family/default-lock", &undersized);
    }

    #[test]
    fn test_script_reference_key_roundtrip() {
        let reference_hash = [0xAB; 32];
        let key = encode_script_reference_key(1, &reference_hash);
        let (hash_type, decoded_hash) = decode_script_reference_key(&key);

        assert_eq!(hash_type, 1);
        assert_eq!(decoded_hash, reference_hash);
    }

    #[test]
    #[should_panic(
        expected = "encode_script_reference_key: reference_hash must be exactly 32 bytes"
    )]
    fn test_script_reference_key_panics_on_oversized_hash() {
        let oversized = [0xAB; 33];
        let _ = encode_script_reference_key(1, &oversized);
    }

    #[test]
    #[should_panic(
        expected = "encode_script_reference_key: reference_hash must be exactly 32 bytes"
    )]
    fn test_script_reference_key_panics_on_undersized_hash() {
        let undersized = [0xAB; 31];
        let _ = encode_script_reference_key(1, &undersized);
    }

    #[test]
    #[should_panic(expected = "encode_dao_by_lock_prefix: expected lock_hash 32 bytes")]
    fn test_dao_lock_prefix_panics_on_non_32_len() {
        let oversized = [0x33; 33];
        let _ = encode_dao_by_lock_prefix(&oversized);
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
    #[should_panic(expected = "decode_block_num: expected at least 8 bytes")]
    fn test_decode_block_num_panics_on_short_key() {
        let _ = decode_block_num(&[0x01; 7]);
    }

    #[test]
    #[should_panic(expected = "decode_tx_idx: expected at least 4 bytes")]
    fn test_decode_tx_idx_panics_on_short_key() {
        let _ = decode_tx_idx(&[0x01; 3]);
    }

    #[test]
    fn test_block_outpoint_key_roundtrip() {
        let block_num = 123_456;
        let tx_hash = [0x55u8; 32];
        let output_index = 9i16;
        let key = encode_block_outpoint_key(block_num, &tx_hash, output_index);
        assert_eq!(key.len(), BLOCK_OUTPOINT_KEY_SIZE);

        let (decoded_block, decoded_hash, decoded_index) = decode_block_outpoint_key(&key);
        assert_eq!(decoded_block, block_num);
        assert_eq!(decoded_hash, tx_hash.to_vec());
        assert_eq!(decoded_index, output_index);
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

    #[test]
    fn test_token_daily_key_roundtrip() {
        let type_hash = [0x77u8; 32];
        let key = encode_token_daily_key(&type_hash, 20260219);
        assert_eq!(key.len(), TOKEN_DAILY_KEY_SIZE);
        let (decoded_hash, decoded_date) = decode_token_daily_key(&key);
        assert_eq!(decoded_hash, type_hash.to_vec());
        assert_eq!(decoded_date, 20260219);
    }

    #[test]
    fn test_token_daily_prefix_is_prefix_of_full_key() {
        let type_hash = [0x11u8; 32];
        let prefix = encode_token_daily_prefix(&type_hash);
        let key = encode_token_daily_key(&type_hash, 20240101);
        assert_eq!(prefix.len(), 33);
        assert!(key.starts_with(&prefix));
    }

    #[test]
    fn test_cluster_daily_key_roundtrip() {
        let cluster_id = [0x88u8; 32];
        let key = encode_cluster_daily_key(&cluster_id, 20260219);
        assert_eq!(key.len(), CLUSTER_DAILY_KEY_SIZE);
        let (decoded_id, decoded_date) = decode_cluster_daily_key(&key);
        assert_eq!(decoded_id, cluster_id.to_vec());
        assert_eq!(decoded_date, 20260219);
    }

    #[test]
    fn test_cluster_daily_prefix_is_prefix_of_full_key() {
        let cluster_id = [0x22u8; 32];
        let prefix = encode_cluster_daily_prefix(&cluster_id);
        let key = encode_cluster_daily_key(&cluster_id, 20240101);
        assert_eq!(prefix.len(), 33);
        assert!(key.starts_with(&prefix));
    }

    #[test]
    fn test_spore_daily_key_roundtrip() {
        let spore_id = [0x99u8; 32];
        let key = encode_spore_daily_key(&spore_id, 20260219);
        assert_eq!(key.len(), SPORE_DAILY_KEY_SIZE);
        let (decoded_id, decoded_date) = decode_spore_daily_key(&key);
        assert_eq!(decoded_id, spore_id.to_vec());
        assert_eq!(decoded_date, 20260219);
    }

    #[test]
    fn test_spore_daily_prefix_is_prefix_of_full_key() {
        let spore_id = [0x33u8; 32];
        let prefix = encode_spore_daily_prefix(&spore_id);
        let key = encode_spore_daily_key(&spore_id, 20240101);
        assert_eq!(prefix.len(), 33);
        assert!(key.starts_with(&prefix));
    }

    #[test]
    fn test_spore_outpoint_key_roundtrip() {
        let tx_hash = [0xABu8; 32];
        let key = encode_spore_outpoint_key(&tx_hash, 7);
        assert_eq!(key.len(), SPORE_OUTPOINT_KEY_SIZE);
        assert_eq!(key[0], STATS_PREFIX_SPORE_OUTPOINT);
        let (decoded_tx_hash, decoded_output_index) = decode_spore_outpoint_key(&key);
        assert_eq!(decoded_tx_hash, tx_hash.to_vec());
        assert_eq!(decoded_output_index, 7);
    }

    #[test]
    fn test_spore_outpoint_by_id_key_roundtrip() {
        let spore_id = [0xCCu8; 32];
        let tx_hash = [0xDDu8; 32];
        let key = encode_spore_outpoint_by_id_key(&spore_id, &tx_hash, 3);
        assert_eq!(key.len(), SPORE_OUTPOINT_BY_ID_KEY_SIZE);
        assert_eq!(key[0], STATS_PREFIX_SPORE_OUTPOINT_BY_ID);
        assert_eq!(&key[1..33], &spore_id);
        let (decoded_tx_hash, decoded_output_index) = decode_spore_outpoint_by_id_key(&key);
        assert_eq!(decoded_tx_hash, tx_hash.to_vec());
        assert_eq!(decoded_output_index, 3);

        let prefix = encode_spore_outpoint_by_id_prefix(&spore_id);
        assert_eq!(prefix.len(), SPORE_OUTPOINT_BY_ID_PREFIX_SIZE);
        assert!(key.starts_with(&prefix));
    }

    #[test]
    fn test_mnft_class_outpoint_key_structure() {
        let tx_hash = [0xACu8; 32];
        let key = encode_mnft_class_outpoint_key(&tx_hash, 8);
        assert_eq!(key.len(), MNFT_CLASS_OUTPOINT_KEY_SIZE);
        assert_eq!(key[0], STATS_PREFIX_MNFT_CLASS_OUTPOINT);
        let (decoded_tx_hash, decoded_output_index) = decode_outpoint(&key[1..35]);
        assert_eq!(decoded_tx_hash, tx_hash.to_vec());
        assert_eq!(decoded_output_index, 8);
    }

    #[test]
    fn test_mnft_token_outpoint_key_structure() {
        let tx_hash = [0xADu8; 32];
        let key = encode_mnft_token_outpoint_key(&tx_hash, 9);
        assert_eq!(key.len(), MNFT_TOKEN_OUTPOINT_KEY_SIZE);
        assert_eq!(key[0], STATS_PREFIX_MNFT_TOKEN_OUTPOINT);
        let (decoded_tx_hash, decoded_output_index) = decode_outpoint(&key[1..35]);
        assert_eq!(decoded_tx_hash, tx_hash.to_vec());
        assert_eq!(decoded_output_index, 9);
    }

    #[test]
    fn test_dotbit_account_outpoint_key_structure() {
        let tx_hash = [0xAEu8; 32];
        let key = encode_dotbit_account_outpoint_key(&tx_hash, 10);
        assert_eq!(key.len(), DOTBIT_ACCOUNT_OUTPOINT_KEY_SIZE);
        assert_eq!(key[0], STATS_PREFIX_DOTBIT_ACCOUNT_OUTPOINT);
        let (decoded_tx_hash, decoded_output_index) = decode_dotbit_account_outpoint_key(&key);
        assert_eq!(decoded_tx_hash, tx_hash.to_vec());
        assert_eq!(decoded_output_index, 10);
    }

    #[test]
    fn test_dotbit_outpoint_by_account_id_key_roundtrip() {
        let account_id = [0xAFu8; 20];
        let tx_hash = [0xBBu8; 32];
        let key = encode_dotbit_outpoint_by_account_id_key(&account_id, &tx_hash, 5);
        assert_eq!(key.len(), DOTBIT_OUTPOINT_BY_ACCOUNT_ID_KEY_SIZE);
        assert_eq!(key[0], STATS_PREFIX_DOTBIT_OUTPOINT_BY_ACCOUNT_ID);
        assert_eq!(&key[1..21], &account_id);
        let (decoded_tx_hash, decoded_output_index) =
            decode_dotbit_outpoint_by_account_id_key(&key);
        assert_eq!(decoded_tx_hash, tx_hash.to_vec());
        assert_eq!(decoded_output_index, 5);

        let prefix = encode_dotbit_outpoint_by_account_id_prefix(&account_id);
        assert_eq!(prefix.len(), DOTBIT_OUTPOINT_BY_ACCOUNT_ID_PREFIX_SIZE);
        assert!(key.starts_with(&prefix));
    }

    #[test]
    fn test_spore_type_index_key_structure() {
        let type_script_hash = [0xBCu8; 32];
        let key = encode_spore_type_index_key(&type_script_hash);
        assert_eq!(key.len(), SPORE_TYPE_INDEX_KEY_SIZE);
        assert_eq!(key[0], STATS_PREFIX_SPORE_TYPE_INDEX);
        assert_eq!(&key[1..33], &type_script_hash);
    }

    #[test]
    fn test_object_daily_key_roundtrip() {
        let collection_id = [0x66u8; 24];
        let key = encode_object_daily_key(&collection_id, 20260219);
        assert_eq!(key.len(), OBJECT_DAILY_KEY_SIZE);
        let (decoded_id, decoded_date) = decode_object_daily_key(&key);
        assert_eq!(&decoded_id[..24], &collection_id);
        assert_eq!(&decoded_id[24..], &[0u8; 8]);
        assert_eq!(decoded_date, 20260219);
    }

    #[test]
    fn test_object_daily_prefix_is_prefix_of_full_key() {
        let collection_id = [0x77u8; 24];
        let prefix = encode_object_daily_prefix(&collection_id);
        let key = encode_object_daily_key(&collection_id, 20240101);
        assert_eq!(prefix.len(), 33);
        assert!(key.starts_with(&prefix));
    }

    #[test]
    fn test_object_type_index_key_structure() {
        let type_script_hash = [0xDDu8; 32];
        let key = encode_object_type_index_key(&type_script_hash);
        assert_eq!(key.len(), OBJECT_TYPE_INDEX_KEY_SIZE);
        assert_eq!(key[0], STATS_PREFIX_OBJECT_TYPE_INDEX);
        assert_eq!(&key[1..33], &type_script_hash);
    }

    #[test]
    fn test_object_by_collection_key_roundtrip() {
        let collection_id = [0xA1u8; 24];
        let object_id = [0xB2u8; 20];
        let key = encode_object_by_collection_key(&collection_id, &object_id);
        assert_eq!(key.len(), 52);
        let (decoded_collection, decoded_object) =
            decode_object_by_collection_key(&key).expect("valid object-by-collection key");
        assert_eq!(&decoded_collection[..24], &collection_id);
        assert_eq!(&decoded_collection[24..], &[0u8; 8]);
        assert_eq!(decoded_object, object_id.to_vec());
    }

    #[test]
    fn test_object_by_collection_prefix_is_prefix_of_full_key() {
        let collection_id = [0xF1u8; 24];
        let object_id = [0x1Fu8; 20];
        let prefix = encode_object_by_collection_prefix(&collection_id);
        let key = encode_object_by_collection_key(&collection_id, &object_id);
        assert_eq!(prefix.len(), 32);
        assert!(key.starts_with(&prefix));
    }

    #[test]
    fn test_reorg_undo_log_key_roundtrip() {
        let key = encode_reorg_undo_log_key(12345, 67890);
        assert_eq!(key.len(), REORG_UNDO_LOG_KEY_SIZE);
        let (block_num, seq) = decode_reorg_undo_log_key(&key);
        assert_eq!(block_num, 12345);
        assert_eq!(seq, 67890);
    }

    #[test]
    fn test_reorg_undo_log_key_sort_order() {
        let k1 = encode_reorg_undo_log_key(100, 1);
        let k2 = encode_reorg_undo_log_key(100, 2);
        let k3 = encode_reorg_undo_log_key(101, 0);
        assert!(k1 < k2);
        assert!(k2 < k3);
    }

    // ---- Activity key ----

    #[test]
    fn test_tx_actions_key_roundtrip() {
        let key = encode_tx_actions_key(123, 7, &[0x44; 32]);
        assert_eq!(key.len(), TX_ACTIONS_KEY_SIZE);

        let (block_num, tx_idx, tx_hash) = decode_tx_actions_key(&key);
        assert_eq!(block_num, 123);
        assert_eq!(tx_idx, 7);
        assert_eq!(tx_hash, vec![0x44; 32]);
    }

    #[test]
    fn test_tx_actions_key_descending_sort_order() {
        let k1 = encode_tx_actions_key(300, 0, &[0x11; 32]);
        let k2 = encode_tx_actions_key(200, 0, &[0x22; 32]);
        let k3 = encode_tx_actions_key(100, 5, &[0x33; 32]);
        let k4 = encode_tx_actions_key(100, 1, &[0x44; 32]);

        assert!(k1 < k2);
        assert!(k2 < k3);
        assert!(k3 < k4);
    }

    #[test]
    fn test_script_daily_key_roundtrip() {
        let code_hash = [0x55u8; 32];
        let key = encode_script_daily_key(&code_hash, true, 20250219);
        assert_eq!(key.len(), SCRIPT_DAILY_KEY_SIZE);
        let (decoded_hash, decoded_is_type, decoded_date) = decode_script_daily_key(&key);
        assert_eq!(decoded_hash, code_hash.to_vec());
        assert!(decoded_is_type);
        assert_eq!(decoded_date, 20250219);
    }

    #[test]
    fn test_script_daily_prefix_is_prefix_of_full_key() {
        let code_hash = [0x42u8; 32];
        let prefix = encode_script_daily_prefix(&code_hash, false);
        let full = encode_script_daily_key(&code_hash, false, 20240101);
        assert_eq!(prefix.len(), 34);
        assert!(full.starts_with(&prefix));
    }

    #[test]
    fn test_timestamp_ms_to_date() {
        // 2024-01-15 00:00:00 UTC = 08:00 UTC+8 → still 20240115
        assert_eq!(timestamp_ms_to_date(1705276800000), 20240115);
        // 2025-06-15 12:30:00 UTC = 20:30 UTC+8 → still 20250615
        assert_eq!(timestamp_ms_to_date(1750000200000), 20250615);
        // UTC+8 boundary test: 2024-01-15 15:59:59 UTC = 2024-01-15 23:59:59 UTC+8 → 20240115
        assert_eq!(timestamp_ms_to_date(1705334399000), 20240115);
        // 2024-01-15 16:00:00 UTC = 2024-01-16 00:00:00 UTC+8 → 20240116
        assert_eq!(timestamp_ms_to_date(1705334400000), 20240116);
    }

    #[test]
    fn test_spore_hourly_key_structure() {
        let cluster_id = [0xCDu8; 32];
        let hour_bucket: i64 = 482_000;
        let key = encode_spore_hourly_key(&cluster_id, hour_bucket);
        assert_eq!(key.len(), 41);
        assert_eq!(key[0], STATS_PREFIX_SPORE_HOURLY);
        assert_eq!(&key[1..33], &cluster_id);
        assert_eq!(
            i64::from_be_bytes(key[33..41].try_into().unwrap()),
            hour_bucket
        );
    }

    #[test]
    fn test_spore_hourly_key_sort_order() {
        let cluster_id = [0x01u8; 32];
        let k1 = encode_spore_hourly_key(&cluster_id, 100);
        let k2 = encode_spore_hourly_key(&cluster_id, 200);
        let k3 = encode_spore_hourly_key(&cluster_id, 300);
        assert!(k1 < k2);
        assert!(k2 < k3);
    }

    #[test]
    fn test_spore_hourly_prefix_is_prefix_of_full_key() {
        let cluster_id = [0x42u8; 32];
        let prefix = encode_spore_hourly_prefix(&cluster_id);
        let full_key = encode_spore_hourly_key(&cluster_id, 999);
        assert_eq!(prefix.len(), 33);
        assert!(full_key.starts_with(&prefix));
    }

    #[test]
    fn test_object_hourly_key_structure() {
        let collection_id = [0xCDu8; 32];
        let hour_bucket: i64 = 482_000;
        let key = encode_object_hourly_key(&collection_id, hour_bucket);
        assert_eq!(key.len(), 41);
        assert_eq!(key[0], STATS_PREFIX_OBJECT_HOURLY);
        assert_eq!(&key[1..33], &collection_id);
        assert_eq!(
            i64::from_be_bytes(key[33..41].try_into().unwrap()),
            hour_bucket
        );
    }

    #[test]
    fn test_object_hourly_key_sort_order() {
        let collection_id = [0x01u8; 32];
        let k1 = encode_object_hourly_key(&collection_id, 100);
        let k2 = encode_object_hourly_key(&collection_id, 200);
        let k3 = encode_object_hourly_key(&collection_id, 300);
        assert!(k1 < k2);
        assert!(k2 < k3);
    }

    #[test]
    fn test_object_hourly_prefix_is_prefix_of_full_key() {
        let collection_id = [0x42u8; 32];
        let prefix = encode_object_hourly_prefix(&collection_id);
        let full_key = encode_object_hourly_key(&collection_id, 999);
        assert_eq!(prefix.len(), 33);
        assert!(full_key.starts_with(&prefix));
    }

    // ---- Regression: short IDs (mNFT class_id = 24 bytes) must not panic ----

    #[test]
    fn test_object_hourly_key_short_collection_id() {
        // mNFT class_id is 24 bytes (20B issuer + 4B class index)
        let short_id = [0xAB; 24];
        let key = encode_object_hourly_key(&short_id, 500);
        assert_eq!(key.len(), 41);
        assert_eq!(key[0], STATS_PREFIX_OBJECT_HOURLY);
        // First 24 bytes of ID field should match, rest zero-padded
        assert_eq!(&key[1..25], &short_id);
        assert_eq!(&key[25..33], &[0u8; 8]);
        assert_eq!(i64::from_be_bytes(key[33..41].try_into().unwrap()), 500);
    }

    #[test]
    fn test_object_hourly_prefix_short_collection_id() {
        let short_id = [0xAB; 24];
        let prefix = encode_object_hourly_prefix(&short_id);
        let full_key = encode_object_hourly_key(&short_id, 999);
        assert_eq!(prefix.len(), 33);
        assert!(full_key.starts_with(&prefix));
    }

    #[test]
    fn test_object_collection_owner_key_structure() {
        let collection_id = [0xA1u8; 32];
        let owner = [0xB2u8; 32];
        let key = encode_object_collection_owner_key(&collection_id, &owner);
        assert_eq!(key.len(), OBJECT_COLLECTION_OWNER_KEY_SIZE);
        assert_eq!(key[0], STATS_PREFIX_OBJECT_COLLECTION_OWNER);
        assert_eq!(&key[1..33], &collection_id);
        assert_eq!(&key[33..65], &owner);
    }

    #[test]
    fn test_object_collection_owner_prefix_short_collection_id() {
        let collection_id = [0xCCu8; 24];
        let prefix = encode_object_collection_owner_prefix(&collection_id);
        let key = encode_object_collection_owner_key(&collection_id, &[0xDDu8; 32]);
        assert_eq!(prefix.len(), 33);
        assert_eq!(prefix[0], STATS_PREFIX_OBJECT_COLLECTION_OWNER);
        assert!(key.starts_with(&prefix));
    }

    #[test]
    fn test_spore_hourly_key_short_cluster_id() {
        let short_id = [0xCD; 20];
        let key = encode_spore_hourly_key(&short_id, 100);
        assert_eq!(key.len(), 41);
        assert_eq!(&key[1..21], &short_id);
        assert_eq!(&key[21..33], &[0u8; 12]);
    }

    #[test]
    fn test_spore_hourly_prefix_short_cluster_id() {
        let short_id = [0xCD; 20];
        let prefix = encode_spore_hourly_prefix(&short_id);
        let full_key = encode_spore_hourly_key(&short_id, 100);
        assert_eq!(prefix.len(), 33);
        assert!(full_key.starts_with(&prefix));
    }

    #[test]
    fn test_pad_id_32_exact() {
        let id = [0xFF; 32];
        assert_eq!(pad_id_32(&id), id);
    }

    #[test]
    fn test_pad_id_32_short() {
        let id = [0xAA; 10];
        let padded = pad_id_32(&id);
        assert_eq!(&padded[..10], &[0xAA; 10]);
        assert_eq!(&padded[10..], &[0u8; 22]);
    }

    #[test]
    fn test_pad_id_32_empty() {
        let padded = pad_id_32(&[]);
        assert_eq!(padded, [0u8; 32]);
    }

    #[test]
    #[should_panic(expected = "ID exceeds 32 bytes")]
    fn test_pad_id_32_rejects_oversized() {
        pad_id_32(&[0xBB; 33]);
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

    // ---- Object collection activity key ----

    #[test]
    fn test_object_collection_activity_key_roundtrip() {
        let collection_id = [0xAAu8; 32];
        for (block, idx) in [
            (0i64, 0i32),
            (1, 0),
            (100, 5),
            (1_000_000, 42),
            (i64::MAX, i32::MAX),
        ] {
            let block_hash = [idx as u8; 32];
            let tx_hash = [block as u8; 32];
            let key = encode_object_collection_activity_key(
                &collection_id,
                block,
                idx,
                &block_hash,
                &tx_hash,
            );
            assert_eq!(key.len(), OBJECT_COLLECTION_ACTIVITY_KEY_SIZE);
            let (decoded_cid, decoded_block, decoded_idx, decoded_block_hash, decoded_tx_hash) =
                decode_object_collection_activity_key(&key);
            assert_eq!(decoded_cid, collection_id);
            assert_eq!(decoded_block, block);
            assert_eq!(decoded_idx, idx);
            assert_eq!(decoded_block_hash, block_hash.to_vec());
            assert_eq!(decoded_tx_hash, tx_hash.to_vec());
        }
    }

    #[test]
    fn test_object_collection_activity_key_descending_sort() {
        let cid = [0xBBu8; 32];
        let block_hash = [0x66u8; 32];
        let tx_hash = [0x77u8; 32];
        let k1 = encode_object_collection_activity_key(&cid, 300, 5, &block_hash, &tx_hash);
        let k2 = encode_object_collection_activity_key(&cid, 200, 5, &block_hash, &tx_hash);
        let k3 = encode_object_collection_activity_key(&cid, 100, 5, &block_hash, &tx_hash);
        // Higher block_num => smaller key (descending)
        assert!(k1 < k2);
        assert!(k2 < k3);

        // Same block, higher tx_idx => smaller key (descending)
        let k4 = encode_object_collection_activity_key(&cid, 100, 10, &block_hash, &tx_hash);
        let k5 = encode_object_collection_activity_key(&cid, 100, 5, &block_hash, &tx_hash);
        assert!(k4 < k5);
    }

    #[test]
    fn test_object_collection_activity_prefix_matching() {
        let cid = [0xCCu8; 32];
        let prefix = encode_object_collection_activity_prefix(&cid);
        let key = encode_object_collection_activity_key(&cid, 500, 3, &[0x77; 32], &[0x88; 32]);
        assert!(key.starts_with(&prefix));

        let other_cid = [0xDDu8; 32];
        let other_key =
            encode_object_collection_activity_key(&other_cid, 500, 3, &[0x78; 32], &[0x99; 32]);
        assert!(!other_key.starts_with(&prefix));
    }

    #[test]
    fn test_object_collection_activity_padded_short_id() {
        let short_id = [0xEEu8; 20];
        let key =
            encode_object_collection_activity_key(&short_id, 100, 0, &[0xAB; 32], &[0xAA; 32]);
        let (decoded_cid, decoded_block, _, decoded_block_hash, decoded_tx_hash) =
            decode_object_collection_activity_key(&key);
        // First 20 bytes match, rest is zero-padded
        assert_eq!(&decoded_cid[..20], &short_id);
        assert_eq!(&decoded_cid[20..], &[0u8; 12]);
        assert_eq!(decoded_block, 100);
        assert_eq!(decoded_block_hash, vec![0xAB; 32]);
        assert_eq!(decoded_tx_hash, vec![0xAA; 32]);
    }

    #[test]
    fn test_encode_addr_tx_key_includes_tx_hash() {
        let lock_hash = [0x11u8; 32];
        let tx_hash = [0xAAu8; 32];
        let key = encode_addr_tx_key(&lock_hash, 100, 3, &tx_hash);
        assert_eq!(key.len(), 76);
        let (decoded_lock_hash, decoded_block, decoded_idx, decoded_tx_hash) =
            decode_addr_tx_key(&key);
        assert_eq!(decoded_lock_hash, lock_hash.to_vec());
        assert_eq!(decoded_block, 100);
        assert_eq!(decoded_idx, 3);
        assert_eq!(decoded_tx_hash, tx_hash.to_vec());
    }

    #[test]
    fn test_encode_object_collection_activity_key_includes_block_hash_and_tx_hash() {
        let collection_id = [0x33u8; 32];
        let block_hash = [0xABu8; 32];
        let tx_hash = [0xCCu8; 32];
        let key =
            encode_object_collection_activity_key(&collection_id, 300, 9, &block_hash, &tx_hash);
        assert_eq!(key.len(), OBJECT_COLLECTION_ACTIVITY_KEY_SIZE);
        let (
            decoded_collection_id,
            decoded_block,
            decoded_idx,
            decoded_block_hash,
            decoded_tx_hash,
        ) = decode_object_collection_activity_key(&key);
        assert_eq!(decoded_collection_id, collection_id);
        assert_eq!(decoded_block, 300);
        assert_eq!(decoded_idx, 9);
        assert_eq!(decoded_block_hash, block_hash.to_vec());
        assert_eq!(decoded_tx_hash, tx_hash.to_vec());
    }

    // -- ranked balance key codecs -----------------------------------------

    /// Balances that exercise every interesting width: zero, one, the u128
    /// ceiling, and values only representable in the widened U256 domain.
    fn ranked_balance_ladder() -> Vec<TokenBalance> {
        vec![
            TokenBalance::zero(),
            TokenBalance::from(1u128),
            TokenBalance::from(u128::MAX - 1),
            TokenBalance::from(u128::MAX),
            // u128::MAX + 1 — the first value that no longer fits in u128.
            "340282366920938463463374607431768211456"
                .parse::<TokenBalance>()
                .expect("u128::MAX + 1 parses as TokenBalance"),
            // 2^255, near the top of the 32-byte domain.
            "57896044618658097711785492504343953926634992332820282019728792003956564819968"
                .parse::<TokenBalance>()
                .expect("2^255 parses as TokenBalance"),
        ]
    }

    #[test]
    fn test_token_holder_balance_key_roundtrip_across_full_balance_domain() {
        let type_hash = [0x11u8; 32];
        let lock_hash = [0x22u8; 32];
        for balance in ranked_balance_ladder() {
            let key = encode_token_holder_balance_key(&type_hash, &balance, &lock_hash);
            assert_eq!(key.len(), TOKEN_HOLDER_BALANCE_KEY_SIZE);
            let (decoded_type, decoded_balance, decoded_lock) =
                decode_token_holder_balance_key(&key);
            assert_eq!(decoded_type, type_hash.to_vec());
            assert_eq!(decoded_lock, lock_hash.to_vec());
            assert_eq!(
                decoded_balance, balance,
                "token holder balance must round-trip exactly for {}",
                balance
            );
        }
    }

    #[test]
    fn test_addr_token_balance_key_roundtrip_across_full_balance_domain() {
        let lock_hash = [0x33u8; 32];
        let type_hash = [0x44u8; 32];
        for balance in ranked_balance_ladder() {
            let key = encode_addr_token_balance_key(&lock_hash, &balance, &type_hash);
            assert_eq!(key.len(), ADDR_TOKEN_BALANCE_KEY_SIZE);
            let (decoded_lock, decoded_balance, decoded_type) = decode_addr_token_balance_key(&key);
            assert_eq!(decoded_lock, lock_hash.to_vec());
            assert_eq!(decoded_type, type_hash.to_vec());
            assert_eq!(
                decoded_balance, balance,
                "addr token balance must round-trip exactly for {}",
                balance
            );
        }
    }

    #[test]
    fn test_token_holder_balance_keys_sort_strictly_descending_by_balance() {
        let type_hash = [0x55u8; 32];
        let lock_hash = [0x66u8; 32];
        let ladder = ranked_balance_ladder();
        // ladder is ascending by balance; ranked keys must be the reverse.
        let mut previous_key: Option<Vec<u8>> = None;
        for balance in ladder.iter().rev() {
            let key = encode_token_holder_balance_key(&type_hash, balance, &lock_hash).to_vec();
            if let Some(previous) = &previous_key {
                assert!(
                    previous.as_slice() < key.as_slice(),
                    "descending balance encoding must be strictly increasing lexicographically as balance falls: balance={}",
                    balance
                );
            }
            previous_key = Some(key);
        }
    }

    #[test]
    fn test_addr_token_balance_keys_sort_strictly_descending_by_balance() {
        let lock_hash = [0x77u8; 32];
        let type_hash = [0x88u8; 32];
        let ladder = ranked_balance_ladder();
        let mut previous_key: Option<Vec<u8>> = None;
        for balance in ladder.iter().rev() {
            let key = encode_addr_token_balance_key(&lock_hash, balance, &type_hash).to_vec();
            if let Some(previous) = &previous_key {
                assert!(
                    previous.as_slice() < key.as_slice(),
                    "descending balance encoding must be strictly increasing lexicographically as balance falls: balance={}",
                    balance
                );
            }
            previous_key = Some(key);
        }
    }

    /// A mixed ordering: the ranked CF is scanned per prefix, so within one
    /// prefix the balance segment must dominate the trailing hash segment.
    #[test]
    fn test_ranked_balance_dominates_trailing_hash_in_key_order() {
        let type_hash = [0x99u8; 32];
        let big = TokenBalance::from(u128::MAX);
        let small = TokenBalance::from(1u128);

        // Large balance with the lexicographically largest lock hash must still
        // sort before a small balance with the smallest lock hash.
        let big_with_max_lock = encode_token_holder_balance_key(&type_hash, &big, &[0xFF; 32]);
        let small_with_min_lock = encode_token_holder_balance_key(&type_hash, &small, &[0x00; 32]);
        assert!(
            big_with_max_lock.as_slice() < small_with_min_lock.as_slice(),
            "balance segment must outrank the trailing lock hash"
        );

        // Equal balances fall back to ascending lock hash order.
        let same_balance_low = encode_token_holder_balance_key(&type_hash, &big, &[0x01; 32]);
        let same_balance_high = encode_token_holder_balance_key(&type_hash, &big, &[0x02; 32]);
        assert!(same_balance_low.as_slice() < same_balance_high.as_slice());

        // Different prefixes never interleave regardless of balance.
        let other_type = [0x9Au8; 32];
        let other_type_big = encode_token_holder_balance_key(&other_type, &big, &[0x00; 32]);
        assert!(small_with_min_lock.as_slice() < other_type_big.as_slice());
    }

    #[test]
    fn test_seek_after_keys_follow_their_base_key_but_precede_the_next_balance() {
        let type_hash = [0xA1u8; 32];
        let lock_hash = [0xA2u8; 32];
        let balance = TokenBalance::from(u128::MAX);
        let next_lower = TokenBalance::from(u128::MAX - 1);

        let base = encode_token_holder_balance_key(&type_hash, &balance, &lock_hash);
        let seek_after =
            encode_token_holder_balance_seek_after_key(&type_hash, &balance, &lock_hash);
        let next = encode_token_holder_balance_key(&type_hash, &next_lower, &[0x00; 32]);

        assert!(base.as_slice() < seek_after.as_slice());
        assert!(seek_after.as_slice() < next.as_slice());

        let addr_base = encode_addr_token_balance_key(&lock_hash, &balance, &type_hash);
        let addr_seek_after =
            encode_addr_token_balance_seek_after_key(&lock_hash, &balance, &type_hash);
        let addr_next = encode_addr_token_balance_key(&lock_hash, &next_lower, &[0x00; 32]);
        assert!(addr_base.as_slice() < addr_seek_after.as_slice());
        assert!(addr_seek_after.as_slice() < addr_next.as_slice());
    }
}
