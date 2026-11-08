//! Direct read-only access to CKB node's RocksDB data.
//!
//! Opens the CKB node's data directory as a **secondary instance**,
//! allowing concurrent reads while the node is running and seeing
//! new blocks via periodic `refresh()` calls.
//! Reads blocks at ~0.1ms instead of ~15ms via JSON-RPC.

mod convert;

use anyhow::{anyhow, Result};
use ckb_types::core::{BlockView, HeaderView, TransactionView, UncleBlockVecView};
use ckb_types::packed;
use ckb_types::prelude::*;
use rocksdb::{ColumnFamilyDescriptor, DBCompressionType, Options, DB};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use tracing::{debug, info};

pub use convert::{
    block_view_to_rpc, convert_transaction_view, RpcBlockResponseWithCycles, RpcBlockView,
    RpcCellDep, RpcCellInput, RpcCellOutput, RpcHeaderView, RpcOutPoint, RpcScript,
    RpcTransactionView, RpcUncleBlockView,
};

/// CKB RocksDB column family names (from ckb-db-schema).
/// These are string identifiers "0" through "18" mapping to CKB's 19 column families.
const COLUMN_INDEX: &str = "0";
const COLUMN_BLOCK_HEADER: &str = "1";
const COLUMN_BLOCK_BODY: &str = "2";
const COLUMN_BLOCK_UNCLE: &str = "3";
const COLUMN_META: &str = "4";
const COLUMN_TRANSACTION_INFO: &str = "5";
const COLUMN_BLOCK_EXT: &str = "6";
const COLUMN_BLOCK_PROPOSAL_IDS: &str = "7";
const COLUMN_CELL_DATA: &str = "12";
const COLUMN_BLOCK_EXTENSION: &str = "15";

/// All 19 column families used by CKB's RocksDB.
const ALL_COLUMN_FAMILIES: &[&str] = &[
    "0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "10", "11", "12", "13", "14", "15", "16",
    "17", "18",
];

/// CKB meta key for tip header hash.
const META_TIP_HEADER_KEY: &[u8] = b"TIP_HEADER";

/// Lightweight block header info extracted from RocksDB.
/// Provides fields not stored in `blocks_index` PostgreSQL table.
#[derive(Debug, Clone)]
pub struct BlockHeaderInfo {
    pub parent_hash: [u8; 32],
    pub nonce: u128,
    pub transactions_root: [u8; 32],
    pub version: u32,
}

/// Direct read-only access to the CKB node's RocksDB store.
///
/// Opens the database as a **secondary instance**, which is safe to use while the CKB node
/// is running. Unlike read-only mode, a secondary instance can see new writes from the
/// primary (CKB node) by calling [`refresh()`](Self::refresh).
pub struct CkbChainReader {
    // Drop order matters: close RocksDB before removing the secondary directory.
    db: DB,
    _secondary_dir: SecondaryDir,
}

// Safety: DB is Send+Sync — RocksDB handles internal synchronization for secondary instances
unsafe impl Send for CkbChainReader {}
unsafe impl Sync for CkbChainReader {}

impl CkbChainReader {
    /// Open a CKB RocksDB directory as a secondary RocksDB instance.
    ///
    /// `ckb_db_path` should be the final resolved RocksDB path for the CKB node.
    /// For the default CKB layout, this is the `data/db` subdirectory under the
    /// CKB node's configured `data_dir`. For Docker, this is typically
    /// `/var/lib/ckb/data/db`.
    ///
    /// The secondary instance starts with a snapshot of the primary's state.
    /// Call [`refresh()`](Self::refresh) periodically to see new blocks written by the node.
    pub fn open(ckb_db_path: &str) -> Result<Self> {
        let db_path = Path::new(ckb_db_path);
        if !db_path.exists() {
            return Err(anyhow!("CKB RocksDB path does not exist: {}", ckb_db_path));
        }

        let secondary_dir = SecondaryDir::create_for_process()?;
        let secondary_path = secondary_dir.path();
        let secondary_path_str = secondary_path.to_str().ok_or_else(|| {
            anyhow!(
                "Secondary path is not valid UTF-8: {}",
                secondary_path.display()
            )
        })?;

        let mut opts = Options::default();
        opts.set_compression_type(DBCompressionType::Lz4);
        // Recommended for secondary instances: keep all files open for best IO performance
        opts.set_max_open_files(-1);

        let cf_descriptors: Vec<ColumnFamilyDescriptor> = ALL_COLUMN_FAMILIES
            .iter()
            .map(|name| {
                let mut cf_opts = Options::default();
                cf_opts.set_compression_type(DBCompressionType::Lz4);
                ColumnFamilyDescriptor::new(*name, cf_opts)
            })
            .collect();

        let db = DB::open_cf_descriptors_as_secondary(
            &opts,
            ckb_db_path,
            secondary_path_str,
            cf_descriptors,
        )
        .map_err(|e| {
            anyhow!(
                "Failed to open CKB RocksDB at {} as secondary: {}",
                ckb_db_path,
                e
            )
        })?;

        // Initial catch-up to see the latest data from the primary
        db.try_catch_up_with_primary()
            .map_err(|e| anyhow!("Failed initial catch-up with CKB RocksDB primary: {}", e))?;

        info!(
            "Opened CKB RocksDB at {} (secondary instance, path: {})",
            ckb_db_path,
            secondary_path.display()
        );

        Ok(Self {
            db,
            _secondary_dir: secondary_dir,
        })
    }

    /// Refresh the secondary instance to see the latest writes from the CKB node.
    ///
    /// This should be called periodically (e.g., before each poll cycle in the indexer)
    /// to pick up new blocks. The operation is very fast (~microseconds) when there are
    /// no new SST files to process.
    pub fn refresh(&self) -> Result<()> {
        self.db
            .try_catch_up_with_primary()
            .map_err(|e| anyhow!("Failed to catch up with CKB RocksDB primary: {}", e))
    }

    /// Get the tip block number from the chain.
    pub fn tip_number(&self) -> Option<u64> {
        let tip_hash = self.tip_hash()?;
        self.get_block_number(&tip_hash)
    }

    /// Get the tip block hash from the chain.
    pub fn tip_hash(&self) -> Option<[u8; 32]> {
        let cf = self.db.cf_handle(COLUMN_META)?;
        let raw = self.db.get_cf(&cf, META_TIP_HEADER_KEY).ok()??;
        if raw.len() != 32 {
            return None;
        }
        let mut hash = [0u8; 32];
        hash.copy_from_slice(&raw);
        Some(hash)
    }

    /// Get the block hash for a given block number.
    ///
    /// Uses COLUMN_INDEX: block_number (packed::Uint64, 8 bytes LE) -> block_hash (32 bytes).
    pub fn get_block_hash(&self, number: u64) -> Option<[u8; 32]> {
        let cf = self.db.cf_handle(COLUMN_INDEX)?;
        let key = number.to_le_bytes();
        let raw = self.db.get_cf(&cf, key).ok()??;
        if raw.len() != 32 {
            return None;
        }
        let mut hash = [0u8; 32];
        hash.copy_from_slice(&raw);
        Some(hash)
    }

    /// Get the block number for a given block hash.
    ///
    /// Uses COLUMN_INDEX: block_hash (32 bytes) -> block_number (packed::Uint64, 8 bytes LE).
    pub fn get_block_number(&self, hash: &[u8; 32]) -> Option<u64> {
        let cf = self.db.cf_handle(COLUMN_INDEX)?;
        let raw = self.db.get_cf(&cf, hash).ok()??;
        if raw.len() != 8 {
            return None;
        }
        Some(u64::from_le_bytes(raw[..8].try_into().ok()?))
    }

    /// Get a full block by number.
    ///
    /// Looks up the block hash for the given number, then reads the full block.
    /// `Ok(None)`: no block at that number; `Err`: the stored block is unreadable.
    pub fn get_block_by_number(&self, number: u64) -> Result<Option<BlockView>> {
        let Some(hash) = self.get_block_hash(number) else {
            return Ok(None);
        };
        self.get_block(&hash)
    }

    /// Get lightweight header info for a block (fields not stored in `blocks_index`).
    ///
    /// Returns `parent_hash`, `nonce`, `transactions_root`, and `version` without
    /// reading the full block body.
    pub fn get_block_header_info(&self, hash: &[u8; 32]) -> Result<Option<BlockHeaderInfo>> {
        Ok(self.get_block_header(hash)?.map(|view| BlockHeaderInfo {
            parent_hash: view.parent_hash().unpack(),
            nonce: view.nonce(),
            transactions_root: view.transactions_root().unpack(),
            version: view.version(),
        }))
    }

