//! The miner a block's cellbase witness names: the one path the block page,
//! fork rows and fork-detail blocks read their miner from — its lock hash,
//! its address and, from `pools.toml`, its pool name (R-12).

use anyhow::Context;
use ckb_store_reader::CkbChainReader;
use ckb_types::prelude::*;
use ckbadger_indexer::pool_labels::PoolLabels;

use crate::utils::script_to_address;

/// How miners are named on one network: addresses in its encoding, pools
/// from its `pools.toml` section.
#[derive(Clone, Copy)]
pub struct MinerNaming<'a> {
    pub network: &'a str,
    pub pools: &'a PoolLabels,
}

/// One block's miner, as its cellbase witness names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CellbaseMiner {
    /// The witness lock's script hash, `0x`-hex.
    pub lock_hash: String,
    pub address: String,
    /// The witness `message`, raw bytes.
    pub message: Vec<u8>,
    /// The pool `pools.toml` names for this lock or message; `None` when no
    /// entry does.
    pub pool: Option<String>,
}

impl MinerNaming<'_> {
    /// The miner of block `block_hash`; `None` only when the node does not
    /// store the block. A cellbase that cannot be read, or whose witness is
    /// missing or not a `CellbaseWitness`, is an error.
    pub fn cellbase_miner(
        &self,
        reader: &CkbChainReader,
        block_hash: &[u8; 32],
    ) -> anyhow::Result<Option<CellbaseMiner>> {
        let Some((lock, message)) = reader.get_cellbase_witness(block_hash)? else {
            return Ok(None);
        };
        let context = || format!("block 0x{}: miner witness", hex::encode(block_hash));
        let address = script_to_address(
            lock.code_hash().as_slice(),
            i16::from(lock.hash_type().as_slice()[0]),
            &lock.args().raw_data(),
            self.network,
        )
        .map_err(|e| anyhow::anyhow!("lock: {e}"))
        .with_context(context)?;
        let lock_hash: [u8; 32] = lock.calc_script_hash().unpack();
        let pool = self
            .pools
            .pool_of(&lock_hash, &message)
            .with_context(context)?
            .map(str::to_string);
        Ok(Some(CellbaseMiner {
            lock_hash: format!("0x{}", hex::encode(lock_hash)),
            address,
            message,
            pool,
        }))
    }
}
