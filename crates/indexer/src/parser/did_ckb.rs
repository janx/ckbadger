//! did:ckb (Web5 DID) identity-cell parser.
//!
//! did:ckb is an independent identity protocol (docs/metadata/scripts/
//! did-ckb.toml, registry `ProtocolScript::DidCkb`). It is NOT a Spore NFT
//! and must never flow through the Spore/object pipeline or the legacy
//! `ProtocolScript::SporeDid` variant.
//!
//! Identity model (matches the shipped identity store + API routes):
//! - item id = the full type-script `args`, verbatim. On chain the args are
//!   NOT fixed-width (live testnet holds both 32-byte and 20-byte ids), so no
//!   width gate is applied here.
//! - cell data (a molecule-wrapped CBOR DID document) is not parsed; the
//!   identity entry carries no derived name.

use crate::rpc::{parse_hex_to_bytes, CellOutput, TransactionView};

use super::cell::ParsedCell;
use super::registry::{ProtocolScript, PROTOCOL_REGISTRY};
use super::script::ScriptParser;

/// A did:ckb identity cell parsed from a transaction output.
#[derive(Debug, Clone)]
pub struct ParsedDidCkbCell {
    /// Identity item id: the full type-script args, verbatim.
    pub did_id: Vec<u8>,
    pub owner_lock_hash: Vec<u8>,
}

pub struct DidCkbParser;

impl DidCkbParser {
    /// Single classification path for did:ckb type scripts (both networks),
    /// resolved through the bundled metadata registry.
    pub fn is_type_script(code_hash: &[u8]) -> bool {
        PROTOCOL_REGISTRY.is(code_hash, ProtocolScript::DidCkb)
    }

    /// Parse a did:ckb identity cell from an RPC output (live sync path).
    /// Returns `None` for cells that are not did:ckb-typed.
    pub fn parse_did_cell(output: &CellOutput) -> Option<ParsedDidCkbCell> {
        let type_script = output.type_.as_ref()?;
        if !Self::is_type_script(&parse_hex_to_bytes(&type_script.code_hash)) {
            return None;
        }

        Some(ParsedDidCkbCell {
            did_id: parse_hex_to_bytes(&type_script.args),
            owner_lock_hash: ScriptParser::compute_script_hash(&output.lock),
        })
    }

    /// Parse a did:ckb identity cell from a `ParsedCell` (bulk facts path).
    /// Returns `None` for cells that are not did:ckb-typed.
    pub fn parse_did_parsed_cell(cell: &ParsedCell) -> Option<ParsedDidCkbCell> {
        let type_code_hash = cell.type_code_hash.as_ref()?;
        let type_args = cell.type_args.as_ref()?;
        if !Self::is_type_script(type_code_hash) {
            return None;
        }

        Some(ParsedDidCkbCell {
            did_id: type_args.clone(),
            owner_lock_hash: cell.lock_script_hash.clone(),
        })
    }

