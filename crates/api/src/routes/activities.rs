use axum::{
    extract::{Path, Query, State},
    routing::get,
    Router,
};
use ckbadger_store::{
    keys,
    types::{
        participant_roles, IdentityStandard, ItemDelta, ItemKind, LockCallEntry, ParticipantDelta,
        ParticipantId, TxActions, TypeCallEntry,
    },
    CkbadgerStore, ResolvedTx,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

use crate::pool::PoolTxRecord;
use crate::response::{
    default_limit, hash_type_to_str, ok, ApiError, ApiResult, ApiRouteError,
    CursorPaginatedResponse,
};
use crate::routes::address_pool;
use crate::utils::address::{
    address_to_lock_script_hash, compute_script_hash, encode_lock_address, resolve_lock_address,
};
use crate::utils::exact_json::integers_as_strings;
use crate::utils::{is_ckb_address, parse_hash32, parse_optional_block_tx_cursor};
use crate::AppState;
use ckbadger_indexer::parser::registry::protocol_display_name;

/// A party's address, from wherever its lock script is known.
///
/// A committed row's parties hold (or held) cells the indexer wrote, so the
/// chain view's `CF_LOCK_SCRIPTS` knows every one of them. A tx-pool row may
/// pay a lock no committed cell has used yet, or spend a cell whose lock exists
/// so far only as the output of another pending transaction; both scripts are
/// on the mirror's record (its outputs and its spent inputs' locks) and are
/// encoded from there — a store miss for a brand-new lock is not "unknown".
/// Either way the address comes from `encode_lock_address`, the one encoder; a
/// lock known to neither is unresolved (`None`), never a fabricated address.
fn resolve_party_address(
    store: &CkbadgerStore,
    network: &str,
    lock_hash: &[u8; 32],
    pool_record: Option<&PoolTxRecord>,
    cache: &mut HashMap<Vec<u8>, Option<String>>,
) -> anyhow::Result<Option<String>> {
    if let Some(lock) = pool_record.and_then(|record| record.lock_script(lock_hash)) {
        return encode_lock_address(
            lock_hash,
            &lock.code_hash,
            lock.hash_type,
            &lock.args,
            network,
        );
    }
    if let Some(cached) = cache.get(lock_hash.as_slice()) {
        return Ok(cached.clone());
    }
    let address =
        resolve_lock_address(store, lock_hash, network)?.and_then(|resolved| resolved.address);
    cache.insert(lock_hash.to_vec(), address.clone());
    Ok(address)
}

/// One party of a transaction as the API reports it.
///
/// A party the protocol named by a 20-byte lock-hash prefix may or may not be
/// resolvable to a full lock hash, so `address` and `lockHash` are optional and
/// `lockHashPrefix` is what the chain actually says. An unresolved party renders
/// as its prefix — never as a fabricated address.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParticipantRef {
    pub address: Option<String>,
    pub lock_hash: Option<String>,
    pub lock_hash_prefix: Option<String>,
    pub lock_kind: Option<String>,
    /// The lock's hash type where its named pair is not the registry's
    /// reference for its code (`utils::ScriptName::by`), written beside the
    /// name; `None` otherwise.
    pub lock_kind_by: Option<String>,
    pub roles: Vec<&'static str>,
}

/// The name of a participant's lock (`utils::ScriptNamer`, the one rule: the
/// code it runs), its pair read from the pool record or the store.
pub(crate) fn participant_lock_kind(
    store: &CkbadgerStore,
    names: &mut ScriptNameCache<'_>,
    lock_hash: &[u8; 32],
    pool_record: Option<&PoolTxRecord>,
) -> anyhow::Result<Option<crate::utils::ScriptName>> {
    // The lock's pair (code hash, hash type), from the pool record or the store.
    let lock = if let Some(lock) = pool_record.and_then(|record| record.lock_script(lock_hash)) {
        Some((lock.code_hash, lock.hash_type))
    } else {
        store
            .get_lock_script(lock_hash)?
            .map(|lock| (lock.code_hash, lock.hash_type))
    };
    lock.map(|(code_hash, hash_type)| {
        names
            .name_stored(&code_hash, hash_type)
            .map_err(|e| anyhow::anyhow!("participant lock 0x{}: {e:#}", hex::encode(lock_hash)))
    })
    .transpose()
    .map(Option::flatten)
}

fn participant_ref(
    store: &CkbadgerStore,
    network: &str,
    participant: &ParticipantDelta,
    pool_record: Option<&PoolTxRecord>,
    cache: &mut HashMap<Vec<u8>, Option<String>>,
    names: &mut ScriptNameCache<'_>,
) -> anyhow::Result<ParticipantRef> {
    let roles = participant_roles::names(participant.roles);
    match participant.id {
        ParticipantId::Lock(hash) => {
            let address = resolve_party_address(store, network, &hash, pool_record, cache)?;
            let lock_name = participant_lock_kind(store, names, &hash, pool_record)?;
            Ok(ParticipantRef {
                address,
                lock_hash: Some(format!("0x{}", hex::encode(hash))),
                lock_hash_prefix: None,
                lock_kind_by: lock_name.as_ref().and_then(|name| name.by()),
                lock_kind: lock_name.map(|name| name.name),
                roles,
            })
        }
        // A protocol-named prefix resolves at read time through the chain
        // view (spec §3), and only when exactly one known lock has it.
        ParticipantId::LockPrefix(prefix) => match store.resolve_lock_hash_prefix(&prefix)? {
            Some((hash, entry)) => {
                let lock_name = names.name_stored(&entry.code_hash, entry.hash_type)?;
                Ok(ParticipantRef {
                    address: encode_lock_address(
                        &hash,
                        &entry.code_hash,
                        entry.hash_type,
                        &entry.args,
                        network,
                    )?,
                    lock_hash: Some(format!("0x{}", hex::encode(hash))),
                    lock_hash_prefix: Some(format!("0x{}", hex::encode(prefix))),
                    lock_kind_by: lock_name.as_ref().and_then(|name| name.by()),
                    lock_kind: lock_name.map(|name| name.name),
                    roles,
                })
            }
            None => Ok(ParticipantRef {
                address: None,
                lock_hash: None,
                lock_hash_prefix: Some(format!("0x{}", hex::encode(prefix))),
                lock_kind: None,
                lock_kind_by: None,
                roles,
            }),
        },
    }
}

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/addresses/{addr}/activities", get(get_address_activities))
        .route("/activities", get(get_global_activities))
        .route("/activities/latest", get(get_latest_activities))
        .route(
            "/transactions/{hash}/activity",
            get(get_transaction_activity),
        )
}

#[derive(Debug, Deserialize)]
pub struct ActivityParams {
    #[serde(default = "default_limit")]
    limit: i64,
    cursor: Option<String>,
    filter: Option<String>,
    #[serde(rename = "blockNumber", alias = "block_number")]
    block_number: Option<i64>,
    /// `asc` (block order) or `desc` (default, newest first); only for a
    /// block-scoped feed (`blockNumber`), whose rows have a block order (BA-14).
    order: Option<String>,
}

/// Per-address activity response: shows one participant's perspective of a transaction.
///
/// Chain position is nullable because the same shape serves transactions that
/// are still in the node's tx pool. Those have no block, no index and no block
/// time — only `poolStatus`, `timeAddedToPool` and how far they could be
/// interpreted. Nothing downstream can mistake one for a committed row.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityResponse {
    pub tx_hash: String,
    pub block_number: Option<i64>,
    pub tx_index: Option<i32>,
    pub timestamp: Option<String>,
    /// `pending`, `proposed` or `committed_awaiting_index` for a tx-pool row;
    /// absent for a committed one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pool_status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time_added_to_pool: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interpretation: Option<crate::pool::InterpretationResponse>,
    // This participant's Layer 1
    pub ckb_delta: String,
    pub used_delta: String,
    pub is_cellbase: bool,
    // This participant's Layer 2
    pub item_deltas: Vec<ItemDeltaResponse>,
    // TX-level Layer 3
    pub type_calls: Vec<TypeCallResponse>,
    pub lock_calls: Vec<LockCallResponse>,
    /// The transaction's items' and Nervos DAO step's type scripts ([`item_types`]); `None` on a
    /// pool row, and on a committed row whose item's script the stores cannot say
    /// (`item_types_unread` says why).
    pub item_types: Option<Vec<ItemTypeResponse>>,
    pub item_types_unread: Option<String>,
    pub protocol_actions: Vec<ProtocolActionResponse>,
    // Other participants
    pub participants: Vec<ParticipantRef>,
    /// Roles the protocol gave THIS participant, if any.
    pub roles: Vec<&'static str>,
    pub tags: u16,
}

