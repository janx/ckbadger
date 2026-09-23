//! `.cell` lifecycle classification.
//!
//! One state-diff classifier over the name cells a transaction consumes and
//! creates. It is the ONLY place a `.cell` transaction is interpreted: the
//! Layer-3 `dotcell:*` actions, the named participants (spec §3), the
//! collection `AssetAction` and the per-item feed all come out of the same
//! `Vec<DotCellTransition>`, so the two sync paths cannot disagree.
//!
//! Ownership is a 20-byte lock-hash prefix in the cell's data, never the
//! cell's lock: every name cell carries the same Account Lock. A name's
//! Layer-2 item follows `owner20` uniformly — including while it is listed,
//! when the chain says a Sale Lock script instance owns it.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use anyhow::{anyhow, bail, Result};
use ckbadger_store::types::{
    participant_roles::{MANAGER_TO, OWNER_FROM, OWNER_TO},
    AssetAction, ItemDelta, ObjectCollectionActivityEntry, ParticipantId, ProtocolAction,
    ITEM_KIND_IDENTITY,
};

use crate::parser::dotcell::{DotCellNameData, DotCellParser};
use crate::parser::registry::{ProtocolScript, PROTOCOL_REGISTRY};

use ckbadger_store::types::{LockCallEntry, TypeCallEntry};

use super::activities::{NamedParticipant, OwnerAccum, ProtocolDetector, TxView};

/// What happened to one `.cell` name in one transaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DotCellTransitionKind {
    Register {
        owner20: [u8; 20],
        manager20: [u8; 20],
        expired_at: u64,
        parent_id: Option<[u8; 20]>,
    },
    /// The empty-label root of a namespace's uniqueness ring: protocol
    /// infrastructure, not an identity.
    RingRoot,
    List {
        from_owner20: [u8; 20],
        sale_hash20: [u8; 20],
        seller32: [u8; 32],
        price: u64,
    },
    CancelSale {
        sale_hash20: [u8; 20],
        seller32: [u8; 32],
        price: u64,
        to_owner20: [u8; 20],
    },
    Buy {
        sale_hash20: [u8; 20],
        seller32: [u8; 32],
        price: u64,
        buyer20: [u8; 20],
    },
    Transfer {
        from20: [u8; 20],
        to20: [u8; 20],
    },
    Renew {
        from: u64,
        to: u64,
    },
    EditRecords,
    EditManager {
        from20: [u8; 20],
        to20: [u8; 20],
    },
    Recycle {
        owner20: [u8; 20],
    },
    /// Re-created byte-identical.
    Touch,
    /// A ring neighbour whose only change is its `next` pointer. Registration
    /// infrastructure, suppressed everywhere.
    RingLink,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DotCellTransition {
    pub(crate) id: [u8; 20],
    pub(crate) label: String,
    pub(crate) kind: DotCellTransitionKind,
    /// Every field that differs, in a fixed order. The transition kind is the
    /// primary fact; this is the whole truth.
    pub(crate) changes: Vec<&'static str>,
    /// The manager this transaction handed the name to, if it set one.
    pub(crate) manager_changed_to: Option<[u8; 20]>,
}

fn hex0x(bytes: &[u8]) -> String {
    format!("0x{}", hex::encode(bytes))
}

/// Every Sale Lock script instance this transaction touches, by the first 20
/// bytes of its own script hash — which is exactly what a listed name stores
/// as its owner. Offer cells appear as outputs when listing and as inputs when
/// buying or cancelling, so both sides are scanned.
fn sale_instances(tx: &TxView<'_>) -> Result<HashMap<[u8; 20], ([u8; 32], u64)>> {
    let mut instances = HashMap::new();
    let cells = tx
        .inputs
        .iter()
        .map(|input| {
            (
                input.lock_code_hash,
                input.lock_args,
                input.lock_script_hash,
            )
        })
        .chain(tx.outputs.iter().map(|output| {
            (
                output.lock_code_hash,
                output.lock_args,
                output.lock_script_hash,
            )
        }));
    for (code_hash, args, script_hash) in cells {
        if !DotCellParser::is_sale_lock(code_hash) {
            continue;
        }
        let (seller, price) = DotCellParser::parse_sale_lock_args(args).map_err(|e| {
            anyhow!(
                "tx 0x{}: sale lock script hash 0x{}: {e}",
                hex::encode(tx.tx_hash),
                hex::encode(script_hash)
            )
        })?;
        if script_hash.len() < 20 {
            bail!(
                "tx 0x{}: sale lock script hash is {} bytes",
                hex::encode(tx.tx_hash),
                script_hash.len()
            );
        }
        let mut hash20 = [0u8; 20];
        hash20.copy_from_slice(&script_hash[..20]);
        instances.insert(hash20, (seller, price));
    }
    Ok(instances)
}

