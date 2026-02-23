use std::collections::HashMap;

use anyhow::Result;
use tracing::warn;

use crate::rpc::{parse_hex_to_bytes, CellOutput, TransactionView};

use super::script::ScriptParser;

pub const DOTBIT_ACCOUNT_CELL_TYPE_ID: &str =
    "0x4f170a048198408f4f4d36bdbcddcebe7a0ae85244d3ab08fd40a80cbfc70918";

pub const DOTBIT_DAS_LOCK_TYPE_ID: &str =
    "0x9376c3b5811942960a846691e16e477cf43d7c7fa654067c9948dfcd09a32137";

const HASH_BYTES_LEN: usize = 32;
const ACCOUNT_ID_LEN: usize = 20;
const DAS_WITNESS_HEADER_LEN: usize = 7; // "das"(3) + action_data_type(4)
const DAS_ACCOUNT_CELL_ACTION_DATA_TYPE: [u8; 4] = [0x01, 0x00, 0x00, 0x00];

#[derive(Debug, Clone)]
pub struct ParsedDotbitAccount {
    pub account_id: Vec<u8>,
    pub account: Option<String>,
    pub type_script_hash: Vec<u8>,
    pub next_account_id: Option<Vec<u8>>,
    pub expired_at: Option<u64>,
    pub owner_lock_hash: Vec<u8>,
}

pub struct DotbitParser;

impl DotbitParser {
    pub fn is_account_cell_type_script(code_hash: &[u8]) -> bool {
        let hash = parse_hex_to_bytes(DOTBIT_ACCOUNT_CELL_TYPE_ID);
        code_hash == hash.as_slice()
    }

    pub fn is_das_lock_script(code_hash: &[u8]) -> bool {
        let hash = parse_hex_to_bytes(DOTBIT_DAS_LOCK_TYPE_ID);
        code_hash == hash.as_slice()
    }

    pub fn parse_account_cell(output: &CellOutput, data_hex: &str) -> Option<ParsedDotbitAccount> {
        let type_script = output.type_.as_ref()?;
        let type_code_hash = parse_hex_to_bytes(&type_script.code_hash);

        if !Self::is_account_cell_type_script(&type_code_hash) {
            return None;
        }

        let data = parse_hex_to_bytes(data_hex);

        let min_len = HASH_BYTES_LEN + ACCOUNT_ID_LEN;
        if data.len() < min_len {
            return None;
        }

        let account_id_from_data = data[HASH_BYTES_LEN..HASH_BYTES_LEN + ACCOUNT_ID_LEN].to_vec();
        let account_id_from_args = parse_hex_to_bytes(&type_script.args);

        // .bit account ID is encoded in type args. Keep compatibility with older data layouts.
        let account_id = if account_id_from_args.len() == ACCOUNT_ID_LEN
            && !account_id_from_args.iter().all(|&b| b == 0)
        {
            account_id_from_args
        } else {
            account_id_from_data
        };

        if account_id.iter().all(|&b| b == 0) {
            return None;
        }

        let next_account_id = if data.len() >= HASH_BYTES_LEN + ACCOUNT_ID_LEN * 2 {
            let next_id =
                data[HASH_BYTES_LEN + ACCOUNT_ID_LEN..HASH_BYTES_LEN + ACCOUNT_ID_LEN * 2].to_vec();
            if next_id.iter().all(|&b| b == 0) {
                None
            } else {
                Some(next_id)
            }
        } else {
            None
        };

        let expired_at = if data.len() >= HASH_BYTES_LEN + ACCOUNT_ID_LEN * 2 + 8 {
            let offset = HASH_BYTES_LEN + ACCOUNT_ID_LEN * 2;
            let bytes: [u8; 8] = data[offset..offset + 8].try_into().ok()?;
            Some(u64::from_le_bytes(bytes))
        } else {
            None
        };

        let type_script_hash = ScriptParser::compute_script_hash(type_script);
        let owner_lock_hash = ScriptParser::compute_script_hash(&output.lock);

        Some(ParsedDotbitAccount {
            account_id,
            account: None,
            type_script_hash,
            next_account_id,
            expired_at,
            owner_lock_hash,
        })
    }

