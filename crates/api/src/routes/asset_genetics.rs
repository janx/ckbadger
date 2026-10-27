//! Whole-collection, read-time DOB evidence. VM output remains the sole trait
//! value source. Current decoder byte rules and cached results are separate.
use super::{asset_history, asset_protocol, assets::NftLifecycleStandard};
use crate::{
    response::{default_limit, ok, ApiError, ApiResult, CursorPaginatedResponse},
    utils::parse_hash32,
    AppState,
};
use anyhow::{ensure, Context, Result};
use axum::{
    extract::{Path, Query, State},
    routing::get,
    Router,
};
use ckbadger_store::{DecodeOutcome, ObjectEntry, ObjectStandard};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{collections::BTreeMap, sync::Arc};

pub(crate) fn merged_traits(
    entry: &ckbadger_store::DobDecodedEntry,
) -> Vec<super::spore::DobTraitResponse> {
    let mut traits: Vec<super::spore::DobTraitResponse> = Vec::new();
    for step in &entry.steps {
        let names: std::collections::HashSet<&str> = step
            .traits
            .iter()
            .map(|trait_value| trait_value.name.as_str())
            .collect();
        // A later decoder replaces a name's group. Values within that one
        // decoder output remain distinct evidence, including repeated values.
        traits.retain(|trait_value| !names.contains(trait_value.name.as_str()));
        traits.extend(
            step.traits
                .iter()
                .map(|trait_value| super::spore::DobTraitResponse {
                    name: trait_value.name.clone(),
                    value: trait_value.value.clone(),
                }),
        );
    }
    traits
}
fn is_dob(entry: &ObjectEntry) -> Result<bool> {
    let ckbadger_store::ObjectExtra::Spore { content_type, .. } = &entry.extra else {
        anyhow::bail!("Spore item has incompatible metadata")
    };
    Ok(matches!(
        ckbadger_indexer::sync::dob_decode_worker::parse_dob_version_from_content_type(
            content_type
        ),
        Some(0 | 1)
    ))
}
fn not_dob_traits() -> Value {
    json!({"status":"notDob","decodedAt":null,"traits":[],"source":"cachedVm","originalDecoderHashRecorded":false})
}
fn cached_traits(state: &AppState, id: &[u8]) -> Result<Value> {
    Ok(match state.store.get_dob_decode_outcome(id)? {
        Some(DecodeOutcome::Decoded(entry)) => {
            json!({"status":"decoded","decodedAt":entry.decoded_at.to_string(),"traits":merged_traits(&entry),"source":"cachedVm","originalDecoderHashRecorded":false})
        }
        Some(DecodeOutcome::Failed(failure)) => {
            json!({"status":"failed","decodedAt":null,"traits":[],"source":"cachedVm","originalDecoderHashRecorded":false,"reason":failure.message})
        }
        None => {
            json!({"status":"pending","decodedAt":null,"traits":[],"source":"cachedVm","originalDecoderHashRecorded":false})
        }
    })
}

pub(crate) fn cluster_items(state: &AppState, id: &[u8]) -> Result<Vec<(Vec<u8>, ObjectEntry)>> {
    let mut items = Vec::new();
    for row in state
        .store
        .prefix_iterator_cf(state.store.cf_spore_by_cluster(), id)
    {
        let (key, _) = row?;
        if !key.starts_with(id) {
            break;
        }
        ensure!(
            key.len() == 64,
            "cluster membership key width {}, cluster=0x{}",
            key.len(),
            hex::encode(id)
        );
        let item_id = key[32..].to_vec();
        let item = state.store.get_spore(&item_id)?.with_context(|| {
            format!(
                "cluster membership points to missing spore 0x{}",
                hex::encode(&item_id)
            )
        })?;
        ensure!(
            item.standard == ObjectStandard::Spore,
            "cluster membership points to non-spore"
        );
        let collection = item.collection_id.as_deref().with_context(|| {
            format!(
                "Spore 0x{} missing normalized collection membership",
                hex::encode(&item_id)
            )
        })?;
        ensure!(
            collection == id,
            "spore collection membership mismatch: 0x{}",
            hex::encode(&item_id)
        );
        items.push((item_id, item));
    }
    items.sort_by(|a, b| {
        b.1.created_at_block
            .cmp(&a.1.created_at_block)
            .then_with(|| a.0.cmp(&b.0))
    });
    Ok(items)
}

