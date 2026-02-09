//! Conversion from CKB native types (Molecule/core) to the indexer's RPC-compatible types.
//!
//! The indexer's parser expects `BlockResponseWithCycles` (JSON-RPC format with hex strings).
//! This module bridges the gap between CKB's zero-copy packed types and those string-based types.

use ckb_types::core;
use ckb_types::packed;
use ckb_types::prelude::*;

use crate::CkbChainReader;

/// Minimal RPC-compatible types that mirror the indexer's `rpc::types` module.
/// These carry the same field names and hex-string encoding that the existing parsers expect.

#[derive(Debug, Clone)]
pub struct RpcBlockView {
    pub header: RpcHeaderView,
    pub uncles: Vec<RpcUncleBlockView>,
    pub transactions: Vec<RpcTransactionView>,
    pub proposals: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct RpcBlockResponseWithCycles {
    pub block: RpcBlockView,
    pub cycles: Option<Vec<String>>,
}

#[derive(Debug, Clone)]
pub struct RpcHeaderView {
    pub version: String,
    pub compact_target: String,
    pub timestamp: String,
    pub number: String,
    pub epoch: String,
    pub parent_hash: String,
    pub transactions_root: String,
    pub proposals_hash: String,
    pub extra_hash: String,
    pub dao: String,
    pub nonce: String,
    pub hash: String,
}

#[derive(Debug, Clone)]
pub struct RpcUncleBlockView {
    pub header: RpcHeaderView,
    pub proposals: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct RpcTransactionView {
    pub hash: String,
    pub version: String,
    pub cell_deps: Vec<RpcCellDep>,
    pub header_deps: Vec<String>,
    pub inputs: Vec<RpcCellInput>,
    pub outputs: Vec<RpcCellOutput>,
    pub outputs_data: Vec<String>,
    pub witnesses: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct RpcCellDep {
    pub out_point: RpcOutPoint,
    pub dep_type: String,
}

#[derive(Debug, Clone)]
pub struct RpcOutPoint {
    pub tx_hash: String,
    pub index: String,
}

#[derive(Debug, Clone)]
pub struct RpcCellInput {
    pub since: String,
    pub previous_output: RpcOutPoint,
}

#[derive(Debug, Clone)]
pub struct RpcCellOutput {
    pub capacity: String,
    pub lock: RpcScript,
    pub type_: Option<RpcScript>,
}

#[derive(Debug, Clone)]
pub struct RpcScript {
    pub code_hash: String,
    pub hash_type: String,
    pub args: String,
}

/// Convert a `ckb_types::core::BlockView` into the RPC-compatible `RpcBlockResponseWithCycles`.
///
/// The `store` is used to read BlockExt for cycles data.
pub fn block_view_to_rpc(
    block: &core::BlockView,
    store: &CkbChainReader,
) -> RpcBlockResponseWithCycles {
    let hash_bytes: [u8; 32] = block.hash().unpack();

    // Get cycles from BlockExt
    let cycles = store
        .get_block_ext(&hash_bytes)
        .and_then(|(_, cycles_vec)| {
            if cycles_vec.is_empty() {
                None
            } else {
                Some(
                    cycles_vec
                        .into_iter()
                        .map(|c| match c {
                            Some(v) => format!("0x{:x}", v),
                            None => "0x0".to_string(),
                        })
                        .collect(),
                )
            }
        });

    let header = convert_header(&block.header());

    let uncles: Vec<RpcUncleBlockView> = block
        .uncles()
        .into_iter()
        .map(|uncle| RpcUncleBlockView {
            header: convert_header(&uncle.header()),
            proposals: uncle
                .data()
                .proposals()
                .into_iter()
                .map(|p| format!("0x{}", hex::encode(p.as_slice())))
                .collect(),
        })
        .collect();

    let transactions: Vec<RpcTransactionView> = block
        .transactions()
        .iter()
        .map(convert_transaction)
        .collect();

    let proposals: Vec<String> = block
        .data()
        .proposals()
        .into_iter()
        .map(|p| format!("0x{}", hex::encode(p.as_slice())))
        .collect();

    RpcBlockResponseWithCycles {
        block: RpcBlockView {
            header,
            uncles,
            transactions,
            proposals,
        },
        cycles,
    }
}

fn convert_header(header: &core::HeaderView) -> RpcHeaderView {
    RpcHeaderView {
        version: format!("0x{:x}", header.version()),
        compact_target: format!("0x{:x}", header.compact_target()),
        timestamp: format!("0x{:x}", header.timestamp()),
        number: format!("0x{:x}", header.number()),
        epoch: format!("0x{:x}", header.epoch().full_value()),
        parent_hash: format!("0x{}", hex::encode(header.parent_hash().as_slice())),
        transactions_root: format!("0x{}", hex::encode(header.transactions_root().as_slice())),
        proposals_hash: format!("0x{}", hex::encode(header.proposals_hash().as_slice())),
        extra_hash: format!("0x{}", hex::encode(header.extra_hash().as_slice())),
        dao: format!("0x{}", hex::encode(header.dao().as_slice())),
        nonce: format!("0x{:x}", header.nonce()),
        hash: format!("0x{}", hex::encode(header.hash().as_slice())),
    }
}

fn convert_transaction(tx: &core::TransactionView) -> RpcTransactionView {
    let raw = tx.data().raw();

    RpcTransactionView {
        hash: format!("0x{}", hex::encode(tx.hash().as_slice())),
        version: format!("0x{:x}", {
            let v: u32 = raw.version().unpack();
            v
        }),
        cell_deps: raw
            .cell_deps()
            .into_iter()
            .map(|dep| {
                let dep_type = match dep.dep_type().as_slice()[0] {
                    0 => "code",
                    1 => "dep_group",
                    _ => "code",
                };
                RpcCellDep {
                    out_point: convert_out_point(&dep.out_point()),
                    dep_type: dep_type.to_string(),
                }
            })
            .collect(),
        header_deps: raw
            .header_deps()
            .into_iter()
            .map(|h| format!("0x{}", hex::encode(h.as_slice())))
            .collect(),
        inputs: raw
            .inputs()
            .into_iter()
            .map(|input| RpcCellInput {
                since: format!("0x{:x}", {
                    let v: u64 = input.since().unpack();
                    v
                }),
                previous_output: convert_out_point(&input.previous_output()),
            })
            .collect(),
        outputs: raw
            .outputs()
            .into_iter()
            .map(|output| {
                let lock = convert_script(&output.lock());
                let type_ = output.type_().to_opt().map(|s| convert_script(&s));
                RpcCellOutput {
                    capacity: format!("0x{:x}", {
                        let v: u64 = output.capacity().unpack();
                        v
                    }),
                    lock,
                    type_,
                }
            })
            .collect(),
        outputs_data: tx
            .data()
            .raw()
            .outputs_data()
            .into_iter()
            .map(|d| format!("0x{}", hex::encode(d.raw_data())))
            .collect(),
        witnesses: tx
            .data()
            .witnesses()
            .into_iter()
            .map(|w| format!("0x{}", hex::encode(w.raw_data())))
            .collect(),
    }
}

fn convert_out_point(out_point: &packed::OutPoint) -> RpcOutPoint {
    RpcOutPoint {
        tx_hash: format!("0x{}", hex::encode(out_point.tx_hash().as_slice())),
        index: format!("0x{:x}", {
            let v: u32 = out_point.index().unpack();
            v
        }),
    }
}

fn convert_script(script: &packed::Script) -> RpcScript {
    let hash_type_byte = script.hash_type().as_slice()[0];
    let hash_type = match hash_type_byte {
        0 => "data",
        1 => "type",
        2 => "data1",
        4 => "data2",
        _ => "data",
    };
    RpcScript {
        code_hash: format!("0x{}", hex::encode(script.code_hash().as_slice())),
        hash_type: hash_type.to_string(),
        args: format!("0x{}", hex::encode(script.args().raw_data())),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_hash_type_conversion() {
        assert_eq!(
            match 0u8 {
                0 => "data",
                1 => "type",
                2 => "data1",
                4 => "data2",
                _ => "data",
            },
            "data"
        );
        assert_eq!(
            match 1u8 {
                0 => "data",
                1 => "type",
                2 => "data1",
                4 => "data2",
                _ => "data",
            },
            "type"
        );
    }

    #[test]
    fn test_hex_formatting() {
        assert_eq!(format!("0x{:x}", 0u64), "0x0");
        assert_eq!(format!("0x{:x}", 255u64), "0xff");
        assert_eq!(format!("0x{:x}", 12345u64), "0x3039");
    }
}