    pub fn parse_accounts(tx: &TransactionView) -> Result<Vec<ParsedDotbitAccount>> {
        let account_name_map = parse_account_names_from_witnesses(&tx.witnesses);
        let mut accounts = Vec::new();
        let mut missing_name_count = 0usize;
        let mut missing_name_samples: Vec<String> = Vec::new();

        for (output, data_hex) in tx.outputs.iter().zip(tx.outputs_data.iter()) {
            let Some(mut account) = Self::parse_account_cell(output, data_hex) else {
                continue;
            };

            account.account = account_name_map.get(&account.account_id).cloned();
            if account.account.is_none() {
                missing_name_count += 1;
                if missing_name_samples.len() < 5 {
                    missing_name_samples.push(format!("0x{}", hex::encode(&account.account_id)));
                }
            }
            accounts.push(account);
        }

        if missing_name_count > 0 {
            let samples = missing_name_samples.join(",");
            warn!(
                tx_hash = %tx.hash,
                missing_account_name_count = missing_name_count,
                missing_account_name_samples = %samples,
                "dotbit account name missing in DAS witness, fallback to account_id"
            );
        }

        Ok(accounts)
    }
}

fn parse_account_names_from_witnesses(witnesses: &[String]) -> HashMap<Vec<u8>, String> {
    let mut result = HashMap::new();

    for witness in witnesses {
        let witness_bytes = parse_hex_to_bytes(witness);
        if witness_bytes.len() <= DAS_WITNESS_HEADER_LEN {
            continue;
        }
        if &witness_bytes[..3] != b"das" {
            continue;
        }
        if witness_bytes[3..7] != DAS_ACCOUNT_CELL_ACTION_DATA_TYPE {
            continue;
        }

        // `Data` molecule table: dep/old/new DataEntityOpt.
        let Some(data_fields) = parse_molecule_table_fields(&witness_bytes[7..], 3) else {
            continue;
        };

        for data_entity_opt in data_fields {
            if data_entity_opt.is_empty() {
                continue;
            }
            // DataEntity table: index(Uint32), version(Uint32), entity(Bytes)
            let Some(data_entity_fields) = parse_molecule_table_fields(data_entity_opt, 3) else {
                continue;
            };
            if data_entity_fields[1].len() != 4 {
                continue;
            }
            let Ok(version_bytes) = <[u8; 4]>::try_from(data_entity_fields[1]) else {
                continue;
            };
            let version = u32::from_le_bytes(version_bytes);
            let Some(entity) = parse_molecule_bytes(data_entity_fields[2]) else {
                continue;
            };
            if let Some((account_id, account_name)) = parse_account_cell_entity(entity, version) {
                result.insert(account_id, account_name);
            }
        }
    }

    result
}

fn parse_account_cell_entity(entity: &[u8], version: u32) -> Option<(Vec<u8>, String)> {
    let min_field_count = match version {
        1 => 6,
        2 => 8,
        3 => 10,
        _ => 11, // v4+
    };
    let fields = parse_molecule_table_fields(entity, min_field_count)?;
    let account_id = fields.first()?.to_vec();
    if account_id.len() != ACCOUNT_ID_LEN {
        return None;
    }

    let account_name = parse_account_chars_to_name(fields.get(1).copied()?)?;
    Some((account_id, account_name))
}

fn parse_account_chars_to_name(account_chars: &[u8]) -> Option<String> {
    let items = parse_molecule_dynvec_items(account_chars)?;
    if items.is_empty() {
        return None;
    }

    let mut account = String::new();
    for item in items {
        // AccountChar table: char_set_name(Uint32), bytes(Bytes)
        let fields = parse_molecule_table_fields(item, 2)?;
        let char_bytes = parse_molecule_bytes(fields.get(1).copied()?)?;
        let ch = std::str::from_utf8(char_bytes).ok()?;
        account.push_str(ch);
    }

    if account.is_empty() {
        return None;
    }
    if !account.ends_with(".bit") {
        account.push_str(".bit");
    }
    Some(account)
}

fn parse_molecule_u32(data: &[u8]) -> Option<usize> {
    let raw: [u8; 4] = data.try_into().ok()?;
    Some(u32::from_le_bytes(raw) as usize)
}