fn exact_bounds(value: &mut Value) {
    // Declared offsets are u64 protocol integers. Preserve them beyond JS's
    // exact-number range; mapped offsets are bounded by a real cell's bytes.
    if let Some(genes) = value.get_mut("genes").and_then(Value::as_array_mut) {
        for gene in genes {
            for key in ["byteOffset", "byteLength"] {
                if let Some(n) = gene.get(key).and_then(Value::as_u64) {
                    gene[key] = Value::String(n.to_string())
                }
            }
        }
    }
}

fn current_pattern(
    state: &AppState,
    cluster: Option<&ObjectEntry>,
    dna: Option<&[u8]>,
) -> Result<Value> {
    let Some(description) = cluster.and_then(|c| c.description.as_deref()) else {
        return Ok(json!({"status":"notDob","steps":[]}));
    };
    let metadata: Value = match serde_json::from_str(description) {
        Ok(v) => v,
        Err(_) => return Ok(json!({"status":"notDob","steps":[]})),
    };
    let Some(dob) = metadata.get("dob") else {
        return Ok(json!({"status":"notDob","steps":[]}));
    };
    use ckbadger_dob_decoder::types::DecoderRef;
    use ckbadger_indexer::sync::dob_decode_worker::{
        parse_decoder_steps, parse_dob_version_from_cluster,
    };
    let version = parse_dob_version_from_cluster(dob)?;
    let mut parsed_steps = parse_decoder_steps(dob)?;
    if version == ckbadger_indexer::sync::dob_decode_worker::DobVersion::V0 {
        parsed_steps.truncate(1);
    }
    let mut steps = Vec::new();
    for (index, step) in parsed_steps.into_iter().enumerate() {
        let (kind, hash) = match &step.decoder_ref {
            DecoderRef::CodeHash(hash) => ("code_hash", hash),
            DecoderRef::TypeId(hash) => ("type_id", hash),
        };
        let hash: [u8; 32] = hash
            .as_slice()
            .try_into()
            .context("DOB decoder hash width")?;
        let reference = format!("0x{}", hex::encode(hash));
        let decoder = json!({"type":kind,"hash":reference});
        let binary = match step.decoder_ref {
            DecoderRef::CodeHash(_) => Some(hash.to_vec()),
            DecoderRef::TypeId(_) => {
                let code = hex::decode(ckbadger_indexer::parser::script::TYPE_ID_CODE_HASH)?;
                let reference_hash = crate::utils::address::compute_script_hash(&code, 1, &hash);
                crate::utils::script_resolution::persisted_reference_version_candidate(
                    &state.store,
                    &state.append_only_store,
                    1,
                    &reference_hash,
                )?
            }
        };
        let (description, mapped) = if let Some(binary) = &binary {
            let hash: [u8; 32] = binary
                .as_slice()
                .try_into()
                .context("DOB binary hash width")?;
            let desc = ckbadger_dob_decoder::pattern::describe_serialized_pattern(
                &hash,
                &step.pattern_json,
            )?;
            let mapped = if matches!(
                desc,
                ckbadger_dob_decoder::pattern::PatternDescription::Supported { .. }
            ) {
                dna.map(|d| ckbadger_dob_decoder::pattern::map_dna(&desc, d))
                    .transpose()?
                    .unwrap_or_default()
            } else {
                Vec::new()
            };
            (desc, mapped)
        } else {
            (
                ckbadger_dob_decoder::pattern::PatternDescription::Unsupported {
                    reason: format!(
                        "decoder reference has no current executable mapping: {kind} {reference}"
                    ),
                },
                Vec::new(),
            )
        };
        let mut desc = serde_json::to_value(description)?;
        exact_bounds(&mut desc);
        let mut bytes = serde_json::to_value(mapped)?;
        for gene in bytes.as_array_mut().expect("mapped array") {
            for key in ["declaredByteOffset", "declaredByteLength"] {
                if let Some(n) = gene.get(key).and_then(Value::as_u64) {
                    gene[key] = Value::String(n.to_string())
                }
            }
        }
        steps.push(json!({"step":index,"decoderReference":decoder,"decoderBinaryHash":binary.map(|h|format!("0x{}",hex::encode(h))),"description":desc,"bytes":bytes}));
    }
    Ok(
        json!({"status":"dob","version":version.number(),"source":"currentCanonicalDecoder","steps":steps}),
    )
}

