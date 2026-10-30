//! Exact transaction script membership and witness byte views.
use super::transactions::{TransactionInputResponse, TransactionOutputResponse};
use crate::response::ScriptResponse;
use ckb_types::{packed, prelude::*};
use ckbadger_indexer::parser::registry::{ProtocolScript, PROTOCOL_REGISTRY};
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
            // A type group without inputs reads WitnessArgs through
            // `Source::GroupOutput`: the witness at its first output index.
            // Every input type is added before any output, so an empty input
            // list here is final. A missing witness is cleared, and a
            // content witness overrides it, in `attach_content_witnesses`.
            if group.kind == "type"
                && group.input_indices.is_empty()
                && group.witness_index.is_none()
            {
                group.witness_index = Some(index);
            }
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
/// is the payload the script actually reads (beyond the group's WitnessArgs).
///
/// A CKBFS group reads the content witness its own output cell data names
/// (the data's first field; the head index of a multi-witness file). That
/// index is the attribution — never the order CKBFS witnesses appear in.
/// `outputs_data` is the transaction's output data, aligned with its outputs.
pub fn attach_content_witnesses<D: AsRef<[u8]>>(
    groups: &mut [ScriptGroupResponse],
    witnesses: &[String],
    outputs_data: &[D],
) -> anyhow::Result<()> {
    for group in groups.iter_mut() {
        if group
            .witness_index
            .is_some_and(|index| index >= witnesses.len())
        {
            group.witness_index = None;
        }
    }
    let mut ckbfs_witnesses = Vec::with_capacity(witnesses.len());
    for (index, raw) in witnesses.iter().enumerate() {
        let bytes = hex::decode(raw.trim_start_matches("0x"))?;
        ckbfs_witnesses.push(ckbfs_fields(&bytes).is_some());
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
        }
    }
    for group in groups.iter_mut() {
        if group.kind != "type" {
            continue;
        }
        let protocol = cell_type(Some(&group.script))
            .map_err(|(_, error)| anyhow::anyhow!(error.0.message.clone()))?;
        if protocol != "ckbfs" {
            continue;
        }
        // A consumed CKBFS cell without a successor declares no new content;
        // its group reads WitnessArgs at its first input like any type group.
        let Some(&output) = group.output_indices.first() else {
            continue;
        };
        let data = outputs_data.get(output).ok_or_else(|| {
            anyhow::anyhow!(
                "CKBFS output {output} has no output data ({} data entries)",
                outputs_data.len()
            )
        })?;
        let index = super::transaction_cells::ckbfs_head_witness_index(data.as_ref())
            .map_err(|e| anyhow::anyhow!("CKBFS output {output} cell data: {e:#}"))?;
        anyhow::ensure!(
            ckbfs_witnesses.get(index) == Some(&true),
            "CKBFS output {output} names witness {index}, which is not a CKBFS witness \
             ({} witnesses)",
            witnesses.len()
        );
        group.witness_index = Some(index);
    }
    Ok(())
}

fn cobuild_action_script_hashes(bytes: &[u8]) -> Option<Vec<String>> {
    Some(
        cobuild_actions(bytes)?
            .into_iter()
            .map(|action| action.script_hash)
            .collect(),
    )
}

/// One CoBuild `Action` of a witness's message: `script_info_hash`,
/// `script_hash` and its raw `data` (the action itself, in the script's own
/// schema; not decoded here).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CobuildActionResponse {
    pub script_info_hash: String,
    pub script_hash: String,
    pub data: String,
    /// The action schema `data` decodes under, when one is known for the
    /// script it is for (TX-36): `"spore"` (spore-contract `SporeAction`).
    /// Absent for any other script — the schema is unknown.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
    /// The action's name in that schema, e.g. `"TransferSpore"`; present
    /// exactly when `schema` is.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// spore-contract `action.mol` `union SporeAction`, in item-id order: each
/// item's name and its table's field count.
const SPORE_ACTIONS: [(&str, usize); 11] = [
    ("MintSpore", 3),
    ("TransferSpore", 3),
    ("BurnSpore", 2),
    ("MintCluster", 3),
    ("TransferCluster", 3),
    ("MintProxy", 3),
    ("TransferProxy", 4),
    ("BurnProxy", 3),
    ("MintAgent", 3),
    ("TransferAgent", 3),
    ("BurnAgent", 2),
];

/// The `SporeAction` name `data` encodes for a script of `family` — the
/// Spore type (items 0..=2) or the Cluster type (items 3..=4) — when `data`
/// is that union with a well-formed item table; `None` otherwise.
fn spore_action_name(family: ProtocolScript, data: &[u8]) -> Option<&'static str> {
    let item = word(data, 0)?;
    let allowed = match family {
        ProtocolScript::SporeNft => 0..=2,
        ProtocolScript::Cluster => 3..=4,
        _ => return None,
    };
    if !allowed.contains(&item) {
        return None;
    }
    let (name, fields) = SPORE_ACTIONS[item];
    molecule_fields(data.get(4..)?, Some(fields))?;
    Some(name)
}