/// Item delta response — tagged enum keyed on `kind`.
///
/// NOTE: `rename_all` on an internally-tagged enum only renames the tag values,
/// NOT the fields inside each variant. Each variant needs its own `rename_all`.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind")]
pub enum ItemDeltaResponse {
    #[serde(rename = "token", rename_all = "camelCase")]
    Token {
        type_script_hash: String,
        delta: String,
        symbol: Option<String>,
        decimals: Option<u8>,
    },
    #[serde(rename = "object", rename_all = "camelCase")]
    Object {
        object_id: String,
        delta: i8,
        /// The object's collection (`transaction_cells::object_delta_context`):
        /// a spore's cluster (id + immutable name; the Sole Spores sentinel
        /// with `name: null` for a clusterless spore), a cluster's own id and
        /// name, an m-NFT token's class id (`name: null`). `null` only on a
        /// pool row whose object is not indexed yet.
        collection: Option<super::transaction_cells::DeltaCollection>,
        /// The site's asset class vocabulary: always `object`.
        class: &'static str,
        /// `CompositionTier::as_str` for a spore; `null` for a Spore cluster,
        /// an m-NFT token and a pool row whose object is not indexed yet.
        composition: Option<&'static str>,
        /// What the object is: `spore | cluster | mnft_token`
        /// (`ObjectDeltaRole::as_str`); `null` only on a pool row whose object
        /// is not indexed yet. A cluster's `collection` is the cluster itself.
        role: Option<&'static str>,
    },
    #[serde(rename = "identity", rename_all = "camelCase")]
    Identity {
        identity_id: String,
        delta: i8,
        /// The identity's standard, as the builder recorded it on the delta,
        /// in its one wire value (`IdentityStandard::asset_standard`:
        /// `dotbit | bit_cell | did_ckb | dotcell`). The frontend links the
        /// item to its standard's page with it.
        standard: &'static str,
    },
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TypeCallResponse {
    pub type_code_hash: String,
    pub type_hash_type: String,
    pub type_args: String,
    pub script_hash: String,
    pub script_name: Option<String>,
    /// The script's hash type where its named pair is not the registry's
    /// reference for its code, written beside the name; `None` otherwise.
    pub script_name_by: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LockCallResponse {
    pub lock_code_hash: String,
    pub lock_hash_type: String,
    pub lock_args: String,
    pub script_hash: String,
    pub script_name: Option<String>,
    /// The script's hash type where its named pair is not the registry's
    /// reference for its code, written beside the name; `None` otherwise.
    pub script_name_by: Option<String>,
    /// Decoded lock args; every integer in it is an exact decimal string
    /// (`utils::exact_json`). `null` when the lock's registry entry declares
    /// no args layout, or the args do not fit it (then `decodeError` says why).
    pub decoded: Option<serde_json::Value>,
    /// Why `decoded` is null although the lock's registry entry declares an
    /// args layout (R-8): `"<script>: args are N bytes, layout … expects …"`.
    pub decode_error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProtocolActionResponse {
    pub protocol: String,
    /// The protocol's display name from the bundled registry
    /// (`docs/metadata/protocols.toml`), the one name every client shows.
    pub protocol_name: String,
    pub action: String,
    /// Protocol-specific JSON; every integer in it is an exact decimal
    /// string (`utils::exact_json`).
    pub metadata: serde_json::Value,
}

/// Global activity response: shows all participants in a transaction.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GlobalActivityResponse {
    pub tx_hash: String,
    pub block_number: i64,
    pub tx_index: i32,
    pub timestamp: String,
    pub is_cellbase: bool,
    // TX-level Layer 3
    pub protocol_actions: Vec<ProtocolActionResponse>,
    pub type_calls: Vec<TypeCallResponse>,
    pub lock_calls: Vec<LockCallResponse>,
    /// The transaction's items' and Nervos DAO step's type scripts ([`item_types`]); `None` on a
    /// pool row, and on a committed row whose item's script the stores cannot say
    /// (`item_types_unread` says why).
    pub item_types: Option<Vec<ItemTypeResponse>>,
    pub item_types_unread: Option<String>,
    // All participants with their data
    pub participants: Vec<ParticipantResponse>,
}

/// A single participant's delta within a global activity response.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParticipantResponse {
    pub address: Option<String>,
    pub lock_hash: Option<String>,
    pub lock_hash_prefix: Option<String>,
    pub lock_kind: Option<String>,
    /// The lock's hash type where its named pair is not the registry's
    /// reference for its code, written beside the name; `None` otherwise.
    pub lock_kind_by: Option<String>,
    pub roles: Vec<&'static str>,
    pub ckb_delta: String,
    pub used_delta: String,
    pub item_deltas: Vec<ItemDeltaResponse>,
    pub tags: u16,
}

#[derive(Debug, Deserialize)]
pub struct LatestActivityParams {
    #[serde(default = "default_latest_limit")]
    limit: usize,
}

fn default_latest_limit() -> usize {
    8
}

/// What an activity reads of a token's row, once: its symbol and decimals, and its type script's
/// code (`item_types`); `None` when no token row exists.
#[derive(Clone)]
struct TokenFacts {
    symbol: Option<String>,
    decimals: Option<u8>,
    type_code_hash: Vec<u8>,
    hash_type: i16,
}
type TokenDisplay = Option<TokenFacts>;

/// What one response has already read for its item deltas: token symbol /
/// decimals, and object delta contexts keyed by `(object id, indexed)` — a
/// pool row's lookup (which may legitimately find no row yet) never answers
/// for a committed row's.
#[derive(Default)]
pub(crate) struct ItemDeltaCache {
    tokens: HashMap<Vec<u8>, TokenDisplay>,
    objects: HashMap<(Vec<u8>, bool), super::transaction_cells::ObjectDeltaContext>,
}

/// Convert an `ItemDelta` to `ItemDeltaResponse`: tokens gain their
/// symbol/decimals, objects their collection, class and composition. The one
/// seam every producer of item deltas (address / global / transaction
/// activity, asset feeds, the pool mirror) goes through. `indexed` is false
/// only for a pool (pending) row.
fn convert_item_delta(
    item: &ItemDelta,
    cache: &mut ItemDeltaCache,
    store: &CkbadgerStore,
    indexed: bool,
) -> anyhow::Result<ItemDeltaResponse> {
    match item.kind {
        ItemKind::Token => {
            let (symbol, decimals) = lookup_token_info(store, &mut cache.tokens, &item.item_id)?;
            Ok(ItemDeltaResponse::Token {
                type_script_hash: format!("0x{}", hex::encode(&item.item_id)),
                delta: format!("{}{}", if item.negative { "-" } else { "" }, item.magnitude),
                symbol,
                decimals,
            })
        }
        ItemKind::Object => {
            let delta = discrete_item_delta(item)?;
            let key = (item.item_id.clone(), indexed);
            let context = match cache.objects.get(&key) {
                Some(context) => context.clone(),
                None => {
                    let context = super::transaction_cells::object_delta_context(
                        store,
                        &item.item_id,
                        indexed,
                    )?;
                    cache.objects.insert(key, context.clone());
                    context
                }
            };
            Ok(ItemDeltaResponse::Object {
                object_id: format!("0x{}", hex::encode(&item.item_id)),
                delta,
                collection: context.collection,
                class: context.class,
                composition: context.composition,
                role: context.role,
            })
        }
        ItemKind::Identity(standard) => Ok(ItemDeltaResponse::Identity {
            identity_id: format!("0x{}", hex::encode(&item.item_id)),
            delta: discrete_item_delta(item)?,
            standard: standard.asset_standard(),
        }),
    }
}

fn discrete_item_delta(item: &ItemDelta) -> anyhow::Result<i8> {
    if item.magnitude != 1 {
        anyhow::bail!(
            "activity discrete-item invariant violated: kind={:?} item=0x{} magnitude={} (expected 1)",
            item.kind,
            hex::encode(&item.item_id),
            item.magnitude
        );
    }
    Ok(if item.negative { -1 } else { 1 })
}

/// One type script an activity's items or its Nervos DAO step carry, by its code: the code hash
/// and the hash type, never the args (the fold compares the code, as ruled at Gate B).
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ItemTypeResponse {
    pub type_code_hash: String,
    pub type_hash_type: String,
    /// The name of the code the type's pair runs (`utils::ScriptNamer`, the
    /// one rule; lane rd/api-lock-names): it named the code hash alone.
    pub script_name: Option<String>,
    /// The pair's hash type where it is not the registry's reference for the
    /// code it runs, written beside the name; `None` otherwise.
    pub script_name_by: Option<String>,
}

/// What a committed activity states of its items' scripts ([`item_types`]): the codes, or, where
/// the stores cannot say one item's, why (`types` is then `None`). A part the stores cannot state
/// fails that part only: the row says so and stands alone in a feed, never the response.
pub(crate) struct ItemTypesRead {
    pub types: Option<Vec<ItemTypeResponse>>,
    pub unread: Option<String>,
}

/// The type scripts a committed activity's items and its Nervos DAO step carry, each code once,
/// in code-hash then hash-type order: the type script of every token, object and identity a
/// participant gained or lost, as the indexer classified the item ([`item_type_script`]), and the
/// Nervos DAO's when the transaction took a DAO step. Read from ckbadger's own stores in the
/// request's pinned view, never from the node. Beside it, `typeCalls` are the type scripts the
/// indexer does not recognise (an item's type never is one), and `lockCalls` the non-standard
/// locks attached to the transaction's OUTPUTS, which did not run: the locks that ran are its
/// inputs', which the stored activity does not name yet (they wait for the indexer's
/// per-participant input flag, a re-sync).
fn item_types(
    store: &CkbadgerStore,
    cells_store: &CkbadgerStore,
    actions: &TxActions,
    names: &mut ScriptNameCache<'_>,
    items: &mut ItemDeltaCache,
) -> anyhow::Result<ItemTypesRead> {
    let mut codes: std::collections::BTreeSet<(Vec<u8>, i16)> = std::collections::BTreeSet::new();
    let mut read: Vec<(ItemKind, &[u8])> = Vec::new();
    let mut outputs = CreatingOutputs::default();
    for item in actions.participants.iter().flat_map(|p| &p.item_deltas) {
        if read.contains(&(item.kind, item.item_id.as_slice())) {
            continue;
        }
        read.push((item.kind, item.item_id.as_slice()));
        match item_type_script(store, cells_store, item, &mut outputs, items)? {
            Ok(code) => {
                codes.insert(code);
            }
            Err(why) => {
                return Ok(ItemTypesRead {
                    types: None,
                    unread: Some(format!(
                        "item 0x{} ({:?}) of tx 0x{}: {why}",
                        hex::encode(&item.item_id),
                        item.kind,
                        hex::encode(&actions.tx_hash)
                    )),
                })
            }
        }
    }
    if actions.protocol_actions.iter().any(|a| a.protocol == "dao") {
        let dao =
            ckbadger_indexer::rpc::parse_hex_to_bytes(ckbadger_indexer::parser::dao::DAO_CODE_HASH);
        // the DAO's code hash is the type id of its deployment, so its cells name it by type
        let type_byte = ckbadger_common::hash_type_from_label("type")
            .ok_or_else(|| anyhow::anyhow!("the hash type table has no \"type\""))?;
        codes.insert((dao, i16::from(type_byte)));
    }
    let types = codes
        .into_iter()
        .map(|(code, hash_type)| {
            let name = names.name_stored(&code, hash_type)?;
            Ok(ItemTypeResponse {
                type_code_hash: format!("0x{}", hex::encode(&code)),
                type_hash_type: hash_type_to_str(hash_type)
                    .ok_or_else(|| anyhow::anyhow!("unsupported hash type {hash_type}"))?
                    .to_string(),
                script_name_by: name.as_ref().and_then(|name| name.by()),
                script_name: name.map(|name| name.name),
            })
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    Ok(ItemTypesRead {
        types: Some(types),
        unread: None,
    })
}

/// A creating transaction's outputs as one activity reads them, each transaction once.
#[derive(Default)]
struct CreatingOutputs(HashMap<Vec<u8>, Vec<Option<ckbadger_store::types::LiveCellInfo>>>);

impl CreatingOutputs {
    /// The outputs of `tx_hash` (`None` where its payload is missing), or why there are none.
    fn of(
        &mut self,
        store: &CkbadgerStore,
        cells_store: &CkbadgerStore,
        tx_hash: &[u8],
    ) -> anyhow::Result<Result<&[Option<ckbadger_store::types::LiveCellInfo>], String>> {
        if !self.0.contains_key(tx_hash) {
            let Some((_, _, tx)) = store.get_tx_by_hash(tx_hash)? else {
                return Ok(Err(format!(
                    "its creating transaction 0x{} is not indexed",
                    hex::encode(tx_hash)
                )));
            };
            let outputs = cells_store.get_tx_output_payloads(tx_hash, tx.outputs_count)?;
            self.0.insert(tx_hash.to_vec(), outputs);
        }
        Ok(Ok(self.0[tx_hash].as_slice()))
    }
}

/// The type script of a cell the indexer recorded, or why its payload does not hold one.
fn recorded_cell_type(
    cells_store: &CkbadgerStore,
    tx_hash: &[u8],
    index: i16,
) -> anyhow::Result<Result<(Vec<u8>, i16), String>> {
    let at = format!("0x{}:{index}", hex::encode(tx_hash));
    let Some(cell) =
        cells_store.get_cell_by_outpoint_key(&keys::encode_outpoint(tx_hash, index))?
    else {
        return Ok(Err(format!("its recorded cell {at} has no payload")));
    };
    Ok(cell_type(&cell).ok_or_else(|| format!("its recorded cell {at} has no type script")))
}

fn cell_type(cell: &ckbadger_store::types::LiveCellInfo) -> Option<(Vec<u8>, i16)> {
    Some((cell.type_code_hash?.to_vec(), cell.type_hash_type?))
}

/// The one code among `codes`, or why there is not one.
fn the_one(
    codes: std::collections::BTreeSet<(Vec<u8>, i16)>,
    what: &str,
) -> Result<(Vec<u8>, i16), String> {
    match codes.len() {
        1 => Ok(codes.into_iter().next().expect("one code")),
        0 => Err(format!("no {what}")),
        n => Err(format!("{n} {what}s under different codes")),
    }
}

/// An item's type script (code hash, hash type) as the indexer classified the item, read from
/// what the stores hold for it, never guessed from bytes a transaction carries (anyone can write a
/// second output whose args equal an item's id under any code):
///
/// - a token: its row (`CF_TOKENS`, keyed by the type script hash the item is), the script itself;
/// - a spore, and an identity of the shared id space (did:ckb, .bit Cell, `.cell`): the first cell
///   the indexer recorded for it in the item outpoint index (`object_outpoint_by_id`); a .bit
///   account: the first cell recorded for its account id; the payload holds its type script, and
///   every cell recorded for one item was classified by the same rule;
/// - an m-NFT token: the outputs of its creating transaction (its row's `created_at_tx`) the
///   indexer recorded as this token (`object_outpoints`, MNFT_TOKEN);
/// - a Spore cluster, for which the indexer records no cell: the outputs of its creating
///   transaction it would classify as a cluster, a code the protocol registry names `Cluster`
///   (the classifier's own table) with the cluster's id as args.
///
/// `Ok(Err(why))` where the stores cannot say: no row or no recorded cell, a recorded payload with
/// no type script, or two codes for one item (a shape a transaction may carry). A read that fails
/// is `Err`.
fn item_type_script(
    store: &CkbadgerStore,
    cells_store: &CkbadgerStore,
    item: &ItemDelta,
    outputs: &mut CreatingOutputs,
    items: &mut ItemDeltaCache,
) -> anyhow::Result<Result<(Vec<u8>, i16), String>> {
    use ckbadger_indexer::parser::registry::{ProtocolScript, PROTOCOL_REGISTRY};
    use ckbadger_store::types::ObjectStandard;
    let id = item.item_id.as_slice();
    let first_recorded = |outpoints: Vec<(Vec<u8>, i16)>| -> anyhow::Result<Result<_, String>> {
        match outpoints.first() {
            Some((tx_hash, index)) => recorded_cell_type(cells_store, tx_hash, *index),
            None => Ok(Err("no cell recorded for it".to_string())),
        }
    };
    match item.kind {
        ItemKind::Token => Ok(match token_facts(store, &mut items.tokens, id)? {
            Some(token) => Ok((token.type_code_hash, token.hash_type)),
            None => Err("no token row".to_string()),
        }),
        ItemKind::Object => {
            if let Some(entry) = store.get_spore(id)? {
                return match entry.standard {
                    ObjectStandard::Spore => {
                        first_recorded(store.list_spore_outpoints_by_spore_id(id)?)
                    }
                    ObjectStandard::SporeCluster => {
                        let made = match outputs.of(store, cells_store, &entry.created_at_tx)? {
                            Ok(made) => made,
                            Err(why) => return Ok(Err(why)),
                        };
                        let codes = made
                            .iter()
                            .flatten()
                            .filter(|cell| cell.type_args.as_deref() == Some(id))
                            .filter_map(cell_type)
                            .filter(|(code, _)| PROTOCOL_REGISTRY.is(code, ProtocolScript::Cluster))
                            .collect();
                        Ok(the_one(codes, "cluster cell in its creating transaction"))
                    }
                    other => Ok(Err(format!("a {other:?} row where an object's is read"))),
                };
            }
            let Some(entry) = store.get_mnft(id)? else {
                return Ok(Err("no spore, cluster or m-NFT row".to_string()));
            };
            if entry.standard != ObjectStandard::MnftToken {
                return Ok(Err(format!(
                    "a {:?} row where an m-NFT token's is read",
                    entry.standard
                )));
            }
            let made_by = entry.created_at_tx.clone();
            let made = match outputs.of(store, cells_store, &made_by)? {
                Ok(made) => made,
                Err(why) => return Ok(Err(why)),
            };
            let indexes: Vec<i16> = (0..made.len())
                .map(i16::try_from)
                .collect::<Result<_, _>>()?;
            let outpoints: Vec<(&[u8], i16)> = indexes
                .iter()
                .map(|index| (made_by.as_slice(), *index))
                .collect();
            let recorded: Vec<i16> = store
                .get_mnft_token_ids_by_outpoints_batch(&outpoints)?
                .into_iter()
                .filter(|(_, _, token)| token.as_slice() == id)
                .map(|(_, index, _)| index)
                .collect();
            let codes = recorded
                .iter()
                .filter_map(|index| made.get(usize::try_from(*index).ok()?)?.as_ref())
                .filter_map(cell_type)
                .collect();
            Ok(the_one(
                codes,
                "recorded m-NFT token cell in its creating transaction",
            ))
        }
        ItemKind::Identity(standard) => first_recorded(match standard {
            IdentityStandard::DotBit => store.list_dotbit_account_outpoints_by_account_id(id)?,
            IdentityStandard::BitCell | IdentityStandard::DidCkb | IdentityStandard::DotCell => {
                store.list_spore_outpoints_by_spore_id(id)?
            }
        }),
    }
}

/// Look up token symbol and decimals from CF_TOKENS, caching results.
fn lookup_token_info(
    store: &CkbadgerStore,
    cache: &mut HashMap<Vec<u8>, TokenDisplay>,
    type_script_hash: &[u8],
) -> anyhow::Result<(Option<String>, Option<u8>)> {
    Ok(token_facts(store, cache, type_script_hash)?
        .map(|t| (t.symbol, t.decimals))
        .unwrap_or((None, None)))
}

/// A token's row as one activity reads it, once for its item deltas and its item types (a row
/// can be large: its icon, its description).
fn token_facts(
    store: &CkbadgerStore,
    cache: &mut HashMap<Vec<u8>, TokenDisplay>,
    type_script_hash: &[u8],
) -> anyhow::Result<TokenDisplay> {
    if let Some(cached) = cache.get(type_script_hash) {
        return Ok(cached.clone());
    }
    let result = store
        .get_token(type_script_hash)
        .map_err(|e| {
            anyhow::anyhow!(
                "failed to read token 0x{} for activity: {e}",
                hex::encode(type_script_hash)
            )
        })?
        .map(|t| TokenFacts {
            symbol: t.symbol.clone(),
            decimals: t.decimals.map(|d| d as u8),
            type_code_hash: t.type_code_hash,
            hash_type: i16::from(t.hash_type),
        });
    cache.insert(type_script_hash.to_vec(), result.clone());
    Ok(result)
}

/// The names of one response's scripts: the request's namer
/// (`utils::ScriptNamer`, the one rule, the code a pair runs), each pair named
/// once.
pub(crate) type ScriptNameCache<'a> = crate::utils::ScriptNamer<'a>;

fn convert_type_call(
    cache: &mut ScriptNameCache<'_>,
    call: &TypeCallEntry,
) -> anyhow::Result<TypeCallResponse> {
    let hash_type = hash_type_to_str(call.type_hash_type).ok_or_else(|| {
        anyhow::anyhow!(
            "unsupported script hash_type {} in activity type_call",
            call.type_hash_type
        )
    })?;
    let script_hash = compute_script_hash(
        &call.type_code_hash,
        call.type_hash_type as u8,
        &call.type_args,
    );
    let script_name = cache.name_stored(&call.type_code_hash, call.type_hash_type)?;

    Ok(TypeCallResponse {
        type_code_hash: format!("0x{}", hex::encode(&call.type_code_hash)),
        type_hash_type: hash_type.to_string(),
        type_args: format!("0x{}", hex::encode(&call.type_args)),
        script_hash: format!("0x{}", hex::encode(script_hash)),
        script_name_by: script_name.as_ref().and_then(|name| name.by()),
        script_name: script_name.map(|name| name.name),
    })
}

fn convert_type_calls(
    cache: &mut ScriptNameCache<'_>,
    calls: &[TypeCallEntry],
) -> anyhow::Result<Vec<TypeCallResponse>> {
    calls
        .iter()
        .map(|call| convert_type_call(cache, call))
        .collect()
}

type ArgsDecoder = fn(&[u8]) -> Option<serde_json::Value>;

/// Parse a 0x-prefixed hex string into bytes. Panics on invalid hex (test constants only).
#[cfg(test)]
fn parse_hex_code_hash(hex_str: &str) -> Vec<u8> {
    let hex = hex_str.strip_prefix("0x").unwrap_or(hex_str);
    hex::decode(hex).expect("invalid hex in lock args decoder constant")
}

/// The lock-args codecs a registry entry can name in `[lock_args] codec`
/// (`docs/metadata/scripts/<slug>.toml`): layouts implemented here from their
/// specs, each with what it expects (the reason text when args do not fit).
/// Which lock uses which codec is registry data, never a code-hash table in
/// this file.
fn lock_args_codec(name: &str) -> Option<(ArgsDecoder, &'static str)> {
    Some(match name {
        "secp256k1_blake160" => (decode_secp256k1_blake160_args, "20 bytes"),
        "multisig" => (decode_multisig_args, "20 or 28 bytes"),
        "anyone_can_pay" => (decode_anyone_can_pay_args, "20 to 22 bytes"),
        "omnilock" => (
            decode_omnilock_args,
            "a 21-byte auth, a flags byte using only bits 0x01-0x08, then exactly the fields \
             those flags turn on",
        ),
        "rgbpp" => (decode_rgbpp_lock_args, "at least 36 bytes"),
        "btc_time_lock" => (decode_btc_time_lock_args, "at least 36 bytes"),
        "fiber_funding" => (decode_fiber_funding_lock_args, "at least 20 bytes"),
        "fiber_commitment" => (
            decode_fiber_commitment_lock_args,
            "36, 56 or 57 bytes, or 58 bytes ending in the features byte 0x01",
        ),
        "utxoswap_intent" => (
            decode_utxoswap_intent_args,
            "at least 57 bytes with a known intent type at byte 56",
        ),
        _ => return None,
    })
}

/// A lock's args read by its registry layout. `decoded` and `error` are
/// never both set: a lock whose registry entry declares no layout has
/// neither; args that do not fit the declared layout have `error` — why,
/// naming the script — and no `decoded`.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct DecodedLockArgs {
    pub decoded: Option<serde_json::Value>,
    pub error: Option<String>,
}

/// The one lock-args decoder (activity lock calls and the transaction
/// detail): the args of a lock with code hash `code_hash`, read by the layout
/// its scripts-registry entry declares (`[lock_args]`, R-8); integers as exact
/// strings. The shape of on-chain lock args is not an indexer invariant, so
/// args that do not fit are a stated `error`, never a failed response; `Err`
/// is only a registry or encoding defect.
pub(crate) fn decode_lock_args(code_hash: &[u8], args: &[u8]) -> Result<DecodedLockArgs, String> {
    use ckbadger_indexer::parser::registry::{LockArgsLayout, SCRIPT_LAYOUTS};
    let Some(script) = SCRIPT_LAYOUTS.get(code_hash) else {
        return Ok(DecodedLockArgs::default());
    };
    let read = match &script.lock_args {
        None => return Ok(DecodedLockArgs::default()),
        Some(LockArgsLayout::Codec(codec)) => {
            let (decoder, expects) = lock_args_codec(codec).ok_or_else(|| {
                format!(
                    "scripts registry entry {} ({}) names lock-args codec `{codec}`, which no \
                     decoder implements",
                    script.name, script.slug
                )
            })?;
            decoder(args).ok_or_else(|| {
                format!(
                    "{}: args are {} bytes, layout {codec} expects {expects}",
                    script.name,
                    args.len()
                )
            })
        }
        Some(LockArgsLayout::Fields(fields)) => {
            decode_lock_args_fields(&script.name, &script.slug, fields, args)
        }
    };
    Ok(match read {
        Ok(decoded) => DecodedLockArgs {
            decoded: Some(integers_as_strings(decoded, "decoded lock args")?),
            error: None,
        },
        Err(error) => DecodedLockArgs {
            decoded: None,
            error: Some(error),
        },
    })
}

/// Read `args` as the registry's fixed field layout: `{ "lock": <slug>,
/// <field>: "0x…", … }`. `Err` says why the args do not fit: not exactly the
/// layout's total length, or a fixed field holding another value.
fn decode_lock_args_fields(
    name: &str,
    slug: &str,
    fields: &[ckbadger_indexer::parser::registry::LockArgsField],
    args: &[u8],
) -> Result<serde_json::Value, String> {
    let total: usize = fields.iter().map(|field| field.bytes).sum();
    if args.len() != total {
        let layout = fields
            .iter()
            .map(|field| format!("{}({})", field.name, field.bytes))
            .collect::<Vec<_>>()
            .join(" + ");
        return Err(format!(
            "{name}: args are {} bytes, layout {layout} expects {total}",
            args.len()
        ));
    }
    let mut decoded = serde_json::Map::new();
    decoded.insert("lock".into(), serde_json::json!(slug));
    let mut offset = 0;
    for field in fields {
        let bytes = &args[offset..offset + field.bytes];
        offset += field.bytes;
        if let Some(expected) = &field.equals {
            if bytes != expected.as_slice() {
                return Err(format!(
                    "{name}: field {}: expected 0x{}, got 0x{}",
                    field.name,
                    hex::encode(expected),
                    hex::encode(bytes)
                ));
            }
        }
        decoded.insert(
            field.name.clone(),
            serde_json::json!(format!("0x{}", hex::encode(bytes))),
        );
    }
    Ok(serde_json::Value::Object(decoded))
}

/// secp256k1-blake160-sighash-all: args are the 20-byte blake160 of the
/// public key.
fn decode_secp256k1_blake160_args(args: &[u8]) -> Option<serde_json::Value> {
    (args.len() == 20).then(|| {
        serde_json::json!({
            "lock": "secp256k1_blake160",
            "pubkeyHash": format!("0x{}", hex::encode(args)),
        })
    })
}

/// secp256k1-blake160-multisig-all: the 20-byte blake160 of the multisig
/// script, optionally followed by an 8-byte `since` (u64 LE) the inputs must
/// satisfy.
fn decode_multisig_args(args: &[u8]) -> Option<serde_json::Value> {
    let mut decoded = serde_json::json!({
        "lock": "multisig",
        "multisigScriptHash": format!("0x{}", hex::encode(args.get(..20)?)),
    });
    match args.len() {
        20 => {}
        28 => {
            decoded["since"] = serde_json::json!(u64::from_le_bytes(args[20..28].try_into().ok()?))
        }
        _ => return None,
    }
    Some(decoded)
}

/// anyone-can-pay (RFC-0026): a 20-byte pubkey hash, then optionally the
/// minimum-CKB exponent and the minimum-UDT exponent, one byte each.
fn decode_anyone_can_pay_args(args: &[u8]) -> Option<serde_json::Value> {
    if !(20..=22).contains(&args.len()) {
        return None;
    }
    let mut decoded = serde_json::json!({
        "lock": "anyone_can_pay",
        "pubkeyHash": format!("0x{}", hex::encode(&args[..20])),
    });
    if let Some(exponent) = args.get(20) {
        decoded["minimumCkbExponent"] = serde_json::json!(exponent);
    }
    if let Some(exponent) = args.get(21) {
        decoded["minimumUdtExponent"] = serde_json::json!(exponent);
    }
    Some(decoded)
}

/// Omnilock (RFC-0042): a 21-byte auth (flag + 20-byte content), the
/// omnilock flags byte, then exactly the optional fields the flags turn on,
/// in order — administrator RC cell type ID (0x1, 32 B), anyone-can-pay
/// minimums (0x2, 2 B), time-lock `since` (0x4, 8 B LE), supply type script
/// hash (0x8, 32 B).
fn decode_omnilock_args(args: &[u8]) -> Option<serde_json::Value> {
    let flag = *args.first()?;
    let flags = *args.get(21)?;
    if flags & !0x0f != 0 {
        return None;
    }
    let auth_kind = match flag {
        0x00 => Some("secp256k1_blake160"),
        0x01 => Some("ethereum"),
        0x02 => Some("eos"),
        0x03 => Some("tron"),
        0x04 => Some("bitcoin"),
        0x05 => Some("dogecoin"),
        0x06 => Some("ckb_multisig"),
        0x07 => Some("ethereum_display"),
        0xfc => Some("owner_lock"),
        0xfd => Some("exec"),
        0xfe => Some("dynamic_linking"),
        _ => None,
    };
    let mut decoded = serde_json::json!({
        "lock": "omnilock",
        "authFlag": flag,
        "authKind": auth_kind,
        "authContent": format!("0x{}", hex::encode(&args[1..21])),
        "omnilockFlags": flags,
    });
    let mut rest = &args[22..];
    let mut take = |len: usize| -> Option<&[u8]> {
        let (field, tail) = (rest.get(..len)?, rest.get(len..)?);
        rest = tail;
        Some(field)
    };
    if flags & 0x1 != 0 {
        decoded["adminListCellTypeId"] = serde_json::json!(format!("0x{}", hex::encode(take(32)?)));
    }
    if flags & 0x2 != 0 {
        let minimums = take(2)?;
        decoded["minimumCkbExponent"] = serde_json::json!(minimums[0]);
        decoded["minimumUdtExponent"] = serde_json::json!(minimums[1]);
    }
    if flags & 0x4 != 0 {
        decoded["since"] = serde_json::json!(u64::from_le_bytes(take(8)?.try_into().ok()?));
    }
    if flags & 0x8 != 0 {
        decoded["supplyTypeScriptHash"] =
            serde_json::json!(format!("0x{}", hex::encode(take(32)?)));
    }
    rest.is_empty().then_some(decoded)
}

fn decode_rgbpp_lock_args(args: &[u8]) -> Option<serde_json::Value> {
    if args.len() < 36 {
        return None;
    }
    let out_index = u32::from_le_bytes(args[0..4].try_into().ok()?);
    let mut btc_txid = args[4..36].to_vec();
    btc_txid.reverse(); // little-endian to big-endian for display
    Some(serde_json::json!({
        "protocol": "rgbpp",
        "btcTxid": hex::encode(&btc_txid),
        "outIndex": out_index,
    }))
}

fn decode_btc_time_lock_args(args: &[u8]) -> Option<serde_json::Value> {
    // BTC time lock args: btc_txid is always the last 32 bytes (LE).
    // Minimum 36 bytes (matching indexer parser).
    if args.len() < 36 {
        return None;
    }
    let mut btc_txid = args[args.len() - 32..].to_vec();
    btc_txid.reverse();
    Some(serde_json::json!({
        "protocol": "rgbpp",
        "action": "btcTimeLock",
        "btcTxid": hex::encode(&btc_txid),
    }))
}

/// Fiber funding-lock args read by the indexer's one funding-lock arbiter
/// (`parser::fiber::parse_funding_lock_args`): at least 20 bytes, of which
/// every deployed lock reads the first 20.
fn decode_fiber_funding_lock_args(args: &[u8]) -> Option<serde_json::Value> {
    let parsed = ckbadger_indexer::parser::fiber::parse_funding_lock_args(args)?;
    Some(serde_json::json!({
        "protocol": "fiber",
        "action": "funding",
        "pubkeyHash": format!("0x{}", hex::encode(parsed.pubkey_hash)),
    }))
}

/// Fiber commitment-lock args read by the indexer's one commitment-lock
/// arbiter (`parser::fiber::parse_commitment_lock_args`): only the layouts a
/// deployed lock accepts. The 57- and 58-byte layouts add the settlement hash
/// and flag.
fn decode_fiber_commitment_lock_args(args: &[u8]) -> Option<serde_json::Value> {
    let parsed = ckbadger_indexer::parser::fiber::parse_commitment_lock_args(args)?;
    let mut obj = serde_json::json!({
        "protocol": "fiber",
        "action": "commitment",
        "pubkeyHash": format!("0x{}", hex::encode(parsed.pubkey_hash)),
        "delayEpoch": parsed.delay_epoch,
        "version": parsed.version,
    });
    if let (Some(settlement_hash), Some(settlement_flag)) =
        (parsed.settlement_hash(), parsed.settlement_flag())
    {
        obj["settlementHash"] = serde_json::json!(format!("0x{}", hex::encode(settlement_hash)));
        obj["settlementFlag"] = serde_json::json!(settlement_flag);
    }
    Some(obj)
}

/// Decodes UTXOSwap intent lock args for live display.
///
/// Both the field layout and the field names come from the single shared
/// decoder in `ckbadger-common`, so this display can never drift from the
/// metadata the indexer persists. Only the `protocol` tag is added here.
fn decode_utxoswap_intent_args(args: &[u8]) -> Option<serde_json::Value> {
    let mut result = ckbadger_common::utxoswap::parse_intent_args(args)?.metadata_json();
    result["protocol"] = serde_json::json!("utxoswap");
    Some(result)
}

fn convert_lock_call(
    cache: &mut ScriptNameCache<'_>,
    call: &LockCallEntry,
) -> anyhow::Result<LockCallResponse> {
    let hash_type = hash_type_to_str(call.lock_hash_type).ok_or_else(|| {
        anyhow::anyhow!(
            "unsupported lock hash_type {} in activity lock_call",
            call.lock_hash_type
        )
    })?;
    let script_hash = compute_script_hash(
        &call.lock_code_hash,
        call.lock_hash_type as u8,
        &call.lock_args,
    );
    let script_name = cache.name_stored(&call.lock_code_hash, call.lock_hash_type)?;
    let DecodedLockArgs {
        decoded,
        error: decode_error,
    } = decode_lock_args(&call.lock_code_hash, &call.lock_args).map_err(anyhow::Error::msg)?;

    Ok(LockCallResponse {
        lock_code_hash: format!("0x{}", hex::encode(&call.lock_code_hash)),
        lock_hash_type: hash_type.to_string(),
        lock_args: format!("0x{}", hex::encode(&call.lock_args)),
        script_hash: format!("0x{}", hex::encode(script_hash)),
        script_name_by: script_name.as_ref().and_then(|name| name.by()),
        script_name: script_name.map(|name| name.name),
        decoded,
        decode_error,
    })
}

fn convert_lock_calls(
    cache: &mut ScriptNameCache<'_>,
    calls: &[LockCallEntry],
) -> anyhow::Result<Vec<LockCallResponse>> {
    calls
        .iter()
        .map(|call| convert_lock_call(cache, call))
        .collect()
}

fn convert_protocol_action(
    action: &ckbadger_store::types::ProtocolAction,
) -> anyhow::Result<ProtocolActionResponse> {
    let metadata = action.metadata_value().map_err(|e| {
        anyhow::anyhow!(
            "failed to decode protocol metadata for protocol={} action={}: {}",
            action.protocol,
            action.action,
            e
        )
    })?;

    let protocol_name = protocol_display_name(&action.protocol).ok_or_else(|| {
        anyhow::anyhow!(
            "protocol action {}/{} has no display name in docs/metadata/protocols.toml",
            action.protocol,
            action.action
        )
    })?;
    let context = format!("{}/{} protocol metadata", action.protocol, action.action);
    Ok(ProtocolActionResponse {
        protocol: action.protocol.clone(),
        protocol_name: protocol_name.to_string(),
        action: action.action.clone(),
        metadata: integers_as_strings(metadata, &context).map_err(anyhow::Error::msg)?,
    })
}

/// What a pool row adds to an activity response.
///
/// Its presence is what turns the chain-position fields null: a transaction the
/// pool holds has no block to report, and reporting the mirror's provisional
/// zeros would let a caller mistake it for genesis.
pub(crate) struct PoolRowMeta<'a> {
    pub status: String,
    pub time_added_to_pool: String,
    pub interpretation: crate::pool::InterpretationResponse,
    /// The mirror's record of the pool transaction: its outputs and its spent
    /// inputs' lock scripts, which the chain view may not know yet.
    pub record: &'a PoolTxRecord,
}

/// Build an address-scoped activity response from a TxActions for a specific participant.
///
/// The SAME builder serves committed rows and tx-pool rows: a transaction must
/// read identically before and after commit, and only its truth status changes.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub(crate) fn build_activity_response(
    store: &CkbadgerStore,
    network: &str,
    actions: &TxActions,
    participant: &ParticipantDelta,
    script_info_cache: &mut ScriptNameCache<'_>,
    item_cache: &mut ItemDeltaCache,
    address_cache: &mut HashMap<Vec<u8>, Option<String>>,
    item_types: Option<ItemTypesRead>,
    pool: Option<PoolRowMeta<'_>>,
) -> anyhow::Result<ActivityResponse> {
    anyhow::ensure!(
        item_types.is_some() == pool.is_none(),
        "activity tx 0x{}: item types are a committed row's, and only a committed row's",
        hex::encode(&actions.tx_hash)
    );
    let item_deltas = participant
        .item_deltas
        .iter()
        .map(|item| convert_item_delta(item, item_cache, store, pool.is_none()))
        .collect::<anyhow::Result<Vec<_>>>()?;

    let participants = actions
        .participants
        .iter()
        .filter(|p| p.id != participant.id)
        .map(|p| {
            participant_ref(
                store,
                network,
                p,
                pool.as_ref().map(|meta| meta.record),
                address_cache,
                script_info_cache,
            )
        })
        .collect::<anyhow::Result<Vec<_>>>()?;

    Ok(ActivityResponse {
        tx_hash: format!("0x{}", hex::encode(&actions.tx_hash)),
        block_number: pool.is_none().then_some(actions.block_number),
        tx_index: pool.is_none().then_some(actions.tx_index),
        timestamp: pool.is_none().then(|| actions.timestamp.to_string()),
        pool_status: pool.as_ref().map(|meta| meta.status.clone()),
        time_added_to_pool: pool.as_ref().map(|meta| meta.time_added_to_pool.clone()),
        interpretation: pool.as_ref().map(|meta| meta.interpretation.clone()),
        ckb_delta: participant.ckb_delta.to_string(),
        used_delta: participant.used_delta.to_string(),
        is_cellbase: actions.is_cellbase,
        item_deltas,
        type_calls: convert_type_calls(script_info_cache, &actions.type_calls)?,
        lock_calls: convert_lock_calls(script_info_cache, &actions.lock_calls)?,
        item_types: item_types.as_ref().and_then(|read| read.types.clone()),
        item_types_unread: item_types.and_then(|read| read.unread),
        protocol_actions: actions
            .protocol_actions
            .iter()
            .map(convert_protocol_action)
            .collect::<anyhow::Result<Vec<_>>>()?,
        participants,
        roles: participant_roles::names(participant.roles),
        tags: participant.tags,
    })
}