pub(crate) fn spore_content(
    state: &AppState,
    id: &[u8],
) -> Result<ckbadger_indexer::parser::spore::ParsedSporeCell> {
    let versions = asset_history::canonical_versions(state, id, NftLifecycleStandard::Spore)?;
    let first = versions
        .first()
        .context("Spore has no canonical cell version")?;
    let tx = asset_protocol::raw_transaction(state, first)?;
    let index = first.response.output_index as usize;
    let parsed = ckbadger_indexer::parser::SporeParser::parse_spore_cell(
        &tx.outputs[index],
        &tx.outputs_data[index],
    )
    .context("invalid Spore molecule")?;
    ensure!(parsed.spore_id == id, "Spore payload ID differs from index");
    Ok(parsed)
}
fn item_dna_number(content_type: &str, content: &[u8]) -> (Option<String>, Option<String>) {
    if !matches!(
        ckbadger_indexer::sync::dob_decode_worker::parse_dob_version_from_content_type(
            content_type
        ),
        Some(0 | 1)
    ) {
        return (None, None);
    }
    let dna = ckbadger_indexer::parser::media_source::parse_dna_hex_from_content(content)
        .map(|s| format!("0x{s}"));
    let number = serde_json::from_slice::<Value>(content)
        .ok()
        .and_then(|v| v.get("id").cloned())
        .and_then(|v| match v {
            Value::Number(n) if n.is_u64() || n.is_i64() => Some(n.to_string()),
            Value::String(s) => Some(s),
            _ => None,
        });
    (dna, number)
}
async fn object(State(state): State<Arc<AppState>>, Path(raw): Path<String>) -> ApiResult<Value> {
    tokio::task::spawn_blocking(move || object_read(state, raw))
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
}
fn object_read(state: Arc<AppState>, raw: String) -> ApiResult<Value> {
    let id = parse_hash32(&raw, "spore ID")?;
    let entry = state
        .store
        .get_spore(&id)
        .map_err(|e| ApiError::internal(e.to_string()))?
        .filter(|entry| entry.standard == ObjectStandard::Spore)
        .ok_or_else(|| ApiError::not_found("Spore not found"))?;
    let result = (|| -> Result<Value> {
        let parsed = spore_content(&state, &id)?;
        if !matches!(
            ckbadger_indexer::sync::dob_decode_worker::parse_dob_version_from_content_type(
                &parsed.content_type
            ),
            Some(0 | 1)
        ) {
            return Ok(
                json!({"dnaHex":null,"number":null,"cachedVmTraits":{"status":"notDob","decodedAt":null,"traits":[],"source":"cachedVm","originalDecoderHashRecorded":false},"currentPattern":{"status":"notDob","steps":[]}}),
            );
        }
        let (dna, number) = item_dna_number(&parsed.content_type, &parsed.content);
        let bytes = dna
            .as_ref()
            .map(|d| hex::decode(d.trim_start_matches("0x")))
            .transpose()?;
        let cluster = entry
            .collection_id
            .as_ref()
            .map(|id| state.store.get_spore(id))
            .transpose()?
            .flatten();
        Ok(
            json!({"dnaHex":dna,"number":number,"cachedVmTraits":cached_traits(&state,&id)?,"currentPattern":current_pattern(&state,cluster.as_ref(),bytes.as_deref())?}),
        )
    })();
    ok(result.map_err(|e| ApiError::internal(format!("spore=0x{}: {e:#}", hex::encode(id))))?)
}

