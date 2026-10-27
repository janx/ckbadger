//! Canonical item cell versions. This reader never treats append-only payloads
//! alone as chain membership and never writes either chain store.
use anyhow::{anyhow, ensure, Context};
use axum::{
    extract::{Path, State},
    routing::get,
    Router,
};
use ckbadger_store::{IdentityStandard, LiveCellInfo, ObjectStandard};
use serde::Serialize;
use std::sync::Arc;

use super::assets::NftLifecycleStandard;
use crate::{
    response::{ok, ApiError, ApiResult, ScriptResponse},
    utils::parse_asset_id_max32,
    AppState,
};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CellAnatomy {
    capacity_bytes: usize,
    lock_bytes: usize,
    type_bytes: usize,
    data_bytes: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CellVersion {
    pub tx_hash: String,
    pub output_index: i16,
    pub block_number: i64,
    pub tx_index: i32,
    pub timestamp: String,
    pub is_live: bool,
    pub consumed_by_tx: Option<String>,
    pub consumed_at_block: Option<i64>,
    pub owner_lock_hash: String,
    pub owner_address: String,
    pub capacity: String,
    pub occupied_capacity: String,
    pub lock: ScriptResponse,
    #[serde(rename = "type")]
    pub type_script: Option<ScriptResponse>,
    pub anatomy: CellAnatomy,
    pub data_hash: Option<String>,
    pub lock_name: Option<String>,
    pub type_name: Option<String>,
}

pub(crate) struct CanonicalCellVersion {
    pub response: CellVersion,
    pub tx_hash: Vec<u8>,
    pub cell: LiveCellInfo,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CellVersionsResponse {
    data: Vec<CellVersion>,
    total: usize,
}

pub(crate) fn item_outpoints(
    state: &AppState,
    id: &[u8],
    standard: NftLifecycleStandard,
) -> anyhow::Result<Vec<(Vec<u8>, i16)>> {
    match standard {
        NftLifecycleStandard::MnftToken => state.store.list_mnft_token_outpoints_by_token_id(id),
        NftLifecycleStandard::DotBit => state.store.list_dotbit_account_outpoints_by_account_id(id),
        _ => state.store.list_spore_outpoints_by_spore_id(id),
    }
}

fn script(code: &[u8], hash_type: i16, args: &[u8]) -> anyhow::Result<ScriptResponse> {
    let byte = u8::try_from(hash_type).context("invalid script hash_type")?;
    let label = ckbadger_common::hash_type_label(byte)
        .ok_or_else(|| anyhow!("invalid script hash_type {hash_type}"))?;
    ensure!(code.len() == 32, "script code hash width {}", code.len());
    Ok(ScriptResponse {
        code_hash: format!("0x{}", hex::encode(code)),
        hash_type: label.to_string(),
        args: format!("0x{}", hex::encode(args)),
    })
}

pub(crate) fn canonical_version_at(
    state: &AppState,
    id: &[u8],
    tx_hash: &[u8],
    index: i16,
) -> anyhow::Result<CanonicalCellVersion> {
    let context = format!(
        "item=0x{} outpoint=0x{}:{index}",
        hex::encode(id),
        hex::encode(tx_hash)
    );
    ensure!(tx_hash.len() == 32 && index >= 0, "invalid {context}");
    let live = state
        .store
        .get_cell(tx_hash, index, &state.append_only_store)
        .with_context(|| context.clone())?;
    let consumed = state
        .store
        .get_consumed_cell_info(tx_hash, index, &state.append_only_store)
        .with_context(|| context.clone())?;
    let (cell, block, consumed_by_tx, consumed_at_block, consumer_location, is_live) =
        match (live, consumed) {
            (Some(_), Some(_)) => {
                return Err(anyhow!(
                    "both live and consumed canonical markers: {context}"
                ))
            }
            (None, None) => {
                return Err(anyhow!(
                    "item reverse index has no canonical marker: {context}"
                ))
            }
            (Some(c), None) => (c.cell, c.created_at_block, None, None, None, true),
            (None, Some(c)) => {
                let consumer = c
                    .consumed_by_tx
                    .ok_or_else(|| anyhow!("consumed marker missing consumer: {context}"))?;
                let (number, consumer_index, _) = state
                    .store
                    .get_tx_by_hash(&consumer)?
                    .ok_or_else(|| anyhow!("consumer missing canonical transaction: {context}"))?;
                ensure!(
                    number == c.consumed_at_block && number >= c.created_at_block,
                    "consumer block mismatch: {context}"
                );
                (
                    c.cell,
                    c.created_at_block,
                    Some(consumer),
                    Some(c.consumed_at_block),
                    Some((number, consumer_index)),
                    false,
                )
            }
        };
    let (tx_block, tx_index, tx) = state
        .store
        .get_tx_by_hash(tx_hash)?
        .ok_or_else(|| anyhow!("creation missing canonical transaction: {context}"))?;
    ensure!(
        tx_block == block && index < tx.outputs_count,
        "creation location/output mismatch: {context}"
    );
    if let Some(consumer) = consumer_location {
        ensure!(
            consumer > (tx_block, tx_index),
            "invalid consumer order: {context}, creation={tx_block}:{tx_index}, consumer={}:{}",
            consumer.0,
            consumer.1
        );
    }
    let lock = script(&cell.lock_code_hash, cell.lock_hash_type, &cell.lock_args)
        .with_context(|| format!("owner address script invalid: {context}"))?;
    ensure!(
        crate::utils::address::compute_script_hash(
            &cell.lock_code_hash,
            u8::try_from(cell.lock_hash_type)?,
            &cell.lock_args
        ) == cell.lock_script_hash,
        "lock script hash mismatch: {context}"
    );
    let type_script = match (
        &cell.type_code_hash,
        cell.type_hash_type,
        &cell.type_args,
        &cell.type_script_hash,
    ) {
        (Some(code), Some(ht), Some(args), Some(hash)) => {
            let response = script(code, ht, args).with_context(|| context.clone())?;
            ensure!(
                crate::utils::address::compute_script_hash(code, u8::try_from(ht)?, args) == *hash,
                "type script hash mismatch: {context}"
            );
            Some(response)
        }
        (None, None, None, None) => None,
        _ => {
            return Err(anyhow!(
                "partial type script (missing or invalid hash_type): {context}"
            ))
        }
    };
    let data_bytes = usize::try_from(cell.data_size)
        .with_context(|| format!("negative data length: {context}"))?;
    let lock_bytes = 33 + cell.lock_args.len();
    let type_bytes = cell.type_args.as_ref().map_or(0, |args| 33 + args.len());
    let occupied = i64::try_from(8 + lock_bytes + type_bytes + data_bytes)?
        .checked_mul(100_000_000)
        .ok_or_else(|| anyhow!("occupied capacity overflow: {context}"))?;
    ensure!(
        cell.occupied_capacity == occupied && cell.capacity >= occupied,
        "cell occupied/capacity mismatch: {context}, stored={} actual={occupied} capacity={}",
        cell.occupied_capacity,
        cell.capacity
    );
    let owner_address = crate::utils::address::encode_lock_address(
        &cell.lock_script_hash,
        &cell.lock_code_hash,
        cell.lock_hash_type,
        &cell.lock_args,
        &state.ckb_network,
    )?;
    Ok(CanonicalCellVersion {
        response: CellVersion {
            tx_hash: format!("0x{}", hex::encode(tx_hash)),
            output_index: index,
            block_number: block,
            tx_index,
            timestamp: tx.timestamp.to_string(),
            is_live,
            consumed_by_tx: consumed_by_tx
                .as_ref()
                .map(|h| format!("0x{}", hex::encode(h))),
            consumed_at_block,
            owner_lock_hash: format!("0x{}", hex::encode(&cell.lock_script_hash)),
            owner_address,
            capacity: cell.capacity.to_string(),
            occupied_capacity: occupied.to_string(),
            lock,
            type_script,
            anatomy: CellAnatomy {
                capacity_bytes: 8,
                lock_bytes,
                type_bytes,
                data_bytes,
            },
            data_hash: cell
                .data_hash
                .as_ref()
                .map(|h| format!("0x{}", hex::encode(h))),
            lock_name: state
                .store
                .get_script_info(&cell.lock_code_hash)?
                .and_then(|info| info.name),
            type_name: match &cell.type_code_hash {
                Some(code) => state
                    .store
                    .get_script_info(code)?
                    .and_then(|info| info.name),
                None => None,
            },
        },
        tx_hash: tx_hash.to_vec(),
        cell,
    })
}

pub(crate) fn canonical_versions(
    state: &AppState,
    id: &[u8],
    standard: NftLifecycleStandard,
) -> anyhow::Result<Vec<CanonicalCellVersion>> {
    let mut versions = Vec::new();
    for (tx_hash, index) in item_outpoints(state, id, standard)? {
        let context = format!(
            "item=0x{} outpoint=0x{}:{index}",
            hex::encode(id),
            hex::encode(&tx_hash)
        );
        ensure!(tx_hash.len() == 32 && index >= 0, "invalid {context}");
        let forward = match standard {
            NftLifecycleStandard::MnftToken => {
                state.store.get_mnft_token_id_by_outpoint(&tx_hash, index)?
            }
            NftLifecycleStandard::DotBit => state
                .store
                .get_dotbit_account_id_by_outpoint(&tx_hash, index)?,
            _ => state.store.get_spore_id_by_outpoint(&tx_hash, index)?,
        };
        ensure!(
            forward.as_deref() == Some(id),
            "item reverse history disagrees with forward outpoint index: {context}"
        );
        versions.push(canonical_version_at(state, id, &tx_hash, index)?);
    }
    versions.sort_by_key(|v| {
        (
            v.response.block_number,
            v.response.tx_index,
            v.response.output_index,
        )
    });
    ensure!(
        versions.iter().filter(|v| v.response.is_live).count() <= 1,
        "multiple live item cells: item=0x{}",
        hex::encode(id)
    );
    Ok(versions)
}

pub(crate) fn parse_item_id(
    raw: &str,
    standard: NftLifecycleStandard,
) -> Result<Vec<u8>, crate::response::ApiRouteError> {
    if matches!(standard, NftLifecycleStandard::DotCell) {
        return super::identities::decode_dotcell_item_ref(raw).map(|id| id.to_vec());
    }
    parse_asset_id_max32(raw, "item ID")
}
pub(crate) fn checked_versions(
    state: &AppState,
    id: &[u8],
    standard: NftLifecycleStandard,
) -> Result<Vec<CanonicalCellVersion>, crate::response::ApiRouteError> {
    let (expected_live, expected_owner) = match standard {
        NftLifecycleStandard::Spore | NftLifecycleStandard::MnftToken => {
            let object = if matches!(standard, NftLifecycleStandard::Spore) {
                state.store.get_spore(id)
            } else {
                state.store.get_mnft(id)
            }
            .map_err(|e| ApiError::internal(e.to_string()))?
            .ok_or_else(|| ApiError::not_found("Object not found"))?;
            let expected = if matches!(standard, NftLifecycleStandard::Spore) {
                ObjectStandard::Spore
            } else {
                ObjectStandard::MnftToken
            };
            if object.standard != expected {
                return Err(ApiError::bad_request("Item standard mismatch"));
            }
            (object.is_live, object.owner_lock_hash)
        }
        _ => {
            let entry = state
                .store
                .get_identity(id)
                .map_err(|e| ApiError::internal(e.to_string()))?
                .ok_or_else(|| ApiError::not_found("Identity not found"))?;
            let expected = match standard {
                NftLifecycleStandard::DotBit => IdentityStandard::DotBit,
                NftLifecycleStandard::DidCkb => IdentityStandard::DidCkb,
                NftLifecycleStandard::BitCell => IdentityStandard::BitCell,
                NftLifecycleStandard::DotCell => IdentityStandard::DotCell,
                _ => unreachable!(),
            };
            if entry.standard != expected {
                return Err(ApiError::bad_request("Item standard mismatch"));
            }
            (entry.is_live, entry.owner_lock_hash)
        }
    };
    let data = canonical_versions(state, id, standard)
        .map_err(|e| ApiError::internal(format!("{e:#}")))?;
    if data.is_empty() {
        return Err(ApiError::internal(format!(
            "item missing canonical history: 0x{}",
            hex::encode(id)
        )));
    }
    if expected_live && !matches!(standard, NftLifecycleStandard::DotCell) {
        let actual = data
            .iter()
            .find(|v| v.response.is_live)
            .map(|v| v.cell.lock_script_hash.as_slice());
        if actual != expected_owner.as_deref() {
            return Err(ApiError::internal(format!(
                "item current owner disagrees with canonical cell: 0x{}",
                hex::encode(id)
            )));
        }
    }
    if data.iter().any(|v| v.response.is_live) != expected_live {
        return Err(ApiError::internal(format!(
            "item live state disagrees with canonical versions: 0x{}",
            hex::encode(id)
        )));
    }
    Ok(data)
}
async fn history(
    state: Arc<AppState>,
    raw: String,
    standard: NftLifecycleStandard,
) -> ApiResult<CellVersionsResponse> {
    let id = parse_item_id(&raw, standard)?;
    let data = tokio::task::spawn_blocking(move || checked_versions(&state, &id, standard))
        .await
        .map_err(|e| ApiError::internal(e.to_string()))??;
    ok(CellVersionsResponse {
        total: data.len(),
        data: data.into_iter().map(|v| v.response).collect(),
    })
}

macro_rules! history_handler {
    ($name:ident,$standard:ident) => {
        async fn $name(
            State(state): State<Arc<AppState>>,
            Path(id): Path<String>,
        ) -> ApiResult<CellVersionsResponse> {
            history(state, id, NftLifecycleStandard::$standard).await
        }
    };
}
history_handler!(spore, Spore);
history_handler!(mnft, MnftToken);
history_handler!(dotbit, DotBit);
history_handler!(did, DidCkb);
history_handler!(bit_cell, BitCell);
history_handler!(dotcell, DotCell);
macro_rules! protocol_handler {
    ($name:ident,$standard:ident) => {
        async fn $name(
            State(state): State<Arc<AppState>>,
            Path(id): Path<String>,
        ) -> ApiResult<super::asset_protocol::ProtocolResponse> {
            tokio::task::spawn_blocking(move || {
                super::asset_protocol::protocol(&state, &id, NftLifecycleStandard::$standard)
            })
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?
        }
    };
}
protocol_handler!(spore_protocol, Spore);
protocol_handler!(mnft_protocol, MnftToken);
protocol_handler!(dotbit_protocol, DotBit);
protocol_handler!(did_protocol, DidCkb);
protocol_handler!(bit_cell_protocol, BitCell);
protocol_handler!(dotcell_protocol, DotCell);

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route(
            "/spore/clusters/{cluster_id}/protocol",
            get(super::asset_protocol::cluster_protocol),
        )
        .route(
            "/assets/identities/did/pds",
            get(super::asset_protocol::pds_groups),
        )
        .route("/spore/objects/{spore_id}/protocol", get(spore_protocol))
        .route(
            "/assets/objects/items/{object_id}/protocol",
            get(mnft_protocol),
        )
        .route(
            "/assets/identities/dotbit/items/{identity_id}/protocol",
            get(dotbit_protocol),
        )
        .route(
            "/assets/identities/did/items/{identity_id}/protocol",
            get(did_protocol),
        )
        .route(
            "/assets/identities/bit-cell/items/{identity_id}/protocol",
            get(bit_cell_protocol),
        )
        .route(
            "/assets/identities/dotcell/items/{id_or_name}/protocol",
            get(dotcell_protocol),
        )
        .route("/spore/objects/{spore_id}/cell-versions", get(spore))
        .route("/assets/objects/items/{object_id}/cell-versions", get(mnft))
        .route(
            "/assets/identities/dotbit/items/{identity_id}/cell-versions",
            get(dotbit),
        )
        .route(
            "/assets/identities/did/items/{identity_id}/cell-versions",
            get(did),
        )
        .route(
            "/assets/identities/bit-cell/items/{identity_id}/cell-versions",
            get(bit_cell),
        )
        .route(
            "/assets/identities/dotcell/items/{id_or_name}/cell-versions",
            get(dotcell),
        )
}

/// One proof path for a current unique type-script instance. The domain type
/// index establishes candidate membership; markers, transaction location and
/// immutable script fields must all agree before returning the Cell.
pub(crate) fn canonical_live_type_version(
    state: &AppState,
    type_hash: &[u8],
) -> anyhow::Result<Option<CanonicalCellVersion>> {
    ensure!(
        type_hash.len() == 32,
        "live type reference hash width {}",
        type_hash.len()
    );
    let mut found = None;
    for row in state
        .store
        .prefix_iterator_cf(state.store.cf_cell_by_type(), type_hash)
    {
        let (key, value) = row?;
        if !key.starts_with(type_hash) {
            break;
        }
        ensure!(
            key.len() == 74 && value.is_empty(),
            "malformed live type index: reference=0x{} key=0x{}",
            hex::encode(type_hash),
            hex::encode(&key)
        );
        let block = i64::from_be_bytes(key[32..40].try_into()?);
        let (tx, index) = ckbadger_store::keys::decode_outpoint(&key[40..]);
        let version = canonical_version_at(state, type_hash, &tx, index)?;
        ensure!(
            version.response.is_live
                && version.response.block_number == block
                && version.cell.type_script_hash.as_deref() == Some(type_hash),
            "live type index disagrees with canonical cell: reference=0x{} outpoint=0x{}:{index}",
            hex::encode(type_hash),
            hex::encode(tx)
        );
        ensure!(
            found.is_none(),
            "multiple live cells for unique type reference=0x{}",
            hex::encode(type_hash)
        );
        found = Some(version);
    }
    Ok(found)
}

/// Cluster metadata has no item reverse index. Current physical existence is
/// instead proved by the canonical live type-instance index, across every
/// Cluster deployment recognized by the same registry as the owning parser.
pub(crate) fn cluster_current(
    state: &AppState,
    id: &[u8],
) -> anyhow::Result<Option<CanonicalCellVersion>> {
    use ckbadger_indexer::parser::registry::{ProtocolScript, PROTOCOL_REGISTRY};
    let mut found = None;
    for (code, _) in PROTOCOL_REGISTRY
        .iter()
        .filter(|(_, kind)| *kind == ProtocolScript::Cluster)
    {
        for hash_type in
            (0..=u8::MAX).filter(|byte| ckbadger_common::hash_type_label(*byte).is_some())
        {
            let type_hash = crate::utils::address::compute_script_hash(code, hash_type, id);
            if let Some(version) = canonical_live_type_version(state, &type_hash)
                .with_context(|| format!("cluster=0x{}", hex::encode(id)))?
            {
                ensure!(
                    found.is_none(),
                    "multiple live cluster cells: cluster=0x{}",
                    hex::encode(id)
                );
                found = Some(version);
            }
        }
    }
    Ok(found)
}
