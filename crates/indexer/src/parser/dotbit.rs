use std::collections::HashMap;
use std::sync::LazyLock;

use anyhow::{anyhow, Result};

use crate::rpc::{parse_hex_to_bytes, CellOutput, TransactionView};

use super::cell::ParsedCell;
use super::script::ScriptParser;

pub const DOTBIT_ACCOUNT_CELL_TYPE_ID: &str =
    "0x4f170a048198408f4f4d36bdbcddcebe7a0ae85244d3ab08fd40a80cbfc70918";

static DOTBIT_TYPE_ID_HASH: LazyLock<Vec<u8>> =
    LazyLock::new(|| parse_hex_to_bytes(DOTBIT_ACCOUNT_CELL_TYPE_ID));

const HASH_BYTES_LEN: usize = 32;
const ACCOUNT_ID_LEN: usize = 20;
const DAS_WITNESS_HEADER_LEN: usize = 7; // "das"(3) + action_data_type(4)
const DAS_WITNESS_HEX_PREFIX: &str = "646173";
const DAS_ACCOUNT_CELL_ACTION_DATA_TYPE: [u8; 4] = [0x01, 0x00, 0x00, 0x00];
const DAS_ACTION_DATA_TYPE: [u8; 4] = [0x00, 0x00, 0x00, 0x00];

#[derive(Debug, Clone)]
pub struct ParsedDotbitAccount {
    pub account_id: Vec<u8>,
    pub account: Option<String>,
    pub type_script_hash: Vec<u8>,
    pub next_account_id: Option<Vec<u8>>,
    pub expired_at: Option<u64>,
    pub registered_at: Option<u64>,
    pub status: Option<u8>,
    pub owner_lock_hash: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct ParsedDotbitAccountOutput {
    pub output_index: i16,
    pub account: ParsedDotbitAccount,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct DotbitWitnessBundle {
    pub(crate) action: Option<String>,
    pub(crate) accounts: HashMap<Vec<u8>, DotbitWitnessAccountData>,
}

#[derive(Debug, Clone)]
pub(crate) struct DotbitWitnessAccountData {
    pub(crate) name: Option<String>,
    pub(crate) registered_at: Option<u64>,
    pub(crate) status: Option<u8>,
}

pub struct DotbitParser;

impl DotbitParser {
    pub fn is_account_cell_type_script(code_hash: &[u8]) -> bool {
        code_hash == DOTBIT_TYPE_ID_HASH.as_slice()
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

        let account_id_from_args = parse_hex_to_bytes(&type_script.args);
        let account_id_from_data = data[HASH_BYTES_LEN..HASH_BYTES_LEN + ACCOUNT_ID_LEN].to_vec();

        // Prefer type args when available (newer layout), but keep data fallback
        // for historical/live cells where args may be empty.
        let account_id = if account_id_from_args.len() == ACCOUNT_ID_LEN
            && !account_id_from_args.iter().all(|&b| b == 0)
        {
            account_id_from_args
        } else if !account_id_from_data.iter().all(|&b| b == 0) {
            account_id_from_data
        } else {
            return None;
        };

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
            registered_at: None,
            status: None,
            owner_lock_hash,
        })
    }

    pub fn parse_account_parsed_cell(cell: &ParsedCell) -> Option<ParsedDotbitAccount> {
        let type_code_hash = cell.type_code_hash.as_ref()?;

        if !Self::is_account_cell_type_script(type_code_hash) {
            return None;
        }

        let data = &cell.data;

        let min_len = HASH_BYTES_LEN + ACCOUNT_ID_LEN;
        if data.len() < min_len {
            return None;
        }

        let account_id_from_args = cell.type_args.as_deref();
        let account_id_from_data = &data[HASH_BYTES_LEN..HASH_BYTES_LEN + ACCOUNT_ID_LEN];

        let account_id = if let Some(args) =
            account_id_from_args.filter(|a| a.len() == ACCOUNT_ID_LEN && !a.iter().all(|&b| b == 0))
        {
            args.to_vec()
        } else if !account_id_from_data.iter().all(|&b| b == 0) {
            account_id_from_data.to_vec()
        } else {
            return None;
        };

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

        Some(ParsedDotbitAccount {
            account_id,
            account: None,
            type_script_hash: cell.type_script_hash.clone()?,
            next_account_id,
            expired_at,
            registered_at: None,
            status: None,
            owner_lock_hash: cell.lock_script_hash.clone(),
        })
    }