async fn insights(State(state): State<Arc<AppState>>, Path(raw): Path<String>) -> ApiResult<Value> {
    tokio::task::spawn_blocking(move || insights_read(state, raw))
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
}
fn insights_read(state: Arc<AppState>, raw: String) -> ApiResult<Value> {
    let id = super::spore::parse_cluster_id_param(&raw)?;
    let (cluster, _) = super::spore::read_cluster(&state, &id)?;
    let result = (|| -> Result<Value> {
        let items = cluster_items(&state, &id)?;
        let mut owners: BTreeMap<Vec<u8>, u64> = BTreeMap::new();
        let mut distribution: BTreeMap<String, BTreeMap<String, u64>> = BTreeMap::new();
        let (mut live, mut decoded, mut pending, mut failed, mut non_dob) = (0, 0, 0, 0, 0);
        for (item_id, item) in &items {
            if !item.is_live {
                continue;
            }
            live += 1;
            let owner = item
                .owner_lock_hash
                .as_ref()
                .context("live Spore owner missing")?;
            *owners.entry(owner.clone()).or_default() += 1;
            if !is_dob(item)? {
                non_dob += 1;
                continue;
            }
            match state.store.get_dob_decode_outcome(item_id)? {
                Some(DecodeOutcome::Decoded(entry)) => {
                    decoded += 1;
                    let values: std::collections::BTreeSet<_> = merged_traits(&entry)
                        .into_iter()
                        .map(|trait_value| (trait_value.name, trait_value.value))
                        .collect();
                    for (name, value) in values {
                        *distribution
                            .entry(name)
                            .or_default()
                            .entry(value)
                            .or_default() += 1
                    }
                }
                Some(DecodeOutcome::Failed(_)) => failed += 1,
                None => pending += 1,
            }
        }
        let ranges = ["1", "2", "3–5", "6+"];
        let mut buckets = [(0u64, 0u64); 4];
        for count in owners.values() {
            let i = match count {
                1 => 0,
                2 => 1,
                3..=5 => 2,
                _ => 3,
            };
            buckets[i].0 += 1;
            buckets[i].1 += count;
        }
        let holders: Vec<_> = ranges
            .iter()
            .zip(buckets)
            .map(|(range, (holders, items))| json!({"range":range,"holders":holders,"items":items}))
            .collect();
        let traits: Vec<_> = if pending == 0 && failed == 0 {
            distribution.into_iter().map(|(name,values)|json!({"name":name,"values":values.into_iter().map(|(value,count)|json!({"value":value,"count":count})).collect::<Vec<_>>()})).collect()
        } else {
            Vec::new()
        };
        let mut weeks: BTreeMap<String, [u64; 3]> = BTreeMap::new();
        let mut cursor = None;
        loop {
            let rows = super::assets::list_canonical_nft_collection_activities_page(
                &state.store,
                &state.append_only_store,
                &id,
                1024,
                cursor,
                None,
            )?;
            if rows.is_empty() {
                break;
            }
            cursor = rows.last().map(|(b, i, _)| (*b, *i));
            for (block, _, activity) in rows {
                let header = state
                    .store
                    .get_block_header(block)?
                    .context("collection activity block missing")?;
                let date = ckbadger_common::stats_bucket::stats_day(header.chain_time_ms)?;
                use chrono::Datelike;
                let monday =
                    date - chrono::Duration::days(date.weekday().num_days_from_monday() as i64);
                let counts = weeks.entry(monday.to_string()).or_default();
                for (action, slot) in [
                    (ckbadger_store::AssetAction::Mint, 0),
                    (ckbadger_store::AssetAction::Transfer, 1),
                    (ckbadger_store::AssetAction::Burn, 2),
                ] {
                    if activity.actions.contains(&action) {
                        counts[slot] += 1
                    }
                }
            }
        }
        let (complete_through, calendar_anchor) = if let Some((tip, header)) =
            state.store.get_sync_tip_block()?
        {
            let tip_hash: [u8; 32] = header
                .hash
                .as_slice()
                .try_into()
                .context("calendar anchor hash width")?;
            let chain = crate::utils::stats_bucket::ChainTime::from_header(
                tip,
                header.timestamp,
                header.chain_time_ms,
                tip_hash,
            )
            .map_err(|(_, error)| anyhow::anyhow!(error.0.message))?;
            let day = chain
                .tip_day()
                .map_err(|(_, error)| anyhow::anyhow!(error.0.message))?;
            use chrono::Datelike;
            let current_monday =
                day - chrono::Duration::days(day.weekday().num_days_from_monday() as i64);
            let last_sunday = current_monday
                .pred_opt()
                .context("weekly cutoff underflow")?;
            weeks.retain(|start, _| start.as_str() < current_monday.to_string().as_str());
            if let Some(first) = weeks.keys().next().cloned() {
                let mut week = chrono::NaiveDate::parse_from_str(&first, "%Y-%m-%d")?;
                while week < current_monday {
                    weeks.entry(week.to_string()).or_default();
                    week = week
                        .checked_add_signed(chrono::Duration::weeks(1))
                        .context("weekly range overflow")?;
                }
            }
            (
                Some(last_sunday.to_string()),
                json!({"blockNumber":tip,"blockHash":format!("0x{}",hex::encode(header.hash)),"chainTime":header.chain_time_ms.to_string()}),
            )
        } else {
            ensure!(
                weeks.is_empty(),
                "collection activity calendar has no canonical tip"
            );
            (None, Value::Null)
        };
        let weekly:Vec<_>=weeks.into_iter().map(|(week,n)|json!({"weekStart":week,"mintTransactions":n[0],"transferTransactions":n[1],"burnTransactions":n[2]})).collect();
        Ok(
            json!({"holderDistribution":holders,"weeklyActivity":weekly,"calendarSource":"indexedChainTimeUtc8","completeThrough":complete_through,"calendarAnchor":calendar_anchor,"genetics":{"liveItems":live,"eligibleItems":live-non_dob,"nonDobItems":non_dob,"decodedItems":decoded,"pendingItems":pending,"failedItems":failed,"complete":pending==0&&failed==0,"traits":traits},"currentPattern":current_pattern(&state,cluster.as_ref(),None)?}),
        )
    })();
    ok(result.map_err(|e| ApiError::internal(format!("cluster=0x{}: {e:#}", hex::encode(id))))?)
}