    /// Get lightweight header info by block number.
    pub fn get_block_header_info_by_number(&self, number: u64) -> Result<Option<BlockHeaderInfo>> {
        let Some(hash) = self.get_block_hash(number) else {
            return Ok(None);
        };
        self.get_block_header_info(&hash)
    }

    /// Get the parsed cellbase witness of a block: the miner's declared lock
    /// script and the free-form miner message.
    ///
    /// Per RFC-0022, the first witness of a cellbase transaction is a
    /// `CellbaseWitness` molecule (`lock: Script, message: Bytes`) — NOT a
    /// `WitnessArgs`. The lock identifies the block's own miner (the cellbase
    /// OUTPUT lock instead pays the reward of the block 11 confirmations
    /// back).
    pub fn get_cellbase_witness(&self, hash: &[u8; 32]) -> Option<(packed::Script, Vec<u8>)> {
        // Read just the cellbase transaction (index 0) from block body
        let cf = self.db.cf_handle(COLUMN_BLOCK_BODY)?;
        let mut key = Vec::with_capacity(36);
        key.extend_from_slice(hash);
        key.extend_from_slice(&0u32.to_be_bytes());
        let raw = self.db.get_cf(&cf, &key).ok()??;
        let tv = packed::TransactionViewReader::from_slice(&raw).ok()?;
        let witnesses = tv.data().witnesses();
        if witnesses.is_empty() {
            return None;
        }
        let first_witness = witnesses.get(0)?;
        let witness_bytes = first_witness.raw_data();
        parse_cellbase_witness(witness_bytes)
    }

    /// Get the miner message embedded in the cellbase witness.
    pub fn get_miner_message(&self, hash: &[u8; 32]) -> Option<Vec<u8>> {
        self.get_cellbase_witness(hash).map(|(_, message)| message)
    }

    /// Get a full block by hash, returned as ckb_types::core::BlockView.
    ///
    /// Every hash in the view (block, uncles, transactions, witnesses) is the
    /// one the node stored with the object; none is computed again.
    /// `Ok(None)`: no block stored under `hash`. `Err`: a read failed, a row
    /// does not decode, or the header is stored without its uncles or
    /// proposals row (the node writes all of a block in one batch).
    pub fn get_block(&self, hash: &[u8; 32]) -> Result<Option<BlockView>> {
        let Some(header) = self.get_block_header(hash)? else {
            return Ok(None);
        };
        let uncles = self.get_block_uncles(hash)?.ok_or_else(|| {
            anyhow!(
                "CKB block 0x{} has a stored header but no uncles row",
                hex::encode(hash)
            )
        })?;
        let proposals = self.get_block_proposals_packed(hash)?.ok_or_else(|| {
            anyhow!(
                "CKB block 0x{} has a stored header but no proposals row",
                hex::encode(hash)
            )
        })?;
        let transactions = self.get_block_transactions(hash)?;
        // Extensions are stored separately by the node. Omitting one changes
        // the serialized block and underreports its byte size.
        let extension_cf = self.column(COLUMN_BLOCK_EXTENSION)?;
        let extension = self
            .db
            .get_cf(extension_cf, hash)
            .map_err(|e| {
                anyhow!(
                    "CKB block extension read failed: block=0x{}: {e}",
                    hex::encode(hash)
                )
            })?
            .map(|bytes| {
                packed::Bytes::from_slice(&bytes).map_err(|e| {
                    anyhow!(
                        "undecodable CKB block extension row: block=0x{}: {e}",
                        hex::encode(hash)
                    )
                })
            })
            .transpose()?;

        Ok(Some(match extension {
            Some(extension) => BlockView::new_unchecked_with_extension(
                header,
                uncles,
                transactions,
                proposals,
                extension,
            ),
            None => BlockView::new_unchecked(header, uncles, transactions, proposals),
        }))
    }

    /// Get a transaction by hash, with the number of the block that holds it.
    ///
    /// Uses COLUMN_TRANSACTION_INFO (column "5") to find the block location,
    /// then reads from COLUMN_BLOCK_BODY. `Ok(None)`: the node stores no such
    /// transaction. `Err`: a read failed, a row does not decode, or the info row
    /// points at a body row that is not stored.
    pub fn get_transaction_with_block_number(
        &self,
        tx_hash: &[u8; 32],
    ) -> Result<Option<(TransactionView, u64)>> {
        let cf_info = self.column(COLUMN_TRANSACTION_INFO)?;
        let Some(raw_info) = self.db.get_cf(cf_info, tx_hash).map_err(|e| {
            anyhow!(
                "CKB transaction info read failed: tx=0x{}: {e}",
                hex::encode(tx_hash)
            )
        })?
        else {
            return Ok(None);
        };

        // TransactionInfo layout (52 bytes):
        //   block_number: Uint64 (8 bytes LE) [0..8]
        //   block_epoch:  Uint64 (8 bytes LE) [8..16]
        //   key: TransactionKey (36 bytes) [16..52]
        //     key.block_hash: Byte32 (32 bytes) [16..48]
        //     key.index: BeUint32 (4 bytes BE) [48..52]
        // The TransactionKey bytes are the COLUMN_BLOCK_BODY key.
        let tx_info = packed::TransactionInfoReader::from_slice(&raw_info).map_err(|e| {
            anyhow!(
                "undecodable CKB transaction info row: tx=0x{}: {e}",
                hex::encode(tx_hash)
            )
        })?;
        let block_number: u64 = tx_info.block_number().unpack();
        let body_key = tx_info.key().as_slice();

        let cf_body = self.column(COLUMN_BLOCK_BODY)?;
        let raw_tx = self
            .db
            .get_cf(cf_body, body_key)
            .map_err(|e| {
                anyhow!(
                    "CKB block body read failed: tx=0x{} body_key=0x{}: {e}",
                    hex::encode(tx_hash),
                    hex::encode(body_key)
                )
            })?
            .ok_or_else(|| {
                anyhow!(
                    "missing CKB block body row: tx=0x{} block={} body_key=0x{} (its transaction info row points there)",
                    hex::encode(tx_hash),
                    block_number,
                    hex::encode(body_key)
                )
            })?;

        // The stored view carries the transaction's hash and witness hash.
        let tx = packed::TransactionViewReader::from_slice(&raw_tx)
            .map_err(|e| {
                anyhow!(
                    "undecodable CKB block body row: tx=0x{} block={} body_key=0x{}: {e}",
                    hex::encode(tx_hash),
                    block_number,
                    hex::encode(body_key)
                )
            })?
            .unpack();
        Ok(Some((tx, block_number)))
    }

    /// Get a transaction by hash; see [`Self::get_transaction_with_block_number`].
    pub fn get_transaction(&self, tx_hash: &[u8; 32]) -> Result<Option<TransactionView>> {
        Ok(self
            .get_transaction_with_block_number(tx_hash)?
            .map(|(tx, _)| tx))
    }

    /// Get cell data for a specific output.
    ///
    /// A live cell's data is in `COLUMN_CELL_DATA` (the node writes a
    /// `CellDataEntry`, or an empty value for empty data); a consumed cell has
    /// no row there and is read from its transaction in `COLUMN_BLOCK_BODY`.
    /// `Ok(None)`: no such transaction or output. `Err`: a read failed or a row
    /// does not decode.
    pub fn get_cell_data(&self, tx_hash: &[u8; 32], index: u32) -> Result<Option<Vec<u8>>> {
        let cf = self.column(COLUMN_CELL_DATA)?;
        let mut key = Vec::with_capacity(36);
        key.extend_from_slice(tx_hash);
        key.extend_from_slice(&index.to_le_bytes());
        let live = self.db.get_cf(cf, &key).map_err(|e| {
            anyhow!(
                "CKB cell data read failed: outpoint=0x{}:{index}: {e}",
                hex::encode(tx_hash)
            )
        })?;
        if let Some(raw) = live {
            if raw.is_empty() {
                return Ok(Some(Vec::new()));
            }
            let entry = packed::CellDataEntryReader::from_slice(&raw).map_err(|e| {
                anyhow!(
                    "undecodable CKB cell data row: outpoint=0x{}:{index}: {e}",
                    hex::encode(tx_hash)
                )
            })?;
            return Ok(Some(entry.output_data().raw_data().to_vec()));
        }

        let Some(tx) = self.get_transaction(tx_hash)? else {
            return Ok(None);
        };
        Ok(tx
            .outputs_data()
            .get(index as usize)
            .map(|item| item.raw_data().to_vec()))
    }

