use std::borrow::Cow;
use std::ops::Range;

use serde::Serialize;

use crate::sync::types::InternId;

#[derive(Debug, Default)]
pub(crate) struct FactsArena {
    pub(crate) blocks: Vec<BlockFacts>,
    pub(crate) txs: Vec<TxFacts>,
    pub(crate) cells: Vec<CellFacts>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct BlockFacts {
    pub(crate) number: i64,
    pub(crate) hash: [u8; 32],
    pub(crate) timestamp_ms: i64,
    pub(crate) epoch_number: i64,
    pub(crate) epoch_index: i32,
    pub(crate) epoch_length: i32,
    pub(crate) dao: [u8; 32],
    pub(crate) transactions_count: i32,
    pub(crate) tx_range: Range<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct SporeProtocolFacts {
    pub(crate) spore_id: [u8; 32],
    pub(crate) is_did: bool,
    pub(crate) content_type: String,
    pub(crate) content: Vec<u8>,
    pub(crate) cluster_id: Option<[u8; 32]>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct ClusterProtocolFacts {
    pub(crate) cluster_id: [u8; 32],
    pub(crate) name: Option<String>,
    pub(crate) description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct MnftIssuerProtocolFacts {
    pub(crate) issuer_id: [u8; 20],
    pub(crate) name: Option<String>,
    pub(crate) info: Option<Vec<u8>>,
    pub(crate) class_count: u32,
    pub(crate) set_count: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct MnftClassProtocolFacts {
    pub(crate) class_id: Vec<u8>,
    pub(crate) issuer_id: [u8; 20],
    pub(crate) name: Option<String>,
    pub(crate) description: Option<String>,
    pub(crate) renderer: Option<String>,
    pub(crate) total: u32,
    pub(crate) issued: u32,
    pub(crate) configure: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct MnftTokenProtocolFacts {
    pub(crate) token_id: Vec<u8>,
    pub(crate) class_id: Vec<u8>,
    pub(crate) token_index: u32,
    pub(crate) characteristic: Vec<u8>,
    pub(crate) configure: u8,
    pub(crate) state: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct DotbitProtocolFacts {
    pub(crate) account_id: [u8; 20],
    pub(crate) account: Option<String>,
    pub(crate) next_account_id: Option<[u8; 20]>,
    pub(crate) expired_at: Option<u64>,
    pub(crate) registered_at: Option<u64>,
    pub(crate) status: Option<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) enum CellProtocolFacts {
    Spore(SporeProtocolFacts),
    Cluster(ClusterProtocolFacts),
    MnftIssuer(MnftIssuerProtocolFacts),
    MnftClass(MnftClassProtocolFacts),
    MnftToken(MnftTokenProtocolFacts),
    Dotbit(DotbitProtocolFacts),
}

#[derive(Debug, Clone, Default)]
pub(crate) struct TxFacts {
    pub(crate) hash: [u8; 32],
    pub(crate) block_number: i64,
    pub(crate) block_hash: [u8; 32],
    pub(crate) timestamp_ms: i64,
    pub(crate) block_dao_ar: u64,
    pub(crate) tx_index: i32,
    pub(crate) is_cellbase: bool,
    pub(crate) inputs_count: i16,
    pub(crate) outputs_count: i16,
    pub(crate) tx_size: i32,
    pub(crate) cycles: Option<i64>,
    pub(crate) dotbit_action: Option<String>,
    pub(crate) input_outpoints: Vec<OutPointKey>,
    pub(crate) output_range: Range<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize)]
pub(crate) struct OutPointKey {
    pub(crate) tx_hash: [u8; 32],
    pub(crate) index: u32,
}

impl OutPointKey {
    pub(crate) const fn new(tx_hash: [u8; 32], index: u32) -> Self {
        Self { tx_hash, index }
    }
}

#[doc(hidden)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum CellSemanticTag {
    Plain,
    Dao,
    Sudt,
    Xudt,
    Dotbit,
    Mnft,
    Spore,
    Cluster,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub(crate) enum DaoCellState {
    Deposit,
    WithdrawRequest { deposit_block_number: i64 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub(crate) struct DaoCompensationArs {
    pub(crate) deposit_ar: u64,
    pub(crate) withdraw_request_ar: u64,
}

#[derive(Debug, Clone)]
pub(crate) struct CellFacts {
    pub(crate) outpoint: OutPointKey,
    pub(crate) created_at_block: i64,
    pub(crate) created_by_block_dao_ar: u64,
    pub(crate) capacity: i64,
    pub(crate) lock_script_hash_id: InternId,
    pub(crate) lock_code_hash_id: InternId,
    pub(crate) lock_hash_type: i16,
    pub(crate) lock_args_id: InternId,
    pub(crate) type_script_hash_id: Option<InternId>,
    pub(crate) type_code_hash_id: Option<InternId>,
    pub(crate) type_hash_type: Option<i16>,
    pub(crate) type_args_id: Option<InternId>,
    pub(crate) occupied_capacity: i64,
    pub(crate) data_size: i32,
    pub(crate) data: Vec<u8>,
    pub(crate) data_hash: Option<[u8; 32]>,
    pub(crate) udt_amount: Option<u128>,
    pub(crate) semantic_tag: CellSemanticTag,
    pub(crate) dao_state: Option<DaoCellState>,
    pub(crate) protocol_facts: Option<CellProtocolFacts>,
}

#[derive(Debug, Clone)]
pub(crate) struct ResolvedInputFacts {
    pub(crate) outpoint: OutPointKey,
    pub(crate) created_at_block: i64,
    pub(crate) created_by_block_dao_ar: u64,
    pub(crate) capacity: i64,
    pub(crate) occupied_capacity: i64,
    pub(crate) udt_amount: Option<u128>,
    pub(crate) lock_script_hash_id: InternId,
    pub(crate) lock_code_hash_id: InternId,
    pub(crate) lock_hash_type: i16,
    pub(crate) lock_args_id: InternId,
    pub(crate) type_script_hash_id: Option<InternId>,
    pub(crate) type_code_hash_id: Option<InternId>,
    pub(crate) type_hash_type: Option<i16>,
    pub(crate) type_args_id: Option<InternId>,
    pub(crate) semantic_tag: CellSemanticTag,
    pub(crate) dao_state: Option<DaoCellState>,
    pub(crate) dao_compensation_ars: Option<DaoCompensationArs>,
    pub(crate) protocol_facts: Option<CellProtocolFacts>,
}

#[derive(Debug, Clone)]
pub(crate) struct ResolvedTxFacts<'a> {
    pub(crate) tx_hash: [u8; 32],
    pub(crate) block_number: i64,
    pub(crate) block_hash: [u8; 32],
    pub(crate) timestamp_ms: i64,
    pub(crate) block_dao_ar: u64,
    pub(crate) tx_index: i32,
    pub(crate) dotbit_action: Option<String>,
    pub(crate) resolved_inputs: Vec<ResolvedInputFacts>,
    pub(crate) cells: Cow<'a, [CellFacts]>,
}

#[doc(hidden)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FactsArenaSnapshot {
    pub tx_count: usize,
    pub cells: Vec<CellFactsSnapshot>,
}

#[doc(hidden)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CellFactsSnapshot {
    pub occupied_capacity: i64,
    pub udt_amount: Option<u128>,
    pub semantic_tag: CellSemanticTag,
}

impl FactsArenaSnapshot {
    pub(crate) fn from_facts_arena(arena: &FactsArena) -> Self {
        Self {
            tx_count: arena.txs.len(),
            cells: arena
                .cells
                .iter()
                .map(|cell| CellFactsSnapshot {
                    occupied_capacity: cell.occupied_capacity,
                    udt_amount: cell.udt_amount,
                    semantic_tag: cell.semantic_tag,
                })
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn facts_arena_defaults_to_empty_indexes() {
        let arena = FactsArena::default();
        assert!(arena.blocks.is_empty());
        assert!(arena.txs.is_empty());
        assert!(arena.cells.is_empty());
    }
}
