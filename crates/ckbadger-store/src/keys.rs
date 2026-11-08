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

/// Byte width of a blake2b-256 identifier (script hash, code hash, tx hash,
/// block hash, spore/cluster id, …) in a key window.
const HASH32_LEN: usize = 32;

/// Byte width of a `.bit` account ID in a key window.
const DOTBIT_ACCOUNT_ID_LEN: usize = 20;

/// Assert that a fixed-width key component exactly fills the window its key
/// layout reserves for it.
///
/// Keys in this module are delimiter-free byte layouts, so both directions are
/// invariant violations and neither may be repaired silently:
///
/// * **Too short** — the component cannot fill its window; slicing it used to
///   die as a bare `range end index N out of range` with no field name, which
///   tells an operator nothing about which caller is wrong.
/// * **Too long** — the surplus bytes are truncated away and the encoder
///   returns a *well-formed key belonging to a different entity*, silently
///   answering for the wrong script, address, token or outpoint. This is the
///   dangerous half, and it is exactly what an exact assert closes.
///
/// Callers own this invariant: request-supplied hashes are length-checked at
/// the API boundary (`ckbadger_api::utils::parse_hash32`) and chain-derived
/// ones are fixed-width by construction (`[u8; 32]` / `[u8; 20]` protocol
/// facts), so reaching this assert means an upstream bug, not bad user input.
///
/// Identifiers that are legitimately narrower than the 32-byte window — mNFT
/// class (24B) and token (28B) IDs, `.bit` account IDs (20B) — do not come
/// through here; they are right-padded by [`pad_id_32`], which enforces its own
/// (upper-bound) invariant.
#[inline]
#[track_caller]
fn assert_key_component_len(encoder: &str, component: &str, actual: usize, expected: usize) {
    assert!(
        actual == expected,
        "{}: {} must be exactly {} bytes, got {}",
        encoder,
        component,
        expected,
        actual
    );
}