/// Diff the `.cell` names on both sides of a transaction.
pub(crate) fn classify_dotcell_transitions(tx: &TxView<'_>) -> Result<Vec<DotCellTransition>> {
    let mut prev: BTreeMap<[u8; 20], &DotCellNameData> = BTreeMap::new();
    for input in &tx.inputs {
        if let Some(name) = input.dotcell {
            if prev.insert(name.id, name).is_some() {
                bail!(
                    "dotcell id 0x{} consumed twice in tx 0x{}",
                    hex::encode(name.id),
                    hex::encode(tx.tx_hash)
                );
            }
        }
    }
    let mut next: BTreeMap<[u8; 20], DotCellNameData> = BTreeMap::new();
    for output in &tx.outputs {
        if !output
            .type_code_hash
            .is_some_and(DotCellParser::is_account_type_script)
        {
            continue;
        }
        let name = DotCellParser::parse_name_data(output.data)
            .map_err(|e| anyhow!("tx 0x{}: {e}", hex::encode(tx.tx_hash)))?;
        let id = name.id;
        if next.insert(id, name).is_some() {
            bail!(
                "dotcell id 0x{} created twice in tx 0x{}",
                hex::encode(id),
                hex::encode(tx.tx_hash)
            );
        }
    }
    if prev.is_empty() && next.is_empty() {
        return Ok(Vec::new());
    }

    let sales = sale_instances(tx)?;
    let ids: BTreeSet<[u8; 20]> = prev.keys().chain(next.keys()).copied().collect();
    let mut out = Vec::with_capacity(ids.len());
    for id in ids {
        let transition = match (prev.get(&id), next.get(&id)) {
            (None, Some(name)) if name.is_root() => DotCellTransition {
                id,
                label: String::new(),
                kind: DotCellTransitionKind::RingRoot,
                changes: Vec::new(),
                manager_changed_to: None,
            },
            (None, Some(name)) => DotCellTransition {
                id,
                label: name.label.clone(),
                kind: DotCellTransitionKind::Register {
                    owner20: name.owner_hash20,
                    manager20: name.manager_hash20,
                    expired_at: name.expired_at,
                    parent_id: name.parent_id(),
                },
                changes: vec!["created"],
                // A registration assigns the manager, so the manager gained a
                // right here even when it is the owner themselves.
                manager_changed_to: Some(name.manager_hash20),
            },
            (Some(previous), None) => DotCellTransition {
                id,
                label: previous.label.clone(),
                kind: DotCellTransitionKind::Recycle {
                    owner20: previous.owner_hash20,
                },
                changes: vec!["removed"],
                manager_changed_to: None,
            },
            (Some(previous), Some(name)) => {
                if previous.label != name.label {
                    bail!(
                        "dotcell id 0x{} changed label from {:?} to {:?} in tx 0x{} — the id is the label's hash, so this cannot happen",
                        hex::encode(id),
                        previous.label,
                        name.label,
                        hex::encode(tx.tx_hash)
                    );
                }
                let mut changes: Vec<&'static str> = Vec::new();
                if previous.owner_hash20 != name.owner_hash20 {
                    changes.push("owner");
                }
                if previous.manager_hash20 != name.manager_hash20 {
                    changes.push("manager");
                }
                if previous.expired_at != name.expired_at {
                    if name.expired_at < previous.expired_at {
                        bail!(
                            "dotcell expiry decreased for 0x{} in tx 0x{}: {} -> {}",
                            hex::encode(id),
                            hex::encode(tx.tx_hash),
                            previous.expired_at,
                            name.expired_at
                        );
                    }
                    changes.push("expiry");
                }
                if previous.records_hash != name.records_hash {
                    changes.push("records");
                }
                if previous.next_id != name.next_id {
                    changes.push("next");
                }

                let kind = if !changes.is_empty() && changes.iter().all(|c| *c == "next") {
                    DotCellTransitionKind::RingLink
                } else if previous.owner_hash20 != name.owner_hash20 {
                    if let Some((seller32, price)) = sales.get(&name.owner_hash20) {
                        DotCellTransitionKind::List {
                            from_owner20: previous.owner_hash20,
                            sale_hash20: name.owner_hash20,
                            seller32: *seller32,
                            price: *price,
                        }
                    } else if let Some((seller32, price)) = sales.get(&previous.owner_hash20) {
                        if name.owner_hash20[..] == seller32[..20] {
                            DotCellTransitionKind::CancelSale {
                                sale_hash20: previous.owner_hash20,
                                seller32: *seller32,
                                price: *price,
                                to_owner20: name.owner_hash20,
                            }
                        } else {
                            DotCellTransitionKind::Buy {
                                sale_hash20: previous.owner_hash20,
                                seller32: *seller32,
                                price: *price,
                                buyer20: name.owner_hash20,
                            }
                        }
                    } else {
                        DotCellTransitionKind::Transfer {
                            from20: previous.owner_hash20,
                            to20: name.owner_hash20,
                        }
                    }
                } else if previous.expired_at != name.expired_at {
                    DotCellTransitionKind::Renew {
                        from: previous.expired_at,
                        to: name.expired_at,
                    }
                } else if previous.records_hash != name.records_hash {
                    DotCellTransitionKind::EditRecords
                } else if previous.manager_hash20 != name.manager_hash20 {
                    DotCellTransitionKind::EditManager {
                        from20: previous.manager_hash20,
                        to20: name.manager_hash20,
                    }
                } else {
                    DotCellTransitionKind::Touch
                };

                DotCellTransition {
                    id,
                    label: name.label.clone(),
                    kind,
                    manager_changed_to: (previous.manager_hash20 != name.manager_hash20)
                        .then_some(name.manager_hash20),
                    changes,
                }
            }
            (None, None) => unreachable!("id came from one of the two maps"),
        };
        out.push(transition);
    }
    Ok(out)
}

/// The `dotcell:*` action name, or `None` for ring infrastructure.
pub(crate) fn action_name(kind: &DotCellTransitionKind) -> Option<&'static str> {
    use DotCellTransitionKind::*;
    Some(match kind {
        RingRoot | RingLink => return None,
        Register {
            parent_id: Some(_), ..
        } => "register_subname",
        Register { .. } => "register",
        List { .. } => "list",
        CancelSale { .. } => "cancel_sale",
        Buy { .. } => "buy",
        Transfer { .. } => "transfer",
        Renew { .. } => "renew",
        EditRecords => "edit_records",
        EditManager { .. } => "edit_manager",
        Recycle { .. } => "recycle",
        Touch => "touch",
    })
}

/// The collection-level asset action one `dotcell:*` action means. Both sync
/// paths derive the collection feed from the SAME already-written
/// `protocol_actions` through this function.
///
/// The action strings are a closed set `action_name` produces, so an unknown
/// one is an invariant violation — a transition kind that gained no mapping —
/// and must stop the batch rather than quietly cost the collection one feed
/// entry.
pub(crate) fn dotcell_asset_action(action: &str) -> Result<AssetAction> {
    Ok(match action {
        "register" | "register_subname" => AssetAction::Mint,
        "transfer" | "buy" => AssetAction::Transfer,
        "renew" => AssetAction::Renew,
        "list" | "cancel_sale" | "edit_records" | "edit_manager" | "touch" => AssetAction::Update,
        "recycle" => AssetAction::Recycle,
        other => bail!("unknown dotcell action {other:?}: no collection AssetAction maps to it"),
    })
}