/// Build a global activity response showing all participants from a TxActions.
#[allow(clippy::type_complexity)]
pub(crate) fn build_global_activity_response(
    store: &CkbadgerStore,
    ao_store: &CkbadgerStore,
    network: &str,
    actions: &TxActions,
    script_info_cache: &mut ScriptNameCache<'_>,
    address_cache: &mut HashMap<Vec<u8>, Option<String>>,
) -> anyhow::Result<GlobalActivityResponse> {
    build_global_activity_response_with_pool(
        store,
        ao_store,
        network,
        actions,
        script_info_cache,
        address_cache,
        None,
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn build_global_activity_response_with_pool(
    store: &CkbadgerStore,
    ao_store: &CkbadgerStore,
    network: &str,
    actions: &TxActions,
    script_info_cache: &mut ScriptNameCache<'_>,
    address_cache: &mut HashMap<Vec<u8>, Option<String>>,
    pool_record: Option<&PoolTxRecord>,
) -> anyhow::Result<GlobalActivityResponse> {
    let mut item_cache = ItemDeltaCache::default();

    let participants = actions
        .participants
        .iter()
        .map(|p| {
            let reference = participant_ref(
                store,
                network,
                p,
                pool_record,
                address_cache,
                script_info_cache,
            )?;
            let item_deltas = p
                .item_deltas
                .iter()
                .map(|item| convert_item_delta(item, &mut item_cache, store, pool_record.is_none()))
                .collect::<anyhow::Result<Vec<_>>>()?;
            Ok(ParticipantResponse {
                address: reference.address,
                lock_hash: reference.lock_hash,
                lock_hash_prefix: reference.lock_hash_prefix,
                lock_kind: reference.lock_kind,
                lock_kind_by: reference.lock_kind_by,
                roles: reference.roles,
                ckb_delta: p.ckb_delta.to_string(),
                used_delta: p.used_delta.to_string(),
                item_deltas,
                tags: p.tags,
            })
        })
        .collect::<anyhow::Result<Vec<_>>>()?;

    let types = match pool_record {
        None => item_types(store, ao_store, actions, script_info_cache, &mut item_cache)?,
        Some(_) => ItemTypesRead {
            types: None,
            unread: None,
        },
    };
    Ok(GlobalActivityResponse {
        tx_hash: format!("0x{}", hex::encode(&actions.tx_hash)),
        block_number: actions.block_number,
        tx_index: actions.tx_index,
        timestamp: actions.timestamp.to_string(),
        is_cellbase: actions.is_cellbase,
        protocol_actions: actions
            .protocol_actions
            .iter()
            .map(convert_protocol_action)
            .collect::<anyhow::Result<Vec<_>>>()?,
        type_calls: convert_type_calls(script_info_cache, &actions.type_calls)?,
        lock_calls: convert_lock_calls(script_info_cache, &actions.lock_calls)?,
        item_types: types.types,
        item_types_unread: types.unread,
        participants,
    })
}

fn validate_activity_filter(filter: Option<&str>) -> Result<(), ApiRouteError> {
    if let Some(value) = filter {
        if !matches!(
            value,
            "all"
                | "ckb"
                | "token"
                | "object"
                | "identity"
                | "dao"
                | "type_call"
                | "lock_call"
                | "script"
        ) && !value.starts_with("protocol:")
        {
            return Err(ApiError::bad_request(format!(
                "invalid activity filter '{}'; expected one of: all, ckb, token, object, identity, dao, type_call, lock_call, script, protocol:<name>",
                value
            )));
        }
    }
    Ok(())
}

fn validate_global_activity_filter(filter: Option<&str>) -> Result<(), ApiRouteError> {
    if let Some(value) = filter {
        if value.is_empty() {
            return Ok(());
        }
        if !matches!(
            value,
            "all" | "ckb" | "token" | "object" | "identity" | "dao" | "script" | "protocol"
        ) {
            return Err(ApiError::bad_request(format!(
                "invalid global activity filter '{}'; expected one of: all, ckb, token, object, identity, dao, script, protocol",
                value
            )));
        }
    }
    Ok(())
}

/// An address's activities, newest first. Every row is canonical by
/// construction: an `activities` row is keyed by its transaction's position and
/// completed from `tx_index` there, so there is no orphaned row to skip (a
/// position without a transaction is an error in the store reader).
fn list_canonical_activities_page(
    store: &CkbadgerStore,
    lock_hash: &[u8],
    limit: usize,
    cursor: Option<(i64, i32)>,
    filter: Option<&str>,
) -> anyhow::Result<Vec<TxActions>> {
    store.list_activities(lock_hash, limit, cursor, filter)
}

/// Whether a `TxActions` falls under a global activity filter. The bucket comes
/// from `ckbadger_store::classify_global_bucket`, the one classification the
/// indexer also keys `CF_TX_ACTIONS_BY_BUCKET` by; `cellbase` and `user` (every
/// transaction but the cellbase) are the filters that name no bucket.
pub(crate) fn matches_global_activity_filter(actions: &TxActions, filter: Option<&str>) -> bool {
    match filter {
        None | Some("") | Some("all") => true,
        Some("cellbase") => actions.is_cellbase,
        Some("user") => !actions.is_cellbase,
        Some(expected) => ckbadger_store::classify_global_bucket(actions)
            .is_some_and(|bucket| bucket.as_filter() == expected),
    }
}

/// List canonical global activities (TX-level), newest first, strictly older
/// than `cursor`.
///
/// A bucket filter reads `CF_TX_ACTIONS_BY_BUCKET`, which the indexer writes
/// beside every non-cellbase `TxActions` row and deletes above the fork point
/// on rollback: each row is canonical by construction, and a page costs one
/// bucket iterator plus one `activities`, one `tx_index` and one
/// `block_headers` multi_get. The unfiltered feed walks `CF_TX_ACTIONS`.
pub(crate) fn list_canonical_global_actions_page(
    store: &CkbadgerStore,
    limit: usize,
    cursor: Option<(i64, i32)>,
    filter: Option<&str>,
) -> anyhow::Result<Vec<TxActions>> {
    if limit == 0 {
        return Ok(Vec::new());
    }
    match filter {
        None | Some("") | Some("all") => {
            list_canonical_unfiltered_actions_page(store, limit, cursor)
        }
        Some(value) => {
            let bucket = ckbadger_store::GlobalBucket::from_filter(value).ok_or_else(|| {
                anyhow::anyhow!("global activity filter '{value}' names no activity bucket")
            })?;
            store.list_tx_actions_page_by_bucket(bucket, cursor, limit)
        }
    }
}

/// The next `limit` rows of the global feed: `activities` holds no cellbase
/// row, so they are exactly the next `limit` rows, completed once.
fn list_canonical_unfiltered_actions_page(
    store: &CkbadgerStore,
    limit: usize,
    cursor: Option<(i64, i32)>,
) -> anyhow::Result<Vec<TxActions>> {
    let rows = store.list_tx_actions_recent(limit, cursor)?;
    ckbadger_store::ensure_no_cellbase_activity(&rows)?;
    Ok(rows)
}

/// Build this lock's tx-pool rows for page one, and the summary describing
/// them: the shared page-one segment, narrowed by this feed's filter.
///
/// Blocking (reads the store for dedup and for script/address resolution).
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub(crate) fn build_pool_activity_rows(
    store: &CkbadgerStore,
    network: &str,
    snapshot: &crate::pool::PoolSnapshot,
    lock_hash: &[u8; 32],
    filter: Option<&str>,
    script_info_cache: &mut ScriptNameCache<'_>,
    item_cache: &mut ItemDeltaCache,
    address_cache: &mut HashMap<Vec<u8>, Option<String>>,
) -> anyhow::Result<(Vec<ActivityResponse>, crate::pool::PoolSummaryResponse)> {
    let segment = address_pool::page_one_segment(store, snapshot, lock_hash)?;

    let mut rows = Vec::with_capacity(segment.rows.len());
    let mut served = Vec::with_capacity(segment.rows.len());
    for row in &segment.rows {
        let record = &row.record;
        // `by_lock` is built from a record's interpreted participants, so a
        // record reached through it must have an interpretation. A missing one
        // is a mirror invariant violation, not a row to skip quietly.
        let actions = record.actions.as_ref().ok_or_else(|| {
            anyhow::anyhow!(
                "pool record indexed by lock 0x{} carries no interpretation: tx=0x{}",
                hex::encode(lock_hash),
                hex::encode(record.tx_hash)
            )
        })?;
        let participant = actions
            .participants
            .iter()
            .find(|p| p.id.matches(lock_hash))
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "pool record indexed by lock 0x{} has no participant for it: tx=0x{}",
                    hex::encode(lock_hash),
                    hex::encode(record.tx_hash)
                )
            })?;
        // The same filter → tag-bitmap mapping the committed index uses.
        if !CkbadgerStore::matches_activity_filter(actions, lock_hash, filter)? {
            continue;
        }

        rows.push(build_activity_response(
            store,
            network,
            actions,
            participant,
            script_info_cache,
            item_cache,
            address_cache,
            None,
            Some(PoolRowMeta {
                status: record.pool_status.as_str().to_string(),
                time_added_to_pool: crate::pool::pool_timestamp_rfc3339(i64::try_from(
                    record.entry.time_added_to_pool_ms,
                )?)
                .map_err(|e| anyhow::anyhow!("{e}"))?,
                interpretation: crate::pool::InterpretationResponse::from(&record.interpretation),
                record,
            }),
        )?);
        served.push(row);
    }

    let summary = address_pool::pool_summary(snapshot, served, segment.truncated)?;
    Ok((rows, summary))
}