pub fn encode_outpoint(tx_hash: &[u8], output_index: i16) -> [u8; OUTPOINT_KEY_SIZE] {
    assert_key_component_len("encode_outpoint", "tx_hash", tx_hash.len(), HASH32_LEN);
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
    assert_key_component_len(
        "encode_script_version_by_label_key",
        "version_hash",
        version_hash.len(),
        SCRIPT_VERSION_HASH_SIZE,
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
    assert_key_component_len(
        "encode_script_version_by_family_key",
        "version_hash",
        version_hash.len(),
        SCRIPT_VERSION_HASH_SIZE,
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
    assert_key_component_len(
        "encode_script_reference_key",
        "reference_hash",
        reference_hash.len(),
        SCRIPT_VERSION_HASH_SIZE,
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

/// Bit position of an undo sequence number's scope. The top 16 bits of the
/// `seq` in `encode_reorg_undo_log_key` name the scope of the row; the low 48
/// bits count entries within the block. This is the ONE definition: the
/// journal composes sequence numbers from it and the rollback reads it back.
pub const UNDO_SEQ_SCOPE_SHIFT: u32 = 48;
/// Largest per-block counter value a sequence number can carry.
pub const UNDO_SEQ_LOCAL_MAX: u64 = (1u64 << UNDO_SEQ_SCOPE_SHIFT) - 1;

/// The scope of an undo-log row. One scope exists: the block journal. A row
/// of any other scope bits is not a row this store writes, and the rollback
/// refuses it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u16)]
pub enum UndoSeqScope {
    /// The block journal (`block_overlay::BlockOverlay`): one entry per
    /// `(block, key)` of every `Journal`-class column family a live batch
    /// writes, holding the key's value at the end of the previous block.
    /// The value 0x0006 is the one the journal has used since it was the
    /// sixth scope; the per-writer scopes 0x0001–0x0005 are gone.
    Journal = 0x0006,
}

impl UndoSeqScope {
    /// The first sequence number of this scope: its scope bits, counter 0.
    pub const fn seq_base(self) -> u64 {
        (self as u64) << UNDO_SEQ_SCOPE_SHIFT
    }

    /// Whether `seq` was recorded under this scope.
    pub const fn owns(self, seq: u64) -> bool {
        seq >> UNDO_SEQ_SCOPE_SHIFT == self as u64
    }
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

pub fn encode_desc_block_num(block_num: i64) -> [u8; 8] {
    assert!(
        block_num >= 0,
        "encode_desc_block_num: expected non-negative block_num, got {}",
        block_num
    );
    (i64::MAX - block_num).to_be_bytes()
}

/// The block number of a newest-first key component (`i64::MAX - block`,
/// BE). [`encode_desc_block_num`] writes only `block >= 0`, so a component
/// with the top bit set encodes no block: a corrupt key, re-sync required.
fn decode_desc_block_num(bytes: &[u8]) -> anyhow::Result<i64> {
    let raw: [u8; 8] = bytes.try_into().map_err(|_| {
        anyhow::anyhow!(
            "newest-first block component has {} bytes, expected 8",
            bytes.len()
        )
    })?;
    i64::MAX
        .checked_sub(i64::from_be_bytes(raw))
        .ok_or_else(|| {
            anyhow::anyhow!(
                "newest-first block component 0x{} encodes no block: a corrupt key, re-sync \
                 required",
                hex::encode(bytes)
            )
        })
}

fn encode_desc_tx_idx(tx_idx: i32) -> [u8; 4] {
    assert!(
        tx_idx >= 0,
        "encode_desc_tx_idx: expected non-negative tx_idx, got {}",
        tx_idx
    );
    (i32::MAX - tx_idx).to_be_bytes()
}

/// The transaction index of a newest-first key component (`i32::MAX -
/// tx_idx`, BE); like [`decode_desc_block_num`], a component with the top bit
/// set is a corrupt key, re-sync required.
fn decode_desc_tx_idx(bytes: &[u8]) -> anyhow::Result<i32> {
    let raw: [u8; 4] = bytes.try_into().map_err(|_| {
        anyhow::anyhow!(
            "newest-first tx_idx component has {} bytes, expected 4",
            bytes.len()
        )
    })?;
    i32::MAX
        .checked_sub(i32::from_be_bytes(raw))
        .ok_or_else(|| {
            anyhow::anyhow!(
                "newest-first tx_idx component 0x{} encodes no transaction index: a corrupt key, \
                 re-sync required",
                hex::encode(bytes)
            )
        })
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

/// The first four bytes of every RISC-V binary CKB-VM runs (ELF magic).
pub const ELF_MAGIC: [u8; 4] = *b"\x7fELF";

/// The consensus Type ID script's code hash: 25 zero bytes, then `TYPE_ID`.
/// For readers that build a Type ID script from its args; recognising one is
/// [`is_type_id_script`]'s job (this code hash, referenced by type).
pub const TYPE_ID_CODE_HASH: [u8; HASH32_LEN] =
    *b"\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0TYPE_ID";

/// Whether `(code_hash, hash_type)` is the consensus Type ID script: the
/// all-zero code hash ending in `TYPE_ID`, referenced by type (hash_type 1).
pub fn is_type_id_script(code_hash: &[u8], hash_type: i16) -> bool {
    hash_type == 1
        && code_hash.len() == HASH32_LEN
        && code_hash[..25] == [0; 25]
        && &code_hash[25..] == b"TYPE_ID"
}

/// Whether a cell's data is code: it starts with [`ELF_MAGIC`] (so it is not
/// empty). CKB-VM loads only ELF binaries, `ckb_dlopen` libraries are ELF
/// shared objects, and the DOB decoder's VM loads only ELF.
///
/// The one "is code" rule: the `cell_by_data_hash` writers (live cell writer,
/// bulk build) and verify F8, the Type ID lineage on both sync paths, the cell
/// page's ELF reading and the code-version reads of the API all ask it. A
/// Type ID cell whose data is not ELF is state (a config cell, a retired
/// deployment), not code, and no reader looks it up by data hash. An
/// empty-data cell is never code: its data hash would index every plain cell
/// of the chain under one key.
pub fn is_code_cell(data: &[u8]) -> bool {
    data.starts_with(&ELF_MAGIC)
}

/// Width of a `cell_by_lock` / `cell_by_type` / `cell_by_data_hash` key:
/// `hash(32) + block(8 BE) + outpoint(34)`.
pub const CELL_INDEX_KEY_SIZE: usize = 74;

/// Encode a cell-by-lock/type/data-hash index key:
/// hash(32B) + block_num(8B BE) + outpoint(34B) = 74 bytes
///
/// # Invariant
///
/// `script_hash` and `tx_hash` must each be exactly 32 bytes: the key window is
/// fixed, so a longer hash is truncated into the block-number bytes of another
/// script's row and the scan answers for a different cell owner.
pub fn encode_cell_index_key(
    script_hash: &[u8],
    block_num: i64,
    tx_hash: &[u8],
    output_index: i16,
) -> Vec<u8> {
    assert_key_component_len(
        "encode_cell_index_key",
        "script_hash",
        script_hash.len(),
        HASH32_LEN,
    );
    assert_key_component_len(
        "encode_cell_index_key",
        "tx_hash",
        tx_hash.len(),
        HASH32_LEN,
    );
    let mut key = Vec::with_capacity(CELL_INDEX_KEY_SIZE);
    key.extend_from_slice(&script_hash[..32]);
    key.extend_from_slice(&block_num.to_be_bytes());
    key.extend_from_slice(&tx_hash[..32]);
    key.extend_from_slice(&output_index.to_be_bytes());
    key
}

/// Cell-by-code index key (CF_CELL_BY_LOCK_CODE / CF_CELL_BY_TYPE_CODE):
/// code_hash(32B) + hash_type(1B) + block_num(8B BE) + outpoint(34B) = 75 bytes.
///
/// The hash_type byte sits directly after the code hash so each script
/// reference form `(code_hash, hash_type)` is one contiguous key range: a
/// prefix scan over `encode_cell_code_index_prefix` hits exactly that form
/// and never pages through sibling forms sharing the same code hash bytes.
pub const CELL_CODE_INDEX_KEY_SIZE: usize = 75;

/// Prefix covering one `(code_hash, hash_type)` form in a cell-by-code index.
pub const CELL_CODE_INDEX_PREFIX_SIZE: usize = 33;

/// # Invariant
///
/// `code_hash` must be exactly 32 bytes. A shorter hash cannot fill the key
/// window, and a longer one would be truncated into the prefix of a *different*
/// script reference form — silently answering for the wrong script. Callers own
/// this invariant: request-supplied hashes are length-checked at the API
/// boundary (`ckbadger_api::utils::parse_hash32`) before they get here.
pub fn encode_cell_code_index_prefix(code_hash: &[u8], hash_type: u8) -> Vec<u8> {
    assert_key_component_len(
        "encode_cell_code_index_prefix",
        "code_hash",
        code_hash.len(),
        HASH32_LEN,
    );
    let mut prefix = Vec::with_capacity(CELL_CODE_INDEX_PREFIX_SIZE);
    prefix.extend_from_slice(&code_hash[..32]);
    prefix.push(hash_type);
    prefix
}

/// # Invariant
///
/// `code_hash` and `tx_hash` must each be exactly 32 bytes, for the same reason
/// as [`encode_cell_code_index_prefix`]: anything longer would be truncated into
/// a key belonging to a different script form or outpoint.
pub fn encode_cell_code_index_key(
    code_hash: &[u8],
    hash_type: u8,
    block_num: i64,
    tx_hash: &[u8],
    output_index: i16,
) -> Vec<u8> {
    assert_key_component_len(
        "encode_cell_code_index_key",
        "code_hash",
        code_hash.len(),
        HASH32_LEN,
    );
    assert_key_component_len(
        "encode_cell_code_index_key",
        "tx_hash",
        tx_hash.len(),
        HASH32_LEN,
    );
    let mut key = Vec::with_capacity(CELL_CODE_INDEX_KEY_SIZE);
    key.extend_from_slice(&code_hash[..32]);
    key.push(hash_type);
    key.extend_from_slice(&block_num.to_be_bytes());
    key.extend_from_slice(&tx_hash[..32]);
    key.extend_from_slice(&output_index.to_be_bytes());
    key
}

/// Convert a stored `i16` script hash_type into the single key byte used by
/// the cell-by-code indexes. Stored hash types are protocol values (0 = data,
/// 1 = type, 2 = data1, 4 = data2); anything outside `u8` range is corrupt
/// state and must fail fast at the call site with outpoint context.
pub fn cell_code_index_hash_type_byte(hash_type: i16) -> anyhow::Result<u8> {
    u8::try_from(hash_type).map_err(|_| {
        anyhow::anyhow!(
            "script hash_type {} does not fit the cell-by-code index key byte",
            hash_type
        )
    })
}

/// Address-tx key: lock_hash(32B) + block_num_desc(8B BE) + tx_idx_desc(4B BE).
/// The transaction is its position; its hash is `tx_index` there.
pub const ADDR_TX_KEY_SIZE: usize = 44;

pub fn encode_addr_tx_key(lock_hash: &[u8], block_num: i64, tx_idx: i32) -> Vec<u8> {
    assert_key_component_len(
        "encode_addr_tx_key",
        "lock_hash",
        lock_hash.len(),
        HASH32_LEN,
    );
    let mut key = Vec::with_capacity(ADDR_TX_KEY_SIZE);
    key.extend_from_slice(&lock_hash[..32]);
    key.extend_from_slice(&encode_desc_block_num(block_num));
    key.extend_from_slice(&encode_desc_tx_idx(tx_idx));
    key
}

/// The seek key for the first `addr_txs` row of `lock_hash` strictly after
/// (older than) the position `(block_num, tx_idx)`: the row key plus one
/// byte, so a forward seek passes the row itself.
pub fn encode_addr_tx_seek_after_key(lock_hash: &[u8], block_num: i64, tx_idx: i32) -> Vec<u8> {
    assert_key_component_len(
        "encode_addr_tx_seek_after_key",
        "lock_hash",
        lock_hash.len(),
        HASH32_LEN,
    );
    let mut key = encode_addr_tx_key(lock_hash, block_num, tx_idx);
    key.push(0);
    key
}

/// Seek key for the newest `addr_txs` row of `lock_hash` at or below
/// `block_num`. Rows are descending by block within a lock-hash prefix, so a
/// forward seek from `lock_hash ‖ desc(block_num)` passes exactly the rows of
/// higher blocks and lands on the first row of `block_num` or below, whatever
/// its tx index.
pub fn encode_addr_tx_block_seek_key(lock_hash: &[u8], block_num: i64) -> Vec<u8> {
    assert_key_component_len(
        "encode_addr_tx_block_seek_key",
        "lock_hash",
        lock_hash.len(),
        HASH32_LEN,
    );
    let mut key = Vec::with_capacity(HASH32_LEN + 8);
    key.extend_from_slice(&lock_hash[..32]);
    key.extend_from_slice(&encode_desc_block_num(block_num));
    key
}

/// `(lock_hash, block_num, tx_idx)` of an `addr_txs` key. A key of any other
/// width — the 76-byte layout that also held the tx hash included — is
/// refused: re-sync required.
pub fn decode_addr_tx_key(key: &[u8]) -> anyhow::Result<([u8; 32], i64, i32)> {
    anyhow::ensure!(
        key.len() == ADDR_TX_KEY_SIZE,
        "addr_txs key 0x{} has {} bytes, expected {} (lock_hash, block, tx_idx): written by an \
         older layout, re-sync required",
        hex::encode(key),
        key.len(),
        ADDR_TX_KEY_SIZE
    );
    let mut lock_hash = [0u8; 32];
    lock_hash.copy_from_slice(&key[..32]);
    Ok((
        lock_hash,
        decode_desc_block_num(&key[32..40])?,
        decode_desc_tx_idx(&key[40..44])?,
    ))
}

/// Address-tx key for a party a protocol named by the first 20 bytes of its lock
/// hash: prefix20 + block_num_desc(8B BE) + tx_idx_desc(4B BE) = 32.
///
/// A separate CF (and therefore a separate fixed width) keeps `ADDR_TX_KEY_SIZE`
/// and every length assert on the full-hash index intact.
pub const ADDR_TX_BY_PREFIX_KEY_SIZE: usize = 32;
const LOCK_PREFIX_LEN: usize = 20;

pub fn encode_addr_tx_by_prefix_key(prefix: &[u8], block_num: i64, tx_idx: i32) -> Vec<u8> {
    assert_key_component_len(
        "encode_addr_tx_by_prefix_key",
        "prefix",
        prefix.len(),
        LOCK_PREFIX_LEN,
    );
    let mut key = Vec::with_capacity(ADDR_TX_BY_PREFIX_KEY_SIZE);
    key.extend_from_slice(prefix);
    // Reuse the full-hash encoder's descending position suffix verbatim, so the
    // two indexes can never drift in ordering.
    key.extend_from_slice(&encode_addr_tx_key(&[0u8; 32], block_num, tx_idx)[32..]);
    key
}

/// [`encode_addr_tx_seek_after_key`] for `addr_txs_by_prefix`.
pub fn encode_addr_tx_by_prefix_seek_after_key(
    prefix: &[u8],
    block_num: i64,
    tx_idx: i32,
) -> Vec<u8> {
    assert_key_component_len(
        "encode_addr_tx_by_prefix_seek_after_key",
        "prefix",
        prefix.len(),
        LOCK_PREFIX_LEN,
    );
    let mut key = encode_addr_tx_by_prefix_key(prefix, block_num, tx_idx);
    key.push(0);
    key
}

/// `(prefix, block_num, tx_idx)` of an `addr_txs_by_prefix` key; any other
/// width (the 64-byte layout with the tx hash included) is refused.
pub fn decode_addr_tx_by_prefix_key(key: &[u8]) -> anyhow::Result<([u8; 20], i64, i32)> {
    anyhow::ensure!(
        key.len() == ADDR_TX_BY_PREFIX_KEY_SIZE,
        "addr_txs_by_prefix key 0x{} has {} bytes, expected {} (prefix, block, tx_idx): written \
         by an older layout, re-sync required",
        hex::encode(key),
        key.len(),
        ADDR_TX_BY_PREFIX_KEY_SIZE
    );
    let mut prefix = [0u8; LOCK_PREFIX_LEN];
    prefix.copy_from_slice(&key[..LOCK_PREFIX_LEN]);
    Ok((
        prefix,
        decode_desc_block_num(&key[20..28])?,
        decode_desc_tx_idx(&key[28..32])?,
    ))
}

/// Encode a token_holders key: type_hash(32B) + lock_hash(32B) = 64 bytes
pub fn encode_token_holder_key(type_hash: &[u8], lock_hash: &[u8]) -> [u8; 64] {
    assert_key_component_len(
        "encode_token_holder_key",
        "type_hash",
        type_hash.len(),
        HASH32_LEN,
    );
    assert_key_component_len(
        "encode_token_holder_key",
        "lock_hash",
        lock_hash.len(),
        HASH32_LEN,
    );
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
    assert_key_component_len(
        "encode_token_holder_balance_key",
        "type_hash",
        type_hash.len(),
        HASH32_LEN,
    );
    assert_key_component_len(
        "encode_token_holder_balance_key",
        "lock_hash",
        lock_hash.len(),
        HASH32_LEN,
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
    assert_key_component_len(
        "encode_addr_token_balance_key",
        "lock_hash",
        lock_hash.len(),
        HASH32_LEN,
    );
    assert_key_component_len(
        "encode_addr_token_balance_key",
        "type_hash",
        type_hash.len(),
        HASH32_LEN,
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
///
/// # Timezone conventions (one canonical chain time, two key formats — read before
/// comparing against these keys)
///
/// - **Date-scoped suffixes (`%Y%m%d`)** — `DAILY`, `DAILY_BLOCK`, `MINER`,
///   `DAO_DAILY_SNAPSHOT`, `ACTIVITY_DAILY`, `HODL_WAVE`, etc. — use the
///   **UTC+8** calendar day (`ckbadger_common::stats_bucket::stats_day(chain_time_ms)`), matching
///   the CKB explorer's day boundary.
/// - **`HOURLY` (`%Y%m%d%H`)** uses the **UTC** clock: the live writer
///   formats the UTC-truncated canonical block hour from
///   `ckbadger_common::stats_bucket::stats_hour(chain_time_ms)`.
/// - **`ACTIVITY_HOURLY` / `ACTIVITY_HOURLY_ADDR_ROW` (`%Y%m%d%H`)** use the
///   **UTC+8** display of that same canonical hour, keeping the
///   hourly activity series aligned with its UTC+8 daily parent.
///
/// Any cutoff/window computation against an hour-keyed prefix MUST use the
/// clock of that prefix.
pub mod stats_prefix {
    /// Chain daily stats. Suffix: `%Y%m%d`, UTC+8 calendar day.
    pub const DAILY: u8 = 0x01;
    /// Chain hourly stats. Suffix: `%Y%m%d%H` on the **UTC** clock — the one
    /// hour-keyed family that is NOT UTC+8.
    pub const HOURLY: u8 = 0x02;
    pub const EPOCH: u8 = 0x03;
    /// Per-miner daily blocks. Suffix: `%Y%m%d` (UTC+8) + 32B miner lock hash
    /// (cellbase-witness miner, RFC-0022).
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
    // 0x13 / 0x1A (spore item outpoint and its by-id reverse index), 0x17 /
    // 0x18 (mNFT class / token outpoint) and 0x19 / 0x1F (`.bit` account
    // outpoint and its by-account reverse index) moved to the
    // `object_outpoints` / `object_outpoint_by_id` CFs (C-4b; see
    // `object_outpoint_kind`) and are not reused.
    pub const SPORE_TYPE_INDEX: u8 = 0x14;
    pub const OBJECT_DAILY: u8 = 0x15;
    pub const OBJECT_TYPE_INDEX: u8 = 0x16;
    pub const DAO_LATEST_STATS: u8 = 0x1B;
    pub const OBJECT_COLLECTION_OWNER: u8 = 0x1C;
    /// Activity daily stats. Suffix: `%Y%m%d`, UTC+8 calendar day.
    pub const ACTIVITY_DAILY: u8 = 0x1D;
    /// Activity hourly stats. Suffix: `%Y%m%d%H` on the **UTC+8** clock
    /// (unlike chain-level `HOURLY`, which is UTC).
    pub const ACTIVITY_HOURLY: u8 = 0x1E;
    pub const DAO_TOP_DEPOSITORS: u8 = 0x20;
    pub const CELL_DISTRIBUTION: u8 = 0x21;
    pub const ADDR_COHORT: u8 = 0x22;
    // 0x23 / 0x24 (the retired `ACTIVITY_*_ADDR_SET` blobs: one bucket's
    // addresses in one value, written by bulk build for every bucket) are
    // not reused. A bucket's addresses are its `ACTIVITY_*_ADDR_ROW` rows
    // (C-3).
    // 0x25 (the unindexed `.bit` AccountCell marker) moved to
    // `object_outpoints` as `object_outpoint_kind::DOTBIT_UNINDEXED` and is
    // not reused.
    /// One unique `Lock` address of a daily activity bucket: the dedup
    /// memory behind `DailyActivityStats::unique_address_count`. Suffix:
    /// `%Y%m%d` (UTC+8) + 32B lock hash. Value: empty. Kept only while a
    /// block can still reach the bucket (C-3): live sync writes one row per
    /// address the bucket has not seen, in the block's scope, and deletes a
    /// bucket's rows once the rollback coverage floor's chain time has left
    /// the bucket; bulk build writes the rows only for the buckets still open
    /// at the handoff tip. One row per address keeps a live block's journal
    /// to one small entry per new address.
    pub const ACTIVITY_DAILY_ADDR_ROW: u8 = 0x26;
    /// One unique `Lock` address of an hourly activity bucket (see
    /// `ACTIVITY_DAILY_ADDR_ROW`). Suffix: `%Y%m%d%H` on the **UTC+8** clock +
    /// 32B lock hash. Value: empty.
    pub const ACTIVITY_HOURLY_ADDR_ROW: u8 = 0x27;
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
pub const STATS_PREFIX_SPORE_TYPE_INDEX: u8 = stats_prefix::SPORE_TYPE_INDEX;
pub const STATS_PREFIX_OBJECT_DAILY: u8 = stats_prefix::OBJECT_DAILY;
pub const STATS_PREFIX_OBJECT_TYPE_INDEX: u8 = stats_prefix::OBJECT_TYPE_INDEX;
pub const STATS_PREFIX_DAO_LATEST_STATS: u8 = stats_prefix::DAO_LATEST_STATS;
pub const STATS_PREFIX_OBJECT_COLLECTION_OWNER: u8 = stats_prefix::OBJECT_COLLECTION_OWNER;
pub const STATS_PREFIX_ACTIVITY_DAILY: u8 = stats_prefix::ACTIVITY_DAILY;
pub const STATS_PREFIX_ACTIVITY_HOURLY: u8 = stats_prefix::ACTIVITY_HOURLY;
pub const STATS_PREFIX_DAO_TOP_DEPOSITORS: u8 = stats_prefix::DAO_TOP_DEPOSITORS;
pub const STATS_PREFIX_CELL_DISTRIBUTION: u8 = stats_prefix::CELL_DISTRIBUTION;
pub const STATS_PREFIX_ADDR_COHORT: u8 = stats_prefix::ADDR_COHORT;
pub const STATS_PREFIX_ACTIVITY_DAILY_ADDR_ROW: u8 = stats_prefix::ACTIVITY_DAILY_ADDR_ROW;
pub const STATS_PREFIX_ACTIVITY_HOURLY_ADDR_ROW: u8 = stats_prefix::ACTIVITY_HOURLY_ADDR_ROW;

/// The two activity bucket families and their two row kinds: the bucket's
/// `DailyActivityStats` (kept forever) and its per-address rows (kept while a
/// block can still reach the bucket; see `stats_prefix::ACTIVITY_DAILY_ADDR_ROW`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ActivityBucketKind {
    /// UTC+8 calendar day, `%Y%m%d`.
    Daily,
    /// UTC+8 hour, `%Y%m%d%H`.
    Hourly,
}

impl ActivityBucketKind {
    /// Prefix of the bucket's `DailyActivityStats` row.
    pub const fn stats_prefix(self) -> u8 {
        match self {
            Self::Daily => stats_prefix::ACTIVITY_DAILY,
            Self::Hourly => stats_prefix::ACTIVITY_HOURLY,
        }
    }

    /// Prefix of the bucket's live per-address rows.
    pub const fn addr_row_prefix(self) -> u8 {
        match self {
            Self::Daily => stats_prefix::ACTIVITY_DAILY_ADDR_ROW,
            Self::Hourly => stats_prefix::ACTIVITY_HOURLY_ADDR_ROW,
        }
    }

    /// Byte length of the bucket label (`%Y%m%d` or `%Y%m%d%H`).
    pub const fn bucket_len(self) -> usize {
        match self {
            Self::Daily => 8,
            Self::Hourly => 10,
        }
    }

    fn check_bucket(self, bucket: &str) -> anyhow::Result<()> {
        if bucket.len() != self.bucket_len() || !bucket.bytes().all(|b| b.is_ascii_digit()) {
            anyhow::bail!(
                "invalid {:?} activity bucket label {:?}: expected {} ASCII digits",
                self,
                bucket,
                self.bucket_len()
            );
        }
        Ok(())
    }

    /// Key of the bucket's `DailyActivityStats` row.
    pub fn stats_key(self, bucket: &str) -> anyhow::Result<Vec<u8>> {
        self.check_bucket(bucket)?;
        Ok(encode_stats_key(self.stats_prefix(), bucket.as_bytes()))
    }

    /// The bucket of this kind a chain time falls in: its UTC+8 day
    /// (`%Y%m%d`) or UTC+8 hour (`%Y%m%d%H`), the labels the live writer and
    /// bulk build key the bucket by. Labels of one kind sort chronologically.
    pub fn bucket_label(self, chain_time_ms: i64) -> anyhow::Result<String> {
        match self {
            Self::Daily => ckbadger_common::stats_bucket::stats_date_key(chain_time_ms),
            Self::Hourly => ckbadger_common::stats_bucket::stats_hour_utc8_key(chain_time_ms),
        }
    }

    /// Key of one live per-address row: prefix ‖ bucket ‖ lock hash.
    pub fn addr_row_key(self, bucket: &str, lock_hash: &[u8; 32]) -> anyhow::Result<Vec<u8>> {
        self.check_bucket(bucket)?;
        let mut key = Vec::with_capacity(1 + self.bucket_len() + HASH32_LEN);
        key.push(self.addr_row_prefix());
        key.extend_from_slice(bucket.as_bytes());
        key.extend_from_slice(lock_hash);
        Ok(key)
    }

    /// Prefix of every per-address row of one bucket (for a prefix scan).
    pub fn addr_row_bucket_prefix(self, bucket: &str) -> anyhow::Result<Vec<u8>> {
        self.check_bucket(bucket)?;
        Ok(encode_stats_key(self.addr_row_prefix(), bucket.as_bytes()))
    }

    /// The lock hash of a per-address row key of `bucket`.
    pub fn decode_addr_row_key(self, bucket: &str, key: &[u8]) -> anyhow::Result<[u8; 32]> {
        let prefix = self.addr_row_bucket_prefix(bucket)?;
        if key.len() != prefix.len() + HASH32_LEN || !key.starts_with(&prefix) {
            anyhow::bail!(
                "invalid {:?} activity address row key for bucket {}: key=0x{}",
                self,
                bucket,
                hex::encode(key)
            );
        }
        let mut lock_hash = [0u8; 32];
        lock_hash.copy_from_slice(&key[prefix.len()..]);
        Ok(lock_hash)
    }
}

/// Token transfers total count key: prefix(1B) + type_hash(32B) = 33 bytes
pub fn encode_token_transfers_key(type_hash: &[u8]) -> Vec<u8> {
    assert_key_component_len(
        "encode_token_transfers_key",
        "type_hash",
        type_hash.len(),
        HASH32_LEN,
    );
    let mut key = Vec::with_capacity(33);
    key.push(STATS_PREFIX_TOKEN_TRANSFERS);
    key.extend_from_slice(&type_hash[..32]);
    key
}

/// Token hourly transfer count key: prefix(1B) + type_hash(32B) + hour_bucket(8B BE) = 41 bytes
/// hour_bucket = stats_hour_index(chain_time_ms)
pub fn encode_token_hourly_key(type_hash: &[u8], hour_bucket: i64) -> Vec<u8> {
    assert_key_component_len(
        "encode_token_hourly_key",
        "type_hash",
        type_hash.len(),
        HASH32_LEN,
    );
    let mut key = Vec::with_capacity(41);
    key.push(STATS_PREFIX_TOKEN_HOURLY);
    key.extend_from_slice(&type_hash[..32]);
    key.extend_from_slice(&hour_bucket.to_be_bytes());
    key
}

/// Prefix for scanning all hourly buckets of a given token.
pub fn encode_token_hourly_prefix(type_hash: &[u8]) -> Vec<u8> {
    assert_key_component_len(
        "encode_token_hourly_prefix",
        "type_hash",
        type_hash.len(),
        HASH32_LEN,
    );
    let mut key = Vec::with_capacity(33);
    key.push(STATS_PREFIX_TOKEN_HOURLY);
    key.extend_from_slice(&type_hash[..32]);
    key
}

/// Token daily stats key: prefix(1B) + type_hash(32B) + date(4B YYYYMMDD BE)
pub const TOKEN_DAILY_KEY_SIZE: usize = 37;

pub fn encode_token_daily_key(type_hash: &[u8], date_yyyymmdd: u32) -> [u8; TOKEN_DAILY_KEY_SIZE] {
    assert_key_component_len(
        "encode_token_daily_key",
        "type_hash",
        type_hash.len(),
        HASH32_LEN,
    );
    let mut key = [0u8; TOKEN_DAILY_KEY_SIZE];
    key[0] = STATS_PREFIX_TOKEN_DAILY;
    key[1..33].copy_from_slice(&type_hash[..32]);
    key[33..37].copy_from_slice(&date_yyyymmdd.to_be_bytes());
    key
}

/// Prefix for scanning all token daily entries.
pub fn encode_token_daily_prefix(type_hash: &[u8]) -> [u8; 33] {
    assert_key_component_len(
        "encode_token_daily_prefix",
        "type_hash",
        type_hash.len(),
        HASH32_LEN,
    );
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
    assert_key_component_len(
        "encode_cluster_daily_key",
        "cluster_id",
        cluster_id.len(),
        HASH32_LEN,
    );
    let mut key = [0u8; CLUSTER_DAILY_KEY_SIZE];
    key[0] = STATS_PREFIX_CLUSTER_DAILY;
    key[1..33].copy_from_slice(&cluster_id[..32]);
    key[33..37].copy_from_slice(&date_yyyymmdd.to_be_bytes());
    key
}

pub fn encode_cluster_daily_prefix(cluster_id: &[u8]) -> [u8; 33] {
    assert_key_component_len(
        "encode_cluster_daily_prefix",
        "cluster_id",
        cluster_id.len(),
        HASH32_LEN,
    );
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
    assert_key_component_len(
        "encode_spore_daily_key",
        "spore_id",
        spore_id.len(),
        HASH32_LEN,
    );
    let mut key = [0u8; SPORE_DAILY_KEY_SIZE];
    key[0] = STATS_PREFIX_SPORE_DAILY;
    key[1..33].copy_from_slice(&spore_id[..32]);
    key[33..37].copy_from_slice(&date_yyyymmdd.to_be_bytes());
    key
}

pub fn encode_spore_daily_prefix(spore_id: &[u8]) -> [u8; 33] {
    assert_key_component_len(
        "encode_spore_daily_prefix",
        "spore_id",
        spore_id.len(),
        HASH32_LEN,
    );
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

/// The kind of object a row of `object_outpoints` / `object_outpoint_by_id`
/// indexes: the first byte of every key of both CFs (C-4b). Every reader asks
/// for one kind, so each kind's rows are one contiguous key range (the mNFT
/// token scans and the per-transaction class seek read only their kind).
pub mod object_outpoint_kind {
    /// Spore, did:ckb and `.bit Cell` items, which share one id space: the
    /// item id at its natural width (32 bytes; did:ckb also 20). By-id rows
    /// carry the item's `AssetAction` byte for an identity, nothing for a
    /// spore.
    pub const SPORE_ITEM: u8 = 0x01;
    /// mNFT classes (outpoint rows only).
    pub const MNFT_CLASS: u8 = 0x02;
    /// mNFT tokens (outpoint rows only).
    pub const MNFT_TOKEN: u8 = 0x03;
    /// `.bit` accounts (20-byte account id); by-id rows carry the
    /// `AssetAction` byte.
    pub const DOTBIT_ACCOUNT: u8 = 0x04;
    /// An output carrying the `.bit` AccountCell type script that names no
    /// account (the DAS root AccountCell with its all-zero account id, or data
    /// too short to name one); value empty, outpoint rows only. A consumed
    /// AccountCell has exactly one of a `DOTBIT_ACCOUNT` row and this one.
    pub const DOTBIT_UNINDEXED: u8 = 0x05;
}

/// Spore item outpoint key (`object_outpoints`): kind(1B) + outpoint(34B)
pub const SPORE_OUTPOINT_KEY_SIZE: usize = 35;

pub fn encode_spore_outpoint_key(
    tx_hash: &[u8],
    output_index: i16,
) -> [u8; SPORE_OUTPOINT_KEY_SIZE] {
    let mut key = [0u8; SPORE_OUTPOINT_KEY_SIZE];
    key[0] = object_outpoint_kind::SPORE_ITEM;
    key[1..35].copy_from_slice(&encode_outpoint(tx_hash, output_index));
    key
}

pub fn decode_spore_outpoint_key(key: &[u8]) -> (Vec<u8>, i16) {
    decode_outpoint(&key[1..35])
}

/// Spore/identity outpoint reverse index key (`object_outpoint_by_id`):
/// `kind(1B) + id(1..=32B, verbatim) + outpoint(34B)`
///
/// The id component is VARIABLE width. Spore and `.bit Cell` ids are always 32
/// bytes, but did:ckb item ids are the type-script args verbatim and occur on
/// chain in more than one width (390 × 32-byte and 31 × 20-byte among the 421
/// live testnet did:ckb cells). The id is stored verbatim — the same way
/// `CF_IDENTITY_DATA` and `identity_by_collection` key it — rather than hashed
/// or padded, so a key still contains the id it belongs to.
///
/// **Scanning contract**: a shorter id is a byte-prefix of a longer id that
/// starts with the same bytes, so its rows share the scan prefix AND can sort
/// *between* the shorter id's own rows. Every reader must therefore
///
/// 1. bound the scan with `prefix(1B) + id`, and
/// 2. keep only keys whose length is exactly
///    [`spore_outpoint_by_id_key_len`] for the id being scanned — skipping
///    (never stopping at) foreign lengths.
///
/// The outpoint is recovered from the fixed-width 34-byte suffix, so it stays
/// decodable for any id width.
pub const SPORE_OUTPOINT_BY_ID_MAX_ID_LEN: usize = HASH32_LEN;

/// Exact key length for a given id width: `prefix(1B) + id + outpoint(34B)`.
pub const fn spore_outpoint_by_id_key_len(id_len: usize) -> usize {
    1 + id_len + OUTPOINT_KEY_SIZE
}

#[inline]
#[track_caller]
fn assert_by_id_len(encoder: &str, id_len: usize) {
    assert!(
        (1..=SPORE_OUTPOINT_BY_ID_MAX_ID_LEN).contains(&id_len),
        "{}: id must be between 1 and {} bytes, got {}",
        encoder,
        SPORE_OUTPOINT_BY_ID_MAX_ID_LEN,
        id_len
    );
}

pub fn encode_spore_outpoint_by_id_key(
    spore_id: &[u8],
    tx_hash: &[u8],
    output_index: i16,
) -> Vec<u8> {
    assert_by_id_len("encode_spore_outpoint_by_id_key", spore_id.len());
    let mut key = Vec::with_capacity(spore_outpoint_by_id_key_len(spore_id.len()));
    key.push(object_outpoint_kind::SPORE_ITEM);
    key.extend_from_slice(spore_id);
    key.extend_from_slice(&encode_outpoint(tx_hash, output_index));
    key
}

pub fn encode_spore_outpoint_by_id_prefix(spore_id: &[u8]) -> Vec<u8> {
    assert_by_id_len("encode_spore_outpoint_by_id_prefix", spore_id.len());
    let mut prefix = Vec::with_capacity(1 + spore_id.len());
    prefix.push(object_outpoint_kind::SPORE_ITEM);
    prefix.extend_from_slice(spore_id);
    prefix
}

pub fn decode_spore_outpoint_by_id_key(key: &[u8]) -> (Vec<u8>, i16) {
    assert!(
        key.len() >= spore_outpoint_by_id_key_len(1),
        "decode_spore_outpoint_by_id_key: key must be at least {} bytes, got {}",
        spore_outpoint_by_id_key_len(1),
        key.len()
    );
    decode_outpoint(&key[key.len() - OUTPOINT_KEY_SIZE..])
}

/// mNFT class outpoint key (`object_outpoints`): kind(1B) + outpoint(34B)
pub const MNFT_CLASS_OUTPOINT_KEY_SIZE: usize = 35;

pub fn encode_mnft_class_outpoint_key(
    tx_hash: &[u8],
    output_index: i16,
) -> [u8; MNFT_CLASS_OUTPOINT_KEY_SIZE] {
    let mut key = [0u8; MNFT_CLASS_OUTPOINT_KEY_SIZE];
    key[0] = object_outpoint_kind::MNFT_CLASS;
    key[1..35].copy_from_slice(&encode_outpoint(tx_hash, output_index));
    key
}

/// The prefix of every mNFT class outpoint key one transaction created:
/// kind(1B) + tx_hash(32B). The outpoint's tx hash leads, so a prefix seek
/// lists exactly that transaction's class outputs.
pub fn encode_mnft_class_outpoint_tx_prefix(tx_hash: &[u8]) -> [u8; 33] {
    assert_key_component_len(
        "encode_mnft_class_outpoint_tx_prefix",
        "tx_hash",
        tx_hash.len(),
        HASH32_LEN,
    );
    let mut prefix = [0u8; 33];
    prefix[0] = object_outpoint_kind::MNFT_CLASS;
    prefix[1..].copy_from_slice(tx_hash);
    prefix
}

/// mNFT token outpoint key (`object_outpoints`): kind(1B) + outpoint(34B)
pub const MNFT_TOKEN_OUTPOINT_KEY_SIZE: usize = 35;

pub fn encode_mnft_token_outpoint_key(
    tx_hash: &[u8],
    output_index: i16,
) -> [u8; MNFT_TOKEN_OUTPOINT_KEY_SIZE] {
    let mut key = [0u8; MNFT_TOKEN_OUTPOINT_KEY_SIZE];
    key[0] = object_outpoint_kind::MNFT_TOKEN;
    key[1..35].copy_from_slice(&encode_outpoint(tx_hash, output_index));
    key
}

/// .bit account outpoint key (`object_outpoints`): kind(1B) + outpoint(34B)
pub const DOTBIT_ACCOUNT_OUTPOINT_KEY_SIZE: usize = 35;

pub fn encode_dotbit_account_outpoint_key(
    tx_hash: &[u8],
    output_index: i16,
) -> [u8; DOTBIT_ACCOUNT_OUTPOINT_KEY_SIZE] {
    let mut key = [0u8; DOTBIT_ACCOUNT_OUTPOINT_KEY_SIZE];
    key[0] = object_outpoint_kind::DOTBIT_ACCOUNT;
    key[1..35].copy_from_slice(&encode_outpoint(tx_hash, output_index));
    key
}

pub fn decode_dotbit_account_outpoint_key(key: &[u8]) -> (Vec<u8>, i16) {
    decode_outpoint(&key[1..35])
}

/// Unindexed `.bit` AccountCell outpoint marker key (`object_outpoints`):
/// kind(1B) + outpoint(34B). The positive record that lets a later consumption
/// of the cell tell "never an account" apart from a lost account outpoint row.
pub const DOTBIT_UNINDEXED_OUTPOINT_KEY_SIZE: usize = 35;

pub fn encode_dotbit_unindexed_outpoint_key(
    tx_hash: &[u8],
    output_index: i16,
) -> [u8; DOTBIT_UNINDEXED_OUTPOINT_KEY_SIZE] {
    let mut key = [0u8; DOTBIT_UNINDEXED_OUTPOINT_KEY_SIZE];
    key[0] = object_outpoint_kind::DOTBIT_UNINDEXED;
    key[1..35].copy_from_slice(&encode_outpoint(tx_hash, output_index));
    key
}

/// .bit outpoint reverse index key (`object_outpoint_by_id`):
/// kind(1B) + account_id(20B) + outpoint(34B)
pub const DOTBIT_OUTPOINT_BY_ACCOUNT_ID_KEY_SIZE: usize = 55;

/// Prefix for scanning all outpoints of a given .bit account: kind(1B) + account_id(20B)
pub const DOTBIT_OUTPOINT_BY_ACCOUNT_ID_PREFIX_SIZE: usize = 21;

pub fn encode_dotbit_outpoint_by_account_id_key(
    account_id: &[u8],
    tx_hash: &[u8],
    output_index: i16,
) -> [u8; DOTBIT_OUTPOINT_BY_ACCOUNT_ID_KEY_SIZE] {
    assert_key_component_len(
        "encode_dotbit_outpoint_by_account_id_key",
        "account_id",
        account_id.len(),
        DOTBIT_ACCOUNT_ID_LEN,
    );
    let mut key = [0u8; DOTBIT_OUTPOINT_BY_ACCOUNT_ID_KEY_SIZE];
    key[0] = object_outpoint_kind::DOTBIT_ACCOUNT;
    key[1..21].copy_from_slice(&account_id[..20]);
    key[21..55].copy_from_slice(&encode_outpoint(tx_hash, output_index));
    key
}

pub fn encode_dotbit_outpoint_by_account_id_prefix(
    account_id: &[u8],
) -> [u8; DOTBIT_OUTPOINT_BY_ACCOUNT_ID_PREFIX_SIZE] {
    assert_key_component_len(
        "encode_dotbit_outpoint_by_account_id_prefix",
        "account_id",
        account_id.len(),
        DOTBIT_ACCOUNT_ID_LEN,
    );
    let mut prefix = [0u8; DOTBIT_OUTPOINT_BY_ACCOUNT_ID_PREFIX_SIZE];
    prefix[0] = object_outpoint_kind::DOTBIT_ACCOUNT;
    prefix[1..21].copy_from_slice(&account_id[..20]);
    prefix
}

pub fn decode_dotbit_outpoint_by_account_id_key(key: &[u8]) -> (Vec<u8>, i16) {
    decode_outpoint(&key[21..55])
}

/// Spore type-script index key: prefix(1B) + type_script_hash(32B)
pub const SPORE_TYPE_INDEX_KEY_SIZE: usize = 33;

pub fn encode_spore_type_index_key(type_script_hash: &[u8]) -> [u8; SPORE_TYPE_INDEX_KEY_SIZE] {
    assert_key_component_len(
        "encode_spore_type_index_key",
        "type_script_hash",
        type_script_hash.len(),
        HASH32_LEN,
    );
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
    // `collection_id` is padded (mNFT class IDs are 24B); the owner segment is a
    // lock hash and must fill its window exactly.
    assert_key_component_len(
        "encode_object_collection_owner_key",
        "lock_hash",
        lock_hash.len(),
        HASH32_LEN,
    );
    let mut key = [0u8; OBJECT_COLLECTION_OWNER_KEY_SIZE];
    key[0] = STATS_PREFIX_OBJECT_COLLECTION_OWNER;
    key[1..33].copy_from_slice(&pad_id_32(collection_id));
    key[33..65].copy_from_slice(lock_hash);
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
    assert_key_component_len(
        "encode_object_type_index_key",
        "type_script_hash",
        type_script_hash.len(),
        HASH32_LEN,
    );
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
    // `collection_id` is padded (sentinel collections and `.bit` IDs are
    // narrower); the owner segment is a lock hash and must fill it exactly.
    assert_key_component_len(
        "encode_identity_owner_key",
        "lock_hash",
        lock_hash.len(),
        HASH32_LEN,
    );
    let mut key = [0u8; IDENTITY_OWNER_KEY_SIZE];
    key[..32].copy_from_slice(&pad_id_32(collection_id));
    key[32..64].copy_from_slice(lock_hash);
    key
}

pub fn encode_identity_owner_prefix(collection_id: &[u8]) -> [u8; 32] {
    pad_id_32(collection_id)
}

/// Identity owner key for a collection whose chain-level owner is a 20-byte
/// lock-hash **prefix** (`.cell`): collection_id(32B) + owner20 + 12 zero
/// bytes, so it shares the fixed 64-byte `CF_STATS_IDENTITY` key width.
///
/// The padding is explicit, and the read side decodes it by collection rather
/// than treating the segment as a lock hash — a 20-byte prefix padded to 32
/// bytes is NOT a lock hash and must never be served as one.
pub fn encode_identity_owner20_key(
    collection_id: &[u8],
    owner20: &[u8],
) -> [u8; IDENTITY_OWNER_KEY_SIZE] {
    assert_key_component_len(
        "encode_identity_owner20_key",
        "owner20",
        owner20.len(),
        DOTCELL_ID_LEN,
    );
    let mut key = [0u8; IDENTITY_OWNER_KEY_SIZE];
    key[..32].copy_from_slice(&pad_id_32(collection_id));
    key[32..52].copy_from_slice(owner20);
    key
}

/// Read the 20-byte owner prefix back out of a 32-byte owner segment written
/// by `encode_identity_owner20_key`.
///
/// Fallible, for read paths serving stored rows: a malformed segment is a store
/// invariant violation to report with the segment, and a read handler must not
/// turn it into a panic (the release profile aborts on panic).
pub fn try_decode_identity_owner20(owner_segment: &[u8]) -> anyhow::Result<[u8; 20]> {
    if owner_segment.len() != HASH32_LEN {
        anyhow::bail!(
            "identity owner segment must be exactly {HASH32_LEN} bytes, got {}: 0x{}",
            owner_segment.len(),
            hex::encode(owner_segment)
        );
    }
    if !owner_segment[20..].iter().all(|byte| *byte == 0) {
        anyhow::bail!(
            "owner segment 0x{} is not a 20-byte prefix padded with zeros",
            hex::encode(owner_segment)
        );
    }
    Ok(owner_segment[..20]
        .try_into()
        .expect("length checked above"))
}

/// [`try_decode_identity_owner20`] for callers holding a segment they encoded
/// themselves, where a malformed one is a bug in the caller.
pub fn decode_identity_owner20(owner_segment: &[u8]) -> [u8; 20] {
    try_decode_identity_owner20(owner_segment)
        .unwrap_or_else(|e| panic!("decode_identity_owner20: {e:#}"))
}

// ---- `.cell` (DotCell) name-by-owner index ----

/// A `.cell` name id, and the owner prefix the chain stores: both 20 bytes.
pub const DOTCELL_ID_LEN: usize = 20;
/// `dotcell_name_by_owner` key: owner_hash20(20B) + name_id(20B).
pub const DOTCELL_NAME_BY_OWNER_KEY_SIZE: usize = 40;

pub fn encode_dotcell_name_by_owner_key(
    owner20: &[u8],
    name_id: &[u8],
) -> [u8; DOTCELL_NAME_BY_OWNER_KEY_SIZE] {
    assert_key_component_len(
        "encode_dotcell_name_by_owner_key",
        "owner20",
        owner20.len(),
        DOTCELL_ID_LEN,
    );
    assert_key_component_len(
        "encode_dotcell_name_by_owner_key",
        "name_id",
        name_id.len(),
        DOTCELL_ID_LEN,
    );
    let mut key = [0u8; DOTCELL_NAME_BY_OWNER_KEY_SIZE];
    key[..20].copy_from_slice(owner20);
    key[20..].copy_from_slice(name_id);
    key
}

pub fn encode_dotcell_name_by_owner_prefix(owner20: &[u8]) -> [u8; DOTCELL_ID_LEN] {
    assert_key_component_len(
        "encode_dotcell_name_by_owner_prefix",
        "owner20",
        owner20.len(),
        DOTCELL_ID_LEN,
    );
    owner20.try_into().expect("20 bytes")
}

pub fn decode_dotcell_name_by_owner_key(key: &[u8]) -> ([u8; 20], [u8; 20]) {
    assert_key_component_len(
        "decode_dotcell_name_by_owner_key",
        "key",
        key.len(),
        DOTCELL_NAME_BY_OWNER_KEY_SIZE,
    );
    (
        key[..20].try_into().expect("20 bytes"),
        key[20..40].try_into().expect("20 bytes"),
    )
}

/// Zero-pad an ID to exactly 32 bytes. IDs shorter than 32 bytes (e.g. mNFT class_id = 24B)
/// are right-padded with zeros. Panics if the ID exceeds 32 bytes to prevent silent key
/// collisions from truncation.
/// Pad a variable-width entity id to the fixed 32 bytes every `*_daily` /
/// `*_hourly` stats key uses.
///
/// Callers that join a stats key back to a CF keyed by the RAW id (for example
/// `cf_mnft_collection_agg`, keyed by the 24-byte mNFT class id) must pad the
/// raw id with this function rather than truncating the stats key — truncating
/// guesses a width the key does not record.
pub fn pad_id_32(id: &[u8]) -> [u8; 32] {
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
/// prefix(1B) + code_hash(32B) + hash_type(1B) + script_kind(1B, 0=lock/1=type) + date(4B YYYYMMDD BE)
///
/// The hash_type byte is part of the row identity: the same code_hash bytes
/// used with different hash_types (data/type/data1/data2) are distinct script
/// references with independent capacity timelines.
pub const SCRIPT_DAILY_KEY_SIZE: usize = 39;

/// Panics on a hash_type outside the CKB script hash_type domain {0,1,2,4}
/// to prevent silently writing rows under a corrupt reference identity.
fn assert_script_hash_type(hash_type: u8) {
    assert!(
        matches!(hash_type, 0 | 1 | 2 | 4),
        "script daily key hash_type must be one of 0/1/2/4, got {}",
        hash_type
    );
}

pub fn encode_script_daily_key(
    code_hash: &[u8],
    hash_type: u8,
    is_type: bool,
    date_yyyymmdd: u32,
) -> [u8; SCRIPT_DAILY_KEY_SIZE] {
    assert_key_component_len(
        "encode_script_daily_key",
        "code_hash",
        code_hash.len(),
        HASH32_LEN,
    );
    assert_script_hash_type(hash_type);
    let mut key = [0u8; SCRIPT_DAILY_KEY_SIZE];
    key[0] = STATS_PREFIX_SCRIPT_DAILY;
    key[1..33].copy_from_slice(&code_hash[..32]);
    key[33] = hash_type;
    key[34] = if is_type { 1 } else { 0 };
    key[35..39].copy_from_slice(&date_yyyymmdd.to_be_bytes());
    key
}

/// Prefix for scanning a script daily timeline by reference form and kind.
pub fn encode_script_daily_prefix(code_hash: &[u8], hash_type: u8, is_type: bool) -> [u8; 35] {
    assert_key_component_len(
        "encode_script_daily_prefix",
        "code_hash",
        code_hash.len(),
        HASH32_LEN,
    );
    assert_script_hash_type(hash_type);
    let mut prefix = [0u8; 35];
    prefix[0] = STATS_PREFIX_SCRIPT_DAILY;
    prefix[1..33].copy_from_slice(&code_hash[..32]);
    prefix[33] = hash_type;
    prefix[34] = if is_type { 1 } else { 0 };
    prefix
}

/// Prefix for scanning all script daily rows of a code_hash across every
/// hash_type form and script kind.
pub fn encode_script_daily_code_hash_prefix(code_hash: &[u8]) -> [u8; 33] {
    assert_key_component_len(
        "encode_script_daily_code_hash_prefix",
        "code_hash",
        code_hash.len(),
        HASH32_LEN,
    );
    let mut prefix = [0u8; 33];
    prefix[0] = STATS_PREFIX_SCRIPT_DAILY;
    prefix[1..33].copy_from_slice(&code_hash[..32]);
    prefix
}

pub fn decode_script_daily_key(key: &[u8]) -> (Vec<u8>, u8, bool, u32) {
    let code_hash = key[1..33].to_vec();
    let hash_type = key[33];
    let is_type = key[34] == 1;
    let date = u32::from_be_bytes(key[35..39].try_into().unwrap());
    (code_hash, hash_type, is_type, date)
}

/// Width of a `token_transfers` key (see [`encode_token_transfer_key`]).
pub const TOKEN_TRANSFER_KEY_SIZE: usize = 44;

/// Token transfer key: type_hash(32B) + block_num_desc(8B BE) + tx_idx_desc(4B BE) = 44 bytes
/// Uses descending block_num and tx_idx so newest transfers come first in prefix scan.
pub fn encode_token_transfer_key(type_hash: &[u8], block_num: i64, tx_idx: i32) -> Vec<u8> {
    assert_key_component_len(
        "encode_token_transfer_key",
        "type_hash",
        type_hash.len(),
        HASH32_LEN,
    );
    let mut key = Vec::with_capacity(TOKEN_TRANSFER_KEY_SIZE);
    key.extend_from_slice(&type_hash[..32]);
    key.extend_from_slice(&encode_desc_block_num(block_num));
    key.extend_from_slice(&encode_desc_tx_idx(tx_idx));
    key
}

/// Decode block_num and tx_idx from a token transfer key. A key of another
/// width, or with a newest-first component that encodes no position, is a
/// corrupt key: re-sync required.
pub fn decode_token_transfer_key(key: &[u8]) -> anyhow::Result<(i64, i32)> {
    anyhow::ensure!(
        key.len() == TOKEN_TRANSFER_KEY_SIZE,
        "token_transfers key 0x{} has {} bytes, expected {TOKEN_TRANSFER_KEY_SIZE}: re-sync required",
        hex::encode(key),
        key.len()
    );
    Ok((
        decode_desc_block_num(&key[32..40])?,
        decode_desc_tx_idx(&key[40..44])?,
    ))
}

/// Cluster owner key: prefix(1B) + cluster_id(32B) + lock_hash(32B) = 65 bytes
/// Stored in the stats_spore CF. Value is i64 LE (live spore count for this owner).
pub const CLUSTER_OWNER_KEY_SIZE: usize = 65;

pub fn encode_cluster_owner_key(
    cluster_id: &[u8],
    lock_hash: &[u8],
) -> [u8; CLUSTER_OWNER_KEY_SIZE] {
    assert_key_component_len(
        "encode_cluster_owner_key",
        "cluster_id",
        cluster_id.len(),
        HASH32_LEN,
    );
    assert_key_component_len(
        "encode_cluster_owner_key",
        "lock_hash",
        lock_hash.len(),
        HASH32_LEN,
    );
    let mut key = [0u8; CLUSTER_OWNER_KEY_SIZE];
    key[0] = STATS_PREFIX_CLUSTER_OWNER;
    key[1..33].copy_from_slice(&cluster_id[..32]);
    key[33..65].copy_from_slice(&lock_hash[..32]);
    key
}

/// Prefix for scanning all owners of a given cluster.
pub fn encode_cluster_owner_prefix(cluster_id: &[u8]) -> [u8; 33] {
    assert_key_component_len(
        "encode_cluster_owner_prefix",
        "cluster_id",
        cluster_id.len(),
        HASH32_LEN,
    );
    let mut prefix = [0u8; 33];
    prefix[0] = STATS_PREFIX_CLUSTER_OWNER;
    prefix[1..33].copy_from_slice(&cluster_id[..32]);
    prefix
}

/// Spore-by-cluster key: cluster_id(32B) + spore_id(32B) = 64 bytes
pub fn encode_spore_by_cluster_key(cluster_id: &[u8], spore_id: &[u8]) -> [u8; 64] {
    assert_key_component_len(
        "encode_spore_by_cluster_key",
        "cluster_id",
        cluster_id.len(),
        HASH32_LEN,
    );
    assert_key_component_len(
        "encode_spore_by_cluster_key",
        "spore_id",
        spore_id.len(),
        HASH32_LEN,
    );
    let mut key = [0u8; 64];
    key[..32].copy_from_slice(&cluster_id[..32]);
    key[32..64].copy_from_slice(&spore_id[..32]);
    key
}

/// `activities` (`CF_TX_ACTIONS`) key: the transaction's position,
/// block_num_desc(8B BE) + tx_idx_desc(4B BE). Its hash is `tx_index` there.
pub const TX_ACTIONS_KEY_SIZE: usize = 12;

pub fn encode_tx_actions_key(block_num: i64, tx_idx: i32) -> [u8; TX_ACTIONS_KEY_SIZE] {
    let mut key = [0u8; TX_ACTIONS_KEY_SIZE];
    key[..8].copy_from_slice(&encode_desc_block_num(block_num));
    key[8..].copy_from_slice(&encode_desc_tx_idx(tx_idx));
    key
}

/// The seek key for the first `activities` row strictly after (older than)
/// the position `(block_num, tx_idx)`: the row key plus one byte.
pub fn encode_tx_actions_seek_after_key(block_num: i64, tx_idx: i32) -> Vec<u8> {
    let mut key = encode_tx_actions_key(block_num, tx_idx).to_vec();
    key.push(0);
    key
}

/// `(block_num, tx_idx)` of an `activities` key read back from the store.
/// Only 12-byte keys are written; any other width — the 44-byte layout that
/// also held the tx hash included — is refused: re-sync required.
pub fn decode_stored_tx_actions_key(key: &[u8]) -> anyhow::Result<(i64, i32)> {
    anyhow::ensure!(
        key.len() == TX_ACTIONS_KEY_SIZE,
        "activities key 0x{} has {} bytes, expected {} (block, tx_idx): written by an older \
         layout, re-sync required",
        hex::encode(key),
        key.len(),
        TX_ACTIONS_KEY_SIZE
    );
    Ok((
        decode_desc_block_num(&key[0..8])?,
        decode_desc_tx_idx(&key[8..12])?,
    ))
}

/// Global activity bucket index key:
/// bucket(1B) + block_num_desc(8B BE) + tx_idx_desc(4B BE). Value is empty.
pub const TX_ACTIONS_BY_BUCKET_KEY_SIZE: usize = 13;

pub fn encode_tx_actions_by_bucket_key(
    bucket: crate::global_bucket::GlobalBucket,
    block_num: i64,
    tx_idx: i32,
) -> [u8; TX_ACTIONS_BY_BUCKET_KEY_SIZE] {
    let mut key = [0u8; TX_ACTIONS_BY_BUCKET_KEY_SIZE];
    key[0] = bucket.as_byte();
    key[1..9].copy_from_slice(&encode_desc_block_num(block_num));
    key[9..13].copy_from_slice(&encode_desc_tx_idx(tx_idx));
    key
}

/// `(bucket, block_num, tx_idx)` of a bucket index key. A key of the wrong
/// width or with an unknown bucket byte is a corrupt row.
pub fn decode_tx_actions_by_bucket_key(
    key: &[u8],
) -> anyhow::Result<(crate::global_bucket::GlobalBucket, i64, i32)> {
    if key.len() != TX_ACTIONS_BY_BUCKET_KEY_SIZE {
        anyhow::bail!(
            "decode_tx_actions_by_bucket_key: expected {} bytes, got {} (key=0x{})",
            TX_ACTIONS_BY_BUCKET_KEY_SIZE,
            key.len(),
            crate::bytes_to_hex(key)
        );
    }
    let bucket = crate::global_bucket::GlobalBucket::from_byte(key[0])?;
    Ok((
        bucket,
        decode_desc_block_num(&key[1..9])?,
        decode_desc_tx_idx(&key[9..13])?,
    ))
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

pub fn decode_dao_by_block_key(key: &[u8]) -> anyhow::Result<(i64, Vec<u8>, i16)> {
    anyhow::ensure!(
        key.len() == DAO_BY_BLOCK_KEY_SIZE,
        "dao_by_block key 0x{} has {} bytes, expected {DAO_BY_BLOCK_KEY_SIZE}: re-sync required",
        hex::encode(key),
        key.len()
    );
    let block_num = decode_desc_block_num(&key[..8])?;
    let (tx_hash, output_index) = decode_outpoint(&key[8..42]);
    Ok((block_num, tx_hash, output_index))
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

#[allow(clippy::type_complexity)]
pub fn decode_dao_by_lock_block_key(key: &[u8]) -> anyhow::Result<(Vec<u8>, i64, Vec<u8>, i16)> {
    anyhow::ensure!(
        key.len() == DAO_BY_LOCK_BLOCK_KEY_SIZE,
        "dao_by_lock_block key 0x{} has {} bytes, expected {DAO_BY_LOCK_BLOCK_KEY_SIZE}: re-sync \
         required",
        hex::encode(key),
        key.len()
    );
    let lock_hash = key[..32].to_vec();
    let block_num = decode_desc_block_num(&key[32..40])?;
    let (tx_hash, output_index) = decode_outpoint(&key[40..74]);
    Ok((lock_hash, block_num, tx_hash, output_index))
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

pub fn decode_dao_by_status_block_key(key: &[u8]) -> anyhow::Result<(i16, i64, Vec<u8>, i16)> {
    anyhow::ensure!(
        key.len() == DAO_BY_STATUS_BLOCK_KEY_SIZE,
        "dao_by_status_block key 0x{} has {} bytes, expected {DAO_BY_STATUS_BLOCK_KEY_SIZE}: \
         re-sync required",
        hex::encode(key),
        key.len()
    );
    let status = i16::from_be_bytes(key[..2].try_into().unwrap());
    let block_num = decode_desc_block_num(&key[2..10])?;
    let (tx_hash, output_index) = decode_outpoint(&key[10..44]);
    Ok((status, block_num, tx_hash, output_index))
}

pub fn encode_dao_by_status_prefix(status: i16) -> [u8; 2] {
    status.to_be_bytes()
}

/// Encode the canonical chain-time statistics day as YYYYMMDD (UTC+8).
/// Parser-stage provisional keys may be built from raw time, but must be
/// remapped by block number after the writer determines canonical chain time.
pub fn timestamp_ms_to_date(timestamp_ms: i64) -> u32 {
    let date = ckbadger_common::stats_bucket::stats_day(timestamp_ms)
        .unwrap_or_else(|error| panic!("invalid statistics chain time {timestamp_ms}: {error}"));
    let s = date.format("%Y%m%d").to_string();
    s.parse::<u32>()
        .expect("timestamp_ms_to_date: formatted date must parse into u32")
}

/// The UTC+8 stats day of a chain time as `YYYYMMDD` — the day number every
/// day-keyed row uses (`stats_day`), failing instead of panicking on a chain
/// time no date can represent.
pub fn stats_day_yyyymmdd(chain_time_ms: i64) -> anyhow::Result<u32> {
    use chrono::Datelike;
    let day = ckbadger_common::stats_bucket::stats_day(chain_time_ms)?;
    let year = u32::try_from(day.year())
        .map_err(|_| anyhow::anyhow!("stats day before year 0: chain_time_ms={chain_time_ms}"))?;
    Ok(year * 10_000 + day.month() * 100 + day.day())
}

/// `collection_activity_daily` key: collection_id (32B, padded like the
/// collection activity keys) + UTC+8 day `YYYYMMDD` (u32 BE) = 36 bytes, so a
/// collection's days sort ascending under its prefix.
pub const COLLECTION_ACTIVITY_DAILY_KEY_SIZE: usize = 36;

pub fn encode_collection_activity_daily_key(
    collection_id: &[u8],
    day_yyyymmdd: u32,
) -> [u8; COLLECTION_ACTIVITY_DAILY_KEY_SIZE] {
    let mut key = [0u8; COLLECTION_ACTIVITY_DAILY_KEY_SIZE];
    key[..32].copy_from_slice(&pad_id_32(collection_id));
    key[32..].copy_from_slice(&day_yyyymmdd.to_be_bytes());
    key
}

pub fn decode_collection_activity_daily_key(key: &[u8]) -> anyhow::Result<([u8; 32], u32)> {
    anyhow::ensure!(
        key.len() == COLLECTION_ACTIVITY_DAILY_KEY_SIZE,
        "collection_activity_daily key has {} bytes, expected {}: key=0x{}",
        key.len(),
        COLLECTION_ACTIVITY_DAILY_KEY_SIZE,
        crate::bytes_to_hex(key)
    );
    let mut collection_id = [0u8; 32];
    collection_id.copy_from_slice(&key[..32]);
    let day = u32::from_be_bytes(key[32..].try_into().expect("4-byte day"));
    Ok((collection_id, day))
}

/// Object / identity collection activity key:
/// collection_id(32B padded) + block_num_desc(8B BE) + tx_idx_desc(4B BE) = 44 bytes.
/// Descending block and tx index, so a prefix scan yields the newest first; the
/// transaction is its position (hash and block hash: `tx_index` / `block_headers`).
pub const OBJECT_COLLECTION_ACTIVITY_KEY_SIZE: usize = 44;

pub fn encode_object_collection_activity_key(
    collection_id: &[u8],
    block_num: i64,
    tx_idx: i32,
) -> [u8; OBJECT_COLLECTION_ACTIVITY_KEY_SIZE] {
    // `collection_id` is padded (mNFT class IDs are 24B, `.bit` IDs 20B).
    let mut key = [0u8; OBJECT_COLLECTION_ACTIVITY_KEY_SIZE];
    key[..32].copy_from_slice(&pad_id_32(collection_id));
    key[32..40].copy_from_slice(&encode_desc_block_num(block_num));
    key[40..44].copy_from_slice(&encode_desc_tx_idx(tx_idx));
    key
}

pub fn encode_object_collection_activity_prefix(collection_id: &[u8]) -> [u8; 32] {
    pad_id_32(collection_id)
}

/// The seek key for the first row of `collection_id` strictly after (older
/// than) the position `(block_num, tx_idx)`: the row key plus one byte.
pub fn encode_object_collection_activity_seek_after_key(
    collection_id: &[u8],
    block_num: i64,
    tx_idx: i32,
) -> Vec<u8> {
    let mut key = encode_object_collection_activity_key(collection_id, block_num, tx_idx).to_vec();
    key.push(0);
    key
}

/// `(collection_id, block_num, tx_idx)` of a collection activity key. Any
/// other width — the 108-byte layout that also held the block and tx hashes
/// included — is refused: re-sync required.
pub fn decode_object_collection_activity_key(key: &[u8]) -> anyhow::Result<([u8; 32], i64, i32)> {
    anyhow::ensure!(
        key.len() == OBJECT_COLLECTION_ACTIVITY_KEY_SIZE,
        "collection activity key 0x{} has {} bytes, expected {} (collection, block, tx_idx): \
         written by an older layout, re-sync required",
        hex::encode(key),
        key.len(),
        OBJECT_COLLECTION_ACTIVITY_KEY_SIZE
    );
    let mut collection_id = [0u8; 32];
    collection_id.copy_from_slice(&key[..32]);
    Ok((
        collection_id,
        decode_desc_block_num(&key[32..40])?,
        decode_desc_tx_idx(&key[40..44])?,
    ))
}

/// A transaction's canonical slot, `block_number (8B BE) ‖ tx_idx (4B BE)`:
/// the key of `tx_index` and of `tx_group_cycles`.
pub fn encode_tx_slot_key(block_number: i64, tx_idx: i32) -> Vec<u8> {
    encode_composite(&[&encode_block_num(block_number), &encode_tx_idx(tx_idx)])
}

/// Sync meta keys
/// The fixed keys of `sync_meta` (C-4b: nothing else is stored there; the
/// reorg-event history is `reorg_events`). The `derive:` and
/// `hourly_retention_state:` keys are closed families: one key per derived
/// aggregate / per hourly retention family.
pub mod sync_meta_keys {
    /// `SyncChainStatus`: tip and totals, journaled per block.
    pub const SYNC_STATUS: &[u8] = b"sync_status";
    /// `SyncRuntimeStatus`: run telemetry, bulk completion and deep-fork
    /// state, written outside the journal.
    pub const SYNC_RUNTIME: &[u8] = b"sync_runtime";
    pub const RUNTIME_STATUS: &[u8] = b"runtime_status";
    /// Automatic fork event awaiting canonical replay and reversal reconciliation.
    pub const REORG_RECONCILE_PENDING: &[u8] = b"reorg_reconcile_pending";
    /// The HODL wave tracker's scalars (bincode `HodlTrackerCursor`): holder
    /// count, open snapshot day, last processed block. Its per-day capacity
    /// is `CF_HODL_CAPACITY_BY_DATE`.
    pub const HODL_CURSOR: &[u8] = b"hodl_cursor";
    /// The cell-distribution tracker's scalars (bincode
    /// `CellDistTrackerCursor`): size-bucket counts and capacities, open
    /// snapshot day, last processed block. Its cohort accumulator is
    /// `CF_CELL_DIST_COHORT_BY_MONTH`.
    pub const CELL_DIST_CURSOR: &[u8] = b"cell_dist_cursor";
    pub const SYNC_PROGRESS: &[u8] = b"sync_progress";
    pub const MEMORY_STATS: &[u8] = b"memory_stats";
    pub const BULK_BUILD_SESSION_IN_PROGRESS: &[u8] = b"bulk_build_session_in_progress";
    pub const BACKGROUND_TASKS: &[u8] = b"background_tasks";
    /// Chain-network tag ("mainnet"/"testnet") the DB was first synced for.
    pub const NETWORK_IDENTITY: &[u8] = b"network_identity";
    /// Genesis economic baseline (bincode `GenesisBaseline`), derived at block 0.
    pub const GENESIS_BASELINE: &[u8] = b"genesis_baseline";
    /// Consensus secondary issuance per epoch in shannons (LE u64), fetched
    /// from the node's `get_consensus` at indexer startup. Required by the
    /// per-block miner-secondary split `floor(s_i * U_{i-1} / C_{i-1})`.
    pub const SECONDARY_EPOCH_REWARD: &[u8] = b"secondary_epoch_reward";
    /// API-visible live-cell summary at the latest fully committed canonical
    /// block. The value is a fixed-width bincode `LiveCellSummary`.
    pub const LIVE_CELL_SUMMARY_CURRENT: &[u8] = b"live_cell_summary:current";
    /// Presence marker distinguishing a pre-feature/rebuild-required store
    /// from corruption that removed both current and history records.
    pub const LIVE_CELL_SUMMARY_INITIALIZED: &[u8] = b"live_cell_summary:initialized";
    /// Entity-stats rollback coverage contract (bincode
    /// `EntityStatsUndoContract`). Its absence on a non-empty store means the
    /// store predates per-block entity-stats undo and cannot be rolled back
    /// correctly — the indexer refuses to write to it.
    pub const ENTITY_STATS_UNDO_CONTRACT: &[u8] = b"entity_stats_undo_contract";
    /// Per-family hourly retention state (bincode `HourlyRetentionState`).
    /// The family name is appended: `hourly_retention_state:token`.
    pub const HOURLY_RETENTION_STATE_PREFIX: &[u8] = b"hourly_retention_state:";
    /// One derived aggregate's derivation state (bincode `DeriveMeta`). The
    /// aggregate name is appended: `derive:dao_live_agg`.
    pub const DERIVE_META_PREFIX: &[u8] = b"derive:";
    /// The UTC+8 activity hour (`%Y%m%d%H`, 10 ASCII digits) of the rollback
    /// coverage floor block the activity address rows were last pruned to:
    /// no `ACTIVITY_HOURLY_ADDR_ROW` row of an earlier hour, and no
    /// `ACTIVITY_DAILY_ADDR_ROW` row of an earlier day (the hour's first 8
    /// digits), remains (C-3). Written by the live batch that prunes, in its
    /// block's scope (journaled), so a rollback of that block restores the
    /// rows and this bound together.
    pub const ACTIVITY_ADDR_ROW_FLOOR: &[u8] = b"activity_addr_row_floor";
}

/// `sync_meta` key of one derived aggregate's `DeriveMeta`.
pub fn encode_derive_meta_key(aggregate: &str) -> Vec<u8> {
    let mut key = sync_meta_keys::DERIVE_META_PREFIX.to_vec();
    key.extend_from_slice(aggregate.as_bytes());
    key
}

/// `sync_meta` key for one hourly family's retention state.
pub fn encode_hourly_retention_state_key(family: &str) -> Vec<u8> {
    let mut key = sync_meta_keys::HOURLY_RETENTION_STATE_PREFIX.to_vec();
    key.extend_from_slice(family.as_bytes());
    key
}

// -- Reorg event history (CF_SYNC_META) --

/// Reorg-event history key: `reorg:<13-digit ms>:<32 hex chars>` (52 bytes).
///
/// The millisecond field is zero-padded to a fixed width and the uuid segment
/// is fixed-length hex, so lexicographic key order **is** chronological order.
/// The listing path pages through this range with a keyset cursor and depends
/// on that equivalence; `decode_reorg_event_key` enforces the shape on read so
/// a key that would break the ordering fails loudly instead of silently
/// reordering the history.
pub const REORG_EVENT_KEY_PREFIX: &[u8] = b"reorg:";
/// Exclusive upper bound of the history range. `;` is `:` + 1, so the range
/// `[reorg:, reorg;)` holds exactly the history keys and cannot wander into
/// other sync-meta keys that merely start with `reorg` (`_` sorts above `;`).
pub const REORG_EVENT_KEY_PREFIX_END: &[u8] = b"reorg;";
/// Fixed width of the millisecond field; covers 1970-01-01 .. 2286-11-20.
pub const REORG_EVENT_MS_DIGITS: usize = 13;
/// Fixed width of the uuid-v4 hex field (16 bytes).
pub const REORG_EVENT_UUID_HEX_LEN: usize = 32;
pub const REORG_EVENT_KEY_SIZE: usize = 6 + REORG_EVENT_MS_DIGITS + 1 + REORG_EVENT_UUID_HEX_LEN;
/// Exclusive upper bound of the millisecond field, set by its fixed width.
const REORG_EVENT_MS_LIMIT: i64 = 10_000_000_000_000;

/// Build the history key for one reorg observation.
///
/// Fails when the host clock is outside the representable window rather than
/// emitting a key whose width breaks the ordering invariant.
pub fn encode_reorg_event_key(detected_at_ms: i64, uuid: &[u8; 16]) -> anyhow::Result<String> {
    if !(0..REORG_EVENT_MS_LIMIT).contains(&detected_at_ms) {
        anyhow::bail!(
            "reorg event timestamp outside representable key range: detected_at_ms={}, allowed=0..{}",
            detected_at_ms,
            REORG_EVENT_MS_LIMIT
        );
    }
    Ok(format!(
        "reorg:{:0width$}:{}",
        detected_at_ms,
        crate::bytes_to_hex(uuid),
        width = REORG_EVENT_MS_DIGITS
    ))
}

/// Millisecond prefix shared by every key of one observation instant.
pub fn reorg_event_key_ms_prefix(detected_at_ms: i64) -> anyhow::Result<String> {
    if !(0..REORG_EVENT_MS_LIMIT).contains(&detected_at_ms) {
        anyhow::bail!(
            "reorg event timestamp outside representable key range: detected_at_ms={}, allowed=0..{}",
            detected_at_ms,
            REORG_EVENT_MS_LIMIT
        );
    }
    Ok(format!(
        "reorg:{:0width$}:",
        detected_at_ms,
        width = REORG_EVENT_MS_DIGITS
    ))
}

/// Parse the detection millisecond out of a history key, enforcing the exact
/// key shape that makes lexicographic order chronological.
pub fn decode_reorg_event_key(key: &[u8]) -> anyhow::Result<i64> {
    if key.len() != REORG_EVENT_KEY_SIZE || !key.starts_with(REORG_EVENT_KEY_PREFIX) {
        anyhow::bail!(
            "malformed reorg event key: expected {} bytes of `reorg:<{}-digit ms>:<{} hex>`, got 0x{}",
            REORG_EVENT_KEY_SIZE,
            REORG_EVENT_MS_DIGITS,
            REORG_EVENT_UUID_HEX_LEN,
            crate::bytes_to_hex(key)
        );
    }
    let ms_end = REORG_EVENT_KEY_PREFIX.len() + REORG_EVENT_MS_DIGITS;
    let ms = &key[REORG_EVENT_KEY_PREFIX.len()..ms_end];
    let uuid = &key[ms_end + 1..];
    if key[ms_end] != b':'
        || !ms.iter().all(u8::is_ascii_digit)
        || !uuid.iter().all(u8::is_ascii_hexdigit)
    {
        anyhow::bail!(
            "malformed reorg event key: expected `reorg:<{}-digit ms>:<{} hex>`, got 0x{}",
            REORG_EVENT_MS_DIGITS,
            REORG_EVENT_UUID_HEX_LEN,
            crate::bytes_to_hex(key)
        );
    }
    std::str::from_utf8(ms)
        .expect("ascii digits are valid utf8")
        .parse::<i64>()
        .map_err(|e| {
            anyhow::anyhow!(
                "invalid millisecond field in reorg event key 0x{}: {}",
                crate::bytes_to_hex(key),
                e
            )
        })
}

// -- Fiber Channels --

pub const FIBER_CHANNEL_KEY_SIZE: usize = 32;

/// # Invariant
///
/// The channel ID is the hash of the funding outpoint, so `funding_tx_hash`
/// must be exactly 32 bytes: a different width silently yields a different
/// channel identity rather than a truncated key, which is just as wrong and
/// far harder to spot.
pub fn encode_fiber_channel_id(funding_tx_hash: &[u8], output_index: u32) -> Vec<u8> {
    assert_key_component_len(
        "encode_fiber_channel_id",
        "funding_tx_hash",
        funding_tx_hash.len(),
        HASH32_LEN,
    );
    use ckb_hash::new_blake2b;
    let mut hasher = new_blake2b();
    hasher.update(funding_tx_hash);
    hasher.update(&output_index.to_le_bytes());
    let mut hash = vec![0u8; 32];
    hasher.finalize(&mut hash);
    hash
}

pub const FIBER_OUTPOINT_SIZE: usize = 36;

/// # Invariant
///
/// `tx_hash` must be exactly 32 bytes so the encoding is exactly
/// [`FIBER_OUTPOINT_SIZE`]. Any other width shifts the trailing index field,
/// and [`decode_fiber_outpoint`] would read a different tx hash and index back
/// out of it.
pub fn encode_fiber_outpoint(tx_hash: &[u8], output_index: u32) -> Vec<u8> {
    assert_key_component_len(
        "encode_fiber_outpoint",
        "tx_hash",
        tx_hash.len(),
        HASH32_LEN,
    );
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
    assert_key_component_len(
        "encode_addr_fiber_channel_key",
        "lock_hash",
        lock_hash.len(),
        HASH32_LEN,
    );
    assert_key_component_len(
        "encode_addr_fiber_channel_key",
        "channel_id",
        channel_id.len(),
        HASH32_LEN,
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

    /// C-2 (narrowed in the fix round, findings 7 + 9): code is ELF data and
    /// nothing else — a Type ID cell whose data is not ELF is state.
    #[test]
    fn is_code_cell_is_elf_data_only() {
        let elf = b"\x7fELF\x02\x01\x01rest";
        assert!(is_code_cell(elf), "an ELF binary is code");
        assert!(is_code_cell(&ELF_MAGIC), "the magic alone starts a binary");
        assert!(
            !is_code_cell(b"{\"json\":1}"),
            "state data is not code, under any type script"
        );
        assert!(
            !is_code_cell(&[0x7f, b'E', b'L']),
            "a truncated magic is not code"
        );
        assert!(
            !is_code_cell(&100u128.to_le_bytes()),
            "a UDT amount is not code"
        );
        assert!(!is_code_cell(&[]), "an empty cell is never code");
    }

    #[test]
    fn is_type_id_script_requires_the_exact_code_hash_and_hash_type_type() {
        let mut type_id = [0u8; 32];
        type_id[25..].copy_from_slice(b"TYPE_ID");
        assert!(is_type_id_script(&type_id, 1));
        assert!(!is_type_id_script(&type_id, 0));
        assert!(!is_type_id_script(&type_id, 2));
        assert!(!is_type_id_script(&type_id[1..], 1));
        let mut other = type_id;
        other[0] = 1;
        assert!(!is_type_id_script(&other, 1));
        // The constant readers build Type ID scripts from is the one the rule
        // recognises: the consensus hex `0x00…00545950455f4944`.
        assert_eq!(TYPE_ID_CODE_HASH, type_id);
        assert_eq!(
            hex::encode(TYPE_ID_CODE_HASH),
            "00000000000000000000000000000000000000000000000000545950455f4944"
        );
        assert!(is_type_id_script(&TYPE_ID_CODE_HASH, 1));
    }

    #[test]
    fn test_reorg_event_key_roundtrip_and_shape() {
        let key = encode_reorg_event_key(1_700_000_123_456, &[0xAB; 16]).unwrap();
        assert_eq!(key.len(), REORG_EVENT_KEY_SIZE);
        assert_eq!(key, format!("reorg:1700000123456:{}", "ab".repeat(16)));
        assert_eq!(
            decode_reorg_event_key(key.as_bytes()).unwrap(),
            1_700_000_123_456
        );
        assert!(key.as_bytes().starts_with(REORG_EVENT_KEY_PREFIX));
        assert!(key.as_bytes() < REORG_EVENT_KEY_PREFIX_END);
    }

    /// Zero padding is what makes lexicographic key order chronological, which
    /// the history listing and its keyset cursor depend on.
    #[test]
    fn test_reorg_event_key_pads_millisecond_field() {
        let key = encode_reorg_event_key(42, &[0x00; 16]).unwrap();
        assert_eq!(&key[..19], "reorg:0000000000042");
        let later = encode_reorg_event_key(1_700_000_000_000, &[0x00; 16]).unwrap();
        assert!(key < later);
    }

    #[test]
    fn test_reorg_event_key_rejects_unrepresentable_timestamps() {
        for ms in [-1i64, 10_000_000_000_000, i64::MAX] {
            let err = encode_reorg_event_key(ms, &[0u8; 16]).unwrap_err();
            assert!(
                err.to_string()
                    .contains("reorg event timestamp outside representable key range"),
                "unexpected error for {ms}: {err}"
            );
            assert!(reorg_event_key_ms_prefix(ms).is_err());
        }
    }

    #[test]
    fn test_decode_reorg_event_key_rejects_malformed_keys() {
        let valid = encode_reorg_event_key(1_700_000_123_456, &[0xAB; 16]).unwrap();
        for bad in [
            b"reorg:1:ab".to_vec(),
            b"reorg_latest_event".to_vec(),
            valid.as_bytes()[..valid.len() - 1].to_vec(),
            // right length, non-digit millisecond field
            format!("reorg:17000001234x6:{}", "ab".repeat(16)).into_bytes(),
            // right length, non-hex uuid field
            format!("reorg:1700000123456:{}z", "ab".repeat(15) + "a").into_bytes(),
        ] {
            let err = decode_reorg_event_key(&bad).unwrap_err();
            assert!(
                err.to_string().contains("malformed reorg event key"),
                "unexpected error for {:?}: {err}",
                String::from_utf8_lossy(&bad)
            );
        }
    }

    #[test]
    fn test_reorg_event_key_ms_prefix_matches_full_key() {
        let prefix = reorg_event_key_ms_prefix(1_700_000_123_456).unwrap();
        let key = encode_reorg_event_key(1_700_000_123_456, &[0x01; 16]).unwrap();
        assert!(key.starts_with(&prefix));
    }

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
    fn test_cell_code_index_key_layout() {
        let code_hash = [0x9b; 32];
        let tx_hash = [0xab; 32];
        let key = encode_cell_code_index_key(&code_hash, 1, 123, &tx_hash, 7);

        assert_eq!(key.len(), CELL_CODE_INDEX_KEY_SIZE);
        assert_eq!(&key[..32], &code_hash);
        assert_eq!(key[32], 1, "hash_type byte sits directly after code_hash");
        assert_eq!(&key[33..41], &123i64.to_be_bytes());
        assert_eq!(&key[41..73], &tx_hash);
        assert_eq!(&key[73..75], &7i16.to_be_bytes());

        let prefix = encode_cell_code_index_prefix(&code_hash, 1);
        assert_eq!(prefix.len(), CELL_CODE_INDEX_PREFIX_SIZE);
        assert!(key.starts_with(&prefix));
    }

    #[test]
    fn test_cell_code_index_key_forms_never_interleave() {
        // Same code hash bytes with different hash_types must be disjoint
        // contiguous key ranges: every key of the lower form sorts strictly
        // before every key of the higher form, regardless of block numbers.
        let code_hash = [0x9b; 32];
        let max_data_form =
            encode_cell_code_index_key(&code_hash, 0, i64::MAX, &[0xff; 32], i16::MAX);
        let min_type_form = encode_cell_code_index_key(&code_hash, 1, 0, &[0x00; 32], 0);
        assert!(max_data_form < min_type_form);

        let data_prefix = encode_cell_code_index_prefix(&code_hash, 0);
        let type_prefix = encode_cell_code_index_prefix(&code_hash, 1);
        assert!(max_data_form.starts_with(&data_prefix));
        assert!(!max_data_form.starts_with(&type_prefix));
        assert!(min_type_form.starts_with(&type_prefix));
        assert!(!min_type_form.starts_with(&data_prefix));
    }

    /// A 33-byte code hash used to be accepted and silently truncated to its
    /// first 32 bytes, producing the prefix of a *different* script reference
    /// form. The encoder now enforces the exact-length invariant.
    #[test]
    #[should_panic(expected = "code_hash must be exactly 32 bytes, got 33")]
    fn test_cell_code_index_prefix_rejects_oversized_code_hash() {
        encode_cell_code_index_prefix(&[0x9b; 33], 1);
    }

    #[test]
    #[should_panic(expected = "code_hash must be exactly 32 bytes, got 31")]
    fn test_cell_code_index_prefix_rejects_undersized_code_hash() {
        encode_cell_code_index_prefix(&[0x9b; 31], 1);
    }

    #[test]
    #[should_panic(expected = "code_hash must be exactly 32 bytes, got 33")]
    fn test_cell_code_index_key_rejects_oversized_code_hash() {
        encode_cell_code_index_key(&[0x9b; 33], 1, 123, &[0xab; 32], 0);
    }

    #[test]
    #[should_panic(expected = "tx_hash must be exactly 32 bytes, got 33")]
    fn test_cell_code_index_key_rejects_oversized_tx_hash() {
        encode_cell_code_index_key(&[0x9b; 32], 1, 123, &[0xab; 33], 0);
    }

    #[test]
    #[should_panic(expected = "code_hash must be exactly 32 bytes, got 31")]
    fn test_cell_code_index_key_rejects_undersized_code_hash() {
        encode_cell_code_index_key(&[0x9b; 31], 1, 123, &[0xab; 32], 0);
    }

    #[test]
    fn test_cell_code_index_hash_type_byte_conversion() {
        assert_eq!(cell_code_index_hash_type_byte(0).unwrap(), 0);
        assert_eq!(cell_code_index_hash_type_byte(1).unwrap(), 1);
        assert_eq!(cell_code_index_hash_type_byte(2).unwrap(), 2);
        assert_eq!(cell_code_index_hash_type_byte(4).unwrap(), 4);
        assert!(cell_code_index_hash_type_byte(-1).is_err());
        assert!(cell_code_index_hash_type_byte(256).is_err());
    }

    #[test]
    fn test_dao_by_block_key_roundtrip() {
        let outpoint = encode_outpoint(&[0xAA; 32], 3);
        let key = encode_dao_by_block_key(123, &outpoint);
        assert_eq!(key.len(), DAO_BY_BLOCK_KEY_SIZE);
        let (block, tx_hash, output_index) = decode_dao_by_block_key(&key).unwrap();
        assert_eq!(block, 123);
        assert_eq!(tx_hash, vec![0xAA; 32]);
        assert_eq!(output_index, 3);
    }

    #[test]
    fn test_dao_by_lock_block_key_roundtrip() {
        let outpoint = encode_outpoint(&[0xBB; 32], 5);
        let key = encode_dao_by_lock_block_key(&[0x11; 32], 999, &outpoint);
        assert_eq!(key.len(), DAO_BY_LOCK_BLOCK_KEY_SIZE);
        let (lock_hash, block, tx_hash, output_index) = decode_dao_by_lock_block_key(&key).unwrap();
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
        let (status, block, tx_hash, output_index) = decode_dao_by_status_block_key(&key).unwrap();
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
        assert_eq!(key[0], object_outpoint_kind::SPORE_ITEM);
        let (decoded_tx_hash, decoded_output_index) = decode_spore_outpoint_key(&key);
        assert_eq!(decoded_tx_hash, tx_hash.to_vec());
        assert_eq!(decoded_output_index, 7);
    }

    #[test]
    fn test_spore_outpoint_by_id_key_roundtrip() {
        let spore_id = [0xCCu8; 32];
        let tx_hash = [0xDDu8; 32];
        let key = encode_spore_outpoint_by_id_key(&spore_id, &tx_hash, 3);
        assert_eq!(key.len(), spore_outpoint_by_id_key_len(spore_id.len()));
        assert_eq!(key.len(), 67);
        assert_eq!(key[0], object_outpoint_kind::SPORE_ITEM);
        assert_eq!(&key[1..33], &spore_id);
        let (decoded_tx_hash, decoded_output_index) = decode_spore_outpoint_by_id_key(&key);
        assert_eq!(decoded_tx_hash, tx_hash.to_vec());
        assert_eq!(decoded_output_index, 3);

        let prefix = encode_spore_outpoint_by_id_prefix(&spore_id);
        assert_eq!(prefix.len(), 33);
        assert!(key.starts_with(&prefix));
    }

    /// did:ckb item ids are the type-script args verbatim and are NOT
    /// fixed-width on chain (390 × 32-byte + 31 × 20-byte on live testnet).
    /// The reverse index therefore encodes a variable-width id between a
    /// 1-byte prefix and a fixed 34-byte outpoint suffix.
    #[test]
    fn test_spore_outpoint_by_id_key_supports_variable_width_ids() {
        let tx_hash = [0xDDu8; 32];

        for id_len in [1usize, 20, 31, 32] {
            let id = vec![0xA7u8; id_len];
            let key = encode_spore_outpoint_by_id_key(&id, &tx_hash, 5);
            assert_eq!(key.len(), 1 + id_len + OUTPOINT_KEY_SIZE);
            assert_eq!(key.len(), spore_outpoint_by_id_key_len(id_len));
            assert_eq!(key[0], object_outpoint_kind::SPORE_ITEM);
            assert_eq!(&key[1..1 + id_len], id.as_slice());

            let prefix = encode_spore_outpoint_by_id_prefix(&id);
            assert_eq!(prefix.len(), 1 + id_len);
            assert!(key.starts_with(&prefix));

            // The outpoint is decoded from the fixed-width suffix, so it stays
            // recoverable no matter how wide the id is.
            let (decoded_tx_hash, decoded_output_index) = decode_spore_outpoint_by_id_key(&key);
            assert_eq!(decoded_tx_hash, tx_hash.to_vec());
            assert_eq!(decoded_output_index, 5);
        }
    }

    /// A short id that is a byte-prefix of a longer id shares the scan prefix,
    /// so keys must remain distinguishable by exact length.
    #[test]
    fn test_spore_outpoint_by_id_short_id_is_length_distinguishable_from_longer_id() {
        let short_id = vec![0x11u8; 20];
        let mut long_id = short_id.clone();
        long_id.extend_from_slice(&[0x11u8; 12]);
        assert_eq!(long_id.len(), 32);
        assert!(long_id.starts_with(&short_id));

        let tx_hash = [0xEEu8; 32];
        let short_key = encode_spore_outpoint_by_id_key(&short_id, &tx_hash, 0);
        let long_key = encode_spore_outpoint_by_id_key(&long_id, &tx_hash, 0);

        // The longer id's rows fall inside the shorter id's scan prefix …
        let short_prefix = encode_spore_outpoint_by_id_prefix(&short_id);
        assert!(long_key.starts_with(&short_prefix));
        // … so only the exact key length separates them.
        assert_ne!(short_key.len(), long_key.len());
        assert_eq!(short_key.len(), spore_outpoint_by_id_key_len(20));
        assert_eq!(long_key.len(), spore_outpoint_by_id_key_len(32));
    }

    #[test]
    #[should_panic(expected = "must be between 1 and 32 bytes")]
    fn test_spore_outpoint_by_id_key_rejects_empty_id() {
        let _ = encode_spore_outpoint_by_id_key(&[], &[0xDDu8; 32], 0);
    }

    #[test]
    #[should_panic(expected = "must be between 1 and 32 bytes")]
    fn test_spore_outpoint_by_id_key_rejects_oversized_id() {
        let _ = encode_spore_outpoint_by_id_key(&[0x01u8; 33], &[0xDDu8; 32], 0);
    }

    #[test]
    #[should_panic(expected = "must be between 1 and 32 bytes")]
    fn test_spore_outpoint_by_id_prefix_rejects_empty_id() {
        let _ = encode_spore_outpoint_by_id_prefix(&[]);
    }

    #[test]
    fn test_mnft_class_outpoint_key_structure() {
        let tx_hash = [0xACu8; 32];
        let key = encode_mnft_class_outpoint_key(&tx_hash, 8);
        assert_eq!(key.len(), MNFT_CLASS_OUTPOINT_KEY_SIZE);
        assert_eq!(key[0], object_outpoint_kind::MNFT_CLASS);
        let (decoded_tx_hash, decoded_output_index) = decode_outpoint(&key[1..35]);
        assert_eq!(decoded_tx_hash, tx_hash.to_vec());
        assert_eq!(decoded_output_index, 8);
    }

    #[test]
    fn test_mnft_token_outpoint_key_structure() {
        let tx_hash = [0xADu8; 32];
        let key = encode_mnft_token_outpoint_key(&tx_hash, 9);
        assert_eq!(key.len(), MNFT_TOKEN_OUTPOINT_KEY_SIZE);
        assert_eq!(key[0], object_outpoint_kind::MNFT_TOKEN);
        let (decoded_tx_hash, decoded_output_index) = decode_outpoint(&key[1..35]);
        assert_eq!(decoded_tx_hash, tx_hash.to_vec());
        assert_eq!(decoded_output_index, 9);
    }

    #[test]
    fn test_dotbit_account_outpoint_key_structure() {
        let tx_hash = [0xAEu8; 32];
        let key = encode_dotbit_account_outpoint_key(&tx_hash, 10);
        assert_eq!(key.len(), DOTBIT_ACCOUNT_OUTPOINT_KEY_SIZE);
        assert_eq!(key[0], object_outpoint_kind::DOTBIT_ACCOUNT);
        let (decoded_tx_hash, decoded_output_index) = decode_dotbit_account_outpoint_key(&key);
        assert_eq!(decoded_tx_hash, tx_hash.to_vec());
        assert_eq!(decoded_output_index, 10);
    }

    #[test]
    fn test_dotbit_unindexed_outpoint_key_structure() {
        let tx_hash = [0xA7u8; 32];
        let key = encode_dotbit_unindexed_outpoint_key(&tx_hash, 3);
        assert_eq!(key.len(), DOTBIT_UNINDEXED_OUTPOINT_KEY_SIZE);
        assert_eq!(key[0], object_outpoint_kind::DOTBIT_UNINDEXED);
        assert_eq!(&key[1..35], encode_outpoint(&tx_hash, 3).as_slice());
        assert_ne!(
            key[0],
            encode_dotbit_account_outpoint_key(&tx_hash, 3)[0],
            "the marker never shares the indexed account kind"
        );
    }

    #[test]
    fn test_dotbit_outpoint_by_account_id_key_roundtrip() {
        let account_id = [0xAFu8; 20];
        let tx_hash = [0xBBu8; 32];
        let key = encode_dotbit_outpoint_by_account_id_key(&account_id, &tx_hash, 5);
        assert_eq!(key.len(), DOTBIT_OUTPOINT_BY_ACCOUNT_ID_KEY_SIZE);
        assert_eq!(key[0], object_outpoint_kind::DOTBIT_ACCOUNT);
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
    fn tx_actions_key_is_the_12_byte_position_and_round_trips() {
        let key = encode_tx_actions_key(123, 7);
        assert_eq!(key.len(), TX_ACTIONS_KEY_SIZE);
        assert_eq!(TX_ACTIONS_KEY_SIZE, 12);
        assert_eq!(decode_stored_tx_actions_key(&key).unwrap(), (123, 7));
        let seek = encode_tx_actions_seek_after_key(123, 7);
        assert!(seek.as_slice() > key.as_slice());
        assert!(seek.as_slice() < encode_tx_actions_key(123, 6).as_slice());
    }

    #[test]
    fn decode_stored_tx_actions_key_refuses_the_44_byte_layout() {
        let mut old = encode_tx_actions_key(123, 7).to_vec();
        old.extend_from_slice(&[0x44; 32]);
        let err = decode_stored_tx_actions_key(&old).unwrap_err().to_string();
        assert!(
            err.contains("has 44 bytes, expected 12") && err.contains("re-sync required"),
            "{err}"
        );
    }

    #[test]
    fn test_tx_actions_key_descending_sort_order() {
        let k1 = encode_tx_actions_key(300, 0);
        let k2 = encode_tx_actions_key(200, 0);
        let k3 = encode_tx_actions_key(100, 5);
        let k4 = encode_tx_actions_key(100, 1);

        assert!(k1 < k2);
        assert!(k2 < k3);
        assert!(k3 < k4);
    }

    // ---- Global activity bucket key ----

    #[test]
    fn test_tx_actions_by_bucket_key_roundtrip() {
        use crate::global_bucket::GlobalBucket;
        for bucket in GlobalBucket::ALL {
            let key = encode_tx_actions_by_bucket_key(bucket, 20_617_704, 13);
            assert_eq!(key.len(), TX_ACTIONS_BY_BUCKET_KEY_SIZE);
            assert_eq!(key[0], bucket.as_byte());
            assert_eq!(
                decode_tx_actions_by_bucket_key(&key).unwrap(),
                (bucket, 20_617_704, 13)
            );
        }
        // The position part is exactly the CF_TX_ACTIONS key.
        let key = encode_tx_actions_by_bucket_key(GlobalBucket::Dao, 7, 2);
        assert_eq!(key[1..], encode_tx_actions_key(7, 2));
    }

    #[test]
    fn test_tx_actions_by_bucket_key_sorts_bucket_then_newest_first() {
        use crate::global_bucket::GlobalBucket;
        let token_new = encode_tx_actions_by_bucket_key(GlobalBucket::Token, 300, 0);
        let token_mid_hi = encode_tx_actions_by_bucket_key(GlobalBucket::Token, 200, 5);
        let token_mid_lo = encode_tx_actions_by_bucket_key(GlobalBucket::Token, 200, 1);
        let token_old = encode_tx_actions_by_bucket_key(GlobalBucket::Token, 0, 0);
        let dao_oldest = encode_tx_actions_by_bucket_key(GlobalBucket::Dao, 0, 0);
        let object_newest = encode_tx_actions_by_bucket_key(GlobalBucket::Object, 900, 9);
        assert!(dao_oldest < token_new);
        assert!(token_new < token_mid_hi);
        assert!(token_mid_hi < token_mid_lo);
        assert!(token_mid_lo < token_old);
        assert!(token_old < object_newest);
    }

    #[test]
    fn test_tx_actions_by_bucket_key_rejects_corrupt_rows() {
        use crate::global_bucket::GlobalBucket;
        let key = encode_tx_actions_by_bucket_key(GlobalBucket::Ckb, 1, 0);
        let err = decode_tx_actions_by_bucket_key(&key[..12]).unwrap_err();
        assert!(
            err.to_string().contains("expected 13 bytes, got 12"),
            "{err}"
        );
        let mut unknown = key;
        unknown[0] = 9;
        let err = decode_tx_actions_by_bucket_key(&unknown).unwrap_err();
        assert!(
            err.to_string()
                .contains("unknown global activity bucket byte: 9"),
            "{err}"
        );
    }

    #[test]
    fn test_script_daily_key_roundtrip() {
        let code_hash = [0x55u8; 32];
        let key = encode_script_daily_key(&code_hash, 2, true, 20250219);
        assert_eq!(key.len(), SCRIPT_DAILY_KEY_SIZE);
        let (decoded_hash, decoded_hash_type, decoded_is_type, decoded_date) =
            decode_script_daily_key(&key);
        assert_eq!(decoded_hash, code_hash.to_vec());
        assert_eq!(decoded_hash_type, 2);
        assert!(decoded_is_type);
        assert_eq!(decoded_date, 20250219);
    }

    #[test]
    fn test_script_daily_key_separates_hash_type_forms() {
        // Same code_hash + kind + date under different hash_types must be
        // distinct rows: the missing hash_type dimension was the root cause of
        // junk data-form capacity leaking into type-form family charts.
        let code_hash = [0x9bu8; 32];
        let type_form = encode_script_daily_key(&code_hash, 1, false, 20240101);
        let data_form = encode_script_daily_key(&code_hash, 0, false, 20240101);
        assert_ne!(type_form, data_form);
    }

    #[test]
    #[should_panic(expected = "script daily key hash_type must be one of 0/1/2/4")]
    fn test_script_daily_key_rejects_invalid_hash_type() {
        let code_hash = [0x55u8; 32];
        let _ = encode_script_daily_key(&code_hash, 3, false, 20240101);
    }

    #[test]
    fn test_script_daily_prefix_is_prefix_of_full_key() {
        let code_hash = [0x42u8; 32];
        let prefix = encode_script_daily_prefix(&code_hash, 0, false);
        let full = encode_script_daily_key(&code_hash, 0, false, 20240101);
        assert_eq!(prefix.len(), 35);
        assert!(full.starts_with(&prefix));

        let code_hash_prefix = encode_script_daily_code_hash_prefix(&code_hash);
        assert_eq!(code_hash_prefix.len(), 33);
        assert!(full.starts_with(&code_hash_prefix));
        assert!(prefix.starts_with(&code_hash_prefix));
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
        let invalid = std::panic::catch_unwind(|| timestamp_ms_to_date(i64::MAX));
        assert!(invalid.is_err(), "invalid chain time must fail immediately");
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
            let key = encode_object_collection_activity_key(&collection_id, block, idx);
            assert_eq!(key.len(), OBJECT_COLLECTION_ACTIVITY_KEY_SIZE);
            assert_eq!(OBJECT_COLLECTION_ACTIVITY_KEY_SIZE, 44);
            assert_eq!(
                decode_object_collection_activity_key(&key).unwrap(),
                (collection_id, block, idx)
            );
        }
    }

    #[test]
    fn decode_object_collection_activity_key_refuses_the_108_byte_layout() {
        let mut old = encode_object_collection_activity_key(&[0xAA; 32], 100, 5).to_vec();
        old.extend_from_slice(&[0x66; 32]);
        old.extend_from_slice(&[0x77; 32]);
        let err = decode_object_collection_activity_key(&old)
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("has 108 bytes, expected 44") && err.contains("re-sync required"),
            "{err}"
        );
    }

    #[test]
    fn test_object_collection_activity_key_descending_sort() {
        let cid = [0xBBu8; 32];
        let k1 = encode_object_collection_activity_key(&cid, 300, 5);
        let k2 = encode_object_collection_activity_key(&cid, 200, 5);
        let k3 = encode_object_collection_activity_key(&cid, 100, 5);
        // Higher block_num => smaller key (descending)
        assert!(k1 < k2);
        assert!(k2 < k3);

        // Same block, higher tx_idx => smaller key (descending)
        let k4 = encode_object_collection_activity_key(&cid, 100, 10);
        let k5 = encode_object_collection_activity_key(&cid, 100, 5);
        assert!(k4 < k5);
        // The seek-after key lands strictly between a row and the next older one.
        let seek = encode_object_collection_activity_seek_after_key(&cid, 100, 10);
        assert!(seek.as_slice() > k4.as_slice() && seek.as_slice() < k5.as_slice());
    }

    #[test]
    fn test_object_collection_activity_prefix_matching() {
        let cid = [0xCCu8; 32];
        let prefix = encode_object_collection_activity_prefix(&cid);
        let key = encode_object_collection_activity_key(&cid, 500, 3);
        assert!(key.starts_with(&prefix));

        let other_cid = [0xDDu8; 32];
        let other_key = encode_object_collection_activity_key(&other_cid, 500, 3);
        assert!(!other_key.starts_with(&prefix));
    }

    #[test]
    fn test_object_collection_activity_padded_short_id() {
        let short_id = [0xEEu8; 20];
        let key = encode_object_collection_activity_key(&short_id, 100, 0);
        let (decoded_cid, decoded_block, _) = decode_object_collection_activity_key(&key).unwrap();
        // First 20 bytes match, rest is zero-padded
        assert_eq!(&decoded_cid[..20], &short_id);
        assert_eq!(&decoded_cid[20..], &[0u8; 12]);
        assert_eq!(decoded_block, 100);
    }

    /// Review 20: a newest-first component with the top bit set encodes no
    /// position (the encoders write only non-negative ones); every decoder
    /// of such a key fails "re-sync required" instead of overflowing.
    #[test]
    fn newest_first_keys_with_the_top_bit_set_are_refused() {
        let mut activities = encode_tx_actions_key(5, 1);
        activities[0] = 0x80;
        let mut addr_tx = encode_addr_tx_key(&[0xAA; 32], 5, 1);
        addr_tx[32] = 0x80;
        let mut by_prefix = encode_addr_tx_by_prefix_key(&[0xAA; 20], 5, 1);
        by_prefix[28] = 0x80;
        let mut collection = encode_object_collection_activity_key(&[0xCC; 32], 5, 1).to_vec();
        collection[40] = 0x80;
        let mut bucket =
            encode_tx_actions_by_bucket_key(crate::global_bucket::GlobalBucket::Token, 5, 1)
                .to_vec();
        bucket[1] = 0x80;
        let mut transfer = encode_token_transfer_key(&[0xAA; 32], 5, 1);
        transfer[40] = 0x80;
        let outpoint = encode_outpoint(&[0xBB; 32], 0);
        let mut dao_block = encode_dao_by_block_key(5, &outpoint);
        dao_block[0] = 0x80;
        let mut dao_lock = encode_dao_by_lock_block_key(&[0xAA; 32], 5, &outpoint);
        dao_lock[32] = 0x80;
        let mut dao_status = encode_dao_by_status_block_key(1, 5, &outpoint);
        dao_status[2] = 0x80;
        let errors = [
            decode_token_transfer_key(&transfer).unwrap_err(),
            decode_dao_by_block_key(&dao_block).unwrap_err(),
            decode_dao_by_lock_block_key(&dao_lock).unwrap_err(),
            decode_dao_by_status_block_key(&dao_status).unwrap_err(),
            decode_stored_tx_actions_key(&activities).unwrap_err(),
            decode_addr_tx_key(&addr_tx).unwrap_err(),
            decode_addr_tx_by_prefix_key(&by_prefix).unwrap_err(),
            decode_object_collection_activity_key(&collection).unwrap_err(),
            decode_tx_actions_by_bucket_key(&bucket).unwrap_err(),
        ];
        for (i, error) in errors.iter().enumerate() {
            let error = error.to_string();
            assert!(
                error.contains("a corrupt key, re-sync required"),
                "decoder {i}: {error}"
            );
        }
        assert!(decode_desc_tx_idx(&[0x80, 0, 0, 0])
            .unwrap_err()
            .to_string()
            .contains("encodes no transaction index"));
        assert!(decode_desc_block_num(&[0x80, 0, 0, 0, 0, 0, 0, 0])
            .unwrap_err()
            .to_string()
            .contains("encodes no block"));
        // The extremes of a valid component still decode.
        assert_eq!(decode_desc_block_num(&i64::MAX.to_be_bytes()).unwrap(), 0);
        assert_eq!(
            decode_desc_block_num(&0i64.to_be_bytes()).unwrap(),
            i64::MAX
        );
        assert_eq!(decode_desc_tx_idx(&0i32.to_be_bytes()).unwrap(), i32::MAX);
    }

    #[test]
    fn addr_tx_key_is_the_44_byte_position_and_round_trips() {
        let lock_hash = [0x11u8; 32];
        let key = encode_addr_tx_key(&lock_hash, 100, 3);
        assert_eq!(key.len(), ADDR_TX_KEY_SIZE);
        assert_eq!(ADDR_TX_KEY_SIZE, 44);
        assert_eq!(&key[..32], &lock_hash);
        assert_eq!(decode_addr_tx_key(&key).unwrap(), (lock_hash, 100, 3));
        // Newer positions first; the seek-after key lands strictly after its row.
        assert!(encode_addr_tx_key(&lock_hash, 100, 4) < key);
        assert!(encode_addr_tx_key(&lock_hash, 101, 0) < key);
        let seek = encode_addr_tx_seek_after_key(&lock_hash, 100, 3);
        assert!(seek > key && seek < encode_addr_tx_key(&lock_hash, 100, 2));
    }

    #[test]
    fn decode_addr_tx_key_refuses_the_76_byte_layout() {
        let mut old = encode_addr_tx_key(&[0x11; 32], 100, 3);
        old.extend_from_slice(&[0xAA; 32]);
        let err = decode_addr_tx_key(&old).unwrap_err().to_string();
        assert!(
            err.contains("has 76 bytes, expected 44") && err.contains("re-sync required"),
            "{err}"
        );
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

    // -- fixed-width key component invariants -------------------------------
    //
    // Keys in this module are delimiter-free byte layouts, so an identifier
    // that does not exactly fill its window is an invariant violation in both
    // directions: too short cannot fill the window, too long is truncated into
    // the *next* component and yields a well-formed key belonging to a
    // different entity. The sweeps below prove every encoder that owns a
    // fixed-width window enforces that uniformly and says so actionably.

    /// Capture the panic message `f` produces, or `None` when it returns.
    fn encoder_panic_message(f: impl FnOnce()) -> Option<String> {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(f))
            .err()
            .map(|payload| {
                payload
                    .downcast_ref::<String>()
                    .cloned()
                    .or_else(|| payload.downcast_ref::<&str>().map(|s| (*s).to_string()))
                    .unwrap_or_else(|| "<non-string panic payload>".to_string())
            })
    }

    /// Invoke one encoder with the supplied bytes in the slot under test and
    /// valid values in every other argument.
    type EncoderCall = fn(&[u8]);

    /// One fixed-width identifier of one encoder.
    struct FixedWidthCase {
        encoder: &'static str,
        component: &'static str,
        expected: usize,
        call: EncoderCall,
    }

    /// Every encoder slot in this module that copies an identifier into a
    /// fixed-width key window and owns its own length assert.
    ///
    /// Excluded on purpose:
    /// * delegating encoders (`encode_spore_outpoint_key`, …) — the window is
    ///   owned by [`encode_outpoint`], covered by its own case plus
    ///   `test_delegating_outpoint_encoders_propagate_the_tx_hash_invariant`;
    /// * the DAO family, which already asserts exact widths in its own wording;
    /// * everything routed through [`pad_id_32`], whose identifiers are
    ///   legitimately narrower than 32 bytes (mNFT class 24B / token 28B,
    ///   `.bit` account 20B) — see
    ///   `test_padded_encoders_still_accept_their_variable_width_ids`.
    fn fixed_width_cases() -> Vec<FixedWidthCase> {
        // `call` is a plain fn pointer, so nothing can be captured; the filler
        // hash is a const and the ranked-key cases build their own balance.
        const H: [u8; 32] = [0xAB; 32];
        vec![
            FixedWidthCase {
                encoder: "encode_outpoint",
                component: "tx_hash",
                expected: 32,
                call: |h| {
                    let _ = encode_outpoint(h, 0);
                },
            },
            FixedWidthCase {
                encoder: "encode_mnft_class_outpoint_tx_prefix",
                component: "tx_hash",
                expected: 32,
                call: |h| {
                    let _ = encode_mnft_class_outpoint_tx_prefix(h);
                },
            },
            FixedWidthCase {
                encoder: "encode_cell_index_key",
                component: "script_hash",
                expected: 32,
                call: |h| {
                    let _ = encode_cell_index_key(h, 1, &H, 0);
                },
            },
            FixedWidthCase {
                encoder: "encode_cell_index_key",
                component: "tx_hash",
                expected: 32,
                call: |h| {
                    let _ = encode_cell_index_key(&H, 1, h, 0);
                },
            },
            FixedWidthCase {
                encoder: "encode_cell_code_index_prefix",
                component: "code_hash",
                expected: 32,
                call: |h| {
                    let _ = encode_cell_code_index_prefix(h, 1);
                },
            },
            FixedWidthCase {
                encoder: "encode_cell_code_index_key",
                component: "code_hash",
                expected: 32,
                call: |h| {
                    let _ = encode_cell_code_index_key(h, 1, 1, &H, 0);
                },
            },
            FixedWidthCase {
                encoder: "encode_cell_code_index_key",
                component: "tx_hash",
                expected: 32,
                call: |h| {
                    let _ = encode_cell_code_index_key(&H, 1, 1, h, 0);
                },
            },
            FixedWidthCase {
                encoder: "encode_script_version_by_label_key",
                component: "version_hash",
                expected: 32,
                call: |h| {
                    let _ = encode_script_version_by_label_key("Default Lock", h);
                },
            },
            FixedWidthCase {
                encoder: "encode_script_version_by_family_key",
                component: "version_hash",
                expected: 32,
                call: |h| {
                    let _ = encode_script_version_by_family_key("family/default-lock", h);
                },
            },
            FixedWidthCase {
                encoder: "encode_script_reference_key",
                component: "reference_hash",
                expected: 32,
                call: |h| {
                    let _ = encode_script_reference_key(1, h);
                },
            },
            FixedWidthCase {
                encoder: "encode_addr_tx_key",
                component: "lock_hash",
                expected: 32,
                call: |h| {
                    let _ = encode_addr_tx_key(h, 1, 0);
                },
            },
            FixedWidthCase {
                encoder: "encode_addr_tx_seek_after_key",
                component: "lock_hash",
                expected: 32,
                call: |h| {
                    let _ = encode_addr_tx_seek_after_key(h, 1, 0);
                },
            },
            FixedWidthCase {
                encoder: "encode_addr_tx_block_seek_key",
                component: "lock_hash",
                expected: 32,
                call: |h| {
                    let _ = encode_addr_tx_block_seek_key(h, 1);
                },
            },
            FixedWidthCase {
                encoder: "encode_token_holder_key",
                component: "type_hash",
                expected: 32,
                call: |h| {
                    let _ = encode_token_holder_key(h, &H);
                },
            },
            FixedWidthCase {
                encoder: "encode_token_holder_key",
                component: "lock_hash",
                expected: 32,
                call: |h| {
                    let _ = encode_token_holder_key(&H, h);
                },
            },
            FixedWidthCase {
                encoder: "encode_token_holder_balance_key",
                component: "type_hash",
                expected: 32,
                call: |h| {
                    let _ = encode_token_holder_balance_key(h, &TokenBalance::from(7u128), &H);
                },
            },
            FixedWidthCase {
                encoder: "encode_token_holder_balance_key",
                component: "lock_hash",
                expected: 32,
                call: |h| {
                    let _ = encode_token_holder_balance_key(&H, &TokenBalance::from(7u128), h);
                },
            },
            FixedWidthCase {
                encoder: "encode_addr_token_balance_key",
                component: "lock_hash",
                expected: 32,
                call: |h| {
                    let _ = encode_addr_token_balance_key(h, &TokenBalance::from(7u128), &H);
                },
            },
            FixedWidthCase {
                encoder: "encode_addr_token_balance_key",
                component: "type_hash",
                expected: 32,
                call: |h| {
                    let _ = encode_addr_token_balance_key(&H, &TokenBalance::from(7u128), h);
                },
            },
            FixedWidthCase {
                encoder: "encode_token_transfers_key",
                component: "type_hash",
                expected: 32,
                call: |h| {
                    let _ = encode_token_transfers_key(h);
                },
            },
            FixedWidthCase {
                encoder: "encode_token_hourly_key",
                component: "type_hash",
                expected: 32,
                call: |h| {
                    let _ = encode_token_hourly_key(h, 1);
                },
            },
            FixedWidthCase {
                encoder: "encode_token_hourly_prefix",
                component: "type_hash",
                expected: 32,
                call: |h| {
                    let _ = encode_token_hourly_prefix(h);
                },
            },
            FixedWidthCase {
                encoder: "encode_token_daily_key",
                component: "type_hash",
                expected: 32,
                call: |h| {
                    let _ = encode_token_daily_key(h, 20240101);
                },
            },
            FixedWidthCase {
                encoder: "encode_token_daily_prefix",
                component: "type_hash",
                expected: 32,
                call: |h| {
                    let _ = encode_token_daily_prefix(h);
                },
            },
            FixedWidthCase {
                encoder: "encode_token_transfer_key",
                component: "type_hash",
                expected: 32,
                call: |h| {
                    let _ = encode_token_transfer_key(h, 1, 0);
                },
            },
            FixedWidthCase {
                encoder: "encode_cluster_daily_key",
                component: "cluster_id",
                expected: 32,
                call: |h| {
                    let _ = encode_cluster_daily_key(h, 20240101);
                },
            },
            FixedWidthCase {
                encoder: "encode_cluster_daily_prefix",
                component: "cluster_id",
                expected: 32,
                call: |h| {
                    let _ = encode_cluster_daily_prefix(h);
                },
            },
            FixedWidthCase {
                encoder: "encode_cluster_owner_key",
                component: "cluster_id",
                expected: 32,
                call: |h| {
                    let _ = encode_cluster_owner_key(h, &H);
                },
            },
            FixedWidthCase {
                encoder: "encode_cluster_owner_key",
                component: "lock_hash",
                expected: 32,
                call: |h| {
                    let _ = encode_cluster_owner_key(&H, h);
                },
            },
            FixedWidthCase {
                encoder: "encode_cluster_owner_prefix",
                component: "cluster_id",
                expected: 32,
                call: |h| {
                    let _ = encode_cluster_owner_prefix(h);
                },
            },
            FixedWidthCase {
                encoder: "encode_spore_by_cluster_key",
                component: "cluster_id",
                expected: 32,
                call: |h| {
                    let _ = encode_spore_by_cluster_key(h, &H);
                },
            },
            FixedWidthCase {
                encoder: "encode_spore_by_cluster_key",
                component: "spore_id",
                expected: 32,
                call: |h| {
                    let _ = encode_spore_by_cluster_key(&H, h);
                },
            },
            FixedWidthCase {
                encoder: "encode_spore_daily_key",
                component: "spore_id",
                expected: 32,
                call: |h| {
                    let _ = encode_spore_daily_key(h, 20240101);
                },
            },
            FixedWidthCase {
                encoder: "encode_spore_daily_prefix",
                component: "spore_id",
                expected: 32,
                call: |h| {
                    let _ = encode_spore_daily_prefix(h);
                },
            },
            // `encode_spore_outpoint_by_id_key`/`_prefix` are deliberately NOT
            // fixed-width — identity item ids are stored verbatim at any width
            // in 1..=32. Their bounds are covered by the dedicated
            // `test_spore_outpoint_by_id_key_rejects_*` tests.
            FixedWidthCase {
                encoder: "encode_spore_type_index_key",
                component: "type_script_hash",
                expected: 32,
                call: |h| {
                    let _ = encode_spore_type_index_key(h);
                },
            },
            FixedWidthCase {
                encoder: "encode_object_type_index_key",
                component: "type_script_hash",
                expected: 32,
                call: |h| {
                    let _ = encode_object_type_index_key(h);
                },
            },
            FixedWidthCase {
                encoder: "encode_object_collection_owner_key",
                component: "lock_hash",
                expected: 32,
                call: |h| {
                    let _ = encode_object_collection_owner_key(&H, h);
                },
            },
            FixedWidthCase {
                encoder: "encode_identity_owner_key",
                component: "lock_hash",
                expected: 32,
                call: |h| {
                    let _ = encode_identity_owner_key(&H, h);
                },
            },
            FixedWidthCase {
                encoder: "encode_dotbit_outpoint_by_account_id_key",
                component: "account_id",
                expected: 20,
                call: |h| {
                    let _ = encode_dotbit_outpoint_by_account_id_key(h, &H, 0);
                },
            },
            FixedWidthCase {
                encoder: "encode_dotbit_outpoint_by_account_id_prefix",
                component: "account_id",
                expected: 20,
                call: |h| {
                    let _ = encode_dotbit_outpoint_by_account_id_prefix(h);
                },
            },
            FixedWidthCase {
                encoder: "encode_script_daily_key",
                component: "code_hash",
                expected: 32,
                call: |h| {
                    let _ = encode_script_daily_key(h, 1, false, 20240101);
                },
            },
            FixedWidthCase {
                encoder: "encode_script_daily_prefix",
                component: "code_hash",
                expected: 32,
                call: |h| {
                    let _ = encode_script_daily_prefix(h, 1, false);
                },
            },
            FixedWidthCase {
                encoder: "encode_script_daily_code_hash_prefix",
                component: "code_hash",
                expected: 32,
                call: |h| {
                    let _ = encode_script_daily_code_hash_prefix(h);
                },
            },
            FixedWidthCase {
                encoder: "encode_fiber_channel_id",
                component: "funding_tx_hash",
                expected: 32,
                call: |h| {
                    let _ = encode_fiber_channel_id(h, 0);
                },
            },
            FixedWidthCase {
                encoder: "encode_fiber_outpoint",
                component: "tx_hash",
                expected: 32,
                call: |h| {
                    let _ = encode_fiber_outpoint(h, 0);
                },
            },
            FixedWidthCase {
                encoder: "encode_addr_fiber_channel_key",
                component: "lock_hash",
                expected: 32,
                call: |h| {
                    let _ = encode_addr_fiber_channel_key(h, &H);
                },
            },
            FixedWidthCase {
                encoder: "encode_addr_fiber_channel_key",
                component: "channel_id",
                expected: 32,
                call: |h| {
                    let _ = encode_addr_fiber_channel_key(&H, h);
                },
            },
        ]
    }

    /// The regression this hardening exists for: an over-long identifier used
    /// to be truncated into its window, producing a key that belongs to a
    /// different entity. Every fixed-width slot must reject it instead.
    #[test]
    fn test_every_fixed_width_encoder_rejects_oversized_identifier() {
        for case in fixed_width_cases() {
            let oversized = vec![0x9B; case.expected + 1];
            let message = encoder_panic_message(|| (case.call)(&oversized)).unwrap_or_else(|| {
                panic!(
                    "{}: {} of {} bytes was accepted and silently truncated into the {}-byte window",
                    case.encoder,
                    case.component,
                    case.expected + 1,
                    case.expected
                )
            });
            let expected = format!(
                "{}: {} must be exactly {} bytes, got {}",
                case.encoder,
                case.component,
                case.expected,
                case.expected + 1
            );
            assert!(
                message.contains(&expected),
                "{} must fail actionably on an oversized {}: expected a message containing {:?}, got {:?}",
                case.encoder,
                case.component,
                expected,
                message
            );
        }
    }

    /// A short identifier used to die as an uncontextual slice-index panic
    /// naming no field. It must now name the encoder, the component and both
    /// lengths so an operator can locate the upstream caller.
    #[test]
    fn test_every_fixed_width_encoder_rejects_undersized_identifier() {
        for case in fixed_width_cases() {
            let undersized = vec![0x9B; case.expected - 1];
            let message = encoder_panic_message(|| (case.call)(&undersized)).unwrap_or_else(|| {
                panic!(
                    "{}: {} of {} bytes was accepted into the {}-byte window",
                    case.encoder,
                    case.component,
                    case.expected - 1,
                    case.expected
                )
            });
            let expected = format!(
                "{}: {} must be exactly {} bytes, got {}",
                case.encoder,
                case.component,
                case.expected,
                case.expected - 1
            );
            assert!(
                message.contains(&expected),
                "{} must fail actionably on an undersized {}: expected a message containing {:?}, got {:?}",
                case.encoder,
                case.component,
                expected,
                message
            );
        }
    }

    /// The asserts must not reject their own valid inputs.
    #[test]
    fn test_every_fixed_width_encoder_accepts_exact_identifier() {
        for case in fixed_width_cases() {
            let exact = vec![0x9B; case.expected];
            if let Some(message) = encoder_panic_message(|| (case.call)(&exact)) {
                panic!(
                    "{} rejected a valid {}-byte {}: {}",
                    case.encoder, case.expected, case.component, message
                );
            }
        }
    }

    /// Pin the exact aliasing the sweep prevents: a 33-byte script hash must
    /// not produce the key its 32-byte prefix produces.
    #[test]
    fn test_oversized_identifier_never_aliases_its_32_byte_prefix() {
        let prefix = [0x9B; 32];
        let mut oversized = prefix.to_vec();
        oversized.push(0xFF);

        let tx_hash = [0xAB; 32];
        let prefix_key = encode_cell_index_key(&prefix, 7, &tx_hash, 1);

        let message = encoder_panic_message(|| {
            let _ = encode_cell_index_key(&oversized, 7, &tx_hash, 1);
        })
        .expect("a 33-byte script_hash must be rejected, not truncated to its 32-byte prefix");
        assert!(
            message.contains("encode_cell_index_key: script_hash must be exactly 32 bytes, got 33"),
            "unexpected panic message: {message}"
        );

        // The 32-byte form is still the only way to reach that key range.
        assert_eq!(prefix_key.len(), 74);
        assert_eq!(&prefix_key[..32], &prefix);
    }

    /// Encoders that delegate their outpoint window to [`encode_outpoint`]
    /// inherit the invariant, and the message names the encoder that owns it.
    #[test]
    fn test_delegating_outpoint_encoders_propagate_the_tx_hash_invariant() {
        let delegating: Vec<(&str, EncoderCall)> = vec![
            ("encode_block_outpoint_key", |h| {
                let _ = encode_block_outpoint_key(1, h, 0);
            }),
            ("encode_spore_outpoint_key", |h| {
                let _ = encode_spore_outpoint_key(h, 0);
            }),
            ("encode_mnft_class_outpoint_key", |h| {
                let _ = encode_mnft_class_outpoint_key(h, 0);
            }),
            ("encode_mnft_token_outpoint_key", |h| {
                let _ = encode_mnft_token_outpoint_key(h, 0);
            }),
            ("encode_dotbit_account_outpoint_key", |h| {
                let _ = encode_dotbit_account_outpoint_key(h, 0);
            }),
        ];

        for (name, call) in delegating {
            let message = encoder_panic_message(|| call(&[0x9B; 33]))
                .unwrap_or_else(|| panic!("{name} accepted a 33-byte tx_hash"));
            assert!(
                message.contains("encode_outpoint: tx_hash must be exactly 32 bytes, got 33"),
                "{name} must surface the outpoint invariant, got {message:?}"
            );
        }
    }

    /// Guard the other half: identifiers that are legitimately narrower than
    /// the 32-byte window keep working. Hardening these into exact-32 asserts
    /// would break mNFT and `.bit` sync outright.
    #[test]
    fn test_padded_encoders_still_accept_their_variable_width_ids() {
        let mnft_class_id = [0xC1u8; 24];
        let mnft_token_id = [0xC2u8; 28];
        let dotbit_account_id = [0xC3u8; 20];
        let lock_hash = [0xD0u8; 32];

        assert_eq!(encode_object_daily_key(&mnft_class_id, 20240101).len(), 37);
        assert_eq!(encode_object_daily_prefix(&mnft_class_id).len(), 33);
        assert_eq!(encode_object_hourly_key(&mnft_class_id, 7).len(), 41);
        assert_eq!(encode_object_hourly_prefix(&mnft_class_id).len(), 33);
        assert_eq!(encode_spore_hourly_key(&dotbit_account_id, 7).len(), 41);
        assert_eq!(encode_spore_hourly_prefix(&dotbit_account_id).len(), 33);
        assert_eq!(
            encode_object_collection_owner_key(&mnft_class_id, &lock_hash).len(),
            OBJECT_COLLECTION_OWNER_KEY_SIZE
        );
        assert_eq!(
            encode_object_collection_owner_prefix(&mnft_class_id).len(),
            33
        );
        assert_eq!(
            encode_identity_owner_key(&dotbit_account_id, &lock_hash).len(),
            IDENTITY_OWNER_KEY_SIZE
        );
        assert_eq!(encode_identity_owner_prefix(&dotbit_account_id).len(), 32);
        assert_eq!(
            encode_object_by_collection_key(&mnft_class_id, &mnft_token_id).len(),
            32 + mnft_token_id.len()
        );
        assert_eq!(
            encode_identity_by_collection_key(&mnft_class_id, &dotbit_account_id).len(),
            32 + dotbit_account_id.len()
        );
        assert_eq!(
            encode_object_collection_activity_key(&mnft_class_id, 1, 0).len(),
            OBJECT_COLLECTION_ACTIVITY_KEY_SIZE
        );
        assert_eq!(
            encode_object_collection_activity_prefix(&mnft_class_id).len(),
            32
        );
        assert_eq!(
            encode_object_collection_activity_seek_after_key(&mnft_class_id, 1, 0).len(),
            OBJECT_COLLECTION_ACTIVITY_KEY_SIZE + 1
        );

        // `.bit` account IDs are 20 bytes by protocol, not a truncated hash.
        assert_eq!(
            encode_dotbit_outpoint_by_account_id_key(&dotbit_account_id, &[0xE3; 32], 0).len(),
            DOTBIT_OUTPOINT_BY_ACCOUNT_ID_KEY_SIZE
        );
    }

    #[test]
    #[should_panic(
        expected = "encode_cell_index_key: script_hash must be exactly 32 bytes, got 33"
    )]
    fn test_cell_index_key_rejects_oversized_script_hash() {
        let _ = encode_cell_index_key(&[0x9B; 33], 1, &[0xAB; 32], 0);
    }

    #[test]
    #[should_panic(
        expected = "encode_cell_index_key: script_hash must be exactly 32 bytes, got 31"
    )]
    fn test_cell_index_key_rejects_undersized_script_hash() {
        let _ = encode_cell_index_key(&[0x9B; 31], 1, &[0xAB; 32], 0);
    }

    #[test]
    #[should_panic(expected = "encode_cell_index_key: tx_hash must be exactly 32 bytes, got 33")]
    fn test_cell_index_key_rejects_oversized_tx_hash() {
        let _ = encode_cell_index_key(&[0x9B; 32], 1, &[0xAB; 33], 0);
    }

    #[test]
    #[should_panic(expected = "encode_outpoint: tx_hash must be exactly 32 bytes, got 33")]
    fn test_outpoint_rejects_oversized_tx_hash() {
        let _ = encode_outpoint(&[0x9B; 33], 0);
    }

    #[test]
    #[should_panic(expected = "encode_outpoint: tx_hash must be exactly 32 bytes, got 31")]
    fn test_outpoint_rejects_undersized_tx_hash() {
        let _ = encode_outpoint(&[0x9B; 31], 0);
    }

    /// The scope is part of the persisted undo-log key: its value is pinned
    /// so a renumbering cannot silently re-scope stored entries.
    #[test]
    fn test_undo_seq_scope_table_is_pinned() {
        assert_eq!(UNDO_SEQ_SCOPE_SHIFT, 48);
        assert_eq!(UNDO_SEQ_LOCAL_MAX, 0x0000_FFFF_FFFF_FFFF);
        let journal = UndoSeqScope::Journal;
        assert_eq!(journal.seq_base(), 0x0006 << 48);
        assert!(journal.owns(journal.seq_base()));
        assert!(journal.owns(journal.seq_base() | UNDO_SEQ_LOCAL_MAX));
        // The retired per-writer scopes' bits, a counter that overflowed into
        // the scope bits, and no scope bits at all are not journal rows.
        for bits in [0x0000u64, 0x0001, 0x0002, 0x0003, 0x0004, 0x0005, 0x0007] {
            assert!(!journal.owns(bits << 48), "{bits:#06x}");
        }
    }

    #[test]
    fn test_addr_tx_block_seek_key_orders_between_blocks() {
        let lock = [0xAA; 32];
        let seek = encode_addr_tx_block_seek_key(&lock, 5);
        // Every row of block 5, whatever its tx index, sorts at or after the seek key.
        assert!(encode_addr_tx_key(&lock, 5, 0) >= seek);
        assert!(encode_addr_tx_key(&lock, 5, i32::MAX) >= seek);
        // Lower blocks follow it; higher blocks precede it.
        assert!(encode_addr_tx_key(&lock, 4, i32::MAX) > seek);
        assert!(encode_addr_tx_key(&lock, 6, 0) < seek);
    }

    #[test]
    #[should_panic(expected = "encode_addr_tx_key: lock_hash must be exactly 32 bytes, got 33")]
    fn test_addr_tx_key_rejects_oversized_lock_hash() {
        let _ = encode_addr_tx_key(&[0x9B; 33], 1, 0);
    }

    #[test]
    #[should_panic(
        expected = "encode_token_holder_balance_key: type_hash must be exactly 32 bytes, got 33"
    )]
    fn test_token_holder_balance_key_rejects_oversized_type_hash() {
        let _ =
            encode_token_holder_balance_key(&[0x9B; 33], &TokenBalance::from(1u128), &[0xAB; 32]);
    }

    #[test]
    #[should_panic(
        expected = "encode_dotbit_outpoint_by_account_id_key: account_id must be exactly 20 bytes, got 21"
    )]
    fn test_dotbit_outpoint_by_account_id_key_rejects_oversized_account_id() {
        let _ = encode_dotbit_outpoint_by_account_id_key(&[0x9B; 21], &[0xAB; 32], 0);
    }
}

#[cfg(test)]
mod addr_tx_by_prefix_key_tests {
    use super::*;

    #[test]
    fn addr_tx_by_prefix_key_roundtrip_is_32_bytes_and_descending() {
        let prefix = [0x5Au8; 20];
        let k1 = encode_addr_tx_by_prefix_key(&prefix, 100, 3);
        let k2 = encode_addr_tx_by_prefix_key(&prefix, 101, 0);
        assert_eq!(k1.len(), ADDR_TX_BY_PREFIX_KEY_SIZE);
        assert_eq!(ADDR_TX_BY_PREFIX_KEY_SIZE, 32);
        assert!(k2 < k1, "更高的块必须排在前面（desc 编码）");
        assert_eq!(decode_addr_tx_by_prefix_key(&k1).unwrap(), (prefix, 100, 3));
        let seek = encode_addr_tx_by_prefix_seek_after_key(&prefix, 100, 3);
        assert!(seek > k1 && seek.starts_with(&prefix));
    }

    #[test]
    fn decode_addr_tx_by_prefix_key_refuses_the_64_byte_layout() {
        let mut old = encode_addr_tx_by_prefix_key(&[0x5A; 20], 100, 3);
        old.extend_from_slice(&[0x77; 32]);
        let err = decode_addr_tx_by_prefix_key(&old).unwrap_err().to_string();
        assert!(
            err.contains("has 64 bytes, expected 32") && err.contains("re-sync required"),
            "{err}"
        );
    }

    #[test]
    #[should_panic(expected = "prefix must be exactly 20 bytes")]
    fn addr_tx_by_prefix_key_rejects_wrong_prefix_width() {
        encode_addr_tx_by_prefix_key(&[0u8; 32], 1, 0);
    }
}

#[cfg(test)]
mod dotcell_key_tests {
    use super::*;

    #[test]
    fn dotcell_name_by_owner_key_is_owner20_then_id20() {
        let key = encode_dotcell_name_by_owner_key(&[0x11; 20], &[0x22; 20]);
        assert_eq!(key.len(), DOTCELL_NAME_BY_OWNER_KEY_SIZE);
        assert_eq!(&key[..20], &[0x11; 20]);
        assert_eq!(&key[20..], &[0x22; 20]);
        assert_eq!(
            decode_dotcell_name_by_owner_key(&key),
            ([0x11; 20], [0x22; 20])
        );
        assert_eq!(encode_dotcell_name_by_owner_prefix(&[0x11; 20]), [0x11; 20]);
        assert!(key.starts_with(&encode_dotcell_name_by_owner_prefix(&[0x11; 20])));
    }

    #[test]
    #[should_panic(expected = "name_id")]
    fn dotcell_name_by_owner_key_rejects_a_32_byte_id() {
        encode_dotcell_name_by_owner_key(&[0x11; 20], &[0x22; 32]);
    }

    #[test]
    fn identity_owner20_key_pads_explicitly_and_roundtrips() {
        let key =
            encode_identity_owner20_key(&crate::types::DOTCELL_SENTINEL_COLLECTION, &[0x33; 20]);
        assert_eq!(key.len(), IDENTITY_OWNER_KEY_SIZE);
        assert_eq!(&key[..32], &crate::types::DOTCELL_SENTINEL_COLLECTION[..]);
        assert_eq!(&key[32..52], &[0x33; 20]);
        assert_eq!(&key[52..], &[0u8; 12]);
        assert_eq!(decode_identity_owner20(&key[32..]), [0x33; 20]);
        assert!(key.starts_with(&encode_identity_owner_prefix(
            &crate::types::DOTCELL_SENTINEL_COLLECTION
        )));
    }

    #[test]
    #[should_panic(expected = "owner20")]
    fn identity_owner20_key_rejects_32_bytes() {
        encode_identity_owner20_key(&crate::types::DOTCELL_SENTINEL_COLLECTION, &[0u8; 32]);
    }

    #[test]
    #[should_panic(expected = "not a 20-byte prefix padded with zeros")]
    fn decode_identity_owner20_rejects_a_real_lock_hash() {
        decode_identity_owner20(&[0x77; 32]);
    }

    #[test]
    fn try_decode_identity_owner20_reports_a_malformed_segment() {
        let mut segment = [0u8; 32];
        segment[..20].copy_from_slice(&[0x33; 20]);
        assert_eq!(try_decode_identity_owner20(&segment).unwrap(), [0x33; 20]);

        let err = try_decode_identity_owner20(&[0x77; 32])
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("not a 20-byte prefix padded with zeros"),
            "{err}"
        );
        assert!(err.contains(&"77".repeat(32)), "{err}");

        let err = try_decode_identity_owner20(&[0x33; 20])
            .unwrap_err()
            .to_string();
        assert!(err.contains("exactly 32 bytes, got 20"), "{err}");
    }
}