pub(crate) fn protocol_actions_for(transitions: &[DotCellTransition]) -> Vec<ProtocolAction> {
    use DotCellTransitionKind::*;
    transitions
        .iter()
        .filter_map(|t| {
            let action = action_name(&t.kind)?;
            let mut meta = serde_json::json!({
                "label": t.label,
                "name": format!("{}.cell", t.label),
                "id": hex0x(&t.id),
                "changes": t.changes,
            });
            let fields = meta.as_object_mut().expect("json object");
            match &t.kind {
                Register {
                    owner20,
                    manager20,
                    expired_at,
                    parent_id,
                } => {
                    fields.insert("to".into(), hex0x(owner20).into());
                    fields.insert("manager".into(), hex0x(manager20).into());
                    fields.insert("expiry".into(), (*expired_at).into());
                    if let Some(parent) = parent_id {
                        fields.insert("parentId".into(), hex0x(parent).into());
                    }
                }
                List {
                    from_owner20,
                    sale_hash20,
                    seller32,
                    price,
                } => {
                    fields.insert("from".into(), hex0x(from_owner20).into());
                    fields.insert("to".into(), hex0x(sale_hash20).into());
                    fields.insert("seller".into(), hex0x(seller32).into());
                    fields.insert("price".into(), price.to_string().into());
                }
                CancelSale {
                    sale_hash20,
                    seller32,
                    price,
                    to_owner20,
                } => {
                    fields.insert("from".into(), hex0x(sale_hash20).into());
                    fields.insert("to".into(), hex0x(to_owner20).into());
                    fields.insert("seller".into(), hex0x(seller32).into());
                    fields.insert("price".into(), price.to_string().into());
                }
                Buy {
                    sale_hash20,
                    seller32,
                    price,
                    buyer20,
                } => {
                    fields.insert("from".into(), hex0x(sale_hash20).into());
                    fields.insert("to".into(), hex0x(buyer20).into());
                    fields.insert("buyer".into(), hex0x(buyer20).into());
                    fields.insert("seller".into(), hex0x(seller32).into());
                    fields.insert("price".into(), price.to_string().into());
                }
                Transfer { from20, to20 } => {
                    fields.insert("from".into(), hex0x(from20).into());
                    fields.insert("to".into(), hex0x(to20).into());
                }
                Renew { from, to } => {
                    fields.insert("expiryFrom".into(), (*from).into());
                    fields.insert("expiryTo".into(), (*to).into());
                }
                EditManager { from20, to20 } => {
                    fields.insert("managerFrom".into(), hex0x(from20).into());
                    fields.insert("managerTo".into(), hex0x(to20).into());
                }
                Recycle { owner20 } => {
                    fields.insert("from".into(), hex0x(owner20).into());
                }
                EditRecords | Touch => {}
                RingRoot | RingLink => unreachable!("filtered out by action_name"),
            }
            Some(ProtocolAction::new("dotcell", action, meta))
        })
        .collect()
}

/// The parties this transaction affects, as the protocol names them in cell
/// data. The builder merges these by id, so one party named twice (owner and
/// manager of the same name) ends up with both roles.
pub(crate) fn named_participants_for(transitions: &[DotCellTransition]) -> Vec<NamedParticipant> {
    use DotCellTransitionKind::*;

    fn delta(id: &[u8; 20], negative: bool) -> ItemDelta {
        ItemDelta {
            item_id: id.to_vec(),
            kind: ITEM_KIND_IDENTITY,
            magnitude: 1,
            negative,
        }
    }

    let mut out: Vec<NamedParticipant> = Vec::new();
    for t in transitions {
        let mut push = |prefix: [u8; 20], item_deltas: Vec<ItemDelta>, roles: u8| {
            out.push(NamedParticipant {
                id: ParticipantId::LockPrefix(prefix),
                item_deltas,
                roles,
            })
        };
        match &t.kind {
            Register { owner20, .. } => push(*owner20, vec![delta(&t.id, false)], OWNER_TO),
            Transfer { from20, to20 } => {
                push(*from20, vec![delta(&t.id, true)], OWNER_FROM);
                push(*to20, vec![delta(&t.id, false)], OWNER_TO);
            }
            List {
                from_owner20,
                sale_hash20,
                ..
            } => {
                push(*from_owner20, vec![delta(&t.id, true)], OWNER_FROM);
                push(*sale_hash20, vec![delta(&t.id, false)], OWNER_TO);
            }
            CancelSale {
                sale_hash20,
                to_owner20,
                ..
            } => {
                push(*sale_hash20, vec![delta(&t.id, true)], OWNER_FROM);
                push(*to_owner20, vec![delta(&t.id, false)], OWNER_TO);
            }
            Buy {
                sale_hash20,
                buyer20,
                ..
            } => {
                push(*sale_hash20, vec![delta(&t.id, true)], OWNER_FROM);
                push(*buyer20, vec![delta(&t.id, false)], OWNER_TO);
            }
            Recycle { owner20 } => push(*owner20, vec![delta(&t.id, true)], OWNER_FROM),
            RingRoot | RingLink | Renew { .. } | EditRecords | EditManager { .. } | Touch => {}
        }
        if let Some(manager) = t.manager_changed_to {
            push(manager, Vec::new(), MANAGER_TO);
        }
    }
    out
}

/// The `.cell` collection feed entry for one transaction, derived from the
/// `dotcell:*` actions already written to `CF_TX_ACTIONS`. Both sync paths call
/// this with the same input, so the feed cannot diverge between them.
pub(crate) fn build_dotcell_tx_activity_entry(
    actions: &[ProtocolAction],
    tx_hash: &[u8],
    block_hash: &[u8],
    timestamp_ms: i64,
) -> Result<Option<ObjectCollectionActivityEntry>> {
    let mut asset_actions: Vec<AssetAction> = Vec::new();
    for action in actions.iter().filter(|a| a.protocol == "dotcell") {
        let asset_action = dotcell_asset_action(&action.action)
            .map_err(|e| anyhow!("tx 0x{}: {e}", hex::encode(tx_hash)))?;
        if !asset_actions.contains(&asset_action) {
            asset_actions.push(asset_action);
        }
    }
    Ok(
        (!asset_actions.is_empty()).then(|| ObjectCollectionActivityEntry {
            tx_hash: tx_hash.to_vec(),
            block_hash: block_hash.to_vec(),
            timestamp_ms,
            actions: asset_actions,
        }),
    )
}

pub(crate) struct DotCellDetector {
    account_code_hashes: HashSet<[u8; 32]>,
}

impl DotCellDetector {
    pub(crate) fn new() -> Self {
        let mut account_code_hashes = HashSet::new();
        for (code_hash, protocol) in PROTOCOL_REGISTRY.iter() {
            if protocol != ProtocolScript::DotCellAccount {
                continue;
            }
            let hash: [u8; 32] = code_hash.as_slice().try_into().unwrap_or_else(|_| {
                panic!(
                    "Cells Account code hash is not 32 bytes: 0x{}",
                    hex::encode(code_hash)
                )
            });
            account_code_hashes.insert(hash);
        }
        Self {
            account_code_hashes,
        }
    }
}

