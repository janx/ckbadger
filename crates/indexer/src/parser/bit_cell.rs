use anyhow::{anyhow, Result};
use ckb_hash::new_blake2b;

use crate::rpc::{parse_hex_to_bytes, CellOutput, TransactionView};

use super::cell::ParsedCell;
use super::dotbit::{parse_molecule_bytes, parse_molecule_table_fields, parse_molecule_u32};
use super::script::ScriptParser;

pub const BIT_CELL_CODE_HASH_MAINNET: &str =
    "0xcfba73b58b6f30e70caed8a999748781b164ef9a1e218424a6fb55ebf641cb33";
pub const BIT_CELL_CODE_HASH_TESTNET: &str =
    "0x0b1f412fbae26853ff7d082d422c2bdd9e2ff94ee8aaec11240a5b34cc6e890f";

const HASH_LEN: usize = 32;
const ACCOUNT_ID_LEN: usize = 20;

#[derive(Debug, Clone)]
pub struct ParsedBitCell {
    pub identity_id: Vec<u8>,
    pub account_id: Vec<u8>,
    pub account: String,
    pub expired_at: u64,
    pub type_script_hash: Vec<u8>,
    pub owner_lock_hash: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct ParsedBitCellOutput {
    pub output_index: i16,
    pub cell: ParsedBitCell,
}

#[derive(Debug)]
struct ParsedBitCellData {
    account: String,
    expired_at: u64,
    account_hash: [u8; HASH_LEN],
}

pub struct BitCellParser;

impl BitCellParser {
    pub fn is_type_script(code_hash: &[u8]) -> bool {
        crate::parser::registry::PROTOCOL_REGISTRY
            .is(code_hash, crate::parser::registry::ProtocolScript::BitCell)
    }

    pub fn parse_cell(output: &CellOutput, data_hex: &str) -> Option<ParsedBitCell> {
        let type_script = output.type_.as_ref()?;
        let type_code_hash = parse_hex_to_bytes(&type_script.code_hash);
        if !Self::is_type_script(&type_code_hash) {
            return None;
        }

        let type_args = parse_hex_to_bytes(&type_script.args);
        let data = parse_hex_to_bytes(data_hex);
        let parsed = Self::parse_data(&data)?;
        let identity_id = Self::identity_id(&type_args, &parsed.account_hash)?;

        Some(ParsedBitCell {
            identity_id,
            account_id: parsed.account_hash[..ACCOUNT_ID_LEN].to_vec(),
            account: parsed.account,
            expired_at: parsed.expired_at,
            type_script_hash: ScriptParser::compute_script_hash(type_script),
            owner_lock_hash: ScriptParser::compute_script_hash(&output.lock),
        })
    }

    pub fn parse_parsed_cell(cell: &ParsedCell) -> Option<ParsedBitCell> {
        let type_code_hash = cell.type_code_hash.as_deref()?;
        if !Self::is_type_script(type_code_hash) {
            return None;
        }

        let parsed = Self::parse_data(&cell.data)?;
        let identity_id = Self::identity_id(
            cell.type_args.as_deref().unwrap_or_default(),
            &parsed.account_hash,
        )?;

        Some(ParsedBitCell {
            identity_id,
            account_id: parsed.account_hash[..ACCOUNT_ID_LEN].to_vec(),
            account: parsed.account,
            expired_at: parsed.expired_at,
            type_script_hash: cell.type_script_hash.clone()?,
            owner_lock_hash: cell.lock_script_hash.clone(),
        })
    }

    pub fn parse_cells(tx: &TransactionView) -> Result<Vec<ParsedBitCellOutput>> {
        if tx.outputs.len() != tx.outputs_data.len() {
            return Err(anyhow!(
                "transaction outputs mismatch while parsing .bit Cells: tx_hash={}, outputs={}, outputs_data={}",
                tx.hash,
                tx.outputs.len(),
                tx.outputs_data.len()
            ));
        }

        let mut cells = Vec::new();
        for (output_index, (output, data_hex)) in
            tx.outputs.iter().zip(tx.outputs_data.iter()).enumerate()
        {
            let Some(type_script) = output.type_.as_ref() else {
                continue;
            };
            let type_code_hash = parse_hex_to_bytes(&type_script.code_hash);
            if !Self::is_type_script(&type_code_hash) {
                continue;
            }

            let cell = Self::parse_cell(output, data_hex).ok_or_else(|| {
                anyhow!(
                    "failed to parse .bit Cell data: tx_hash={}, output_index={}, code_hash=0x{}, type_args_len={}, data_len={}",
                    tx.hash,
                    output_index,
                    hex::encode(&type_code_hash),
                    parse_hex_to_bytes(&type_script.args).len(),
                    parse_hex_to_bytes(data_hex).len()
                )
            })?;
            let output_index = i16::try_from(output_index).map_err(|_| {
                anyhow!(
                    ".bit Cell output index exceeds i16 range: tx_hash={}, output_index={}",
                    tx.hash,
                    output_index
                )
            })?;
            cells.push(ParsedBitCellOutput { output_index, cell });
        }

        Ok(cells)
    }

