#![allow(clippy::type_complexity)]

use axum::{
    extract::{Path, Query, State},
    routing::{get, post},
    Json, Router,
};
use ckb_types::{packed, prelude::*};
use ckbadger_store::{ScriptFamilyMembershipIndex, ScriptFamilyUsage};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

use super::statistics::{StackedAreaChartResponse, StackedAreaDataPoint, StackedAreaSeries};
use crate::response::{
    decode_cursor_single, default_limit, encode_cursor_single, ok, ApiError, ApiResult,
    ApiRouteError, CursorPaginatedResponse, ScriptCodeClassTotalResponse,
    ScriptCustodyOwnerResponse, ScriptCustodyResponse, ScriptFamiliesListResponse,
    ScriptFamilyDetailResponse, ScriptFamilyListItemResponse, ScriptHistoryMarkerResponse,
    ScriptObservedReferenceResponse, ScriptRoleCountsResponse, ScriptVersionDeploymentResponse,
    ScriptVersionDetailResponse,
};
use crate::utils::script_resolution::resolve_dep_cells_for_transaction;
use crate::utils::{
    apply_owned_capacity_delta, date_keys_inclusive, deployment_reference_hashes,
    hash_type_to_string, hash_type_to_u8, list_version_code_cells, merge_script_info_for_reference,
    parse_chart_date_range, parse_hash32, reference_form_member_version, resolve_script_by_hash,
    resolve_script_form_by_hash, CurrentScriptVersionResolution, VersionCodeCell,
};
use crate::warmup::{CACHE_KEY_SCRIPTS_ALL, CACHE_KEY_SCRIPT_FAMILIES_ALL};
use crate::AppState;

fn load_script_infos_cached(
    state: &Arc<AppState>,
) -> Result<Vec<(Vec<u8>, ckbadger_store::ScriptInfo)>, ApiRouteError> {
    state
        .mem_cache
        .get::<Vec<(Vec<u8>, ckbadger_store::ScriptInfo)>>(CACHE_KEY_SCRIPTS_ALL)
        .ok_or_else(|| ApiError::warmup_pending("script cache unavailable; warmup in progress"))
}

pub(crate) fn load_script_families_cached(
    state: &Arc<AppState>,
) -> Result<Vec<(String, ckbadger_store::types::ScriptFamilyInfo)>, ApiRouteError> {
    state
        .mem_cache
        .get::<Vec<(String, ckbadger_store::types::ScriptFamilyInfo)>>(
            CACHE_KEY_SCRIPT_FAMILIES_ALL,
        )
        .ok_or_else(|| ApiError::warmup_pending("script cache unavailable; warmup in progress"))
}

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/scripts", get(list_scripts))
        .route("/scripts/lookup", post(lookup_scripts))
        .route("/scripts/code-cell", get(get_code_cell))
        .route("/scripts/code-cells", get(get_code_cells))
        .route(
            "/scripts/charts/capacity-history",
            get(get_script_capacity_history_chart_by_code_hash),
        )
        .route("/scripts/{name}", get(get_script))
        .route("/scripts/{name}/usage", get(get_script_usage))
        .route(
            "/scripts/{name}/charts/capacity-history",
            get(get_script_capacity_history_chart),
        )
}