    /// Extract the .bit action name from the transaction's ActionData witness.
    ///
    /// Layout: witness bytes = "das"(3) + ActionDataType(4) + ActionData(molecule table)
    /// ActionDataType 0x00000000 = ActionData witness
    /// ActionData molecule table: field[0] = Action (Bytes), field[1] = Params (Bytes)
    pub fn parse_das_action(witnesses: &[String]) -> Option<String> {
        parse_dotbit_witness_bundle(witnesses).action
    }

    pub fn parse_accounts(tx: &TransactionView) -> Result<Vec<ParsedDotbitAccountOutput>> {
        if tx.outputs.len() != tx.outputs_data.len() {
            return Err(anyhow!(
                "transaction outputs mismatch while parsing dotbit accounts: tx_hash={}, outputs={}, outputs_data={}",
                tx.hash,
                tx.outputs.len(),
                tx.outputs_data.len()
            ));
        }
        let witness_bundle = parse_dotbit_witness_bundle(&tx.witnesses);
        let mut accounts = Vec::new();
        let mut missing_name_count = 0usize;
        let mut missing_name_samples: Vec<String> = Vec::new();

        for (output_index, (output, data_hex)) in
            tx.outputs.iter().zip(tx.outputs_data.iter()).enumerate()
        {
            let Some(mut account) = Self::parse_account_cell(output, data_hex) else {
                continue;
            };

            let output_index = i16::try_from(output_index).map_err(|_| {
                anyhow!(
                    "dotbit output index exceeds i16 range: tx_hash={}, output_index={}",
                    tx.hash,
                    output_index
                )
            })?;
            if let Some(wd) = witness_bundle.accounts.get(&account.account_id) {
                account.account = wd.name.clone();
                account.registered_at = wd.registered_at;
                account.status = wd.status;
            }
            if account.account.is_none() {
                missing_name_count += 1;
                if missing_name_samples.len() < 5 {
                    missing_name_samples.push(format!("0x{}", hex::encode(&account.account_id)));
                }
            }
            accounts.push(ParsedDotbitAccountOutput {
                output_index,
                account,
            });
        }

        if missing_name_count > 0 {
            return Err(anyhow!(
                "dotbit account name missing in DAS witness: tx_hash={}, missing_account_name_count={}, missing_account_name_samples={}",
                tx.hash,
                missing_name_count,
                missing_name_samples.join(",")
            ));
        }

        Ok(accounts)
    }
}

pub(crate) fn parse_dotbit_witness_bundle(witnesses: &[String]) -> DotbitWitnessBundle {
    let mut bundle = DotbitWitnessBundle::default();

    for witness in witnesses {
        if !witness_has_das_hex_prefix(witness) {
            continue;
        }
        let witness_bytes = parse_hex_to_bytes(witness);
        if witness_bytes.len() <= DAS_WITNESS_HEADER_LEN {
            continue;
        }
        if &witness_bytes[..3] != b"das" {
            continue;
        }

        let action_data_type = &witness_bytes[3..7];
        if action_data_type == DAS_ACTION_DATA_TYPE {
            if bundle.action.is_none() {
                bundle.action = parse_das_action_from_witness_bytes(&witness_bytes);
            }
        } else if action_data_type == DAS_ACCOUNT_CELL_ACTION_DATA_TYPE {
            parse_account_data_from_witness_bytes(&witness_bytes, &mut bundle.accounts);
        }
    }

    bundle
}

pub(crate) fn may_contain_das_witness(witnesses: &[String]) -> bool {
    witnesses
        .iter()
        .any(|witness| witness_has_das_hex_prefix(witness))
}

fn witness_has_das_hex_prefix(witness: &str) -> bool {
    let Some(hex_body) = witness
        .strip_prefix("0x")
        .or_else(|| witness.strip_prefix("0X"))
    else {
        return false;
    };

    hex_body.len() > DAS_WITNESS_HEADER_LEN * 2
        && hex_body
            .get(..DAS_WITNESS_HEX_PREFIX.len())
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case(DAS_WITNESS_HEX_PREFIX))
}