    /// Get the block extension data (contains total_difficulty, cycles, sizes).
    pub fn get_block_ext(&self, hash: &[u8; 32]) -> Option<(u64, Vec<Option<u64>>)> {
        let cf = self.db.cf_handle(COLUMN_BLOCK_EXT)?;
        let raw = self.db.get_cf(&cf, hash).ok()??;

        // Try BlockExtV1 first (newer format with cycles and txs_sizes)
        if let Ok(reader) = packed::BlockExtV1Reader::from_compatible_slice(&raw) {
            let td_bytes = reader.total_difficulty().raw_data();
            let total_difficulty = u64::from_le_bytes(td_bytes[..8].try_into().unwrap_or([0u8; 8]));

            let cycles: Vec<Option<u64>> = reader
                .cycles()
                .to_opt()
                .map(|c| {
                    let entity = c.to_entity();
                    (0..entity.len())
                        .map(|i| {
                            let v = entity.get(i).expect("valid index");
                            let bytes = v.raw_data();
                            Some(u64::from_le_bytes(
                                bytes[..8].try_into().unwrap_or([0u8; 8]),
                            ))
                        })
                        .collect()
                })
                .unwrap_or_default();

            Some((total_difficulty, cycles))
        } else if let Ok(reader) = packed::BlockExtReader::from_compatible_slice(&raw) {
            let td_bytes = reader.total_difficulty().raw_data();
            let total_difficulty = u64::from_le_bytes(td_bytes[..8].try_into().unwrap_or([0u8; 8]));
            Some((total_difficulty, vec![]))
        } else {
            None
        }
    }

    // -- Batch methods (multi_get) --

    /// Batch-fetch block hashes for multiple block numbers.
    ///
    /// Uses RocksDB `multi_get_cf` to batch all lookups into a single call,
    /// reducing lock contention compared to individual `get_block_hash` calls.
    pub fn get_block_hashes_batch(&self, numbers: &[u64]) -> Vec<Option<[u8; 32]>> {
        if numbers.is_empty() {
            return vec![];
        }
        let cf = match self.db.cf_handle(COLUMN_INDEX) {
            Some(cf) => cf,
            None => return vec![None; numbers.len()],
        };
        let keys: Vec<[u8; 8]> = numbers.iter().map(|n| n.to_le_bytes()).collect();
        let cf_keys: Vec<_> = keys.iter().map(|k| (cf, k.as_slice())).collect();
        self.db
            .multi_get_cf(cf_keys)
            .into_iter()
            .map(|r| {
                let raw = r.ok()??;
                if raw.len() != 32 {
                    return None;
                }
                let mut hash = [0u8; 32];
                hash.copy_from_slice(&raw);
                Some(hash)
            })
            .collect()
    }

    /// Batch-fetch full blocks for multiple block hashes.
    ///
    /// Point lookups (headers, uncles, proposals) are batched into a single
    /// `multi_get_cf` call across three column families. Block bodies still
    /// require per-block prefix iteration.
    pub fn get_blocks_batch(&self, hashes: &[[u8; 32]]) -> Result<Vec<Option<BlockView>>> {
        if hashes.is_empty() {
            return Ok(vec![]);
        }
        let cf_header = self.column(COLUMN_BLOCK_HEADER)?;
        let cf_uncle = self.column(COLUMN_BLOCK_UNCLE)?;
        let cf_proposals = self.column(COLUMN_BLOCK_PROPOSAL_IDS)?;

        let n = hashes.len();
        // Mixed-CF multi_get: [N headers, N uncles, N proposals]
        let mut cf_keys: Vec<_> = Vec::with_capacity(3 * n);
        for hash in hashes {
            cf_keys.push((cf_header, hash.as_slice()));
        }
        for hash in hashes {
            cf_keys.push((cf_uncle, hash.as_slice()));
        }
        for hash in hashes {
            cf_keys.push((cf_proposals, hash.as_slice()));
        }

        let mg = self.db.multi_get_cf(cf_keys);
        // mg layout: [0..n) headers, [n..2n) uncles, [2n..3n) proposals

        let mut blocks = Vec::with_capacity(n);
        for (i, hash) in hashes.iter().enumerate() {
            // Header, uncle and transaction views carry the hashes the node
            // stored with them; the view is assembled without hashing.
            let read = |slot: usize, what: &str| match &mg[slot] {
                Ok(raw) => Ok(raw.as_deref()),
                Err(e) => Err(anyhow!(
                    "CKB block {what} read failed: block=0x{}: {e}",
                    hex::encode(hash)
                )),
            };
            let Some(raw_header) = read(i, "header")? else {
                blocks.push(None);
                continue;
            };
            let header = decode_header(hash, raw_header)?;
            let uncles = decode_uncles(
                hash,
                read(n + i, "uncles")?.ok_or_else(|| {
                    anyhow!(
                        "CKB block 0x{} has a stored header but no uncles row",
                        hex::encode(hash)
                    )
                })?,
            )?;
            let proposals = decode_proposals(
                hash,
                read(2 * n + i, "proposals")?.ok_or_else(|| {
                    anyhow!(
                        "CKB block 0x{} has a stored header but no proposals row",
                        hex::encode(hash)
                    )
                })?,
            )?;

            // Body: prefix iteration per block (cannot be batched)
            let transactions = self.get_block_transactions(hash)?;

            // No extension: nothing on the bulk path reads it, and the batch
            // view has never carried one (`get_block` does).
            blocks.push(Some(BlockView::new_unchecked(
                header,
                uncles,
                transactions,
                proposals,
            )));
        }

        Ok(blocks)
    }