    pub(crate) fn parse_identity_id_from_data(
        type_code_hash: &[u8],
        type_args: Option<&[u8]>,
        data: &[u8],
    ) -> Option<Vec<u8>> {
        if !Self::is_type_script(type_code_hash) {
            return None;
        }
        let parsed = Self::parse_data(data)?;
        Self::identity_id(type_args.unwrap_or_default(), &parsed.account_hash)
    }

    fn identity_id(type_args: &[u8], account_hash: &[u8; HASH_LEN]) -> Option<Vec<u8>> {
        if type_args.is_empty() {
            return Some(account_hash.to_vec());
        }
        if type_args.len() == HASH_LEN && !type_args.iter().all(|byte| *byte == 0) {
            return Some(type_args.to_vec());
        }
        None
    }

    fn parse_data(data: &[u8]) -> Option<ParsedBitCellData> {
        let leading_word = parse_molecule_u32(data.get(..4)?)?;
        let (account_bytes, expired_at) = if leading_word == 0 {
            Self::parse_legacy_data(data)?
        } else if leading_word == data.len() {
            Self::parse_current_data(data)?
        } else {
            return None;
        };

        let account = std::str::from_utf8(account_bytes).ok()?;
        if account.is_empty() || !account.ends_with(".bit") {
            return None;
        }

        let mut hasher = new_blake2b();
        hasher.update(account_bytes);
        let mut account_hash = [0u8; HASH_LEN];
        hasher.finalize(&mut account_hash);

        Some(ParsedBitCellData {
            account: account.to_string(),
            expired_at,
            account_hash,
        })
    }

    /// Early testnet layout: `DidCellData` union tag followed by the
    /// `DidCellDataV0` molecule table.
    fn parse_legacy_data(data: &[u8]) -> Option<(&[u8], u64)> {
        let fields = parse_molecule_table_fields(data.get(4..)?, 3)?;
        // Official DidCellDataV0 schema: witness_hash: Byte20,
        // expire_at: Uint64, account: Bytes. The witness hash is not an
        // account ID and must not be used as the persistent identity key.
        if fields[0].len() != ACCOUNT_ID_LEN || fields[1].len() != 8 {
            return None;
        }
        let expired_at = u64::from_le_bytes(fields[1].try_into().ok()?);
        let account = parse_molecule_bytes(fields[2])?;
        Some((account, expired_at))
    }