#[derive(Deserialize)]
struct GalleryParams {
    #[serde(default = "default_limit")]
    limit: i64,
    cursor: Option<String>,
    traits: Option<String>,
}
async fn gallery(
    State(state): State<Arc<AppState>>,
    Path(raw): Path<String>,
    Query(params): Query<GalleryParams>,
) -> ApiResult<CursorPaginatedResponse<Value>> {
    tokio::task::spawn_blocking(move || gallery_read(state, raw, params))
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
}
fn gallery_read(
    state: Arc<AppState>,
    raw: String,
    params: GalleryParams,
) -> ApiResult<CursorPaginatedResponse<Value>> {
    let id = super::spore::parse_cluster_id_param(&raw)?;
    super::spore::read_cluster(&state, &id)?;
    let limit = params.limit.clamp(1, 100);
    let cursor = crate::utils::params::parse_optional_block_id_cursor(
        params.cursor.as_deref(),
        "gallery cursor",
    )?;
    let filters: BTreeMap<String, String> = params
        .traits
        .as_deref()
        .map(serde_json::from_str)
        .transpose()
        .map_err(|e| ApiError::bad_request(format!("traits must be a JSON string map: {e}")))?
        .unwrap_or_default();
    let mut matches = Vec::new();
    for (item_id, entry) in
        cluster_items(&state, &id).map_err(|e| ApiError::internal(format!("{e:#}")))?
    {
        if !entry.is_live {
            continue;
        }
        let eligible = is_dob(&entry).map_err(|e| ApiError::internal(e.to_string()))?;
        if !eligible && !filters.is_empty() {
            continue;
        }
        let traits = if eligible {
            cached_traits(&state, &item_id).map_err(|e| ApiError::internal(e.to_string()))?
        } else {
            not_dob_traits()
        };
        if !filters.is_empty() && traits["status"] != "decoded" {
            return Err(ApiError::service_unavailable(
                "Trait filtering requires complete live-collection VM decode coverage",
            ));
        }
        let values = traits["traits"].as_array().expect("trait array");
        if filters.iter().all(|(name, value)| {
            values
                .iter()
                .any(|t| t["name"] == *name && t["value"] == *value)
        }) {
            matches.push((item_id, entry, traits));
        }
    }
    let total = matches.len() as i64;
    if let Some(c) = &cursor {
        if !matches
            .iter()
            .any(|(id, e, _)| e.created_at_block == c.0 && *id == c.1)
        {
            return Err(ApiError::bad_request(
                "gallery cursor not in filtered scope",
            ));
        }
    }
    let mut selected: Vec<_> = matches
        .into_iter()
        .filter(|(id, e, _)| {
            cursor.as_ref().is_none_or(|c| {
                e.created_at_block < c.0 || (e.created_at_block == c.0 && *id > c.1)
            })
        })
        .take(limit as usize + 1)
        .collect();
    let more = selected.len() > limit as usize;
    selected.truncate(limit as usize);
    let next = if more {
        selected
            .last()
            .map(|(id, e, _)| format!("{}:0x{}", e.created_at_block, hex::encode(id)))
    } else {
        None
    };
    let mut data = Vec::new();
    for (item_id, entry, traits) in selected {
        let parsed =
            spore_content(&state, &item_id).map_err(|e| ApiError::internal(format!("{e:#}")))?;
        let (dna, number) = item_dna_number(&parsed.content_type, &parsed.content);
        let versions =
            asset_history::checked_versions(&state, &item_id, NftLifecycleStandard::Spore)?;
        let current = versions
            .iter()
            .find(|v| v.response.is_live)
            .ok_or_else(|| ApiError::internal("live gallery item has no live cell"))?;
        let mut item = serde_json::to_value(super::spore::spore_to_response(&item_id, &entry)?)
            .map_err(|e| ApiError::internal(e.to_string()))?;
        let object = item.as_object_mut().expect("Spore response object");
        object.insert("dnaHex".into(), json!(dna));
        object.insert("number".into(), json!(number));
        object.insert("traits".into(), traits["traits"].clone());
        object.insert("decodeStatus".into(), traits["status"].clone());
        object.insert("ownerAddress".into(), json!(current.response.owner_address));
        object.insert("txHash".into(), json!(current.response.tx_hash));
        object.insert("outputIndex".into(), json!(current.response.output_index));
        data.push(item);
    }
    ok(CursorPaginatedResponse::new(data, total, limit, next))
}

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/spore/objects/{spore_id}/genetics", get(object))
        .route("/spore/clusters/{cluster_id}/insights", get(insights))
        .route("/spore/clusters/{cluster_id}/gallery", get(gallery))
}