async fn get_address_activities(
    State(state): State<Arc<AppState>>,
    Path(addr): Path<String>,
    Query(params): Query<ActivityParams>,
) -> ApiResult<CursorPaginatedResponse<ActivityResponse>> {
    validate_activity_filter(params.filter.as_deref())?;
    // Routing via the shared `is_ckb_address` (not an inline prefix check) so
    // every address entry point shares one case-handling rule.
    let lock_hash = if is_ckb_address(&addr) {
        address_to_lock_script_hash(&addr)
            .map_err(|e| ApiError::bad_request(format!("Invalid address: {}", e)))?
    } else {
        parse_hash32(&addr, "address/lock script hash")?
    };

    let limit = params.limit.clamp(1, 100) as usize;

    let cursor = parse_optional_block_tx_cursor(params.cursor.as_deref(), "activity cursor")?;

    let filter = params.filter.clone();
    let store = state.store.clone();
    let cells = state.append_only_store.clone();
    let network = state.ckb_network.clone();
    let lock32: [u8; 32] = lock_hash.as_slice().try_into().map_err(|_| {
        ApiError::internal(format!(
            "address activities expect a 32-byte lock hash, got {} bytes",
            lock_hash.len()
        ))
    })?;
    // Pool rows are a page-one-only segment: a pool transaction can only land
    // in a future block, so it is later than every committed transaction in
    // canonical order. Keeping it out of cursors is what keeps pagination
    // stable while the pool churns.
    let pool_snapshot = if cursor.is_none() {
        Some(state.pool_mirror.load())
    } else {
        None
    };
    let (next_cursor, activities, pool, total) =
        tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
            let results = list_canonical_activities_page(
                store.as_ref(),
                &lock_hash,
                limit + 1,
                cursor,
                filter.as_deref(),
            )?;

            let has_more = results.len() > limit;
            let page: Vec<_> = results.into_iter().take(limit).collect();

            // The total is the unfiltered activity count, read from the same
            // pinned view as the page; a filtered feed has no stored count.
            let total = if matches!(filter.as_deref(), None | Some("all")) {
                let total = store.address_activity_count(&lock32)?;
                ensure_activity_total_covers_page_one(
                    &lock32,
                    total,
                    cursor,
                    page.len(),
                    has_more,
                )?;
                Some(total)
            } else {
                None
            };

            let next_cursor = if has_more {
                page.last()
                    .map(|actions| format!("{}:{}", actions.block_number, actions.tx_index))
            } else {
                None
            };

            let mut script_info_cache = crate::utils::ScriptNamer::new(&store, &cells);
            let mut item_cache = ItemDeltaCache::default();
            let mut address_cache = HashMap::new();

            let (mut activities, pool) = match pool_snapshot.as_deref() {
                Some(snapshot) => {
                    let (rows, summary) = build_pool_activity_rows(
                        store.as_ref(),
                        &network,
                        snapshot,
                        &lock32,
                        filter.as_deref(),
                        &mut script_info_cache,
                        &mut item_cache,
                        &mut address_cache,
                    )?;
                    (rows, Some(summary))
                }
                None => (Vec::new(), None),
            };

            for actions in &page {
                // `list_activities` serves only transactions the address
                // takes part in (`matches_activity_filter` fails on a stale
                // row); a row without its participant here is a bug, never a
                // row to drop from the page.
                let participant = actions
                    .participants
                    .iter()
                    .find(|p| p.id.matches(&lock32))
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                            "activity of address 0x{} at block={}, tx_idx={} has no participant \
                             for the address",
                            hex::encode(lock32),
                            actions.block_number,
                            actions.tx_index
                        )
                    })?;
                let types = item_types(
                    store.as_ref(),
                    cells.as_ref(),
                    actions,
                    &mut script_info_cache,
                    &mut item_cache,
                )?;
                activities.push(build_activity_response(
                    store.as_ref(),
                    &network,
                    actions,
                    participant,
                    &mut script_info_cache,
                    &mut item_cache,
                    &mut address_cache,
                    Some(types),
                    None,
                )?);
            }

            Ok((next_cursor, activities, pool, total))
        })
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .map_err(|e| ApiError::internal(e.to_string()))?;

    // `total` is the address's activity count (`address_activity_count`):
    // its transactions minus its cellbase transactions, which are never
    // persisted in CF_TX_ACTIONS (W5). It is NOT `AddressBalance.txs_count`,
    // which also counts cellbases (mainnet block 12000000's cellbase-output
    // address: 4_727_769 transactions against 1_682 activities). Committed
    // rows only; pool rows are reported beside it in `pool`.
    let response = match total {
        Some(total) => CursorPaginatedResponse::new(activities, total, limit as i64, next_cursor),
        None => CursorPaginatedResponse::without_total(activities, limit as i64, next_cursor),
    };
    ok(match pool {
        Some(pool) => response.with_pool(pool),
        None => response,
    })
}

