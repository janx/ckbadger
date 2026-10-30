//! Exact transaction script membership and witness byte views.
use super::transactions::{TransactionInputResponse, TransactionOutputResponse};
use crate::response::ScriptResponse;
use ckb_types::{packed, prelude::*};
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptGroupResponse {
    pub script_hash: String,
    pub kind: String,
    pub ran: bool,
    pub input_indices: Vec<usize>,
    pub output_indices: Vec<usize>,
    pub witness_index: Option<usize>,
    pub script: ScriptResponse,
    pub cycles: Option<String>,
}

/// Canonical protocol identity from the same registry as the indexer.
/// Unrecognized type scripts stay typed; absence of a type stays plain.
pub fn cell_type(
    script: Option<&ScriptResponse>,
) -> Result<String, crate::response::ApiRouteError> {
    let Some(script) = script else {
        return Ok("plain".into());
    };
    let code_hash = hex::decode(script.code_hash.trim_start_matches("0x")).map_err(|e| {
        crate::response::ApiError::internal(format!("invalid cell type code hash: {e}"))
    })?;
    if let Some(standard) =
        ckbadger_indexer::parser::udt::UdtParser::is_udt_code_hash_bytes(&code_hash, 0)
    {
        return Ok(standard.as_str().into());
    }
    Ok(ckbadger_indexer::parser::registry::PROTOCOL_REGISTRY
        .get(&code_hash)
        .map(|protocol| protocol.slug())
        .unwrap_or("typed")
        .into())
}

pub fn script_hash(script: &ScriptResponse) -> anyhow::Result<String> {
    let code_hash = hex::decode(script.code_hash.trim_start_matches("0x"))?;
    anyhow::ensure!(code_hash.len() == 32, "script code hash must be 32 bytes");
    let args = hex::decode(script.args.trim_start_matches("0x"))?;
    let hash_type = ckbadger_common::hash_type_from_label(&script.hash_type)
        .ok_or_else(|| anyhow::anyhow!("invalid script hash type {}", script.hash_type))?;
    Ok(format!(
        "0x{}",
        hex::encode(crate::utils::address::compute_script_hash(
            &code_hash, hash_type, &args
        ))
    ))
}

pub fn script_groups(
    inputs: &[TransactionInputResponse],
    outputs: &[TransactionOutputResponse],
) -> anyhow::Result<Vec<ScriptGroupResponse>> {
    let mut groups: Vec<ScriptGroupResponse> = Vec::new();
    let mut positions = BTreeMap::new();
    let mut add = |script: &ScriptResponse,
                   kind: &str,
                   ran: bool,
                   input: Option<usize>,
                   output: Option<usize>|
     -> anyhow::Result<()> {
        let hash = script_hash(script)?;
        let key = (kind.to_string(), ran, hash.clone());
        let position = *positions.entry(key).or_insert_with(|| {
            let position = groups.len();
            groups.push(ScriptGroupResponse {
                script_hash: hash,
                kind: kind.to_string(),
                ran,
                input_indices: Vec::new(),
                output_indices: Vec::new(),
                witness_index: None,
                script: script.clone(),
                cycles: None,
            });
            position
        });
        let group = &mut groups[position];
        if let Some(index) = input {
            group.input_indices.push(index);
            // Lock and type groups both read the witness at the group's first
            // input index (WitnessArgs). Cobuild / CKBFS content witnesses are
            // attached afterwards by `attach_content_witnesses`.
            if group.witness_index.is_none() {
                group.witness_index = Some(index);
            }
        }
        if let Some(index) = output {
            group.output_indices.push(index);
        }
        Ok(())
    };
    for (index, input) in inputs.iter().enumerate() {
        if let Some(lock) = &input.lock {
            add(lock, "lock", true, Some(index), None)?;
        }
    }
    for (index, input) in inputs.iter().enumerate() {
        if let Some(script) = &input.r#type {
            add(script, "type", true, Some(index), None)?;
        }
    }
    for (index, output) in outputs.iter().enumerate() {
        if let Some(script) = &output.r#type {
            add(script, "type", true, None, Some(index))?;
        }
    }
    for (index, output) in outputs.iter().enumerate() {
        if let Some(lock) = &output.lock {
            add(lock, "lock", false, None, Some(index))?;
        }
    }
    Ok(groups)
}