fn parse_das_action_from_witness_bytes(witness_bytes: &[u8]) -> Option<String> {
    let fields = parse_molecule_table_fields(&witness_bytes[7..], 2)?;
    let action_bytes = parse_molecule_bytes(fields[0])?;
    let action = std::str::from_utf8(action_bytes).ok()?;
    Some(action.to_string())
}

fn parse_account_data_from_witness_bytes(
    witness_bytes: &[u8],
    accounts: &mut HashMap<Vec<u8>, DotbitWitnessAccountData>,
) {
    let Some(data_fields) = parse_molecule_table_fields(&witness_bytes[7..], 3) else {
        return;
    };

    for data_entity_opt in data_fields {
        if data_entity_opt.is_empty() {
            continue;
        }
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
        if let Some((account_id, data)) = parse_account_cell_entity(entity, version) {
            accounts.insert(account_id, data);
        }
    }
}

fn parse_account_cell_entity(
    entity: &[u8],
    version: u32,
) -> Option<(Vec<u8>, DotbitWitnessAccountData)> {
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

    let name = parse_account_chars_to_name(fields.get(1).copied()?);

    // field[2] = registered_at (u64 LE, 8 bytes)
    let registered_at = fields.get(2).and_then(|f| {
        if f.len() != 8 {
            return None;
        }
        let bytes: [u8; 8] = (*f).try_into().ok()?;
        let val = u64::from_le_bytes(bytes);
        if val == 0 {
            None
        } else {
            Some(val)
        }
    });

    // field[6] = status (u8, 1 byte) — only available in v2+
    let status = if version >= 2 {
        fields.get(6).and_then(|f| f.first().copied())
    } else {
        None
    };

    Some((
        account_id,
        DotbitWitnessAccountData {
            name,
            registered_at,
            status,
        },
    ))
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
    use crate::parser::test_helpers::create_lock_script;
    use crate::rpc::{CellDep, CellInput, CellOutput, OutPoint, Script, TransactionView};

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
            type_: Some(create_account_cell_type_script_with_args(account_id)),
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
            type_: Some(create_account_cell_type_script_with_args(&account_id)),
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
            type_: Some(create_account_cell_type_script_with_args(&account_id)),
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
    fn test_parse_account_cell_falls_back_to_data_account_id_when_type_args_missing() {
        let account_id_from_data: [u8; 20] = [0x55; 20];

        let output = CellOutput {
            capacity: "0x174876e800".to_string(),
            lock: create_lock_script(),
            type_: Some(create_account_cell_type_script_with_args(&[])),
        };

        let data = create_account_cell_data(&account_id_from_data, None, None);
        let data_hex = format!("0x{}", hex::encode(&data));

        let result = DotbitParser::parse_account_cell(&output, &data_hex);
        assert!(result.is_some());
        let parsed = result.unwrap();
        assert_eq!(parsed.account_id, account_id_from_data.to_vec());
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
            type_: Some(create_account_cell_type_script_with_args(&[0x11; 20])),
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
        assert_eq!(parsed_accounts[0].output_index, 0);
        assert_eq!(parsed_accounts[0].account.account_id, account_id.to_vec());
        assert_eq!(
            parsed_accounts[0].account.account.as_deref(),
            Some("alice.bit")
        );
    }

    #[test]
    fn test_parse_account_parsed_cell_matches_raw_path() {
        let account_id = [0x19u8; 20];
        let output = CellOutput {
            capacity: "0x174876e800".to_string(),
            lock: create_lock_script(),
            type_: Some(create_account_cell_type_script_with_args(&account_id)),
        };
        let data_hex = format!(
            "0x{}",
            hex::encode(create_account_cell_data(
                &account_id,
                None,
                Some(1735689600)
            ))
        );
        let parsed_cell =
            crate::parser::cell::CellParser::parse_output(&output, &data_hex).expect("parsed cell");

        let raw = DotbitParser::parse_account_cell(&output, &data_hex).expect("raw");
        let preparsed = DotbitParser::parse_account_parsed_cell(&parsed_cell).expect("preparsed");

        assert_eq!(preparsed.account_id, raw.account_id);
        assert_eq!(preparsed.type_script_hash, raw.type_script_hash);
        assert_eq!(preparsed.next_account_id, raw.next_account_id);
        assert_eq!(preparsed.expired_at, raw.expired_at);
        assert_eq!(preparsed.owner_lock_hash, raw.owner_lock_hash);
    }

    #[test]
    fn test_parse_accounts_fails_when_witness_name_missing() {
        let account_id = [0x22u8; 20];
        let tx = create_dotbit_tx(&account_id, false);

        let err = DotbitParser::parse_accounts(&tx).expect_err("missing name must fail");
        let msg = err.to_string();
        assert!(msg.contains("dotbit account name missing in DAS witness"));
        assert!(msg.contains(&tx.hash));
        assert!(msg.contains(&format!("0x{}", hex::encode(account_id))));
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
        assert_eq!(parsed_accounts[0].output_index, 0);
        assert_eq!(parsed_accounts[0].account.account_id, account_id.to_vec());
        assert_eq!(
            parsed_accounts[0].account.account.as_deref(),
            Some("alice.bit")
        );
    }

    #[test]
    fn test_parse_accounts_preserves_original_output_index() {
        let account_id = [0x44u8; 20];
        let dotbit_output = CellOutput {
            capacity: "0x174876e800".to_string(),
            lock: create_lock_script(),
            type_: Some(create_account_cell_type_script_with_args(&account_id)),
        };
        let dotbit_data_hex = format!(
            "0x{}",
            hex::encode(create_account_cell_data(&[0u8; 20], None, None))
        );

        let other_output = CellOutput {
            capacity: "0x174876e800".to_string(),
            lock: create_lock_script(),
            type_: None,
        };

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
            outputs: vec![other_output, dotbit_output],
            outputs_data: vec!["0x".to_string(), dotbit_data_hex],
            witnesses: vec![encode_dotbit_account_cell_witness(&account_id, "alice.bit")],
        };

        let parsed_accounts = DotbitParser::parse_accounts(&tx).expect("parse dotbit accounts");
        assert_eq!(parsed_accounts.len(), 1);
        assert_eq!(parsed_accounts[0].output_index, 1);
        assert_eq!(parsed_accounts[0].account.account_id, account_id.to_vec());
    }

    #[test]
    fn test_parse_dotbit_witness_bundle_supports_v2_bytes_fixvec() {
        let account_id = [0x55u8; 20];
        let witness = encode_dotbit_account_cell_witness_v2(&account_id, "smartest.bit");
        let result = parse_dotbit_witness_bundle(&[witness]);

        let data = result
            .accounts
            .get(account_id.as_slice())
            .expect("should find account");
        assert_eq!(data.name.as_deref(), Some("smartest.bit"));
    }

    #[test]
    fn test_parse_dotbit_witness_bundle_extracts_account_data_and_action_once() {
        let account_id = [0x77u8; 20];
        let account_witness = encode_dotbit_account_cell_witness(&account_id, "alice.bit");
        let action_witness = encode_das_action_witness("transfer_account");

        let bundle = parse_dotbit_witness_bundle(&[account_witness, action_witness]);

        assert_eq!(bundle.action.as_deref(), Some("transfer_account"));
        let account = bundle
            .accounts
            .get(account_id.as_slice())
            .expect("account bundle entry");
        assert_eq!(account.name.as_deref(), Some("alice.bit"));
    }

    #[test]
    fn test_parse_dotbit_witness_bundle_handles_non_das_witnesses() {
        let bundle = parse_dotbit_witness_bundle(&["0xaabbccdd".to_string()]);

        assert!(bundle.action.is_none());
        assert!(bundle.accounts.is_empty());
    }

    #[test]
    fn test_may_contain_das_witness_detects_das_headers_without_full_decode() {
        let account_id = [0x33u8; 20];
        let account_witness = encode_dotbit_account_cell_witness(&account_id, "alice.bit");
        let action_witness = encode_das_action_witness("transfer_account");

        assert!(may_contain_das_witness(&[account_witness]));
        assert!(may_contain_das_witness(&[action_witness]));
    }

    #[test]
    fn test_may_contain_das_witness_rejects_non_das_and_too_short_witnesses() {
        assert!(!may_contain_das_witness(&["0x".to_string()]));
        assert!(!may_contain_das_witness(&["0x64617300000000".to_string()]));
        assert!(!may_contain_das_witness(&["0xaabbccdd".to_string()]));
    }

    // ---- parse_das_action tests ----

    fn encode_das_action_witness(action: &str) -> String {
        let action_bytes = encode_molecule_bytes(action.as_bytes());
        let params_bytes = encode_molecule_bytes(&[]);
        let action_data = encode_molecule_table(&[action_bytes, params_bytes]);

        let mut witness = Vec::new();
        witness.extend_from_slice(b"das");
        witness.extend_from_slice(&DAS_ACTION_DATA_TYPE);
        witness.extend_from_slice(&action_data);
        format!("0x{}", hex::encode(witness))
    }

    #[test]
    fn test_parse_das_action_transfer_account() {
        let witness = encode_das_action_witness("transfer_account");
        let result = DotbitParser::parse_das_action(&[witness]);
        assert_eq!(result.as_deref(), Some("transfer_account"));
    }

    #[test]
    fn test_parse_das_action_recycle_expired_account() {
        let witness = encode_das_action_witness("recycle_expired_account");
        let result = DotbitParser::parse_das_action(&[witness]);
        assert_eq!(result.as_deref(), Some("recycle_expired_account"));
    }

    #[test]
    fn test_parse_das_action_confirm_proposal() {
        let witness = encode_das_action_witness("confirm_proposal");
        let result = DotbitParser::parse_das_action(&[witness]);
        assert_eq!(result.as_deref(), Some("confirm_proposal"));
    }

    #[test]
    fn test_parse_das_action_no_das_witness() {
        let plain_witness = "0xaabbccdd".to_string();
        let result = DotbitParser::parse_das_action(&[plain_witness]);
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_das_action_wrong_action_data_type() {
        // Use AccountCellData type (0x01000000) — should NOT match ActionData (0x00000000)
        let account_witness = encode_dotbit_account_cell_witness(&[0x11; 20], "alice.bit");
        let result = DotbitParser::parse_das_action(&[account_witness]);
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_das_action_picks_first_action_witness() {
        let action_witness = encode_das_action_witness("edit_records");
        let account_witness = encode_dotbit_account_cell_witness(&[0x11; 20], "alice.bit");
        // ActionData witness comes after AccountCellData — should still be found
        let result = DotbitParser::parse_das_action(&[account_witness, action_witness]);
        assert_eq!(result.as_deref(), Some("edit_records"));
    }

    #[test]
    fn test_parse_das_action_empty_witnesses() {
        let result = DotbitParser::parse_das_action(&[]);
        assert!(result.is_none());
    }

    // ---- registered_at / status enrichment tests ----

    #[test]
    fn test_parse_accounts_extracts_registered_at_and_status() {
        let account_id = [0x66u8; 20];
        let registered_at = 1700000000u64;
        let status = 1u8; // selling

        // Build a v3 witness with registered_at and status set
        let mut account_items = Vec::new();
        let account_str = "bob";
        for ch in account_str.chars() {
            let char_table = encode_molecule_table(&[
                2u32.to_le_bytes().to_vec(),
                encode_molecule_bytes(ch.to_string().as_bytes()),
            ]);
            account_items.push(char_table);
        }
        let account_chars = encode_molecule_dynvec(&account_items);
        let records_empty = encode_molecule_dynvec(&[]);

        let entity = encode_molecule_table(&[
            account_id.to_vec(),
            account_chars,
            registered_at.to_le_bytes().to_vec(), // field[2]
            0u64.to_le_bytes().to_vec(),
            0u64.to_le_bytes().to_vec(),
            0u64.to_le_bytes().to_vec(),
            vec![status], // field[6]
            records_empty,
            vec![0],
            0u64.to_le_bytes().to_vec(),
        ]);

        let data_entity = encode_molecule_table(&[
            0u32.to_le_bytes().to_vec(),
            3u32.to_le_bytes().to_vec(),
            encode_molecule_bytes(&entity),
        ]);

        let data = encode_molecule_table(&[Vec::new(), Vec::new(), data_entity]);

        let mut witness = Vec::new();
        witness.extend_from_slice(b"das");
        witness.extend_from_slice(&DAS_ACCOUNT_CELL_ACTION_DATA_TYPE);
        witness.extend_from_slice(&data);
        let witness_hex = format!("0x{}", hex::encode(witness));

        let output = CellOutput {
            capacity: "0x174876e800".to_string(),
            lock: create_lock_script(),
            type_: Some(create_account_cell_type_script_with_args(&account_id)),
        };
        let data_hex = format!(
            "0x{}",
            hex::encode(create_account_cell_data(
                &account_id,
                None,
                Some(1800000000)
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
            witnesses: vec![witness_hex],
        };

        let parsed = DotbitParser::parse_accounts(&tx).expect("parse");
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].account.account.as_deref(), Some("bob.bit"));
        assert_eq!(parsed[0].account.registered_at, Some(registered_at));
        assert_eq!(parsed[0].account.status, Some(status));
    }

    #[test]
    fn test_parse_account_parsed_cell_falls_back_to_data_when_type_args_absent() {
        let account_id = [0x55u8; 20];
        // Build a ParsedCell with type_code_hash set to DotBit AccountCell
        // but type_args = None (simulating historical cells without type args).
        let dotbit_code_hash = crate::rpc::parse_hex_to_bytes(DOTBIT_ACCOUNT_CELL_TYPE_ID);
        let type_script_hash = vec![0xAA; 32]; // dummy

        let data = create_account_cell_data(&account_id, None, Some(1735689600));

        let parsed_cell = crate::parser::cell::ParsedCell {
            capacity: 100_00000000,
            lock_code_hash: vec![0; 32],
            lock_hash_type: 0,
            lock_args: vec![],
            lock_script_hash: vec![0xBB; 32],
            type_code_hash: Some(dotbit_code_hash),
            type_hash_type: Some(1),
            type_args: None, // <-- the key condition: no type_args
            type_script_hash: Some(type_script_hash),
            data_hash: [0; 32],
            data_size: data.len() as i32,
            data,
        };

        let result = DotbitParser::parse_account_parsed_cell(&parsed_cell);
        assert!(
            result.is_some(),
            "should fall back to data[32..52] when type_args is None"
        );
        assert_eq!(result.unwrap().account_id, account_id.to_vec());
    }

    #[test]
    fn test_parse_account_parsed_cell_returns_none_when_both_sources_zero() {
        let dotbit_code_hash = crate::rpc::parse_hex_to_bytes(DOTBIT_ACCOUNT_CELL_TYPE_ID);

        let zero_account_id = [0u8; 20];
        let data = create_account_cell_data(&zero_account_id, None, None);

        let parsed_cell = crate::parser::cell::ParsedCell {
            capacity: 100_00000000,
            lock_code_hash: vec![0; 32],
            lock_hash_type: 0,
            lock_args: vec![],
            lock_script_hash: vec![0xBB; 32],
            type_code_hash: Some(dotbit_code_hash),
            type_hash_type: Some(1),
            type_args: None,
            type_script_hash: Some(vec![0xAA; 32]),
            data_hash: [0; 32],
            data_size: data.len() as i32,
            data,
        };

        let result = DotbitParser::parse_account_parsed_cell(&parsed_cell);
        assert!(
            result.is_none(),
            "should return None when both type_args and data account_id are zero"
        );
    }

    #[test]
    fn test_parse_accounts_errors_on_outputs_data_length_mismatch() {
        let tx = TransactionView {
            hash: "0xcccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc".to_string(),
            version: "0x0".to_string(),
            cell_deps: Vec::<CellDep>::new(),
            header_deps: Vec::new(),
            inputs: Vec::<CellInput>::new(),
            outputs: vec![CellOutput {
                capacity: "0x174876e800".to_string(),
                lock: create_lock_script(),
                type_: Some(create_account_cell_type_script_with_args(&[0x11; 20])),
            }],
            outputs_data: vec![],
            witnesses: vec![],
        };

        let err = DotbitParser::parse_accounts(&tx).unwrap_err();
        assert!(err
            .to_string()
            .contains("transaction outputs mismatch while parsing dotbit accounts"));
    }
}