    /// Current layout: SporeData table whose content is
    /// `[prefix=0, version=1, witness_hash(20), expired_at(8), account]`.
    fn parse_current_data(data: &[u8]) -> Option<(&[u8], u64)> {
        const PREFIX_LEN: usize = 1;
        const VERSION_LEN: usize = 1;
        const WITNESS_HASH_LEN: usize = 20;
        const EXPIRED_AT_LEN: usize = 8;
        const ACCOUNT_OFFSET: usize = PREFIX_LEN + VERSION_LEN + WITNESS_HASH_LEN + EXPIRED_AT_LEN;

        let fields = parse_molecule_table_fields(data, 3)?;
        parse_molecule_bytes(fields[0])?;
        if !fields[2].is_empty() {
            parse_molecule_bytes(fields[2])?;
        }
        let content = parse_molecule_bytes(fields[1])?;
        if content.len() <= ACCOUNT_OFFSET || content[0] != 0 || content[1] != 1 {
            return None;
        }
        let expired_at = u64::from_le_bytes(
            content[PREFIX_LEN + VERSION_LEN + WITNESS_HASH_LEN..ACCOUNT_OFFSET]
                .try_into()
                .ok()?,
        );
        Some((&content[ACCOUNT_OFFSET..], expired_at))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::cell::CellParser;
    use crate::parser::test_helpers::create_lock_script;
    use crate::rpc::{CellOutput, Script};

    fn output(code_hash: &str, args: &str) -> CellOutput {
        CellOutput {
            capacity: "0x4a817c800".to_string(),
            lock: create_lock_script(),
            type_: Some(Script {
                code_hash: code_hash.to_string(),
                hash_type: "type".to_string(),
                args: args.to_string(),
            }),
        }
    }

    #[test]
    fn parses_legacy_testnet_cell_from_failed_bulk_sync_transaction() {
        let output = output(BIT_CELL_CODE_HASH_TESTNET, "0x");
        let data = "0x000000003c00000010000000240000002c000000a7d4860aaf1dc83daedf75d6022811d2c2ae250b1b46fc69000000000c00000032303234303530372e626974";

        let parsed = BitCellParser::parse_cell(&output, data)
            .expect("valid legacy testnet .bit Cell must parse with empty type args");
        assert_eq!(parsed.account, "20240507.bit");
        assert_eq!(parsed.expired_at, 1_778_140_699);
        assert_eq!(
            hex::encode(&parsed.account_id),
            "81d34cd1dfc27716073d1018a63712926d8e3ab3"
        );
        assert_eq!(
            hex::encode(&parsed.identity_id),
            "81d34cd1dfc27716073d1018a63712926d8e3ab36345847129d0cc4135d1ffd4"
        );

        let parsed_cell = CellParser::parse_output(&output, data).expect("bulk cell fixture");
        let bulk = BitCellParser::parse_parsed_cell(&parsed_cell)
            .expect("bulk path must use the same parser");
        assert_eq!(bulk.identity_id, parsed.identity_id);
        assert_eq!(bulk.account_id, parsed.account_id);
        assert_eq!(bulk.account, parsed.account);

        let tx = TransactionView {
            hash: "0xccef03c785caba4144d106b98e87f8bab2dedbb850dd8002356ab6eba5d572be".to_string(),
            version: "0x0".to_string(),
            cell_deps: Vec::new(),
            header_deps: Vec::new(),
            inputs: Vec::new(),
            outputs: vec![output],
            outputs_data: vec![data.to_string()],
            witnesses: Vec::new(),
        };
        let live = BitCellParser::parse_cells(&tx).expect("live parser");
        assert_eq!(live.len(), 1);
        assert_eq!(live[0].cell.identity_id, parsed.identity_id);
    }

    #[test]
    fn parses_current_mainnet_spore_data_and_uses_type_id_args() {
        let args = "0x006d5680998ef5d153b1f07f1c13f5f0d910ceaa28f8954906476551f61ff026";
        let output = output(BIT_CELL_CODE_HASH_MAINNET, args);
        let data = "0x6400000010000000140000004000000000000000280000000001ffd4c1648c7831e9751abc387717ac92ccd8bb4bb268eb69000000003838383839392e62697420000000cff856f49d7a01d48c6a167b5f1bf974d31c375548eea3cf63145a233929f938";

        let parsed = BitCellParser::parse_cell(&output, data)
            .expect("current mainnet .bit Cell SporeData must parse");
        assert_eq!(parsed.account, "888899.bit");
        assert_eq!(parsed.expired_at, 1_777_035_442);
        assert_eq!(parsed.identity_id, parse_hex_to_bytes(args));
        assert_eq!(
            hex::encode(parsed.account_id),
            "c988c3c10cc284410f76a7fd0c8eeba3531aca54"
        );
    }

    #[test]
    fn malformed_recognized_cell_fails_live_parser_with_context() {
        let tx = TransactionView {
            hash: format!("0x{}", "dd".repeat(32)),
            version: "0x0".to_string(),
            cell_deps: Vec::new(),
            header_deps: Vec::new(),
            inputs: Vec::new(),
            outputs: vec![output(BIT_CELL_CODE_HASH_TESTNET, "0x")],
            outputs_data: vec!["0x010203".to_string()],
            witnesses: Vec::new(),
        };

        let err = BitCellParser::parse_cells(&tx).expect_err("malformed .bit Cell must fail fast");
        let message = err.to_string();
        assert!(message.contains("failed to parse .bit Cell data"));
        assert!(message.contains(&tx.hash));
        assert!(message.contains("output_index=0"));
    }
}