fn parse_molecule_table_fields(data: &[u8], min_field_count: usize) -> Option<Vec<&[u8]>> {
    let header_size = 4 + min_field_count * 4;
    if data.len() < header_size {
        return None;
    }
    let total_size = parse_molecule_u32(&data[0..4])?;
    if total_size != data.len() {
        return None;
    }

    let first_offset = parse_molecule_u32(&data[4..8])?;
    if first_offset < 8 || first_offset > total_size || first_offset % 4 != 0 {
        return None;
    }
    let field_count = first_offset / 4 - 1;
    if field_count < min_field_count {
        return None;
    }
    let exact_header_size = 4 + field_count * 4;
    if exact_header_size != first_offset {
        return None;
    }

    let mut offsets = Vec::with_capacity(field_count + 1);
    for idx in 0..field_count {
        let start = 4 + idx * 4;
        let end = start + 4;
        offsets.push(parse_molecule_u32(&data[start..end])?);
    }
    offsets.push(total_size);

    for pair in offsets.windows(2) {
        if pair[0] > pair[1] || pair[1] > total_size {
            return None;
        }
    }

    Some(
        offsets
            .windows(2)
            .map(|pair| &data[pair[0]..pair[1]])
            .collect(),
    )
}

fn parse_molecule_bytes(data: &[u8]) -> Option<&[u8]> {
    if data.len() < 4 {
        return None;
    }
    // Molecule `Bytes` is encoded as `FixVec<byte>`:
    // 4-byte item count (payload length) + payload bytes.
    let item_count = parse_molecule_u32(&data[0..4])?;
    if item_count + 4 != data.len() {
        return None;
    }
    Some(&data[4..])
}