impl ProtocolDetector for DotCellDetector {
    fn might_apply_batch(
        &self,
        _lock_code_hashes: &HashSet<[u8; 32]>,
        type_code_hashes: &HashSet<[u8; 32]>,
    ) -> bool {
        type_code_hashes
            .iter()
            .any(|hash| self.account_code_hashes.contains(hash))
    }

    fn might_apply(&self, tx: &TxView<'_>) -> bool {
        tx.inputs.iter().any(|input| input.dotcell.is_some())
            || tx.outputs.iter().any(|output| {
                output
                    .type_code_hash
                    .is_some_and(DotCellParser::is_account_type_script)
            })
    }

    fn detect(
        &self,
        tx: &TxView<'_>,
        _owner_lock_hash: &[u8],
        _accum: &OwnerAccum<'_>,
        _item_deltas: &[ItemDelta],
        _type_calls: &[TypeCallEntry],
        _lock_calls: &[LockCallEntry],
    ) -> Result<Vec<ProtocolAction>> {
        // Tx-level: the builder asks once per participating owner and dedups
        // by (protocol, action, metadata). Each action's metadata carries the
        // name id, so two names in one tx never collapse into one.
        Ok(protocol_actions_for(&classify_dotcell_transitions(tx)?))
    }

    fn name_participants(&self, tx: &TxView<'_>) -> Result<Vec<NamedParticipant>> {
        Ok(named_participants_for(&classify_dotcell_transitions(tx)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::writer::activities::{
        build_tx_actions_for_block_with_io, InputCellView, OutputCellView, TxView,
    };
    use crate::parser::dotcell_fixtures::{CellFixture, TxFixture};
    use crate::parser::test_helpers::real_dotcell as fixture;
    use crate::parser::{DotCellNameData, DotCellParser, ScriptParser};
    use crate::rpc::parse_hex_to_bytes;
    use ckbadger_store::types::{
        participant_roles::{MANAGER_TO, OWNER_FROM, OWNER_TO},
        AssetAction, ItemDelta, ParticipantId, ITEM_KIND_IDENTITY,
    };

    // ── Fixture → TxView bridge ────────────────────────────────────────────

    struct OwnedCell {
        capacity: i64,
        lock_script_hash: Vec<u8>,
        lock_code_hash: Vec<u8>,
        lock_args: Vec<u8>,
        type_code_hash: Option<Vec<u8>>,
        type_args: Option<Vec<u8>>,
        type_script_hash: Option<Vec<u8>>,
        data: Vec<u8>,
        /// For inputs: the name this cell carried before it was spent.
        dotcell: Option<DotCellNameData>,
    }

    impl OwnedCell {
        fn from_fixture(cell: &CellFixture) -> Self {
            let lock = cell.lock_script();
            let type_script = cell.type_script();
            let data = parse_hex_to_bytes(cell.data);
            let dotcell = DotCellParser::parse_name_cell(&cell.output(), cell.data)
                .expect("fixture name cell must decode");
            Self {
                capacity: i64::from_str_radix(cell.capacity.trim_start_matches("0x"), 16)
                    .expect("hex capacity"),
                lock_script_hash: ScriptParser::compute_script_hash(&lock),
                lock_code_hash: parse_hex_to_bytes(cell.lock_code_hash),
                lock_args: parse_hex_to_bytes(cell.lock_args),
                type_code_hash: cell.type_code_hash.map(parse_hex_to_bytes),
                type_args: cell.type_args.map(parse_hex_to_bytes),
                type_script_hash: type_script.as_ref().map(ScriptParser::compute_script_hash),
                data,
                dotcell,
            }
        }

        fn input_view(&self) -> InputCellView<'_> {
            InputCellView {
                previous_tx_hash: &[0u8; 32],
                previous_output_index: 0,
                lock_script_hash: &self.lock_script_hash,
                lock_code_hash: &self.lock_code_hash,
                lock_hash_type: 1,
                lock_args: &self.lock_args,
                capacity: self.capacity,
                occupied_capacity: 0,
                type_code_hash: self.type_code_hash.as_deref(),
                type_hash_type: self.type_code_hash.as_ref().map(|_| 1),
                type_script_hash: self.type_script_hash.as_deref(),
                type_args: self.type_args.as_deref(),
                udt_amount: None,
                bit_cell_identity_id: None,
                // Both sync paths carry this; the input itself has no data.
                dotcell: self.dotcell.as_ref(),
                data: &[],
                is_dao_withdraw_request: false,
                dao_compensation: None,
            }
        }

        fn output_view(&self) -> OutputCellView<'_> {
            OutputCellView {
                capacity: self.capacity,
                lock_code_hash: &self.lock_code_hash,
                lock_hash_type: 1,
                lock_args: &self.lock_args,
                lock_script_hash: &self.lock_script_hash,
                type_code_hash: self.type_code_hash.as_deref(),
                type_hash_type: self.type_code_hash.as_ref().map(|_| 1),
                type_args: self.type_args.as_deref(),
                type_script_hash: self.type_script_hash.as_deref(),
                data_hash: &[],
                data_size: self.data.len() as i32,
                data: &self.data,
            }
        }
    }

    struct OwnedTx {
        tx_hash: Vec<u8>,
        block_hash: Vec<u8>,
        inputs: Vec<OwnedCell>,
        outputs: Vec<OwnedCell>,
    }

    impl OwnedTx {
        fn from_fixture(f: &TxFixture) -> Self {
            Self {
                tx_hash: parse_hex_to_bytes(f.tx_hash),
                block_hash: parse_hex_to_bytes(f.block_hash),
                inputs: f.inputs.iter().map(OwnedCell::from_fixture).collect(),
                outputs: f.outputs.iter().map(OwnedCell::from_fixture).collect(),
            }
        }

        fn view(&self) -> TxView<'_> {
            TxView {
                tx_hash: &self.tx_hash,
                block_hash: &self.block_hash,
                tx_index: 1,
                block_number: 1,
                timestamp: 1_700_000_000_000,
                is_cellbase: false,
                inputs: self.inputs.iter().map(OwnedCell::input_view).collect(),
                outputs: self.outputs.iter().map(OwnedCell::output_view).collect(),
            }
        }
    }

    fn hex20(hex: &str) -> [u8; 20] {
        parse_hex_to_bytes(hex).try_into().expect("20 bytes")
    }

    fn identity_plus(id: &[u8; 20]) -> ItemDelta {
        ItemDelta {
            item_id: id.to_vec(),
            kind: ITEM_KIND_IDENTITY,
            magnitude: 1,
            negative: false,
        }
    }