/// Page one of an unfiltered address feed starts at the newest activity, so
/// the persisted total must cover its committed rows, and equal them when
/// there is no next page. Anything else means the counter and the rows it
/// counts disagree: an error naming the address, never a clamped total.
fn ensure_activity_total_covers_page_one(
    lock_hash: &[u8; 32],
    total: i64,
    cursor: Option<(i64, i32)>,
    committed_rows: usize,
    has_more: bool,
) -> anyhow::Result<()> {
    if cursor.is_some() {
        return Ok(());
    }
    let committed_rows = i64::try_from(committed_rows)?;
    if committed_rows > total || (!has_more && committed_rows != total) {
        anyhow::bail!(
            "address 0x{} activity total {} disagrees with page one: {} committed rows, has_more={}",
            hex::encode(lock_hash),
            total,
            committed_rows,
            has_more
        );
    }
    Ok(())
}

/// Transactions of one block whose actions `/activities?blockNumber=` reads
/// with one `activities` multi_get.
const BLOCK_ACTIONS_CHUNK: usize = 128;

/// The `TxActions` of one block's transactions (`txs`, in the page's order),
/// filtered, until `limit + 1` match: `BLOCK_ACTIONS_CHUNK` transactions'
/// rows at a time with one `activities` read, completed with the block's hash
/// read once; a cellbase goes through [`cellbase_transaction_actions`].
fn block_transaction_actions(
    store: &CkbadgerStore,
    node: Option<&ckb_store_reader::CkbChainReader>,
    block: i64,
    txs: &[(i32, ckbadger_store::TxIndexEntry)],
    filter: Option<&str>,
    limit: usize,
) -> anyhow::Result<Vec<TxActions>> {
    if txs.is_empty() {
        return Ok(Vec::new());
    }
    let block_hash = store
        .get_block_header(block)?
        .ok_or_else(|| anyhow::anyhow!("activities of block {block}: no block header"))?
        .hash;
    let mut rows = Vec::new();
    for chunk in txs.chunks(BLOCK_ACTIONS_CHUNK) {
        let page: Vec<ResolvedTx<'_>> = chunk
            .iter()
            .map(|(index, entry)| ResolvedTx {
                block_number: block,
                tx_index: *index,
                entry,
                block_hash: &block_hash,
            })
            .collect();
        for ((index, entry), actions) in chunk.iter().zip(store.tx_actions_of(&page)?) {
            let actions = match actions {
                Some(actions) => actions,
                None => cellbase_transaction_actions(node, block, *index, entry, &block_hash)?,
            };
            if matches_global_activity_filter(&actions, filter) {
                rows.push(actions);
            }
            if rows.len() > limit {
                return Ok(rows);
            }
        }
    }
    Ok(rows)
}

