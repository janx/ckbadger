use std::borrow::Cow;
use std::ops::Range;

use anyhow::{anyhow, Result};
use serde::Serialize;
use tracing::warn;

use crate::parser::bit_cell::BitCellParser;
use crate::parser::cell::ParsedCell;
use crate::parser::dotbit::{DotbitParser, DotbitWitnessBundle};
use crate::parser::mnft::MnftParser;
use crate::parser::spore::SporeParser;
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
    pub(crate) parent_hash: [u8; 32],
    pub(crate) timestamp_ms: i64,
    pub(crate) epoch_number: i64,
    pub(crate) epoch_index: i32,
    pub(crate) epoch_length: i32,
    pub(crate) dao: [u8; 32],
    pub(crate) compact_target: u32,
    pub(crate) uncles_count: i32,
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
pub(crate) struct BitCellProtocolFacts {
    pub(crate) identity_id: [u8; 32],
    pub(crate) account_id: [u8; 20],
    pub(crate) account: String,
    pub(crate) expired_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) enum CellProtocolFacts {
    Spore(SporeProtocolFacts),
    Cluster(ClusterProtocolFacts),
    MnftIssuer(MnftIssuerProtocolFacts),
    MnftClass(MnftClassProtocolFacts),
    MnftToken(MnftTokenProtocolFacts),
    Dotbit(DotbitProtocolFacts),
    BitCell(BitCellProtocolFacts),
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
    BitCell,
    Mnft,
    Spore,
    Cluster,
}

