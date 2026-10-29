//! Current physical lock ownership. Protocol-named `.cell` rights are served
//! separately by dotcell-names and never contribute capacity here.
use anyhow::{anyhow, ensure, Context, Result};
use axum::{
    extract::{Path, Query, State},
    routing::get,
    Router,
};
use ckbadger_indexer::parser::{
    registry::{ProtocolScript, PROTOCOL_REGISTRY},
    udt::UdtParser,
};
use ckbadger_store::{
    keys,
    types::{
        derive_dotcell_id, CompositionTier, DaoDepositCacheEntry, IdentityEntry, IdentityExtra,
        IdentityStandard, ObjectEntry, ObjectExtra, ObjectStandard,
    },
    CkbadgerStore, PositionedCellInfo,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    sync::Arc,
};

use super::{dao, transaction_cells};
use crate::{
    response::{
        default_limit, ok, ApiError, ApiResult, ApiRouteError, CursorPaginatedResponse,
        ScriptResponse,
    },
    utils::{address_to_lock_script_hash, is_ckb_address, parse_hash32},
    AppState,
};

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/addresses/{addr}/holdings", get(holdings))
        .route("/addresses/{addr}/cells", get(cells))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Class {
    Dao,
    Token,
    Object,
    Identity,
    Script,
    Plain,
}
impl Class {
    const ALL: [Self; 6] = [
        Self::Dao,
        Self::Token,
        Self::Object,
        Self::Identity,
        Self::Script,
        Self::Plain,
    ];
    fn label(self) -> &'static str {
        match self {
            Self::Dao => "dao",
            Self::Token => "token",
            Self::Object => "object",
            Self::Identity => "identity",
            Self::Script => "script",
            Self::Plain => "plain",
        }
    }
}
#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct Totals {
    cell_count: i64,
    #[serde(serialize_with = "decimal")]
    capacity: i128,
    #[serde(serialize_with = "decimal")]
    occupied_capacity: i128,
}
fn decimal<S: serde::Serializer>(value: &i128, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(&value.to_string())
}
#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct Composition {
    pure_ckb: i64,
    btc_ckb: i64,
    decentralized_mixture: i64,
    centralized_mixture: i64,
    unknown: i64,
    decode_pending: i64,
}
impl Composition {
    fn add(&mut self, tier: CompositionTier) -> Result<()> {
        let value = match tier {
            CompositionTier::PureCkb => &mut self.pure_ckb,
            CompositionTier::BtcCkb => &mut self.btc_ckb,
            CompositionTier::DecentralizedMixture => &mut self.decentralized_mixture,
            CompositionTier::CentralizedMixture => &mut self.centralized_mixture,
            CompositionTier::Unknown => &mut self.unknown,
            CompositionTier::DecodePending => &mut self.decode_pending,
        };
        *value = value.checked_add(1).context("composition count overflow")?;
        Ok(())
    }
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Collection {
    id: Option<String>,
    standard: String,
    name: Option<String>,
    item_count: i64,
    composition_counts: Composition,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Summary {
    lock_script_hash: String,
    balance: String,
    occupied_capacity: String,
    free_capacity: String,
    dao_free_capacity: String,
    live_cells_count: i64,
    holdings: BTreeMap<Class, i64>,
    classes: BTreeMap<Class, Totals>,
    object_composition: Composition,
    object_collections: Vec<Collection>,
    deposit_cells_count: i64,
    withdrawal_request_cells_count: i64,
    dao_unclaimed_compensation: String,
    estimated_apc: Option<String>,
}
#[derive(Debug, Deserialize)]
struct Params {
    class: Option<Class>,
    #[serde(default = "default_limit")]
    limit: i64,
    cursor: Option<String>,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Cell {
    tx_hash: String,
    output_index: i32,
    capacity: String,
    occupied_capacity: String,
    lock_script_hash: String,
    type_script_hash: Option<String>,
    type_code_hash: Option<String>,
    type_hash_type: Option<String>,
    data_size: i32,
    created_at_block: i64,
    class: Class,
    display: Option<transaction_cells::CellDisplay>,
    expires_at: Option<u64>,
    held_since_block: Option<i64>,
    dao: Option<dao::DaoDepositResponse>,
}
struct Evidence {
    class: Class,
    item: Option<(Vec<u8>, Item)>,
    dao: Option<(Vec<u8>, DaoDepositCacheEntry)>,
}
enum Item {
    Object(ObjectEntry),
    Identity(IdentityEntry),
}

fn lock_hash(addr: &str) -> Result<Vec<u8>, ApiRouteError> {
    if is_ckb_address(addr) {
        address_to_lock_script_hash(addr).map_err(|e| ApiError::bad_request(e.to_string()))
    } else {
        parse_hash32(addr, "address lock hash")
    }
}
fn route_error(error: anyhow::Error) -> ApiRouteError {
    ApiError::internal(format!("{error:#}"))
}

/// Stream the existing owner index. The generic store listing omits missing
/// targets; an exact whole-owner projection instead rejects every orphan row.
///
/// `after` seeks past a cursor key (exclusive). `visit` returns `true` to keep
/// scanning or `false` to stop early once a page is full.
fn scan<F>(state: &AppState, owner: &[u8], after: Option<&[u8]>, mut visit: F) -> Result<()>
where
    F: FnMut(&[u8], &[u8], i16, &PositionedCellInfo, Evidence) -> Result<bool>,
{
    let start = after.unwrap_or(owner);
    let rows = state.store.iterator_cf(
        state.store.cf_cell_by_lock(),
        rocksdb::IteratorMode::From(start, rocksdb::Direction::Forward),
    );
    for row in rows {
        let (key, value) = row.context("reading owner live-cell index")?;
        if !key.starts_with(owner) {
            break;
        }
        if after.is_some_and(|after| key.as_ref() <= after) {
            continue;
        }
        ensure!(
            key.len() == 74,
            "invalid owner live-cell index key owner=0x{} key=0x{}",
            hex::encode(owner),
            hex::encode(&key)
        );
        ensure!(
            value.is_empty(),
            "invalid owner live-cell index value owner=0x{} key=0x{}",
            hex::encode(owner),
            hex::encode(&key)
        );
        let (hash, index) = keys::decode_outpoint(&key[40..]);
        let context = format!(
            "address 0x{} cell 0x{}:{index}",
            hex::encode(owner),
            hex::encode(&hash)
        );
        let keep_going = (|| -> Result<bool> {
            ensure!(index >= 0, "negative output index");
            let info = state
                .store
                .get_cell(&hash, index, &state.append_only_store)?
                .context("owner index points to missing live marker")?;
            ensure!(info.created_at_block >= 0, "negative cell creation height");
            ensure!(
                info.lock_script_hash == owner,
                "owner index disagrees with immutable lock hash"
            );
            ensure!(
                keys::encode_cell_index_key(owner, info.created_at_block, &hash, index).as_slice()
                    == key.as_ref(),
                "owner index height disagrees with live marker"
            );
            ensure!(
                info.capacity >= info.occupied_capacity && info.occupied_capacity > 0,
                "invalid capacity/occupied capacity {}/{}",
                info.capacity,
                info.occupied_capacity
            );
            ensure!(info.data_size >= 0, "negative data size");
            let evidence = classify(&state.store, &hash, index, &info)?;
            visit(&key, &hash, index, &info, evidence)
        })()
        .with_context(|| context)?;
        if !keep_going {
            break;
        }
    }
    Ok(())
}

/// Registry classification plus the indexer's own outpoint identity. No raw
/// node bytes are needed to count current holdings or distinguish ring roots.
fn classify(
    store: &CkbadgerStore,
    hash: &[u8],
    index: i16,
    info: &PositionedCellInfo,
) -> Result<Evidence> {
    let mut result = Evidence {
        class: Class::Plain,
        item: None,
        dao: None,
    };
    let Some(code) = info.type_code_hash.as_deref() else {
        ensure!(
            info.type_script_hash.is_none()
                && info.type_hash_type.is_none()
                && info.type_args.is_none(),
            "incomplete untyped cell metadata"
        );
        return Ok(result);
    };
    let type_hash = info
        .type_script_hash
        .as_deref()
        .context("typed cell missing type hash")?;
    ensure!(
        code.len() == 32 && type_hash.len() == 32,
        "invalid typed cell hash widths"
    );
    let hash_type = info
        .type_hash_type
        .context("typed cell missing hash type")?;
    ensure!(
        u8::try_from(hash_type)
            .ok()
            .and_then(ckbadger_common::hash_type_label)
            .is_some(),
        "invalid type hash type {hash_type}"
    );
    let args = info
        .type_args
        .as_deref()
        .context("typed cell missing type args")?;
    result.class = Class::Script;
    if UdtParser::is_udt_code_hash_bytes(code, hash_type).is_some() {
        ensure!(
            store.get_token(type_hash)?.is_some(),
            "missing indexed token metadata 0x{}",
            hex::encode(type_hash)
        );
        ensure!(
            info.udt_amount.is_some(),
            "token cell missing indexed amount"
        );
        result.class = Class::Token;
        return Ok(result);
    }
    let protocol = PROTOCOL_REGISTRY.get(code);
    if protocol == Some(ProtocolScript::Dao) && hash_type == 1 {
        ensure!(
            info.data_size == 8,
            "invalid DAO cell data size {}",
            info.data_size
        );
        let outpoint = keys::encode_outpoint(hash, index);
        let (key, entry) = if let Some(entry) = store.get_dao_deposit(&outpoint)? {
            ensure!(
                entry.status == 0,
                "live DAO deposit has status {}",
                entry.status
            );
            ensure!(
                entry.lock_script_hash == info.lock_script_hash
                    && entry.capacity == info.capacity
                    && entry.occupied_capacity == info.occupied_capacity
                    && entry.deposit_block_number == info.created_at_block,
                "DAO deposit metadata disagrees with current cell"
            );
            (outpoint.to_vec(), entry)
        } else {
            let key = store
                .get_dao_deposit_by_withdraw_tx(hash, index)?
                .context("live DAO cell lacks deposit/withdrawal index")?;
            let entry = store
                .get_dao_deposit(&key)?
                .context("DAO withdrawal index points to missing deposit")?;
            ensure!(
                entry.status == 1
                    && entry.withdraw_request_tx.as_deref() == Some(hash)
                    && entry.withdraw_request_output_index == Some(index)
                    && entry.withdraw_request_block == Some(info.created_at_block),
                "DAO withdrawal metadata disagrees with current outpoint"
            );
            ensure!(
                entry.capacity == info.capacity
                    && entry.withdraw_request_occupied_capacity == Some(info.occupied_capacity),
                "DAO withdrawal capacity disagrees with current cell"
            );
            (key, entry)
        };
        result.class = Class::Dao;
        result.dao = Some((key, entry));
        return Ok(result);
    }
    let (id, item) = match protocol {
        Some(ProtocolScript::SporeNft) => {
            let id = store
                .get_spore_id_by_outpoint(hash, index)?
                .context("missing indexed Spore outpoint")?;
            ensure!(
                id == args,
                "Spore outpoint identity disagrees with type args"
            );
            let entry = store
                .get_spore(&id)?
                .context("missing indexed object metadata")?;
            ensure!(
                entry.standard == ObjectStandard::Spore
                    && matches!(entry.extra, ObjectExtra::Spore { .. }),
                "Spore standard/payload mismatch"
            );
            (id, Item::Object(entry))
        }
        Some(ProtocolScript::MnftToken) => {
            let id = store
                .get_cf(
                    store.cf_stats_mnft(),
                    &keys::encode_mnft_token_outpoint_key(hash, index),
                )?
                .context("missing indexed mNFT outpoint")?;
            ensure!(
                id == args,
                "mNFT outpoint identity disagrees with type args"
            );
            let entry = store
                .get_mnft(&id)?
                .context("missing indexed object metadata")?;
            ensure!(
                entry.standard == ObjectStandard::MnftToken
                    && matches!(entry.extra, ObjectExtra::MnftToken { .. }),
                "mNFT standard/payload mismatch"
            );
            (id, Item::Object(entry))
        }
        Some(
            p @ (ProtocolScript::DidCkb
            | ProtocolScript::BitCell
            | ProtocolScript::DotbitAccount
            | ProtocolScript::DotCellAccount),
        ) => {
            let id = if p == ProtocolScript::DotbitAccount {
                store.get_dotbit_account_id_by_outpoint(hash, index)?
            } else {
                store.get_spore_id_by_outpoint(hash, index)?
            }
            .context("missing indexed identity outpoint")?;
            if p == ProtocolScript::DotCellAccount && id == derive_dotcell_id("") {
                let ring = store
                    .get_dotcell_ring(args.try_into().context("invalid .cell namespace args")?)?
                    .context("missing indexed .cell ring root")?;
                ensure!(
                    ring.root_tx_hash == hash && ring.root_output_index == index,
                    ".cell root metadata disagrees with current outpoint"
                );
                return Ok(result);
            }
            let entry = store
                .get_identity(&id)?
                .context("missing indexed identity metadata")?;
            ensure!(
                matches!(
                    (p, entry.standard, &entry.extra),
                    (
                        ProtocolScript::DidCkb,
                        IdentityStandard::DidCkb,
                        IdentityExtra::DidCkb
                    ) | (
                        ProtocolScript::BitCell,
                        IdentityStandard::BitCell,
                        IdentityExtra::BitCell { .. }
                    ) | (
                        ProtocolScript::DotbitAccount,
                        IdentityStandard::DotBit,
                        IdentityExtra::DotBit { .. }
                    ) | (
                        ProtocolScript::DotCellAccount,
                        IdentityStandard::DotCell,
                        IdentityExtra::DotCell { .. }
                    )
                ),
                "identity standard/payload mismatch"
            );
            (id, Item::Identity(entry))
        }
        _ => return Ok(result),
    };
    match &item {
        Item::Object(entry) => {
            ensure!(
                entry.is_live
                    && entry.owner_lock_hash.as_deref() == Some(info.lock_script_hash.as_slice()),
                "indexed object liveness/owner contradicts current cell"
            );
            result.class = Class::Object;
        }
        Item::Identity(entry) => {
            ensure!(entry.is_live, "indexed identity is not live");
            if entry.standard != IdentityStandard::DotCell {
                ensure!(
                    entry.owner_lock_hash.as_deref() == Some(info.lock_script_hash.as_slice()),
                    "indexed identity owner contradicts current cell"
                );
            }
            result.class = Class::Identity;
        }
    }
    result.item = Some((id, item));
    Ok(result)
}

/// Sole Spores is an index grouping, not an on-chain cluster identity.
fn visible_collection_id(entry: &ObjectEntry) -> Option<&[u8]> {
    entry.collection_id.as_deref().filter(|id| {
        !(entry.standard == ObjectStandard::Spore
            && *id == ckbadger_store::types::SOLE_SPORES_SENTINEL_COLLECTION)
    })
}
fn object_collection(
    store: &CkbadgerStore,
    entry: &ObjectEntry,
) -> Result<(Option<String>, CompositionTier)> {
    match &entry.extra {
        ObjectExtra::Spore { media_profile, .. } => {
            let name = visible_collection_id(entry)
                .map(|id| -> Result<_> {
                    let c = store
                        .get_spore(id)?
                        .context("missing indexed Spore collection")?;
                    ensure!(
                        c.standard == ObjectStandard::SporeCluster
                            && matches!(c.extra, ObjectExtra::SporeCluster),
                        "invalid Spore collection metadata"
                    );
                    Ok(c.name)
                })
                .transpose()?
                .flatten();
            Ok((name, media_profile.tier))
        }
        ObjectExtra::MnftToken { .. } => {
            let id = entry
                .collection_id
                .as_deref()
                .context("mNFT token lacks collection id")?;
            let c = store
                .get_mnft(id)?
                .context("missing indexed mNFT collection")?;
            ensure!(
                c.standard == ObjectStandard::MnftClass,
                "invalid mNFT collection standard"
            );
            let ObjectExtra::MnftClass {
                composition_tier, ..
            } = c.extra
            else {
                anyhow::bail!("invalid mNFT collection payload")
            };
            Ok((c.name, composition_tier))
        }
        _ => anyhow::bail!("object item has non-item payload"),
    }
}

async fn holdings(
    State(state): State<Arc<AppState>>,
    Path(addr): Path<String>,
) -> ApiResult<Summary> {
    let owner = lock_hash(&addr)?;
    let response = tokio::task::spawn_blocking(move || summary(&state, &owner))
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .map_err(route_error)?;
    ok(response)
}
fn summary(state: &AppState, owner: &[u8]) -> Result<Summary> {
    let mut classes: BTreeMap<_, _> = Class::ALL
        .into_iter()
        .map(|c| (c, Totals::default()))
        .collect();
    let mut tokens = HashSet::new();
    let mut items = HashSet::new();
    let mut groups = BTreeMap::new();
    let mut composition = Composition::default();
    let mut deposit_cells_count = 0;
    let mut withdrawal_request_cells_count = 0;
    let mut unclaimed = 0i128;
    let mut dao_tip = None;
    scan(state, owner, None, |_, _, _, info, evidence| {
        let totals = classes
            .get_mut(&evidence.class)
            .expect("all classes initialized");
        totals.cell_count = totals
            .cell_count
            .checked_add(1)
            .context("cell count overflow")?;
        totals.capacity = totals
            .capacity
            .checked_add(i128::from(info.capacity))
            .context("capacity overflow")?;
        totals.occupied_capacity = totals
            .occupied_capacity
            .checked_add(i128::from(info.occupied_capacity))
            .context("occupied capacity overflow")?;
        // The existing token balance list contains only positive balances.
        // Zero-amount cells still belong to the token capacity/class partition.
        if evidence.class == Class::Token && info.udt_amount.is_some_and(|amount| amount > 0) {
            tokens.insert(
                info.type_script_hash
                    .clone()
                    .context("token lacks type hash")?,
            );
        }
        if let Some((id, item)) = evidence.item {
            ensure!(
                items.insert((evidence.class, id.clone())),
                "item has multiple current cells: 0x{}",
                hex::encode(id)
            );
            if let Item::Object(entry) = item {
                let (name, tier) = object_collection(&state.store, &entry)?;
                composition.add(tier)?;
                let standard = entry.standard.asset_standard().to_string();
                let key = (
                    standard.clone(),
                    visible_collection_id(&entry).map(Vec::from),
                );
                let group = groups.entry(key).or_insert_with(|| Collection {
                    id: visible_collection_id(&entry).map(|id| format!("0x{}", hex::encode(id))),
                    standard,
                    name,
                    item_count: 0,
                    composition_counts: Composition::default(),
                });
                group.item_count += 1;
                group.composition_counts.add(tier)?;
            }
        }
        if let Some((_, entry)) = evidence.dao {
            if entry.status == 0 {
                deposit_cells_count += 1;
            } else {
                withdrawal_request_cells_count += 1;
            }
            let (height, ar) = match dao_tip {
                Some(tip) => tip,
                None => {
                    let (height, ar, _, _) =
                        dao::resolve_latest_block_and_ar(state, "address current holdings")
                            .map_err(|(_, e)| anyhow!(e.0.message))?;
                    dao_tip = Some((height, ar));
                    (height, ar)
                }
            };
            unclaimed = unclaimed
                .checked_add(
                    ckbadger_store::dao_compensation_for_entry_at(&entry, height, ar)?.unclaimed,
                )
                .context("DAO compensation overflow")?;
        }
        Ok(true)
    })?;
    let mut count = 0i64;
    let mut balance = 0i128;
    let mut occupied = 0i128;
    for totals in classes.values() {
        count = count
            .checked_add(totals.cell_count)
            .context("total count overflow")?;
        balance = balance
            .checked_add(totals.capacity)
            .context("balance overflow")?;
        occupied = occupied
            .checked_add(totals.occupied_capacity)
            .context("occupied overflow")?;
    }
    match state.store.get_addr_balance(owner)? {
        Some(stored)=>ensure!(stored.balance==balance && stored.used_capacity==occupied && i64::from(stored.live_cells_count)==count,"address 0x{} aggregate disagrees with live cells: stored balance={} occupied={} count={}; scanned balance={balance} occupied={occupied} count={count}",hex::encode(owner),stored.balance,stored.used_capacity,stored.live_cells_count),
        None=>ensure!(count==0,"address 0x{} has live cells but no balance aggregate",hex::encode(owner)),
    }
    let dao_free = classes[&Class::Dao].capacity - classes[&Class::Dao].occupied_capacity;
    let holdings = BTreeMap::from([
        (Class::Dao, classes[&Class::Dao].cell_count),
        (Class::Token, i64::try_from(tokens.len())?),
        (Class::Object, classes[&Class::Object].cell_count),
        (Class::Identity, classes[&Class::Identity].cell_count),
    ]);
    let estimated_apc = dao_tip
        .map(|_| dao::estimated_apc_from_state(state).map_err(|(_, e)| anyhow!(e.0.message)))
        .transpose()?;
    ensure!(
        estimated_apc.as_ref().is_none_or(|value| !value.is_empty()),
        "DAO holdings APC unavailable at canonical tip {dao_tip:?}"
    );
    Ok(Summary {
        lock_script_hash: format!("0x{}", hex::encode(owner)),
        balance: balance.to_string(),
        occupied_capacity: occupied.to_string(),
        free_capacity: (balance - occupied - dao_free).to_string(),
        dao_free_capacity: dao_free.to_string(),
        live_cells_count: count,
        holdings,
        classes,
        object_composition: composition,
        object_collections: groups.into_values().collect(),
        deposit_cells_count,
        withdrawal_request_cells_count,
        dao_unclaimed_compensation: unclaimed.to_string(),
        estimated_apc,
    })
}

fn cursor(raw: &str, owner: &[u8], class: Option<Class>) -> Result<Vec<u8>, ApiRouteError> {
    let expected = format!("v1:{}:", class.map_or("all", Class::label));
    let suffix = raw
        .strip_prefix(&expected)
        .ok_or_else(|| ApiError::bad_request("cell cursor belongs to a different class"))?;
    let key = hex::decode(suffix).map_err(|_| ApiError::bad_request("invalid cell cursor hex"))?;
    if key.len() != 74 || !key.starts_with(owner) {
        return Err(ApiError::bad_request(
            "cell cursor belongs to a different address or has invalid length",
        ));
    }
    Ok(key)
}
async fn cells(
    State(state): State<Arc<AppState>>,
    Path(addr): Path<String>,
    Query(params): Query<Params>,
) -> ApiResult<CursorPaginatedResponse<Cell>> {
    let owner = lock_hash(&addr)?;
    if !(1..=100).contains(&params.limit) {
        return Err(ApiError::bad_request("limit must be between 1 and 100"));
    }
    let after = params
        .cursor
        .as_deref()
        .map(|s| cursor(s, &owner, params.class))
        .transpose()?;
    let page = tokio::task::spawn_blocking(move || -> Result<_> {
        let need = params.limit as usize + 1;
        let mut rows = Vec::new();
        // Seek from the cursor (or owner prefix) and stop once the page is full —
        // do not scan the whole owner set just to compute a filtered total.
        scan(
            &state,
            &owner,
            after.as_deref(),
            |key, hash, index, info, evidence| {
                if params.class.is_none_or(|c| c == evidence.class) {
                    rows.push((key.to_vec(), hash.to_vec(), index, info.clone(), evidence));
                    return Ok(rows.len() < need);
                }
                Ok(true)
            },
        )?;
        let has_more = rows.len() > params.limit as usize;
        rows.truncate(params.limit as usize);
        let next = has_more.then(|| {
            format!(
                "v1:{}:{}",
                params.class.map_or("all", Class::label),
                hex::encode(&rows.last().expect("nonempty next page").0)
            )
        });
        let rows = rows
            .into_iter()
            .map(|(_, hash, index, info, evidence)| {
                cell_response(&state, &hash, index, &info, evidence).with_context(|| {
                    format!(
                        "address 0x{} cell 0x{}:{index}",
                        hex::encode(&owner),
                        hex::encode(hash)
                    )
                })
            })
            .collect::<Result<Vec<_>>>()?;
        if params.class.is_some() {
            return Ok(CursorPaginatedResponse::without_total(
                rows,
                params.limit,
                next,
            ));
        }
        let total = match state.store.get_addr_balance(&owner)? {
            Some(balance) => i64::from(balance.live_cells_count),
            None => {
                ensure!(
                    rows.is_empty(),
                    "address 0x{} has live cells but no balance aggregate",
                    hex::encode(&owner)
                );
                0
            }
        };
        Ok(CursorPaginatedResponse::new(
            rows,
            total,
            params.limit,
            next,
        ))
    })
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?
    .map_err(route_error)?;
    ok(page)
}

fn script(code: &[u8], hash_type: i16, args: &[u8]) -> Result<ScriptResponse> {
    let label = u8::try_from(hash_type)
        .ok()
        .and_then(ckbadger_common::hash_type_label)
        .context("invalid immutable script hash type")?;
    Ok(ScriptResponse {
        code_hash: format!("0x{}", hex::encode(code)),
        hash_type: label.into(),
        args: format!("0x{}", hex::encode(args)),
    })
}
fn cell_response(
    state: &AppState,
    hash: &[u8],
    index: i16,
    info: &PositionedCellInfo,
    evidence: Evidence,
) -> Result<Cell> {
    let lock = script(&info.lock_code_hash, info.lock_hash_type, &info.lock_args)?;
    let typed = info
        .type_code_hash
        .as_deref()
        .map(|code| {
            script(
                code,
                info.type_hash_type.context("missing type hash type")?,
                info.type_args.as_deref().context("missing type args")?,
            )
        })
        .transpose()?;
    let mut display = if transaction_cells::has_display(typed.as_ref())? {
        let data = if info.data_size == 0 {
            Vec::new()
        } else {
            let reader = state
                .ckb_store
                .as_ref()
                .context("cell display requires CKB RocksDB reader")?;
            let tx = reader
                .get_transaction(&hash.try_into()?)
                .context("cell display transaction missing from CKB RocksDB")?;
            let data = tx
                .outputs_data()
                .get(index as usize)
                .context("cell display output missing from CKB RocksDB")?
                .raw_data();
            ensure!(
                data.len() == info.data_size as usize,
                "cell data size disagrees with node"
            );
            if let Some(expected) = &info.data_hash {
                ensure!(
                    expected.as_slice() == ckb_hash::blake2b_256(&data),
                    "cell data hash disagrees with immutable payload"
                );
            }
            data.to_vec()
        };
        transaction_cells::from_parts(
            &state.store,
            &info.capacity.to_string(),
            &lock,
            typed.as_ref(),
            &data,
            true,
        )?
    } else {
        None
    };
    let mut expires_at = None;
    let mut held_since_block = None;
    if let Some((id, item)) = &evidence.item {
        let d = display
            .as_mut()
            .context("indexed holding lacks decoded display")?;
        ensure!(
            d.id == format!("0x{}", hex::encode(id)) && d.kind == evidence.class.label(),
            "decoded display contradicts indexed holding identity"
        );
        match item {
            Item::Object(entry) => {
                ensure!(
                    d.collection_id
                        == visible_collection_id(entry).map(|id| format!("0x{}", hex::encode(id))),
                    "decoded object collection contradicts indexed metadata"
                );
                let (name, tier) = object_collection(&state.store, entry)?;
                d.collection_name = name;
                d.composition_tier = Some(tier.as_str().into());
            }
            Item::Identity(entry) => {
                expires_at = match entry.extra {
                    IdentityExtra::DotBit { expired_at, .. } => expired_at,
                    IdentityExtra::BitCell { expired_at, .. }
                    | IdentityExtra::DotCell { expired_at, .. } => Some(expired_at),
                    IdentityExtra::DidCkb => None,
                };
                held_since_block = Some(identity_held_since(
                    state,
                    id,
                    entry.standard,
                    hash,
                    index,
                    info,
                )?);
            }
        }
    }
    if evidence.class == Class::Token {
        ensure!(
            display.as_ref().and_then(|d| d.amount.as_deref())
                == info.udt_amount.map(|n| n.to_string()).as_deref(),
            "decoded token amount contradicts indexed payload"
        );
    }
    let dao = evidence
        .dao
        .map(|(key, entry)| {
            dao::deposit_to_response(&key, &entry, state).map_err(|(_, e)| anyhow!(e.0.message))
        })
        .transpose()?;
    Ok(Cell {
        tx_hash: format!("0x{}", hex::encode(hash)),
        output_index: i32::from(index),
        capacity: info.capacity.to_string(),
        occupied_capacity: info.occupied_capacity.to_string(),
        lock_script_hash: format!("0x{}", hex::encode(&info.lock_script_hash)),
        type_script_hash: info
            .type_script_hash
            .as_ref()
            .map(|h| format!("0x{}", hex::encode(h))),
        type_code_hash: info
            .type_code_hash
            .as_ref()
            .map(|h| format!("0x{}", hex::encode(h))),
        type_hash_type: typed.map(|script| script.hash_type),
        data_size: info.data_size,
        created_at_block: info.created_at_block,
        class: evidence.class,
        display,
        expires_at,
        held_since_block,
        dao,
    })
}

/// Follow exact canonical consume→recreate links, stopping on ownership change
/// or a disconnected mint. An earlier same-owner incarnation after a burn is
/// not continuous ownership. This history is read only for returned identities.
fn identity_held_since(
    state: &AppState,
    id: &[u8],
    standard: IdentityStandard,
    hash: &[u8],
    index: i16,
    info: &PositionedCellInfo,
) -> Result<i64> {
    let outpoints = if standard == IdentityStandard::DotBit {
        state
            .store
            .list_dotbit_account_outpoints_by_account_id(id)?
    } else {
        state.store.list_spore_outpoints_by_spore_id(id)?
    };
    ensure!(
        outpoints.iter().any(|(h, i)| h == hash && *i == index),
        "identity reverse history lacks current outpoint"
    );
    let mut predecessors = HashMap::new();
    for (h, i) in outpoints {
        if h == hash && i == index {
            continue;
        }
        if let Some(previous) =
            state
                .store
                .get_consumed_cell_info(&h, i, &state.append_only_store)?
        {
            let indexed_id = if standard == IdentityStandard::DotBit {
                state.store.get_dotbit_account_id_by_outpoint(&h, i)?
            } else {
                state.store.get_spore_id_by_outpoint(&h, i)?
            };
            ensure!(
                indexed_id.as_deref() == Some(id),
                "identity reverse history contradicts canonical outpoint index: 0x{}:{i}",
                hex::encode(&h)
            );
            let consumer = previous
                .consumed_by_tx
                .clone()
                .context("identity history consumed marker lacks consuming transaction")?;
            ensure!(
                predecessors.insert(consumer, (h, i, previous)).is_none(),
                "identity history has multiple predecessors in one transaction"
            );
        } else {
            ensure!(
                state
                    .store
                    .get_cell(&h, i, &state.append_only_store)?
                    .is_none(),
                "identity history has another current live outpoint"
            );
            // Reverted immutable payloads may remain; without a canonical marker
            // they prove no step in the current ownership history.
        }
    }
    let mut tx = hash.to_vec();
    let mut since = info.created_at_block;
    let mut seen = HashSet::new();
    while let Some((h, _, previous)) = predecessors.remove(&tx) {
        ensure!(
            seen.insert(tx),
            "identity ownership history contains a cycle"
        );
        ensure!(
            previous.consumed_at_block == since && previous.created_at_block <= since,
            "identity predecessor heights contradict consume→recreate link"
        );
        if previous.cell.lock_script_hash != info.lock_script_hash {
            break;
        }
        since = previous.created_at_block;
        tx = h;
    }
    Ok(since)
}