    /// Batch-fetch block extension data for multiple block hashes.
    ///
    /// Uses RocksDB `multi_get_cf` to batch all lookups into a single call.
    pub fn get_block_exts_batch(
        &self,
        hashes: &[[u8; 32]],
    ) -> Vec<Option<(u64, Vec<Option<u64>>)>> {
        if hashes.is_empty() {
            return vec![];
        }
        let cf = match self.db.cf_handle(COLUMN_BLOCK_EXT) {
            Some(cf) => cf,
            None => return vec![None; hashes.len()],
        };
        let cf_keys: Vec<_> = hashes.iter().map(|h| (cf, h.as_slice())).collect();
        self.db
            .multi_get_cf(cf_keys)
            .into_iter()
            .map(|r| {
                let raw = r.ok()??;
                if let Ok(reader) = packed::BlockExtV1Reader::from_compatible_slice(&raw) {
                    let td_bytes = reader.total_difficulty().raw_data();
                    let total_difficulty =
                        u64::from_le_bytes(td_bytes[..8].try_into().unwrap_or([0u8; 8]));
                    let cycles: Vec<Option<u64>> = reader
                        .cycles()
                        .to_opt()
                        .map(|c| {
                            let entity = c.to_entity();
                            (0..entity.len())
                                .map(|j| {
                                    let v = entity.get(j).expect("valid index");
                                    let bytes = v.raw_data();
                                    Some(u64::from_le_bytes(
                                        bytes[..8].try_into().unwrap_or([0u8; 8]),
                                    ))
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    Some((total_difficulty, cycles))
                } else if let Ok(reader) = packed::BlockExtReader::from_compatible_slice(&raw) {
                    let td_bytes = reader.total_difficulty().raw_data();
                    let total_difficulty =
                        u64::from_le_bytes(td_bytes[..8].try_into().unwrap_or([0u8; 8]));
                    Some((total_difficulty, vec![]))
                } else {
                    None
                }
            })
            .collect()
    }

    // -- Internal helpers --

    fn column(&self, name: &'static str) -> Result<&rocksdb::ColumnFamily> {
        self.db
            .cf_handle(name)
            .ok_or_else(|| anyhow!("CKB RocksDB column family {name} is not open"))
    }

    fn get_row(
        &self,
        column: &'static str,
        what: &str,
        hash: &[u8; 32],
    ) -> Result<Option<Vec<u8>>> {
        self.db.get_cf(self.column(column)?, hash).map_err(|e| {
            anyhow!(
                "CKB block {what} read failed: block=0x{}: {e}",
                hex::encode(hash)
            )
        })
    }

    /// The stored header view: the header and the hash the node stored with it.
    fn get_block_header(&self, hash: &[u8; 32]) -> Result<Option<HeaderView>> {
        self.get_row(COLUMN_BLOCK_HEADER, "header", hash)?
            .map(|raw| decode_header(hash, &raw))
            .transpose()
    }

    /// The stored uncle views: the uncles and the hashes the node stored.
    fn get_block_uncles(&self, hash: &[u8; 32]) -> Result<Option<UncleBlockVecView>> {
        self.get_row(COLUMN_BLOCK_UNCLE, "uncles", hash)?
            .map(|raw| decode_uncles(hash, &raw))
            .transpose()
    }

    /// Read all transactions for a block from COLUMN_BLOCK_BODY, each with the
    /// hash and witness hash the node stored with it.
    /// Keys are `block_hash (32 bytes) + tx_index (4 bytes BE)`, iterated by prefix.
    /// A failed read or a row that does not decode is an error: a block is
    /// never returned without one of its transactions.
    fn get_block_transactions(&self, hash: &[u8; 32]) -> Result<Vec<TransactionView>> {
        let cf = self.column(COLUMN_BLOCK_BODY)?;
        let prefix = hash.as_slice();
        let mut txs = Vec::new();
        for item in self.db.prefix_iterator_cf(cf, prefix) {
            let (key, value) = item.map_err(|e| {
                anyhow!(
                    "CKB block body read failed: block=0x{} after {} transactions: {e}",
                    hex::encode(hash),
                    txs.len()
                )
            })?;
            if !key.starts_with(prefix) {
                break;
            }
            let reader = packed::TransactionViewReader::from_slice(&value).map_err(|e| {
                anyhow!(
                    "undecodable CKB block body row: block=0x{} body_key=0x{}: {e}",
                    hex::encode(hash),
                    hex::encode(&key)
                )
            })?;
            txs.push(reader.unpack());
        }
        Ok(txs)
    }

    fn get_block_proposals_packed(
        &self,
        hash: &[u8; 32],
    ) -> Result<Option<packed::ProposalShortIdVec>> {
        self.get_row(COLUMN_BLOCK_PROPOSAL_IDS, "proposals", hash)?
            .map(|raw| decode_proposals(hash, &raw))
            .transpose()
    }
}

fn decode_header(hash: &[u8; 32], raw: &[u8]) -> Result<HeaderView> {
    Ok(packed::HeaderViewReader::from_slice(raw)
        .map_err(|e| {
            anyhow!(
                "undecodable CKB block header row: block=0x{}: {e}",
                hex::encode(hash)
            )
        })?
        .unpack())
}

fn decode_uncles(hash: &[u8; 32], raw: &[u8]) -> Result<UncleBlockVecView> {
    Ok(packed::UncleBlockVecViewReader::from_slice(raw)
        .map_err(|e| {
            anyhow!(
                "undecodable CKB block uncles row: block=0x{}: {e}",
                hex::encode(hash)
            )
        })?
        .unpack())
}

fn decode_proposals(hash: &[u8; 32], raw: &[u8]) -> Result<packed::ProposalShortIdVec> {
    Ok(packed::ProposalShortIdVecReader::from_slice(raw)
        .map_err(|e| {
            anyhow!(
                "undecodable CKB block proposals row: block=0x{}: {e}",
                hex::encode(hash)
            )
        })?
        .to_entity())
}

struct SecondaryDir {
    path: PathBuf,
}

static SECONDARY_DIR_SEQUENCE: AtomicU64 = AtomicU64::new(0);

impl SecondaryDir {
    fn create_for_process() -> Result<Self> {
        // Secondary instances need their own directory for manifest tracking.
        // Keep process identity in the name for log correlation, but make the directory
        // unique per reader instance so concurrent readers in the same process do not race.
        let sequence = SECONDARY_DIR_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "ckbadger-rocksdb-secondary-{}-{}",
            std::process::id(),
            sequence
        ));
        std::fs::create_dir_all(&path)
            .map_err(|e| anyhow!("Failed to create secondary path {}: {}", path.display(), e))?;
        Ok(Self { path })
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for SecondaryDir {
    fn drop(&mut self) {
        if let Err(error) = std::fs::remove_dir_all(&self.path) {
            if error.kind() != std::io::ErrorKind::NotFound {
                debug!(
                    "Failed to remove CKB secondary directory {}: {}",
                    self.path.display(),
                    error
                );
            }
        }
    }
}

/// Parse a raw cellbase first-witness as the `CellbaseWitness` molecule
/// (RFC-0022: `lock: Script, message: Bytes`), returning the miner's declared
/// lock script and the miner message. Returns `None` when the bytes are not a
/// valid `CellbaseWitness` (it is NOT a `WitnessArgs` — parsing it as one was
/// the historical bug that made miner messages come back empty).
pub fn parse_cellbase_witness(data: &[u8]) -> Option<(packed::Script, Vec<u8>)> {
    let reader = packed::CellbaseWitnessReader::from_slice(data).ok()?;
    let witness = reader.to_entity();
    let message = witness.message().raw_data().to_vec();
    Some((witness.lock(), message))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_column_family_names() {
        assert_eq!(ALL_COLUMN_FAMILIES.len(), 19);
        assert_eq!(COLUMN_INDEX, "0");
        assert_eq!(COLUMN_BLOCK_HEADER, "1");
        assert_eq!(COLUMN_BLOCK_BODY, "2");
        assert_eq!(COLUMN_META, "4");
    }

    #[test]
    fn test_meta_tip_header_key() {
        assert_eq!(META_TIP_HEADER_KEY, b"TIP_HEADER");
    }

    /// Regression (F5/F8): the cellbase first witness is a CellbaseWitness
    /// molecule, not WitnessArgs. Vector: mainnet block 12,000,000. The lock
    /// inside the witness is the block's TRUE miner (args 0x8211f1b9…), which
    /// differs from the cellbase output lock (that one pays block 11,999,989's
    /// miner). The message carries the miner's client version string.
    #[test]
    fn test_parse_cellbase_witness_mainnet_block_12m() {
        let witness = hex::decode(
            "7a0000000c00000055000000490000001000000030000000310000009bd7e06f3ecf4be0f2f\
             cd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce801140000008211f1b938a107cd53b6\
             302cc752a6fc3965638d210000000000000020302e3131332e3020283832383731613320323\
             032342d30312d303929",
        )
        .unwrap();

        let (lock, message) =
            parse_cellbase_witness(&witness).expect("mainnet cellbase witness must parse");
        assert_eq!(
            hex::encode(lock.code_hash().raw_data()),
            "9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8",
        );
        assert_eq!(lock.hash_type().as_bytes()[0], 1); // type
        assert_eq!(
            hex::encode(lock.args().raw_data()),
            "8211f1b938a107cd53b6302cc752a6fc3965638d",
        );
        assert!(
            String::from_utf8_lossy(&message).contains("0.113.0"),
            "message must contain the miner client version",
        );

        // The historical bug: parsing the same bytes as WitnessArgs must fail —
        // it silently produced None for every block.
        assert!(packed::WitnessArgsReader::from_slice(&witness).is_err());
    }

    /// Genesis cellbase witness parses as CellbaseWitness with an all-zero
    /// lock script (the genesis block has no miner; callers special-case it).
    #[test]
    fn test_parse_cellbase_witness_genesis() {
        let mut genesis_hex = String::from("450000000c00000041000000350000001000000030000000");
        genesis_hex.push_str("31000000");
        genesis_hex.push_str(&"00".repeat(41)); // code_hash(32) + hash_type(1) + args len(4) + message len(4)
        let witness = hex::decode(&genesis_hex).unwrap();
        assert_eq!(witness.len(), 69);
        let (lock, message) = parse_cellbase_witness(&witness).expect("genesis witness parses");
        assert_eq!(
            lock.code_hash().raw_data().to_vec(),
            vec![0u8; 32],
            "genesis witness lock is the zero script"
        );
        assert!(message.is_empty());
    }

    #[test]
    fn test_block_number_key_encoding() {
        let number: u64 = 12345;
        let key = number.to_le_bytes();
        assert_eq!(key.len(), 8);
        let decoded = u64::from_le_bytes(key);
        assert_eq!(decoded, number);
    }

    #[test]
    fn secondary_dirs_are_unique_per_reader_instance() {
        let first = SecondaryDir::create_for_process().unwrap();
        let second = SecondaryDir::create_for_process().unwrap();

        assert_ne!(first.path(), second.path());
        assert!(first.path().exists());
        assert!(second.path().exists());
    }

    #[test]
    fn ckb_reader_drop_removes_secondary_directory() {
        let primary_dir = tempdir().unwrap();
        let mut db_opts = Options::default();
        db_opts.create_if_missing(true);
        db_opts.create_missing_column_families(true);
        let cf_descriptors: Vec<ColumnFamilyDescriptor> = ALL_COLUMN_FAMILIES
            .iter()
            .map(|name| ColumnFamilyDescriptor::new(*name, Options::default()))
            .collect();
        let _primary_db =
            DB::open_cf_descriptors(&db_opts, primary_dir.path(), cf_descriptors).unwrap();

        let secondary_path;

        {
            let reader = CkbChainReader::open(primary_dir.path().to_str().unwrap()).unwrap();
            secondary_path = reader._secondary_dir.path().to_path_buf();
            assert!(secondary_path.exists());
            assert!(reader.tip_number().is_none());
        }

        assert!(
            !secondary_path.exists(),
            "secondary path should be removed when reader is dropped: {}",
            secondary_path.display()
        );
    }

    #[test]
    fn batch_methods_return_empty_for_empty_input() {
        let primary_dir = tempdir().unwrap();
        let mut db_opts = Options::default();
        db_opts.create_if_missing(true);
        db_opts.create_missing_column_families(true);
        let cf_descriptors: Vec<ColumnFamilyDescriptor> = ALL_COLUMN_FAMILIES
            .iter()
            .map(|name| ColumnFamilyDescriptor::new(*name, Options::default()))
            .collect();
        let _primary_db =
            DB::open_cf_descriptors(&db_opts, primary_dir.path(), cf_descriptors).unwrap();

        let reader = CkbChainReader::open(primary_dir.path().to_str().unwrap()).unwrap();
        let secondary_path = reader._secondary_dir.path().to_path_buf();

        assert!(reader.get_block_hashes_batch(&[]).is_empty());
        assert!(reader.get_blocks_batch(&[]).unwrap().is_empty());
        assert!(reader.get_block_exts_batch(&[]).is_empty());
        assert!(secondary_path.exists());
    }

    #[test]
    fn block_read_preserves_extension_and_serialized_size() {
        let primary_dir = tempdir().unwrap();
        let mut options = Options::default();
        options.create_if_missing(true);
        options.create_missing_column_families(true);
        let descriptors = ALL_COLUMN_FAMILIES
            .iter()
            .map(|name| ColumnFamilyDescriptor::new(*name, Options::default()));
        let primary = DB::open_cf_descriptors(&options, primary_dir.path(), descriptors).unwrap();
        let extension = ckb_types::bytes::Bytes::from(vec![0x42; 32]).pack();
        let block = ckb_types::core::BlockBuilder::default()
            .extension(Some(extension.clone()))
            .build();
        let hash: [u8; 32] = block.hash().unpack();
        for (name, bytes) in [
            ("1", block.header().pack().as_slice().to_vec()),
            ("3", block.uncles().pack().as_slice().to_vec()),
            ("7", block.data().proposals().as_slice().to_vec()),
            ("15", extension.as_slice().to_vec()),
        ] {
            let cf = primary.cf_handle(name).unwrap();
            primary.put_cf(&cf, hash, bytes).unwrap();
            primary.flush_cf(&cf).unwrap();
        }
        let reader = CkbChainReader::open(primary_dir.path().to_str().unwrap()).unwrap();
        let actual = reader.get_block(&hash).unwrap().unwrap();
        assert_eq!(actual.extension(), Some(extension));
        assert_eq!(
            actual.data().as_slice().len(),
            block.data().as_slice().len()
        );
    }

    #[test]
    fn batch_methods_return_none_for_missing_blocks() {
        let primary_dir = tempdir().unwrap();
        let mut db_opts = Options::default();
        db_opts.create_if_missing(true);
        db_opts.create_missing_column_families(true);
        let cf_descriptors: Vec<ColumnFamilyDescriptor> = ALL_COLUMN_FAMILIES
            .iter()
            .map(|name| ColumnFamilyDescriptor::new(*name, Options::default()))
            .collect();
        let _primary_db =
            DB::open_cf_descriptors(&db_opts, primary_dir.path(), cf_descriptors).unwrap();

        let reader = CkbChainReader::open(primary_dir.path().to_str().unwrap()).unwrap();
        let secondary_path = reader._secondary_dir.path().to_path_buf();

        // Non-existent block numbers should return None
        let hashes = reader.get_block_hashes_batch(&[0, 1, 2]);
        assert_eq!(hashes.len(), 3);
        assert!(hashes.iter().all(|h| h.is_none()));

        // Non-existent block hashes should return None
        let fake_hashes = [[0u8; 32], [1u8; 32]];
        let blocks = reader.get_blocks_batch(&fake_hashes).unwrap();
        assert_eq!(blocks.len(), 2);
        assert!(blocks.iter().all(|b| b.is_none()));

        let exts = reader.get_block_exts_batch(&fake_hashes);
        assert_eq!(exts.len(), 2);
        assert!(exts.iter().all(|e| e.is_none()));
        assert!(secondary_path.exists());
    }

    /// A block shaped like a mainnet one: several transactions whose witnesses
    /// differ from their raw parts (so witness hash != hash), two uncles,
    /// proposals, and optionally an extension. All hashes and roots valid.
    fn fixture_block(number: u64, with_extension: bool) -> BlockView {
        use ckb_types::core::{
            BlockBuilder, Capacity, EpochNumberWithFraction, TransactionBuilder,
        };

        let script = |byte: u8| {
            packed::Script::new_builder()
                .code_hash(packed::Byte32::new([byte; 32]))
                .hash_type(ckb_types::core::ScriptHashType::Type.into())
                .args(ckb_types::bytes::Bytes::from(vec![byte; 20]).pack())
                .build()
        };
        let output = |byte: u8, shannons: u64| {
            packed::CellOutput::new_builder()
                .capacity(Capacity::shannons(shannons).pack())
                .lock(script(byte))
                .build()
        };
        let cellbase = TransactionBuilder::default()
            .input(packed::CellInput::new_cellbase_input(number))
            .output(output(0x11, 100_000_000_000))
            .output_data(ckb_types::bytes::Bytes::new().pack())
            .witness(ckb_types::bytes::Bytes::from(vec![0x77; 40]).pack())
            .build();
        let spend = |byte: u8| {
            TransactionBuilder::default()
                .input(packed::CellInput::new(
                    packed::OutPoint::new(packed::Byte32::new([byte; 32]), number as u32),
                    0,
                ))
                .output(output(byte, 20_000_000_000))
                .output(output(byte.wrapping_add(1), 30_000_000_000))
                .output_data(ckb_types::bytes::Bytes::from(vec![byte; 16]).pack())
                .output_data(ckb_types::bytes::Bytes::new().pack())
                .witness(ckb_types::bytes::Bytes::from(vec![byte; 65]).pack())
                .build()
        };
        let uncle = |nonce: u128| {
            BlockBuilder::default()
                .number((number - 1).pack())
                .epoch(EpochNumberWithFraction::new(10, 4, 1800).pack())
                .compact_target(0x1a08_a97e_u32.pack())
                .timestamp(1_700_000_000_000u64.pack())
                .nonce(nonce.pack())
                .proposal(packed::ProposalShortId::new([nonce as u8; 10]))
                .build()
                .as_uncle()
        };
        let builder = BlockBuilder::default()
            .number(number.pack())
            .epoch(EpochNumberWithFraction::new(10, 5, 1800).pack())
            .compact_target(0x1a08_a97e_u32.pack())
            .timestamp(1_700_000_010_000u64.pack())
            .parent_hash(packed::Byte32::new([0x0f; 32]))
            .dao(packed::Byte32::new([0x0d; 32]))
            .nonce(0x0123_4567_89ab_cdef_u128.pack())
            .transaction(cellbase)
            .transaction(spend(0x21))
            .transaction(spend(0x31))
            .uncle(uncle(1))
            .uncle(uncle(2))
            .proposal(packed::ProposalShortId::new([0xa1; 10]))
            .proposal(packed::ProposalShortId::new([0xa2; 10]));
        let builder = if with_extension {
            builder.extension(Some(ckb_types::bytes::Bytes::from(vec![0x42; 32]).pack()))
        } else {
            builder
        };
        builder.build()
    }

    /// The hashes a fixture is stored under. `Real` stores each object's own
    /// hash; `Sentinel` stores made-up ones, so a reader that recomputes a
    /// hash instead of reading it returns a value that differs from the store.
    #[derive(Clone, Copy, PartialEq)]
    enum StoredHashes {
        Real,
        Sentinel,
    }

    fn sentinel(seed: u8, block_number: u64, index: usize) -> packed::Byte32 {
        let mut bytes = [seed; 32];
        bytes[30] = block_number as u8;
        bytes[31] = index as u8;
        packed::Byte32::new(bytes)
    }

    /// Write one block into a primary node store exactly as the node lays it
    /// out (header view, uncle views, tx views keyed `block_hash ++ be32(i)`,
    /// proposals, extension, number index, tx info). Returns the block key.
    fn store_block(primary: &DB, block: &BlockView, hashes: StoredHashes) -> [u8; 32] {
        let header_hash = match hashes {
            StoredHashes::Real => block.hash(),
            StoredHashes::Sentinel => sentinel(0xb0, block.number(), 0),
        };
        let key: [u8; 32] = header_hash.unpack();
        let header_view = packed::HeaderView::new_builder()
            .data(block.data().header())
            .hash(header_hash)
            .build();
        let uncle_hashes = match hashes {
            StoredHashes::Real => block.uncle_hashes(),
            StoredHashes::Sentinel => (0..block.uncle_hashes().len())
                .map(|index| sentinel(0xc0, block.number(), index))
                .collect::<Vec<_>>()
                .pack(),
        };
        let uncles_view = packed::UncleBlockVecView::new_builder()
            .data(block.data().uncles())
            .hashes(uncle_hashes)
            .build();
        let put = |cf: &str, key: &[u8], value: &[u8]| {
            primary
                .put_cf(&primary.cf_handle(cf).unwrap(), key, value)
                .unwrap();
        };
        put(COLUMN_INDEX, &block.number().to_le_bytes(), &key);
        put(COLUMN_BLOCK_HEADER, &key, header_view.as_slice());
        put(COLUMN_BLOCK_UNCLE, &key, uncles_view.as_slice());
        put(
            COLUMN_BLOCK_PROPOSAL_IDS,
            &key,
            block.data().proposals().as_slice(),
        );
        if let Some(extension) = block.extension() {
            put(COLUMN_BLOCK_EXTENSION, &key, extension.as_slice());
        }
        for (index, tx) in block.transactions().into_iter().enumerate() {
            let (tx_hash, witness_hash) = match hashes {
                StoredHashes::Real => (tx.hash(), tx.witness_hash()),
                StoredHashes::Sentinel => (
                    sentinel(0xd0, block.number(), index),
                    sentinel(0xe0, block.number(), index),
                ),
            };
            let tx_view = packed::TransactionView::new_builder()
                .data(tx.data())
                .hash(tx_hash.clone())
                .witness_hash(witness_hash)
                .build();
            let mut body_key = key.to_vec();
            body_key.extend_from_slice(&(index as u32).to_be_bytes());
            put(COLUMN_BLOCK_BODY, &body_key, tx_view.as_slice());
            let info = packed::TransactionInfo::new_builder()
                .block_number(block.number().pack())
                .block_epoch(block.epoch().full_value().pack())
                .key(
                    packed::TransactionKey::new_builder()
                        .block_hash(header_view.hash())
                        .index((index as u32).pack())
                        .build(),
                )
                .build();
            put(COLUMN_TRANSACTION_INFO, tx_hash.as_slice(), info.as_slice());
        }
        for cf in ALL_COLUMN_FAMILIES {
            primary.flush_cf(&primary.cf_handle(cf).unwrap()).unwrap();
        }
        key
    }

    fn open_primary(dir: &Path) -> DB {
        let mut options = Options::default();
        options.create_if_missing(true);
        options.create_missing_column_families(true);
        let descriptors = ALL_COLUMN_FAMILIES
            .iter()
            .map(|name| ColumnFamilyDescriptor::new(*name, Options::default()));
        DB::open_cf_descriptors(&options, dir, descriptors).unwrap()
    }

    /// Everything a `BlockView` carries. Its `PartialEq` compares only the
    /// block hash.
    #[derive(Debug, PartialEq)]
    struct BlockFields {
        data: Vec<u8>,
        hash: packed::Byte32,
        uncle_hashes: Vec<u8>,
        tx_hashes: Vec<packed::Byte32>,
        tx_witness_hashes: Vec<packed::Byte32>,
    }

    fn block_fields(view: &BlockView) -> BlockFields {
        BlockFields {
            data: view.data().as_slice().to_vec(),
            hash: view.hash(),
            uncle_hashes: view.uncle_hashes().as_slice().to_vec(),
            tx_hashes: view.tx_hashes().to_vec(),
            tx_witness_hashes: view.tx_witness_hashes().to_vec(),
        }
    }

    /// D-2 red test: every read returns the hashes the node stored (here made
    /// up), which proves it did not hash the transactions, uncles or header
    /// again. The node writes those hashes when it stores the block.
    #[test]
    fn block_and_transaction_reads_return_the_stored_hashes() {
        let primary_dir = tempdir().unwrap();
        let primary = open_primary(primary_dir.path());
        let with_extension = fixture_block(1_234, true);
        let without_extension = fixture_block(1_235, false);
        let keys = [
            store_block(&primary, &with_extension, StoredHashes::Sentinel),
            store_block(&primary, &without_extension, StoredHashes::Sentinel),
        ];
        let reader = CkbChainReader::open(primary_dir.path().to_str().unwrap()).unwrap();

        let batch = reader.get_blocks_batch(&keys).unwrap();
        for ((key, block), view) in keys
            .iter()
            .zip([&with_extension, &without_extension])
            .zip(&batch)
        {
            let view = view.as_ref().expect("stored block must be read");
            let single = reader
                .get_block(key)
                .unwrap()
                .expect("stored block must be read");
            for view in [view, &single] {
                assert_eq!(view.hash().as_slice(), key.as_slice());
                assert_eq!(view.header().hash().as_slice(), key.as_slice());
                let number = block.number();
                assert_eq!(
                    view.uncle_hashes().as_slice(),
                    vec![sentinel(0xc0, number, 0), sentinel(0xc0, number, 1)]
                        .pack()
                        .as_slice()
                );
                assert_eq!(
                    view.tx_hashes(),
                    (0..3)
                        .map(|index| sentinel(0xd0, number, index))
                        .collect::<Vec<_>>()
                );
                assert_eq!(
                    view.tx_witness_hashes(),
                    (0..3)
                        .map(|index| sentinel(0xe0, number, index))
                        .collect::<Vec<_>>()
                );
                // The block's own bytes are still the node's bytes.
                assert_eq!(
                    view.data().header().as_slice(),
                    block.data().header().as_slice()
                );
                assert_eq!(
                    view.data().transactions().as_slice(),
                    block.data().transactions().as_slice()
                );
            }
        }

        let (tx, number) = reader
            .get_transaction_with_block_number(&sentinel(0xd0, 1_234, 1).unpack())
            .unwrap()
            .expect("stored transaction must be read");
        assert_eq!(number, 1_234);
        assert_eq!(tx.hash(), sentinel(0xd0, 1_234, 1));
        assert_eq!(tx.witness_hash(), sentinel(0xe0, 1_234, 1));
        assert_eq!(
            tx.data().as_slice(),
            with_extension.transactions()[1].data().as_slice()
        );
    }

    /// The constructions `get_blocks_batch`, `get_block`,
    /// `get_transaction_with_block_number` and `get_block_header_info` used
    /// before D-2 (they recomputed every hash the node had stored), kept as
    /// the oracle for the hash-preserving ones.
    mod rehashing_oracle {
        use super::*;

        // The pre-D-2 private readers, verbatim apart from `unwrap`.
        fn get_block_header_packed(reader: &CkbChainReader, hash: &[u8; 32]) -> packed::HeaderView {
            let cf = reader.db.cf_handle(COLUMN_BLOCK_HEADER).unwrap();
            let raw = reader.db.get_cf(&cf, hash).unwrap().unwrap();
            packed::HeaderViewReader::from_slice(&raw)
                .unwrap()
                .to_entity()
        }

        fn get_block_uncles_packed(
            reader: &CkbChainReader,
            hash: &[u8; 32],
        ) -> packed::UncleBlockVecView {
            let cf = reader.db.cf_handle(COLUMN_BLOCK_UNCLE).unwrap();
            let raw = reader.db.get_cf(&cf, hash).unwrap().unwrap();
            packed::UncleBlockVecViewReader::from_slice(&raw)
                .unwrap()
                .to_entity()
        }

        fn get_block_proposals_packed(
            reader: &CkbChainReader,
            hash: &[u8; 32],
        ) -> packed::ProposalShortIdVec {
            let cf = reader.db.cf_handle(COLUMN_BLOCK_PROPOSAL_IDS).unwrap();
            let raw = reader.db.get_cf(&cf, hash).unwrap().unwrap();
            packed::ProposalShortIdVecReader::from_slice(&raw)
                .unwrap()
                .to_entity()
        }

        fn get_block_body_packed(
            reader: &CkbChainReader,
            hash: &[u8; 32],
        ) -> Vec<packed::TransactionView> {
            let cf = reader.db.cf_handle(COLUMN_BLOCK_BODY).unwrap();
            let mut txs = Vec::new();
            for item in reader.db.prefix_iterator_cf(&cf, hash) {
                let (key, value) = item.unwrap();
                if !key.starts_with(hash) {
                    break;
                }
                txs.push(
                    packed::TransactionViewReader::from_slice(&value)
                        .unwrap()
                        .to_entity(),
                );
            }
            txs
        }

        pub(super) fn batch_block(reader: &CkbChainReader, hash: &[u8; 32]) -> BlockView {
            let header = get_block_header_packed(reader, hash);
            let uncles = get_block_uncles_packed(reader, hash);
            let proposals = get_block_proposals_packed(reader, hash);
            let transactions = get_block_body_packed(reader, hash);
            let tx_views: Vec<ckb_types::core::TransactionView> = transactions
                .into_iter()
                .map(|tv| {
                    let hash = tv.hash();
                    let witness_hash = tv.witness_hash();
                    tv.data()
                        .into_view()
                        .fake_hash(hash)
                        .fake_witness_hash(witness_hash)
                })
                .collect();
            let block = packed::Block::new_builder()
                .header(header.data())
                .uncles(uncles.data())
                .transactions(
                    packed::TransactionVec::new_builder()
                        .set(tx_views.iter().map(|tv| tv.data()).collect::<Vec<_>>())
                        .build(),
                )
                .proposals(proposals)
                .build();
            block.into_view().fake_hash(packed::Byte32::new(*hash))
        }

        pub(super) fn single_block(reader: &CkbChainReader, hash: &[u8; 32]) -> BlockView {
            let header = get_block_header_packed(reader, hash);
            let uncles = get_block_uncles_packed(reader, hash);
            let transactions = get_block_body_packed(reader, hash);
            let proposals = get_block_proposals_packed(reader, hash);
            let extension_cf = reader.db.cf_handle(COLUMN_BLOCK_EXTENSION).unwrap();
            let extension = reader
                .db
                .get_cf(&extension_cf, hash)
                .unwrap()
                .map(|bytes| packed::Bytes::from_slice(&bytes).unwrap());
            let tx_views: Vec<ckb_types::core::TransactionView> = transactions
                .into_iter()
                .map(|tv| {
                    let hash = tv.hash();
                    let witness_hash = tv.witness_hash();
                    tv.data()
                        .into_view()
                        .fake_hash(hash)
                        .fake_witness_hash(witness_hash)
                })
                .collect();
            let block = packed::Block::new_builder()
                .header(header.data())
                .uncles(uncles.data())
                .transactions(
                    packed::TransactionVec::new_builder()
                        .set(tx_views.iter().map(|tv| tv.data()).collect::<Vec<_>>())
                        .build(),
                )
                .proposals(proposals)
                .build();
            block
                .into_view()
                .as_advanced_builder()
                .extension(extension)
                .build()
                .fake_hash(packed::Byte32::new(*hash))
        }

        pub(super) fn transaction(
            reader: &CkbChainReader,
            tx_hash: &[u8; 32],
        ) -> ckb_types::core::TransactionView {
            let cf_info = reader.db.cf_handle(COLUMN_TRANSACTION_INFO).unwrap();
            let raw_info = reader.db.get_cf(&cf_info, tx_hash).unwrap().unwrap();
            let tx_info = packed::TransactionInfoReader::from_slice(&raw_info).unwrap();
            let cf_body = reader.db.cf_handle(COLUMN_BLOCK_BODY).unwrap();
            let raw_tx = reader
                .db
                .get_cf(&cf_body, tx_info.key().as_slice())
                .unwrap()
                .unwrap();
            let entity = packed::TransactionViewReader::from_slice(&raw_tx)
                .unwrap()
                .to_entity();
            let hash = entity.hash();
            let witness_hash = entity.witness_hash();
            entity
                .data()
                .into_view()
                .fake_hash(hash)
                .fake_witness_hash(witness_hash)
        }

        pub(super) fn header_info(reader: &CkbChainReader, hash: &[u8; 32]) -> BlockHeaderInfo {
            let view = get_block_header_packed(reader, hash).data().into_view();
            BlockHeaderInfo {
                parent_hash: view.parent_hash().unpack(),
                nonce: view.nonce(),
                transactions_root: view.transactions_root().unpack(),
                version: view.version(),
            }
        }
    }

    /// D-2 oracle: on a store holding the real hashes, the hash-preserving
    /// reads produce what the rehashing ones did, field for field, and both
    /// equal the block as built. One difference is by construction: the batch
    /// read carries no extension (it never did; nothing on the bulk path reads
    /// it), and the old batch construction rebuilt the header's `extra_hash`
    /// from the uncles alone, which for a block with an extension is not the
    /// stored value. The new batch read keeps the stored header.
    #[test]
    fn hash_preserving_reads_match_the_rehashing_construction() {
        let primary_dir = tempdir().unwrap();
        let primary = open_primary(primary_dir.path());
        let with_extension = fixture_block(1_234, true);
        let without_extension = fixture_block(1_235, false);
        let blocks = [&with_extension, &without_extension];
        let keys = blocks.map(|block| store_block(&primary, block, StoredHashes::Real));
        let reader = CkbChainReader::open(primary_dir.path().to_str().unwrap()).unwrap();

        let batch = reader.get_blocks_batch(&keys).unwrap();
        for ((key, block), view) in keys.iter().zip(blocks).zip(&batch) {
            let view = view.as_ref().expect("stored block must be read");
            let oracle = rehashing_oracle::batch_block(&reader, key);
            assert_eq!(view.hash().as_slice(), key.as_slice());
            assert_eq!(view.hash(), block.hash());
            assert_eq!(
                view.uncle_hashes().as_slice(),
                block.uncle_hashes().as_slice()
            );
            assert_eq!(view.tx_hashes(), block.tx_hashes());
            assert_eq!(view.tx_witness_hashes(), block.tx_witness_hashes());
            assert_eq!(
                view.data().proposals().as_slice(),
                block.data().proposals().as_slice()
            );
            assert_eq!(
                view.data().uncles().as_slice(),
                block.data().uncles().as_slice()
            );
            assert_eq!(
                view.data().header().as_slice(),
                block.data().header().as_slice()
            );
            assert_eq!(view.extension(), None);
            assert_eq!(oracle.extension(), None);
            assert_eq!(view.data().total_size(), oracle.data().total_size());
            if block.extension().is_none() {
                assert_eq!(block_fields(view), block_fields(&oracle));
            } else {
                let view_fields = block_fields(view);
                let oracle_fields = block_fields(&oracle);
                assert_eq!(
                    BlockFields {
                        data: Vec::new(),
                        ..view_fields
                    },
                    BlockFields {
                        data: Vec::new(),
                        ..oracle_fields
                    }
                );
                assert_ne!(
                    view.data().as_slice(),
                    oracle.data().as_slice(),
                    "the old batch header rewrote extra_hash"
                );
                assert_eq!(
                    oracle.extra_hash(),
                    block.calc_uncles_hash(),
                    "the old batch header's extra_hash is the uncles hash alone"
                );
                assert_eq!(view.extra_hash(), block.extra_hash());
                // Apart from extra_hash the old header was the stored one.
                assert_eq!(
                    oracle
                        .header()
                        .as_advanced_builder()
                        .extra_hash(block.extra_hash())
                        .build()
                        .data()
                        .as_slice(),
                    block.data().header().as_slice()
                );
            }

            let single = reader
                .get_block(key)
                .unwrap()
                .expect("stored block must be read");
            let single_oracle = rehashing_oracle::single_block(&reader, key);
            assert_eq!(block_fields(&single), block_fields(&single_oracle));
            assert_eq!(block_fields(&single), block_fields(block));
            assert_eq!(single.extension(), block.extension());

            let info = reader.get_block_header_info(key).unwrap().unwrap();
            let info_oracle = rehashing_oracle::header_info(&reader, key);
            assert_eq!(
                (
                    info.parent_hash,
                    info.nonce,
                    info.transactions_root,
                    info.version
                ),
                (
                    info_oracle.parent_hash,
                    info_oracle.nonce,
                    info_oracle.transactions_root,
                    info_oracle.version
                )
            );
            assert_eq!(info.parent_hash, [0x0f; 32]);
            assert_eq!(info.nonce, 0x0123_4567_89ab_cdef);

            for tx in block.transactions() {
                let tx_hash: [u8; 32] = tx.hash().unpack();
                let (read, number) = reader
                    .get_transaction_with_block_number(&tx_hash)
                    .unwrap()
                    .unwrap();
                let oracle = rehashing_oracle::transaction(&reader, &tx_hash);
                assert_eq!(number, block.number());
                assert_eq!(read.hash(), oracle.hash());
                assert_eq!(read.witness_hash(), oracle.witness_hash());
                assert_eq!(read.data().as_slice(), oracle.data().as_slice());
                assert_eq!(read.hash(), tx.hash());
                assert_eq!(read.witness_hash(), tx.witness_hash());
            }
        }
    }

    /// Overwrite one row of a fixture store with `value` and flush it.
    fn put_row(primary: &DB, cf: &str, key: &[u8], value: &[u8]) {
        let handle = primary.cf_handle(cf).unwrap();
        primary.put_cf(&handle, key, value).unwrap();
        primary.flush_cf(&handle).unwrap();
    }

    fn body_key(block_hash: &[u8; 32], index: u32) -> Vec<u8> {
        let mut key = block_hash.to_vec();
        key.extend_from_slice(&index.to_be_bytes());
        key
    }

    fn cell_key(tx_hash: &[u8; 32], index: u32) -> Vec<u8> {
        let mut key = tx_hash.to_vec();
        key.extend_from_slice(&index.to_le_bytes());
        key
    }

    /// D-2b: a body row that does not decode fails the block read with the
    /// block and row named; before, both reads returned the block without that
    /// transaction. A block that is not stored stays `None`.
    #[test]
    fn an_undecodable_body_row_fails_the_block_read() {
        let primary_dir = tempdir().unwrap();
        let primary = open_primary(primary_dir.path());
        let block = fixture_block(1_234, true);
        let key = store_block(&primary, &block, StoredHashes::Real);
        put_row(&primary, COLUMN_BLOCK_BODY, &body_key(&key, 1), &[1, 2, 3]);
        let reader = CkbChainReader::open(primary_dir.path().to_str().unwrap()).unwrap();

        let expected_key = hex::encode(body_key(&key, 1));
        for error in [
            reader.get_block(&key).expect_err("single read"),
            reader
                .get_block_by_number(1_234)
                .expect_err("read by number"),
            reader.get_blocks_batch(&[key]).expect_err("batch read"),
        ] {
            let message = format!("{error:#}");
            assert!(
                message.contains("undecodable CKB block body row")
                    && message.contains(&hex::encode(key))
                    && message.contains(&expected_key),
                "{message}"
            );
        }

        assert!(reader.get_block(&[0x5a; 32]).unwrap().is_none());
        assert!(reader.get_block_by_number(9_999).unwrap().is_none());
        let batch = reader.get_blocks_batch(&[[0x5a; 32]]).unwrap();
        assert_eq!(batch.len(), 1);
        assert!(batch[0].is_none());
    }

    /// D-2b: a stored header without its uncles or proposals row is a store no
    /// node writes (one write batch per block), so it is an error, not `None`.
    #[test]
    fn a_header_without_its_uncles_or_proposals_row_fails_the_block_read() {
        for (cf, what) in [
            (COLUMN_BLOCK_UNCLE, "uncles"),
            (COLUMN_BLOCK_PROPOSAL_IDS, "proposals"),
        ] {
            let primary_dir = tempdir().unwrap();
            let primary = open_primary(primary_dir.path());
            let block = fixture_block(1_234, false);
            let key = store_block(&primary, &block, StoredHashes::Real);
            let handle = primary.cf_handle(cf).unwrap();
            primary.delete_cf(&handle, key).unwrap();
            primary.flush_cf(&handle).unwrap();
            let reader = CkbChainReader::open(primary_dir.path().to_str().unwrap()).unwrap();
            for error in [
                reader.get_block(&key).expect_err("single read"),
                reader.get_blocks_batch(&[key]).expect_err("batch read"),
            ] {
                let message = format!("{error:#}");
                assert!(
                    message.contains(what) && message.contains(&hex::encode(key)),
                    "{message}"
                );
            }
        }
    }

    /// D-2b: transaction reads tell "not stored" (`None`) from "stored but
    /// unreadable" (an error naming the key); before, both were `None`.
    #[test]
    fn transaction_reads_tell_absent_from_unreadable() {
        let primary_dir = tempdir().unwrap();
        let primary = open_primary(primary_dir.path());
        let block = fixture_block(1_234, true);
        let key = store_block(&primary, &block, StoredHashes::Real);
        let tx1: [u8; 32] = block.transactions()[1].hash().unpack();
        let tx2: [u8; 32] = block.transactions()[2].hash().unpack();
        // An info row that does not decode.
        put_row(&primary, COLUMN_TRANSACTION_INFO, &[0x77; 32], &[9, 9]);
        // tx 1's body row does not decode; tx 2's body row is gone.
        put_row(&primary, COLUMN_BLOCK_BODY, &body_key(&key, 1), &[1, 2, 3]);
        let handle = primary.cf_handle(COLUMN_BLOCK_BODY).unwrap();
        primary.delete_cf(&handle, body_key(&key, 2)).unwrap();
        primary.flush_cf(&handle).unwrap();
        let reader = CkbChainReader::open(primary_dir.path().to_str().unwrap()).unwrap();

        assert!(reader.get_transaction(&[0x66; 32]).unwrap().is_none());
        assert!(reader
            .get_transaction_with_block_number(&[0x66; 32])
            .unwrap()
            .is_none());
        let (cellbase, number) = reader
            .get_transaction_with_block_number(&block.transactions()[0].hash().unpack())
            .unwrap()
            .expect("stored cellbase");
        assert_eq!(number, 1_234);
        assert_eq!(cellbase.hash(), block.transactions()[0].hash());

        for (tx_hash, needle) in [
            ([0x77; 32], "undecodable CKB transaction info row"),
            (tx1, "undecodable CKB block body row"),
            (tx2, "missing CKB block body row"),
        ] {
            for error in [
                reader
                    .get_transaction(&tx_hash)
                    .expect_err("get_transaction"),
                reader
                    .get_transaction_with_block_number(&tx_hash)
                    .expect_err("get_transaction_with_block_number"),
            ] {
                let message = format!("{error:#}");
                assert!(
                    message.contains(needle) && message.contains(&hex::encode(tx_hash)),
                    "{message}"
                );
            }
        }
    }

    /// D-2b: cell data reads. The node writes a `CellDataEntry` for a live
    /// cell with data and an empty value for one without; a consumed cell has
    /// no row and is read from its transaction. A non-empty row that does not
    /// decode is an error naming the outpoint (before, its bytes came back as
    /// the cell's data); a cell that does not exist stays `None`.
    #[test]
    fn cell_data_reads_tell_absent_from_unreadable() {
        let primary_dir = tempdir().unwrap();
        let primary = open_primary(primary_dir.path());
        let block = fixture_block(1_234, true);
        store_block(&primary, &block, StoredHashes::Real);
        let spend: [u8; 32] = block.transactions()[1].hash().unpack();
        let other: [u8; 32] = block.transactions()[2].hash().unpack();
        let live_data = vec![0x21; 16];
        let entry = packed::CellDataEntry::new_builder()
            .output_data(ckb_types::bytes::Bytes::from(live_data.clone()).pack())
            .output_data_hash(packed::Byte32::new([0x99; 32]))
            .build();
        put_row(
            &primary,
            COLUMN_CELL_DATA,
            &cell_key(&spend, 0),
            entry.as_slice(),
        );
        put_row(&primary, COLUMN_CELL_DATA, &cell_key(&spend, 1), &[]);
        put_row(&primary, COLUMN_CELL_DATA, &cell_key(&other, 1), &[5, 5, 5]);
        let reader = CkbChainReader::open(primary_dir.path().to_str().unwrap()).unwrap();

        assert_eq!(reader.get_cell_data(&spend, 0).unwrap(), Some(live_data));
        assert_eq!(reader.get_cell_data(&spend, 1).unwrap(), Some(Vec::new()));
        // Consumed: no cell data row, read from the transaction body.
        assert_eq!(
            reader.get_cell_data(&other, 0).unwrap(),
            Some(vec![0x31; 16])
        );
        assert_eq!(reader.get_cell_data(&other, 2).unwrap(), None);
        assert_eq!(reader.get_cell_data(&[0x66; 32], 0).unwrap(), None);

        let message = format!(
            "{:#}",
            reader
                .get_cell_data(&other, 1)
                .expect_err("corrupt cell data row")
        );
        assert!(
            message.contains("undecodable CKB cell data row")
                && message.contains(&format!("0x{}:1", hex::encode(other))),
            "{message}"
        );
    }
}
