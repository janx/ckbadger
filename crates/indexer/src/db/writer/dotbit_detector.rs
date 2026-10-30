//! `.bit` (DAS) Layer-3 actions.
//!
//! A DAS transaction names its action in its `ActionData` witness
//! (`confirm_proposal`, `renew_account`, `edit_records`, `transfer_account`,
//! `recycle_expired_account`, …). This detector records that action verbatim,
//! once per `.bit` account the action is about, as
//! `ProtocolAction { protocol: "dotbit", action, metadata: {"accountId": "0x…"} }`.
//!
//! Which accounts an action is about comes from the AccountCells the
//! transaction consumes and creates, by account id:
//!
//! - an account only created (registration) or only consumed (recycle) is a
//!   subject; the accounts present on both sides are then ring neighbours
//!   whose `next` pointer was rewritten, not subjects;
//! - when every account is present on both sides (renew, edit, transfer,
//!   manager change, sub-account enable, cross-chain lock, …), they are all
//!   subjects.
//!
//! Subjects are listed in cell order: inputs first, then outputs. The DAS root
//! AccountCell and AccountCells creation did not index name no account and are
//! never subjects. A DAS transaction that touches no AccountCell (pre-register,
//! proposal, income or balance maintenance) moves no account and records no
//! `dotbit` action.
//!
//! Live sync, bulk build and the tx-pool mirror reach this through the one
//! production detector list, so the three cannot interpret a `.bit`
//! transaction differently.

use anyhow::Result;
use ckbadger_store::types::{ItemDelta, LockCallEntry, ProtocolAction, TypeCallEntry};

use crate::parser::dotbit::DotbitParser;

use super::activities::{OwnerAccum, ProtocolDetector, TxView};

/// The protocol id of this detector's actions (`docs/metadata/protocols.toml`).
pub const PROTOCOL: &str = "dotbit";

pub(crate) struct DotbitDetector;

impl DotbitDetector {
    pub(crate) fn new() -> Self {
        Self
    }
}

/// The `.bit` account ids `tx` consumes and creates, each in cell order and
/// without repeats.
fn account_ids(tx: &TxView<'_>) -> (Vec<Vec<u8>>, Vec<Vec<u8>>) {
    let mut consumed: Vec<Vec<u8>> = Vec::new();
    for input in &tx.inputs {
        let Some(type_code_hash) = input.type_code_hash else {
            continue;
        };
        // `data` is the id the consumed cell was indexed under (empty for a
        // cell creation did not index), or the real data in the tx-pool
        // mirror — the same resolution the activity builder's item deltas use.
        if let Some(id) =
            DotbitParser::resolve_account_id(type_code_hash, input.type_args, input.data)
        {
            if !consumed.contains(&id) {
                consumed.push(id);
            }
        }
    }
    let mut created: Vec<Vec<u8>> = Vec::new();
    for output in &tx.outputs {
        let Some(type_code_hash) = output.type_code_hash else {
            continue;
        };
        if let Some(id) =
            DotbitParser::parse_account_id_from_data(type_code_hash, output.type_args, output.data)
        {
            if !created.contains(&id) {
                created.push(id);
            }
        }
    }
    (consumed, created)
}

/// The accounts a DAS action is about, in cell order (see the module docs).
pub(crate) fn subject_account_ids(consumed: &[Vec<u8>], created: &[Vec<u8>]) -> Vec<Vec<u8>> {
    let removed = consumed.iter().filter(|id| !created.contains(id));
    let added = created.iter().filter(|id| !consumed.contains(id));
    let appeared_or_disappeared: Vec<Vec<u8>> = removed.chain(added).cloned().collect();
    if appeared_or_disappeared.is_empty() {
        // Every account is on both sides: the action rewrote each in place.
        consumed.to_vec()
    } else {
        appeared_or_disappeared
    }
}

/// The `dotbit:*` actions of one transaction: one per subject account, all
/// carrying the transaction's ActionData action verbatim.
pub(crate) fn dotbit_protocol_actions(tx: &TxView<'_>) -> Vec<ProtocolAction> {
    let Some(action) = tx.dotbit_action else {
        return Vec::new();
    };
    let (consumed, created) = account_ids(tx);
    subject_account_ids(&consumed, &created)
        .into_iter()
        .map(|account_id| {
            ProtocolAction::new(
                PROTOCOL,
                action,
                serde_json::json!({ "accountId": format!("0x{}", hex::encode(account_id)) }),
            )
        })
        .collect()
}