    /// Parse all did:ckb identity cells in a transaction's outputs, preserving
    /// real output indices (live sync path).
    pub fn parse_did_cells_with_output_indices(
        tx: &TransactionView,
    ) -> Vec<(usize, ParsedDidCkbCell)> {
        tx.outputs
            .iter()
            .enumerate()
            .filter_map(|(output_index, output)| {
                Self::parse_did_cell(output).map(|did| (output_index, did))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::cell::CellParser;
    use crate::parser::spore::SPORE_CODE_HASH_MAINNET_V2;
    use crate::parser::test_helpers::{create_lock_script, real_did_ckb};
    use crate::rpc::Script;

    #[test]
    fn test_real_did_ckb_code_hashes_classify_on_both_networks() {
        let testnet = parse_hex_to_bytes(real_did_ckb::TYPE_CODE_HASH_TESTNET);
        assert!(
            DidCkbParser::is_type_script(&testnet),
            "did:ckb testnet code_hash must classify as did:ckb"
        );
        let mainnet = parse_hex_to_bytes(real_did_ckb::TYPE_CODE_HASH_MAINNET);
        assert!(
            DidCkbParser::is_type_script(&mainnet),
            "did:ckb mainnet code_hash must classify as did:ckb"
        );
    }

    #[test]
    fn test_real_testnet_did_ckb_cell_parses_item_id_from_args() {
        // Live-path parse of the audited testnet cell 0x00290adc…:0
        // (block 18082860): the identity item id is the full type-script args.
        let (output, _data_hex) = real_did_ckb::cell_32();
        let parsed = DidCkbParser::parse_did_cell(&output)
            .expect("real did:ckb cell must be classified by the live parse path");
        assert_eq!(
            parsed.did_id,
            parse_hex_to_bytes(real_did_ckb::CELL_32_ARGS),
            "item id must be the exact type-script args"
        );
        assert_eq!(
            parsed.owner_lock_hash,
            ScriptParser::compute_script_hash(&output.lock)
        );
    }

    #[test]
    fn test_real_testnet_did_ckb_20_byte_args_cell_parses_in_bulk_path() {
        // 31 of 421 live testnet did:ckb cells carry 20-byte args. The bulk
        // (ParsedCell) path must classify them and preserve the exact 20-byte
        // item id.
        let (output, data_hex) = real_did_ckb::cell_20();
        let parsed_cell = CellParser::parse_output(&output, data_hex).expect("parsed cell");
        let parsed = DidCkbParser::parse_did_parsed_cell(&parsed_cell)
            .expect("real 20-byte-args did:ckb cell must be classified by the bulk parse path");
        let expected_id = parse_hex_to_bytes(real_did_ckb::CELL_20_ARGS);
        assert_eq!(expected_id.len(), 20);
        assert_eq!(
            parsed.did_id, expected_id,
            "20-byte item id must be preserved verbatim"
        );
        assert_eq!(parsed.owner_lock_hash, parsed_cell.lock_script_hash);
    }

    #[test]
    fn test_live_and_bulk_paths_agree_on_real_cells() {
        for (output, data_hex) in [real_did_ckb::cell_32(), real_did_ckb::cell_20()] {
            let live = DidCkbParser::parse_did_cell(&output).expect("live parse");
            let parsed_cell = CellParser::parse_output(&output, data_hex).expect("parsed cell");
            let bulk = DidCkbParser::parse_did_parsed_cell(&parsed_cell).expect("bulk parse");
            assert_eq!(live.did_id, bulk.did_id);
            assert_eq!(live.owner_lock_hash, bulk.owner_lock_hash);
        }
    }

    #[test]
    fn test_spore_nft_cell_is_not_did_ckb() {
        let output = CellOutput {
            capacity: "0x174876e800".to_string(),
            lock: create_lock_script(),
            type_: Some(Script {
                code_hash: SPORE_CODE_HASH_MAINNET_V2.to_string(),
                hash_type: "data1".to_string(),
                args: "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef"
                    .to_string(),
            }),
        };
        assert!(DidCkbParser::parse_did_cell(&output).is_none());
        assert!(!DidCkbParser::is_type_script(&parse_hex_to_bytes(
            SPORE_CODE_HASH_MAINNET_V2
        )));
    }

    #[test]
    fn test_bit_cell_is_not_did_ckb() {
        use crate::parser::bit_cell::BIT_CELL_CODE_HASH_MAINNET;
        let output = CellOutput {
            capacity: "0x174876e800".to_string(),
            lock: create_lock_script(),
            type_: Some(Script {
                code_hash: BIT_CELL_CODE_HASH_MAINNET.to_string(),
                hash_type: "type".to_string(),
                args: "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef"
                    .to_string(),
            }),
        };
        assert!(DidCkbParser::parse_did_cell(&output).is_none());
        assert!(!DidCkbParser::is_type_script(&parse_hex_to_bytes(
            BIT_CELL_CODE_HASH_MAINNET
        )));
    }

    #[test]
    fn test_typeless_cell_is_not_did_ckb() {
        let output = CellOutput {
            capacity: "0x174876e800".to_string(),
            lock: create_lock_script(),
            type_: None,
        };
        assert!(DidCkbParser::parse_did_cell(&output).is_none());
    }

    #[test]
    fn test_parse_did_cells_with_output_indices_preserves_real_index() {
        let (did_output, _) = real_did_ckb::cell_32();
        let plain_output = CellOutput {
            capacity: "0x174876e800".to_string(),
            lock: create_lock_script(),
            type_: None,
        };
        let tx = TransactionView {
            hash: "0xaa".to_string(),
            version: "0x0".to_string(),
            cell_deps: vec![],
            header_deps: vec![],
            inputs: vec![],
            outputs: vec![plain_output, did_output],
            outputs_data: vec!["0x".to_string(), real_did_ckb::CELL_32_DATA.to_string()],
            witnesses: vec![],
        };

        let parsed = DidCkbParser::parse_did_cells_with_output_indices(&tx);
        assert_eq!(parsed.len(), 1);
        assert_eq!(
            parsed[0].0, 1,
            "did cell at output index 1 must preserve real index"
        );
        assert_eq!(
            parsed[0].1.did_id,
            parse_hex_to_bytes(real_did_ckb::CELL_32_ARGS)
        );
    }
}
