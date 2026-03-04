use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use anyhow::{anyhow, Result};
use tracing::warn;

use ckbadger_store::types::LiveCellInfo;
use ckbadger_store::CkbadgerStore;

use super::helpers::*;
use super::types::{TxData, XudtExtensionScript};

// ---------------------------------------------------------------------------
// Omnilock constants
// ---------------------------------------------------------------------------

pub(crate) const OMNILOCK_CODE_HASH_MAINNET_V2: &str =
    "0x9b819793a64463aed77c615d6cb226eea5487ccfc0783043a587254cda2b6f26";
pub(crate) const OMNILOCK_CODE_HASH_MAINNET_V1: &str =
    "0xa4398768d87bd17aea1361edc3accd6a0117774dc4ebc813bfa173e8ac0d086d";
pub(crate) const OMNILOCK_CODE_HASH_TESTNET_V2: &str =
    "0xf329effd1c475a2978453c8600e1eaf0bc2087ee093c3ee64cc96ec6847752cb";
pub(crate) const OMNILOCK_CODE_HASH_TESTNET_V1: &str =
    "0x79f90bb5e892d80dd213439eeab551120eb417678824f282b4ffb5f21bad2e1e";
const OMNILOCK_AUTH_LEN: usize = 21;
const OMNILOCK_SUPPLY_MODE_FLAG: u8 = 0b0000_1000;
const OMNILOCK_ADMIN_MODE_FLAG: u8 = 0b0000_0001;
const OMNILOCK_ACP_MODE_FLAG: u8 = 0b0000_0010;
const OMNILOCK_TIMELOCK_MODE_FLAG: u8 = 0b0000_0100;
const OMNILOCK_SUPPLY_INFO_CELL_MIN_DATA_LEN: usize = 65;
const OMNILOCK_SUPPLY_INFO_CELL_VERSION_V0: u8 = 0;

// ---------------------------------------------------------------------------
// XUDT / token-info constants
// ---------------------------------------------------------------------------

const XUDT_TYPE_ARGS_OWNER_LOCK_HASH_LEN: usize = 32;
const XUDT_TYPE_ARGS_FLAGS_LEN: usize = 4;
const XUDT_TYPE_ARGS_MIN_LEN: usize = XUDT_TYPE_ARGS_OWNER_LOCK_HASH_LEN + XUDT_TYPE_ARGS_FLAGS_LEN;
const XUDT_FLAGS_EXTENSION_MASK: u32 = 0x1FFF_FFFF;
const XUDT_FLAGS_EXTENSION_IN_ARGS: u32 = 0x1;
const XUDT_FLAGS_EXTENSION_IN_WITNESS: u32 = 0x2;
const XUDT_FLAGS_WITNESS_SCRIPT_HASH_LEN: usize = 20;
pub(crate) const UNIQUE_TYPE_ARGS_LEN: usize = 20;
const TOKEN_INFO_TAG_TOTAL_SUPPLY: u32 = 1;
const TOKEN_INFO_TOTAL_SUPPLY_DATA_LEN: usize = 16;

static OMNILOCK_CODE_HASHES: OnceLock<Vec<Vec<u8>>> = OnceLock::new();

// ---------------------------------------------------------------------------
// Omnilock functions
// ---------------------------------------------------------------------------

pub(crate) fn omnilock_code_hashes() -> &'static Vec<Vec<u8>> {
    OMNILOCK_CODE_HASHES.get_or_init(|| {
        [
            OMNILOCK_CODE_HASH_MAINNET_V2,
            OMNILOCK_CODE_HASH_MAINNET_V1,
            OMNILOCK_CODE_HASH_TESTNET_V2,
            OMNILOCK_CODE_HASH_TESTNET_V1,
        ]
        .iter()
        .map(|h| crate::rpc::parse_hex_to_bytes(h))
        .collect()
    })
}

pub(crate) fn is_omnilock_code_hash(code_hash: &[u8]) -> bool {
    omnilock_code_hashes()
        .iter()
        .any(|known| known.as_slice() == code_hash)
}

pub(crate) fn extract_omnilock_supply_info_type_hash(lock_args: &[u8]) -> Option<[u8; 32]> {
    if lock_args.len() <= OMNILOCK_AUTH_LEN {
        return None;
    }

    let omnilock_args = &lock_args[OMNILOCK_AUTH_LEN..];
    let flags = *omnilock_args.first()?;
    if flags & OMNILOCK_SUPPLY_MODE_FLAG == 0 {
        return None;
    }

    let mut offset = 1usize;
    if flags & OMNILOCK_ADMIN_MODE_FLAG != 0 {
        offset += 32;
    }
    if flags & OMNILOCK_ACP_MODE_FLAG != 0 {
        offset += 2;
    }
    if flags & OMNILOCK_TIMELOCK_MODE_FLAG != 0 {
        offset += 8;
    }

    if omnilock_args.len() < offset + 32 {
        return None;
    }

    let mut hash = [0u8; 32];
    hash.copy_from_slice(&omnilock_args[offset..offset + 32]);
    Some(hash)
}