impl CellSemanticTag {
    /// Convert to bitmask bit for `TxIndexEntry.semantic_tags`.
    pub fn to_bit(self) -> u16 {
        use ckbadger_store::types::semantic_tags;
        match self {
            Self::Plain => semantic_tags::PLAIN,
            Self::Dao => semantic_tags::DAO,
            Self::Sudt => semantic_tags::SUDT,
            Self::Xudt => semantic_tags::XUDT,
            Self::Dotbit => semantic_tags::DOTBIT,
            Self::BitCell => semantic_tags::BIT_CELL,
            Self::Mnft => semantic_tags::MNFT,
            Self::Spore => semantic_tags::SPORE,
            Self::Cluster => semantic_tags::CLUSTER,
        }
    }
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
    pub(crate) data_size: i32,
    pub(crate) data_hash: Option<[u8; 32]>,
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

// ---------------------------------------------------------------------------
// Shared protocol facts parsing (single calculation path for both
// pipeline.rs and binary_facts.rs)
// ---------------------------------------------------------------------------

pub(crate) fn parse_fixed_protocol_id<const N: usize>(
    bytes: &[u8],
    label: &str,
    tx_hash: &[u8; 32],
    output_index: i16,
) -> Result<[u8; N]> {
    bytes.try_into().map_err(|_| {
        anyhow!(
            "invalid {} length in protocol facts: tx=0x{}, output_index={}, expected={}, actual={}",
            label,
            hex::encode(tx_hash),
            output_index,
            N,
            bytes.len()
        )
    })
}

pub(crate) fn parse_optional_fixed_protocol_id<const N: usize>(
    bytes: Option<&Vec<u8>>,
    label: &str,
    tx_hash: &[u8; 32],
    output_index: i16,
) -> Result<Option<[u8; N]>> {
    bytes
        .map(|value| parse_fixed_protocol_id::<N>(value, label, tx_hash, output_index))
        .transpose()
}

pub(crate) fn parse_protocol_facts(
    cell: &ParsedCell,
    semantic_tag: CellSemanticTag,
    witness_bundle: &DotbitWitnessBundle,
    tx_hash: &[u8; 32],
    output_index: i16,
) -> Result<Option<CellProtocolFacts>> {
    match semantic_tag {
        CellSemanticTag::Plain
        | CellSemanticTag::Dao
        | CellSemanticTag::Sudt
        | CellSemanticTag::Xudt => Ok(None),
        CellSemanticTag::BitCell => {
            let cell = BitCellParser::parse_parsed_cell(cell).ok_or_else(|| {
                anyhow!(
                    "failed to parse .bit Cell semantics in protocol facts: tx=0x{}, output_index={}, data_len={}",
                    hex::encode(tx_hash),
                    output_index,
                    cell.data.len()
                )
            })?;
            Ok(Some(CellProtocolFacts::BitCell(BitCellProtocolFacts {
                identity_id: parse_fixed_protocol_id::<32>(
                    &cell.identity_id,
                    ".bit Cell identity_id",
                    tx_hash,
                    output_index,
                )?,
                account_id: parse_fixed_protocol_id::<20>(
                    &cell.account_id,
                    ".bit Cell account_id",
                    tx_hash,
                    output_index,
                )?,
                account: cell.account,
                expired_at: cell.expired_at,
            })))
        }
        CellSemanticTag::Spore => {
            let spore = SporeParser::parse_spore_parsed_cell(cell).ok_or_else(|| {
                anyhow!(
                    "failed to parse Spore cell semantics in protocol facts: tx=0x{}, output_index={}",
                    hex::encode(tx_hash),
                    output_index
                )
            })?;
            Ok(Some(CellProtocolFacts::Spore(SporeProtocolFacts {
                spore_id: parse_fixed_protocol_id::<32>(
                    &spore.spore_id,
                    "spore_id",
                    tx_hash,
                    output_index,
                )?,
                is_did: spore.is_did,
                content_type: spore.content_type,
                content: spore.content,
                cluster_id: parse_optional_fixed_protocol_id::<32>(
                    spore.cluster_id.as_ref(),
                    "spore cluster_id",
                    tx_hash,
                    output_index,
                )?,
            })))
        }
        CellSemanticTag::Cluster => {
            let cluster = SporeParser::parse_cluster_parsed_cell(cell).ok_or_else(|| {
                anyhow!(
                    "failed to parse Cluster cell semantics in protocol facts: tx=0x{}, output_index={}",
                    hex::encode(tx_hash),
                    output_index
                )
            })?;
            Ok(Some(CellProtocolFacts::Cluster(ClusterProtocolFacts {
                cluster_id: parse_fixed_protocol_id::<32>(
                    &cluster.cluster_id,
                    "cluster_id",
                    tx_hash,
                    output_index,
                )?,
                name: cluster.name,
                description: cluster.description,
            })))
        }
        CellSemanticTag::Mnft => {
            if let Some(issuer) = MnftParser::parse_issuer_parsed_cell(cell) {
                return Ok(Some(CellProtocolFacts::MnftIssuer(
                    MnftIssuerProtocolFacts {
                        issuer_id: parse_fixed_protocol_id::<20>(
                            &issuer.issuer_id,
                            "mnft issuer_id",
                            tx_hash,
                            output_index,
                        )?,
                        name: issuer.name,
                        info: issuer.info,
                        class_count: issuer.class_count,
                        set_count: issuer.set_count,
                    },
                )));
            }

            if let Some(class) = MnftParser::parse_class_parsed_cell(cell) {
                return Ok(Some(CellProtocolFacts::MnftClass(MnftClassProtocolFacts {
                    class_id: class.class_id,
                    issuer_id: parse_fixed_protocol_id::<20>(
                        &class.issuer_id,
                        "mnft class issuer_id",
                        tx_hash,
                        output_index,
                    )?,
                    name: class.name,
                    description: class.description,
                    renderer: class.renderer,
                    total: class.total,
                    issued: class.issued,
                    configure: class.configure,
                })));
            }

            if let Some(token) = MnftParser::parse_token_parsed_cell(cell) {
                return Ok(Some(CellProtocolFacts::MnftToken(MnftTokenProtocolFacts {
                    token_id: token.token_id,
                    class_id: token.class_id,
                    token_index: token.token_index,
                    characteristic: token.characteristic,
                    configure: token.configure,
                    state: token.state,
                })));
            }

            Err(anyhow!(
                "failed to parse mNFT cell semantics in protocol facts: tx=0x{}, output_index={}",
                hex::encode(tx_hash),
                output_index
            ))
        }
        CellSemanticTag::Dotbit => {
            let Some(mut account) = DotbitParser::parse_account_parsed_cell(cell) else {
                warn!(
                    tx = hex::encode(tx_hash),
                    output_index,
                    data_len = cell.data.len(),
                    "skipping unparseable DotBit AccountCell in protocol facts"
                );
                return Ok(None);
            };

            if let Some(data) = witness_bundle.accounts.get(account.account_id.as_slice()) {
                account.account = data.name.clone();
                account.registered_at = data.registered_at;
                account.status = data.status;
            }

            if account.account.is_none() {
                warn!(
                    tx = hex::encode(tx_hash),
                    output_index,
                    account_id = hex::encode(&account.account_id),
                    "skipping DotBit cell: account name missing in DAS witness"
                );
                return Ok(None);
            }

            Ok(Some(CellProtocolFacts::Dotbit(DotbitProtocolFacts {
                account_id: parse_fixed_protocol_id::<20>(
                    &account.account_id,
                    "dotbit account_id",
                    tx_hash,
                    output_index,
                )?,
                account: account.account,
                next_account_id: parse_optional_fixed_protocol_id::<20>(
                    account.next_account_id.as_ref(),
                    "dotbit next_account_id",
                    tx_hash,
                    output_index,
                )?,
                expired_at: account.expired_at,
                registered_at: account.registered_at,
                status: account.status,
            })))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn malformed_bit_cell_fails_protocol_fact_parsing_with_context() {
        let tx_hash = [0x7a; 32];
        let cell = ParsedCell {
            capacity: 100_00000000,
            lock_code_hash: vec![0; 32],
            lock_hash_type: 1,
            lock_args: Vec::new(),
            lock_script_hash: vec![0x11; 32],
            type_code_hash: Some(crate::rpc::parse_hex_to_bytes(
                crate::parser::bit_cell::BIT_CELL_CODE_HASH_TESTNET,
            )),
            type_hash_type: Some(1),
            type_args: Some(Vec::new()),
            type_script_hash: Some(vec![0x22; 32]),
            data_hash: [0; 32],
            data_size: 3,
            data: vec![1, 2, 3],
        };

        let err = parse_protocol_facts(
            &cell,
            CellSemanticTag::BitCell,
            &DotbitWitnessBundle::default(),
            &tx_hash,
            4,
        )
        .expect_err("recognized malformed .bit Cell must fail fast");

        let message = err.to_string();
        assert!(message.contains("failed to parse .bit Cell semantics"));
        assert!(message.contains(&format!("tx=0x{}", hex::encode(tx_hash))));
        assert!(message.contains("output_index=4"));
    }

    #[test]
    fn facts_arena_defaults_to_empty_indexes() {
        let arena = FactsArena::default();
        assert!(arena.blocks.is_empty());
        assert!(arena.txs.is_empty());
        assert!(arena.cells.is_empty());
    }
}
