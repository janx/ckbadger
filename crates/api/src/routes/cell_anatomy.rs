//! Exact Molecule representation of a CellOutput. The byte map partitions the
//! serialized value; `occupied` identifies protocol occupied-capacity bytes.
use anyhow::{bail, ensure, Context, Result};
use ckbadger_store::PositionedCellInfo;
use serde::Serialize;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MoleculeField {
    pub label: String,
    pub start: usize,
    pub end: usize,
    pub occupied: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MoleculeCellOutput {
    pub bytes: String,
    pub fields: Vec<MoleculeField>,
    pub lock_script_hash: String,
    pub type_script_hash: Option<String>,
}

fn u32_bytes(n: usize, context: &str) -> Result<[u8; 4]> {
    Ok(u32::try_from(n)
        .with_context(|| format!("{context} exceeds Molecule u32 size"))?
        .to_le_bytes())
}

fn field(fields: &mut Vec<MoleculeField>, label: &str, start: usize, end: usize, occupied: bool) {
    fields.push(MoleculeField {
        label: label.into(),
        start,
        end,
        occupied,
    });
}

fn append_script(
    bytes: &mut Vec<u8>,
    fields: &mut Vec<MoleculeField>,
    prefix: &str,
    code_hash: &[u8],
    hash_type: i16,
    args: &[u8],
) -> Result<String> {
    ensure!(
        code_hash.len() == 32,
        "{prefix} code hash has {} bytes",
        code_hash.len()
    );
    let hash_type = match hash_type {
        0 | 1 | 2 | 4 => hash_type as u8,
        other => bail!("{prefix} has invalid hash type {other}"),
    };
    let start = bytes.len();
    let size = 16usize
        .checked_add(32 + 1 + 4)
        .and_then(|n| n.checked_add(args.len()))
        .context("Molecule script size overflow")?;
    bytes.extend(u32_bytes(size, prefix)?);
    field(
        fields,
        &format!("{prefix}.total_size"),
        start,
        start + 4,
        false,
    );
    for (i, (label, offset)) in [
        ("offset_code_hash", 16),
        ("offset_hash_type", 48),
        ("offset_args", 49),
    ]
    .into_iter()
    .enumerate()
    {
        bytes.extend((offset as u32).to_le_bytes());
        field(
            fields,
            &format!("{prefix}.{label}"),
            start + 4 + 4 * i,
            start + 8 + 4 * i,
            false,
        );
    }
    let hash_start = bytes.len();
    bytes.extend(code_hash);
    field(
        fields,
        &format!("{prefix}.code_hash"),
        hash_start,
        bytes.len(),
        true,
    );
    let kind_start = bytes.len();
    bytes.push(hash_type);
    field(
        fields,
        &format!("{prefix}.hash_type"),
        kind_start,
        bytes.len(),
        true,
    );
    let len_start = bytes.len();
    bytes.extend(u32_bytes(args.len(), prefix)?);
    field(
        fields,
        &format!("{prefix}.args_length"),
        len_start,
        bytes.len(),
        false,
    );
    let args_start = bytes.len();
    bytes.extend(args);
    if !args.is_empty() {
        field(
            fields,
            &format!("{prefix}.args"),
            args_start,
            bytes.len(),
            true,
        );
    }
    ensure!(
        bytes.len() == start + size,
        "{prefix} serialized size mismatch"
    );
    let mut digest = [0u8; 32];
    let mut hasher = ckb_hash::new_blake2b();
    hasher.update(&bytes[start..]);
    hasher.finalize(&mut digest);
    Ok(format!("0x{}", hex::encode(digest)))
}

pub fn cell_output(info: &PositionedCellInfo) -> Result<MoleculeCellOutput> {
    let mut bytes = vec![0; 16];
    let mut fields = Vec::new();
    for (i, label) in [
        "total_size",
        "offset_capacity",
        "offset_lock",
        "offset_type",
    ]
    .into_iter()
    .enumerate()
    {
        field(&mut fields, label, 4 * i, 4 * (i + 1), false);
    }
    let capacity_start = bytes.len();
    bytes.extend(info.capacity.to_le_bytes());
    field(&mut fields, "capacity", capacity_start, bytes.len(), true);
    let lock_start = bytes.len();
    let lock_script_hash = append_script(
        &mut bytes,
        &mut fields,
        "lock",
        &info.lock_code_hash,
        info.lock_hash_type,
        &info.lock_args,
    )?;
    let type_start = bytes.len();
    let type_script_hash = match (&info.type_code_hash, info.type_hash_type, &info.type_args) {
        (None, None, None) => None,
        (Some(code_hash), Some(hash_type), Some(args)) => Some(append_script(
            &mut bytes,
            &mut fields,
            "type",
            code_hash,
            hash_type,
            args,
        )?),
        _ => bail!("incomplete type script components in indexed cell"),
    };
    let size = bytes.len();
    bytes[0..4].copy_from_slice(&u32_bytes(size, "CellOutput")?);
    bytes[4..8].copy_from_slice(&16u32.to_le_bytes());
    bytes[8..12].copy_from_slice(&u32_bytes(lock_start, "lock offset")?);
    bytes[12..16].copy_from_slice(&u32_bytes(type_start, "type offset")?);
    ensure!(
        fields.windows(2).all(|pair| pair[0].end == pair[1].start)
            && fields.last().is_some_and(|last| last.end == size),
        "CellOutput field map does not partition all serialized bytes"
    );
    Ok(MoleculeCellOutput {
        bytes: format!("0x{}", hex::encode(bytes)),
        fields,
        lock_script_hash,
        type_script_hash,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ckb_types::{packed, prelude::*};
    use ckbadger_store::types::LiveCellInfo;

    #[test]
    fn packed_mainnet_oracles_match_serializer_and_occupied_partition() {
        for oracle in [
            include_str!("../../tests/fixtures/d5-cell-output.hex"),
            include_str!("../../tests/fixtures/d5-codecell-output.hex"),
        ] {
            let raw = hex::decode(oracle.trim()).unwrap();
            let packed = packed::CellOutput::from_slice(&raw).unwrap();
            let lock = packed.lock();
            let type_script = packed.type_().to_opt();
            let capacity: u64 = packed.capacity().unpack();
            let script_hash =
                |script: &packed::Script| script.calc_script_hash().as_slice().to_vec();
            let payload = LiveCellInfo {
                capacity: i64::try_from(capacity).unwrap(),
                lock_script_hash: script_hash(&lock),
                lock_code_hash: lock.code_hash().as_slice().to_vec(),
                lock_hash_type: lock.hash_type().as_slice()[0] as i16,
                lock_args: lock.args().raw_data().to_vec(),
                type_script_hash: type_script.as_ref().map(script_hash),
                type_code_hash: type_script
                    .as_ref()
                    .map(|script| script.code_hash().as_slice().to_vec()),
                type_hash_type: type_script
                    .as_ref()
                    .map(|script| script.hash_type().as_slice()[0] as i16),
                type_args: type_script
                    .as_ref()
                    .map(|script| script.args().raw_data().to_vec()),
                data_size: 0,
                occupied_capacity: 0,
                udt_amount: None,
                data_hash: None,
            };
            let anatomy = cell_output(&PositionedCellInfo::new(payload, 1)).unwrap();
            assert_eq!(
                hex::decode(anatomy.bytes.trim_start_matches("0x")).unwrap(),
                raw
            );
            let occupied: usize = anatomy
                .fields
                .iter()
                .filter(|field| field.occupied)
                .map(|field| field.end - field.start)
                .sum();
            let expected = 8
                + 33
                + lock.args().raw_data().len()
                + type_script
                    .as_ref()
                    .map_or(0, |script| 33 + script.args().raw_data().len());
            assert_eq!(occupied, expected);
            assert_eq!(
                anatomy.lock_script_hash,
                format!("0x{}", hex::encode(script_hash(&lock)))
            );
        }
    }
}