pub(crate) fn parse_omnilock_supply_info_cell_data(data: &[u8]) -> Option<(i128, [u8; 32])> {
    if data.len() < OMNILOCK_SUPPLY_INFO_CELL_MIN_DATA_LEN {
        return None;
    }

    let version = data[0];
    if version != OMNILOCK_SUPPLY_INFO_CELL_VERSION_V0 {
        return None;
    }

    let current_supply = u128::from_le_bytes(data[1..17].try_into().ok()?);
    let max_supply = u128::from_le_bytes(data[17..33].try_into().ok()?);
    if current_supply > max_supply {
        return None;
    }
    if max_supply > i128::MAX as u128 {
        return None;
    }

    let mut token_type_hash = [0u8; 32];
    token_type_hash.copy_from_slice(&data[33..65]);
    Some((max_supply as i128, token_type_hash))
}

// ---------------------------------------------------------------------------
// Molecule parsing
// ---------------------------------------------------------------------------

pub(crate) fn parse_molecule_u32(data: &[u8]) -> Option<usize> {
    let raw: [u8; 4] = data.try_into().ok()?;
    Some(u32::from_le_bytes(raw) as usize)
}

pub(crate) fn parse_molecule_table_fields(data: &[u8], field_count: usize) -> Option<Vec<&[u8]>> {
    let header_size = 4 + field_count * 4;
    if data.len() < header_size {
        return None;
    }
    let total_size = parse_molecule_u32(&data[0..4])?;
    if total_size != data.len() {
        return None;
    }

    let mut offsets = Vec::with_capacity(field_count + 1);
    for idx in 0..field_count {
        let start = 4 + idx * 4;
        let end = start + 4;
        offsets.push(parse_molecule_u32(&data[start..end])?);
    }
    offsets.push(total_size);

    if offsets.first().copied()? != header_size {
        return None;
    }
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

pub(crate) fn parse_molecule_bytes(data: &[u8]) -> Option<&[u8]> {
    if data.len() < 4 {
        return None;
    }
    let total_size = parse_molecule_u32(&data[0..4])?;
    if total_size != data.len() {
        return None;
    }
    Some(&data[4..])
}

pub(crate) fn parse_molecule_dynvec_items(data: &[u8]) -> Option<Vec<&[u8]>> {
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

fn parse_molecule_script(data: &[u8]) -> Option<XudtExtensionScript> {
    let fields = parse_molecule_table_fields(data, 3)?;
    if fields[0].len() != 32 || fields[1].len() != 1 {
        return None;
    }
    let args = parse_molecule_bytes(fields[2])?.to_vec();
    Some(XudtExtensionScript { args })
}

// ---------------------------------------------------------------------------
// XUDT extension script extraction
// ---------------------------------------------------------------------------

pub(crate) fn parse_xudt_extension_scripts_from_script_vec(
    script_vec: &[u8],
) -> Option<Vec<XudtExtensionScript>> {
    let mut scripts = Vec::new();
    for item in parse_molecule_dynvec_items(script_vec)? {
        scripts.push(parse_molecule_script(item)?);
    }
    Some(scripts)
}

pub(crate) fn extract_xudt_witness_extension_script_vec(xudt_witness: &[u8]) -> Option<&[u8]> {
    let fields = parse_molecule_table_fields(xudt_witness, 4)?;
    if fields[2].is_empty() {
        None
    } else {
        Some(fields[2])
    }
}

pub(crate) fn extract_xudt_extension_scripts_from_witnesses(
    witnesses: &[String],
    expected_script_vec_hash: &[u8; 20],
) -> Option<Vec<XudtExtensionScript>> {
    for witness_hex in witnesses {
        let witness_bytes = crate::rpc::parse_hex_to_bytes(witness_hex);
        let witness_fields = match parse_molecule_table_fields(&witness_bytes, 3) {
            Some(fields) => fields,
            None => continue,
        };

        for bytes_opt_field in [&witness_fields[1], &witness_fields[2]] {
            if bytes_opt_field.is_empty() {
                continue;
            }
            let Some(xudt_witness_bytes) = parse_molecule_bytes(bytes_opt_field) else {
                continue;
            };
            let Some(script_vec_bytes) =
                extract_xudt_witness_extension_script_vec(xudt_witness_bytes)
            else {
                continue;
            };
            if blake160(script_vec_bytes) != *expected_script_vec_hash {
                continue;
            }
            if let Some(parsed) = parse_xudt_extension_scripts_from_script_vec(script_vec_bytes) {
                return Some(parsed);
            }
        }
    }
    None
}

pub(crate) fn extract_xudt_extension_scripts(
    type_args: &[u8],
    witnesses: &[String],
) -> Option<Vec<XudtExtensionScript>> {
    if type_args.len() < XUDT_TYPE_ARGS_MIN_LEN {
        return None;
    }
    let flags = u32::from_le_bytes(
        type_args[XUDT_TYPE_ARGS_OWNER_LOCK_HASH_LEN..XUDT_TYPE_ARGS_MIN_LEN]
            .try_into()
            .ok()?,
    );
    let extension_mode = flags & XUDT_FLAGS_EXTENSION_MASK;

    match extension_mode {
        XUDT_FLAGS_EXTENSION_IN_ARGS => {
            parse_xudt_extension_scripts_from_script_vec(&type_args[XUDT_TYPE_ARGS_MIN_LEN..])
        }
        XUDT_FLAGS_EXTENSION_IN_WITNESS => {
            let tail = &type_args[XUDT_TYPE_ARGS_MIN_LEN..];
            if tail.len() < XUDT_FLAGS_WITNESS_SCRIPT_HASH_LEN {
                return None;
            }
            let mut expected = [0u8; XUDT_FLAGS_WITNESS_SCRIPT_HASH_LEN];
            expected.copy_from_slice(&tail[..XUDT_FLAGS_WITNESS_SCRIPT_HASH_LEN]);
            extract_xudt_extension_scripts_from_witnesses(witnesses, &expected)
        }
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Token info / supply helpers
// ---------------------------------------------------------------------------

pub(crate) fn parse_token_info_total_supply(data: &[u8]) -> Option<i128> {
    if data.len() < 3 {
        return None;
    }

    let mut index = 0usize;
    index += 1; // decimal

    let name_len = *data.get(index)? as usize;
    index += 1;
    if data.len() < index + name_len + 1 {
        return None;
    }
    index += name_len;

    let symbol_len = *data.get(index)? as usize;
    index += 1;
    if data.len() < index + symbol_len {
        return None;
    }
    index += symbol_len;

    while index + 8 <= data.len() {
        let tag = u32::from_le_bytes(data[index..index + 4].try_into().ok()?);
        index += 4;
        let data_len = u32::from_le_bytes(data[index..index + 4].try_into().ok()?) as usize;
        index += 4;
        if data.len() < index + data_len {
            return None;
        }
        let value = &data[index..index + data_len];
        if tag == TOKEN_INFO_TAG_TOTAL_SUPPLY && data_len == TOKEN_INFO_TOTAL_SUPPLY_DATA_LEN {
            let raw = u128::from_le_bytes(value.try_into().ok()?);
            if raw > i128::MAX as u128 {
                return None;
            }
            return Some(raw as i128);
        }
        index += data_len;
    }

    None
}

pub(crate) fn collect_unique_cell_total_supply_by_type_args(
    cells: &[crate::parser::cell::ParsedCell],
) -> HashMap<Vec<u8>, i128> {
    let mut totals = HashMap::new();
    for cell in cells {
        let Some(type_args) = cell.type_args.as_ref() else {
            continue;
        };
        if type_args.len() != UNIQUE_TYPE_ARGS_LEN {
            continue;
        }
        let Some(total_supply) = parse_token_info_total_supply(&cell.data) else {
            continue;
        };
        totals.insert(type_args.clone(), total_supply);
    }
    totals
}

pub(crate) fn observe_max_supply(
    observations: &mut HashMap<Vec<u8>, i128>,
    tx_hash: &[u8; 32],
    token_type_hash: Vec<u8>,
    max_supply: i128,
    source: &str,
) {
    if let Some(existing) = observations.get(&token_type_hash) {
        if *existing != max_supply {
            warn!(
                tx_hash = %hex::encode(tx_hash),
                token_type_hash = %hex::encode(&token_type_hash),
                existing_max_supply = existing,
                observed_max_supply = max_supply,
                source = source,
                "conflicting max supply observations in the same batch; keeping first value"
            );
        }
        return;
    }

    observations.insert(token_type_hash, max_supply);
}

pub(crate) fn collect_token_max_supply_observations(
    all_tx_data: &[TxData],
) -> HashMap<Vec<u8>, i128> {
    let mut observations = HashMap::new();

    for tx_data in all_tx_data {
        let unique_cell_total_supply_by_type_args =
            collect_unique_cell_total_supply_by_type_args(&tx_data.cells);

        for cell in &tx_data.cells {
            if !is_omnilock_code_hash(&cell.lock_code_hash) {
                continue;
            }

            let Some(supply_info_type_hash) =
                extract_omnilock_supply_info_type_hash(&cell.lock_args)
            else {
                continue;
            };
            let Some(cell_type_hash) = cell.type_script_hash.as_ref() else {
                continue;
            };
            if cell_type_hash.as_slice() != supply_info_type_hash {
                continue;
            }

            let Some((max_supply, token_type_hash)) =
                parse_omnilock_supply_info_cell_data(&cell.data)
            else {
                continue;
            };
            observe_max_supply(
                &mut observations,
                &tx_data.hash,
                token_type_hash.to_vec(),
                max_supply,
                "omnilock_supply_info_cell",
            );
        }

        if unique_cell_total_supply_by_type_args.is_empty() {
            continue;
        }

        for cell in &tx_data.cells {
            let Some(type_code_hash) = cell.type_code_hash.as_ref() else {
                continue;
            };
            let Some(type_hash_type) = cell.type_hash_type else {
                continue;
            };
            if !matches!(
                crate::parser::UdtParser::is_udt_code_hash_bytes(type_code_hash, type_hash_type),
                Some(crate::parser::udt::UdtStandard::Xudt)
            ) {
                continue;
            }

            let Some(type_args) = cell.type_args.as_ref() else {
                continue;
            };
            let Some(token_type_hash) = cell.type_script_hash.as_ref() else {
                continue;
            };

            let Some(extension_scripts) =
                extract_xudt_extension_scripts(type_args, &tx_data.witnesses)
            else {
                continue;
            };

            for extension in extension_scripts {
                if extension.args.len() != UNIQUE_TYPE_ARGS_LEN {
                    continue;
                }
                let Some(max_supply) = unique_cell_total_supply_by_type_args
                    .get(&extension.args)
                    .copied()
                else {
                    continue;
                };
                observe_max_supply(
                    &mut observations,
                    &tx_data.hash,
                    token_type_hash.clone(),
                    max_supply,
                    "xudt_extension_script_unique_cell",
                );
            }
        }
    }

    observations
}

#[allow(clippy::type_complexity)]
pub(crate) fn load_activity_token_info_cache(
    store: &CkbadgerStore,
    tx_data: &[TxData],
    input_cell_info: &HashMap<(Vec<u8>, i16), LiveCellInfo>,
    batch_cell_infos: &HashMap<(Vec<u8>, i16), LiveCellInfo>,
) -> Result<HashMap<Vec<u8>, (Option<String>, Option<u8>)>> {
    let mut type_hashes = HashSet::<Vec<u8>>::new();

    for tx in tx_data {
        for cell in &tx.cells {
            if let Some(type_script_hash) = &cell.type_script_hash {
                type_hashes.insert(type_script_hash.clone());
            }
        }

        if tx.is_cellbase {
            continue;
        }

        for input in &tx.inputs {
            let key = (
                input.previous_tx_hash.to_vec(),
                parsed_input_outpoint_index_i16(input.previous_output_index, "sync_indexer"),
            );
            let cell_info = input_cell_info
                .get(&key)
                .or_else(|| batch_cell_infos.get(&key));
            if let Some(info) = cell_info {
                if let Some(type_script_hash) = &info.type_script_hash {
                    type_hashes.insert(type_script_hash.clone());
                }
            }
        }
    }

    let type_hash_vec: Vec<Vec<u8>> = type_hashes.into_iter().collect();
    let mut token_info_cache: HashMap<Vec<u8>, (Option<String>, Option<u8>)> = HashMap::new();
    for (type_hash, info) in store.get_tokens_batch(&type_hash_vec)? {
        let Some(info) = info else {
            continue;
        };
        let decimals = match info.decimals {
            Some(value) => Some(u8::try_from(value).map_err(|_| {
                anyhow!(
                    "token decimals out of u8 range while building activity cache: type_hash=0x{}, decimals={}",
                    hex::encode(&type_hash),
                    value
                )
            })?),
            None => None,
        };
        let symbol = info.symbol.clone().or(info.name.clone());
        token_info_cache.insert(type_hash, (symbol, decimals));
    }

    Ok(token_info_cache)
}

pub(crate) fn parse_parsed_cell_udt_amount(
    cell: &crate::parser::cell::ParsedCell,
    tx_hash: &[u8],
    output_index: i16,
    standard_hint: Option<&str>,
) -> Result<Option<u128>> {
    let standard = if let (Some(type_code_hash), Some(hash_type)) =
        (cell.type_code_hash.as_deref(), cell.type_hash_type)
    {
        crate::parser::UdtParser::is_udt_code_hash_bytes(type_code_hash, hash_type)
    } else {
        None
    };
    let standard = match standard {
        Some(standard) => standard,
        None => match standard_hint.and_then(crate::parser::UdtStandard::from_standard_hint) {
            Some(crate::parser::UdtStandard::Xudt) => crate::parser::UdtStandard::Xudt,
            _ => return Ok(None),
        },
    };
    let type_code_hash = cell.type_code_hash.as_deref().unwrap_or(&[]);

    let Some(amount) = crate::parser::UdtParser::parse_amount(&cell.data) else {
        // xUDT-compatible cells can carry non-amount payloads (for example owner-mode cells).
        // They should not be indexed as fungible UDT balances/transfers.
        if matches!(standard, crate::parser::UdtStandard::Xudt) {
            return Ok(None);
        }
        return Err(anyhow!(
            "failed to parse UDT amount from parsed output data: outpoint=0x{}:{}, type_code_hash=0x{}",
            hex::encode(tx_hash),
            output_index,
            hex::encode(type_code_hash)
        ));
    };
    Ok(Some(amount))
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use ckbadger_store::batch::StoreBatch;

    // -- molecule encoding helpers (test-only) --------------------------------

    fn molecule_u32(value: usize) -> [u8; 4] {
        (value as u32).to_le_bytes()
    }

    fn molecule_table(fields: &[Vec<u8>]) -> Vec<u8> {
        let header_size = 4 + fields.len() * 4;
        let total_size = header_size + fields.iter().map(|field| field.len()).sum::<usize>();

        let mut out = Vec::with_capacity(total_size);
        out.extend_from_slice(&molecule_u32(total_size));

        let mut offset = header_size;
        for field in fields {
            out.extend_from_slice(&molecule_u32(offset));
            offset += field.len();
        }
        for field in fields {
            out.extend_from_slice(field);
        }
        out
    }

    fn molecule_bytes(value: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(4 + value.len());
        out.extend_from_slice(&molecule_u32(4 + value.len()));
        out.extend_from_slice(value);
        out
    }

    fn molecule_dynvec(items: &[Vec<u8>]) -> Vec<u8> {
        if items.is_empty() {
            return molecule_u32(4).to_vec();
        }

        let header_size = 4 + items.len() * 4;
        let total_size = header_size + items.iter().map(|item| item.len()).sum::<usize>();

        let mut out = Vec::with_capacity(total_size);
        out.extend_from_slice(&molecule_u32(total_size));

        let mut offset = header_size;
        for item in items {
            out.extend_from_slice(&molecule_u32(offset));
            offset += item.len();
        }
        for item in items {
            out.extend_from_slice(item);
        }
        out
    }

    fn encode_script(args: &[u8]) -> Vec<u8> {
        molecule_table(&[vec![0xCC; 32], vec![1], molecule_bytes(args)])
    }

    fn encode_script_vec_with_unique_args(unique_type_args: &[u8]) -> Vec<u8> {
        molecule_dynvec(&[encode_script(unique_type_args)])
    }

    fn encode_xudt_witness(script_vec: &[u8]) -> Vec<u8> {
        molecule_table(&[Vec::new(), Vec::new(), script_vec.to_vec(), Vec::new()])
    }

    fn encode_witness_args(input_type: Option<&[u8]>, output_type: Option<&[u8]>) -> Vec<u8> {
        let lock = Vec::new();
        let input_type = input_type.map(molecule_bytes).unwrap_or_default();
        let output_type = output_type.map(molecule_bytes).unwrap_or_default();
        molecule_table(&[lock, input_type, output_type])
    }

    // -- token data helpers (test-only) ----------------------------------------

    fn build_token_info_data(total_supply: u128) -> Vec<u8> {
        let mut data = Vec::new();
        data.push(8); // decimal
        data.push(5); // name len
        data.extend_from_slice(b"Token");
        data.push(3); // symbol len
        data.extend_from_slice(b"TKN");
        data.extend_from_slice(&TOKEN_INFO_TAG_TOTAL_SUPPLY.to_le_bytes());
        data.extend_from_slice(&(TOKEN_INFO_TOTAL_SUPPLY_DATA_LEN as u32).to_le_bytes());
        data.extend_from_slice(&total_supply.to_le_bytes());
        data
    }

    fn dummy_unique_token_info_cell(
        unique_type_args: Vec<u8>,
        total_supply: u128,
    ) -> crate::parser::cell::ParsedCell {
        let data = build_token_info_data(total_supply);
        crate::parser::cell::ParsedCell {
            capacity: 100_00000000,
            lock_code_hash: vec![0x10; 32],
            lock_hash_type: 1,
            lock_args: vec![0x20; 20],
            lock_script_hash: vec![0x30; 32],
            type_code_hash: Some(vec![0x40; 32]),
            type_hash_type: Some(1),
            type_args: Some(unique_type_args),
            type_script_hash: Some(vec![0x50; 32]),
            data_hash: vec![0x60; 32],
            data_size: data.len() as i32,
            data,
        }
    }

    fn dummy_xudt_cell(
        token_type_hash: [u8; 32],
        type_args: Vec<u8>,
    ) -> crate::parser::cell::ParsedCell {
        let type_code_hash =
            crate::rpc::parse_hex_to_bytes(crate::parser::udt::XUDT_CODE_HASH_TYPE);
        crate::parser::cell::ParsedCell {
            capacity: 100_00000000,
            lock_code_hash: vec![0x11; 32],
            lock_hash_type: 1,
            lock_args: vec![0x22; 20],
            lock_script_hash: vec![0x33; 32],
            type_code_hash: Some(type_code_hash),
            type_hash_type: Some(1),
            type_args: Some(type_args),
            type_script_hash: Some(token_type_hash.to_vec()),
            data_hash: vec![0x44; 32],
            data_size: 16,
            data: vec![0u8; 16],
        }
    }

    fn build_xudt_type_args_with_extension_in_args(
        owner_lock_hash: [u8; 32],
        script_vec: &[u8],
    ) -> Vec<u8> {
        let mut type_args = Vec::with_capacity(XUDT_TYPE_ARGS_MIN_LEN + script_vec.len());
        type_args.extend_from_slice(&owner_lock_hash);
        type_args.extend_from_slice(&XUDT_FLAGS_EXTENSION_IN_ARGS.to_le_bytes());
        type_args.extend_from_slice(script_vec);
        type_args
    }

    fn build_xudt_type_args_with_extension_in_witness(
        owner_lock_hash: [u8; 32],
        script_vec_hash: [u8; 20],
    ) -> Vec<u8> {
        let mut type_args = Vec::with_capacity(XUDT_TYPE_ARGS_MIN_LEN + script_vec_hash.len());
        type_args.extend_from_slice(&owner_lock_hash);
        type_args.extend_from_slice(&XUDT_FLAGS_EXTENSION_IN_WITNESS.to_le_bytes());
        type_args.extend_from_slice(&script_vec_hash);
        type_args
    }

    fn dummy_tx_data(
        hash: [u8; 32],
        is_cellbase: bool,
        inputs: Vec<crate::parser::transaction::ParsedInput>,
        cells: Vec<crate::parser::cell::ParsedCell>,
        witnesses: Vec<String>,
        outputs_data: Vec<String>,
    ) -> TxData {
        let inputs_count =
            i16::try_from(inputs.len()).expect("test helper inputs_count exceeds i16 range");
        let outputs_count =
            i16::try_from(cells.len()).expect("test helper outputs_count exceeds i16 range");
        let witnesses_count =
            i16::try_from(witnesses.len()).expect("test helper witnesses_count exceeds i16 range");
        TxData {
            hash,
            block_number: 0,
            block_hash: vec![],
            tx_index: 0,
            version: 0,
            inputs_count,
            outputs_count,
            witnesses_count,
            cell_deps_count: 0,
            header_deps_count: 0,
            is_cellbase,
            inputs,
            cells,
            witnesses,
            outputs_data,
            total_input_capacity: 0,
            total_output_capacity: 0,
            fee: 0,
            tx_size: 0,
            cycles: None,
            timestamp: Utc::now(),
        }
    }

    fn make_token_info(
        decimals: Option<i32>,
        symbol: Option<&str>,
    ) -> ckbadger_store::types::TokenInfo {
        ckbadger_store::types::TokenInfo {
            type_code_hash: vec![0x77; 32],
            hash_type: 1,
            type_args: vec![0x88; 32],
            standard: "xudt".to_string(),
            name: Some("FallbackName".to_string()),
            symbol: symbol.map(|s| s.to_string()),
            decimals,
            total_supply: Some(0),
            max_supply: None,
            holders_count: 0,
            first_seen_block: 0,
            icon_url: None,
            description: None,
            transfers_count: 0,
        }
    }

    // -- parse_parsed_cell_udt_amount tests ------------------------------------

    #[test]
    fn test_parse_parsed_cell_udt_amount_allows_xudt_without_amount_payload() {
        let mut cell = dummy_xudt_cell([0xAB; 32], vec![0xCD; XUDT_TYPE_ARGS_MIN_LEN]);
        cell.data.clear();
        cell.data_size = 0;

        let tx_hash = [0x81; 32];
        let amount = parse_parsed_cell_udt_amount(&cell, &tx_hash, 3, None).unwrap();
        assert_eq!(amount, None);
    }

    #[test]
    fn test_parse_parsed_cell_udt_amount_rejects_invalid_sudt_payload() {
        let sudt_code_hash = crate::rpc::parse_hex_to_bytes(crate::parser::udt::SUDT_CODE_HASH);
        let cell = crate::parser::cell::ParsedCell {
            capacity: 100_00000000,
            lock_code_hash: vec![0x11; 32],
            lock_hash_type: 1,
            lock_args: vec![0x22; 20],
            lock_script_hash: vec![0x33; 32],
            type_code_hash: Some(sudt_code_hash.clone()),
            type_hash_type: Some(1),
            type_args: Some(vec![0x44; 32]),
            type_script_hash: Some(vec![0x55; 32]),
            data_hash: vec![0x66; 32],
            data_size: 0,
            data: vec![],
        };

        let tx_hash = [0x82; 32];
        let err = parse_parsed_cell_udt_amount(&cell, &tx_hash, 7, None).unwrap_err();
        assert!(err.to_string().contains("failed to parse UDT amount"));
        assert!(err.to_string().contains("0x8282828282828282"));
    }

    #[test]
    fn test_parse_parsed_cell_udt_amount_supports_xudt_compatible_hint() {
        let amount = 15_778_600u128;
        let mut data = vec![0u8; 16];
        data.copy_from_slice(&amount.to_le_bytes());
        let cell = crate::parser::cell::ParsedCell {
            capacity: 100_00000000,
            lock_code_hash: vec![0x11; 32],
            lock_hash_type: 1,
            lock_args: vec![0x22; 20],
            lock_script_hash: vec![0x33; 32],
            type_code_hash: Some(vec![0x42; 32]), // non-standard xUDT code hash
            type_hash_type: Some(1),
            type_args: Some(vec![0x44; 32]),
            type_script_hash: Some(vec![0x55; 32]),
            data_hash: vec![0x66; 32],
            data_size: 16,
            data,
        };

        let tx_hash = [0x83; 32];
        let parsed =
            parse_parsed_cell_udt_amount(&cell, &tx_hash, 0, Some("xudt_compatible")).unwrap();
        assert_eq!(parsed, Some(amount));

        let no_hint = parse_parsed_cell_udt_amount(&cell, &tx_hash, 0, None).unwrap();
        assert_eq!(no_hint, None);
    }

    // -- omnilock tests --------------------------------------------------------

    #[test]
    fn test_extract_omnilock_supply_info_type_hash_with_all_modes() {
        let mut lock_args = vec![0u8; OMNILOCK_AUTH_LEN];
        let flags = OMNILOCK_SUPPLY_MODE_FLAG
            | OMNILOCK_ADMIN_MODE_FLAG
            | OMNILOCK_ACP_MODE_FLAG
            | OMNILOCK_TIMELOCK_MODE_FLAG;
        lock_args.push(flags);
        lock_args.extend_from_slice(&[0xAA; 32]); // admin list type id
        lock_args.extend_from_slice(&[0x01, 0x02]); // ACP min
        lock_args.extend_from_slice(&[0xBB; 8]); // since
        lock_args.extend_from_slice(&[0xCC; 32]); // supply info type script hash

        let parsed = extract_omnilock_supply_info_type_hash(&lock_args).unwrap();
        assert_eq!(parsed, [0xCC; 32]);
    }

    #[test]
    fn test_parse_omnilock_supply_info_cell_data_validates_bounds() {
        let mut data = Vec::with_capacity(65);
        data.push(0u8); // version
        data.extend_from_slice(&5u128.to_le_bytes()); // current
        data.extend_from_slice(&10u128.to_le_bytes()); // max
        data.extend_from_slice(&[0x11; 32]); // sUDT/xUDT type script hash

        let parsed = parse_omnilock_supply_info_cell_data(&data).unwrap();
        assert_eq!(parsed.0, 10);
        assert_eq!(parsed.1, [0x11; 32]);

        let mut invalid = data.clone();
        invalid[1..17].copy_from_slice(&11u128.to_le_bytes()); // current > max
        assert!(parse_omnilock_supply_info_cell_data(&invalid).is_none());
    }

    // -- collect_token_max_supply_observations tests ---------------------------

    #[test]
    fn test_collect_token_max_supply_observations_from_omnilock_info_cells() {
        let supply_info_type_hash = [0x22; 32];
        let token_type_hash = [0x33; 32];

        let mut lock_args = vec![0u8; OMNILOCK_AUTH_LEN];
        lock_args.push(OMNILOCK_SUPPLY_MODE_FLAG);
        lock_args.extend_from_slice(&supply_info_type_hash);

        let mut info_cell_data = Vec::with_capacity(65);
        info_cell_data.push(0u8);
        info_cell_data.extend_from_slice(&100u128.to_le_bytes());
        info_cell_data.extend_from_slice(&1_000u128.to_le_bytes());
        info_cell_data.extend_from_slice(&token_type_hash);

        let info_cell = crate::parser::cell::ParsedCell {
            capacity: 100_00000000,
            lock_code_hash: crate::rpc::parse_hex_to_bytes(OMNILOCK_CODE_HASH_MAINNET_V2),
            lock_hash_type: 1,
            lock_args,
            lock_script_hash: vec![0x44; 32],
            type_code_hash: Some(vec![0x55; 32]),
            type_hash_type: Some(1),
            type_args: Some(vec![0x66; 32]),
            type_script_hash: Some(supply_info_type_hash.to_vec()),
            data_hash: vec![0x77; 32],
            data_size: info_cell_data.len() as i32,
            data: info_cell_data,
        };

        let tx = dummy_tx_data([0x88; 32], false, vec![], vec![info_cell], vec![], vec![]);
        let observations = collect_token_max_supply_observations(&[tx]);
        assert_eq!(observations.get(token_type_hash.as_slice()), Some(&1_000));
    }

    #[test]
    fn test_collect_token_max_supply_observations_from_xudt_extension_flags_0x1() {
        let unique_type_args = vec![0xAB; UNIQUE_TYPE_ARGS_LEN];
        let total_supply = 42_000u128;
        let token_type_hash = [0x91; 32];
        let script_vec = encode_script_vec_with_unique_args(&unique_type_args);
        let type_args = build_xudt_type_args_with_extension_in_args([0x01; 32], &script_vec);

        let unique_cell = dummy_unique_token_info_cell(unique_type_args.clone(), total_supply);
        let xudt_cell = dummy_xudt_cell(token_type_hash, type_args);
        let tx = dummy_tx_data(
            [0xEE; 32],
            false,
            vec![],
            vec![unique_cell, xudt_cell],
            vec![],
            vec![],
        );

        let observations = collect_token_max_supply_observations(&[tx]);
        assert_eq!(
            observations.get(token_type_hash.as_slice()),
            Some(&(total_supply as i128))
        );
    }

    #[test]
    fn test_collect_token_max_supply_observations_from_xudt_extension_flags_0x2_witness() {
        let unique_type_args = vec![0xBC; UNIQUE_TYPE_ARGS_LEN];
        let total_supply = 100_001u128;
        let token_type_hash = [0x92; 32];
        let script_vec = encode_script_vec_with_unique_args(&unique_type_args);
        let script_vec_hash = blake160(&script_vec);
        let type_args = build_xudt_type_args_with_extension_in_witness([0x02; 32], script_vec_hash);

        let xudt_witness = encode_xudt_witness(&script_vec);
        let witness_args = encode_witness_args(Some(&xudt_witness), None);
        let witness_hex = format!("0x{}", hex::encode(witness_args));

        let unique_cell = dummy_unique_token_info_cell(unique_type_args.clone(), total_supply);
        let xudt_cell = dummy_xudt_cell(token_type_hash, type_args);
        let tx = dummy_tx_data(
            [0xEF; 32],
            false,
            vec![],
            vec![unique_cell, xudt_cell],
            vec![witness_hex],
            vec![],
        );

        let observations = collect_token_max_supply_observations(&[tx]);
        assert_eq!(
            observations.get(token_type_hash.as_slice()),
            Some(&(total_supply as i128))
        );
    }

    #[test]
    fn test_collect_token_max_supply_observations_skips_xudt_extension_flags_0x2_when_witness_invalid(
    ) {
        let unique_type_args = vec![0xCD; UNIQUE_TYPE_ARGS_LEN];
        let total_supply = 77_700u128;
        let token_type_hash = [0x93; 32];
        let script_vec = encode_script_vec_with_unique_args(&unique_type_args);
        let type_args =
            build_xudt_type_args_with_extension_in_witness([0x03; 32], blake160(&script_vec));

        let mismatched_script_vec =
            encode_script_vec_with_unique_args(&[0xDD; UNIQUE_TYPE_ARGS_LEN]);
        let mismatched_witness =
            encode_witness_args(Some(&encode_xudt_witness(&mismatched_script_vec)), None);
        let tx_with_hash_mismatch = dummy_tx_data(
            [0xA1; 32],
            false,
            vec![],
            vec![
                dummy_unique_token_info_cell(unique_type_args.clone(), total_supply),
                dummy_xudt_cell(token_type_hash, type_args.clone()),
            ],
            vec![format!("0x{}", hex::encode(mismatched_witness))],
            vec![],
        );

        let tx_without_witness = dummy_tx_data(
            [0xA2; 32],
            false,
            vec![],
            vec![
                dummy_unique_token_info_cell(unique_type_args, total_supply),
                dummy_xudt_cell(token_type_hash, type_args),
            ],
            vec![],
            vec![],
        );

        let mismatch_observations = collect_token_max_supply_observations(&[tx_with_hash_mismatch]);
        assert!(!mismatch_observations.contains_key(token_type_hash.as_slice()));

        let missing_observations = collect_token_max_supply_observations(&[tx_without_witness]);
        assert!(!missing_observations.contains_key(token_type_hash.as_slice()));
    }

    // -- load_activity_token_info_cache tests ----------------------------------

    #[test]
    fn test_load_activity_token_info_cache_prefers_symbol_and_converts_decimals() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_domain(dir.path().to_str().unwrap()).unwrap();

        let type_hash = vec![0xAA; 32];
        let mut batch = StoreBatch::new(&store);
        batch.put_token(&type_hash, &make_token_info(Some(8), Some("OTTER")));
        batch.commit().unwrap();

        let tx = dummy_tx_data(
            [0x11; 32],
            false,
            vec![],
            vec![dummy_xudt_cell(
                <[u8; 32]>::try_from(type_hash.clone()).unwrap(),
                vec![0x99; 32],
            )],
            vec![],
            vec![],
        );

        let cache = load_activity_token_info_cache(&store, &[tx], &HashMap::new(), &HashMap::new())
            .unwrap();

        assert_eq!(
            cache.get(&type_hash),
            Some(&(Some("OTTER".to_string()), Some(8)))
        );
    }

    #[test]
    fn test_load_activity_token_info_cache_errors_on_invalid_decimals() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_domain(dir.path().to_str().unwrap()).unwrap();

        let type_hash = vec![0xAB; 32];
        let mut batch = StoreBatch::new(&store);
        batch.put_token(&type_hash, &make_token_info(Some(300), None));
        batch.commit().unwrap();

        let tx = dummy_tx_data(
            [0x12; 32],
            false,
            vec![],
            vec![dummy_xudt_cell(
                <[u8; 32]>::try_from(type_hash.clone()).unwrap(),
                vec![0x98; 32],
            )],
            vec![],
            vec![],
        );

        let err = load_activity_token_info_cache(&store, &[tx], &HashMap::new(), &HashMap::new())
            .unwrap_err();

        assert!(err.to_string().contains("out of u8 range"));
    }
}