/// Name the CoBuild actions whose schema is known (TX-36). An action is for
/// the script whose hash it carries; when that is a type script of this
/// transaction's inputs or outputs registered as a Spore or Cluster, its data
/// is decoded as `SporeAction`. Every other action keeps only its hashes and
/// raw data: no schema registry exists for it.
pub fn name_cobuild_actions(
    witnesses: &mut [WitnessResponse],
    inputs: &[TransactionInputResponse],
    outputs: &[TransactionOutputResponse],
) -> anyhow::Result<()> {
    let mut families = BTreeMap::new();
    for script in inputs
        .iter()
        .filter_map(|input| input.r#type.as_ref())
        .chain(outputs.iter().filter_map(|output| output.r#type.as_ref()))
    {
        let code_hash = hex::decode(script.code_hash.trim_start_matches("0x"))?;
        if let Some(family) = PROTOCOL_REGISTRY.get(&code_hash) {
            families.insert(script_hash(script)?, family);
        }
    }
    for action in witnesses
        .iter_mut()
        .filter_map(|witness| witness.actions.as_mut())
        .flatten()
    {
        let Some(family) = families.get(&action.script_hash) else {
            continue;
        };
        let data = hex::decode(action.data.trim_start_matches("0x"))?;
        if let Some(name) = spore_action_name(*family, &data) {
            action.schema = Some("spore".to_string());
            action.name = Some(name.to_string());
        }
    }
    Ok(())
}

/// The actions of a CoBuild witness's message (`Message { actions:
/// ActionVec }`, `Action { script_info_hash, script_hash, data: Bytes }`),
/// `None` when the witness has no message or a malformed one.
fn cobuild_actions(bytes: &[u8]) -> Option<Vec<CobuildActionResponse>> {
    let fields = cobuild_fields(bytes)?;
    let (_, start, end) = fields.iter().find(|(name, _, _)| name == "message")?;
    let message = bytes.get(*start..*end)?;
    let message_fields = molecule_fields(message, Some(1))?;
    let (actions_start, actions_end) = message_fields[0];
    let actions = message.get(actions_start..actions_end)?;
    let rows = molecule_fields(actions, None)?;
    let mut out = Vec::with_capacity(rows.len());
    for &(start, end) in &rows {
        let action = actions.get(start..end)?;
        let fields = molecule_fields(action, Some(3))?;
        let hash = |(start, end): (usize, usize)| {
            (end - start == 32).then(|| format!("0x{}", hex::encode(&action[start..end])))
        };
        let (data_start, data_end) = fields[2];
        let data = action.get(data_start..data_end)?;
        if !molecule_bytes(data) {
            return None;
        }
        out.push(CobuildActionResponse {
            script_info_hash: hash(fields[0])?,
            script_hash: hash(fields[1])?,
            data: format!("0x{}", hex::encode(&data[4..])),
            schema: None,
            name: None,
        });
    }
    Some(out)
}

/// A cellbase's witness, `CellbaseWitness { lock: Script, message: Bytes }`:
/// the miner's lock and the client message it signed the block with.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CellbaseWitnessResponse {
    pub lock: ScriptResponse,
    pub message: String,
}

/// Decode `bytes` as a `CellbaseWitness`; `None` when it is not one.
pub fn cellbase_witness(bytes: &[u8]) -> Option<CellbaseWitnessResponse> {
    let witness = packed::CellbaseWitness::from_slice(bytes).ok()?;
    let lock = witness.lock();
    let hash_type: u8 = lock.hash_type().into();
    Some(CellbaseWitnessResponse {
        lock: ScriptResponse {
            code_hash: format!("0x{}", hex::encode(lock.code_hash().raw_data())),
            hash_type: ckbadger_common::hash_type_label(hash_type)?.to_string(),
            args: format!("0x{}", hex::encode(lock.args().raw_data())),
        },
        message: format!("0x{}", hex::encode(witness.message().raw_data())),
    })
}

/// The text of a cellbase message, decided once (BA-17).
///
/// The CKB node's block assembler writes the message as `versionbits (u32
/// LE) + " "` — when it computes versionbits, as every node since the 2021
/// hardfork does — then the client's binary version and its configured
/// message. Consensus reads the first four bytes as that version word, and
/// accepts it as one only when its top three bits are `000`
/// (`VERSIONBITS_TOP_MASK`), i.e. when the fourth byte is below `0x20`. A
/// printable character there is always `>= 0x20`, so the prefix is exactly:
/// at least five bytes, byte 3 below `0x20`, byte 4 a space. The text is the
/// rest of the message when it is UTF-8 without control characters; any other
/// message (a pool's binary extra-nonce, say) has no text and `None` says so.
pub(crate) fn cellbase_message_text(message: &[u8]) -> Option<String> {
    let payload = match message {
        [_, _, _, version_top, b' ', rest @ ..] if *version_top < 0x20 => rest,
        _ => message,
    };
    let text = std::str::from_utf8(payload).ok()?;
    (!text.is_empty() && !text.chars().any(char::is_control)).then(|| text.to_string())
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedDependencyCell {
    pub tx_hash: String,
    pub index: u32,
    pub data_hash: String,
    pub type_hash: Option<String>,
    /// Scripts that ran in this transaction (input locks, input and output
    /// types) whose code this cell supplies.
    pub script_hashes: Vec<String>,
    /// Output locks whose code this cell supplies: attached here, run only
    /// when that output is spent — "not run here" (TX-14). A lock that is
    /// also an input lock ran, and is in `scriptHashes` instead.
    pub output_lock_script_hashes: Vec<String>,
    /// The cell's data length in bytes: the code's size (TX-15).
    pub code_size: usize,
    /// The script registry's names of the references this cell supplies in
    /// this transaction (run or attached), distinct and sorted; empty when it
    /// supplies none or none is registered (TX-15).
    pub script_names: Vec<String>,
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
    store: &ckbadger_store::CkbadgerStore,
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
    // Output locks do not run in this transaction; one that is also a script
    // that ran is listed only there.
    let output_locks: BTreeMap<String, packed::Script> = tx
        .outputs()
        .into_iter()
        .map(|output| {
            let lock = output.lock();
            (
                format!("0x{}", hex::encode(lock.calc_script_hash().as_slice())),
                lock,
            )
        })
        .filter(|(hash, _)| !scripts.contains_key(hash))
        .collect();
    let supplies = |script: &packed::Script,
                    data_hash: &packed::Byte32,
                    type_hash: Option<&packed::Byte32>| {
        if script.hash_type().as_slice()[0] == 1 {
            type_hash.is_some_and(|hash| *hash == script.code_hash())
        } else {
            *data_hash == script.code_hash()
        }
    };
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
                    let matching = |set: &BTreeMap<String, packed::Script>| -> Vec<String> {
                        set.iter()
                            .filter(|(_, script)| supplies(script, &data_hash, type_hash.as_ref()))
                            .map(|(hash, _)| hash.clone())
                            .collect()
                    };
                    let script_hashes = matching(&scripts);
                    let output_lock_script_hashes = matching(&output_locks);
                    let mut script_names = std::collections::BTreeSet::new();
                    for hash in script_hashes.iter().chain(&output_lock_script_hashes) {
                        let script = scripts
                            .get(hash)
                            .or_else(|| output_locks.get(hash))
                            .expect("matched hash is a key of its set");
                        if let Some(name) = crate::utils::script_resolution::registered_script_name(
                            store,
                            script.code_hash().as_slice(),
                        )? {
                            script_names.insert(name);
                        }
                    }
                    Ok(ResolvedDependencyCell {
                        tx_hash: format!("0x{}", hex::encode(point.tx_hash().as_slice())),
                        index: point.index().unpack(),
                        data_hash: format!("0x{}", hex::encode(data_hash.as_slice())),
                        type_hash: type_hash
                            .map(|hash| format!("0x{}", hex::encode(hash.as_slice()))),
                        script_hashes,
                        output_lock_script_hashes,
                        code_size: data.len(),
                        script_names: script_names.into_iter().collect(),
                    })
                })
                .collect::<anyhow::Result<_>>()?;
            let script_hashes = resolved_cells
                .iter()
                .flat_map(|cell| cell.script_hashes.iter().cloned())
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .collect();
            let output_lock_script_hashes = resolved_cells
                .iter()
                .flat_map(|cell| cell.output_lock_script_hashes.iter().cloned())
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
                output_lock_script_hashes,
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
    /// `cobuild` witnesses with a message: its actions, in message order.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actions: Option<Vec<CobuildActionResponse>>,
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

/// Every witness's byte layout. `is_cellbase` makes witness 0 a
/// `CellbaseWitness` candidate (kind `cellbase`: lock, message).
pub fn witness_details(
    witnesses: &[String],
    is_cellbase: bool,
) -> anyhow::Result<Vec<WitnessResponse>> {
    witnesses
        .iter()
        .enumerate()
        .map(|(index, raw)| {
            let bytes = hex::decode(raw.trim_start_matches("0x"))?;
            let mut fields = Vec::new();
            let mut actions = None;
            let cellbase = (is_cellbase && index == 0)
                .then(|| packed::CellbaseWitness::from_slice(&bytes).ok())
                .flatten();
            let kind = if bytes.is_empty() {
                "empty"
            } else if let Some(witness) = cellbase {
                let lock = witness.lock();
                let lock_start = 12;
                let lock_end = lock_start + lock.as_slice().len();
                for (name, start, end) in [
                    ("table header", 0, lock_start),
                    ("lock", lock_start, lock_end),
                    ("message", lock_end, bytes.len()),
                ] {
                    fields.push(WitnessField {
                        name: name.into(),
                        start,
                        end,
                        hex: format!("0x{}", hex::encode(&bytes[start..end])),
                    });
                }
                "cellbase"
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
                actions = cobuild_actions(&bytes);
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
                actions,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cellbase_message_text_strips_the_versionbits_prefix_only() {
        // The node's own format: versionbits 0 + " " + binary version.
        let mut message = vec![0, 0, 0, 0, b' '];
        message.extend_from_slice(b"0.210.0 (2592ddf 2026-09-17)");
        assert_eq!(
            cellbase_message_text(&message).as_deref(),
            Some("0.210.0 (2592ddf 2026-09-17)")
        );
        // A signalling version word (bit 1 set) is still the prefix.
        let mut signalling = vec![2, 0, 0, 0, b' '];
        signalling.extend_from_slice(b"0.119.0");
        assert_eq!(
            cellbase_message_text(&signalling).as_deref(),
            Some("0.119.0")
        );
        // Text from the first byte (no versionbits) is kept whole.
        assert_eq!(
            cellbase_message_text(b"ckb 0.210.0").as_deref(),
            Some("ckb 0.210.0")
        );
        // Trailing binary bytes, a bare prefix, or no bytes: no text.
        let mut binary = b"0.210.0 (860c9bb 2026-09-21) ".to_vec();
        binary.extend_from_slice(&[0xff, 0, 1, 2, 3, 4, 5, 6]);
        assert_eq!(cellbase_message_text(&binary), None);
        assert_eq!(cellbase_message_text(&[0, 0, 0, 0, b' ']), None);
        assert_eq!(cellbase_message_text(&[]), None);
    }

    #[test]
    fn witness_args_fields_retain_byte_ranges_and_payload() {
        let lock = packed::BytesOpt::new_builder()
            .set(Some([7u8; 65].pack()))
            .build();
        let bytes = packed::WitnessArgs::new_builder().lock(lock).build();
        let rows =
            witness_details(&[format!("0x{}", hex::encode(bytes.as_slice()))], false).unwrap();
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
        let rows = witness_details(&["0x".into(), "0x123456".into()], false).unwrap();
        assert_eq!(rows[0].kind, "empty");
        assert_eq!(rows[1].kind, "opaque");
        assert_eq!(rows[1].fields[0].hex, "0x123456");
        assert!(witness_details(&["0xz".into()], false).is_err());
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
        let rows = witness_details(&[format!("0x{}", hex::encode(&data))], false).unwrap();
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

    /// TX-36: a CoBuild witness lists the actions of its message — each
    /// action's script info hash, script hash and raw action data — as the
    /// existing CoBuild parser validates them.
    #[test]
    fn cobuild_witness_lists_its_message_actions() {
        let bytes = |b: &[u8]| {
            let mut v = (b.len() as u32).to_le_bytes().to_vec();
            v.extend(b);
            v
        };
        let action = table(&[vec![0x11; 32], vec![0x30; 32], bytes(&[1, 0, 0, 0, 0xaa])]);
        let message = table(&[table(&[action])]);
        let body = table(&[vec![0, 0, 0, 0], message]);
        let mut data = 0xff000001u32.to_le_bytes().to_vec();
        data.extend(body);
        let rows = witness_details(&[format!("0x{}", hex::encode(&data))], false).unwrap();
        assert_eq!(rows[0].kind, "cobuild");
        assert_eq!(
            serde_json::to_value(&rows[0].actions).unwrap(),
            serde_json::json!([{
                "scriptInfoHash": format!("0x{}", "11".repeat(32)),
                "scriptHash": format!("0x{}", "30".repeat(32)),
                "data": "0x01000000aa",
            }])
        );
        // Witnesses that are not CoBuild carry no action list.
        let rows = witness_details(&["0x".into()], false).unwrap();
        assert!(rows[0].actions.is_none());
    }

    #[test]
    fn spore_actions_decode_only_for_their_own_script_family() {
        let script = table(&[vec![0x22; 32], vec![1], vec![0, 0, 0, 0]]);
        let address = [0u32.to_le_bytes().to_vec(), script].concat();
        let union =
            |item: u32, fields: &[Vec<u8>]| [item.to_le_bytes().to_vec(), table(fields)].concat();
        let transfer = union(1, &[vec![0x41; 32], address.clone(), address.clone()]);
        assert_eq!(
            spore_action_name(ProtocolScript::SporeNft, &transfer),
            Some("TransferSpore")
        );
        // A Spore item is not a Cluster action, and vice versa.
        assert_eq!(spore_action_name(ProtocolScript::Cluster, &transfer), None);
        let mint_cluster = union(3, &[vec![0x41; 32], address.clone(), vec![0x55; 32]]);
        assert_eq!(
            spore_action_name(ProtocolScript::Cluster, &mint_cluster),
            Some("MintCluster")
        );
        // A wrong field count or an unknown item is not a SporeAction.
        let burn_with_three = union(2, &[vec![0x41; 32], address.clone(), address]);
        assert_eq!(
            spore_action_name(ProtocolScript::SporeNft, &burn_with_three),
            None
        );
        assert_eq!(
            spore_action_name(ProtocolScript::SporeNft, &[11, 0, 0, 0]),
            None
        );
        assert_eq!(spore_action_name(ProtocolScript::Sudt, &transfer), None);
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
            lock_args_decoded: None,
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
            lock_args_decoded: None,
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
        attach_content_witnesses(&mut groups, &witnesses, &[] as &[&[u8]]).unwrap();
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
        let rows = witness_details(&[format!("0x{}", hex::encode(&data))], false).unwrap();
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
            lock_args_decoded: None,
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
            is_change: false,
            lock_args_decoded: None,
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

    fn script(code_hash: &str, hash_type: &str, args: &str) -> ScriptResponse {
        ScriptResponse {
            code_hash: code_hash.into(),
            hash_type: hash_type.into(),
            args: args.into(),
        }
    }
    fn input(lock: &ScriptResponse, r#type: Option<&ScriptResponse>) -> TransactionInputResponse {
        TransactionInputResponse {
            lock_args_decoded: None,
            display: None,
            previous_output: None,
            since: "0x0".into(),
            capacity: Some("100".into()),
            used_capacity: Some("74".into()),
            lock: Some(lock.clone()),
            r#type: r#type.cloned(),
            address: None,
            cell_type: None,
        }
    }
    fn output(lock: &ScriptResponse, r#type: Option<&ScriptResponse>) -> TransactionOutputResponse {
        TransactionOutputResponse {
            display: None,
            capacity: "99".into(),
            used_capacity: "74".into(),
            virtual_used_capacity: None,
            cell_type: None,
            lock: Some(lock.clone()),
            r#type: r#type.cloned(),
            address: None,
            status: None,
            consumed_by_tx: None,
            is_change: false,
            lock_args_decoded: None,
        }
    }
    fn hex_bytes(raw: &str) -> Vec<u8> {
        hex::decode(raw.trim_start_matches("0x")).unwrap()
    }
    fn type_group_witness(
        groups: &[ScriptGroupResponse],
        script: &ScriptResponse,
    ) -> Option<usize> {
        let hash = script_hash(script).unwrap();
        groups
            .iter()
            .find(|g| g.kind == "type" && g.script_hash == hash)
            .unwrap()
            .witness_index
    }
    fn ckbfs_content_witness(content: &[u8]) -> String {
        let mut bytes = b"CKBFS".to_vec();
        bytes.push(0);
        bytes.extend(content);
        format!("0x{}", hex::encode(bytes))
    }
    /// 5-field CKBFS cell data whose index fixvec names `indexes`.
    fn ckbfs_v2_data(indexes: &[u32]) -> Vec<u8> {
        let mut index = (indexes.len() as u32).to_le_bytes().to_vec();
        for i in indexes {
            index.extend(i.to_le_bytes());
        }
        table(&[
            index,
            42u32.to_le_bytes().to_vec(),
            [4u32.to_le_bytes().to_vec(), b"text".to_vec()].concat(),
            [5u32.to_le_bytes().to_vec(), b"a.txt".to_vec()].concat(),
            4u32.to_le_bytes().to_vec(),
        ])
    }
    const CKBFS_V2: &str = "0x31e6376287d223b8c0410d562fb422f04d1d617b2947596a14c3d2efb7218d3a";
    const CKBFS_V3: &str = "0xb5d13ffe0547c78021c01fe24dce2e959a1ed8edbca3cb93dd2e9f57fb56d695";
    const SECP: &str = "0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8";

    /// Review #8: two published files whose cells name witnesses [3] and [1].
    /// Pairing CKBFS groups with CKBFS witnesses by order swapped them.
    #[test]
    fn ckbfs_groups_read_the_witness_their_own_cell_data_names() {
        let lock = script(SECP, "type", &format!("0x{}", "11".repeat(20)));
        let first = script(CKBFS_V2, "data1", &format!("0x{}", "aa".repeat(32)));
        let second = script(CKBFS_V2, "data1", &format!("0x{}", "bb".repeat(32)));
        let mut groups = script_groups(
            &[input(&lock, None)],
            &[output(&lock, Some(&first)), output(&lock, Some(&second))],
        )
        .unwrap();
        let witnesses = vec![
            "0x10000000100000001000000010000000".to_string(),
            ckbfs_content_witness(b"second file"),
            "0x".to_string(),
            ckbfs_content_witness(b"first file"),
        ];
        let data = [ckbfs_v2_data(&[3]), ckbfs_v2_data(&[1])];
        attach_content_witnesses(&mut groups, &witnesses, &data).unwrap();
        assert_eq!(type_group_witness(&groups, &first), Some(3));
        assert_eq!(type_group_witness(&groups, &second), Some(1));
        let lock_group = groups.iter().find(|g| g.kind == "lock").unwrap();
        assert_eq!(lock_group.witness_index, Some(0));
    }

    /// A v3 head chaining to a continuation witness, beside a transfer whose
    /// own witness comes later: each group follows its own declared index and
    /// the continuation is attributed to no group.
    #[test]
    fn ckbfs_attribution_ignores_witness_order_and_chain_links() {
        let lock = script(SECP, "type", &format!("0x{}", "11".repeat(20)));
        let transferred = script(CKBFS_V3, "data1", &format!("0x{}", "aa".repeat(32)));
        let published = script(CKBFS_V3, "data1", &format!("0x{}", "bb".repeat(32)));
        let mut groups = script_groups(
            &[input(&lock, Some(&transferred))],
            &[
                output(&lock, Some(&transferred)),
                output(&lock, Some(&published)),
            ],
        )
        .unwrap();
        let v3 = |next: u32, content: &[u8]| {
            let mut bytes = b"CKBFS".to_vec();
            bytes.push(3);
            bytes.extend([0u8; 40]);
            bytes.extend(next.to_le_bytes());
            bytes.extend(content);
            format!("0x{}", hex::encode(bytes))
        };
        let witnesses = vec![
            "0x10000000100000001000000010000000".to_string(),
            v3(2, b"part1"),
            v3(0, b"part2"),
            v3(0, b"TRANSFER"),
        ];
        let v3_data = |index: u32| {
            table(&[
                index.to_le_bytes().to_vec(),
                42u32.to_le_bytes().to_vec(),
                [4u32.to_le_bytes().to_vec(), b"text".to_vec()].concat(),
                [5u32.to_le_bytes().to_vec(), b"a.txt".to_vec()].concat(),
            ])
        };
        let data = [v3_data(3), v3_data(1)];
        attach_content_witnesses(&mut groups, &witnesses, &data).unwrap();
        assert_eq!(type_group_witness(&groups, &transferred), Some(3));
        assert_eq!(type_group_witness(&groups, &published), Some(1));
        assert!(groups.iter().all(|g| g.witness_index != Some(2)));
    }

    #[test]
    fn ckbfs_attribution_rejects_unreadable_or_mismatched_indexes() {
        let lock = script(SECP, "type", &format!("0x{}", "11".repeat(20)));
        let file = script(CKBFS_V2, "data1", &format!("0x{}", "aa".repeat(32)));
        let groups = script_groups(&[input(&lock, None)], &[output(&lock, Some(&file))]).unwrap();
        let witnesses = vec![
            "0x10000000100000001000000010000000".to_string(),
            ckbfs_content_witness(b"file"),
        ];
        let error = |data: Vec<u8>, witnesses: &[String]| {
            attach_content_witnesses(&mut groups.clone(), witnesses, &[data])
                .unwrap_err()
                .to_string()
        };
        // Malformed molecule.
        assert!(error(vec![1, 2, 3], &witnesses).contains("CKBFS output 0 cell data"));
        // A fixvec whose count disagrees with its bytes, and an empty fixvec.
        let mut bad = ckbfs_v2_data(&[1]);
        bad[24] = 2;
        assert!(error(bad, &witnesses).contains("declares 2 entries"));
        assert!(error(ckbfs_v2_data(&[]), &witnesses).contains("names no witness"));
        // The named witness is WitnessArgs, or does not exist.
        assert!(error(ckbfs_v2_data(&[0]), &witnesses).contains("names witness 0"));
        assert!(error(ckbfs_v2_data(&[5]), &witnesses).contains("names witness 5"));
        assert!(error(ckbfs_v2_data(&[1]), &[]).contains("names witness 1"));
    }

    #[test]
    fn consumed_ckbfs_without_successor_keeps_its_first_input_witness_args() {
        let lock = script(SECP, "type", &format!("0x{}", "11".repeat(20)));
        let file = script(CKBFS_V3, "data1", &format!("0x{}", "aa".repeat(32)));
        let mut groups =
            script_groups(&[input(&lock, Some(&file))], &[output(&lock, None)]).unwrap();
        let witnesses = vec!["0x10000000100000001000000010000000".to_string()];
        attach_content_witnesses(&mut groups, &witnesses, &[Vec::<u8>::new()]).unwrap();
        assert_eq!(type_group_witness(&groups, &file), Some(0));
        // Without witnesses the default WitnessArgs index is cleared.
        attach_content_witnesses(&mut groups, &[], &[Vec::<u8>::new()]).unwrap();
        assert!(groups.iter().all(|g| g.witness_index.is_none()));
    }

    /// Mainnet tx 0xfcf43898…4fb47dab: a 4-field CKBFS v3 transfer (index 1)
    /// beside a Spore transfer whose CoBuild witness is 2.
    #[test]
    fn mainnet_ckbfs_v3_transfer_beside_cobuild_spore() {
        let secp = script(SECP, "type", "0x55c5d42e9d8b708f0d099a4139e361b4c273a0b9");
        let ckbfs = script(
            CKBFS_V3,
            "data1",
            "0xfd9e85a3542c681c95c8178fed1727de087f78c09fb64f45c55a59e4b46d2193",
        );
        let spore = script(
            "0x4a4dce1df3dffff7f8b2cd7dff7303df3b6150c9788cb75dcf6747247132b9f5",
            "data1",
            "0x3b97fb30a0a5c02d6abc66d478ec33cdd864702282d7651ed9dc9c570336a240",
        );
        let owner = script(
            "0x43b292bb8345189a1eb3f80330fbc2c75c547ab0627ddd26e7ca27ce4925895d",
            "type",
            "0x03240f480572de2a923b701e09636dcf0fae3dfa2af6cde008dcc6bb5a025970f4",
        );
        let spore_owner = script(
            "0x6361d4b20d845953d9c9431bbba08905573005a71e2a2432e7e0e7c685666f24",
            "data1",
            "0x012c785eac8e2e741828ecdcec3826f01b31260a12496e6e28af556e6ffbb15c15",
        );
        let mut groups = script_groups(
            &[
                input(&secp, Some(&ckbfs)),
                input(&secp, None),
                input(&secp, Some(&spore)),
            ],
            &[
                output(&owner, Some(&ckbfs)),
                output(&spore_owner, Some(&spore)),
                output(&secp, None),
            ],
        )
        .unwrap();
        let data = [
            hex_bytes("0x4500000014000000180000001c0000002a000000010000001e62cd580a000000746578742f706c61696e170000003358303132354e4d4e5641503033333430312e4c4f4753"),
            hex_bytes("0xb500000010000000190000009100000005000000646f622f30740000007b22646e61223a2233636363343566306165363139306138343332343936363161356534356165656330326164363633653335356263343638376237613461653137356130336435303333333538333033313332333534653464346535363431353033303333333333343330333130303030227d2000000026a1fe3f4e84ae2cd1dc3e319ca416e27624c5ebe2f1b0aa731ca18d67784c4c"),
            Vec::new(),
        ];
        let witnesses = vec![
            "0x5500000010000000550000005500000041000000673bacf254cd8f7a0ad1f67a76991e94383b6ef7be883c9003dbc694174468046eb42707cf7734894df30ec4cd2327b576928c8d4cc1f5e9b70ab04295d64fdd01".to_string(),
            "0x434b424653035c42e55ba7a52d5f2c4d1d3ca3d169c6ccd2bc5369dd322ffd955c8586548ebe01000000c9140b2f000000005452414e534645522c46524f4d3a30783033333633613136613464343733396430666339613564363766353837396163613738363235346237396466383632626235393862306531383063613238613737662c544f3a30783033323430663438303537326465326139323362373031653039363336646366306661653364666132616636636465303038646363366262356130323539373066340a434c41494d2c46524f4d3a30783033303030303030303030303030303030303030303030303030303030303030303030303030303030303030303030303030303030303030303030303030303030302c544f3a30783033353235343764633835363463383565306165656637653236346330346237313737383832643335656636656538353834633839383234623436363431383538340a".to_string(),
            "0x010000ff4f0100000c00000010000000000000003f0100000800000037010000080000002f01000010000000300000005000000023cde495cfd125019140ea933e76e3cbf16189483232ab5a05d88fb34d1023e1dd5179dc214b6d6c022bfd6e8e80bfe240ec4d5ac7c6228755302104a1927c73db00000001000000d700000010000000300000007d0000003b97fb30a0a5c02d6abc66d478ec33cdd864702282d7651ed9dc9c570336a24000000000490000001000000030000000310000009bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8011400000055c5d42e9d8b708f0d099a4139e361b4c273a0b900000000560000001000000030000000310000006361d4b20d845953d9c9431bbba08905573005a71e2a2432e7e0e7c685666f240221000000012c785eac8e2e741828ecdcec3826f01b31260a12496e6e28af556e6ffbb15c15".to_string(),
        ];
        attach_content_witnesses(&mut groups, &witnesses, &data).unwrap();
        assert_eq!(type_group_witness(&groups, &ckbfs), Some(1));
        assert_eq!(type_group_witness(&groups, &spore), Some(2));
        let rows = witness_details(&witnesses, false).unwrap();
        assert_eq!(rows[1].kind, "ckbfs");
        assert_eq!(rows[2].kind, "cobuild");
    }

    /// Mainnet tx 0x35df73ff…7c5acc5: the v3 file's cell names witness 0, which
    /// is a CKBFS head; the secp input's WitnessArgs is witness 1.
    #[test]
    fn mainnet_ckbfs_v3_content_at_witness_zero() {
        let owner = script(
            "0x43b292bb8345189a1eb3f80330fbc2c75c547ab0627ddd26e7ca27ce4925895d",
            "type",
            "0x03ce623219c147d8271d738c3292ff91eb201c3971ea01d0fea49babaf0d1f78b4",
        );
        let ckbfs = script(
            CKBFS_V3,
            "data1",
            "0xfb934bec5d638acbf8406b3a945c10321422319281d8b8ac23b5916fe25d6d35",
        );
        let payer = script(SECP, "type", "0x1c9f58f2445f805d97fc92378e6b777aa6419e86");
        let change = script(
            "0xd00c84f0ec8fd441c38bc3f87a371f547190f2fcff88e642bc5bf54b9e318323",
            "type",
            "0x0001d499102af334e2f44d3fa563d27a2db1af45de7e",
        );
        let mut groups = script_groups(
            &[input(&owner, Some(&ckbfs)), input(&payer, None)],
            &[output(&owner, Some(&ckbfs)), output(&change, None)],
        )
        .unwrap();
        let data = [
            hex_bytes("0x4500000014000000180000001c0000002a00000000000000576a6ce50a000000746578742f706c61696e170000003358303132354e4d4e5641503030303130322e4c4f4753"),
            Vec::new(),
        ];
        let witnesses = vec![
            "0x434b42465303f843339d255f1c0c7f61a194998fe6621934324c87f8a291b8b68f63dd6d96a2010000002062cb1a00000000474956455f4e414d452c4e45575f4e414d453a58754368656e670a".to_string(),
            "0x55000000100000005500000055000000410000004e790b7253df7cfdd896e97575856258a74af758c3bf7b584bf1af9f045a05575d9c213df96ab9ac0f0d177d8dcaff2e17b1c7252f64185b05930c1850336dbb00".to_string(),
        ];
        attach_content_witnesses(&mut groups, &witnesses, &data).unwrap();
        assert_eq!(type_group_witness(&groups, &ckbfs), Some(0));
        let payer_hash = script_hash(&payer).unwrap();
        let payer_group = groups
            .iter()
            .find(|g| g.kind == "lock" && g.script_hash == payer_hash)
            .unwrap();
        assert_eq!(payer_group.witness_index, Some(1));
    }

    /// Mainnet tx 0x1750e961…b63d6f3f: a 5-field CKBFS publish naming [1].
    #[test]
    fn mainnet_ckbfs_v2_publish_names_its_witness_in_a_fixvec() {
        let secp = script(SECP, "type", "0x9d938bb5eece35e6d9264edafb752c34304c4587");
        let ckbfs = script(
            CKBFS_V2,
            "data1",
            "0x7d18022c252e266e8719196ffa66487e6a1c0f29f85b101d74d0908dfd25663f",
        );
        let mut groups = script_groups(
            &[input(&secp, None)],
            &[output(&secp, Some(&ckbfs)), output(&secp, None)],
        )
        .unwrap();
        let data = [
            hex_bytes("0x5400000018000000200000002400000032000000500000000100000001000000290b82b20a000000746578742f706c61696e1a0000006469726563745f636f6e74656e745f6578616d706c652e74787404000000"),
            Vec::new(),
        ];
        let witnesses = vec![
            "0x550000001000000055000000550000004100000092e6256ddd5aa37d833d0f7fc4a2e40b4655924d87a1e4a447cc48a560048aeb4f2157ce6617ea664e9d83df4ecc5ebd4ec3cbd3ac895a2f727dd8a03e1e835301".to_string(),
            "0x434b4246530048656c6c6f20434b4246532066726f6d2064697265637420636f6e74656e7421".to_string(),
        ];
        attach_content_witnesses(&mut groups, &witnesses, &data).unwrap();
        assert_eq!(type_group_witness(&groups, &ckbfs), Some(1));
    }

    /// Mainnet tx 0xd824b272…4b141d0e (cell data only; its witnesses are
    /// 77 KB): a multi-witness file's head index is the fixvec's first entry.
    #[test]
    fn mainnet_ckbfs_v2_multi_witness_head_index() {
        let data = hex_bytes("0x6300000018000000280000002c0000003a0000005f00000003000000010000000200000003000000482330f50a000000696d6167652f6a70656721000000666f756e64696e672d6d656d6265722d646f622d6f7074696d697a65642e6a706704000000");
        assert_eq!(
            super::super::transaction_cells::ckbfs_head_witness_index(&data).unwrap(),
            1
        );
    }

    /// Review #15: a type group with outputs only reads WitnessArgs through
    /// `Source::GroupOutput` — the witness at its first output index.
    #[test]
    fn output_only_type_groups_read_witness_args_at_their_first_output() {
        let lock = script(SECP, "type", &format!("0x{}", "11".repeat(20)));
        let minted = script(
            &format!("0x{}", "77".repeat(32)),
            "type",
            &format!("0x{}", "aa".repeat(32)),
        );
        let groups = script_groups(
            &[input(&lock, None)],
            &[
                output(&lock, None),
                output(&lock, Some(&minted)),
                output(&lock, Some(&minted)),
            ],
        )
        .unwrap();
        assert_eq!(type_group_witness(&groups, &minted), Some(1));
        // The witness exists: the index stands.
        let witnesses = vec![
            "0x10000000100000001000000010000000".to_string(),
            "0x10000000100000001000000010000000".to_string(),
        ];
        let data = [Vec::<u8>::new(), Vec::new(), Vec::new()];
        let mut with_witness = groups.clone();
        attach_content_witnesses(&mut with_witness, &witnesses, &data).unwrap();
        assert_eq!(type_group_witness(&with_witness, &minted), Some(1));
        // No witness at the first output index: none is read.
        let mut without = groups;
        attach_content_witnesses(&mut without, &witnesses[..1], &data).unwrap();
        assert_eq!(type_group_witness(&without, &minted), None);
    }
}