/// Attach the indexer's canonical, validated result by exact execution-group identity.
/// Output locks are displayed as cell structure but are never execution results.
pub fn apply_group_cycles(
    groups: &mut [ScriptGroupResponse],
    execution_exempt: bool,
    result: Option<&ckbadger_common::cycles_task::TransactionCycles>,
) -> anyhow::Result<()> {
    use ckbadger_common::cycles_task::ScriptGroupKind;
    if execution_exempt {
        for group in groups {
            group.ran = false;
            group.witness_index = None;
            group.cycles = None;
        }
        return Ok(());
    }
    let Some(result) = result else {
        return Ok(());
    };
    let mut cycles = BTreeMap::new();
    for group in &result.groups {
        let kind = match group.group_type {
            ScriptGroupKind::Lock => "lock",
            ScriptGroupKind::Type => "type",
        };
        let hash = format!("0x{}", hex::encode(group.script_hash));
        anyhow::ensure!(
            cycles.insert((kind, hash.clone()), group.cycles).is_none(),
            "duplicate {kind} cycles group {hash}"
        );
    }
    for group in groups.iter_mut().filter(|group| group.ran) {
        let count = cycles
            .remove(&(group.kind.as_str(), group.script_hash.clone()))
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "canonical cycles result is missing executed {} group {}",
                    group.kind,
                    group.script_hash
                )
            })?;
        group.cycles = Some(count.to_string());
    }
    anyhow::ensure!(
        cycles.is_empty(),
        "canonical cycles result contains groups absent from transaction: {cycles:?}"
    );
    Ok(())
}

/// Override type-group `witness_index` when a Cobuild or CKBFS content witness
/// is the payload the script actually reads (beyond WitnessArgs at the first input).
pub fn attach_content_witnesses(
    groups: &mut [ScriptGroupResponse],
    witnesses: &[String],
) -> anyhow::Result<()> {
    for group in groups.iter_mut() {
        if group
            .witness_index
            .is_some_and(|index| index >= witnesses.len())
        {
            group.witness_index = None;
        }
    }
    let mut ckbfs_indices = Vec::new();
    let mut ckbfs_continuation_indices = std::collections::BTreeSet::new();
    for (index, raw) in witnesses.iter().enumerate() {
        let bytes = hex::decode(raw.trim_start_matches("0x"))?;
        if cobuild_fields(&bytes).is_some() {
            if let Some(hashes) = cobuild_action_script_hashes(&bytes) {
                for hash in hashes {
                    for group in groups.iter_mut() {
                        if group.kind == "type" && group.script_hash == hash {
                            group.witness_index = Some(index);
                        }
                    }
                }
            }
        } else if ckbfs_fields(&bytes).is_some() {
            ckbfs_indices.push(index);
            if bytes.get(5) == Some(&3) && bytes.len() >= 50 {
                let next_index =
                    u32::from_le_bytes(bytes[46..50].try_into().expect("4-byte slice")) as usize;
                if next_index != 0 {
                    ckbfs_continuation_indices.insert(next_index);
                }
            }
        }
    }
    if ckbfs_indices.is_empty() {
        return Ok(());
    }
    let head_ckbfs_indices: Vec<usize> = ckbfs_indices
        .iter()
        .copied()
        .filter(|index| !ckbfs_continuation_indices.contains(index))
        .collect();
    let assigned_indices = if head_ckbfs_indices.is_empty() {
        &ckbfs_indices
    } else {
        &head_ckbfs_indices
    };
    let mut ckbfs_groups: Vec<&mut ScriptGroupResponse> = groups
        .iter_mut()
        .filter(|group| {
            group.kind == "type" && cell_type(Some(&group.script)).ok().as_deref() == Some("ckbfs")
        })
        .collect();
    if ckbfs_groups.len() > assigned_indices.len() {
        // Output-only CKBFS groups (publishes) have no input WitnessArgs and
        // require a CKBFS content witness, whereas input+output CKBFS groups
        // may be plain ownership transfers without a CKBFS witness.
        ckbfs_groups.sort_by_key(|group| !group.input_indices.is_empty());
    }
    for (group, &index) in ckbfs_groups.iter_mut().zip(assigned_indices) {
        group.witness_index = Some(index);
    }
    Ok(())
}