/// The `TxActions` of one canonical transaction: its `activities` row, or
/// for a cellbase [`cellbase_transaction_actions`].
fn canonical_transaction_actions(
    store: &CkbadgerStore,
    node: Option<&ckb_store_reader::CkbChainReader>,
    block: i64,
    index: i32,
    entry: &ckbadger_store::TxIndexEntry,
) -> anyhow::Result<TxActions> {
    let block_hash = store
        .get_block_header(block)?
        .ok_or_else(|| anyhow::anyhow!("transaction block {block} has no block header"))?
        .hash;
    let [actions] = store
        .tx_actions_of(&[ResolvedTx {
            block_number: block,
            tx_index: index,
            entry,
            block_hash: &block_hash,
        }])?
        .try_into()
        .expect("one transaction, one row");
    match actions {
        Some(actions) => Ok(actions),
        None => cellbase_transaction_actions(node, block, index, entry, &block_hash),
    }
}

/// Cellbase actions are intentionally not materialized. Reuse the indexer's
/// interpreter over the node's exact outputs when a block/detail requests one.
fn cellbase_transaction_actions(
    node: Option<&ckb_store_reader::CkbChainReader>,
    block: i64,
    index: i32,
    entry: &ckbadger_store::TxIndexEntry,
    block_hash: &[u8],
) -> anyhow::Result<TxActions> {
    anyhow::ensure!(
        entry.is_cellbase,
        "transaction 0x{} at block={block} index={index} is not a cellbase",
        hex::encode(entry.tx_hash)
    );
    use ckb_types::prelude::*;
    let node = node.ok_or_else(|| {
        anyhow::anyhow!("cellbase activity at block {block} requires CKB RocksDB reader")
    })?;
    let tx = node.get_transaction(&entry.tx_hash)?.ok_or_else(|| {
        anyhow::anyhow!(
            "cellbase transaction 0x{} at block {block} missing from CKB RocksDB",
            hex::encode(entry.tx_hash)
        )
    })?;
    anyhow::ensure!(
        tx.is_cellbase(),
        "indexed cellbase 0x{} is not cellbase in node bytes",
        hex::encode(entry.tx_hash)
    );
    anyhow::ensure!(
        tx.outputs().len() == tx.outputs_data().len(),
        "cellbase output/data length mismatch at block {block}"
    );
    let cells = tx
        .outputs()
        .into_iter()
        .zip(tx.outputs_data())
        .map(|(output, data)| -> anyhow::Result<_> {
            let lock = output.lock();
            let capacity: u64 = output.capacity().unpack();
            let type_script = output.type_().to_opt().map(|script| {
                (
                    script.code_hash().as_slice().to_vec(),
                    i16::from(script.hash_type().as_slice()[0]),
                    script.args().raw_data().to_vec(),
                )
            });
            let data = data.raw_data();
            let data_hash = ckb_types::packed::CellOutput::calc_data_hash(&data)
                .as_slice()
                .to_vec();
            let cell = crate::pool::ResolvedCell::new(
                i64::try_from(capacity)?,
                lock.code_hash().as_slice().to_vec(),
                i16::from(lock.hash_type().as_slice()[0]),
                lock.args().raw_data().to_vec(),
                type_script,
                data.to_vec(),
            )
            .map_err(anyhow::Error::msg)?;
            Ok((cell, data_hash))
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    let outputs = cells
        .iter()
        .map(|(cell, data_hash)| -> anyhow::Result<_> {
            Ok(ckbadger_indexer::db::OutputCellView {
                capacity: cell.capacity,
                lock_code_hash: &cell.lock_code_hash,
                lock_hash_type: cell.lock_hash_type,
                lock_args: &cell.lock_args,
                lock_script_hash: &cell.lock_script_hash,
                type_code_hash: cell.type_code_hash.as_deref(),
                type_hash_type: cell.type_hash_type,
                type_args: cell.type_args.as_deref(),
                type_script_hash: cell.type_script_hash.as_deref(),
                data_hash,
                data_size: i32::try_from(cell.data.len())?,
                data: &cell.data,
            })
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    let view = ckbadger_indexer::db::TxView {
        tx_hash: &entry.tx_hash,
        block_hash,
        tx_index: index,
        block_number: block,
        timestamp: entry.timestamp,
        is_cellbase: true,
        dotbit_action: None,
        inputs: vec![],
        outputs,
    };
    ckbadger_indexer::db::build_tx_actions_for_block(&[view], &[])?
        .pop()
        .ok_or_else(|| anyhow::anyhow!("cellbase interpreter returned no action at block {block}"))
}

async fn get_transaction_activity(
    State(state): State<Arc<AppState>>,
    Path(hash): Path<String>,
) -> ApiResult<Option<GlobalActivityResponse>> {
    let hash = parse_hash32(&hash, "transaction hash")?;
    let (block, index, tx) = state
        .store
        .get_tx_by_hash(&hash)
        .map_err(|e| ApiError::internal(e.to_string()))?
        .ok_or_else(|| ApiError::not_found("Committed transaction not found"))?;
    let actions =
        canonical_transaction_actions(&state.store, state.ckb_store.as_deref(), block, index, &tx)
            .map_err(|e| ApiError::internal(e.to_string()))?;
    let activity = build_global_activity_response(
        &state.store,
        &state.append_only_store,
        &state.ckb_network,
        &actions,
        &mut crate::utils::ScriptNamer::new(&state.store, &state.append_only_store),
        &mut HashMap::new(),
    )
    .map_err(|e| ApiError::internal(e.to_string()))?;
    ok(Some(activity))
}

async fn get_global_activities(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ActivityParams>,
) -> ApiResult<CursorPaginatedResponse<GlobalActivityResponse>> {
    validate_global_activity_filter(params.filter.as_deref())?;

    let limit = params.limit.clamp(1, 100) as usize;
    let cursor = parse_optional_block_tx_cursor(params.cursor.as_deref(), "activity cursor")?;

    let block_number = params
        .block_number
        .map(|n| crate::utils::validate_block_number(n, "blockNumber"))
        .transpose()?;
    let ascending = match (params.order.as_deref(), block_number) {
        (None | Some("desc"), _) => false,
        (Some("asc"), Some(_)) => true,
        (Some("asc"), None) => {
            return Err(ApiError::bad_request(
                "order=asc reads one block's activities in block order; it needs blockNumber",
            ))
        }
        (Some(other), _) => {
            return Err(ApiError::bad_request(format!(
                "invalid order '{other}'; expected asc or desc"
            )))
        }
    };
    if let (Some(block), Some((cursor_block, _))) = (block_number, cursor) {
        if block != cursor_block {
            return Err(ApiError::bad_request(
                "activity cursor belongs to another block",
            ));
        }
    }
    let store = state.store.clone();
    let ao_store = state.append_only_store.clone();
    let network = state.ckb_network.clone();
    let node = state.ckb_store.clone();
    let filter = params.filter.clone();
    let (next_cursor, activities) = tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
        let results = if let Some(block) = block_number {
            let mut txs = store.list_block_txs(block)?;
            if !ascending {
                txs.reverse();
            }
            // The cursor is the last row served: continue past it in the
            // page's own direction.
            txs.retain(|(index, _)| {
                !cursor.is_some_and(|(_, last)| {
                    if ascending {
                        *index <= last
                    } else {
                        *index >= last
                    }
                })
            });
            block_transaction_actions(
                &store,
                node.as_deref(),
                block,
                &txs,
                filter.as_deref(),
                limit,
            )?
        } else {
            list_canonical_global_actions_page(
                store.as_ref(),
                limit + 1,
                cursor,
                filter.as_deref(),
            )?
        };

        let has_more = results.len() > limit;
        let page: Vec<_> = results.into_iter().take(limit).collect();
        let next_cursor = if has_more {
            page.last()
                .map(|actions| format!("{}:{}", actions.block_number, actions.tx_index))
        } else {
            None
        };

        let mut script_info_cache = crate::utils::ScriptNamer::new(&store, &ao_store);
        let mut address_cache = HashMap::new();
        let activities = page
            .iter()
            .map(|actions| {
                build_global_activity_response(
                    store.as_ref(),
                    ao_store.as_ref(),
                    &network,
                    actions,
                    &mut script_info_cache,
                    &mut address_cache,
                )
            })
            .collect::<anyhow::Result<Vec<_>>>()?;

        Ok((next_cursor, activities))
    })
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?
    .map_err(|e| ApiError::internal(e.to_string()))?;

    ok(CursorPaginatedResponse::without_total(
        activities,
        limit as i64,
        next_cursor,
    ))
}

async fn get_latest_activities(
    State(state): State<Arc<AppState>>,
    Query(params): Query<LatestActivityParams>,
) -> ApiResult<Vec<GlobalActivityResponse>> {
    let limit = params.limit.clamp(1, 64);
    let store = state.store.clone();
    let ao_store = state.append_only_store.clone();
    let network = state.ckb_network.clone();
    let activities = tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
        let items = store.get_latest_activities()?;

        // Every row is canonical: completed from `tx_index` at its position.
        let mut script_info_cache = crate::utils::ScriptNamer::new(&store, &ao_store);
        let mut address_cache = HashMap::new();
        let activities: Vec<GlobalActivityResponse> = items
            .iter()
            .take(limit)
            .map(|actions| {
                build_global_activity_response(
                    store.as_ref(),
                    ao_store.as_ref(),
                    &network,
                    actions,
                    &mut script_info_cache,
                    &mut address_cache,
                )
            })
            .collect::<anyhow::Result<Vec<_>>>()?;

        Ok(activities)
    })
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?
    .map_err(|e| ApiError::internal(e.to_string()))?;

    ok(activities)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discrete_item_delta_requires_exact_unit_magnitude() {
        let item = ItemDelta {
            item_id: vec![0xAA; 32],
            kind: ItemKind::Object,
            magnitude: 2,
            negative: false,
        };
        let err = discrete_item_delta(&item).unwrap_err().to_string();
        assert!(err.contains("magnitude=2"));
        assert!(err.contains("expected 1"));
    }

    #[test]
    fn token_metadata_read_error_is_not_an_unnamed_token() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        let token_id = [0xAA; 32];
        store
            .put_cf(store.cf_tokens(), &token_id, b"corrupt-token-row")
            .unwrap();
        let mut cache = HashMap::new();
        let error = lookup_token_info(&store, &mut cache, &token_id)
            .unwrap_err()
            .to_string();
        assert!(error.contains(&hex::encode(token_id)), "{error}");
        assert!(
            cache.is_empty(),
            "failed reads must not poison the metadata cache"
        );
    }

    #[test]
    fn discrete_item_delta_preserves_sign() {
        let mut item = ItemDelta {
            item_id: vec![0xBB; 32],
            kind: ItemKind::Identity(ckbadger_store::types::IdentityStandard::DotCell),
            magnitude: 1,
            negative: false,
        };
        assert_eq!(discrete_item_delta(&item).unwrap(), 1);
        item.negative = true;
        assert_eq!(discrete_item_delta(&item).unwrap(), -1);
    }

    #[test]
    fn test_validate_activity_filter_rejects_unknown() {
        let err = validate_activity_filter(Some("tok")).unwrap_err();
        assert_eq!(err.0, axum::http::StatusCode::BAD_REQUEST);
        assert!(err.1 .0.message.contains("invalid activity filter"));
    }

    #[test]
    fn test_validate_activity_filter_accepts_protocol_prefix() {
        assert!(validate_activity_filter(Some("protocol:rgbpp")).is_ok());
        assert!(validate_activity_filter(Some("protocol:fiber")).is_ok());
        assert!(validate_activity_filter(Some("invalid")).is_err());
    }

    #[test]
    fn test_validate_global_activity_filter_only_accepts_all() {
        assert!(validate_global_activity_filter(None).is_ok());
        assert!(validate_global_activity_filter(Some("")).is_ok());
        assert!(validate_global_activity_filter(Some("all")).is_ok());
        assert!(validate_global_activity_filter(Some("token")).is_ok());
        assert!(validate_global_activity_filter(Some("object")).is_ok());
        assert!(validate_global_activity_filter(Some("identity")).is_ok());
        assert!(validate_global_activity_filter(Some("dao")).is_ok());
        assert!(validate_global_activity_filter(Some("script")).is_ok());
        assert!(validate_global_activity_filter(Some("protocol")).is_ok());
        assert!(validate_global_activity_filter(Some("nft")).is_err());
    }

    // Integration tests for activities are in crates/api/tests/api_activities.rs

    #[test]
    fn test_decode_rgbpp_lock_args_valid() {
        // out_index=2 (LE), btc_txid=32 bytes
        let mut args = vec![0; 36];
        args[0..4].copy_from_slice(&2u32.to_le_bytes());
        // btc_txid: stored little-endian, reversed for display
        for i in 0..32 {
            args[4 + i] = (32 - i) as u8;
        }
        let result = decode_rgbpp_lock_args(&args).unwrap();
        assert_eq!(result["protocol"], "rgbpp");
        assert_eq!(result["outIndex"], 2);
        // After reverse, should be 01020304...
        let txid = result["btcTxid"].as_str().unwrap();
        assert!(txid.starts_with("0102"));
    }

    #[test]
    fn test_decode_rgbpp_lock_args_too_short() {
        let args = vec![0; 35]; // 1 byte short
        assert!(decode_rgbpp_lock_args(&args).is_none());
    }

    #[test]
    fn test_decode_btc_time_lock_args_valid() {
        // btc_txid is the last 32 bytes (LE, reversed for display)
        let mut args = vec![0; 68];
        for i in 0..32 {
            args[36 + i] = (32 - i) as u8;
        }
        let result = decode_btc_time_lock_args(&args).unwrap();
        assert_eq!(result["protocol"], "rgbpp");
        assert_eq!(result["action"], "btcTimeLock");
        assert!(result.get("after").is_none());
        let txid = result["btcTxid"].as_str().unwrap();
        assert!(txid.starts_with("0102"));
    }

    #[test]
    fn test_decode_btc_time_lock_args_extracts_last_32_bytes() {
        // Verify "last 32 bytes" semantics with variable-length args
        let mut args = vec![0; 100];
        // Put txid in last 32 bytes (not at fixed offset 36..68)
        for i in 0..32 {
            args[68 + i] = (32 - i) as u8;
        }
        let result = decode_btc_time_lock_args(&args).unwrap();
        let txid = result["btcTxid"].as_str().unwrap();
        assert!(txid.starts_with("0102"));
    }

    #[test]
    fn test_decode_btc_time_lock_args_too_short() {
        let args = vec![0; 35]; // 1 byte short of minimum 36
        assert!(decode_btc_time_lock_args(&args).is_none());
    }

    #[test]
    fn test_decode_fiber_funding_lock_args_valid() {
        let mut args = vec![0u8; 20];
        for (i, byte) in args.iter_mut().enumerate() {
            *byte = (i + 1) as u8;
        }
        let result = decode_fiber_funding_lock_args(&args).unwrap();
        assert_eq!(result["protocol"], "fiber");
        assert_eq!(result["action"], "funding");
        let pubkey_hash = result["pubkeyHash"].as_str().unwrap();
        assert_eq!(pubkey_hash, "0x0102030405060708090a0b0c0d0e0f1011121314");
    }

    #[test]
    fn test_decode_fiber_funding_lock_args_longer_than_minimum() {
        // Funding lock args may be longer than 20 bytes; decoder uses first 20
        let mut args = vec![0xFFu8; 40];
        for (i, byte) in args[..20].iter_mut().enumerate() {
            *byte = (i + 1) as u8;
        }
        let result = decode_fiber_funding_lock_args(&args).unwrap();
        assert_eq!(result["protocol"], "fiber");
        let pubkey_hash = result["pubkeyHash"].as_str().unwrap();
        assert_eq!(pubkey_hash, "0x0102030405060708090a0b0c0d0e0f1011121314");
    }

    #[test]
    fn test_decode_fiber_funding_lock_args_too_short() {
        let args = vec![0; 19]; // 1 byte short of minimum 20
        assert!(decode_fiber_funding_lock_args(&args).is_none());
    }

    #[test]
    fn test_decode_fiber_commitment_lock_args_valid() {
        let mut args = vec![0u8; 57];
        // pubkey_hash [0..20]
        for (i, byte) in args[..20].iter_mut().enumerate() {
            *byte = (i + 1) as u8;
        }
        // delay_epoch [20..28] LE = 1000
        args[20..28].copy_from_slice(&1000u64.to_le_bytes());
        // version [28..36] BE = 1
        args[28..36].copy_from_slice(&1u64.to_be_bytes());
        // settlement_hash [36..56]
        for (i, byte) in args[36..56].iter_mut().enumerate() {
            *byte = (0xA0 + i) as u8;
        }
        // settlement_flag [56] = 1
        args[56] = 1;

        let result = decode_fiber_commitment_lock_args(&args).unwrap();
        assert_eq!(result["protocol"], "fiber");
        assert_eq!(result["action"], "commitment");
        assert_eq!(
            result["pubkeyHash"].as_str().unwrap(),
            "0x0102030405060708090a0b0c0d0e0f1011121314"
        );
        assert_eq!(result["delayEpoch"], 1000);
        assert_eq!(result["version"], 1);
        assert_eq!(
            result["settlementHash"].as_str().unwrap(),
            "0xa0a1a2a3a4a5a6a7a8a9aaabacadaeafb0b1b2b3"
        );
        assert_eq!(result["settlementFlag"], 1);
    }

    #[test]
    fn test_decode_fiber_commitment_lock_args_too_short() {
        let args = vec![0; 35]; // 1 byte short of minimum 36
        assert!(decode_fiber_commitment_lock_args(&args).is_none());
    }

    /// The decoder is the indexer's commitment-lock arbiter: lengths no
    /// deployed lock accepts are not commitment args (the old decoder read
    /// any >= 36 bytes), the 58-byte 0.7.0 layout reads like 57, and the
    /// daric 56-byte layout carries no settlement fields.
    #[test]
    fn test_decode_fiber_commitment_lock_args_follows_the_arbiter() {
        let mut head = vec![0x11u8; 20];
        head.extend_from_slice(&42u64.to_le_bytes());
        head.extend_from_slice(&3u64.to_be_bytes());
        let with = |tail: &[u8]| [head.as_slice(), tail].concat();
        for args in [
            with(&[0x22; 4]),
            with(&[0x22; 23]),
            with(&[[0x22; 20].as_slice(), &[0x01, 0x00]].concat()),
        ] {
            assert!(
                decode_fiber_commitment_lock_args(&args).is_none(),
                "{} bytes",
                args.len()
            );
        }
        let v1 = decode_fiber_commitment_lock_args(&with(
            &[[0x22; 20].as_slice(), &[0x01, 0x01]].concat(),
        ))
        .unwrap();
        assert_eq!(
            v1["settlementHash"],
            format!("0x{}", hex::encode([0x22; 20]))
        );
        assert_eq!(v1["settlementFlag"], 1);
        assert_eq!(v1["delayEpoch"], 42);
        let daric56 = decode_fiber_commitment_lock_args(&with(&[0x22; 20])).unwrap();
        assert_eq!(daric56["version"], 3);
        assert!(daric56.get("settlementHash").is_none());
        assert!(daric56.get("settlementFlag").is_none());
    }

    #[test]
    fn test_decode_fiber_commitment_lock_args_short_format() {
        // 36-byte short format: no settlement fields
        let mut args = vec![0u8; 36];
        args[0..20].copy_from_slice(&[0x11; 20]); // pubkey_hash
        args[20..28].copy_from_slice(&42u64.to_le_bytes()); // delay_epoch
        args[28..36].copy_from_slice(&3u64.to_be_bytes()); // version

        let result = decode_fiber_commitment_lock_args(&args).unwrap();
        assert_eq!(result["delayEpoch"], 42);
        assert_eq!(result["version"], 3);
        assert!(result.get("settlementHash").is_none());
        assert!(result.get("settlementFlag").is_none());
    }

    /// Every codec the scripts registry names is implemented, and each
    /// ordinary lock family's registry entry names its codec on every code
    /// hash its file lists (the hard-coded table this replaced held exactly
    /// these hashes).
    #[test]
    fn lock_args_codecs_follow_the_script_registry() {
        use ckbadger_indexer::parser::registry::{LockArgsLayout, SCRIPT_LAYOUTS};
        for (code_hash, script) in SCRIPT_LAYOUTS.iter() {
            if let Some(LockArgsLayout::Codec(codec)) = &script.lock_args {
                assert!(
                    lock_args_codec(codec).is_some(),
                    "{} (0x{}) names unknown codec {codec}",
                    script.slug,
                    hex::encode(code_hash)
                );
            }
        }
        let registry = |toml: &str| -> Vec<Vec<u8>> {
            toml.lines()
                .filter_map(|line| line.trim().strip_prefix("canonical_ref_hash = \""))
                .map(|rest| parse_hex_code_hash(rest.trim_end_matches('"')))
                .collect()
        };
        for (toml, codec) in [
            (
                include_str!("../../../../docs/metadata/scripts/default-lock.toml"),
                "secp256k1_blake160",
            ),
            (
                include_str!("../../../../docs/metadata/scripts/default-multisig.toml"),
                "multisig",
            ),
            (
                include_str!("../../../../docs/metadata/scripts/anyone-can-pay-lock.toml"),
                "anyone_can_pay",
            ),
            (
                include_str!("../../../../docs/metadata/scripts/omni-lock.toml"),
                "omnilock",
            ),
            (
                include_str!("../../../../docs/metadata/scripts/rgb.toml"),
                "rgbpp",
            ),
            (
                include_str!("../../../../docs/metadata/scripts/btc-time-lock.toml"),
                "btc_time_lock",
            ),
            (
                include_str!("../../../../docs/metadata/scripts/fiber-funding-lock.toml"),
                "fiber_funding",
            ),
            (
                include_str!("../../../../docs/metadata/scripts/fiber-commitment-lock.toml"),
                "fiber_commitment",
            ),
            (
                include_str!("../../../../docs/metadata/scripts/utxoswap-intent-lock.toml"),
                "utxoswap_intent",
            ),
        ] {
            let hashes = registry(toml);
            assert!(!hashes.is_empty(), "{codec}");
            for hash in &hashes {
                assert_eq!(
                    SCRIPT_LAYOUTS
                        .get(hash)
                        .and_then(|script| script.lock_args.clone()),
                    Some(LockArgsLayout::Codec(codec.into())),
                    "{codec}: 0x{}",
                    hex::encode(hash)
                );
            }
        }
    }

    /// R-8: a registry field layout decodes exactly its total length; args
    /// that do not fit — a field layout or a codec — are a stated reason
    /// naming the script, never an `Err` (the ruling of 2026-10-31).
    #[test]
    fn registry_layouts_decode_exactly_or_state_why_not() {
        let joyid = parse_hex_code_hash(
            "0xd00c84f0ec8fd441c38bc3f87a371f547190f2fcff88e642bc5bf54b9e318323",
        );
        let args = [[0x00u8, 0x01].as_slice(), &[0xab; 20]].concat();
        assert_eq!(
            decode_lock_args(&joyid, &args).unwrap(),
            DecodedLockArgs {
                decoded: Some(serde_json::json!({
                    "lock": "joyid",
                    "subType": "0x0001",
                    "pubkeyHash": format!("0x{}", "ab".repeat(20)),
                })),
                error: None,
            }
        );
        assert_eq!(
            decode_lock_args(&joyid, &[0; 23]).unwrap(),
            DecodedLockArgs {
                decoded: None,
                error: Some(
                    "JoyID: args are 23 bytes, layout subType(2) + pubkeyHash(20) expects 22"
                        .into()
                ),
            }
        );
        let shadow = parse_hex_code_hash(
            "0x6361d4b20d845953d9c9431bbba08905573005a71e2a2432e7e0e7c685666f24",
        );
        for prefix in [0x00u8, 0x01, 0x07] {
            let read =
                decode_lock_args(&shadow, &[[prefix].as_slice(), &[1; 32]].concat()).unwrap();
            assert_eq!(read.decoded.unwrap()["prefix"], format!("0x{prefix:02x}"));
        }
        let default_lock = parse_hex_code_hash(
            "0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8",
        );
        assert_eq!(
            decode_lock_args(&default_lock, &[0; 21])
                .unwrap()
                .error
                .as_deref(),
            Some("Default Lock: args are 21 bytes, layout secp256k1_blake160 expects 20 bytes")
        );
        // A fixed field (`equals`) states the value it found.
        let fixed = [ckbadger_indexer::parser::registry::LockArgsField {
            name: "prefix".into(),
            bytes: 1,
            equals: Some(vec![0x01]),
        }];
        assert_eq!(
            decode_lock_args_fields("Fixture Lock", "fixture", &fixed, &[0x07]).unwrap_err(),
            "Fixture Lock: field prefix: expected 0x01, got 0x07"
        );
        // A lock no registry entry lays out has neither a value nor a reason.
        assert_eq!(
            decode_lock_args(&[0x99; 32], &[1, 2, 3]).unwrap(),
            DecodedLockArgs::default()
        );
    }

    #[test]
    fn ordinary_lock_args_decode_by_their_specified_layouts() {
        assert_eq!(
            decode_multisig_args(&[[7u8; 20].as_slice(), &5u64.to_le_bytes()].concat()).unwrap()
                ["since"],
            5
        );
        assert!(decode_multisig_args(&[7u8; 21]).is_none());
        let acp = decode_anyone_can_pay_args(&[[1u8; 20].as_slice(), &[8, 2]].concat()).unwrap();
        assert_eq!(acp["minimumCkbExponent"], 8);
        assert_eq!(acp["minimumUdtExponent"], 2);
        assert!(decode_anyone_can_pay_args(&[1u8; 23]).is_none());
        // Omnilock: the flags must account for every byte after the auth.
        let plain = [[0u8].as_slice(), &[9u8; 20], &[0]].concat();
        assert_eq!(decode_omnilock_args(&plain).unwrap()["omnilockFlags"], 0);
        assert!(decode_omnilock_args(&[plain.as_slice(), &[1]].concat()).is_none());
        let acp_mode = [[0x06u8].as_slice(), &[9u8; 20], &[0x2], &[3, 4]].concat();
        let decoded = decode_omnilock_args(&acp_mode).unwrap();
        assert_eq!(decoded["authKind"], "ckb_multisig");
        assert_eq!(decoded["minimumUdtExponent"], 4);
        assert!(decode_secp256k1_blake160_args(&[0u8; 21]).is_none());
    }

    #[test]
    fn test_fiber_locks_have_decoders() {
        // The Fiber lock code hashes decode through their registry codecs.
        let funding_mainnet = parse_hex_code_hash(
            "0xe45b1f8f21bff23137035a3ab751d75b36a981deec3e7820194b9c042967f4f1",
        );
        let commitment_mainnet = parse_hex_code_hash(
            "0x2d45c4d3ed3e942f1945386ee82a5d1b7e4bb16d7fe1ab015421174ab747406c",
        );
        assert_eq!(
            decode_lock_args(&funding_mainnet, &[7; 20])
                .unwrap()
                .decoded
                .unwrap()["action"],
            "funding"
        );
        let commitment = [
            [7u8; 20].as_slice(),
            &42u64.to_le_bytes(),
            &3u64.to_be_bytes(),
        ]
        .concat();
        assert!(decode_lock_args(&commitment_mainnet, &commitment)
            .unwrap()
            .decoded
            .is_some());
    }

    /// Real mainnet swap intent args (tx 0x44f659be…, block 13,845,652).
    const REAL_SWAP_INTENT_ARGS: &str = "0xbefc0a6053441e9bcba6d3f6c1599c37a1d8187a235edb927fc68f446e06f2e677fb52aa7f158ae800000000000000000000000000000000030180ea822b000000000000000000000000324f4251220000000000000000000000";

    /// Real mainnet AddLiquidity intent args (tx 0x18d1b37e…, block 14,046,271).
    const REAL_ADD_LIQUIDITY_INTENT_ARGS: &str = "0x0001d85947f67df16556a1caef3b7f939a69fb2329273406698f36e9bdf46db404176859b0ba3a6b00000000000000000000000000000000013a219800000000000000000000000000805e9700000000000000000000000000506c0300000000000000000000000000ee670300000000000000000000000000";

    /// Real mainnet RemoveLiquidity intent args (tx 0x416ed0a3…, block 20,003,047).
    const REAL_REMOVE_LIQUIDITY_INTENT_ARGS: &str = "0xc41696293f5b16b471f9116631da82a4102c5b01b82e9073fee07b9caf625f0be45d3ec061be221200000000000000000100000000000000025b4ff3776d2f00000000000000000000b7d95acc3505000000000000000000001b35f86b53d501000000000000000000";

    #[test]
    fn test_decode_utxoswap_intent_swap() {
        let args = parse_hex_code_hash(REAL_SWAP_INTENT_ARGS);
        let result = decode_utxoswap_intent_args(&args).unwrap();
        assert_eq!(result["protocol"], "utxoswap");
        assert_eq!(result["intentType"], "SwapExactInputForOutput");
        assert_eq!(
            result["poolTypeHash"],
            "0x235edb927fc68f446e06f2e677fb52aa7f158ae8"
        );
        assert_eq!(result["assetInIndex"], 1);
        assert_eq!(result["amountIn"], "730000000");
        assert_eq!(result["amountOutMin"], "147392188210");
        assert!(result.get("assetX").is_none());
    }

    /// The API's live display used to re-implement the decode and carried the
    /// identical bug: the 90-byte swap layout applied to a 121-byte
    /// AddLiquidity payload, yielding 2^127-scale amounts. It now shares the
    /// one decoder in `ckbadger-common`.
    #[test]
    fn test_decode_utxoswap_intent_add_liquidity() {
        let args = parse_hex_code_hash(REAL_ADD_LIQUIDITY_INTENT_ARGS);
        let result = decode_utxoswap_intent_args(&args).unwrap();
        assert_eq!(result["protocol"], "utxoswap");
        assert_eq!(result["intentType"], "AddLiquidity");
        assert_eq!(result["desiredX"], "9969978");
        assert_eq!(result["minX"], "9920128");
        assert_eq!(result["desiredY"], "224336");
        assert_eq!(result["minY"], "223214");

        assert!(result.get("amountIn").is_none());
        assert!(result.get("amountOutMin").is_none());
        assert!(result.get("assetInIndex").is_none());
        assert!(
            !result
                .to_string()
                .contains("170141183460469231731687303715884144673"),
            "old 2^127-scale garbage resurfaced: {result}"
        );
    }

    #[test]
    fn test_decode_utxoswap_intent_remove_liquidity() {
        let args = parse_hex_code_hash(REAL_REMOVE_LIQUIDITY_INTENT_ARGS);
        let result = decode_utxoswap_intent_args(&args).unwrap();
        assert_eq!(result["intentType"], "RemoveLiquidity");
        assert_eq!(result["lpAmount"], "52147210375003");
        assert_eq!(result["minX"], "5728619911607");
        assert_eq!(result["minY"], "516029247141147");
        assert!(result.get("amountIn").is_none());
    }

    #[test]
    fn test_decode_utxoswap_intent_create_pool() {
        // Real mainnet CreatePool intent args (tx 0xdd8b76a8…, block 13,372,041).
        let args = parse_hex_code_hash("0x4d93a976fd4eb7a6349c020fadc3ef65834701dc000000000000000000000000000000000000000000000000000000000100000000000000001e000000000000000000000000000000000000000000000000000000000000000061bd91b121e5b7bbf9ccb4bc46c3106ac69c2dfd7b1c1143c4b4fdb33fd6182600e40b5402000000000000000000000000e40b54020000000000000000000000");

        let result = decode_utxoswap_intent_args(&args).unwrap();
        assert_eq!(result["protocol"], "utxoswap");
        assert_eq!(result["intentType"], "CreatePool");
        assert_eq!(result["totalFeeRate"], 30);
        assert_eq!(result["amountX"], "10000000000");
        assert_eq!(result["amountY"], "10000000000");
        assert_eq!(
            result["assetY"],
            "0x61bd91b121e5b7bbf9ccb4bc46c3106ac69c2dfd7b1c1143c4b4fdb33fd61826"
        );
        // CreatePool no longer carries bogus zero-valued swap fields.
        assert!(result.get("amountIn").is_none());
        assert!(result.get("assetInIndex").is_none());
    }

    #[test]
    fn test_decode_utxoswap_intent_too_short() {
        // Below the 57-byte shared header there is nothing trustworthy to read.
        let args = vec![0u8; 56];
        assert!(decode_utxoswap_intent_args(&args).is_none());
    }

    /// A recognised type with an unexpected payload length must be reported as
    /// unparsed rather than decoded with some other type's layout.
    #[test]
    fn test_decode_utxoswap_intent_unexpected_length_is_unparsed() {
        let mut args = parse_hex_code_hash(REAL_SWAP_INTENT_ARGS);
        args[56] = 1; // claim AddLiquidity on a 90-byte payload
        let result = decode_utxoswap_intent_args(&args).unwrap();
        assert_eq!(result["intentType"], "AddLiquidity");
        assert_eq!(result["payloadUnparsed"], true);
        assert_eq!(result["argsLen"], 90);
        assert!(result.get("amountIn").is_none());
        assert!(result.get("desiredX").is_none());
    }

    #[test]
    fn test_utxoswap_intent_locks_have_decoders() {
        let mainnet = parse_hex_code_hash(
            "0x3547c9aa563804e47ba3ebd37e6012e447c91a238f7aa71b1a75319f11df060e",
        );
        let testnet = parse_hex_code_hash(
            "0x4e9c30c8d6ce275740fbe69eae49c3d8c213578c5bd066f4938fe3c7dec6e101",
        );
        let args = parse_hex_code_hash(REAL_SWAP_INTENT_ARGS);
        for code_hash in [mainnet, testnet] {
            assert!(decode_lock_args(&code_hash, &args)
                .unwrap()
                .decoded
                .is_some());
        }
    }

    #[test]
    fn test_item_delta_response_json_format() {
        let token = ItemDeltaResponse::Token {
            type_script_hash: "0xabcdef1234".to_string(),
            delta: "100".to_string(),
            symbol: None,
            decimals: None,
        };
        let json = serde_json::to_string(&token).unwrap();
        assert!(
            json.contains("\"typeScriptHash\""),
            "missing typeScriptHash: {}",
            json
        );
        assert!(
            json.contains("\"kind\":\"token\""),
            "missing kind: {}",
            json
        );

        let object = ItemDeltaResponse::Object {
            object_id: "0xabcdef".to_string(),
            delta: 1,
            collection: None,
            class: "object",
            composition: None,
            role: None,
        };
        let json = serde_json::to_string(&object).unwrap();
        assert!(json.contains("\"objectId\""), "missing objectId: {}", json);
        assert!(
            json.contains("\"kind\":\"object\""),
            "missing kind: {}",
            json
        );

        let identity = ItemDeltaResponse::Identity {
            identity_id: "0xabcdef".to_string(),
            delta: -1,
            standard: ckbadger_store::types::IdentityStandard::DotCell.asset_standard(),
        };
        let json = serde_json::to_string(&identity).unwrap();
        assert!(
            json.contains("\"identityId\""),
            "missing identityId: {}",
            json
        );
        assert!(
            json.contains("\"standard\":\"dotcell\""),
            "missing standard: {}",
            json
        );
        assert!(
            json.contains("\"kind\":\"identity\""),
            "missing kind: {}",
            json
        );

        // A token delta > i128::MAX must render as its exact decimal string. This is the
        // exact expression convert_item_delta uses for a token (`format!("{sign}{magnitude}")`),
        // proving no truncation at the API/JSON boundary. (The indexer-side regression test
        // pins that `magnitude` reaches here un-wrapped.)
        let magnitude: u128 = 222_044_604_925_031_325_468_940_491_728_862_838_784; // 2.22e38
        let negative = false;
        let big_token = ItemDeltaResponse::Token {
            type_script_hash: "0xdd".to_string(),
            delta: format!("{}{}", if negative { "-" } else { "" }, magnitude),
            symbol: None,
            decimals: None,
        };
        let json = serde_json::to_string(&big_token).unwrap();
        assert!(
            json.contains("\"222044604925031325468940491728862838784\""),
            "big token delta must serialize as exact decimal string: {}",
            json
        );
    }
}
