//! `addr_txs` rows derived from `TxActions.participants`.
//!
//! The ONE derivation of those rows. Live sync, bulk build and the API's
//! tx-pool mirror all call it, so a transaction indexes identically no matter
//! which path reaches it — there is no second cell-walking derivation left to
//! disagree with this one.

use anyhow::{bail, Result};
use ckbadger_store::types::{AddrTxValue, ParticipantId, TxActions};

use super::activities::ParticipantIo;

/// One `addr_txs` row per participant, in participant order.
///
/// `io[i]` describes `actions.participants[i]`; the builder returns the two
/// index-aligned, and a length mismatch is a bug in the caller, not something
/// to paper over.
pub fn addr_tx_rows(
    actions: &TxActions,
    io: &[ParticipantIo],
) -> Result<Vec<(ParticipantId, AddrTxValue)>> {
    if io.len() != actions.participants.len() {
        bail!(
            "participant io length {} != participants {} for tx 0x{}",
            io.len(),
            actions.participants.len(),
            hex::encode(&actions.tx_hash)
        );
    }
    let mut rows = Vec::with_capacity(actions.participants.len());
    for (p, io) in actions.participants.iter().zip(io) {
        let capacity_change = i64::try_from(p.ckb_delta).map_err(|_| {
            anyhow::anyhow!(
                "participant ckb_delta {} exceeds i64 for addr_tx row: tx=0x{} participant=0x{}",
                p.ckb_delta,
                hex::encode(&actions.tx_hash),
                hex::encode(p.id.as_bytes())
            )
        })?;
        if matches!(p.id, ParticipantId::LockPrefix(_)) && (io.has_inputs || io.has_outputs) {
            bail!(
                "prefix participant with cells must have been merged by the builder: tx=0x{} prefix=0x{}",
                hex::encode(&actions.tx_hash),
                hex::encode(p.id.as_bytes())
            );
        }
        rows.push((
            p.id,
            AddrTxValue::new(capacity_change, io.has_inputs, io.has_outputs, p.tags),
        ));
    }
    Ok(rows)
}