fn cobuild_action_script_hashes(bytes: &[u8]) -> Option<Vec<String>> {
    let fields = cobuild_fields(bytes)?;
    let (_, start, end) = fields.iter().find(|(name, _, _)| name == "message")?;
    let message = bytes.get(*start..*end)?;
    let message_fields = molecule_fields(message, Some(1))?;
    let (actions_start, actions_end) = message_fields[0];
    let actions = message.get(actions_start..actions_end)?;
    let rows = molecule_fields(actions, None)?;
    let mut hashes = Vec::with_capacity(rows.len());
    for &(start, end) in &rows {
        let action = actions.get(start..end)?;
        let fields = molecule_fields(action, Some(3))?;
        let (hash_start, hash_end) = fields[1];
        if hash_end - hash_start != 32 {
            return None;
        }
        hashes.push(format!(
            "0x{}",
            hex::encode(action.get(hash_start..hash_end)?)
        ));
    }
    Some(hashes)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedDependencyCell {
    pub tx_hash: String,
    pub index: u32,
    pub data_hash: String,
    pub type_hash: Option<String>,
    pub script_hashes: Vec<String>,
}

fn node_cell(
    reader: &ckb_store_reader::CkbChainReader,
    outpoint: &packed::OutPoint,
) -> anyhow::Result<(packed::CellOutput, ckb_types::bytes::Bytes)> {
    let hash: [u8; 32] = outpoint.tx_hash().unpack();
    let index: u32 = outpoint.index().unpack();
    let tx = reader.get_transaction(&hash).ok_or_else(|| {
        anyhow::anyhow!(
            "dependency/input transaction 0x{} missing from CKB RocksDB",
            hex::encode(hash)
        )
    })?;
    let output = tx
        .outputs()
        .get(index as usize)
        .ok_or_else(|| anyhow::anyhow!("missing output 0x{}:{index}", hex::encode(hash)))?;
    let data = tx
        .outputs_data()
        .get(index as usize)
        .ok_or_else(|| anyhow::anyhow!("missing output data 0x{}:{index}", hex::encode(hash)))?
        .raw_data();
    Ok((output, data))
}

pub fn resolved_dependencies(
    tx: &ckb_types::core::TransactionView,
    reader: &ckb_store_reader::CkbChainReader,
) -> anyhow::Result<Vec<super::transactions::CellDepResponse>> {
    let mut scripts = BTreeMap::<String, packed::Script>::new();
    for input in tx
        .inputs()
        .into_iter()
        .filter(|input| !input.previous_output().is_null())
    {
        let (cell, _) = node_cell(reader, &input.previous_output())?;
        let lock = cell.lock();
        scripts.insert(
            format!("0x{}", hex::encode(lock.calc_script_hash().as_slice())),
            lock,
        );
        if let Some(script) = cell.type_().to_opt() {
            scripts.insert(
                format!("0x{}", hex::encode(script.calc_script_hash().as_slice())),
                script,
            );
        }
    }
    for output in tx.outputs() {
        if let Some(script) = output.type_().to_opt() {
            scripts.insert(
                format!("0x{}", hex::encode(script.calc_script_hash().as_slice())),
                script,
            );
        }
    }
    tx.cell_deps()
        .into_iter()
        .map(|dep| {
            let outpoint = dep.out_point();
            let dep_type = ckb_types::core::DepType::try_from(dep.dep_type())?;
            let outpoints = match dep_type {
                ckb_types::core::DepType::Code => vec![outpoint.clone()],
                ckb_types::core::DepType::DepGroup => {
                    let (_, data) = node_cell(reader, &outpoint)?;
                    packed::OutPointVec::from_slice(&data)
                        .map_err(|e| anyhow::anyhow!("malformed dep group: {e}"))?
                        .into_iter()
                        .collect()
                }
            };
            let resolved_cells: Vec<_> = outpoints
                .into_iter()
                .map(|point| {
                    let (cell, data) = node_cell(reader, &point)?;
                    let data_hash = packed::CellOutput::calc_data_hash(&data);
                    let type_hash = cell
                        .type_()
                        .to_opt()
                        .map(|script| script.calc_script_hash());
                    let script_hashes = scripts
                        .iter()
                        .filter_map(|(hash, script)| {
                            let matched = if script.hash_type().as_slice()[0] == 1 {
                                type_hash
                                    .as_ref()
                                    .is_some_and(|hash| *hash == script.code_hash())
                            } else {
                                data_hash == script.code_hash()
                            };
                            matched.then(|| hash.clone())
                        })
                        .collect();
                    Ok(ResolvedDependencyCell {
                        tx_hash: format!("0x{}", hex::encode(point.tx_hash().as_slice())),
                        index: point.index().unpack(),
                        data_hash: format!("0x{}", hex::encode(data_hash.as_slice())),
                        type_hash: type_hash
                            .map(|hash| format!("0x{}", hex::encode(hash.as_slice()))),
                        script_hashes,
                    })
                })
                .collect::<anyhow::Result<_>>()?;
            let script_hashes = resolved_cells
                .iter()
                .flat_map(|cell| cell.script_hashes.iter().cloned())
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .collect();
            Ok(super::transactions::CellDepResponse {
                out_point_tx_hash: format!("0x{}", hex::encode(outpoint.tx_hash().as_slice())),
                out_point_index: i32::try_from(u32::from_le_bytes(
                    outpoint.index().as_slice().try_into().expect("uint32"),
                ))?,
                dep_type: match dep_type {
                    ckb_types::core::DepType::Code => "code",
                    ckb_types::core::DepType::DepGroup => "dep_group",
                }
                .into(),
                resolved_cells,
                script_hashes,
            })
        })
        .collect()
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WitnessField {
    pub name: String,
    pub start: usize,
    pub end: usize,
    pub hex: String,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WitnessResponse {
    pub index: usize,
    pub size: usize,
    pub kind: String,
    pub fields: Vec<WitnessField>,
}

fn word(bytes: &[u8], at: usize) -> Option<usize> {
    Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?) as usize)
}

/// Verified Molecule table/dynvec field boundaries, excluding its header.
fn molecule_fields(bytes: &[u8], count: Option<usize>) -> Option<Vec<(usize, usize)>> {
    if word(bytes, 0)? != bytes.len() {
        return None;
    }
    if bytes.len() == 4 {
        return (count.is_none() || count == Some(0)).then(Vec::new);
    }
    let header = word(bytes, 4)?;
    if header < 8 || header % 4 != 0 || header > bytes.len() {
        return None;
    }
    let n = header / 4 - 1;
    if count.is_some_and(|expected| expected != n) {
        return None;
    }
    let mut offsets = (0..n)
        .map(|i| word(bytes, 4 + i * 4))
        .collect::<Option<Vec<_>>>()?;
    offsets.push(bytes.len());
    if offsets.windows(2).any(|pair| pair[0] > pair[1]) {
        return None;
    }
    Some(offsets.windows(2).map(|pair| (pair[0], pair[1])).collect())
}
fn molecule_bytes(bytes: &[u8]) -> bool {
    word(bytes, 0).is_some_and(|count| count.checked_add(4) == Some(bytes.len()))
}
fn valid_message(bytes: &[u8]) -> Option<usize> {
    let fields = molecule_fields(bytes, Some(1))?;
    let (start, end) = fields[0];
    let actions = &bytes[start..end];
    let rows = molecule_fields(actions, None)?;
    for &(start, end) in &rows {
        let action = &actions[start..end];
        let fields = molecule_fields(action, Some(3))?;
        if fields[0].1 - fields[0].0 != 32
            || fields[1].1 - fields[1].0 != 32
            || !molecule_bytes(&action[fields[2].0..fields[2].1])
        {
            return None;
        }
    }
    Some(rows.len())
}

/// CoBuild WitnessLayout (CCC / Spore on-chain): SighashAll is seal THEN message.
/// cryptape/ckb-transaction-cobuild-poc `basic.mol` once listed message first; mainnet
/// Spore witnesses and `@ckb-ccc` encode seal first. Reserved union ids alone are not a valid decode.
fn cobuild_fields(bytes: &[u8]) -> Option<Vec<(String, usize, usize)>> {
    let id = word(bytes, 0)?;
    let body = bytes.get(4..)?;
    let names: &[&str] = match id {
        0xff000001 => &["seal", "message"],
        0xff000002 => &["seal"],
        0xff000003 => &[
            "inputCells",
            "outputCells",
            "cellDeps",
            "headerDeps",
            "message",
            "seals",
        ],
        0xff000004 => &[
            "startInputCell",
            "startOutputCell",
            "startCellDeps",
            "startHeaderDeps",
        ],
        _ => return None,
    };
    let fields = molecule_fields(body, Some(names.len()))?;
    let mut result = vec![
        ("CoBuild union id".into(), 0, 4),
        ("table header".into(), 4, 4 + fields[0].0),
    ];
    for (name, &(start, end)) in names.iter().zip(&fields) {
        let data = &body[start..end];
        match *name {
            "message" => {
                valid_message(data)?;
            }
            "seal" => {
                if !molecule_bytes(data) {
                    return None;
                }
            }
            "seals" => {
                for (start, end) in molecule_fields(data, None)? {
                    let pair = &data[start..end];
                    let fields = molecule_fields(pair, Some(2))?;
                    if fields[0].1 - fields[0].0 != 32
                        || !molecule_bytes(&pair[fields[1].0..fields[1].1])
                    {
                        return None;
                    }
                }
            }
            _ => {
                if data.len() != 4 {
                    return None;
                }
            }
        }
        result.push(((*name).into(), start + 4, end + 4));
    }
    Some(result)
}

/// CKBFS head witnesses, as specified in code-monad/ckbfs RFC/RFC.v3.
fn ckbfs_fields(bytes: &[u8]) -> Option<Vec<(String, usize, usize)>> {
    if bytes.get(..5)? != b"CKBFS" {
        return None;
    }
    let mut result = vec![("CKBFS magic".into(), 0, 5), ("version".into(), 5, 6)];
    let content = match *bytes.get(5)? {
        0 | 2 => 6,
        3 if bytes.len() >= 50 => {
            result.extend([
                ("previousTxHash".into(), 6, 38),
                ("previousWitnessIndex".into(), 38, 42),
                ("previousChecksum".into(), 42, 46),
                ("nextWitnessIndex".into(), 46, 50),
            ]);
            50
        }
        _ => return None,
    };
    if bytes.len() > content {
        result.push(("content".into(), content, bytes.len()));
    }
    Some(result)
}

pub fn witness_details(witnesses: &[String]) -> anyhow::Result<Vec<WitnessResponse>> {
    witnesses
        .iter()
        .enumerate()
        .map(|(index, raw)| {
            let bytes = hex::decode(raw.trim_start_matches("0x"))?;
            let mut fields = Vec::new();
            let kind = if bytes.is_empty() {
                "empty"
            } else if let Some(decoded) = ckbfs_fields(&bytes) {
                fields.extend(decoded.into_iter().map(|(name, start, end)| WitnessField {
                    name,
                    start,
                    end,
                    hex: format!("0x{}", hex::encode(&bytes[start..end])),
                }));
                "ckbfs"
            } else if let Some(decoded) = cobuild_fields(&bytes) {
                fields.extend(decoded.into_iter().map(|(name, start, end)| WitnessField {
                    name,
                    start,
                    end,
                    hex: format!("0x{}", hex::encode(&bytes[start..end])),
                }));
                "cobuild"
            } else if packed::WitnessArgs::from_slice(&bytes).is_ok() {
                // Molecule verification above validates all table offsets and BytesOpt fields.
                fields.push(WitnessField {
                    name: "table header".into(),
                    start: 0,
                    end: 16,
                    hex: format!("0x{}", hex::encode(&bytes[..16])),
                });
                let offsets: Vec<usize> = [4, 8, 12]
                    .into_iter()
                    .map(|at| {
                        u32::from_le_bytes(bytes[at..at + 4].try_into().expect("verified table"))
                            as usize
                    })
                    .chain(std::iter::once(bytes.len()))
                    .collect();
                for (field, name) in ["lock", "inputType", "outputType"].iter().enumerate() {
                    let start = offsets[field];
                    let end = offsets[field + 1];
                    if start < end {
                        fields.push(WitnessField {
                            name: (*name).into(),
                            start,
                            end,
                            hex: format!("0x{}", hex::encode(&bytes[start..end])),
                        });
                    }
                }
                "witness_args"
            } else {
                fields.push(WitnessField {
                    name: "raw".into(),
                    start: 0,
                    end: bytes.len(),
                    hex: raw.clone(),
                });
                "opaque"
            };
            Ok(WitnessResponse {
                index,
                size: bytes.len(),
                kind: kind.into(),
                fields,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn witness_args_fields_retain_byte_ranges_and_payload() {
        let lock = packed::BytesOpt::new_builder()
            .set(Some([7u8; 65].pack()))
            .build();
        let bytes = packed::WitnessArgs::new_builder().lock(lock).build();
        let rows = witness_details(&[format!("0x{}", hex::encode(bytes.as_slice()))]).unwrap();
        assert_eq!(rows[0].kind, "witness_args");
        assert_eq!(rows[0].fields[1].name, "lock");
        assert_eq!(rows[0].fields[1].start, 16);
        assert_eq!(rows[0].fields[1].end, 85);
        assert_eq!(
            rows[0]
                .fields
                .iter()
                .map(|f| f.end - f.start)
                .sum::<usize>(),
            bytes.as_slice().len()
        );
    }
    #[test]
    fn witness_decode_preserves_unknown_data_without_guessing() {
        let rows = witness_details(&["0x".into(), "0x123456".into()]).unwrap();
        assert_eq!(rows[0].kind, "empty");
        assert_eq!(rows[1].kind, "opaque");
        assert_eq!(rows[1].fields[0].hex, "0x123456");
        assert!(witness_details(&["0xz".into()]).is_err());
    }
    fn table(fields: &[Vec<u8>]) -> Vec<u8> {
        let mut size = 4 + fields.len() * 4;
        let mut offsets = Vec::new();
        for field in fields {
            offsets.extend_from_slice(&(size as u32).to_le_bytes());
            size += field.len();
        }
        let mut bytes = (size as u32).to_le_bytes().to_vec();
        bytes.extend(offsets);
        for field in fields {
            bytes.extend(field);
        }
        bytes
    }
    #[test]
    fn cobuild_validates_nested_messages_and_preserves_exact_ranges() {
        let message = table(&[4u32.to_le_bytes().to_vec()]);
        // SighashAll on-chain / CCC order: seal, then message.
        let body = table(&[vec![0, 0, 0, 0], message.clone()]);
        let mut data = 0xff000001u32.to_le_bytes().to_vec();
        data.extend(body);
        let rows = witness_details(&[format!("0x{}", hex::encode(&data))]).unwrap();
        assert_eq!(rows[0].kind, "cobuild");
        assert_eq!(rows[0].fields[2].name, "seal");
        assert_eq!(rows[0].fields[2].hex, "0x00000000");
        assert_eq!(rows[0].fields[3].name, "message");
        assert_eq!(rows[0].fields[3].hex, format!("0x{}", hex::encode(message)));
        assert_eq!(
            rows[0]
                .fields
                .iter()
                .map(|f| f.end - f.start)
                .sum::<usize>(),
            data.len()
        );
        // Corrupt the message table (second field) so decode fails.
        let message_start = rows[0].fields[3].start;
        data[message_start + 4] = 0xff;
        assert!(
            cobuild_fields(&data).is_none(),
            "malformed message is not labelled CoBuild"
        );
    }

    #[test]
    fn cobuild_rejects_message_before_seal_layout() {
        let message = table(&[4u32.to_le_bytes().to_vec()]);
        let body = table(&[message, vec![0, 0, 0, 0]]);
        let mut data = 0xff000001u32.to_le_bytes().to_vec();
        data.extend(body);
        assert!(
            cobuild_fields(&data).is_none(),
            "message-then-seal is not the on-chain SighashAll layout"
        );
    }

    #[test]
    fn type_groups_receive_the_first_input_witness_index() {
        let script = ScriptResponse {
            code_hash: format!("0x{}", "11".repeat(32)),
            hash_type: "type".into(),
            args: "0x".into(),
        };
        let input = TransactionInputResponse {
            display: None,
            previous_output: None,
            since: "0x0".into(),
            capacity: Some("100".into()),
            used_capacity: Some("74".into()),
            lock: Some(script.clone()),
            r#type: Some(script),
            address: None,
            cell_type: Some("typed".into()),
        };
        let groups = script_groups(&[input], &[]).unwrap();
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].kind, "lock");
        assert_eq!(groups[0].witness_index, Some(0));
        assert_eq!(groups[1].kind, "type");
        assert_eq!(groups[1].witness_index, Some(0));
    }

    #[test]
    fn cobuild_content_witness_overrides_type_group_first_input_index() {
        let spore_args = vec![0x55; 32];
        let spore = ScriptResponse {
            code_hash: format!("0x{}", "22".repeat(32)),
            hash_type: "type".into(),
            args: format!("0x{}", hex::encode(&spore_args)),
        };
        let lock = ScriptResponse {
            code_hash: format!("0x{}", "11".repeat(32)),
            hash_type: "type".into(),
            args: "0x".into(),
        };
        let input = TransactionInputResponse {
            display: None,
            previous_output: None,
            since: "0x0".into(),
            capacity: Some("100".into()),
            used_capacity: Some("74".into()),
            lock: Some(lock),
            r#type: Some(spore.clone()),
            address: None,
            cell_type: Some("spore".into()),
        };
        let mut groups = script_groups(&[input], &[]).unwrap();
        let spore_hash = script_hash(&spore).unwrap();
        let action = table(&[
            vec![0x33; 32],
            hex::decode(spore_hash.trim_start_matches("0x")).unwrap(),
            {
                let mut bytes = 4u32.to_le_bytes().to_vec();
                bytes.extend(b"xfer");
                bytes
            },
        ]);
        let message = table(&[table(&[action])]);
        let body = table(&[vec![0, 0, 0, 0], message]);
        let mut cobuild = 0xff000001u32.to_le_bytes().to_vec();
        cobuild.extend(body);
        let witnesses = vec![
            "0x10000000100000001000000010000000".into(),
            format!("0x{}", hex::encode(cobuild)),
        ];
        attach_content_witnesses(&mut groups, &witnesses).unwrap();
        let type_group = groups.iter().find(|g| g.kind == "type").unwrap();
        assert_eq!(type_group.witness_index, Some(1));
        let lock_group = groups.iter().find(|g| g.kind == "lock").unwrap();
        assert_eq!(lock_group.witness_index, Some(0));
    }
    #[test]
    fn ckbfs_head_decodes_version_and_link_bytes_exactly() {
        let mut data = b"CKBFS".to_vec();
        data.push(3);
        data.extend([0u8; 44]);
        data.extend(b"hello");
        let rows = witness_details(&[format!("0x{}", hex::encode(&data))]).unwrap();
        assert_eq!(rows[0].kind, "ckbfs");
        assert_eq!(rows[0].fields.last().unwrap().start, 50);
        assert_eq!(rows[0].fields.last().unwrap().hex, "0x68656c6c6f");
        assert!(ckbfs_fields(&data[..49]).is_none());
        data[5] = 9;
        assert!(ckbfs_fields(&data).is_none());
    }
    #[test]
    fn groups_keep_executed_and_output_locks_separate_and_coalesce_types() {
        let script = ScriptResponse {
            code_hash: format!("0x{}", "11".repeat(32)),
            hash_type: "type".into(),
            args: "0x".into(),
        };
        let input = TransactionInputResponse {
            display: None,
            previous_output: None,
            since: "0x0".into(),
            capacity: Some("100".into()),
            used_capacity: Some("74".into()),
            lock: Some(script.clone()),
            r#type: Some(script.clone()),
            address: None,
            cell_type: Some("typed".into()),
        };
        let output = TransactionOutputResponse {
            display: None,
            capacity: "99".into(),
            used_capacity: "74".into(),
            virtual_used_capacity: None,
            cell_type: Some("typed".into()),
            lock: Some(script.clone()),
            r#type: Some(script),
            address: None,
            status: None,
            consumed_by_tx: None,
        };
        let mut groups = script_groups(&[input.clone(), input], &[output]).unwrap();
        assert_eq!(groups.len(), 3);
        assert_eq!(groups[0].input_indices, vec![0, 1]);
        assert_eq!(groups[0].witness_index, Some(0));
        assert_eq!(groups[1].input_indices, vec![0, 1]);
        assert_eq!(groups[1].witness_index, Some(0));
        assert_eq!(groups[1].output_indices, vec![0]);
        assert!(!groups[2].ran);
        assert_eq!(groups[2].output_indices, vec![0]);
        assert!(groups[2].witness_index.is_none());
        use ckbadger_common::cycles_task::{ScriptGroupCycles, ScriptGroupKind, TransactionCycles};
        let hash: [u8; 32] = hex::decode(groups[0].script_hash.trim_start_matches("0x"))
            .unwrap()
            .try_into()
            .unwrap();
        let result = TransactionCycles {
            block_hash: [0; 32],
            cycles: 150,
            groups: vec![
                ScriptGroupCycles {
                    script_hash: hash,
                    group_type: ScriptGroupKind::Lock,
                    cycles: 100,
                },
                ScriptGroupCycles {
                    script_hash: hash,
                    group_type: ScriptGroupKind::Type,
                    cycles: 50,
                },
            ],
        };
        apply_group_cycles(&mut groups, false, Some(&result)).unwrap();
        assert_eq!(groups[0].cycles.as_deref(), Some("100"));
        assert_eq!(groups[1].cycles.as_deref(), Some("50"));
        assert!(groups[2].cycles.is_none());
        let missing = TransactionCycles {
            groups: result.groups[..1].to_vec(),
            ..result.clone()
        };
        assert!(
            apply_group_cycles(&mut groups.clone(), false, Some(&missing))
                .unwrap_err()
                .to_string()
                .contains("missing executed type")
        );
        let mut foreign = result;
        foreign.groups[0].script_hash = [0xff; 32];
        assert!(apply_group_cycles(&mut groups.clone(), false, Some(&foreign)).is_err());
        let mut cellbase = groups.clone();
        apply_group_cycles(&mut cellbase, true, None).unwrap();
        assert!(cellbase
            .iter()
            .all(|group| !group.ran && group.witness_index.is_none() && group.cycles.is_none()));
        assert_eq!(cell_type(None).unwrap(), "plain");
        assert_eq!(cell_type(Some(&groups[0].script)).unwrap(), "typed");
    }

    #[test]
    fn ckbfs_witness_attachment_handles_chains_transfers_and_missing_witnesses() {
        let ckbfs_code_hash = "0x31e6376287d223b8c0410d562fb422f04d1d617b2947596a14c3d2efb7218d3a";
        let lock = ScriptResponse {
            code_hash: format!("0x{}", "11".repeat(32)),
            hash_type: "type".into(),
            args: "0x".into(),
        };
        let transferred_ckbfs = ScriptResponse {
            code_hash: ckbfs_code_hash.into(),
            hash_type: "data1".into(),
            args: format!("0x{}", "aa".repeat(32)),
        };
        let published_ckbfs = ScriptResponse {
            code_hash: ckbfs_code_hash.into(),
            hash_type: "data1".into(),
            args: format!("0x{}", "bb".repeat(32)),
        };
        let second_lock = ScriptResponse {
            code_hash: format!("0x{}", "22".repeat(32)),
            hash_type: "type".into(),
            args: "0x".into(),
        };
        let input0 = TransactionInputResponse {
            display: None,
            previous_output: None,
            since: "0x0".into(),
            capacity: Some("100".into()),
            used_capacity: Some("74".into()),
            lock: Some(lock.clone()),
            r#type: Some(transferred_ckbfs.clone()),
            address: None,
            cell_type: Some("ckbfs".into()),
        };
        let input1 = TransactionInputResponse {
            display: None,
            previous_output: None,
            since: "0x0".into(),
            capacity: Some("100".into()),
            used_capacity: Some("74".into()),
            lock: Some(second_lock.clone()),
            r#type: None,
            address: None,
            cell_type: Some("plain".into()),
        };
        let output0 = TransactionOutputResponse {
            display: None,
            capacity: "99".into(),
            used_capacity: "74".into(),
            virtual_used_capacity: None,
            cell_type: Some("ckbfs".into()),
            lock: Some(lock.clone()),
            r#type: Some(transferred_ckbfs.clone()),
            address: None,
            status: None,
            consumed_by_tx: None,
        };
        let output1 = TransactionOutputResponse {
            display: None,
            capacity: "99".into(),
            used_capacity: "74".into(),
            virtual_used_capacity: None,
            cell_type: Some("ckbfs".into()),
            lock: Some(lock),
            r#type: Some(published_ckbfs.clone()),
            address: None,
            status: None,
            consumed_by_tx: None,
        };
        let mut groups = script_groups(&[input0, input1], &[output0, output1]).unwrap();
        // Empty witness list clears out-of-bounds default input witness indices.
        let mut no_witness_groups = groups.clone();
        attach_content_witnesses(&mut no_witness_groups, &[]).unwrap();
        assert!(no_witness_groups.iter().all(|g| g.witness_index.is_none()));

        // Witness 1 is a v3 CKBFS head witness chaining to continuation witness 2.
        let mut head = b"CKBFS".to_vec();
        head.push(3);
        head.extend([0u8; 40]);
        head.extend(2u32.to_le_bytes()); // nextWitnessIndex = 2
        head.extend(b"part1");
        let mut cont = b"CKBFS".to_vec();
        cont.push(3);
        cont.extend([0u8; 44]);
        cont.extend(b"part2");
        let witnesses = vec![
            "0x10000000100000001000000010000000".into(),
            format!("0x{}", hex::encode(&head)),
            format!("0x{}", hex::encode(&cont)),
        ];
        attach_content_witnesses(&mut groups, &witnesses).unwrap();
        let transfer_hash = script_hash(&transferred_ckbfs).unwrap();
        let publish_hash = script_hash(&published_ckbfs).unwrap();
        let transfer_group = groups
            .iter()
            .find(|g| g.kind == "type" && g.script_hash == transfer_hash)
            .unwrap();
        let publish_group = groups
            .iter()
            .find(|g| g.kind == "type" && g.script_hash == publish_hash)
            .unwrap();
        assert_eq!(publish_group.witness_index, Some(1));
        assert_eq!(transfer_group.witness_index, Some(0));
    }
}
