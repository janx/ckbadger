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

/// Convert a CKB core TransactionView to RPC-compatible format.
pub fn convert_transaction_view(tx: &core::TransactionView) -> RpcTransactionView {
    convert_transaction(tx)
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
            .map(|dep| RpcCellDep {
                out_point: convert_out_point(&dep.out_point()),
                dep_type: convert_dep_type(dep.dep_type().as_slice()[0]).to_string(),
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

/// The RPC label of a cell dep's `dep_type` byte.
///
/// CKB's own store holds only the two consensus values; anything else means
/// the store is corrupt or from an incompatible version, and defaulting it to
/// `code` would hand the indexer a transaction that is not the one on chain.
fn convert_dep_type(byte: u8) -> &'static str {
    match byte {
        0 => "code",
        1 => "dep_group",
        other => panic!(
            "CKB store cell dep dep_type byte {other:#04x} is not a consensus value (code=0, dep_group=1)"
        ),
    }
}

fn convert_script(script: &packed::Script) -> RpcScript {
    let hash_type_byte = script.hash_type().as_slice()[0];
    // The label comes from the workspace's one hash_type table. A byte that
    // table does not know is not a consensus value: mapping it to any label
    // would give the indexer a script that hashes to something other than the
    // one in the CKB store, so it stops here with the script named.
    let hash_type = ckbadger_common::hash_type_label(hash_type_byte).unwrap_or_else(|| {
        panic!(
            "CKB store script hash_type byte {hash_type_byte:#04x} is not a consensus value \
             (data=0, type=1, data1=2, data2=4): code_hash=0x{}, args=0x{}",
            hex::encode(script.code_hash().as_slice()),
            hex::encode(script.args().raw_data())
        )
    });
    RpcScript {
        code_hash: format!("0x{}", hex::encode(script.code_hash().as_slice())),
        hash_type: hash_type.to_string(),
        args: format!("0x{}", hex::encode(script.args().raw_data())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn script_with_hash_type(byte: u8) -> packed::Script {
        packed::Script::new_builder()
            .hash_type(packed::Byte::new(byte))
            .build()
    }

    /// The conversion names every consensus hash_type through the workspace's
    /// one table — `data2` is 4 — and refuses a byte that table does not know
    /// instead of defaulting it to `data`.
    #[test]
    fn convert_script_names_every_consensus_hash_type() {
        for (byte, label) in [(0u8, "data"), (1, "type"), (2, "data1"), (4, "data2")] {
            assert_eq!(
                convert_script(&script_with_hash_type(byte)).hash_type,
                label
            );
        }
    }

    #[test]
    #[should_panic(expected = "hash_type byte 0x03 is not a consensus value")]
    fn convert_script_refuses_an_unknown_hash_type_byte() {
        convert_script(&script_with_hash_type(3));
    }

    #[test]
    fn convert_dep_type_names_both_consensus_kinds() {
        assert_eq!(convert_dep_type(0), "code");
        assert_eq!(convert_dep_type(1), "dep_group");
    }

    #[test]
    #[should_panic(expected = "dep_type byte 0x02 is not a consensus value")]
    fn convert_dep_type_refuses_an_unknown_byte() {
        convert_dep_type(2);
    }

    #[test]
    fn test_hex_formatting() {
        assert_eq!(format!("0x{:x}", 0u64), "0x0");
        assert_eq!(format!("0x{:x}", 255u64), "0xff");
        assert_eq!(format!("0x{:x}", 12345u64), "0x3039");
    }
}