/// The prefixes of participants that hold no cell in this transaction.
///
/// These are the participations `addr_balance.txs_count` cannot see, so they are
/// what `CF_ADDR_PREFIX_STATS` counts.
pub fn standalone_prefixes(actions: &TxActions, io: &[ParticipantIo]) -> Vec<[u8; 20]> {
    actions
        .participants
        .iter()
        .zip(io)
        .filter_map(|(p, io)| match p.id {
            ParticipantId::LockPrefix(pf) if !io.has_inputs && !io.has_outputs => Some(pf),
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::super::activities::{
        build_tx_actions_for_block_with_io, test_fixtures, ParticipantIo, TxView,
    };
    use super::*;
    use ckbadger_store::types::{
        participant_roles, AddrTxValue, ParticipantDelta, ParticipantId, TxActions, TAG_IDENTITY,
        TAG_TOKEN,
    };
    use std::collections::BTreeMap;

    fn test_tx_actions(block_number: i64, tx_index: i32) -> TxActions {
        TxActions {
            tx_hash: vec![0xAA; 32],
            block_hash: vec![0xBB; 32],
            block_number,
            tx_index,
            timestamp: 0,
            is_cellbase: false,
            protocol_actions: vec![],
            type_calls: vec![],
            lock_calls: vec![],
            participants: vec![],
        }
    }

    #[test]
    fn rows_follow_participants_one_to_one() {
        let actions = TxActions {
            participants: vec![
                ParticipantDelta {
                    id: ParticipantId::Lock([0x11; 32]),
                    ckb_delta: -100,
                    used_delta: 0,
                    item_deltas: vec![],
                    tags: 0,
                    roles: 0,
                },
                ParticipantDelta {
                    id: ParticipantId::Lock([0x22; 32]),
                    ckb_delta: 90,
                    used_delta: 0,
                    item_deltas: vec![],
                    tags: TAG_TOKEN,
                    roles: 0,
                },
                ParticipantDelta {
                    id: ParticipantId::LockPrefix([0x33; 20]),
                    ckb_delta: 0,
                    used_delta: 0,
                    item_deltas: vec![],
                    tags: TAG_IDENTITY,
                    roles: participant_roles::OWNER_TO,
                },
            ],
            ..test_tx_actions(7, 1)
        };
        let io = vec![
            ParticipantIo {
                has_inputs: true,
                has_outputs: true,
            },
            ParticipantIo {
                has_inputs: false,
                has_outputs: true,
            },
            ParticipantIo {
                has_inputs: false,
                has_outputs: false,
            },
        ];
        let rows = addr_tx_rows(&actions, &io).unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(
            rows[0],
            (
                ParticipantId::Lock([0x11; 32]),
                AddrTxValue::new(-100, true, true, 0)
            )
        );
        assert_eq!(rows[1].1.tx_type_str(), "received");
        assert_eq!(rows[2].0, ParticipantId::LockPrefix([0x33; 20]));
        assert_eq!(rows[2].1.tx_type_str(), "named");
        assert_eq!(rows[2].1.capacity_change, 0);
        assert_eq!(rows[2].1.tags, TAG_IDENTITY);
        assert_eq!(standalone_prefixes(&actions, &io), vec![[0x33u8; 20]]);
    }

    #[test]
    fn rows_reject_io_length_mismatch_and_i64_overflow() {
        let actions = TxActions {
            participants: vec![ParticipantDelta {
                id: ParticipantId::Lock([0x11; 32]),
                ckb_delta: i128::from(i64::MAX) + 1,
                used_delta: 0,
                item_deltas: vec![],
                tags: 0,
                roles: 0,
            }],
            ..test_tx_actions(7, 1)
        };
        assert!(addr_tx_rows(&actions, &[]).is_err());
        assert!(addr_tx_rows(
            &actions,
            &[ParticipantIo {
                has_inputs: true,
                has_outputs: false
            }]
        )
        .unwrap_err()
        .to_string()
        .contains("exceeds i64"));
    }

    #[test]
    fn rows_reject_a_prefix_participant_that_holds_cells() {
        let actions = TxActions {
            participants: vec![ParticipantDelta {
                id: ParticipantId::LockPrefix([0x33; 20]),
                ckb_delta: 0,
                used_delta: 0,
                item_deltas: vec![],
                tags: 0,
                roles: 0,
            }],
            ..test_tx_actions(7, 1)
        };
        let err = addr_tx_rows(
            &actions,
            &[ParticipantIo {
                has_inputs: false,
                has_outputs: true,
            }],
        )
        .unwrap_err();
        assert!(
            err.to_string()
                .contains("prefix participant with cells must have been merged"),
            "{err}"
        );
    }

    /// The pre-refactor derivation, kept ONLY here: it is the yardstick the new
    /// single derivation is measured against, never a second production path.
    fn legacy_rows_from_cells(
        tx: &TxView<'_>,
        actions: &TxActions,
    ) -> BTreeMap<Vec<u8>, AddrTxValue> {
        // (output_cap_sum, input_cap_sum, has_outputs, has_inputs)
        let mut per_addr: BTreeMap<Vec<u8>, (i64, i64, bool, bool)> = BTreeMap::new();
        for cell in &tx.outputs {
            let e = per_addr.entry(cell.lock_script_hash.to_vec()).or_default();
            e.0 += cell.capacity;
            e.2 = true;
        }
        for input in &tx.inputs {
            let e = per_addr.entry(input.lock_script_hash.to_vec()).or_default();
            e.1 += input.capacity;
            e.3 = true;
        }
        per_addr
            .into_iter()
            .map(|(lock_hash, (out_cap, in_cap, has_out, has_in))| {
                let tags = actions
                    .participants
                    .iter()
                    .find(|p| p.id.as_bytes() == lock_hash.as_slice())
                    .map(|p| p.tags)
                    .expect("legacy derivation: every touched lock is a participant");
                (
                    lock_hash,
                    AddrTxValue::new(out_cap - in_cap, has_in, has_out, tags),
                )
            })
            .collect()
    }

    /// Phase 1a invariant: with no detector naming anybody, the participant-derived
    /// row set equals the cell-derived one, byte for byte.
    #[test]
    fn participant_rows_equal_legacy_cell_derivation_for_lock_only_txs() {
        let fixtures = test_fixtures::legacy_fixture_txs();
        assert!(!fixtures.is_empty());
        for owned in &fixtures {
            let tx = owned.view();
            let built = build_tx_actions_for_block_with_io(std::slice::from_ref(&tx), &[])
                .unwrap()
                .remove(0);
            let new_rows: BTreeMap<Vec<u8>, AddrTxValue> =
                addr_tx_rows(&built.actions, &built.participant_io)
                    .unwrap()
                    .into_iter()
                    .map(|(id, v)| (id.as_bytes().to_vec(), v))
                    .collect();
            let legacy_rows = legacy_rows_from_cells(&tx, &built.actions);
            assert_eq!(
                new_rows,
                legacy_rows,
                "tx {} ({})",
                hex::encode(tx.tx_hash),
                owned.name
            );
        }
    }
}