fn parse_molecule_dynvec_items(data: &[u8]) -> Option<Vec<&[u8]>> {
    if data.len() < 4 {
        return None;
    }
    let total_size = parse_molecule_u32(&data[0..4])?;
    if total_size != data.len() {
        return None;
    }
    if total_size == 4 {
        return Some(Vec::new());
    }
    if data.len() < 8 {
        return None;
    }

    let first_offset = parse_molecule_u32(&data[4..8])?;
    if first_offset < 8 || first_offset > total_size || first_offset % 4 != 0 {
        return None;
    }
    let item_count = first_offset / 4 - 1;
    let header_size = 4 + item_count * 4;
    if header_size != first_offset {
        return None;
    }

    let mut offsets = Vec::with_capacity(item_count + 1);
    for idx in 0..item_count {
        let start = 4 + idx * 4;
        let end = start + 4;
        offsets.push(parse_molecule_u32(&data[start..end])?);
    }
    offsets.push(total_size);

    for pair in offsets.windows(2) {
        if pair[0] > pair[1] || pair[1] > total_size {
            return None;
        }
    }

    Some(
        offsets
            .windows(2)
            .map(|pair| &data[pair[0]..pair[1]])
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rpc::{CellDep, CellInput, CellOutput, OutPoint, Script, TransactionView};

    fn create_lock_script() -> Script {
        Script {
            code_hash: "0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8"
                .to_string(),
            hash_type: "type".to_string(),
            args: "0x927f3e74dceb87c81ba65a19da4f098b4de75a0d".to_string(),
        }
    }

    fn create_account_cell_type_script() -> Script {
        create_account_cell_type_script_with_args(&[])
    }

    fn create_account_cell_type_script_with_args(args: &[u8]) -> Script {
        Script {
            code_hash: DOTBIT_ACCOUNT_CELL_TYPE_ID.to_string(),
            hash_type: "type".to_string(),
            args: format!("0x{}", hex::encode(args)),
        }
    }

    fn create_account_cell_data(
        account_id: &[u8; 20],
        next_account_id: Option<&[u8; 20]>,
        expired_at: Option<u64>,
    ) -> Vec<u8> {
        let mut data = vec![0u8; 32];

        data.extend_from_slice(account_id);

        if let Some(next_id) = next_account_id {
            data.extend_from_slice(next_id);
        } else {
            data.extend_from_slice(&[0u8; 20]);
        }

        if let Some(exp) = expired_at {
            data.extend_from_slice(&exp.to_le_bytes());
        }

        data
    }

    fn encode_molecule_table(fields: &[Vec<u8>]) -> Vec<u8> {
        let header_size = 4 + fields.len() * 4;
        let total_size: usize = header_size + fields.iter().map(|f| f.len()).sum::<usize>();
        let mut out = Vec::with_capacity(total_size);
        out.extend_from_slice(&(total_size as u32).to_le_bytes());
        let mut offset = header_size as u32;
        for field in fields {
            out.extend_from_slice(&offset.to_le_bytes());
            offset += field.len() as u32;
        }
        for field in fields {
            out.extend_from_slice(field);
        }
        out
    }

    fn encode_molecule_bytes(payload: &[u8]) -> Vec<u8> {
        let total_size = 4 + payload.len();
        let mut out = Vec::with_capacity(total_size);
        out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        out.extend_from_slice(payload);
        out
    }

    fn encode_molecule_dynvec(items: &[Vec<u8>]) -> Vec<u8> {
        if items.is_empty() {
            return 4u32.to_le_bytes().to_vec();
        }
        let header_size = 4 + items.len() * 4;
        let total_size: usize = header_size + items.iter().map(|item| item.len()).sum::<usize>();
        let mut out = Vec::with_capacity(total_size);
        out.extend_from_slice(&(total_size as u32).to_le_bytes());

        let mut offset = header_size as u32;
        for item in items {
            out.extend_from_slice(&offset.to_le_bytes());
            offset += item.len() as u32;
        }
        for item in items {
            out.extend_from_slice(item);
        }
        out
    }

    fn encode_dotbit_account_cell_witness(account_id: &[u8; 20], account: &str) -> String {
        let mut account_items = Vec::new();
        let account_without_suffix = account.strip_suffix(".bit").unwrap_or(account);
        for ch in account_without_suffix.chars() {
            let char_table = encode_molecule_table(&[
                2u32.to_le_bytes().to_vec(), // En
                encode_molecule_bytes(ch.to_string().as_bytes()),
            ]);
            account_items.push(char_table);
        }
        let account_chars = encode_molecule_dynvec(&account_items);
        let records_empty = encode_molecule_dynvec(&[]);

        // AccountCellDataV3 entity
        let entity = encode_molecule_table(&[
            account_id.to_vec(), // id
            account_chars,       // account
            0u64.to_le_bytes().to_vec(),
            0u64.to_le_bytes().to_vec(),
            0u64.to_le_bytes().to_vec(),
            0u64.to_le_bytes().to_vec(),
            vec![0], // status
            records_empty,
            vec![0], // enable_sub_account
            0u64.to_le_bytes().to_vec(),
        ]);

        let data_entity = encode_molecule_table(&[
            0u32.to_le_bytes().to_vec(), // index
            3u32.to_le_bytes().to_vec(), // version v3
            encode_molecule_bytes(&entity),
        ]);

        let data = encode_molecule_table(&[
            Vec::new(),  // dep
            Vec::new(),  // old
            data_entity, // new
        ]);

        let mut witness = Vec::new();
        witness.extend_from_slice(b"das");
        witness.extend_from_slice(&DAS_ACCOUNT_CELL_ACTION_DATA_TYPE);
        witness.extend_from_slice(&data);
        format!("0x{}", hex::encode(witness))
    }

    fn encode_dotbit_account_cell_witness_v2(account_id: &[u8; 20], account: &str) -> String {
        let mut account_items = Vec::new();
        let account_without_suffix = account.strip_suffix(".bit").unwrap_or(account);
        for ch in account_without_suffix.chars() {
            let char_table = encode_molecule_table(&[
                2u32.to_le_bytes().to_vec(), // En
                encode_molecule_bytes(ch.to_string().as_bytes()),
            ]);
            account_items.push(char_table);
        }
        let account_chars = encode_molecule_dynvec(&account_items);
        let records_empty = encode_molecule_dynvec(&[]);

        // AccountCellDataV2 entity
        let entity = encode_molecule_table(&[
            account_id.to_vec(), // id
            account_chars,       // account
            0u64.to_le_bytes().to_vec(),
            0u64.to_le_bytes().to_vec(),
            0u64.to_le_bytes().to_vec(),
            0u64.to_le_bytes().to_vec(),
            vec![0], // status
            records_empty,
        ]);

        let data_entity = encode_molecule_table(&[
            0u32.to_le_bytes().to_vec(), // index
            2u32.to_le_bytes().to_vec(), // version v2
            encode_molecule_bytes(&entity),
        ]);

        let data = encode_molecule_table(&[
            Vec::new(),  // dep
            Vec::new(),  // old
            data_entity, // new
        ]);

        let mut witness = Vec::new();
        witness.extend_from_slice(b"das");
        witness.extend_from_slice(&DAS_ACCOUNT_CELL_ACTION_DATA_TYPE);
        witness.extend_from_slice(&data);
        format!("0x{}", hex::encode(witness))
    }

    fn create_dotbit_tx(account_id: &[u8; 20], include_witness: bool) -> TransactionView {
        let output = CellOutput {
            capacity: "0x174876e800".to_string(),
            lock: create_lock_script(),
            type_: Some(create_account_cell_type_script()),
        };
        let data_hex = format!(
            "0x{}",
            hex::encode(create_account_cell_data(account_id, None, None))
        );
        let witnesses = if include_witness {
            vec![encode_dotbit_account_cell_witness(account_id, "alice.bit")]
        } else {
            Vec::new()
        };

        TransactionView {
            hash: "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string(),
            version: "0x0".to_string(),
            cell_deps: Vec::<CellDep>::new(),
            header_deps: Vec::new(),
            inputs: vec![CellInput {
                since: "0x0".to_string(),
                previous_output: OutPoint {
                    tx_hash: "0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
                        .to_string(),
                    index: "0x0".to_string(),
                },
            }],
            outputs: vec![output],
            outputs_data: vec![data_hex],
            witnesses,
        }
    }

    #[test]
    fn test_is_account_cell_type_script() {
        let hash = parse_hex_to_bytes(DOTBIT_ACCOUNT_CELL_TYPE_ID);
        assert!(DotbitParser::is_account_cell_type_script(&hash));

        let other = parse_hex_to_bytes(
            "0x0000000000000000000000000000000000000000000000000000000000000000",
        );
        assert!(!DotbitParser::is_account_cell_type_script(&other));
    }

    #[test]
    fn test_parse_account_cell_basic() {
        let account_id: [u8; 20] = [0xab; 20];
        let next_account_id: [u8; 20] = [0xcd; 20];
        let expired_at = 1735689600u64;

        let output = CellOutput {
            capacity: "0x174876e800".to_string(),
            lock: create_lock_script(),
            type_: Some(create_account_cell_type_script()),
        };

        let data = create_account_cell_data(&account_id, Some(&next_account_id), Some(expired_at));
        let data_hex = format!("0x{}", hex::encode(&data));

        let result = DotbitParser::parse_account_cell(&output, &data_hex);
        assert!(result.is_some());

        let parsed = result.unwrap();
        assert_eq!(parsed.account_id, account_id.to_vec());
        assert!(parsed.account.is_none());
        assert_eq!(parsed.next_account_id, Some(next_account_id.to_vec()));
        assert_eq!(parsed.expired_at, Some(expired_at));
    }

    #[test]
    fn test_parse_account_cell_no_next() {
        let account_id: [u8; 20] = [0xab; 20];
        let expired_at = 1735689600u64;

        let output = CellOutput {
            capacity: "0x174876e800".to_string(),
            lock: create_lock_script(),
            type_: Some(create_account_cell_type_script()),
        };

        let data = create_account_cell_data(&account_id, None, Some(expired_at));
        let data_hex = format!("0x{}", hex::encode(&data));

        let result = DotbitParser::parse_account_cell(&output, &data_hex);
        assert!(result.is_some());

        let parsed = result.unwrap();
        assert_eq!(parsed.account_id, account_id.to_vec());
        assert!(parsed.account.is_none());
        assert!(parsed.next_account_id.is_none());
    }

    #[test]
    fn test_parse_account_cell_uses_type_args_account_id_when_data_is_placeholder() {
        let account_id_from_args: [u8; 20] = [0x44; 20];
        let placeholder_data_account_id: [u8; 20] = [0x00; 20];

        let output = CellOutput {
            capacity: "0x174876e800".to_string(),
            lock: create_lock_script(),
            type_: Some(create_account_cell_type_script_with_args(
                &account_id_from_args,
            )),
        };

        let data = create_account_cell_data(&placeholder_data_account_id, None, None);
        let data_hex = format!("0x{}", hex::encode(&data));

        let result = DotbitParser::parse_account_cell(&output, &data_hex);
        assert!(result.is_some());

        let parsed = result.unwrap();
        assert_eq!(parsed.account_id, account_id_from_args.to_vec());
    }

    #[test]
    fn test_parse_account_cell_no_type() {
        let output = CellOutput {
            capacity: "0x174876e800".to_string(),
            lock: create_lock_script(),
            type_: None,
        };

        let result = DotbitParser::parse_account_cell(&output, "0x");
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_account_cell_wrong_type() {
        let output = CellOutput {
            capacity: "0x174876e800".to_string(),
            lock: create_lock_script(),
            type_: Some(Script {
                code_hash: "0x0000000000000000000000000000000000000000000000000000000000000000"
                    .to_string(),
                hash_type: "type".to_string(),
                args: "0x".to_string(),
            }),
        };

        let result = DotbitParser::parse_account_cell(&output, "0x");
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_account_cell_data_too_short() {
        let output = CellOutput {
            capacity: "0x174876e800".to_string(),
            lock: create_lock_script(),
            type_: Some(create_account_cell_type_script()),
        };

        let short_data = vec![0u8; 40];
        let data_hex = format!("0x{}", hex::encode(&short_data));

        let result = DotbitParser::parse_account_cell(&output, &data_hex);
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_accounts_extracts_human_readable_name_from_witness() {
        let account_id = [0x11u8; 20];
        let tx = create_dotbit_tx(&account_id, true);

        let parsed_accounts = DotbitParser::parse_accounts(&tx).expect("parse dotbit accounts");
        assert_eq!(parsed_accounts.len(), 1);
        assert_eq!(parsed_accounts[0].account_id, account_id.to_vec());
        assert_eq!(parsed_accounts[0].account.as_deref(), Some("alice.bit"));
    }

    #[test]
    fn test_parse_accounts_allows_missing_witness_name_with_fallback() {
        let account_id = [0x22u8; 20];
        let tx = create_dotbit_tx(&account_id, false);

        let parsed_accounts = DotbitParser::parse_accounts(&tx).expect("parse dotbit accounts");
        assert_eq!(parsed_accounts.len(), 1);
        assert_eq!(parsed_accounts[0].account_id, account_id.to_vec());
        assert!(parsed_accounts[0].account.is_none());
    }

    #[test]
    fn test_parse_accounts_resolves_account_id_from_type_args() {
        let account_id = [0x33u8; 20];
        let output = CellOutput {
            capacity: "0x174876e800".to_string(),
            lock: create_lock_script(),
            type_: Some(create_account_cell_type_script_with_args(&account_id)),
        };
        let placeholder_data_account_id = [0u8; 20];
        let data_hex = format!(
            "0x{}",
            hex::encode(create_account_cell_data(
                &placeholder_data_account_id,
                None,
                None
            ))
        );

        let tx = TransactionView {
            hash: "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string(),
            version: "0x0".to_string(),
            cell_deps: Vec::<CellDep>::new(),
            header_deps: Vec::new(),
            inputs: vec![CellInput {
                since: "0x0".to_string(),
                previous_output: OutPoint {
                    tx_hash: "0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
                        .to_string(),
                    index: "0x0".to_string(),
                },
            }],
            outputs: vec![output],
            outputs_data: vec![data_hex],
            witnesses: vec![encode_dotbit_account_cell_witness(&account_id, "alice.bit")],
        };

        let parsed_accounts = DotbitParser::parse_accounts(&tx).expect("parse dotbit accounts");
        assert_eq!(parsed_accounts.len(), 1);
        assert_eq!(parsed_accounts[0].account_id, account_id.to_vec());
        assert_eq!(parsed_accounts[0].account.as_deref(), Some("alice.bit"));
    }

    #[test]
    fn test_parse_account_names_from_witnesses_supports_v2_bytes_fixvec() {
        let account_id = [0x55u8; 20];
        let witness = encode_dotbit_account_cell_witness_v2(&account_id, "smartest.bit");
        let result = parse_account_names_from_witnesses(&[witness]);

        assert_eq!(
            result.get(account_id.as_slice()),
            Some(&"smartest.bit".to_string())
        );
    }
}