#[derive(Debug, Deserialize)]
pub struct ListParams {
    #[serde(default = "default_limit")]
    limit: i64,
    cursor: Option<String>,
    network: Option<String>,
    #[serde(rename = "decoder_type")]
    _decoder_type: Option<String>,
    search: Option<String>,
    role: Option<String>,
    #[serde(default = "default_script_sort_key")]
    sort_key: ScriptSortKey,
    #[serde(default = "default_sort_direction")]
    sort_direction: SortDirection,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ScriptSortKey {
    Name,
    UsedAs,
    Description,
    Used,
    Capacity,
    UsedRatio,
    LiveCells,
    Cells,
}

fn default_script_sort_key() -> ScriptSortKey {
    ScriptSortKey::Name
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
enum SortDirection {
    Asc,
    Desc,
}

fn default_sort_direction() -> SortDirection {
    SortDirection::Asc
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptResolutionAmbiguityResponse {
    pub version_hashes: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptUsageResponse {
    pub name: String,
    pub cells_count: i64,
    pub live_cells_count: i64,
    pub capacity_sum: String,
    pub owned_capacity_sum: String,
    #[serde(rename = "commonKnowledgeSizeSum")]
    pub used_capacity_sum: String,
    pub owned_knowledge_sum: String,
    /// Current binary bindings only; retired family references remain in the
    /// top-level totals and are not assigned to a historical binary.
    pub by_deployment: Vec<DeploymentUsage>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeploymentUsage {
    pub code_hash: String,
    pub script_kind: Option<String>,
    pub cells_count: i64,
    pub live_cells_count: i64,
    pub capacity_sum: String,
    pub owned_capacity_sum: String,
    #[serde(rename = "commonKnowledgeSizeSum")]
    pub used_capacity_sum: String,
    pub owned_knowledge_sum: String,
}

/// Request body for bulk script lookup by code_hash
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LookupScriptsRequest {
    /// List of code_hash values (hex strings with 0x prefix)
    pub code_hashes: Vec<String>,
    #[serde(default)]
    pub tx_hash: Option<String>,
}

/// Lightweight script info for lookup results
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptLookupInfo {
    pub reference_hash: String,
    pub code_hash: String,
    pub name: String,
    /// The queried reference's own label description (`ScriptInfo.description`);
    /// `null` when the reference has none.
    pub description: Option<String>,
    pub deprecated: bool,
    pub script_kind: Option<String>,
    pub decoder_type: Option<String>,
    pub hash_type: Option<String>,
    pub deployment_type_hash: Option<String>,
    pub deployment_data_hash: Option<String>,
    pub code_cell_tx_hash: Option<String>,
    pub code_cell_output_index: Option<i32>,
    pub live_cells_count: i64,
    pub owned_capacity_sum: String,
    pub owned_knowledge_sum: String,
    pub code_cells_live_count: i64,
    pub code_cells_total: i64,
    pub resolution_state: String,
    pub ambiguity: Option<ScriptResolutionAmbiguityResponse>,
}

/// Resolve the deployment code cell outpoint for a script.
///
/// Tries type-ref lookup first, then data-hash lookup as fallback (not mutually exclusive).
/// Both paths use ckbadger's own domain store indexes — no dependency on CKB node RocksDB.
#[cfg_attr(not(test), allow(dead_code))]
fn resolve_code_cell(
    info: &ckbadger_store::ScriptInfo,
    store: &ckbadger_store::CkbadgerStore,
    cells_store: &ckbadger_store::CkbadgerStore,
) -> Result<(Option<String>, Option<i32>), ApiRouteError> {
    let (type_ref, data_ref) = deployment_reference_hashes(info);

    // Step 1: try type-ref lookup (CF_CELL_BY_TYPE)
    if let Some(type_hash) = type_ref.as_deref() {
        let cells = store
            .list_cells_by_type(type_hash, 1, None, cells_store)
            .map_err(|e| {
                ApiError::internal(format!(
                    "failed to resolve code cell by deployment type hash 0x{}: {}",
                    hex::encode(type_hash),
                    e
                ))
            })?;
        if let Some((tx_hash, idx, _)) = cells.first() {
            let output_index = i32::from(*idx);
            return Ok((
                Some(format!("0x{}", hex::encode(tx_hash))),
                Some(output_index),
            ));
        }
    }

    // Step 2: try data-hash lookup (CF_CELL_BY_DATA_HASH) — runs even if type-ref was attempted.
    // Uses find_any_cell_by_data_hash which checks both live and consumed cells,
    // since code cells may have been consumed while the script is still in use.
    if let Some(data_hash) = data_ref.as_deref() {
        if let Some((tx_hash, idx, _)) = store
            .find_any_cell_by_data_hash(data_hash, cells_store)
            .map_err(|e| {
                ApiError::internal(format!(
                    "failed to resolve code cell by deployment data hash 0x{}: {}",
                    hex::encode(data_hash),
                    e
                ))
            })?
        {
            let output_index = i32::from(idx);
            return Ok((
                Some(format!("0x{}", hex::encode(tx_hash))),
                Some(output_index),
            ));
        }
    }

    // Step 3: use the imported outpoint when no live deployment cell found.
    if let (Some(tx_hash), Some(idx)) = (&info.code_cell_tx_hash, info.code_cell_output_index) {
        if !tx_hash.is_empty() {
            return Ok((
                Some(format!("0x{}", hex::encode(tx_hash))),
                Some(idx as i32),
            ));
        }
    }

    Ok((None, None))
}

fn script_kind_from_counts(lock_cells_count: i64, type_cells_count: i64) -> Option<String> {
    match (lock_cells_count > 0, type_cells_count > 0) {
        (true, true) => Some("lock+type".to_string()),
        (true, false) => Some("lock".to_string()),
        (false, true) => Some("type".to_string()),
        (false, false) => None,
    }
}

/// Usage is the authority once the chain observes a role. An unused family
/// uses its declared role from the same effective bundled/workdir registry
/// metadata as label import. Check names so stale declarations fail loudly.
fn script_kind_for_family(
    state: &AppState,
    info: &ckbadger_store::types::ScriptFamilyInfo,
) -> Result<Option<String>, ApiRouteError> {
    if let Some(observed) = script_kind_from_counts(info.lock_cells_count, info.type_cells_count) {
        return Ok(Some(observed));
    }
    let Some((name, role)) = state.script_roles.get(&info.family_id) else {
        return Ok(None);
    };
    if name != &info.name {
        return Err(ApiError::internal(format!(
            "effective script role name conflicts with imported family {} ({})",
            info.family_id, info.name
        )));
    }
    Ok(Some(role.clone()))
}

const SECP_TYPE_HASH: &str = "9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8";
const MULTISIG_TYPE_HASHES: [&str; 2] = [
    "5c5069eb0857efc65e1bca0c07df34c31663b3622fd3876c876320fc9634e2a8",
    "d1a9f877aed3f5e07cb9c52b61ab96d06f250ae6883cc7f0a2423db0976fc821",
];
const MULTISIG_DATA1_HASH: &str =
    "36c971b8d41fbd94aabca77dc75e826729ac98447b46f91e00796155dddb0d29";

fn custody_without_witness(
    class: &str,
    cell: Option<&ckbadger_store::types::LiveCellInfo>,
) -> ScriptCustodyResponse {
    ScriptCustodyResponse {
        class: class.to_string(),
        lock_code_hash: cell.map(|c| format!("0x{}", hex::encode(&c.lock_code_hash))),
        lock_hash_type: cell
            .and_then(|c| u8::try_from(c.lock_hash_type).ok())
            .and_then(hash_type_to_string)
            .map(str::to_string),
        lock_args: cell.map(|c| format!("0x{}", hex::encode(&c.lock_args))),
        threshold: None,
        key_count: None,
        require_first_n: None,
        key_hashes: Vec::new(),
        verified_by_tx: None,
        witness_state: if class == "fixed" || class == "unspendable" {
            "not_applicable"
        } else {
            "unavailable"
        }
        .to_string(),
        owners: Vec::new(),
    }
}

fn classify_code_cell_lock(
    cell: &ckbadger_store::types::LiveCellInfo,
) -> Result<ScriptCustodyResponse, ApiRouteError> {
    let kind = match (cell.lock_code_hash.as_slice(), cell.lock_hash_type) {
        (hash, 0) if hash == [0u8; 32] => "unspendable",
        (hash, 1) if hex::encode(hash) == SECP_TYPE_HASH && cell.lock_args.len() == 20 => "one_key",
        (hash, 1)
            if MULTISIG_TYPE_HASHES.contains(&hex::encode(hash).as_str())
                && matches!(cell.lock_args.len(), 20 | 28) =>
        {
            "multisig"
        }
        (hash, 2)
            if hex::encode(hash) == MULTISIG_DATA1_HASH
                && matches!(cell.lock_args.len(), 20 | 28) =>
        {
            "multisig"
        }
        _ => "unsupported",
    };
    if hash_type_to_string(u8::try_from(cell.lock_hash_type).map_err(|_| {
        ApiError::internal(format!(
            "invalid code-cell lock hash type {}",
            cell.lock_hash_type
        ))
    })?)
    .is_none()
    {
        return Err(ApiError::internal(format!(
            "invalid code-cell lock hash type {}",
            cell.lock_hash_type
        )));
    }
    Ok(custody_without_witness(kind, Some(cell)))
}

/// A lock cannot have supplied a signing witness if none of its cells has ever
/// been consumed. The address balance counts *all* cells under this exact lock
/// hash, including non-code cells; a missing row is insufficient evidence.
fn mark_never_signed_if_proven(
    state: &AppState,
    cell: &ckbadger_store::types::LiveCellInfo,
    custody: &mut ScriptCustodyResponse,
) -> Result<(), ApiRouteError> {
    if custody.class != "multisig" {
        return Ok(());
    }
    let balance = state
        .store
        .get_addr_balance(&cell.lock_script_hash)
        .map_err(|e| {
            ApiError::internal(format!(
                "custodian address balance 0x{}: {e}",
                hex::encode(&cell.lock_script_hash)
            ))
        })?;
    if let Some(balance) = balance {
        if balance.total_cells_count < i64::from(balance.live_cells_count)
            || balance.live_cells_count < 0
        {
            return Err(ApiError::internal(format!(
                "invalid custodian cell counts at lock 0x{}: total={} live={}",
                hex::encode(&cell.lock_script_hash),
                balance.total_cells_count,
                balance.live_cells_count
            )));
        }
        if balance.total_cells_count > 0
            && balance.total_cells_count == i64::from(balance.live_cells_count)
        {
            custody.witness_state = "never_signed".to_string();
        }
    }
    Ok(())
}

/// The legacy and v2 system multisig locks commit the S/R/M/N config plus N
/// key hashes in the first twenty bytes of lock args. Signatures follow that
/// config in the first group witness and are deliberately excluded from the
/// committed hash. See ckb-system-scripts/secp256k1_blake160_multisig_all.c.
fn verified_multisig_config(
    lock_args: &[u8],
    witness: &[u8],
    tx_hash: &[u8],
) -> Result<ScriptCustodyResponse, ApiRouteError> {
    let parsed = packed::WitnessArgs::from_slice(witness).map_err(|e| {
        ApiError::internal(format!(
            "multisig witness in tx 0x{} is invalid WitnessArgs: {e}",
            hex::encode(tx_hash)
        ))
    })?;
    let lock = parsed.lock().to_opt().ok_or_else(|| {
        ApiError::internal(format!(
            "multisig witness in tx 0x{} has no lock field",
            hex::encode(tx_hash)
        ))
    })?;
    let bytes = lock.raw_data();
    if bytes.len() < 4 {
        return Err(ApiError::internal(format!(
            "multisig witness in tx 0x{} lacks S/R/M/N",
            hex::encode(tx_hash)
        )));
    }
    let (s, r, m, n) = (bytes[0], bytes[1], bytes[2], bytes[3]);
    if s != 0 || n == 0 || m == 0 || m > n || r > m {
        return Err(ApiError::internal(format!(
            "invalid multisig config in tx 0x{}: S={s} R={r} M={m} N={n}",
            hex::encode(tx_hash)
        )));
    }
    let config_len = 4usize + 20usize * n as usize;
    let expected_len = config_len + 65usize * m as usize;
    if bytes.len() != expected_len {
        return Err(ApiError::internal(format!(
            "multisig witness length mismatch in tx 0x{}: got {} expected {expected_len}",
            hex::encode(tx_hash),
            bytes.len()
        )));
    }
    if !matches!(lock_args.len(), 20 | 28)
        || ckb_hash::blake2b_256(&bytes[..config_len])[..20] != lock_args[..20]
    {
        return Err(ApiError::internal(format!(
            "multisig config hash differs from lock args in tx 0x{}",
            hex::encode(tx_hash)
        )));
    }
    Ok(ScriptCustodyResponse {
        class: "multisig".to_string(),
        lock_code_hash: None,
        lock_hash_type: None,
        lock_args: Some(format!("0x{}", hex::encode(lock_args))),
        threshold: Some(m),
        key_count: Some(n),
        require_first_n: Some(r),
        key_hashes: bytes[4..config_len]
            .as_chunks::<20>()
            .0
            .iter()
            .map(|key| format!("0x{}", hex::encode(key)))
            .collect(),
        verified_by_tx: Some(format!("0x{}", hex::encode(tx_hash))),
        witness_state: "verified".to_string(),
        owners: Vec::new(),
    })
}

fn attach_multisig_witness(
    state: &AppState,
    tx_hash: &[u8],
    output_index: i16,
    cell: &ckbadger_store::types::LiveCellInfo,
    consuming_tx: Option<&[u8]>,
    custody: &mut ScriptCustodyResponse,
) -> Result<(), ApiRouteError> {
    if custody.class != "multisig" {
        return Ok(());
    }
    let Some(consuming_tx) = consuming_tx else {
        return Ok(());
    };
    let Some(reader) = &state.ckb_store else {
        return Ok(());
    };
    let hash: [u8; 32] = consuming_tx.try_into().map_err(|_| {
        ApiError::internal(format!(
            "invalid consuming tx hash for 0x{}:{}",
            hex::encode(tx_hash),
            output_index
        ))
    })?;
    let view = reader.get_transaction(&hash).ok_or_else(|| {
        ApiError::internal(format!(
            "missing committed upgrade tx 0x{} from node store",
            hex::encode(consuming_tx)
        ))
    })?;
    let tx = ckb_store_reader::convert_transaction_view(&view);
    let mut first_group_input = None;
    let mut target_seen = false;
    for (index, input) in tx.inputs.iter().enumerate() {
        let input_hash = hex::decode(input.previous_output.tx_hash.trim_start_matches("0x"))
            .map_err(|e| {
                ApiError::internal(format!(
                    "invalid upgrade input hash in 0x{}: {e}",
                    hex::encode(consuming_tx)
                ))
            })?;
        let input_index =
            u32::from_str_radix(input.previous_output.index.trim_start_matches("0x"), 16).map_err(
                |e| {
                    ApiError::internal(format!(
                        "invalid upgrade input index in 0x{}: {e}",
                        hex::encode(consuming_tx)
                    ))
                },
            )?;
        let input_index = i16::try_from(input_index).map_err(|_| {
            ApiError::internal(format!(
                "upgrade input index exceeds i16 in 0x{}",
                hex::encode(consuming_tx)
            ))
        })?;
        if input_hash == tx_hash && input_index == output_index {
            target_seen = true;
        }
        let input_cell = state
            .append_only_store
            .get_cell_by_outpoint_key(&ckbadger_store::keys::encode_outpoint(
                &input_hash,
                input_index,
            ))
            .map_err(|e| {
                ApiError::internal(format!(
                    "upgrade input cell in 0x{}: {e}",
                    hex::encode(consuming_tx)
                ))
            })?
            .ok_or_else(|| {
                ApiError::internal(format!(
                    "missing canonical upgrade input 0x{}:{} in tx 0x{}",
                    hex::encode(input_hash),
                    input_index,
                    hex::encode(consuming_tx)
                ))
            })?;
        if input_cell.lock_script_hash == cell.lock_script_hash && first_group_input.is_none() {
            first_group_input = Some(index);
        }
    }
    if !target_seen {
        return Err(ApiError::internal(format!(
            "consuming tx 0x{} does not input code cell 0x{}:{}",
            hex::encode(consuming_tx),
            hex::encode(tx_hash),
            output_index
        )));
    }
    let witness_index = first_group_input.ok_or_else(|| {
        ApiError::internal(format!(
            "no lock group for code cell 0x{}:{} in 0x{}",
            hex::encode(tx_hash),
            output_index,
            hex::encode(consuming_tx)
        ))
    })?;
    let witness = tx.witnesses.get(witness_index).ok_or_else(|| {
        ApiError::internal(format!(
            "missing multisig witness {witness_index} in 0x{}",
            hex::encode(consuming_tx)
        ))
    })?;
    let witness = hex::decode(witness.trim_start_matches("0x")).map_err(|e| {
        ApiError::internal(format!(
            "invalid witness hex in 0x{}: {e}",
            hex::encode(consuming_tx)
        ))
    })?;
    let verified = verified_multisig_config(&cell.lock_args, &witness, consuming_tx)?;
    custody.threshold = verified.threshold;
    custody.key_count = verified.key_count;
    custody.require_first_n = verified.require_first_n;
    custody.key_hashes = verified.key_hashes;
    custody.verified_by_tx = verified.verified_by_tx;
    custody.witness_state = verified.witness_state;
    Ok(())
}

fn reference_custody(
    state: &AppState,
    reference_hash: &[u8],
    hash_type: u8,
) -> Result<ScriptCustodyResponse, ApiRouteError> {
    match hash_type {
        0 | 2 | 4 => Ok(custody_without_witness("fixed", None)),
        1 => {
            let current = reference_form_member_version(
                &state.store,
                &state.append_only_store,
                hash_type,
                reference_hash,
                &|_| true,
            )
            .map_err(|e| {
                ApiError::internal(format!(
                    "current code binding for type reference 0x{}: {e}",
                    hex::encode(reference_hash)
                ))
            })?;
            if current.is_none() {
                return Ok(custody_without_witness("retired", None));
            }
            let cells = state
                .store
                .list_cells_by_type(reference_hash, usize::MAX, None, &state.append_only_store)
                .map_err(|e| {
                    ApiError::internal(format!(
                        "code cells for type reference 0x{}: {e}",
                        hex::encode(reference_hash)
                    ))
                })?;
            if cells.is_empty() {
                return Ok(custody_without_witness("retired", None));
            }
            let mut version = None;
            let mut owners: BTreeMap<(Vec<u8>, i16, Vec<u8>), (String, Vec<String>, bool)> =
                BTreeMap::new();
            for (tx_hash, output_index, cell) in cells {
                let data_hash = cell.data_hash.as_ref().ok_or_else(|| {
                    ApiError::internal(format!(
                        "live code cell 0x{}:{} for type reference 0x{} lacks data hash",
                        hex::encode(&tx_hash),
                        output_index,
                        hex::encode(reference_hash)
                    ))
                })?;
                if version
                    .as_ref()
                    .is_some_and(|previous| previous != data_hash)
                {
                    return Ok(custody_without_witness("ambiguous", None));
                }
                version = Some(data_hash.clone());
                let mut custody = classify_code_cell_lock(&cell.cell)?;
                mark_never_signed_if_proven(state, &cell.cell, &mut custody)?;
                let key = (
                    cell.lock_code_hash.clone(),
                    cell.lock_hash_type,
                    cell.lock_args.clone(),
                );
                let entry = owners.entry(key).or_insert_with(|| {
                    (
                        custody.class,
                        Vec::new(),
                        custody.witness_state == "never_signed",
                    )
                });
                entry
                    .1
                    .push(format!("0x{}:{}", hex::encode(&tx_hash), output_index));
            }
            let mut owner_rows = Vec::with_capacity(owners.len());
            let single_owner_never_signed =
                owners.len() == 1 && owners.values().next().is_some_and(|(_, _, never)| *never);
            for ((code_hash, hash_type, args), (class, code_cells, _)) in owners {
                owner_rows.push(ScriptCustodyOwnerResponse {
                    lock_code_hash: format!("0x{}", hex::encode(code_hash)),
                    lock_hash_type: hash_type_to_string(u8::try_from(hash_type).map_err(|_| {
                        ApiError::internal(format!(
                            "invalid lock hash type {hash_type} for reference 0x{}",
                            hex::encode(reference_hash)
                        ))
                    })?)
                    .ok_or_else(|| {
                        ApiError::internal(format!(
                            "invalid lock hash type {hash_type} for reference 0x{}",
                            hex::encode(reference_hash)
                        ))
                    })?
                    .to_string(),
                    lock_args: format!("0x{}", hex::encode(args)),
                    class,
                    code_cells,
                });
            }
            let mut custody = if owner_rows.len() == 1 {
                let owner = &owner_rows[0];
                ScriptCustodyResponse {
                    class: owner.class.clone(),
                    lock_code_hash: Some(owner.lock_code_hash.clone()),
                    lock_hash_type: Some(owner.lock_hash_type.clone()),
                    lock_args: Some(owner.lock_args.clone()),
                    ..custody_without_witness(&owner.class, None)
                }
            } else {
                custody_without_witness("mixed", None)
            };
            if single_owner_never_signed {
                custody.witness_state = "never_signed".to_string();
            }
            custody.owners = owner_rows;
            Ok(custody)
        }
        _ => Err(ApiError::internal(format!(
            "unknown script reference hash type {hash_type}: 0x{}",
            hex::encode(reference_hash)
        ))),
    }
}

fn version_totals(
    version: &ckbadger_store::types::ScriptVersionInfo,
) -> (i64, i64, i128, i128, i128, i128) {
    (
        version.lock_cells_count + version.type_cells_count,
        version.lock_live_cells_count + version.type_live_cells_count,
        version.lock_capacity_sum + version.type_capacity_sum,
        version.lock_owned_capacity_sum + version.type_owned_capacity_sum,
        version.lock_used_capacity_sum + version.type_used_capacity_sum,
        version.lock_owned_knowledge_sum + version.type_owned_knowledge_sum,
    )
}

fn version_script_kind(info: &ckbadger_store::types::ScriptVersionInfo) -> Option<String> {
    script_kind_from_counts(info.lock_cells_count, info.type_cells_count)
}

fn sort_code_cells(code_cells: &mut [VersionCodeCell]) {
    code_cells.sort_by(|left, right| {
        right
            .3
            .cmp(&left.3)
            .then_with(|| left.2.created_at_block.cmp(&right.2.created_at_block))
            .then_with(|| left.0.cmp(&right.0))
            .then_with(|| left.1.cmp(&right.1))
    });
}

struct ResolvedScriptIdentifier {
    version_hash: Vec<u8>,
    version_info: ckbadger_store::types::ScriptVersionInfo,
    code_cells: Vec<VersionCodeCell>,
}

struct AmbiguousScriptIdentifier {
    version_hashes: Vec<Vec<u8>>,
}

enum ScriptIdentifierResolution {
    Resolved(Box<ResolvedScriptIdentifier>),
    Ambiguous(AmbiguousScriptIdentifier),
    NotFound,
}

fn fallback_script_version_info(
    state: &AppState,
    reference_hash: &[u8],
    version_hash: &[u8],
) -> Result<ckbadger_store::types::ScriptVersionInfo, ApiRouteError> {
    let direct_info = state
        .store
        .get_script_info(reference_hash)
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let version_info = if reference_hash == version_hash {
        None
    } else {
        state
            .store
            .get_script_info(version_hash)
            .map_err(|e| ApiError::internal(e.to_string()))?
    };

    let fallback = match (direct_info, version_info) {
        (Some(direct), Some(version)) => {
            let candidates = vec![direct.clone(), version];
            merge_script_info_for_reference(&candidates, reference_hash).unwrap_or(direct)
        }
        (Some(direct), None) => direct,
        (None, Some(version)) => version,
        (None, None) => {
            return Ok(ckbadger_store::types::ScriptVersionInfo {
                version_hash: version_hash.to_vec(),
                ..Default::default()
            });
        }
    };

    Ok(ckbadger_store::types::ScriptVersionInfo {
        version_hash: version_hash.to_vec(),
        name: fallback.name,
        family_id: None,
        deprecated: fallback.deprecated,
        category: fallback.category,
        website: fallback.website,
        description: fallback.description,
        lock_cells_count: fallback.lock_cells_count,
        lock_live_cells_count: fallback.lock_live_cells_count,
        lock_capacity_sum: fallback.lock_capacity_sum,
        lock_owned_capacity_sum: fallback.lock_owned_capacity_sum,
        lock_used_capacity_sum: fallback.lock_used_capacity_sum,
        lock_owned_knowledge_sum: fallback.lock_owned_knowledge_sum,
        type_cells_count: fallback.type_cells_count,
        type_live_cells_count: fallback.type_live_cells_count,
        type_capacity_sum: fallback.type_capacity_sum,
        type_owned_capacity_sum: fallback.type_owned_capacity_sum,
        type_used_capacity_sum: fallback.type_used_capacity_sum,
        type_owned_knowledge_sum: fallback.type_owned_knowledge_sum,
        canonical_reference_hash: None,
        canonical_hash_type: None,
        type_id_deployments: Vec::new(),
        registered_references: Vec::new(),
    })
}

fn resolve_script_identifier(
    state: &AppState,
    hash_bytes: &[u8],
) -> Result<ScriptIdentifierResolution, ApiRouteError> {
    let resolution = resolve_script_by_hash(&state.store, &state.append_only_store, hash_bytes)
        .map_err(|e| ApiError::internal(e.to_string()))?;
    finish_script_identifier_resolution(state, hash_bytes, resolution)
}

/// Resolve exactly one observed reference form (code_hash + hash_type) into the
/// same shape [`resolve_script_identifier`] returns, so callers that know the
/// form get its code cells and nothing else.
fn resolve_script_identifier_for_form(
    state: &AppState,
    hash_bytes: &[u8],
    hash_type: u8,
) -> Result<ScriptIdentifierResolution, ApiRouteError> {
    let resolution = resolve_script_form_by_hash(
        &state.store,
        &state.append_only_store,
        hash_type,
        hash_bytes,
    )
    .map_err(|e| ApiError::internal(e.to_string()))?;
    finish_script_identifier_resolution(state, hash_bytes, resolution)
}

fn finish_script_identifier_resolution(
    state: &AppState,
    hash_bytes: &[u8],
    resolution: CurrentScriptVersionResolution,
) -> Result<ScriptIdentifierResolution, ApiRouteError> {
    match resolution {
        CurrentScriptVersionResolution::Resolved(resolved) => {
            let resolved = *resolved;
            let version_info = match resolved.version_info {
                Some(version_info) => version_info,
                None => fallback_script_version_info(state, hash_bytes, &resolved.version_hash)?,
            };
            let code_cells = list_version_code_cells(
                &state.store,
                &state.append_only_store,
                &resolved.version_hash,
            )
            .map_err(|e| ApiError::internal(e.to_string()))?;
            Ok(ScriptIdentifierResolution::Resolved(Box::new(
                ResolvedScriptIdentifier {
                    version_hash: resolved.version_hash,
                    version_info,
                    code_cells,
                },
            )))
        }
        CurrentScriptVersionResolution::Ambiguous(ambiguous) => {
            let ambiguous = *ambiguous;
            Ok(ScriptIdentifierResolution::Ambiguous(
                AmbiguousScriptIdentifier {
                    version_hashes: ambiguous.version_hashes,
                },
            ))
        }
        CurrentScriptVersionResolution::NotFound => Ok(ScriptIdentifierResolution::NotFound),
    }
}

fn checked_capacity_totals(
    info: &ckbadger_store::ScriptInfo,
    context: &str,
) -> Result<(i128, i128), ApiRouteError> {
    let capacity = info.lock_owned_capacity_sum + info.type_owned_capacity_sum;
    let used = info.lock_owned_knowledge_sum + info.type_owned_knowledge_sum;
    if capacity < 0 {
        return Err(ApiError::internal(format!(
            "negative live capacity in {}: code_hash=0x{}, capacity={}",
            context,
            hex::encode(&info.code_hash),
            capacity
        )));
    }
    if used < 0 {
        return Err(ApiError::internal(format!(
            "negative live common knowledge size in {}: code_hash=0x{}, used={}",
            context,
            hex::encode(&info.code_hash),
            used
        )));
    }
    if used > capacity {
        return Err(ApiError::internal(format!(
            "live common knowledge size exceeds total in {}: code_hash=0x{}, used={}, capacity={}",
            context,
            hex::encode(&info.code_hash),
            used,
            capacity
        )));
    }
    Ok((capacity, used))
}

/// The queried reference's OWN live usage: (live_cells, owned_capacity,
/// owned_knowledge), validated fail-fast.
///
/// `/scripts/lookup` responses are keyed by the reference the caller asked
/// about, so the numbers must be that reference's usage rollup
/// (`CF_SCRIPT_INFO`, keyed by the reference bytes) and nothing else. A
/// bytecode version can be deployed under several independent references
/// (e.g. the RGB++ testnet signet and BTC-testnet3 type-id deployments share
/// one binary); resolving stats through the version — or through any
/// version-level "associated" reference slot — served one deployment's
/// numbers for every sibling. A reference with no ScriptInfo row has no
/// indexed usage: zero is the truth, not a fallback.
fn reference_own_stats(
    script_info: Option<&ckbadger_store::ScriptInfo>,
) -> Result<(i64, i128, i128), ApiRouteError> {
    match script_info {
        Some(info) => {
            let (owned_capacity, owned_knowledge) =
                checked_capacity_totals(info, "script lookup reference stats")?;
            Ok((
                info.lock_live_cells_count + info.type_live_cells_count,
                owned_capacity,
                owned_knowledge,
            ))
        }
        None => Ok((0, 0, 0)),
    }
}
fn apply_direction(ordering: Ordering, direction: SortDirection) -> Ordering {
    match direction {
        SortDirection::Asc => ordering,
        SortDirection::Desc => ordering.reverse(),
    }
}

fn compare_used_ratio(
    left: Option<(i128, i128)>,
    right: Option<(i128, i128)>,
    direction: SortDirection,
) -> Ordering {
    match (left, right) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Greater,
        (Some(_), None) => Ordering::Less,
        (Some((left_used, left_capacity)), Some((right_used, right_capacity))) => {
            let left_side = left_used * right_capacity;
            let right_side = right_used * left_capacity;
            apply_direction(left_side.cmp(&right_side), direction)
        }
    }
}

fn checked_family_totals(
    info: &ckbadger_store::types::ScriptFamilyInfo,
    context: &str,
) -> Result<(i128, i128), ApiRouteError> {
    if info.owned_capacity_sum < 0 {
        return Err(ApiError::internal(format!(
            "negative live capacity in {}: family_id={}, capacity={}",
            context, info.family_id, info.owned_capacity_sum
        )));
    }
    if info.owned_knowledge_sum < 0 {
        return Err(ApiError::internal(format!(
            "negative live common knowledge size in {}: family_id={}, used={}",
            context, info.family_id, info.owned_knowledge_sum
        )));
    }
    if info.owned_knowledge_sum > info.owned_capacity_sum {
        return Err(ApiError::internal(format!(
            "live common knowledge size exceeds total in {}: family_id={}, used={}, capacity={}",
            context, info.family_id, info.owned_knowledge_sum, info.owned_capacity_sum
        )));
    }
    Ok((info.owned_capacity_sum, info.owned_knowledge_sum))
}

fn family_display_name(info: &ckbadger_store::types::ScriptFamilyInfo) -> &str {
    info.name.as_str()
}

fn family_used_as_for_sort<'a>(
    info: &ckbadger_store::types::ScriptFamilyInfo,
    declared: Option<&'a str>,
) -> &'a str {
    match (info.lock_cells_count > 0, info.type_cells_count > 0) {
        (true, true) => "lock+type",
        (true, false) => "lock",
        (false, true) => "type",
        (false, false) => declared.unwrap_or(""),
    }
}

fn used_ratio_for_family_sort(
    info: &ckbadger_store::types::ScriptFamilyInfo,
) -> Option<(i128, i128)> {
    if info.owned_capacity_sum <= 0 {
        return None;
    }
    Some((info.owned_knowledge_sum, info.owned_capacity_sum))
}

fn compare_script_family_entries(
    left: &(String, ckbadger_store::types::ScriptFamilyInfo),
    right: &(String, ckbadger_store::types::ScriptFamilyInfo),
    sort_key: ScriptSortKey,
    direction: SortDirection,
    roles: &HashMap<String, String>,
) -> Ordering {
    let compared = match sort_key {
        ScriptSortKey::Name => apply_direction(
            family_display_name(&left.1).cmp(family_display_name(&right.1)),
            direction,
        ),
        ScriptSortKey::UsedAs => apply_direction(
            family_used_as_for_sort(&left.1, roles.get(&left.0).map(String::as_str)).cmp(
                family_used_as_for_sort(&right.1, roles.get(&right.0).map(String::as_str)),
            ),
            direction,
        ),
        ScriptSortKey::Description => apply_direction(
            left.1
                .description
                .as_deref()
                .unwrap_or("")
                .cmp(right.1.description.as_deref().unwrap_or("")),
            direction,
        ),
        ScriptSortKey::Used => apply_direction(
            left.1.owned_knowledge_sum.cmp(&right.1.owned_knowledge_sum),
            direction,
        ),
        ScriptSortKey::Capacity => apply_direction(
            left.1.owned_capacity_sum.cmp(&right.1.owned_capacity_sum),
            direction,
        ),
        ScriptSortKey::UsedRatio => compare_used_ratio(
            used_ratio_for_family_sort(&left.1),
            used_ratio_for_family_sort(&right.1),
            direction,
        ),
        ScriptSortKey::LiveCells => apply_direction(
            left.1.live_cells_count.cmp(&right.1.live_cells_count),
            direction,
        ),
        ScriptSortKey::Cells => {
            apply_direction(left.1.cells_count.cmp(&right.1.cells_count), direction)
        }
    };

    if compared != Ordering::Equal {
        return compared;
    }

    family_display_name(&left.1)
        .cmp(family_display_name(&right.1))
        .then_with(|| left.0.cmp(&right.0))
}

fn script_family_to_response(
    state: &AppState,
    family_id: &str,
    info: &ckbadger_store::types::ScriptFamilyInfo,
) -> Result<ScriptFamilyListItemResponse, ApiRouteError> {
    let (owned_capacity, owned_knowledge) = checked_family_totals(info, "script family response")?;
    Ok(ScriptFamilyListItemResponse {
        family_id: family_id.to_string(),
        name: info.name.clone(),
        description: info.description.clone(),
        script_kind: script_kind_for_family(state, info)?,
        deprecated: info.deprecated,
        website: info.website.clone(),
        live_cells_count: info.live_cells_count,
        cells_count: info.cells_count,
        owned_capacity_sum: owned_capacity.to_string(),
        owned_knowledge_sum: owned_knowledge.to_string(),
        versions_count: info.versions_count,
        code_classes: family_code_classes(state, family_id)?,
    })
}

fn reference_totals(info: &ckbadger_store::types::ScriptReferenceInfo) -> (i64, i64, i128, i128) {
    (
        info.lock_live_cells_count + info.type_live_cells_count,
        info.lock_cells_count + info.type_cells_count,
        info.lock_owned_capacity_sum + info.type_owned_capacity_sum,
        info.lock_owned_knowledge_sum + info.type_owned_knowledge_sum,
    )
}

fn reference_info_to_response(
    state: &AppState,
    member: &FamilyMemberForm,
) -> Result<ScriptObservedReferenceResponse, ApiRouteError> {
    let info = &member.info;
    let (live_cells_count, cells_count, owned_capacity_sum, owned_knowledge_sum) =
        reference_totals(info);
    if owned_capacity_sum < 0 || owned_knowledge_sum < 0 || owned_knowledge_sum > owned_capacity_sum
    {
        return Err(ApiError::internal(format!(
            "invalid script reference totals while building family detail: reference_hash=0x{}, hash_type={}, owned_capacity_sum={}, owned_knowledge_sum={}",
            hex::encode(&info.reference_hash),
            info.hash_type,
            owned_capacity_sum,
            owned_knowledge_sum
        )));
    }
    Ok(ScriptObservedReferenceResponse {
        reference_hash: format!("0x{}", hex::encode(&info.reference_hash)),
        hash_type: hash_type_to_string(info.hash_type)
            .ok_or_else(|| {
                ApiError::internal(format!(
                    "invalid hash type {} for reference 0x{}",
                    info.hash_type,
                    hex::encode(&info.reference_hash)
                ))
            })?
            .to_string(),
        live_cells_count,
        cells_count,
        owned_capacity_sum: owned_capacity_sum.to_string(),
        owned_knowledge_sum: owned_knowledge_sum.to_string(),
        current_version_hash: member
            .current_version_hash
            .as_ref()
            .map(|hash| format!("0x{}", hex::encode(hash))),
        custody: reference_custody(state, &member.reference_hash, member.hash_type)?,
    })
}

fn class_totals_for_members(
    state: &AppState,
    members: &[FamilyMemberForm],
) -> Result<Vec<ScriptCodeClassTotalResponse>, ApiRouteError> {
    let mut totals: BTreeMap<String, (i64, i128, i64)> = BTreeMap::new();
    for member in members {
        let capacity = member.info.lock_owned_capacity_sum;
        if capacity < 0 || member.info.lock_live_cells_count < 0 {
            return Err(ApiError::internal(format!(
                "negative lock usage for script reference 0x{} hash_type={}",
                hex::encode(&member.reference_hash),
                member.hash_type
            )));
        }
        let custody = reference_custody(state, &member.reference_hash, member.hash_type)?;
        let entry = totals.entry(custody.class).or_default();
        entry.0 = entry
            .0
            .checked_add(member.info.lock_live_cells_count)
            .ok_or_else(|| ApiError::internal("script class lock-cell count overflow"))?;
        entry.1 = entry
            .1
            .checked_add(capacity)
            .ok_or_else(|| ApiError::internal("script class capacity overflow"))?;
        entry.2 += 1;
    }
    Ok(totals
        .into_iter()
        .map(|(class, (lock_cells_count, capacity, reference_count))| {
            ScriptCodeClassTotalResponse {
                class,
                lock_cells_count,
                lock_owned_capacity_sum: capacity.to_string(),
                reference_count,
            }
        })
        .collect())
}

fn family_members(
    state: &AppState,
    family_id: &str,
) -> Result<Vec<FamilyMemberForm>, ApiRouteError> {
    let versions = state
        .store
        .list_script_versions()
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let family_versions: Vec<_> = versions
        .into_iter()
        .filter(|(_, info)| info.family_id.as_deref() == Some(family_id))
        .collect();
    let allowed_versions = family_versions
        .iter()
        .map(|(hash, _)| hash.clone())
        .collect();
    let registered = registered_reference_set(&family_versions);
    family_member_reference_forms(
        state,
        family_id,
        Some(family_id),
        &allowed_versions,
        &registered,
    )
}

fn family_code_classes(
    state: &AppState,
    family_id: &str,
) -> Result<Vec<ScriptCodeClassTotalResponse>, ApiRouteError> {
    class_totals_for_members(state, &family_members(state, family_id)?)
}

fn checked_version_totals(
    version_hash: &[u8],
    info: &ckbadger_store::types::ScriptVersionInfo,
    context: &str,
) -> Result<(i64, i64, i128, i128, i128, i128), ApiRouteError> {
    let (
        cells_count,
        live_cells_count,
        capacity_sum,
        owned_capacity_sum,
        used_capacity_sum,
        owned_knowledge_sum,
    ) = version_totals(info);

    if capacity_sum < 0 {
        return Err(ApiError::internal(format!(
            "negative capacity in {}: version_hash=0x{}, value={}",
            context,
            hex::encode(version_hash),
            capacity_sum
        )));
    }
    if used_capacity_sum < 0 {
        return Err(ApiError::internal(format!(
            "negative used capacity in {}: version_hash=0x{}, value={}",
            context,
            hex::encode(version_hash),
            used_capacity_sum
        )));
    }
    if used_capacity_sum > capacity_sum {
        return Err(ApiError::internal(format!(
            "used capacity exceeds total in {}: version_hash=0x{}, used={}, capacity={}",
            context,
            hex::encode(version_hash),
            used_capacity_sum,
            capacity_sum
        )));
    }
    if owned_capacity_sum < 0 {
        return Err(ApiError::internal(format!(
            "negative live capacity in {}: version_hash=0x{}, value={}",
            context,
            hex::encode(version_hash),
            owned_capacity_sum
        )));
    }
    if owned_knowledge_sum < 0 {
        return Err(ApiError::internal(format!(
            "negative live common knowledge size in {}: version_hash=0x{}, value={}",
            context,
            hex::encode(version_hash),
            owned_knowledge_sum
        )));
    }
    if owned_knowledge_sum > owned_capacity_sum {
        return Err(ApiError::internal(format!(
            "live common knowledge size exceeds total in {}: version_hash=0x{}, used={}, capacity={}",
            context,
            hex::encode(version_hash),
            owned_knowledge_sum,
            owned_capacity_sum
        )));
    }

    Ok((
        cells_count,
        live_cells_count,
        capacity_sum,
        owned_capacity_sum,
        used_capacity_sum,
        owned_knowledge_sum,
    ))
}

fn code_cell_timestamp(
    state: &AppState,
    version_hash: &[u8],
    created_at_block: i64,
) -> Result<i64, ApiRouteError> {
    let header = state
        .store
        .get_block_header(created_at_block)
        .map_err(|e| ApiError::internal(e.to_string()))?
        .ok_or_else(|| {
            ApiError::internal(format!(
                "missing block header while building script family detail: version_hash=0x{}, created_at_block={}",
                hex::encode(version_hash),
                created_at_block
            ))
        })?;

    Ok(header.timestamp)
}

fn first_code_cell_timestamp(
    state: &AppState,
    version_hash: &[u8],
    code_cells: &[VersionCodeCell],
) -> Result<Option<i64>, ApiRouteError> {
    let Some(block) = code_cells
        .iter()
        .map(|(_, _, cell, _)| cell.created_at_block)
        .min()
    else {
        return Ok(None);
    };
    Ok(Some(code_cell_timestamp(state, version_hash, block)?))
}

fn script_version_deployments(
    state: &AppState,
    version_hash: &[u8],
    _info: &ckbadger_store::types::ScriptVersionInfo,
    code_cells: &[VersionCodeCell],
) -> Result<Vec<ScriptVersionDeploymentResponse>, ApiRouteError> {
    code_cells
        .iter()
        .map(|(tx_hash, output_index, cell, is_live)| {
            let hash_type = if cell.type_script_hash.is_some() {
                "type".to_string()
            } else {
                "data".to_string()
            };
            let type_reference_hash = cell
                .type_script_hash
                .as_ref()
                .map(|hash| format!("0x{}", hex::encode(hash)));

            let consumed = if *is_live {
                None
            } else {
                Some(
                    state
                        .store
                        .get_consumed_cell_info(tx_hash, *output_index, &state.append_only_store)
                        .map_err(|e| {
                            ApiError::internal(format!(
                                "consumed code cell 0x{}:{}: {e}",
                                hex::encode(tx_hash),
                                output_index
                            ))
                        })?
                        .ok_or_else(|| {
                            ApiError::internal(format!(
                                "indexed dead code cell is missing consumed marker: 0x{}:{}",
                                hex::encode(tx_hash),
                                output_index
                            ))
                        })?,
                )
            };

            let mut custody = classify_code_cell_lock(&cell.cell)?;
            mark_never_signed_if_proven(state, &cell.cell, &mut custody)?;
            attach_multisig_witness(
                state,
                tx_hash,
                *output_index,
                &cell.cell,
                consumed.as_ref().and_then(|c| c.consumed_by_tx.as_deref()),
                &mut custody,
            )?;

            Ok(ScriptVersionDeploymentResponse {
                hash_type,
                type_reference_hash,
                data_reference_hash: format!("0x{}", hex::encode(version_hash)),
                code_cell_tx_hash: format!("0x{}", hex::encode(tx_hash)),
                code_cell_output_index: i32::from(*output_index),
                deployed_at: code_cell_timestamp(state, version_hash, cell.created_at_block)?,
                created_at_block: cell.created_at_block,
                status: if *is_live { "live" } else { "consumed" }.to_string(),
                consumed_at_block: consumed.as_ref().map(|c| c.consumed_at_block),
                consumed_by_tx: consumed
                    .as_ref()
                    .and_then(|c| c.consumed_by_tx.as_ref())
                    .map(|hash| format!("0x{}", hex::encode(hash))),
                custody,
            })
        })
        .collect()
}

/// Family provenance survives retirement; a current binary binding may not.
struct FamilyMemberForm {
    reference_hash: Vec<u8>,
    hash_type: u8,
    current_version_hash: Option<Vec<u8>>,
    info: ckbadger_store::types::ScriptReferenceInfo,
}

fn registered_reference_set(
    versions: &[(Vec<u8>, ckbadger_store::types::ScriptVersionInfo)],
) -> HashSet<(u8, Vec<u8>)> {
    versions
        .iter()
        .flat_map(|(_, info)| {
            info.registered_references
                .iter()
                .map(|(reference, hash_type)| (*hash_type, reference.clone()))
        })
        .collect()
}

/// Enumerate family provenance independently of current binary resolution.
/// Charts include retired references; version rows include only current bindings.
/// The indexer and API share the same family membership operator.
fn family_member_reference_forms(
    state: &AppState,
    membership_context: &str,
    family_id: Option<&str>,
    allowed_versions: &HashSet<Vec<u8>>,
    registered_references: &HashSet<(u8, Vec<u8>)>,
) -> Result<Vec<FamilyMemberForm>, ApiRouteError> {
    let versions = state
        .store
        .list_script_versions()
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let membership = ScriptFamilyMembershipIndex::new(versions.iter().map(|(_, info)| info))
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let mut members = Vec::new();
    for ((reference_hash, hash_type), info) in state
        .store
        .list_script_reference_infos()
        .map_err(|e| ApiError::internal(e.to_string()))?
    {
        let current_version_hash = reference_form_member_version(
            &state.store,
            &state.append_only_store,
            hash_type,
            &reference_hash,
            &|hash: &[u8]| allowed_versions.contains(hash),
        )
        .map_err(|e| ApiError::internal(e.to_string()))?;
        let resolved_family = membership
            .resolve_family(&reference_hash, hash_type, current_version_hash.as_deref())
            .map_err(|e| ApiError::internal(e.to_string()))?;
        let is_member = match family_id {
            Some(family_id) => resolved_family == Some(family_id),
            None => current_version_hash
                .as_ref()
                .is_some_and(|hash| allowed_versions.contains(hash)),
        };
        if !is_member {
            if current_version_hash.is_none()
                && registered_references.contains(&(hash_type, reference_hash.clone()))
            {
                return Err(ApiError::internal(format!(
                    "script family detail is missing reference->version mapping: context={}, reference_hash=0x{}, hash_type={}",
                    membership_context, hex::encode(&reference_hash), hash_type
                )));
            }
            continue;
        }
        if current_version_hash
            .as_ref()
            .is_some_and(|hash| !allowed_versions.contains(hash))
        {
            return Err(ApiError::internal(format!(
                "script family membership resolved to an unlisted version: context={}, reference_hash=0x{}, hash_type={}",
                membership_context, hex::encode(&reference_hash), hash_type
            )));
        }
        members.push(FamilyMemberForm {
            reference_hash,
            hash_type,
            current_version_hash,
            info,
        });
    }
    Ok(members)
}

fn observed_references_by_version(
    state: &AppState,
    family_id: &str,
    versions: &[(Vec<u8>, ckbadger_store::types::ScriptVersionInfo)],
) -> Result<
    (
        HashMap<Vec<u8>, Vec<ScriptObservedReferenceResponse>>,
        Vec<ScriptObservedReferenceResponse>,
        Vec<FamilyMemberForm>,
    ),
    ApiRouteError,
> {
    let mut by_version: HashMap<Vec<u8>, Vec<ScriptObservedReferenceResponse>> = HashMap::new();
    let mut all_references = Vec::new();
    let allowed_versions: HashSet<Vec<u8>> = versions
        .iter()
        .map(|(version_hash, _)| version_hash.clone())
        .collect();
    let registered_references = registered_reference_set(versions);

    let members = family_member_reference_forms(
        state,
        family_id,
        Some(family_id),
        &allowed_versions,
        &registered_references,
    )?;
    for member in &members {
        let response = reference_info_to_response(state, member)?;
        if let Some(version_hash) = &member.current_version_hash {
            by_version
                .entry(version_hash.clone())
                .or_default()
                .push(response.clone());
        }
        all_references.push(response);
    }

    for references in by_version.values_mut() {
        references.sort_by(|left, right| {
            right
                .live_cells_count
                .cmp(&left.live_cells_count)
                .then_with(|| right.cells_count.cmp(&left.cells_count))
                .then_with(|| left.hash_type.cmp(&right.hash_type))
                .then_with(|| left.reference_hash.cmp(&right.reference_hash))
        });
    }

    all_references.sort_by(|a, b| {
        a.reference_hash
            .cmp(&b.reference_hash)
            .then_with(|| a.hash_type.cmp(&b.hash_type))
    });
    Ok((by_version, all_references, members))
}

fn load_script_family_by_name(
    state: &AppState,
    name: &str,
) -> Result<(String, ckbadger_store::types::ScriptFamilyInfo), ApiRouteError> {
    let family_id = state
        .store
        .get_script_family_id_by_name(name)
        .map_err(|e| ApiError::internal(e.to_string()))?
        .ok_or_else(|| ApiError::not_found("Script not found"))?;
    let family = state
        .store
        .get_script_family(&family_id)
        .map_err(|e| ApiError::internal(e.to_string()))?
        .ok_or_else(|| {
            ApiError::internal(format!(
                "script family name index points to missing family: name={}, family_id={}",
                name, family_id
            ))
        })?;

    if family.family_id != family_id {
        return Err(ApiError::internal(format!(
            "script family record mismatch while resolving family by name: name={}, indexed_family_id={}, record_family_id={}",
            name, family_id, family.family_id
        )));
    }

    Ok((family_id, family))
}

fn load_family_versions(
    state: &Arc<AppState>,
    family_id: &str,
) -> Result<Vec<(Vec<u8>, ckbadger_store::types::ScriptVersionInfo)>, ApiRouteError> {
    let version_hashes = state
        .store
        .list_script_version_hashes_by_family(family_id)
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let mut versions = Vec::with_capacity(version_hashes.len());
    for version_hash in version_hashes {
        // The family index, binding, and mutable version counters must all
        // come from this request's pinned canonical read view.
        let version_info = state
            .store
            .get_script_version(&version_hash)
            .map_err(|e| ApiError::internal(e.to_string()))?
            .ok_or_else(|| {
                ApiError::internal(format!(
                    "script family references missing version: family_id={}, version_hash=0x{}",
                    family_id,
                    hex::encode(&version_hash)
                ))
            })?;

        if version_info.family_id.as_deref() != Some(family_id) {
            return Err(ApiError::internal(format!(
                "script version family mismatch while building family detail: family_id={}, version_hash=0x{}, version_family_id={:?}",
                family_id,
                hex::encode(&version_hash),
                version_info.family_id
            )));
        }

        versions.push((version_hash, version_info));
    }

    Ok(versions)
}

fn resolved_version_name(
    state: &AppState,
    version_hash: &[u8],
    info: &ckbadger_store::types::ScriptVersionInfo,
    unknown_fallback: &str,
) -> Result<String, ApiRouteError> {
    if let Some(name) = info.name.clone() {
        return Ok(name);
    }

    let Some(family_id) = info.family_id.as_deref() else {
        return Ok(unknown_fallback.to_string());
    };

    let family = state
        .store
        .get_script_family(family_id)
        .map_err(|e| ApiError::internal(e.to_string()))?
        .ok_or_else(|| {
            ApiError::internal(format!(
                "script version points to missing family while resolving name: version_hash=0x{}, family_id={}",
                hex::encode(version_hash),
                family_id
            ))
        })?;

    Ok(family.name)
}

fn script_version_to_detail_response(
    state: &AppState,
    version_hash: &[u8],
    info: &ckbadger_store::types::ScriptVersionInfo,
    observed_references: Vec<ScriptObservedReferenceResponse>,
) -> Result<ScriptVersionDetailResponse, ApiRouteError> {
    let (cells_count, live_cells_count, _, owned_capacity_sum, _, owned_knowledge_sum) =
        checked_version_totals(version_hash, info, "script family detail")?;
    let mut code_cells =
        list_version_code_cells(&state.store, &state.append_only_store, version_hash)
            .map_err(|e| ApiError::internal(e.to_string()))?;
    sort_code_cells(&mut code_cells);

    let deployed_at = first_code_cell_timestamp(state, version_hash, &code_cells)?;
    let code_cells_live_count = code_cells
        .iter()
        .filter(|(_, _, _, is_live)| *is_live)
        .count() as i64;
    let code_cells_total = code_cells.len() as i64;
    let deployments = script_version_deployments(state, version_hash, info, &code_cells)?;
    let first_position = deployments
        .iter()
        .map(|deployment| {
            Ok((
                deployment.created_at_block,
                required_script_tx_index(
                    state,
                    &deployment.code_cell_tx_hash,
                    deployment.created_at_block,
                    "code cell creation",
                )?,
            ))
        })
        .collect::<Result<Vec<_>, ApiRouteError>>()?
        .into_iter()
        .min();

    Ok(ScriptVersionDetailResponse {
        version_hash: format!("0x{}", hex::encode(version_hash)),
        name: resolved_version_name(
            state,
            version_hash,
            info,
            &format!("0x{}", hex::encode(version_hash)),
        )?,
        description: info.description.clone(),
        script_kind: version_script_kind(info),
        website: info.website.clone(),
        deprecated: info.deprecated,
        canonical_reference_hash: info
            .canonical_reference_hash
            .as_ref()
            .map(|hash| format!("0x{}", hex::encode(hash))),
        canonical_hash_type: info
            .canonical_hash_type
            .and_then(hash_type_to_string)
            .map(str::to_string),
        deployed_at,
        first_deployed_block: first_position.map(|(block, _)| block),
        first_deployed_tx_index: first_position.map(|(_, index)| index),
        live_cells_count,
        cells_count,
        owned_capacity_sum: owned_capacity_sum.to_string(),
        owned_knowledge_sum: owned_knowledge_sum.to_string(),
        code_cells_live_count,
        code_cells_total,
        deployments,
        references: observed_references,
    })
}

fn required_script_tx_index(
    state: &AppState,
    tx_hash: &str,
    expected_block: i64,
    event: &str,
) -> Result<i32, ApiRouteError> {
    let bytes = hex::decode(tx_hash.trim_start_matches("0x")).map_err(|e| {
        ApiError::internal(format!(
            "{event} transaction {tx_hash} has invalid hash: {e}"
        ))
    })?;
    let (block, index) = state
        .store
        .get_tx_location(&bytes)
        .map_err(|e| ApiError::internal(format!("{event} transaction {tx_hash}: {e}")))?
        .ok_or_else(|| {
            ApiError::internal(format!(
                "{event} transaction {tx_hash} at block {expected_block} lacks canonical tx location"
            ))
        })?;
    if block != expected_block {
        return Err(ApiError::internal(format!(
            "{event} transaction {tx_hash} location block {block} differs from code cell block {expected_block}"
        )));
    }
    Ok(index)
}

fn script_history(
    state: &AppState,
    family_versions: &[(Vec<u8>, ckbadger_store::types::ScriptVersionInfo)],
    versions: &[ScriptVersionDetailResponse],
) -> Result<Vec<ScriptHistoryMarkerResponse>, ApiRouteError> {
    fn calendar(state: &AppState, block_number: i64) -> Result<(i64, String), ApiRouteError> {
        let header = state
            .store
            .get_block_header(block_number)
            .map_err(|e| ApiError::internal(format!("script history block {block_number}: {e}")))?
            .ok_or_else(|| {
                ApiError::internal(format!(
                    "script history missing block header {block_number}"
                ))
            })?;
        let indexed_day = ckbadger_common::stats_bucket::stats_day(header.chain_time_ms)
            .map_err(|e| {
                ApiError::internal(format!(
                    "script history block {block_number} invalid chain time: {e}"
                ))
            })?
            .format("%Y-%m-%d")
            .to_string();
        Ok((header.chain_time_ms, indexed_day))
    }
    let mut history = Vec::new();
    for ((version_hash, info), response) in family_versions.iter().zip(versions) {
        if response.version_hash != format!("0x{}", hex::encode(version_hash)) {
            return Err(ApiError::internal(format!(
                "version response order mismatch for 0x{}",
                hex::encode(version_hash)
            )));
        }
        for deployment in &response.deployments {
            let mut references: Vec<(String, String)> = Vec::new();
            if let Some(type_hash) = &deployment.type_reference_hash {
                let raw = hex::decode(type_hash.trim_start_matches("0x"))
                    .map_err(|e| ApiError::internal(e.to_string()))?;
                if info
                    .type_id_deployments
                    .iter()
                    .any(|(hash, block)| hash == &raw && *block == deployment.created_at_block)
                {
                    references.push((type_hash.clone(), "type".to_string()));
                }
            }
            for (reference, hash_type) in &info.registered_references {
                if matches!(hash_type, 0 | 2 | 4) && reference == version_hash {
                    references.push((
                        format!("0x{}", hex::encode(reference)),
                        hash_type_to_string(*hash_type)
                            .ok_or_else(|| {
                                ApiError::internal(format!(
                                    "invalid registered hash type {}",
                                    hash_type
                                ))
                            })?
                            .to_string(),
                    ));
                }
            }
            for (reference_hash, hash_type) in references {
                let (chain_time_ms, indexed_day) = calendar(state, deployment.created_at_block)?;
                let tx_index = required_script_tx_index(
                    state,
                    &deployment.code_cell_tx_hash,
                    deployment.created_at_block,
                    "code cell creation",
                )?;
                history.push(ScriptHistoryMarkerResponse {
                    reference_hash: reference_hash.clone(),
                    hash_type: hash_type.clone(),
                    version_hash: Some(response.version_hash.clone()),
                    event: "deployed".to_string(),
                    block_number: deployment.created_at_block,
                    tx_index: Some(tx_index),
                    timestamp: deployment.deployed_at,
                    chain_time_ms,
                    indexed_day,
                    code_cell_tx_hash: deployment.code_cell_tx_hash.clone(),
                    code_cell_output_index: deployment.code_cell_output_index,
                    event_tx_hash: deployment.code_cell_tx_hash.clone(),
                    custody: deployment.custody.clone(),
                });
                if let Some(block_number) = deployment.consumed_at_block {
                    let (chain_time_ms, indexed_day) = calendar(state, block_number)?;
                    let consuming_tx = deployment.consumed_by_tx.as_ref().ok_or_else(|| {
                        ApiError::internal(format!(
                            "consumed code cell {}:{} lacks consuming transaction",
                            deployment.code_cell_tx_hash, deployment.code_cell_output_index
                        ))
                    })?;
                    let consumed_tx_index = required_script_tx_index(
                        state,
                        consuming_tx,
                        block_number,
                        "code cell consumption",
                    )?;
                    if (block_number, consumed_tx_index) <= (deployment.created_at_block, tx_index)
                    {
                        return Err(ApiError::internal(format!("code cell {}:{} consumption at ({block_number},{consumed_tx_index}) does not follow creation at ({},{tx_index})", deployment.code_cell_tx_hash, deployment.code_cell_output_index, deployment.created_at_block)));
                    }
                    history.push(ScriptHistoryMarkerResponse {
                        reference_hash,
                        hash_type,
                        version_hash: Some(response.version_hash.clone()),
                        event: "consumed".to_string(),
                        block_number,
                        tx_index: Some(consumed_tx_index),
                        timestamp: code_cell_timestamp(state, version_hash, block_number)?,
                        chain_time_ms,
                        indexed_day,
                        code_cell_tx_hash: deployment.code_cell_tx_hash.clone(),
                        code_cell_output_index: deployment.code_cell_output_index,
                        event_tx_hash: consuming_tx.clone(),
                        custody: deployment.custody.clone(),
                    });
                }
            }
        }
    }
    history.sort_by(|a, b| {
        a.block_number
            .cmp(&b.block_number)
            .then_with(|| a.tx_index.cmp(&b.tx_index))
            .then_with(|| a.reference_hash.cmp(&b.reference_hash))
            .then_with(|| a.hash_type.cmp(&b.hash_type))
            .then_with(|| a.code_cell_output_index.cmp(&b.code_cell_output_index))
            .then_with(|| a.event.cmp(&b.event))
    });
    Ok(history)
}

/// Shared family chronology for code-cell and Script routes. Family membership
/// and deployment evidence are computed by the same path as the Script detail.
#[expect(dead_code, reason = "shared by CodeCell route after integration")]
pub(crate) fn script_family_history(
    state: &Arc<AppState>,
    family_id: &str,
) -> Result<Vec<ScriptHistoryMarkerResponse>, ApiRouteError> {
    let family_versions = load_family_versions(state, family_id)?;
    let (mut references, _, _) =
        observed_references_by_version(state, family_id, &family_versions)?;
    let versions = family_versions
        .iter()
        .map(|(hash, info)| {
            let refs = references.remove(hash).unwrap_or_default();
            script_version_to_detail_response(state, hash, info, refs)
        })
        .collect::<Result<Vec<_>, _>>()?;
    script_history(state, &family_versions, &versions)
}

/// Build a ScriptLookupInfo from a resolved script identifier.
///
/// Shared by both per-tx and global resolution paths. The response is keyed
/// by the queried reference, so everything deployment-scoped in it — usage
/// stats and code cells — belongs to THAT reference's deployment:
///
/// - Stats come from the queried reference's own `CF_SCRIPT_INFO` row
///   ([`reference_own_stats`]), never resolved through the version.
/// - Code cells are the resolved bytecode's instances restricted to this
///   deployment: for a type-form reference the cells whose type script IS the
///   reference; when the reference is the bytecode hash itself, every
///   instance of that bytecode.
fn build_lookup_info(
    state: &AppState,
    reference_hash: &[u8],
    reference_hash_hex: &str,
    version_hash: &[u8],
    version_info: &ckbadger_store::types::ScriptVersionInfo,
    code_cells: Vec<VersionCodeCell>,
) -> Result<ScriptLookupInfo, ApiRouteError> {
    let mut code_cells: Vec<VersionCodeCell> = code_cells
        .into_iter()
        .filter(|(_, _, cell, _)| {
            reference_hash == version_hash
                || cell.type_script_hash.as_deref() == Some(reference_hash)
        })
        .collect();
    sort_code_cells(&mut code_cells);
    let script_info = state
        .store
        .get_script_info(reference_hash)
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let (live_cells_count, owned_capacity_sum, owned_knowledge) =
        reference_own_stats(script_info.as_ref())?;
    let code_cell = code_cells.first();
    let hash_type = script_info
        .as_ref()
        .and_then(|info| hash_type_to_string(info.hash_type).map(|s| s.to_string()));
    let (deployment_type_hash, deployment_data_hash) = script_info
        .as_ref()
        .map(|info| {
            let (type_ref, data_ref) = deployment_reference_hashes(info);
            (
                type_ref.map(|h| format!("0x{}", hex::encode(h))),
                data_ref.map(|h| format!("0x{}", hex::encode(h))),
            )
        })
        .unwrap_or((None, None));

    Ok(ScriptLookupInfo {
        reference_hash: reference_hash_hex.to_string(),
        code_hash: format!("0x{}", hex::encode(version_hash)),
        name: resolved_version_name(state, version_hash, version_info, "Unknown")?,
        description: script_info
            .as_ref()
            .and_then(|info| info.description.clone()),
        deprecated: version_info.deprecated
            || script_info
                .as_ref()
                .map(|info| info.deprecated)
                .unwrap_or(false),
        script_kind: script_info
            .as_ref()
            .and_then(|info| script_kind_from_counts(info.lock_cells_count, info.type_cells_count)),
        decoder_type: version_info.category.clone(),
        hash_type,
        deployment_type_hash,
        deployment_data_hash,
        code_cell_tx_hash: code_cell
            .map(|(tx_hash, _, _, _)| format!("0x{}", hex::encode(tx_hash))),
        code_cell_output_index: code_cell.map(|(_, output_index, _, _)| i32::from(*output_index)),
        live_cells_count,
        owned_capacity_sum: owned_capacity_sum.to_string(),
        owned_knowledge_sum: owned_knowledge.to_string(),
        code_cells_live_count: code_cells
            .iter()
            .filter(|(_, _, _, is_live)| *is_live)
            .count() as i64,
        code_cells_total: code_cells.len() as i64,
        resolution_state: "resolved".to_string(),
        ambiguity: None,
    })
}

async fn lookup_scripts(
    State(state): State<Arc<AppState>>,
    Json(request): Json<LookupScriptsRequest>,
) -> ApiResult<HashMap<String, ScriptLookupInfo>> {
    if request.code_hashes.len() > 100 {
        return Err(ApiError::bad_request(
            "Too many code_hashes, maximum is 100",
        ));
    }

    // Every request-supplied hash is validated before any short-circuit below,
    // so whether a malformed one is reported never depends on an unrelated
    // field. `txHash` used to be checked after the empty-`code_hashes` early
    // return, which made `{"codeHashes": [], "txHash": "0x1234"}` a silent 200
    // and the same body with one code hash a 400 — one field, two contracts.
    let tx_hash = match request.tx_hash.as_deref() {
        Some(tx_hash) => {
            parse_hash32(tx_hash, "txHash")?;
            Some(tx_hash)
        }
        None => None,
    };

    // One malformed code_hash fails the whole batch rather than being truncated
    // into a lookup for a different script.
    let code_hash_bytes = request
        .code_hashes
        .iter()
        .map(|h| parse_hash32(h, "code_hashes entry"))
        .collect::<Result<Vec<Vec<u8>>, _>>()?;

    if code_hash_bytes.is_empty() {
        return ok(HashMap::new());
    }

    let mut result: HashMap<String, ScriptLookupInfo> = HashMap::new();
    let all_script_infos = load_script_infos_cached(&state)?;
    let per_tx_mappings =
        tx_hash.and_then(|tx_hash| resolve_dep_cells_for_transaction(&state, tx_hash));

    for code_hash in &code_hash_bytes {
        let reference_hash_hex = format!("0x{}", hex::encode(code_hash));

        // Per-tx resolution: if cell_deps provide a version_hash, use it directly
        if let Some(version_hash) = per_tx_mappings
            .as_ref()
            .and_then(|m| m.get(code_hash.as_slice()))
        {
            let version_info = match state.store.get_script_version(version_hash).map_err(|e| {
                ApiError::internal(format!(
                    "failed to read script version 0x{} for tx lookup: {e}",
                    hex::encode(version_hash)
                ))
            })? {
                Some(info) => info,
                None => fallback_script_version_info(&state, code_hash, version_hash)?,
            };

            let code_cells =
                list_version_code_cells(&state.store, &state.append_only_store, version_hash)
                    .map_err(|e| {
                        ApiError::internal(format!(
                            "failed to list code cells for script version 0x{}: {e}",
                            hex::encode(version_hash)
                        ))
                    })?;

            let info = build_lookup_info(
                &state,
                code_hash,
                &reference_hash_hex,
                version_hash,
                &version_info,
                code_cells,
            )?;
            result.insert(reference_hash_hex.clone(), info);
            continue;
        }

        match resolve_script_identifier(&state, code_hash)? {
            ScriptIdentifierResolution::Resolved(resolved) => {
                let ResolvedScriptIdentifier {
                    version_hash,
                    version_info,
                    code_cells,
                } = *resolved;
                let info = build_lookup_info(
                    &state,
                    code_hash,
                    &reference_hash_hex,
                    &version_hash,
                    &version_info,
                    code_cells,
                )?;
                result.insert(reference_hash_hex.clone(), info);
            }
            ScriptIdentifierResolution::Ambiguous(ambiguous) => {
                let AmbiguousScriptIdentifier { version_hashes } = ambiguous;
                let infos_bare: Vec<ckbadger_store::ScriptInfo> = all_script_infos
                    .iter()
                    .map(|(_, info)| info.clone())
                    .collect();
                let merged = merge_script_info_for_reference(&infos_bare, code_hash);
                // Try to get a known name: first from merged ScriptInfo, then from any version
                let merged_name = merged
                    .as_ref()
                    .and_then(|m| m.name.clone())
                    .filter(|n| crate::utils::is_known_script_name(Some(n.as_str())));
                let name = if let Some(n) = merged_name {
                    n
                } else {
                    // Try each version_hash for a name
                    let mut found_name = None;
                    for vh in &version_hashes {
                        if let Some(vi) = state
                            .store
                            .get_script_version(vh)
                            .map_err(|e| ApiError::internal(e.to_string()))?
                        {
                            let n = resolved_version_name(&state, vh, &vi, "")?;
                            if !n.is_empty() {
                                found_name = Some(n);
                                break;
                            }
                        }
                    }
                    found_name.unwrap_or_else(|| "Ambiguous Script Reference".to_string())
                };
                let script_kind = merged
                    .as_ref()
                    .and_then(|m| script_kind_from_counts(m.lock_cells_count, m.type_cells_count));
                let hash_type = merged
                    .as_ref()
                    .and_then(|m| hash_type_to_string(m.hash_type).map(|s| s.to_string()));
                // The reference's own deprecated flag (merged across its
                // related deployment rows) is known even when the version is
                // ambiguous — report it, never a hardcoded false.
                let deprecated = merged.as_ref().map(|m| m.deprecated).unwrap_or(false);
                result.insert(
                    reference_hash_hex.clone(),
                    ScriptLookupInfo {
                        reference_hash: reference_hash_hex.clone(),
                        code_hash: reference_hash_hex.clone(),
                        name,
                        description: merged.as_ref().and_then(|m| m.description.clone()),
                        deprecated,
                        script_kind,
                        decoder_type: None,
                        hash_type,
                        deployment_type_hash: None,
                        deployment_data_hash: None,
                        code_cell_tx_hash: None,
                        code_cell_output_index: None,
                        live_cells_count: 0,
                        owned_capacity_sum: "0".to_string(),
                        owned_knowledge_sum: "0".to_string(),
                        code_cells_live_count: 0,
                        code_cells_total: 0,
                        resolution_state: "ambiguous".to_string(),
                        ambiguity: Some(ScriptResolutionAmbiguityResponse {
                            version_hashes: version_hashes
                                .into_iter()
                                .map(|hash| format!("0x{}", hex::encode(hash)))
                                .collect(),
                        }),
                    },
                );
            }
            ScriptIdentifierResolution::NotFound => {}
        }
    }

    ok(result)
}

#[derive(Debug, Deserialize)]
pub struct CodeCellQuery {
    code_hash: String,
    hash_type: Option<String>,
}

impl CodeCellQuery {
    /// Resolve the queried script the way the caller asked for it: with a
    /// hash_type this is the exact observed reference form, without one it is
    /// the whole deployment family. Both code-cell endpoints go through here so
    /// they can never disagree.
    fn resolve(&self, state: &AppState) -> Result<ScriptIdentifierResolution, ApiRouteError> {
        let code_hash_bytes = parse_hash32(&self.code_hash, "code_hash")?;

        match self.hash_type.as_deref() {
            Some(raw) => {
                let hash_type = hash_type_to_u8(raw).ok_or_else(|| {
                    ApiError::bad_request(format!(
                        "Invalid hash_type '{raw}': expected one of data, type, data1, data2"
                    ))
                })?;
                resolve_script_identifier_for_form(state, &code_hash_bytes, hash_type)
            }
            None => resolve_script_identifier(state, &code_hash_bytes),
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct ScriptCapacityHistoryQuery {
    code_hash: Option<String>,
    script_kind: Option<String>,
    from: Option<String>,
    to: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ScriptCapacityHistoryByCodeHashQuery {
    code_hash: String,
    hash_type: Option<String>,
    script_kind: Option<String>,
    from: Option<String>,
    to: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodeCellResponse {
    pub tx_hash: Option<String>,
    pub output_index: Option<i32>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodeCellEntry {
    pub tx_hash: String,
    pub output_index: i32,
    pub status: &'static str,
    pub created_at_block: i64,
    pub capacity: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodeCellsResponse {
    pub code_cells: Vec<CodeCellEntry>,
    pub live_count: i64,
    pub total_count: i64,
    pub resolved_version_hash: Option<String>,
    pub ambiguity: Option<ScriptResolutionAmbiguityResponse>,
}

async fn get_code_cell(
    State(state): State<Arc<AppState>>,
    Query(params): Query<CodeCellQuery>,
) -> ApiResult<CodeCellResponse> {
    match params.resolve(&state)? {
        ScriptIdentifierResolution::Resolved(resolved) => {
            let ResolvedScriptIdentifier { mut code_cells, .. } = *resolved;
            sort_code_cells(&mut code_cells);
            let tx_hash = code_cells
                .first()
                .map(|(tx_hash, _, _, _)| format!("0x{}", hex::encode(tx_hash)));
            let output_index = code_cells
                .first()
                .map(|(_, output_index, _, _)| i32::from(*output_index));
            ok(CodeCellResponse {
                tx_hash,
                output_index,
            })
        }
        ScriptIdentifierResolution::Ambiguous(_) | ScriptIdentifierResolution::NotFound => {
            ok(CodeCellResponse {
                tx_hash: None,
                output_index: None,
            })
        }
    }
}

async fn get_code_cells(
    State(state): State<Arc<AppState>>,
    Query(params): Query<CodeCellQuery>,
) -> ApiResult<CodeCellsResponse> {
    match params.resolve(&state)? {
        ScriptIdentifierResolution::Resolved(resolved) => {
            let ResolvedScriptIdentifier {
                version_hash,
                mut code_cells,
                ..
            } = *resolved;
            sort_code_cells(&mut code_cells);
            let live_count = code_cells
                .iter()
                .filter(|(_, _, _, is_live)| *is_live)
                .count() as i64;
            let total_count = code_cells.len() as i64;
            let code_cells = code_cells
                .into_iter()
                .map(|(tx_hash, output_index, cell, is_live)| CodeCellEntry {
                    tx_hash: format!("0x{}", hex::encode(&tx_hash)),
                    output_index: i32::from(output_index),
                    status: if is_live { "live" } else { "consumed" },
                    created_at_block: cell.created_at_block,
                    capacity: cell.cell.capacity.to_string(),
                })
                .collect();

            ok(CodeCellsResponse {
                code_cells,
                live_count,
                total_count,
                resolved_version_hash: Some(format!("0x{}", hex::encode(version_hash))),
                ambiguity: None,
            })
        }
        ScriptIdentifierResolution::Ambiguous(ambiguous) => ok(CodeCellsResponse {
            code_cells: vec![],
            live_count: 0,
            total_count: 0,
            resolved_version_hash: None,
            ambiguity: Some(ScriptResolutionAmbiguityResponse {
                version_hashes: ambiguous
                    .version_hashes
                    .into_iter()
                    .map(|hash| format!("0x{}", hex::encode(hash)))
                    .collect(),
            }),
        }),
        ScriptIdentifierResolution::NotFound => ok(CodeCellsResponse {
            code_cells: vec![],
            live_count: 0,
            total_count: 0,
            resolved_version_hash: None,
            ambiguity: None,
        }),
    }
}

async fn list_scripts(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListParams>,
) -> ApiResult<ScriptFamiliesListResponse> {
    let limit = params.limit.clamp(1, 100) as usize;
    let _network = params.network.as_deref().unwrap_or(&state.ckb_network);
    let all_families = load_script_families_cached(&state)?;

    let search_pattern = params.search.as_ref().map(|s| s.to_lowercase());

    let mut filtered: Vec<_> = all_families
        .into_iter()
        .filter(|(_, info)| {
            // Exclude families with zero versions (e.g. other-network-only scripts
            // that leaked through a prior label import).
            if info.versions_count <= 0 {
                return false;
            }
            if let Some(ref pattern) = search_pattern {
                if !info.name.to_lowercase().contains(pattern) {
                    return false;
                }
            }
            true
        })
        .collect();

    let mut roles = HashMap::new();
    for (family_id, info) in &filtered {
        checked_family_totals(info, "list script families")?;
        if let Some(role) = script_kind_for_family(&state, info)? {
            roles.insert(family_id.clone(), role);
        }
    }
    let role_counts = ScriptRoleCountsResponse {
        all: filtered.len() as i64,
        locks: filtered
            .iter()
            .filter(|(id, _)| {
                roles
                    .get(id)
                    .is_some_and(|role| role == "lock" || role == "lock+type")
            })
            .count() as i64,
        types: filtered
            .iter()
            .filter(|(id, _)| {
                roles
                    .get(id)
                    .is_some_and(|role| role == "type" || role == "lock+type")
            })
            .count() as i64,
    };
    if let Some(role) = params.role.as_deref() {
        if !matches!(role, "lock" | "type") {
            return Err(ApiError::bad_request(format!(
                "Invalid script role {role:?}: expected lock or type"
            )));
        }
        filtered.retain(|(id, _)| {
            roles
                .get(id)
                .is_some_and(|actual| actual == role || actual == "lock+type")
        });
    }
    filtered.sort_by(|a, b| {
        compare_script_family_entries(a, b, params.sort_key, params.sort_direction, &roles)
    });

    let total = filtered.len() as i64;
    let all_items = filtered
        .iter()
        .map(|(family_id, info)| script_family_to_response(&state, family_id, info))
        .collect::<Result<Vec<_>, _>>()?;
    let mut class_totals: BTreeMap<String, (i64, i128, i64)> = BTreeMap::new();
    for item in &all_items {
        for class in &item.code_classes {
            let capacity: i128 = class.lock_owned_capacity_sum.parse().map_err(|e| {
                ApiError::internal(format!(
                    "invalid class capacity for family {}: {e}",
                    item.family_id
                ))
            })?;
            let entry = class_totals.entry(class.class.clone()).or_default();
            entry.0 = entry
                .0
                .checked_add(class.lock_cells_count)
                .ok_or_else(|| ApiError::internal("script list class cell count overflow"))?;
            entry.1 = entry
                .1
                .checked_add(capacity)
                .ok_or_else(|| ApiError::internal("script list class capacity overflow"))?;
            entry.2 = entry
                .2
                .checked_add(class.reference_count)
                .ok_or_else(|| ApiError::internal("script list class reference count overflow"))?;
        }
    }
    let class_summary = class_totals
        .into_iter()
        .map(|(class, (lock_cells_count, capacity, reference_count))| {
            ScriptCodeClassTotalResponse {
                class,
                lock_cells_count,
                lock_owned_capacity_sum: capacity.to_string(),
                reference_count,
            }
        })
        .collect();

    // An offset cursor, not a key — but a malformed one silently collapsing to
    // 0 hands the client page 1 again, so it is reported like every other
    // cursor in the API instead of being swallowed.
    let start_idx = match params.cursor.as_deref() {
        None | Some("") => 0usize,
        Some(cursor) => decode_cursor_single(cursor)
            .and_then(|v| usize::try_from(v).ok())
            .ok_or_else(|| {
                ApiError::bad_request(format!(
                    "Invalid cursor: expected a non-negative offset, got {cursor:?}"
                ))
            })?,
    };

    let page: Vec<_> = all_items
        .into_iter()
        .skip(start_idx)
        .take(limit + 1)
        .collect();
    let has_more = page.len() > limit;
    let page: Vec<_> = page.into_iter().take(limit).collect();

    let next_cursor = if has_more {
        Some(encode_cursor_single((start_idx + limit) as i64))
    } else {
        None
    };

    ok(ScriptFamiliesListResponse {
        page: CursorPaginatedResponse::new(page, total, limit as i64, next_cursor),
        class_summary,
        role_counts,
    })
}

async fn get_script(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> ApiResult<ScriptFamilyDetailResponse> {
    let (family_id, family) = load_script_family_by_name(&state, &name)?;
    let family_versions = load_family_versions(&state, &family_id)?;
    let (mut observed_references, all_references, members) =
        observed_references_by_version(&state, &family_id, &family_versions)?;
    let code_classes = class_totals_for_members(&state, &members)?;
    let mut versions = family_versions
        .iter()
        .map(|(version_hash, version_info)| {
            let refs = observed_references.remove(version_hash).unwrap_or_default();
            script_version_to_detail_response(&state, version_hash, version_info, refs)
        })
        .collect::<Result<Vec<_>, _>>()?;

    let history = script_history(&state, &family_versions, &versions)?;

    versions.sort_by(|left, right| {
        match (
            left.first_deployed_block.zip(left.first_deployed_tx_index),
            right
                .first_deployed_block
                .zip(right.first_deployed_tx_index),
        ) {
            (Some(left_position), Some(right_position)) => right_position.cmp(&left_position),
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => Ordering::Equal,
        }
        .then_with(|| left.version_hash.cmp(&right.version_hash))
    });

    let (owned_capacity_sum, owned_knowledge_sum) =
        checked_family_totals(&family, "script family detail")?;
    let script_kind = script_kind_for_family(&state, &family)?;

    ok(ScriptFamilyDetailResponse {
        family_id,
        name: family.name,
        description: family.description,
        script_kind,
        website: family.website,
        live_cells_count: family.live_cells_count,
        cells_count: family.cells_count,
        owned_capacity_sum: owned_capacity_sum.to_string(),
        owned_knowledge_sum: owned_knowledge_sum.to_string(),
        versions_count: family.versions_count,
        versions,
        references: all_references,
        code_classes,
        history,
    })
}

async fn get_script_usage(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> ApiResult<ScriptUsageResponse> {
    let (family_id, family) = load_script_family_by_name(&state, &name)?;
    let versions = load_family_versions(&state, &family_id)?;
    let allowed_versions = versions.iter().map(|(hash, _)| hash.clone()).collect();
    let registered_references = registered_reference_set(&versions);
    let members = family_member_reference_forms(
        &state,
        &family_id,
        Some(&family_id),
        &allowed_versions,
        &registered_references,
    )?;
    let mut usage = ScriptFamilyUsage::default();
    for member in members {
        usage
            .add_reference(&member.info)
            .map_err(|e| ApiError::internal(e.to_string()))?;
    }
    usage
        .validate_persisted(&family)
        .map_err(|e| ApiError::internal(e.to_string()))?;

    // Family totals include retired references. Per-binary rows describe only
    // currently bound versions and need not sum to the family provenance totals.
    let mut by_deployment: Vec<DeploymentUsage> = versions
        .into_iter()
        .map(|(version_hash, info)| {
            let (
                cells_count,
                live_cells_count,
                capacity_sum,
                owned_capacity_sum,
                used_capacity_sum,
                owned_knowledge_sum,
            ) = checked_version_totals(&version_hash, &info, "script family usage")?;
            let capacity_sum = capacity_sum as u128;
            let owned_capacity_sum = owned_capacity_sum as u128;
            let used_capacity_sum = used_capacity_sum as u128;
            let owned_knowledge_sum = owned_knowledge_sum as u128;

            Ok(DeploymentUsage {
                code_hash: format!("0x{}", hex::encode(&version_hash)),
                script_kind: version_script_kind(&info),
                cells_count,
                live_cells_count,
                capacity_sum: capacity_sum.to_string(),
                owned_capacity_sum: owned_capacity_sum.to_string(),
                used_capacity_sum: used_capacity_sum.to_string(),
                owned_knowledge_sum: owned_knowledge_sum.to_string(),
            })
        })
        .collect::<Result<Vec<_>, ApiRouteError>>()?;
    by_deployment.sort_by(|left, right| {
        right
            .live_cells_count
            .cmp(&left.live_cells_count)
            .then_with(|| left.code_hash.cmp(&right.code_hash))
    });

    ok(ScriptUsageResponse {
        name,
        cells_count: family.cells_count,
        live_cells_count: family.live_cells_count,
        capacity_sum: usage.capacity_sum.to_string(),
        owned_capacity_sum: family.owned_capacity_sum.to_string(),
        used_capacity_sum: usage.used_capacity_sum.to_string(),
        owned_knowledge_sum: family.owned_knowledge_sum.to_string(),
        by_deployment,
    })
}

fn format_yyyymmdd_for_chart(date_yyyymmdd: u32) -> String {
    let date = format!("{date_yyyymmdd:08}");
    format!("{}-{}-{}", &date[0..4], &date[4..6], &date[6..8])
}

fn parse_script_kind_filter(script_kind: Option<&str>) -> Result<Vec<bool>, ApiRouteError> {
    match script_kind {
        None => Ok(vec![false, true]),
        Some("lock") => Ok(vec![false]),
        Some("type") => Ok(vec![true]),
        Some("both") | Some("lock+type") => Ok(vec![false, true]),
        Some(_) => Err(ApiError::bad_request(
            "Invalid script_kind, expected lock/type/both",
        )),
    }
}

fn apply_script_chart_delta(
    cumulative_capacity: i128,
    cumulative_used: i128,
    cap_delta: i128,
    used_delta: i128,
    context: &str,
) -> Result<(i128, i128), ApiRouteError> {
    apply_owned_capacity_delta(
        cumulative_capacity,
        cumulative_used,
        cap_delta,
        used_delta,
        context,
    )
    .map_err(|e| ApiError::internal(e.to_string()))
}

fn latest_complete_script_chart_date_from_tip(
    tip_chain_time_ms: i64,
) -> Result<u32, ApiRouteError> {
    let tip_date = ckbadger_common::stats_bucket::stats_day(tip_chain_time_ms)
        .map_err(|e| ApiError::internal(format!("invalid script chart tip chain time: {e}")))?;
    let latest_complete = tip_date
        .checked_sub_signed(chrono::Duration::days(1))
        .ok_or_else(|| {
            ApiError::internal(format!(
                "tip chain time does not have a previous CKB chart day: tip_chain_time_ms={}",
                tip_chain_time_ms
            ))
        })?;

    latest_complete
        .format("%Y%m%d")
        .to_string()
        .parse::<u32>()
        .map_err(|e| {
            ApiError::internal(format!(
                "failed to encode latest complete script chart date: tip_chain_time_ms={}, date={}, error={}",
                tip_chain_time_ms,
                latest_complete,
                e
            ))
        })
}

fn latest_complete_script_chart_date(state: &AppState) -> Result<u32, ApiRouteError> {
    let (_, header) = state
        .store
        .get_sync_tip_block()
        .map_err(|e| ApiError::internal(e.to_string()))?
        .ok_or_else(|| {
            ApiError::internal(
                "script capacity history requires a sync tip block when script daily deltas exist",
            )
        })?;

    latest_complete_script_chart_date_from_tip(header.chain_time_ms)
}

fn resolve_script_capacity_chart_bounds(
    daily_deltas: &BTreeMap<u32, (i128, i128)>,
    from_date: Option<u32>,
    to_date: Option<u32>,
    default_end_date: Option<u32>,
    has_history: bool,
) -> Option<(u32, u32)> {
    if !has_history {
        return None;
    }

    let first_delta_date = daily_deltas.keys().next().copied();
    let bounds = match (from_date, to_date) {
        (Some(from), Some(to)) => Some((from, to)),
        (Some(from), None) => default_end_date.map(|end| (from, end)),
        (None, Some(to)) => first_delta_date.map(|first| (first, to)),
        (None, None) => first_delta_date.zip(default_end_date),
    };

    bounds.filter(|(start, end)| start <= end)
}

fn build_script_capacity_history_chart(
    state: &AppState,
    targets: Vec<(Vec<u8>, u8, bool)>,
    title: String,
    from_date: Option<u32>,
    to_date: Option<u32>,
) -> Result<StackedAreaChartResponse, ApiRouteError> {
    let series = vec![
        StackedAreaSeries {
            key: "used".to_string(),
            label: "Used".to_string(),
            color: "#f59e0b".to_string(),
        },
        StackedAreaSeries {
            key: "unused".to_string(),
            label: "Unused".to_string(),
            color: "#00c389".to_string(),
        },
    ];

    if targets.is_empty() {
        return Ok(StackedAreaChartResponse {
            data: vec![],
            series,
            title,
        });
    }

    let mut dedup = HashSet::new();
    let unique_targets: Vec<(Vec<u8>, u8, bool)> = targets
        .into_iter()
        .filter(|target| dedup.insert(target.clone()))
        .collect();

    let mut cumulative_capacity: i128 = 0;
    let mut cumulative_used: i128 = 0;
    if let Some(from) = from_date {
        let mut baseline_daily: BTreeMap<u32, (i128, i128)> = BTreeMap::new();
        for (code_hash, hash_type, is_type) in &unique_targets {
            let baseline = state
                .store
                .list_script_daily_deltas_in_range(
                    code_hash,
                    *hash_type,
                    *is_type,
                    None,
                    Some(from.saturating_sub(1)),
                )
                .map_err(|e| ApiError::internal(e.to_string()))?;
            for (date, delta) in baseline {
                let entry = baseline_daily.entry(date).or_insert((0, 0));
                entry.0 += delta.owned_capacity_delta;
                entry.1 += delta.owned_knowledge_delta;
            }
        }
        for (_, (cap_delta, used_delta)) in baseline_daily {
            (cumulative_capacity, cumulative_used) = apply_script_chart_delta(
                cumulative_capacity,
                cumulative_used,
                cap_delta,
                used_delta,
                "building script baseline capacity history chart",
            )?;
        }
    }

    let mut daily_deltas: BTreeMap<u32, (i128, i128)> = BTreeMap::new();
    for (code_hash, hash_type, is_type) in &unique_targets {
        let deltas = state
            .store
            .list_script_daily_deltas_in_range(code_hash, *hash_type, *is_type, from_date, to_date)
            .map_err(|e| ApiError::internal(e.to_string()))?;
        for (date, delta) in deltas {
            let entry = daily_deltas.entry(date).or_insert((0, 0));
            entry.0 += delta.owned_capacity_delta;
            entry.1 += delta.owned_knowledge_delta;
        }
    }

    let has_history = !daily_deltas.is_empty() || cumulative_capacity != 0 || cumulative_used != 0;
    let default_end_date = if to_date.is_none() && has_history {
        Some(latest_complete_script_chart_date(state)?)
    } else {
        None
    };
    let chart_bounds = resolve_script_capacity_chart_bounds(
        &daily_deltas,
        from_date,
        to_date,
        default_end_date,
        has_history,
    );
    let dates = if let Some((start, end)) = chart_bounds {
        date_keys_inclusive(start, end).map_err(ApiError::internal)?
    } else {
        Vec::new()
    };

    let mut data = Vec::with_capacity(dates.len());
    for date in dates {
        let (cap_delta, used_delta) = daily_deltas.get(&date).copied().unwrap_or((0, 0));
        (cumulative_capacity, cumulative_used) = apply_script_chart_delta(
            cumulative_capacity,
            cumulative_used,
            cap_delta,
            used_delta,
            &format!("building script capacity history chart at date {}", date),
        )?;
        let unused = cumulative_capacity - cumulative_used;
        data.push(StackedAreaDataPoint {
            date: format_yyyymmdd_for_chart(date),
            values: HashMap::from([
                ("used".to_string(), cumulative_used.to_string()),
                ("unused".to_string(), unused.to_string()),
            ]),
        });
    }

    Ok(StackedAreaChartResponse {
        data,
        series,
        title,
    })
}

async fn get_script_capacity_history_chart(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
    Query(params): Query<ScriptCapacityHistoryQuery>,
) -> ApiResult<StackedAreaChartResponse> {
    let (from_date, to_date) = parse_chart_date_range(params.from.as_deref(), params.to.as_deref())
        .map_err(|msg| ApiError::bad_request(&msg))?;

    let code_hash_filter = params
        .code_hash
        .as_deref()
        .map(|raw| parse_hash32(raw, "code_hash"))
        .transpose()?;
    let kind_filter = parse_script_kind_filter(params.script_kind.as_deref())?;

    // The chart aggregates exactly the family's member reference forms — the
    // same membership computation the usage counters and family detail use.
    let (family_id, _family) = load_script_family_by_name(&state, &name)?;
    let family_versions = load_family_versions(&state, &family_id)?;
    let allowed_versions: HashSet<Vec<u8>> = family_versions
        .iter()
        .map(|(version_hash, _)| version_hash.clone())
        .collect();
    let registered_references = registered_reference_set(&family_versions);
    let member_forms = family_member_reference_forms(
        &state,
        &family_id,
        Some(&family_id),
        &allowed_versions,
        &registered_references,
    )?;

    let mut targets = Vec::new();
    for member in member_forms {
        if code_hash_filter
            .as_ref()
            .map(|filter| &member.reference_hash == filter)
            .unwrap_or(true)
        {
            for is_type in &kind_filter {
                targets.push((member.reference_hash.clone(), member.hash_type, *is_type));
            }
        }
    }

    ok(build_script_capacity_history_chart(
        &state,
        targets,
        format!("{name} Capacity History"),
        from_date,
        to_date,
    )?)
}

/// Capacity-history chart addressed by code_hash.
///
/// With an explicit `hash_type` parameter the chart covers exactly the single
/// observed reference form `(code_hash, hash_type)` — no resolution.
///
/// Without `hash_type` the reference is resolved and the chart covers the
/// member reference set of the resolved version's family — the same set the
/// `/scripts/{name}/charts/capacity-history` path aggregates, so both
/// endpoints and the family counters share one membership computation. A
/// resolved version without a family charts the forms observed for that
/// version alone; an unknown reference charts every form recorded under the
/// code_hash bytes.
async fn get_script_capacity_history_chart_by_code_hash(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ScriptCapacityHistoryByCodeHashQuery>,
) -> ApiResult<StackedAreaChartResponse> {
    let (from_date, to_date) = parse_chart_date_range(params.from.as_deref(), params.to.as_deref())
        .map_err(|msg| ApiError::bad_request(&msg))?;

    let code_hash = parse_hash32(&params.code_hash, "code_hash")?;
    let kind_filter = parse_script_kind_filter(params.script_kind.as_deref())?;
    let title = format!("0x{} Capacity History", hex::encode(&code_hash));

    if let Some(hash_type_str) = params.hash_type.as_deref() {
        let hash_type = hash_type_to_u8(hash_type_str).ok_or_else(|| {
            ApiError::bad_request("Invalid hash_type, expected data/type/data1/data2")
        })?;
        let targets = kind_filter
            .iter()
            .map(|is_type| (code_hash.clone(), hash_type, *is_type))
            .collect();
        return ok(build_script_capacity_history_chart(
            &state, targets, title, from_date, to_date,
        )?);
    }

    let mut targets = Vec::new();
    match resolve_script_identifier(&state, &code_hash)? {
        ScriptIdentifierResolution::Resolved(resolved) => {
            let ResolvedScriptIdentifier {
                version_hash,
                version_info,
                ..
            } = *resolved;
            let (allowed_versions, registered_references, membership_context) =
                if let Some(family_id) = version_info.family_id.as_deref() {
                    let family_versions = load_family_versions(&state, family_id)?;
                    let allowed: HashSet<Vec<u8>> = family_versions
                        .iter()
                        .map(|(version_hash, _)| version_hash.clone())
                        .collect();
                    let registered = registered_reference_set(&family_versions);
                    (allowed, registered, family_id.to_string())
                } else {
                    (
                        HashSet::from([version_hash.clone()]),
                        HashSet::new(),
                        format!("version 0x{}", hex::encode(&version_hash)),
                    )
                };
            let member_forms = family_member_reference_forms(
                &state,
                &membership_context,
                version_info.family_id.as_deref(),
                &allowed_versions,
                &registered_references,
            )?;
            for member in member_forms {
                for is_type in &kind_filter {
                    targets.push((member.reference_hash.clone(), member.hash_type, *is_type));
                }
            }
        }
        ScriptIdentifierResolution::Ambiguous(_) => {
            return Err(ApiError::bad_request(
                "script capacity history requires an unambiguous script reference",
            ));
        }
        ScriptIdentifierResolution::NotFound => {
            for hash_type in [0u8, 1u8, 2u8, 4u8] {
                for is_type in &kind_filter {
                    targets.push((code_hash.clone(), hash_type, *is_type));
                }
            }
        }
    }

    ok(build_script_capacity_history_chart(
        &state, targets, title, from_date, to_date,
    )?)
}

#[cfg(test)]
mod tests {
    use super::{
        apply_script_chart_delta, checked_capacity_totals,
        latest_complete_script_chart_date_from_tip, reference_own_stats, resolve_code_cell,
        resolve_script_capacity_chart_bounds, verified_multisig_config, CodeCellQuery, ListParams,
    };
    use axum::extract::Query;
    use axum::http::StatusCode;
    use axum::http::Uri;
    use ckb_types::{packed, prelude::*};
    use ckbadger_store::ScriptInfo;
    use std::collections::BTreeMap;

    #[test]
    fn multisig_config_is_reported_only_when_witness_matches_lock_args() {
        let mut config = vec![0, 1, 3, 5];
        for key in 1..=5 {
            config.extend([key; 20]);
        }
        let args = ckb_hash::blake2b_256(&config)[..20].to_vec();
        let mut lock = config;
        lock.extend([0u8; 65 * 3]);
        let witness = packed::WitnessArgs::new_builder()
            .lock(
                packed::BytesOpt::new_builder()
                    .set(Some(lock.pack()))
                    .build(),
            )
            .build();
        let custody = verified_multisig_config(&args, witness.as_slice(), &[0x55; 32]).unwrap();
        assert_eq!(custody.threshold, Some(3));
        assert_eq!(custody.key_count, Some(5));
        assert_eq!(custody.require_first_n, Some(1));
        assert_eq!(custody.key_hashes.len(), 5);
        assert_eq!(custody.witness_state, "verified");
        let mut wrong = args;
        wrong[0] ^= 1;
        assert!(verified_multisig_config(&wrong, witness.as_slice(), &[0x55; 32]).is_err());
    }

    #[test]
    fn apply_script_chart_delta_accepts_delta_beyond_i64() {
        let huge = i128::from(i64::MAX) + 1;
        let (capacity, used) = apply_script_chart_delta(0, 0, huge, 0, "script chart").unwrap();
        assert_eq!(capacity, huge);
        assert_eq!(used, 0);
    }

    #[test]
    fn apply_script_chart_delta_errors_on_invariant_violation() {
        let err = apply_script_chart_delta(10, 5, 0, 10, "script chart").unwrap_err();
        assert_eq!(err.0, StatusCode::INTERNAL_SERVER_ERROR);
        assert!(err
            .1
             .0
            .message
            .contains("owned knowledge exceeds owned capacity"));
    }

    #[test]
    fn checked_capacity_totals_errors_on_negative_capacity() {
        let info = ScriptInfo {
            code_hash: vec![0xAA; 32],
            lock_owned_capacity_sum: -1,
            ..Default::default()
        };
        let err = checked_capacity_totals(&info, "test").unwrap_err();
        assert_eq!(err.0, StatusCode::INTERNAL_SERVER_ERROR);
        assert!(err.1 .0.message.contains("negative live capacity"));
    }

    #[test]
    fn checked_capacity_totals_errors_when_occupied_exceeds_capacity() {
        let info = ScriptInfo {
            code_hash: vec![0xBB; 32],
            lock_owned_capacity_sum: 100,
            lock_owned_knowledge_sum: 101,
            ..Default::default()
        };
        let err = checked_capacity_totals(&info, "test").unwrap_err();
        assert_eq!(err.0, StatusCode::INTERNAL_SERVER_ERROR);
        assert!(err
            .1
             .0
            .message
            .contains("live common knowledge size exceeds total"));
    }

    #[test]
    fn latest_complete_script_chart_date_from_tip_uses_ckb_day_boundary() {
        assert_eq!(
            latest_complete_script_chart_date_from_tip(1_705_536_000_000).unwrap(),
            20240117
        );
    }

    #[test]
    fn resolve_script_capacity_chart_bounds_extends_unbounded_chart_to_latest_complete_day() {
        let daily_deltas = BTreeMap::from([(20240115, (100, 40))]);

        assert_eq!(
            resolve_script_capacity_chart_bounds(&daily_deltas, None, None, Some(20240117), true,),
            Some((20240115, 20240117))
        );
    }

    #[test]
    fn resolve_script_capacity_chart_bounds_preserves_from_only_flat_history() {
        let daily_deltas = BTreeMap::new();

        assert_eq!(
            resolve_script_capacity_chart_bounds(
                &daily_deltas,
                Some(20240116),
                None,
                Some(20240117),
                true,
            ),
            Some((20240116, 20240117))
        );
    }

    #[test]
    fn list_params_deserializes_unused_query_schema_fields() {
        let uri: Uri = "/scripts?decoder_type=typeid&cursor=cursor-token&limit=25"
            .parse()
            .unwrap();
        let Query(params) = Query::<ListParams>::try_from_uri(&uri).unwrap();

        assert_eq!(params.cursor.as_deref(), Some("cursor-token"));
        assert_eq!(params.limit, 25);
    }

    #[test]
    fn code_cell_query_keeps_hash_type_field() {
        let uri: Uri = "/scripts/code-cell?code_hash=0x1234&hash_type=data"
            .parse()
            .unwrap();
        let Query(params) = Query::<CodeCellQuery>::try_from_uri(&uri).unwrap();

        assert_eq!(params.code_hash, "0x1234");
        assert_eq!(params.hash_type.as_deref(), Some("data"));

        let uri: Uri = "/scripts/code-cell?code_hash=0x1234".parse().unwrap();
        let Query(params) = Query::<CodeCellQuery>::try_from_uri(&uri).unwrap();
        assert_eq!(params.hash_type, None);
    }

    /// When type-ref lookup fails (dep_type_hash set but no matching live cell),
    /// resolve_code_cell must fall through to the data-hash lookup instead of
    /// returning (None, None).
    #[test]
    fn resolve_code_cell_falls_through_type_to_data_hash() {
        use ckbadger_store::{CkbadgerStore, LiveCellInfo, StoreBatch};
        use std::sync::Arc;

        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_test_unified(dir.path()).unwrap());

        let tx_hash = vec![0xCA; 32];
        let output_index: i16 = 0;
        let block_num: i64 = 5;
        let data_hash = vec![0xDD; 32];

        // Write a live cell that is findable by data_hash.
        let outpoint_key = ckbadger_store::keys::encode_outpoint(&tx_hash, output_index);
        let cell_info = LiveCellInfo {
            capacity: 200_00000000,
            lock_script_hash: vec![0x11; 32],
            lock_code_hash: vec![0x22; 32],
            lock_hash_type: 1,
            lock_args: vec![0x33; 20],
            type_script_hash: None,
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            data_size: 100,
            occupied_capacity: 100_00000000,
            udt_amount: None,
            data_hash: None,
        };
        let marker = ckbadger_store::types::encode_live_cell_marker(block_num);
        let payload = bincode::serialize(&cell_info).unwrap();
        store
            .put_cf(store.cf_live_cells(), &outpoint_key, &marker)
            .unwrap();
        store
            .put_cf(store.cf_cells(), &outpoint_key, &payload)
            .unwrap();

        // Write CF_CELL_BY_DATA_HASH index entry.
        let mut batch = StoreBatch::new(&store);
        batch.put_cell_by_data_hash(&data_hash, block_num, &tx_hash, output_index);
        batch.commit().unwrap();

        // ScriptInfo with dep_type_hash set (triggers type-ref path) but no matching cell,
        // AND data_ref available via hash_type=0.
        let info = ScriptInfo {
            code_hash: data_hash.clone(),
            hash_type: 0,
            dep_type_hash: Some(vec![0xFF; 32]), // no live cell with this type hash
            ..Default::default()
        };

        let (resolved_tx, resolved_idx) = resolve_code_cell(&info, &store, &store).unwrap();
        assert_eq!(
            resolved_tx.as_deref(),
            Some(format!("0x{}", hex::encode(&tx_hash)).as_str()),
            "should resolve code cell via data_hash fallback"
        );
        assert_eq!(resolved_idx, Some(0));
    }

    /// When the code cell has been consumed, resolve_code_cell must still find it
    /// via the preserved CF_CELL_BY_DATA_HASH index entry + consumed cell lookup.
    #[test]
    fn resolve_code_cell_finds_consumed_code_cell_by_data_hash() {
        use ckbadger_store::{CkbadgerStore, LiveCellInfo, StoreBatch};
        use std::sync::Arc;

        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_test_unified(dir.path()).unwrap());

        let tx_hash = vec![0xCB; 32];
        let output_index: i16 = 0;
        let block_num: i64 = 5;
        let data_hash = vec![0xEE; 32];

        // Write cell payload (append-only — never deleted).
        let outpoint_key = ckbadger_store::keys::encode_outpoint(&tx_hash, output_index);
        let cell_info = LiveCellInfo {
            capacity: 200_00000000,
            lock_script_hash: vec![0x11; 32],
            lock_code_hash: vec![0x22; 32],
            lock_hash_type: 1,
            lock_args: vec![0x33; 20],
            type_script_hash: None,
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            data_size: 100,
            occupied_capacity: 100_00000000,
            udt_amount: None,
            data_hash: None,
        };
        let payload = bincode::serialize(&cell_info).unwrap();
        store
            .put_cf(store.cf_cells(), &outpoint_key, &payload)
            .unwrap();

        // Write consumed cell metadata (not live — no CF_LIVE_CELLS entry).
        let mut batch = StoreBatch::new(&store);
        batch.put_consumed_cell(&tx_hash, output_index, &cell_info, block_num, 10);
        batch.commit().unwrap();

        // Write CF_CELL_BY_DATA_HASH index entry (preserved on consumption).
        let mut batch = StoreBatch::new(&store);
        batch.put_cell_by_data_hash(&data_hash, block_num, &tx_hash, output_index);
        batch.commit().unwrap();

        let info = ScriptInfo {
            code_hash: data_hash.clone(),
            hash_type: 0, // data hash type
            ..Default::default()
        };

        let (resolved_tx, resolved_idx) = resolve_code_cell(&info, &store, &store).unwrap();
        assert_eq!(
            resolved_tx.as_deref(),
            Some(format!("0x{}", hex::encode(&tx_hash)).as_str()),
            "should resolve consumed code cell via data_hash index"
        );
        assert_eq!(resolved_idx, Some(0));
    }

    #[test]
    fn reference_own_stats_serves_the_reference_rows_own_numbers() {
        let info = ScriptInfo {
            code_hash: vec![0xAA; 32],
            lock_live_cells_count: 3,
            type_live_cells_count: 2,
            lock_owned_capacity_sum: 100_00000000,
            type_owned_capacity_sum: 50_00000000,
            lock_owned_knowledge_sum: 61_00000000,
            type_owned_knowledge_sum: 10_00000000,
            ..Default::default()
        };
        let (live, owned_capacity, owned_knowledge) = reference_own_stats(Some(&info)).unwrap();
        assert_eq!(live, 5);
        assert_eq!(owned_capacity, 150_00000000);
        assert_eq!(owned_knowledge, 71_00000000);
    }

    #[test]
    fn reference_own_stats_reports_zero_for_unindexed_reference() {
        // A reference with no ScriptInfo row has no indexed usage; zero is the
        // truth, not a fallback into some other deployment's numbers.
        assert_eq!(reference_own_stats(None).unwrap(), (0, 0, 0));
    }

    #[test]
    fn reference_own_stats_rejects_invalid_reference_totals() {
        let info = ScriptInfo {
            code_hash: vec![0xEE; 32],
            lock_owned_capacity_sum: 100,
            lock_owned_knowledge_sum: 101,
            ..Default::default()
        };
        let err = reference_own_stats(Some(&info)).unwrap_err();
        assert_eq!(err.0, StatusCode::INTERNAL_SERVER_ERROR);
        assert!(err
            .1
             .0
            .message
            .contains("live common knowledge size exceeds total"));
    }
}