    fn identity_minus(id: &[u8; 20]) -> ItemDelta {
        ItemDelta {
            item_id: id.to_vec(),
            kind: ITEM_KIND_IDENTITY,
            magnitude: 1,
            negative: true,
        }
    }

    fn transitions(f: &TxFixture) -> Vec<DotCellTransition> {
        let owned = OwnedTx::from_fixture(f);
        classify_dotcell_transitions(&owned.view()).expect("classification")
    }

    fn actions_for(f: &TxFixture) -> Vec<ckbadger_store::types::ProtocolAction> {
        protocol_actions_for(&transitions(f))
    }

    /// The raw per-transition list, before the builder merges by party.
    fn named_for(f: &TxFixture) -> Vec<NamedParticipant> {
        named_participants_for(&transitions(f))
    }

    /// What the builder actually persists: one `ParticipantDelta` per party,
    /// with named roles merged into the lock participant when that party also
    /// holds a cell here.
    fn built_participants(f: &TxFixture) -> Vec<ckbadger_store::types::ParticipantDelta> {
        let owned = OwnedTx::from_fixture(f);
        let built = build_tx_actions_for_block_with_io(
            &[owned.view()],
            &[Box::new(DotCellDetector::new()) as Box<dyn ProtocolDetector>],
        )
        .expect("build");
        built
            .into_iter()
            .next()
            .expect("one tx")
            .actions
            .participants
    }