#[cfg(test)]
mod tests {
    #[test]
    fn vm_trait_groups_preserve_same_step_values_and_latest_step_overrides_group() {
        use ckbadger_store::{DobDecodedEntry, DobDecodedStep, DobDecodedTrait};
        let step = |index, values: Vec<(&str, &str)>| DobDecodedStep {
            step: index,
            media_type: "application/json".into(),
            size: 0,
            hash: String::new(),
            traits: values
                .into_iter()
                .map(|(name, value)| DobDecodedTrait {
                    name: name.into(),
                    value: value.into(),
                })
                .collect(),
        };
        let entry = DobDecodedEntry {
            steps: vec![
                step(0, vec![("Face", "Old"), ("Face", "Old2"), ("Color", "Red")]),
                step(
                    1,
                    vec![("Face", "Smile"), ("Face", "Wide"), ("Face", "Smile")],
                ),
            ],
            media_sources: vec![],
            decoded_at: 1,
        };
        let got: Vec<_> = super::merged_traits(&entry)
            .into_iter()
            .map(|t| (t.name, t.value))
            .collect();
        assert_eq!(
            got,
            [
                ("Color", "Red"),
                ("Face", "Smile"),
                ("Face", "Wide"),
                ("Face", "Smile")
            ]
            .map(|(n, v)| (n.into(), v.into()))
        );
    }
    #[test]
    fn dna_number_extraction_requires_dob_content_type() {
        assert_eq!(
            super::item_dna_number("image/svg+xml", b"00aabb"),
            (None, None)
        );
        assert_eq!(
            super::item_dna_number("dob/0", br#"{"id":18446744073709551615,"dna":"00aabb"}"#),
            (Some("0x00aabb".into()), Some("18446744073709551615".into()))
        );
    }
}
