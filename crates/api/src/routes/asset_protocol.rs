//! Exact protocol descriptions from original cell bytes; no derived writes.
use anyhow::{Context, Result};
use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MnftPermissions {
    pub allow_claim: bool,
    pub allow_lock: bool,
    pub allow_extension: bool,
    pub allow_characteristic_update: bool,
    pub allow_transfer_before_claim: bool,
    pub allow_transfer_after_claim: bool,
    pub allow_destroy_before_claim: bool,
    pub allow_destroy_after_claim: bool,
}
// Official mNFT contract: configure bits are prohibitions (zero permits).
pub(crate) fn mnft_permissions(configure: u8) -> MnftPermissions {
    MnftPermissions {
        allow_claim: configure & 1 == 0,
        allow_lock: configure & 2 == 0,
        allow_extension: configure & 4 == 0,
        allow_characteristic_update: configure & 8 == 0,
        allow_transfer_before_claim: configure & 16 == 0,
        allow_transfer_after_claim: configure & 32 == 0,
        allow_destroy_before_claim: configure & 64 == 0,
        allow_destroy_after_claim: configure & 128 == 0,
    }
}

fn u32le(bytes: &[u8]) -> Result<u32> {
    Ok(u32::from_le_bytes(bytes.try_into().map_err(|_| {
        anyhow::anyhow!("expected 4-byte integer, got {}", bytes.len())
    })?))
}
fn u64le(bytes: &[u8]) -> Result<u64> {
    Ok(u64::from_le_bytes(bytes.try_into().map_err(|_| {
        anyhow::anyhow!("expected 8-byte integer, got {}", bytes.len())
    })?))
}
fn molecule_bytes(bytes: &[u8]) -> Result<&[u8]> {
    anyhow::ensure!(bytes.len() >= 4, "Molecule Bytes header truncated");
    anyhow::ensure!(
        u32le(&bytes[..4])? as usize == bytes.len() - 4,
        "Molecule Bytes length mismatch"
    );
    Ok(&bytes[4..])
}
fn molecule_fields(bytes: &[u8]) -> Result<Vec<&[u8]>> {
    anyhow::ensure!(bytes.len() >= 4, "Molecule table/vector truncated");
    anyhow::ensure!(
        u32le(&bytes[..4])? as usize == bytes.len(),
        "Molecule total length mismatch"
    );
    if bytes.len() == 4 {
        return Ok(Vec::new());
    }
    anyhow::ensure!(bytes.len() >= 8, "Molecule first offset missing");
    let first = u32le(&bytes[4..8])? as usize;
    anyhow::ensure!(
        first >= 8 && first.is_multiple_of(4) && first <= bytes.len(),
        "Molecule invalid first offset {first}"
    );
    let mut offsets = Vec::new();
    for chunk in bytes[4..first].as_chunks::<4>().0 {
        offsets.push(u32le(chunk)? as usize)
    }
    offsets.push(bytes.len());
    let mut fields = Vec::new();
    for pair in offsets.windows(2) {
        anyhow::ensure!(
            pair[0] >= first && pair[0] <= pair[1] && pair[1] <= bytes.len(),
            "Molecule noncanonical offsets"
        );
        fields.push(&bytes[pair[0]..pair[1]])
    }
    Ok(fields)
}
fn json_cbor(value: ciborium::Value) -> Result<Value> {
    use ciborium::Value as C;
    Ok(match value {
        C::Null => Value::Null,
        C::Bool(v) => Value::Bool(v),
        C::Text(v) => Value::String(v),
        C::Integer(v) => {
            let value: i128 = v.into();
            if (-9_007_199_254_740_991..=9_007_199_254_740_991).contains(&value) {
                Value::from(i64::try_from(value)?)
            } else {
                serde_json::json!({"$integer": value.to_string()})
            }
        }
        C::Bytes(v) => serde_json::json!({"$bytes":format!("0x{}",hex::encode(v))}),
        C::Array(items) => Value::Array(items.into_iter().map(json_cbor).collect::<Result<_>>()?),
        C::Map(items) => {
            let mut seen = std::collections::HashSet::new();
            let plain = items
                .iter()
                .all(|(key, _)| matches!(key,C::Text(text) if seen.insert(text)));
            if plain {
                let mut map = serde_json::Map::new();
                for (key, value) in items {
                    let C::Text(key) = key else {
                        unreachable!("validated text map")
                    };
                    map.insert(key, json_cbor(value)?);
                }
                Value::Object(map)
            } else {
                let pairs = items
                    .into_iter()
                    .map(|(key, value)| Ok(vec![json_cbor(key)?, json_cbor(value)?]))
                    .collect::<Result<Vec<_>>>()?;
                serde_json::json!({"$map":pairs})
            }
        }
        C::Tag(tag, value) => {
            serde_json::json!({"$tag":tag.to_string(),"value":json_cbor(*value)?})
        }
        C::Float(v) => {
            if let Some(number) = serde_json::Number::from_f64(v) {
                Value::Number(number)
            } else {
                serde_json::json!({"$float":if v.is_nan(){"NaN"}else if v.is_sign_negative(){"-Infinity"}else{"Infinity"}})
            }
        }
        _ => anyhow::bail!("unsupported CBOR value"),
    })
}
fn did_document(data: &[u8], id: &[u8]) -> Result<Value> {
    anyhow::ensure!(id.len() >= 20, "DID type args shorter than 20 bytes");
    anyhow::ensure!(data.len() >= 4, "DID union tag missing");
    let tag = u32le(&data[..4])?;
    anyhow::ensure!(tag == 0, "unsupported DID document union {tag}");
    let fields = molecule_fields(&data[4..])?;
    anyhow::ensure!(
        fields.len() == 2,
        "DID V1 needs document and local_id fields"
    );
    let raw = molecule_bytes(fields[0])?;
    let mut cursor = std::io::Cursor::new(raw);
    let cbor: ciborium::Value = ciborium::from_reader(&mut cursor)?;
    anyhow::ensure!(
        cursor.position() == raw.len() as u64,
        "trailing DID CBOR bytes"
    );
    let document = json_cbor(cbor)?;
    let local_id = if fields[1].is_empty() {
        None
    } else {
        Some(std::str::from_utf8(molecule_bytes(fields[1])?)?.to_string())
    };
    let did = format!(
        "did:ckb:{}",
        data_encoding::BASE32_NOPAD
            .encode(&id[..20])
            .to_ascii_lowercase()
    );
    let mut pds = Vec::new();
    if let Some(services) = document.get("services").and_then(Value::as_object) {
        for service in services.values() {
            if service.get("type").and_then(Value::as_str) == Some("AtprotoPersonalDataServer") {
                if let Some(endpoint) = service.get("endpoint").and_then(Value::as_str) {
                    pds.push(endpoint.to_string())
                }
            }
        }
    }
    if let Some(services) = document.get("service").and_then(Value::as_array) {
        for service in services {
            if service.get("type").and_then(Value::as_str) == Some("AtprotoPersonalDataServer") {
                if let Some(endpoint) = service.get("serviceEndpoint").and_then(Value::as_str) {
                    pds.push(endpoint.to_string())
                }
            }
        }
    }
    pds.sort();
    pds.dedup();
    Ok(
        serde_json::json!({"did":did,"itemId":format!("0x{}",hex::encode(id)),"documentVersion":1,"documentEncoding":"cborLosslessJson","documentCborHex":format!("0x{}",hex::encode(raw)),"document":document,"localId":local_id,"pdsEndpoints":pds}),
    )
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProtocolField {
    name: String,
    start: usize,
    end: usize,
    value_hex: String,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProtocolVersion {
    cell: super::asset_history::CellVersion,
    data_hex: String,
    fields: Vec<ProtocolField>,
    decoded: Value,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProtocolResponse {
    kind: String,
    item_id: String,
    versions: Vec<ProtocolVersion>,
}

fn field(data: &[u8], name: &str, start: usize, end: usize) -> Result<ProtocolField> {
    let raw = data.get(start..end).ok_or_else(|| {
        anyhow::anyhow!(
            "protocol field {name} [{start},{end}) exceeds {} bytes",
            data.len()
        )
    })?;
    Ok(ProtocolField {
        name: name.to_string(),
        start,
        end,
        value_hex: format!("0x{}", hex::encode(raw)),
    })
}

/// Only the node's immutable creation transaction supplies bytes. There is no
/// network fallback and no substitution from a different cell version.
pub(crate) fn raw_transaction(
    state: &crate::AppState,
    version: &super::asset_history::CanonicalCellVersion,
) -> Result<ckbadger_indexer::rpc::TransactionView> {
    use ckb_types::prelude::*;
    let context = format!(
        "{}:{}",
        version.response.tx_hash, version.response.output_index
    );
    let reader = state.ckb_store.as_ref().ok_or_else(|| {
        anyhow::anyhow!("protocol data requires local CKB RocksDB reader: {context}")
    })?;
    let hash: [u8; 32] = version.tx_hash.as_slice().try_into()?;
    let (tx, block) = reader
        .get_transaction_with_block_number(&hash)
        .ok_or_else(|| anyhow::anyhow!("protocol transaction missing in CKB store: {context}"))?;
    anyhow::ensure!(
        block as i128 == version.response.block_number as i128,
        "protocol transaction block mismatch: {context}"
    );
    anyhow::ensure!(
        tx.hash().as_slice() == hash,
        "protocol transaction hash mismatch: {context}"
    );
    let index = version.response.output_index as usize;
    let output = tx
        .outputs()
        .get(index)
        .ok_or_else(|| anyhow::anyhow!("protocol output missing: {context}"))?;
    let data = tx
        .outputs_data()
        .get(index)
        .ok_or_else(|| anyhow::anyhow!("protocol data missing: {context}"))?
        .raw_data();
    let cap: u64 = output.capacity().unpack();
    anyhow::ensure!(
        i128::from(cap) == i128::from(version.cell.capacity),
        "protocol capacity mismatch: {context}"
    );
    anyhow::ensure!(
        output.lock().calc_script_hash().as_slice() == version.cell.lock_script_hash,
        "protocol lock mismatch: {context}"
    );
    let type_hash = output
        .type_()
        .to_opt()
        .map(|s| s.calc_script_hash().as_slice().to_vec());
    anyhow::ensure!(
        type_hash == version.cell.type_script_hash,
        "protocol type mismatch: {context}"
    );
    anyhow::ensure!(
        data.len() == version.cell.data_size as usize,
        "protocol data size mismatch: {context}"
    );
    let expected = version
        .cell
        .data_hash
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("canonical data hash missing: {context}"))?;
    verify_indexed_data_hash(&data, expected, &context)?;
    Ok(super::transactions::indexer_tx_view(
        &ckb_store_reader::convert_transaction_view(&tx),
    ))
}

/// Compare original node bytes with the indexer's persisted CellData hash.
/// CKB's `CellOutput::calc_data_hash` substitutes a zero sentinel for empty
/// data, but the indexer stores the Blake2b digest for every byte sequence.
fn verify_indexed_data_hash(data: &[u8], indexed: &[u8], context: &str) -> Result<()> {
    let digest = ckbadger_indexer::parser::script::ScriptParser::compute_data_hash(data);
    anyhow::ensure!(
        digest.as_slice() == indexed,
        "protocol data hash mismatch: {context}"
    );
    Ok(())
}

#[cfg(test)]
mod data_hash_tests {
    use super::verify_indexed_data_hash;

    #[test]
    fn empty_data_requires_the_indexer_digest_not_the_protocol_zero_sentinel() {
        let digest = ckbadger_indexer::parser::script::ScriptParser::compute_data_hash(&[]);
        assert_ne!(digest, [0; 32]);
        verify_indexed_data_hash(&[], &digest, "empty cell").unwrap();
        let error = verify_indexed_data_hash(&[], &[0; 32], "empty cell").unwrap_err();
        assert!(error
            .to_string()
            .contains("protocol data hash mismatch: empty cell"));
    }
}

/// DOB metadata names native TYPE_ID args, not a script usage reference. A
/// decoder-only deployment need not occur in the usage-to-version projection.
/// Read its unique current Cell directly and verify original local node bytes.
pub(crate) fn current_type_id_binary(
    state: &crate::AppState,
    args: &[u8; 32],
) -> Result<Option<Vec<u8>>> {
    let code = hex::decode(ckbadger_indexer::parser::script::TYPE_ID_CODE_HASH)?;
    let reference = crate::utils::address::compute_script_hash(&code, 1, args);
    let Some(version) = super::asset_history::canonical_live_type_version(state, &reference)?
    else {
        return Ok(None);
    };
    anyhow::ensure!(
        version.cell.type_code_hash.as_deref() == Some(code.as_slice())
            && version.cell.type_hash_type == Some(1)
            && version.cell.type_args.as_deref() == Some(args.as_slice()),
        "native Type ID script differs from requested decoder: args=0x{} outpoint={}:{}",
        hex::encode(args),
        version.response.tx_hash,
        version.response.output_index
    );
    let tx = raw_transaction(state, &version)?;
    let data = &tx.outputs_data[version.response.output_index as usize];
    let bytes = hex::decode(data.strip_prefix("0x").unwrap_or(data))?;
    // Same executable recognition as the owning script detector. Retirement
    // may leave a live TYPE_ID cell with non-executable data.
    if !bytes.starts_with(b"\x7fELF") {
        return Ok(None);
    }
    Ok(Some(
        version
            .cell
            .data_hash
            .context("validated decoder data hash missing")?,
    ))
}

fn dotbit_algorithm_width(algorithm: u8) -> Result<usize> {
    match algorithm {
        1 => Ok(28),
        6 => Ok(32),
        8 => Ok(21),
        2 | 3 | 4 | 5 | 7 => Ok(20),
        _ => anyhow::bail!("unsupported .bit lock algorithm {algorithm}"),
    }
}
fn dotbit_lock(args: &[u8]) -> Result<(Value, Value)> {
    let algorithm = *args
        .first()
        .ok_or_else(|| anyhow::anyhow!(".bit lock args empty"))?;
    let length = dotbit_algorithm_width(algorithm)?;
    let owner = args
        .get(1..1 + length)
        .ok_or_else(|| anyhow::anyhow!("truncated .bit owner args for algorithm {algorithm}"))?;
    let manager_type = *args
        .get(1 + length)
        .ok_or_else(|| anyhow::anyhow!("missing .bit manager algorithm"))?;
    let manager_length = dotbit_algorithm_width(manager_type)?;
    let manager = &args[2 + length..];
    anyhow::ensure!(
        manager.len() == manager_length,
        ".bit manager algorithm {manager_type} expects {manager_length} bytes, got {}",
        manager.len()
    );
    Ok((
        serde_json::json!({"algorithm":algorithm,"argsHex":format!("0x{}",hex::encode(owner))}),
        serde_json::json!({"algorithm":manager_type,"argsHex":format!("0x{}",hex::encode(manager))}),
    ))
}

fn dotbit_account_witness(
    tx: &ckbadger_indexer::rpc::TransactionView,
    index: usize,
    id: &[u8],
    data: &[u8],
) -> Result<Value> {
    let mut found = None;
    for witness in &tx.witnesses {
        let bytes = hex::decode(witness.strip_prefix("0x").unwrap_or(witness))?;
        if bytes.get(..7) != Some(&[b'd', b'a', b's', 1, 0, 0, 0]) {
            continue;
        }
        let data_fields = molecule_fields(&bytes[7..])?;
        anyhow::ensure!(
            data_fields.len() == 3,
            ".bit witness Data needs dep/old/new"
        );
        if data_fields[2].is_empty() {
            continue;
        }
        let entity = molecule_fields(data_fields[2])?;
        anyhow::ensure!(
            entity.len() == 3,
            ".bit DataEntity needs index/version/entity"
        );
        if u32le(entity[0])? as usize != index {
            continue;
        }
        anyhow::ensure!(
            found.is_none(),
            "duplicate .bit new witness for output {index}"
        );
        let version = u32le(entity[1])?;
        let raw = molecule_bytes(entity[2])?;
        anyhow::ensure!(
            data.len() >= 32 && ckb_hash::blake2b_256(raw).as_slice() == &data[..32],
            ".bit witness entity hash differs from cell commitment"
        );
        let fields = molecule_fields(raw)?;
        let expected = match version {
            1 => 6,
            2 => 8,
            3 => 10,
            4 => 11,
            _ => anyhow::bail!("unsupported .bit AccountCellData version {version}"),
        };
        anyhow::ensure!(
            fields.len() == expected,
            ".bit AccountCellData v{version} field count {} != {expected}",
            fields.len()
        );
        anyhow::ensure!(
            fields[0] == id,
            ".bit witness account ID differs from indexed item"
        );
        let mut account = String::new();
        for character in molecule_fields(fields[1])? {
            let ch = molecule_fields(character)?;
            anyhow::ensure!(ch.len() == 2, "invalid .bit AccountChar");
            u32le(ch[0])?;
            account.push_str(std::str::from_utf8(molecule_bytes(ch[1])?)?);
        }
        if !account.ends_with(".bit") {
            account.push_str(".bit")
        }
        let status_index = if version == 1 { 4 } else { 6 };
        let records_index = if version == 1 { 5 } else { 7 };
        anyhow::ensure!(fields[status_index].len() == 1, ".bit status width invalid");
        let mut records = Vec::new();
        for record in molecule_fields(fields[records_index])? {
            let f = molecule_fields(record)?;
            anyhow::ensure!(f.len() == 5, ".bit Record field count invalid");
            records.push(serde_json::json!({"type":std::str::from_utf8(molecule_bytes(f[0])?)?,"key":std::str::from_utf8(molecule_bytes(f[1])?)?,"label":std::str::from_utf8(molecule_bytes(f[2])?)?,"value":std::str::from_utf8(molecule_bytes(f[3])?).ok(),"valueHex":format!("0x{}",hex::encode(molecule_bytes(f[3])?)),"ttl":u32le(f[4])?}));
        }
        // AccountCellData v1: id, account, registered_at, updated_at, status,
        // records. v2 replaces updated_at by last_transfer_account_at,
        // last_edit_manager_at, last_edit_records_at; v3 appends
        // enable_sub_account (Uint8) and renew_sub_account_price (Uint64); v4
        // appends approval.
        // Uint64 fields as exact decimal strings (seconds, or shannons).
        let u64_text = |index: usize| -> Result<Value> {
            Ok(Value::String(u64le(fields[index])?.to_string()))
        };
        let (updated_at, last_transfer, last_edit_manager, last_edit_records) = if version == 1 {
            (u64_text(3)?, Value::Null, Value::Null, Value::Null)
        } else {
            (Value::Null, u64_text(3)?, u64_text(4)?, u64_text(5)?)
        };
        let (enable_sub_account, renew_sub_account_price) = if version >= 3 {
            anyhow::ensure!(
                fields[8].len() == 1,
                ".bit enable_sub_account width invalid"
            );
            (Value::from(fields[8][0]), u64_text(9)?)
        } else {
            (Value::Null, Value::Null)
        };
        found = Some(
            serde_json::json!({"account":account,"witnessVersion":version,"registeredAt":u64le(fields[2])?.to_string(),"updatedAt":updated_at,"lastTransferAccountAt":last_transfer,"lastEditManagerAt":last_edit_manager,"lastEditRecordsAt":last_edit_records,"enableSubAccount":enable_sub_account,"renewSubAccountPrice":renew_sub_account_price,"status":fields[status_index][0],"records":records}),
        );
    }
    found.ok_or_else(|| {
        anyhow::anyhow!(".bit output {index} missing its committed AccountCellData new witness")
    })
}

pub(crate) fn protocol(
    state: &crate::AppState,
    raw: &str,
    standard: super::assets::NftLifecycleStandard,
) -> crate::response::ApiResult<ProtocolResponse> {
    use super::assets::NftLifecycleStandard as S;
    use crate::response::{ok, ApiError};
    let id = super::asset_history::parse_item_id(raw, standard)?;
    let versions = super::asset_history::checked_versions(state, &id, standard)?;
    let mut response = Vec::new();
    for version in versions {
        let parsed=(||->Result<ProtocolVersion>{
            let tx=raw_transaction(state,&version)?;let index=version.response.output_index as usize;
            let data=hex::decode(tx.outputs_data[index].strip_prefix("0x").unwrap_or(&tx.outputs_data[index]))?;
            let mut fields=Vec::new();
            let decoded=match standard {
                S::DidCkb=>{fields.push(field(&data,"union",0,4)?);let f=molecule_fields(&data[4..])?;anyhow::ensure!(f.len()==2,"DID table field count");let start=f[0].as_ptr() as usize-data.as_ptr() as usize;fields.push(field(&data,"document",start+4,start+f[0].len())?);did_document(&data,&id)?},
                S::MnftToken=>{anyhow::ensure!(data.len()>=11,"mNFT token data shorter than 11 bytes");fields.push(field(&data,"version",0,1)?);fields.push(field(&data,"characteristic",1,9)?);fields.push(field(&data,"configure",9,10)?);fields.push(field(&data,"state",10,11)?);serde_json::json!({"characteristicHex":format!("0x{}",hex::encode(&data[1..9])),"configure":data[9],"state":data[10],"permissions":mnft_permissions(data[9]),"claimed":data[10]&1!=0,"locked":data[10]&2!=0})},
                S::Spore=>{
                    let parsed=ckbadger_indexer::parser::SporeParser::parse_spore_cell(&tx.outputs[index],&tx.outputs_data[index]).ok_or_else(||anyhow::anyhow!("invalid Spore molecule content"))?;
                    anyhow::ensure!(parsed.spore_id==id,"Spore payload ID differs from indexed item");
                    for (name,part) in ["contentType","content","clusterId"].iter().zip(molecule_fields(&data)?) {let start=part.as_ptr() as usize-data.as_ptr() as usize;fields.push(field(&data,name,start,start+part.len())?);}
                    let dna=if matches!(ckbadger_indexer::sync::dob_decode_worker::parse_dob_version_from_content_type(&parsed.content_type),Some(0|1)){ckbadger_indexer::parser::media_source::parse_dna_hex_from_content(&parsed.content)}else{None};
                    serde_json::json!({"contentHex":format!("0x{}",hex::encode(parsed.content)),"dnaHex":dna.map(|d|format!("0x{d}"))})
                },
                S::BitCell=>{
                    let bit=ckbadger_indexer::parser::BitCellParser::parse_cell(&tx.outputs[index],&tx.outputs_data[index]).ok_or_else(||anyhow::anyhow!("invalid .bit Cell payload"))?;
                    anyhow::ensure!(bit.identity_id==id,".bit Cell identity differs from indexed item");
                    if data.get(..4)==Some(&[0,0,0,0]) {
                        let slices=molecule_fields(&data[4..])?;
                        for (name,part) in ["witnessHash","expiredAt","account"].iter().zip(slices){let start=part.as_ptr() as usize-data.as_ptr() as usize;fields.push(field(&data,name,start,start+part.len())?);}
                    } else {
                        let slices=molecule_fields(&data)?;let content=molecule_bytes(slices[1])?;let start=content.as_ptr() as usize-data.as_ptr() as usize;
                        for(name,a,b)in [("prefix",0,1),("version",1,2),("witnessHash",2,22),("expiredAt",22,30),("account",30,content.len())]{fields.push(field(&data,name,start+a,start+b)?)}
                    }

                    let linked=state.store.get_identity(&bit.account_id)?.map(|entry|->Result<Value>{
                        anyhow::ensure!(entry.standard==ckbadger_store::IdentityStandard::DotBit,"linked .bit account has another identity standard");
                        let ckbadger_store::IdentityExtra::DotBit{registered_at,expired_at,..}=entry.extra else {anyhow::bail!("linked .bit account has incompatible metadata")};
                        Ok(serde_json::json!({"id":format!("0x{}",hex::encode(&bit.account_id)),"name":entry.name,"isLive":entry.is_live,"registeredAt":registered_at.map(|v|v.to_string()),"expiredAt":expired_at.map(|v|v.to_string())}))
                    }).transpose()?;
                    serde_json::json!({"identityId":format!("0x{}",hex::encode(&id)),"accountId":format!("0x{}",hex::encode(bit.account_id)),"account":bit.account,"expiredAt":bit.expired_at.to_string(),"linkedAccount":linked,"linkedAccountSource":"currentCanonicalAccount"})
                },
                S::DotBit=>{
                    let account=ckbadger_indexer::parser::DotbitParser::parse_account_cell(&tx.outputs[index],&tx.outputs_data[index]).ok_or_else(||anyhow::anyhow!("invalid .bit account cell"))?;
                    anyhow::ensure!(account.account_id==id,".bit cell identity differs from indexed item");
                    let mut value=dotbit_account_witness(&tx,index,&id,&data)?;
                    let next_name=match &account.next_account_id {Some(next)=>state.store.get_identity(next)?.and_then(|e|e.name),None=>None};
                    let args=hex::decode(tx.outputs[index].lock.args.strip_prefix("0x").unwrap_or(&tx.outputs[index].lock.args))?;
                    let (owner,manager)=dotbit_lock(&args)?;
                    let map=value.as_object_mut().expect("decoded account object");map.insert("accountId".into(),Value::String(format!("0x{}",hex::encode(&id))));map.insert("nextAccountId".into(),serde_json::to_value(account.next_account_id.map(|n|format!("0x{}",hex::encode(n))))?);map.insert("nextAccountName".into(),serde_json::to_value(next_name)?);map.insert("expiredAt".into(),serde_json::to_value(account.expired_at.map(|n|n.to_string()))?);map.insert("owner".into(),owner);map.insert("manager".into(),manager);
                    fields.push(field(&data,"accountDataHash",0,32)?);fields.push(field(&data,"accountId",32,52)?);if data.len()>=72 {fields.push(field(&data,"nextAccountId",52,72)?)}
                    if data.len()>=80{fields.push(field(&data,"expiredAt",72,80)?)}
                    // ID-B3: the account name the cell carries after expiredAt.
                    if data.len()>80{fields.push(field(&data,"account",80,data.len())?)}value
                },
                S::DotCell=>{
                    let name=ckbadger_indexer::parser::DotCellParser::parse_name_data(&data)?;anyhow::ensure!(name.id.as_slice()==id,".cell payload identity mismatch");
                    let witness=tx.witnesses.get(index).ok_or_else(||anyhow::anyhow!(".cell records witness missing at output {index}"))?;
                    let records=ckbadger_indexer::parser::DotCellParser::parse_witness_records(witness,&name.records_hash)?;
                    let resolver=super::identities::DotCellPartyResolver{store:&state.store,network:&state.ckb_network};
                    let owner=resolver.resolve(&name.owner_hash20).map_err(|(_,e)|anyhow::anyhow!("{}",e.0.message))?;let manager=resolver.resolve(&name.manager_hash20).map_err(|(_,e)|anyhow::anyhow!("{}",e.0.message))?;
                    for (n,a,b) in [("layoutVersion",0,1),("recordsHash",1,33),("nextId",33,53),("expiredAt",53,58),("owner",58,78),("manager",78,98),("label",98,data.len())]{fields.push(field(&data,n,a,b)?)}
                    let records:Vec<_>=records.iter().map(super::identities::dotcell_record_response).collect();
                    serde_json::json!({"name":format!("{}.cell",name.label),"label":name.label,"layoutVersion":name.layout_version,"expiredAt":name.expired_at.to_string(),"recordsHash":format!("0x{}",hex::encode(name.records_hash)),"nextId":format!("0x{}",hex::encode(name.next_id)),"owner":owner,"manager":manager,"records":records})
                }
            };
            Ok(ProtocolVersion{cell:version.response.clone(),data_hex:format!("0x{}",hex::encode(data)),fields,decoded})
        })().map_err(|e|ApiError::internal(format!("item=0x{} outpoint={}:{}: {e:#}",hex::encode(&id),version.response.tx_hash,version.response.output_index)))?;
        response.push(parsed);
    }
    let kind = match standard {
        S::Spore => "spore",
        S::MnftToken => "m-nft",
        S::DotBit => "dotbit",
        S::DidCkb => "did_ckb",
        S::BitCell => "bit_cell",
        S::DotCell => "dotcell",
    };
    ok(ProtocolResponse {
        kind: kind.to_string(),
        item_id: format!("0x{}", hex::encode(id)),
        versions: response,
    })
}
pub(crate) async fn pds_groups(
    axum::extract::State(state): axum::extract::State<std::sync::Arc<crate::AppState>>,
) -> crate::response::ApiResult<Value> {
    tokio::task::spawn_blocking(move || pds_groups_read(&state))
        .await
        .map_err(|e| crate::response::ApiError::internal(e.to_string()))?
}
fn pds_groups_read(state: &crate::AppState) -> crate::response::ApiResult<Value> {
    let result = (|| -> Result<Value> {
        let collection =
            ckbadger_store::types::identity_sentinel_for(ckbadger_store::IdentityStandard::DidCkb);
        let ids = state
            .store
            .list_identity_ids_by_collection(collection, None, usize::MAX)?;
        let mut groups = std::collections::BTreeMap::<String, u64>::new();
        let mut total = 0u64;
        let mut without = 0u64;
        for id in ids {
            let entry = state.store.get_identity(&id)?.ok_or_else(|| {
                anyhow::anyhow!("DID collection index points to missing identity")
            })?;
            anyhow::ensure!(
                entry.standard == ckbadger_store::IdentityStandard::DidCkb,
                "DID collection contains another identity standard"
            );
            if !entry.is_live {
                continue;
            }
            total += 1;
            let versions = super::asset_history::checked_versions(
                state,
                &id,
                super::assets::NftLifecycleStandard::DidCkb,
            )
            .map_err(|(_, error)| anyhow::anyhow!(error.0.message))?;
            let live = versions
                .iter()
                .find(|v| v.response.is_live)
                .ok_or_else(|| {
                    anyhow::anyhow!("live DID missing canonical cell: 0x{}", hex::encode(&id))
                })?;
            let tx = raw_transaction(state, live)?;
            let hex = &tx.outputs_data[live.response.output_index as usize];
            let raw = hex::decode(hex.trim_start_matches("0x"))?;
            let doc = did_document(&raw, &id)?;
            let endpoints = doc["pdsEndpoints"].as_array().expect("PDS endpoints array");
            if endpoints.is_empty() {
                without += 1
            }
            for endpoint in endpoints {
                *groups
                    .entry(endpoint.as_str().expect("endpoint string").to_string())
                    .or_default() += 1;
            }
        }
        Ok(
            serde_json::json!({"totalLiveIdentities":total,"withoutPds":without,"groups":groups.into_iter().map(|(endpoint,count)|serde_json::json!({"endpoint":endpoint,"count":count})).collect::<Vec<_>>(),"source":"currentCanonicalDocuments"}),
        )
    })();
    crate::response::ok(
        result
            .map_err(|e| crate::response::ApiError::internal(format!("DID PDS grouping: {e:#}")))?,
    )
}

pub(crate) async fn cluster_protocol(
    axum::extract::State(state): axum::extract::State<std::sync::Arc<crate::AppState>>,
    axum::extract::Path(raw): axum::extract::Path<String>,
) -> crate::response::ApiResult<Value> {
    tokio::task::spawn_blocking(move||{
        let id=super::spore::parse_cluster_id_param(&raw)?;
        super::spore::read_cluster(&state,&id)?;
        let result=(||->Result<Value>{
            let current=super::asset_history::cluster_current(&state,&id)?;
            let Some(current)=current else {return Ok(serde_json::json!({"kind":"spore_cluster","clusterId":format!("0x{}",hex::encode(&id)),"source":"canonicalLiveTypeIndex","current":null}))};
            let tx=raw_transaction(&state,&current)?;
            let index=usize::try_from(current.response.output_index)?;
            let data=hex::decode(tx.outputs_data[index].trim_start_matches("0x"))?;
            let parsed=ckbadger_indexer::parser::SporeParser::parse_cluster_cell(&tx.outputs[index],&tx.outputs_data[index]).ok_or_else(||anyhow::anyhow!("invalid cluster payload"))?;
            anyhow::ensure!(parsed.cluster_id==id,"cluster payload ID disagrees with canonical index");
            let parts=molecule_fields(&data)?;
            anyhow::ensure!(parts.len()==2 || parts.len()==3,"unsupported cluster field count {}",parts.len());
            let mut fields=Vec::new();
            let mut offset=4+parts.len()*4;
            for (index,part) in parts.iter().enumerate() {
                fields.push(field(&data,["name","description","mutantId"][index],offset,offset+part.len())?);
                offset+=part.len();
            }
            let current=ProtocolVersion{cell:current.response,data_hex:format!("0x{}",hex::encode(data)),fields,decoded:serde_json::json!({"name":parsed.name,"description":parsed.description})};
            Ok(serde_json::json!({"kind":"spore_cluster","clusterId":format!("0x{}",hex::encode(&id)),"source":"canonicalLiveTypeIndex","current":current}))
        })();
        crate::response::ok(result.map_err(|e|crate::response::ApiError::internal(format!("cluster=0x{}: {e:#}",hex::encode(&id))))?)
    }).await.map_err(|e|crate::response::ApiError::internal(e.to_string()))?
}

#[cfg(test)]
mod tests {
    use super::*;
    fn bytes(raw: &[u8]) -> Vec<u8> {
        let mut b = (raw.len() as u32).to_le_bytes().to_vec();
        b.extend(raw);
        b
    }
    fn table(fields: &[Vec<u8>]) -> Vec<u8> {
        let header = 4 * (fields.len() + 1);
        let total = header + fields.iter().map(Vec::len).sum::<usize>();
        let mut b = (total as u32).to_le_bytes().to_vec();
        let mut off = header;
        for f in fields {
            b.extend((off as u32).to_le_bytes());
            off += f.len()
        }
        for f in fields {
            b.extend(f)
        }
        b
    }
    #[test]
    fn cbor_integer_document_values_are_lossless_in_javascript() {
        use ciborium::Value as C;
        assert_eq!(
            json_cbor(C::Integer(u64::MAX.into())).unwrap(),
            serde_json::json!({"$integer":u64::MAX.to_string()})
        );
        assert_eq!(
            json_cbor(C::Integer((-9_007_199_254_740_992i64).into())).unwrap(),
            serde_json::json!({"$integer":"-9007199254740992"})
        );
        assert_eq!(
            json_cbor(C::Integer(9_007_199_254_740_991u64.into())).unwrap(),
            serde_json::json!(9_007_199_254_740_991u64)
        );
        assert_eq!(
            json_cbor(C::Tag(u64::MAX, Box::new(C::Null))).unwrap(),
            serde_json::json!({"$tag":u64::MAX.to_string(),"value":null})
        );
    }
    #[test]
    fn dotbit_lock_requires_exact_owner_and_manager_algorithm_widths() {
        for (owner, owner_len) in [
            (1, 28),
            (2, 20),
            (3, 20),
            (4, 20),
            (5, 20),
            (6, 32),
            (7, 20),
            (8, 21),
        ] {
            for (manager, manager_len) in [
                (1, 28),
                (2, 20),
                (3, 20),
                (4, 20),
                (5, 20),
                (6, 32),
                (7, 20),
                (8, 21),
            ] {
                let mut args = vec![owner];
                args.extend(vec![0x11; owner_len]);
                args.push(manager);
                args.extend(vec![0x22; manager_len]);
                let (o, m) = dotbit_lock(&args).unwrap();
                assert_eq!(o["algorithm"], owner);
                assert_eq!(m["algorithm"], manager);
                assert!(
                    dotbit_lock(&args[..args.len() - 1]).is_err(),
                    "truncated manager {manager}"
                );
                args.push(0);
                assert!(
                    dotbit_lock(&args).is_err(),
                    "trailing manager bytes {manager}"
                );
            }
            let args = vec![owner];
            assert!(dotbit_lock(&args).is_err());
        }
        assert!(dotbit_lock(&[0]).is_err());
    }
    #[test]
    fn arbitrary_cbor_non_json_values_remain_explicit_raw_evidence() {
        use ciborium::Value as C;
        let map = C::Map(vec![(C::Integer(1.into()), C::Bytes(vec![2]))]);
        assert_eq!(
            json_cbor(map).unwrap(),
            serde_json::json!({"$map":[[1,{"$bytes":"0x02"}]]})
        );
        assert_eq!(
            json_cbor(C::Float(f64::INFINITY)).unwrap(),
            serde_json::json!({"$float":"Infinity"})
        );
        assert_eq!(
            json_cbor(C::Float(f64::NAN)).unwrap(),
            serde_json::json!({"$float":"NaN"})
        );
        let duplicate = C::Map(vec![
            (C::Text("a".into()), C::Bool(true)),
            (C::Text("a".into()), C::Bool(false)),
        ]);
        assert_eq!(
            json_cbor(duplicate).unwrap(),
            serde_json::json!({"$map":[["a",true],["a",false]]})
        );
    }
    #[test]
    fn dotbit_witness_uses_only_matching_committed_new_entity() {
        let id = [0x33; 20];
        let raw = table(&[
            id.to_vec(),
            table(&[table(&[0u32.to_le_bytes().to_vec(), bytes(b"alice")])]),
            1u64.to_le_bytes().to_vec(),
            2u64.to_le_bytes().to_vec(),
            vec![0],
            table(&[]),
        ]);
        let entity = table(&[
            0u32.to_le_bytes().to_vec(),
            1u32.to_le_bytes().to_vec(),
            bytes(&raw),
        ]);
        let witness = |old: Vec<u8>, new: Vec<u8>| {
            let mut w = b"das\x01\0\0\0".to_vec();
            w.extend(table(&[vec![], old, new]));
            format!("0x{}", hex::encode(w))
        };
        let mut tx = ckbadger_indexer::rpc::TransactionView {
            hash: "0x00".into(),
            version: "0x0".into(),
            cell_deps: vec![],
            header_deps: vec![],
            inputs: vec![],
            outputs: vec![],
            outputs_data: vec![],
            witnesses: vec![witness(entity.clone(), entity.clone())],
        };
        let hash = ckb_hash::blake2b_256(&raw);
        let decoded = dotbit_account_witness(&tx, 0, &id, &hash).unwrap();
        assert_eq!(decoded["account"], "alice.bit");
        assert!(dotbit_account_witness(&tx, 1, &id, &hash).is_err());
        assert!(dotbit_account_witness(&tx, 0, &id, &[0; 32])
            .unwrap_err()
            .to_string()
            .contains("commitment"));
        tx.witnesses = vec![witness(entity, vec![])];
        assert!(dotbit_account_witness(&tx, 0, &id, &hash)
            .unwrap_err()
            .to_string()
            .contains("new witness"));
    }
    #[test]
    fn dotbit_binary_record_value_is_lossless() {
        let id = [0x33; 20];
        let record = table(&[
            bytes(b"custom"),
            bytes(b"binary"),
            bytes(b""),
            bytes(&[0xff, 0, 0x80]),
            300u32.to_le_bytes().to_vec(),
        ]);
        let raw = table(&[
            id.to_vec(),
            table(&[table(&[0u32.to_le_bytes().to_vec(), bytes(b"alice")])]),
            1u64.to_le_bytes().to_vec(),
            2u64.to_le_bytes().to_vec(),
            vec![0],
            table(&[record]),
        ]);
        let entity = table(&[
            0u32.to_le_bytes().to_vec(),
            1u32.to_le_bytes().to_vec(),
            bytes(&raw),
        ]);
        let mut witness = b"das\x01\0\0\0".to_vec();
        witness.extend(table(&[vec![], vec![], entity]));
        let tx = ckbadger_indexer::rpc::TransactionView {
            hash: "0x00".into(),
            version: "0x0".into(),
            cell_deps: vec![],
            header_deps: vec![],
            inputs: vec![],
            outputs: vec![],
            outputs_data: vec![],
            witnesses: vec![format!("0x{}", hex::encode(witness))],
        };
        let decoded = dotbit_account_witness(&tx, 0, &id, &ckb_hash::blake2b_256(&raw)).unwrap();
        assert!(decoded["records"][0]["value"].is_null());
        assert_eq!(decoded["records"][0]["valueHex"], "0xff0080");
    }
    #[test]
    fn mnft_configure_uses_all_eight_inverse_permission_bits() {
        let p = mnft_permissions(0xc0);
        assert!(
            p.allow_claim
                && p.allow_lock
                && p.allow_extension
                && p.allow_characteristic_update
                && p.allow_transfer_before_claim
                && p.allow_transfer_after_claim
        );
        assert!(!p.allow_destroy_before_claim && !p.allow_destroy_after_claim);
        for bit in 0..8 {
            let value = serde_json::to_value(mnft_permissions(1 << bit)).unwrap();
            assert_eq!(
                value
                    .as_object()
                    .unwrap()
                    .values()
                    .filter(|v| **v == false)
                    .count(),
                1
            );
        }
    }
    #[test]
    fn did_document_preserves_cbor_fields_and_full_item_id() {
        // {"alsoKnownAs":["at://alice.test"], "services":{}}
        let cbor = hex::decode(
            "a26b616c736f4b6e6f776e4173816f61743a2f2f616c6963652e74657374687365727669636573a0",
        )
        .unwrap();
        let mut data = 0u32.to_le_bytes().to_vec();
        data.extend(table(&[bytes(&cbor), Vec::new()]));
        let parsed = did_document(&data, &[0x11; 32]).unwrap();
        assert_eq!(parsed["document"]["alsoKnownAs"][0], "at://alice.test");
        assert_eq!(parsed["did"], "did:ckb:ceirceirceirceirceirceirceirceir");
        assert_eq!(parsed["itemId"], format!("0x{}", hex::encode([0x11; 32])));
        assert!(did_document(&data[..data.len() - 1], &[0x11; 32]).is_err());
    }
}