impl ProtocolDetector for DotbitDetector {
    fn protocol(&self) -> &'static str {
        PROTOCOL
    }

    fn might_apply(&self, tx: &TxView<'_>) -> bool {
        tx.dotbit_action.is_some()
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
        // by (protocol, action, metadata). Each action names its account, so
        // two accounts of one transaction never collapse into one.
        Ok(dotbit_protocol_actions(tx))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::writer::activities::{
        build_tx_actions_for_block_with_io, InputCellView, OutputCellView,
    };
    use crate::parser::dotbit::fixtures::{account_cell_data, MAINNET_ROOT_ACCOUNT_CELL_DATA};

    /// valar.bit's account id (`0x2d2ed79a…90e9`).
    const VALAR: [u8; 20] = [
        0x2d, 0x2e, 0xd7, 0x9a, 0x32, 0xf0, 0x78, 0xc5, 0x11, 0x10, 0x87, 0x65, 0xa3, 0x84, 0xa1,
        0x83, 0xe1, 0x49, 0x90, 0xe9,
    ];
    /// The recycled neighbour `0x2d30364d…` (padded) and two registrations.
    const NEIGHBOUR: [u8; 20] = [
        0x2d, 0x30, 0x36, 0x4d, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1,
    ];
    const NEW_A: [u8; 20] = [0x31; 20];
    const NEW_B: [u8; 20] = [0x32; 20];
    const OWNER_1: [u8; 32] = [0xA1; 32];
    const OWNER_2: [u8; 32] = [0xA2; 32];
    const LOCK_CODE: [u8; 32] = [0x10; 32];

    fn account_code_hash() -> Vec<u8> {
        crate::rpc::parse_hex_to_bytes(crate::parser::dotbit::DOTBIT_ACCOUNT_CELL_TYPE_ID)
    }

    struct Fixture {
        code_hash: Vec<u8>,
        /// (owner, indexed id or empty) per input.
        inputs: Vec<([u8; 32], Vec<u8>)>,
        /// (owner, cell data) per output.
        outputs: Vec<([u8; 32], Vec<u8>)>,
        action: Option<&'static str>,
    }

    impl Fixture {
        fn new(action: Option<&'static str>) -> Self {
            Self {
                code_hash: account_code_hash(),
                inputs: vec![],
                outputs: vec![],
                action,
            }
        }
        fn consume(mut self, owner: [u8; 32], id: &[u8]) -> Self {
            self.inputs.push((owner, id.to_vec()));
            self
        }
        fn create(mut self, owner: [u8; 32], data: Vec<u8>) -> Self {
            self.outputs.push((owner, data));
            self
        }
        fn view(&self) -> TxView<'_> {
            TxView {
                tx_hash: &[0x77; 32],
                block_hash: &[0x88; 32],
                tx_index: 1,
                block_number: 9_000_000,
                timestamp: 1_700_000_000_000,
                is_cellbase: false,
                dotbit_action: self.action,
                inputs: self
                    .inputs
                    .iter()
                    .map(|(owner, id)| InputCellView {
                        previous_tx_hash: &[0x66; 32],
                        previous_output_index: 0,
                        lock_script_hash: owner,
                        lock_code_hash: &LOCK_CODE,
                        lock_hash_type: 1,
                        lock_args: &[],
                        capacity: 300_00000000,
                        occupied_capacity: 200_00000000,
                        type_code_hash: Some(&self.code_hash),
                        type_hash_type: Some(1),
                        type_script_hash: Some(&[0x12; 32]),
                        type_args: Some(&[]),
                        udt_amount: None,
                        bit_cell_identity_id: None,
                        dotcell: None,
                        data: id,
                        is_dao_withdraw_request: false,
                        dao_compensation: None,
                    })
                    .collect(),
                outputs: self
                    .outputs
                    .iter()
                    .map(|(owner, data)| OutputCellView {
                        capacity: 300_00000000,
                        lock_code_hash: &LOCK_CODE,
                        lock_hash_type: 1,
                        lock_args: &[],
                        lock_script_hash: owner,
                        type_code_hash: Some(&self.code_hash),
                        type_hash_type: Some(1),
                        type_args: Some(&[]),
                        type_script_hash: Some(&[0x12; 32]),
                        data_hash: &[0x13; 32],
                        data_size: data.len() as i32,
                        data,
                    })
                    .collect(),
            }
        }
        /// `(action, accountId)` of every `dotbit` action the production
        /// builder emits for this transaction, in order.
        fn actions(&self) -> Vec<(String, String)> {
            let built = build_tx_actions_for_block_with_io(
                &[self.view()],
                &[Box::new(DotbitDetector::new()) as Box<dyn ProtocolDetector>],
            )
            .unwrap();
            built[0]
                .actions
                .protocol_actions
                .iter()
                .map(|a| {
                    assert_eq!(a.protocol, PROTOCOL);
                    let metadata = a.metadata_value().unwrap();
                    (
                        a.action.clone(),
                        metadata["accountId"].as_str().unwrap().to_string(),
                    )
                })
                .collect()
        }
    }

    fn data(id: [u8; 20], expired_at: u64) -> Vec<u8> {
        crate::rpc::parse_hex_to_bytes(&account_cell_data(id, [0xFF; 20], expired_at))
    }

    fn hex_id(id: [u8; 20]) -> String {
        format!("0x{}", hex::encode(id))
    }

    #[test]
    fn renew_is_about_the_renewed_account() {
        let fx = Fixture::new(Some("renew_account"))
            .consume(OWNER_1, &VALAR)
            .create(OWNER_1, data(VALAR, 2_000_000_000));
        assert_eq!(
            fx.actions(),
            vec![("renew_account".to_string(), hex_id(VALAR))]
        );
    }

    #[test]
    fn edit_records_and_transfer_are_about_the_account_they_rewrite() {
        let edit = Fixture::new(Some("edit_records"))
            .consume(OWNER_1, &VALAR)
            .create(OWNER_1, data(VALAR, 1_000));
        assert_eq!(
            edit.actions(),
            vec![("edit_records".to_string(), hex_id(VALAR))]
        );
        let transfer = Fixture::new(Some("transfer_account"))
            .consume(OWNER_1, &VALAR)
            .create(OWNER_2, data(VALAR, 1_000));
        assert_eq!(
            transfer.actions(),
            vec![("transfer_account".to_string(), hex_id(VALAR))]
        );
    }

    /// valar's cell is rewritten (its `next` skips the recycled account), but
    /// the recycle is about the neighbour that disappears.
    #[test]
    fn neighbour_recycle_is_about_the_recycled_account_not_valar() {
        let fx = Fixture::new(Some("recycle_expired_account"))
            .consume(OWNER_1, &VALAR)
            .consume(OWNER_2, &NEIGHBOUR)
            .create(OWNER_1, data(VALAR, 1_000));
        assert_eq!(
            fx.actions(),
            vec![("recycle_expired_account".to_string(), hex_id(NEIGHBOUR))]
        );
    }

    /// One ActionData registering two accounts: one entry per registration,
    /// in output order; the ring neighbour rewritten in place is not one.
    #[test]
    fn confirm_proposal_names_each_registered_account_in_output_order() {
        let fx = Fixture::new(Some("confirm_proposal"))
            .consume(OWNER_1, &VALAR)
            .create(OWNER_1, data(VALAR, 1_000))
            .create(OWNER_2, data(NEW_B, 1_000))
            .create(OWNER_2, data(NEW_A, 1_000));
        assert_eq!(
            fx.actions(),
            vec![
                ("confirm_proposal".to_string(), hex_id(NEW_B)),
                ("confirm_proposal".to_string(), hex_id(NEW_A)),
            ]
        );
    }

    #[test]
    fn root_account_cell_and_unindexed_inputs_are_never_subjects() {
        let root = crate::rpc::parse_hex_to_bytes(MAINNET_ROOT_ACCOUNT_CELL_DATA);
        let fx = Fixture::new(Some("confirm_proposal"))
            .consume(OWNER_1, &[])
            .create(OWNER_1, root)
            .create(OWNER_2, data(NEW_A, 1_000));
        assert_eq!(
            fx.actions(),
            vec![("confirm_proposal".to_string(), hex_id(NEW_A))]
        );
    }

    #[test]
    fn no_action_witness_or_no_account_cell_records_nothing() {
        let without_witness = Fixture::new(None)
            .consume(OWNER_1, &VALAR)
            .create(OWNER_2, data(VALAR, 1_000));
        assert!(without_witness.actions().is_empty());
        let without_accounts = Fixture::new(Some("pre_register"));
        assert!(without_accounts.actions().is_empty());
    }

    #[test]
    fn unknown_actions_are_recorded_verbatim() {
        let fx = Fixture::new(Some("lock_account_for_cross_chain"))
            .consume(OWNER_1, &VALAR)
            .create(OWNER_1, data(VALAR, 1_000));
        assert_eq!(
            fx.actions(),
            vec![("lock_account_for_cross_chain".to_string(), hex_id(VALAR))]
        );
    }
}