    fn kind_of<'a>(ts: &'a [DotCellTransition], label: &str) -> &'a DotCellTransitionKind {
        &ts.iter()
            .find(|t| t.label == label)
            .unwrap_or_else(|| panic!("no transition for {label}"))
            .kind
    }

    // ── Real transactions ──────────────────────────────────────────────────

    #[test]
    fn m2_registration_yields_register_and_suppresses_ring_link() {
        let ts = transitions(&fixture::M2_REGISTER_SUPPORT);
        assert_eq!(
            ts.iter()
                .filter(|t| matches!(t.kind, DotCellTransitionKind::RingLink))
                .count(),
            1,
            "the ring predecessor `cellula` only relinked: {ts:?}"
        );
        assert!(matches!(
            kind_of(&ts, "support"),
            DotCellTransitionKind::Register {
                parent_id: None,
                ..
            }
        ));

        let actions = actions_for(&fixture::M2_REGISTER_SUPPORT);
        assert_eq!(
            actions.len(),
            1,
            "the ring relink emits nothing: {actions:?}"
        );
        assert_eq!(actions[0].protocol, "dotcell");
        assert_eq!(actions[0].action, "register");

        let support_id = hex20("0x62d71147ac82b83c8531126cacb0d2f072bfd94a");
        let registrant20 = hex20("0x57d926a44d83fc13b21ce037b1e31f4223e3c867");
        assert_eq!(
            named_for(&fixture::M2_REGISTER_SUPPORT)
                .iter()
                .map(|n| n.id)
                .collect::<Vec<_>>(),
            vec![
                ParticipantId::LockPrefix(registrant20),
                ParticipantId::LockPrefix(registrant20)
            ],
            "owner_to and manager_to are named separately; the builder merges them"
        );

        // The registrant DOES hold cells here (it paid), so the named party is
        // the same party as that lock participant, not a second one.
        let built = built_participants(&fixture::M2_REGISTER_SUPPORT);
        let registrant = built
            .iter()
            .find(|p| p.id.as_bytes()[..20] == registrant20[..])
            .expect("registrant participates");
        assert_eq!(
            registrant.id,
            ParticipantId::lock(&parse_hex_to_bytes(
                "0x57d926a44d83fc13b21ce037b1e31f4223e3c867cfa3f60e1324d5bfd5cd742d"
            ))
            .unwrap(),
            "a named prefix that matches exactly one cell owner IS that owner"
        );
        assert_eq!(registrant.item_deltas, vec![identity_plus(&support_id)]);
        assert_eq!(registrant.roles, OWNER_TO | MANAGER_TO);

        // The Account Lock holds every name cell, and gains no identity.
        let account_lock = ScriptParser::compute_script_hash(
            &fixture::M2_REGISTER_SUPPORT.outputs[1].lock_script(),
        );
        let account_lock_party = built
            .iter()
            .find(|p| p.id == ParticipantId::lock(&account_lock).unwrap())
            .expect("the Account Lock holds the name cells");
        assert!(
            account_lock_party.item_deltas.is_empty(),
            "the protocol's own lock never owns the names it holds"
        );
    }

    #[test]
    fn m3_transfer_names_previous_and_new_owner() {
        let ts = transitions(&fixture::M3_TRANSFER_ABUSE);
        let from = hex20("0x57d926a44d83fc13b21ce037b1e31f4223e3c867");
        let to = hex20("0xac55d7dab2e9a4b85775a811bb4063e94cc98182");
        assert_eq!(
            kind_of(&ts, "abuse"),
            &DotCellTransitionKind::Transfer {
                from20: from,
                to20: to
            }
        );
        let abuse_id = hex20("0xa8d5f7507b9f3d30090253a741c1c80cb0cb121c");
        let built = built_participants(&fixture::M3_TRANSFER_ABUSE);

        // The previous owner spends their own cell here, so they are a LOCK
        // participant carrying the -1.
        let sender = built
            .iter()
            .find(|p| {
                p.id == ParticipantId::lock(&parse_hex_to_bytes(
                    "0x57d926a44d83fc13b21ce037b1e31f4223e3c867cfa3f60e1324d5bfd5cd742d",
                ))
                .unwrap()
            })
            .expect("previous owner participates through its own cell");
        assert_eq!(sender.item_deltas, vec![identity_minus(&abuse_id)]);
        assert_eq!(sender.roles, OWNER_FROM);

        // The recipient holds no cell: a standalone prefix participant.
        let recipient = built
            .iter()
            .find(|p| p.id == ParticipantId::LockPrefix(to))
            .expect("recipient is named");
        assert_eq!(recipient.ckb_delta, 0);
        assert_eq!(recipient.item_deltas, vec![identity_plus(&abuse_id)]);
        assert_eq!(recipient.roles, OWNER_TO | MANAGER_TO);
        assert!(from != to);

        assert_eq!(
            actions_for(&fixture::M3_TRANSFER_ABUSE)[0].action,
            "transfer"
        );
    }

    /// The recipient of a mainnet transfer holds no cell in the transaction
    /// (0/57 mainnet owner changes did). The builder must still make them a
    /// standalone participant of their own.
    #[test]
    fn m4_transfer_recipient_absent_from_tx_is_still_named() {
        let recipient = hex20("0x1e3a88ca5cc39f1bd38c091b53e33b7c29ebd019");
        let owned = OwnedTx::from_fixture(&fixture::M4_TRANSFER_APT);
        let tx = owned.view();
        assert!(
            !tx.inputs
                .iter()
                .any(|i| i.lock_script_hash[..20] == recipient[..])
                && !tx
                    .outputs
                    .iter()
                    .any(|o| o.lock_script_hash[..20] == recipient[..]),
            "the fixture's point is that the recipient holds no cell here"
        );

        let built = build_tx_actions_for_block_with_io(
            &[tx],
            &[Box::new(DotCellDetector::new()) as Box<dyn ProtocolDetector>],
        )
        .expect("build");
        let apt_id = hex20("0x37561deee27ed512016aa4fd60418487fec9f944");
        let party = built[0]
            .actions
            .participants
            .iter()
            .find(|p| p.id == ParticipantId::LockPrefix(recipient))
            .expect("the recipient must be a participant of this transaction");
        assert_eq!(party.ckb_delta, 0, "no cell of theirs moved");
        assert_eq!(party.item_deltas, vec![identity_plus(&apt_id)]);
        assert_eq!(party.roles, OWNER_TO | MANAGER_TO);
    }

    #[test]
    fn m5_list_names_seller_and_sale_lock_instance() {
        let ts = transitions(&fixture::M5_LIST_SATOSHI);
        let seller20 = hex20("0xac55d7dab2e9a4b85775a811bb4063e94cc98182");
        let sale20 = hex20("0x4136f1b0aa24b8372b2e52a13e77a24b676d5689");
        assert_eq!(
            kind_of(&ts, "satoshi"),
            &DotCellTransitionKind::List {
                from_owner20: seller20,
                sale_hash20: sale20,
                seller32: parse_hex_to_bytes(
                    "0xac55d7dab2e9a4b85775a811bb4063e94cc98182835681d1b7706a491abf6bcf"
                )
                .try_into()
                .unwrap(),
                price: 100_000_000_000_000,
            }
        );
        let actions = actions_for(&fixture::M5_LIST_SATOSHI);
        assert_eq!(actions[0].action, "list");
        assert_eq!(
            actions[0].metadata.to_value().unwrap()["price"],
            serde_json::json!("100000000000000")
        );

        let named = named_for(&fixture::M5_LIST_SATOSHI);
        let satoshi_id = hex20("0xc6384c03addbed0fce0a6b66a53f7b9f279e72f9");
        let seller = named
            .iter()
            .find(|n| n.id == ParticipantId::LockPrefix(seller20))
            .expect("seller named");
        assert_eq!(seller.item_deltas, vec![identity_minus(&satoshi_id)]);
        assert_eq!(seller.roles, OWNER_FROM);
        let sale = named
            .iter()
            .find(|n| n.id == ParticipantId::LockPrefix(sale20))
            .expect("sale lock instance named");
        assert_eq!(sale.item_deltas, vec![identity_plus(&satoshi_id)]);
        assert_ne!(sale.roles & OWNER_TO, 0);
    }

    #[test]
    fn t7_buy_names_sale_lock_and_buyer_not_seller() {
        let ts = transitions(&fixture::T7_BUY_CARTAOPROVA);
        let sale20 = hex20("0xcb736f437a28b77ecb038cc147c2171ed83fc371");
        let buyer20 = hex20("0x58e6c6f873af57732daae458be3c56c2c847b141");
        assert_eq!(
            kind_of(&ts, "cartaoprova3695"),
            &DotCellTransitionKind::Buy {
                sale_hash20: sale20,
                seller32: parse_hex_to_bytes(
                    "0x9d602bfc26415da790c79526703cbbc1e9267cbe4be38f8f98b769ad0a90e1a1"
                )
                .try_into()
                .unwrap(),
                price: 10_000_000_000,
                buyer20,
            }
        );
        assert_eq!(actions_for(&fixture::T7_BUY_CARTAOPROVA)[0].action, "buy");

        let named = named_for(&fixture::T7_BUY_CARTAOPROVA);
        let id = hex20("0xac485eebad1a642cc759559e02782791ad3ae89c");
        assert_eq!(
            named
                .iter()
                .find(|n| n.id == ParticipantId::LockPrefix(sale20))
                .unwrap()
                .item_deltas,
            vec![identity_minus(&id)]
        );
        assert_eq!(
            named
                .iter()
                .find(|n| n.id == ParticipantId::LockPrefix(buyer20))
                .unwrap()
                .item_deltas,
            vec![identity_plus(&id)]
        );
        let seller20 = hex20("0x9d602bfc26415da790c79526703cbbc1e9267cbe");
        assert!(
            !named
                .iter()
                .any(|n| n.id == ParticipantId::LockPrefix(seller20)),
            "the seller is paid in a cell of their own, so they are a LOCK participant, not a named one"
        );
    }

    #[test]
    fn t8_cancel_returns_name_to_seller() {
        let ts = transitions(&fixture::T8_CANCEL_CARTAOPROVA);
        let sale20 = hex20("0xe9d1bfb8a04cb4323b0b2514635dd85ba88161c1");
        let seller20 = hex20("0x58e6c6f873af57732daae458be3c56c2c847b141");
        assert_eq!(
            kind_of(&ts, "cartaoprova3695"),
            &DotCellTransitionKind::CancelSale {
                sale_hash20: sale20,
                seller32: parse_hex_to_bytes(
                    "0x58e6c6f873af57732daae458be3c56c2c847b14123b09908c29da905e754fc42"
                )
                .try_into()
                .unwrap(),
                price: 10_000_000_000,
                to_owner20: seller20,
            }
        );
        assert_eq!(
            actions_for(&fixture::T8_CANCEL_CARTAOPROVA)[0].action,
            "cancel_sale"
        );
        let named = named_for(&fixture::T8_CANCEL_CARTAOPROVA);
        let id = hex20("0xac485eebad1a642cc759559e02782791ad3ae89c");
        assert_eq!(
            named
                .iter()
                .find(|n| n.id == ParticipantId::LockPrefix(sale20))
                .unwrap()
                .item_deltas,
            vec![identity_minus(&id)]
        );
        assert_eq!(
            named
                .iter()
                .find(|n| n.id == ParticipantId::LockPrefix(seller20))
                .unwrap()
                .item_deltas,
            vec![identity_plus(&id)]
        );
    }

    #[test]
    fn t4_edit_manager_names_new_manager_without_delta() {
        let ts = transitions(&fixture::T4_EDIT_MANAGER);
        let from = hex20("0xd5026c2c742a379b0c422271a4e1f59b211a6c31");
        let to = hex20("0x58e6c6f873af57732daae458be3c56c2c847b141");
        assert_eq!(
            kind_of(&ts, "v3-first-name"),
            &DotCellTransitionKind::EditManager {
                from20: from,
                to20: to
            }
        );
        assert_eq!(
            actions_for(&fixture::T4_EDIT_MANAGER)[0].action,
            "edit_manager"
        );
        assert_eq!(
            named_for(&fixture::T4_EDIT_MANAGER),
            vec![NamedParticipant {
                id: ParticipantId::LockPrefix(to),
                item_deltas: vec![],
                roles: MANAGER_TO,
            }],
            "a new manager gained a right, not an item"
        );
    }

    #[test]
    fn t5_renew_has_no_named_participants() {
        let ts = transitions(&fixture::T5_RENEW);
        assert_eq!(
            kind_of(&ts, "v3-first-name"),
            &DotCellTransitionKind::Renew {
                from: 1_820_524_007,
                to: 1_852_060_007
            }
        );
        let actions = actions_for(&fixture::T5_RENEW);
        assert_eq!(actions[0].action, "renew");
        assert_eq!(
            actions[0].metadata.to_value().unwrap()["expiryFrom"],
            serde_json::json!(1_820_524_007u64)
        );
        assert_eq!(
            actions[0].metadata.to_value().unwrap()["expiryTo"],
            serde_json::json!(1_852_060_007u64)
        );
        assert!(named_for(&fixture::T5_RENEW).is_empty());
    }

    #[test]
    fn t3_subname_registration_carries_parent_id() {
        let ts = transitions(&fixture::T3_REGISTER_SUBNAME);
        let parent = hex20("0x4144e782dfaadeeb07625e11e4b6de717893aacb");
        assert!(matches!(
            kind_of(&ts, "shop.v3-first-name"),
            DotCellTransitionKind::Register {
                parent_id: Some(p),
                ..
            } if *p == parent
        ));
        assert!(
            matches!(
                kind_of(&ts, "v3-first-name"),
                DotCellTransitionKind::RingLink
            ),
            "the parent is also the ring predecessor here: only `next` changed"
        );
        let actions = actions_for(&fixture::T3_REGISTER_SUBNAME);
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].action, "register_subname");
        assert_eq!(
            actions[0].metadata.to_value().unwrap()["parentId"],
            serde_json::json!("0x4144e782dfaadeeb07625e11e4b6de717893aacb")
        );
    }

    #[test]
    fn t9_identical_recreation_is_touch() {
        let ts = transitions(&fixture::T9_TOUCH_REF43EUEW);
        assert_eq!(kind_of(&ts, "ref43euew"), &DotCellTransitionKind::Touch);
        assert_eq!(actions_for(&fixture::T9_TOUCH_REF43EUEW)[0].action, "touch");
        assert!(named_for(&fixture::T9_TOUCH_REF43EUEW).is_empty());
    }

    #[test]
    fn ring_root_is_not_an_identity_and_emits_nothing() {
        for f in [&fixture::M1_RING_ROOT, &fixture::T1_RING_ROOT] {
            let ts = transitions(f);
            assert_eq!(ts.len(), 1);
            assert_eq!(ts[0].kind, DotCellTransitionKind::RingRoot);
            assert!(actions_for(f).is_empty());
            assert!(named_for(f).is_empty());
        }
    }

    // ── Synthetic shapes the chain has not produced yet ────────────────────

    /// Build a name cell's data from parts, so unobserved lifecycle shapes
    /// (recycle, expiry regression, multi-change) can be exercised.
    fn name_data(
        label: &str,
        owner: [u8; 20],
        manager: [u8; 20],
        expiry: u64,
        next: [u8; 20],
        records_hash: [u8; 32],
    ) -> Vec<u8> {
        let mut data = vec![3u8];
        data.extend_from_slice(&records_hash);
        data.extend_from_slice(&next);
        data.extend_from_slice(&expiry.to_le_bytes()[..5]);
        data.extend_from_slice(&owner);
        data.extend_from_slice(&manager);
        data.extend_from_slice(label.as_bytes());
        data
    }

    fn synthetic_cell(data: Vec<u8>, as_input: bool) -> OwnedCell {
        let name = DotCellParser::parse_name_data(&data).expect("synthetic name data");
        OwnedCell {
            capacity: 240_00000000,
            lock_script_hash: vec![0x01; 32],
            lock_code_hash: parse_hex_to_bytes(fixture::ACCOUNT_LOCK_CODE_HASH_MAINNET),
            lock_args: Vec::new(),
            type_code_hash: Some(parse_hex_to_bytes(fixture::ACCOUNT_TYPE_CODE_HASH_MAINNET)),
            type_args: Some(parse_hex_to_bytes(fixture::NAMESPACE_ARGS_MAINNET)),
            type_script_hash: Some(vec![0x02; 32]),
            data: data.clone(),
            dotcell: as_input.then_some(name),
        }
    }

    fn synthetic_tx(inputs: Vec<OwnedCell>, outputs: Vec<OwnedCell>) -> OwnedTx {
        OwnedTx {
            tx_hash: vec![0xAB; 32],
            block_hash: vec![0xCD; 32],
            inputs,
            outputs,
        }
    }

    #[test]
    fn recycle_is_input_only() {
        let owner = [0x44u8; 20];
        let owned = synthetic_tx(
            vec![synthetic_cell(
                name_data("gone", owner, owner, 1_700_000_000, [0u8; 20], [0u8; 32]),
                true,
            )],
            vec![],
        );
        let ts = classify_dotcell_transitions(&owned.view()).unwrap();
        assert_eq!(ts.len(), 1);
        assert_eq!(
            ts[0].kind,
            DotCellTransitionKind::Recycle { owner20: owner }
        );
        let named = named_participants_for(&ts);
        assert_eq!(named.len(), 1);
        assert_eq!(named[0].id, ParticipantId::LockPrefix(owner));
        assert_eq!(named[0].roles, OWNER_FROM);
        assert_eq!(
            named[0].item_deltas,
            vec![identity_minus(&DotCellParser::derive_id("gone"))]
        );
        assert_eq!(protocol_actions_for(&ts)[0].action, "recycle");
    }

    #[test]
    fn expiry_decrease_is_an_error() {
        let owner = [0x44u8; 20];
        let owned = synthetic_tx(
            vec![synthetic_cell(
                name_data("back", owner, owner, 1_800_000_000, [0u8; 20], [0u8; 32]),
                true,
            )],
            vec![synthetic_cell(
                name_data("back", owner, owner, 1_700_000_000, [0u8; 20], [0u8; 32]),
                false,
            )],
        );
        let err = classify_dotcell_transitions(&owned.view()).unwrap_err();
        assert!(err.to_string().contains("expiry decreased"), "{err}");
    }

    #[test]
    fn a_name_consumed_twice_in_one_tx_is_an_error() {
        let owner = [0x44u8; 20];
        let data = name_data("dup", owner, owner, 1_700_000_000, [0u8; 20], [0u8; 32]);
        let owned = synthetic_tx(
            vec![
                synthetic_cell(data.clone(), true),
                synthetic_cell(data.clone(), true),
            ],
            vec![],
        );
        let err = classify_dotcell_transitions(&owned.view()).unwrap_err();
        assert!(err.to_string().contains("consumed twice"), "{err}");
    }

    #[test]
    fn multi_change_tx_emits_one_action_with_changes_list() {
        let old_owner = [0x44u8; 20];
        let new_owner = [0x55u8; 20];
        let owned = synthetic_tx(
            vec![synthetic_cell(
                name_data(
                    "multi",
                    old_owner,
                    old_owner,
                    1_800_000_000,
                    [0u8; 20],
                    [0u8; 32],
                ),
                true,
            )],
            vec![synthetic_cell(
                name_data(
                    "multi",
                    new_owner,
                    new_owner,
                    1_800_000_000,
                    [0u8; 20],
                    [0x99u8; 32],
                ),
                false,
            )],
        );
        let ts = classify_dotcell_transitions(&owned.view()).unwrap();
        assert_eq!(ts.len(), 1);
        assert_eq!(
            ts[0].kind,
            DotCellTransitionKind::Transfer {
                from20: old_owner,
                to20: new_owner
            },
            "ownership is the primary fact when several change at once"
        );
        assert_eq!(ts[0].changes, vec!["owner", "manager", "records"]);
        let actions = protocol_actions_for(&ts);
        assert_eq!(actions.len(), 1);
        assert_eq!(
            actions[0].metadata.to_value().unwrap()["changes"],
            serde_json::json!(["owner", "manager", "records"])
        );
    }

    // ── Mapping and plumbing ──────────────────────────────────────────────

    #[test]
    fn dotcell_asset_action_mapping() {
        for (action, expected) in [
            ("register", Some(AssetAction::Mint)),
            ("register_subname", Some(AssetAction::Mint)),
            ("transfer", Some(AssetAction::Transfer)),
            ("buy", Some(AssetAction::Transfer)),
            ("renew", Some(AssetAction::Renew)),
            ("list", Some(AssetAction::Update)),
            ("cancel_sale", Some(AssetAction::Update)),
            ("edit_records", Some(AssetAction::Update)),
            ("edit_manager", Some(AssetAction::Update)),
            ("touch", Some(AssetAction::Update)),
            ("recycle", Some(AssetAction::Recycle)),
        ] {
            assert_eq!(dotcell_asset_action(action).ok(), expected, "{action}");
        }
        // The action strings are a closed set this module produces itself, so
        // an unknown one is a bug (a transition kind that gained no mapping),
        // not a transaction to file with one feed entry missing.
        let err = dotcell_asset_action("not_a_dotcell_action").unwrap_err();
        assert!(err.to_string().contains("not_a_dotcell_action"), "{err}");
    }

    #[test]
    fn collection_activity_entry_from_protocol_actions() {
        let actions = actions_for(&fixture::M2_REGISTER_SUPPORT);
        let entry = build_dotcell_tx_activity_entry(&actions, &[0xAA; 32], &[0xBB; 32], 1_234)
            .expect("a known action maps")
            .expect("a register must reach the collection feed");
        assert_eq!(entry.actions, vec![AssetAction::Mint]);
        assert_eq!(entry.tx_hash, vec![0xAA; 32]);
        assert_eq!(entry.block_hash, vec![0xBB; 32]);
        assert_eq!(entry.timestamp_ms, 1_234);

        assert!(
            build_dotcell_tx_activity_entry(
                &actions_for(&fixture::M1_RING_ROOT),
                &[0xAA; 32],
                &[0xBB; 32],
                1_234
            )
            .unwrap()
            .is_none(),
            "a ring root is not a collection event"
        );
        assert!(build_dotcell_tx_activity_entry(
            &[ckbadger_store::types::ProtocolAction::new(
                "rgbpp",
                "leap_to_ckb",
                serde_json::json!({})
            )],
            &[0xAA; 32],
            &[0xBB; 32],
            1_234
        )
        .unwrap()
        .is_none());

        // An unknown `dotcell:*` action stops the batch, naming the action and
        // the transaction.
        let err = build_dotcell_tx_activity_entry(
            &[ckbadger_store::types::ProtocolAction::new(
                "dotcell",
                "teleport",
                serde_json::json!({}),
            )],
            &[0xAA; 32],
            &[0xBB; 32],
            1_234,
        )
        .unwrap_err();
        assert!(err.to_string().contains("teleport"), "{err}");
    }

    #[test]
    fn detector_batch_prefilter_skips_batches_without_account_type() {
        use std::collections::HashSet;
        let detector = DotCellDetector::new();
        let empty: HashSet<[u8; 32]> = HashSet::new();
        assert!(!detector.might_apply_batch(&empty, &empty));

        let mut types: HashSet<[u8; 32]> = HashSet::new();
        types.insert(
            parse_hex_to_bytes(fixture::ACCOUNT_TYPE_CODE_HASH_TESTNET)
                .try_into()
                .unwrap(),
        );
        assert!(detector.might_apply_batch(&empty, &types));

        // The locks are not enough on their own: the name cell's TYPE script is
        // what makes a transaction a `.cell` transaction.
        let mut locks: HashSet<[u8; 32]> = HashSet::new();
        locks.insert(
            parse_hex_to_bytes(fixture::ACCOUNT_LOCK_CODE_HASH_MAINNET)
                .try_into()
                .unwrap(),
        );
        assert!(!detector.might_apply_batch(&locks, &empty));
    }
}
