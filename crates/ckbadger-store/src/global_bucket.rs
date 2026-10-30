//! The global activity bucket of a transaction: the single classification the
//! site-wide activity and transaction filters use, and the key prefix of
//! `CF_TX_ACTIONS_BY_BUCKET`.
//!
//! The indexer computes it once when it writes a transaction's `TxActions`
//! (both sync paths), the API reads the persisted index, and the per-block and
//! tx-pool views classify a `TxActions` they already hold through this same
//! function. There is no second copy.

use crate::types::TAG_TYPE_CALL;
use crate::types::{TxActions, TAG_DAO, TAG_IDENTITY, TAG_LOCK_CALL, TAG_OBJECT, TAG_TOKEN};

/// One global activity bucket. The discriminant is the 1-byte key prefix of
/// `CF_TX_ACTIONS_BY_BUCKET`, so the order is part of the on-disk format.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum GlobalBucket {
    Dao = 1,
    Token = 2,
    Object = 3,
    Identity = 4,
    Protocol = 5,
    Script = 6,
    Ckb = 7,
}

impl GlobalBucket {
    /// Every bucket, in key-prefix order.
    pub const ALL: [GlobalBucket; 7] = [
        GlobalBucket::Dao,
        GlobalBucket::Token,
        GlobalBucket::Object,
        GlobalBucket::Identity,
        GlobalBucket::Protocol,
        GlobalBucket::Script,
        GlobalBucket::Ckb,
    ];

    /// The 1-byte key prefix.
    pub const fn as_byte(self) -> u8 {
        self as u8
    }

    /// The bucket a persisted key prefix names; an unknown byte is a corrupt
    /// row, never a bucket.
    pub fn from_byte(byte: u8) -> anyhow::Result<Self> {
        Self::ALL
            .into_iter()
            .find(|bucket| bucket.as_byte() == byte)
            .ok_or_else(|| anyhow::anyhow!("unknown global activity bucket byte: {byte}"))
    }

    /// The API filter value that selects this bucket.
    pub const fn as_filter(self) -> &'static str {
        match self {
            GlobalBucket::Dao => "dao",
            GlobalBucket::Token => "token",
            GlobalBucket::Object => "object",
            GlobalBucket::Identity => "identity",
            GlobalBucket::Protocol => "protocol",
            GlobalBucket::Script => "script",
            GlobalBucket::Ckb => "ckb",
        }
    }

    /// The bucket an API filter value selects, if it names one.
    pub fn from_filter(filter: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|bucket| bucket.as_filter() == filter)
    }
}

/// Protocols whose Layer-3 actions are item actions on one asset class and
/// therefore claim that class's bucket ahead of the generic `Protocol` bucket.
///
/// `dao`: a DAO deposit or withdrawal is an action on DAO cells.
/// `dotbit`, `dotcell`: every `.bit` / `.cell` action is about one name (an
/// identity standard of this site), so those transactions sit in Identity —
/// including in-place edits and renewals that move no item — rather than in
/// the generic Protocol bucket, which the Transactions page cannot filter.
fn protocol_item_bucket(protocol: &str) -> Option<GlobalBucket> {
    match protocol {
        "dao" => Some(GlobalBucket::Dao),
        "dotbit" | "dotcell" => Some(GlobalBucket::Identity),
        _ => None,
    }
}

/// The global bucket of a transaction, or `None` for a cellbase, which is
/// never indexed by bucket (it has no `CF_TX_ACTIONS` row either).
///
/// Order of precedence: item-action protocols (`dao`, then `dotbit` /
/// `dotcell`), any other protocol action, then the participants' aggregate
/// tags (DAO, token, object, identity), then script calls; a transaction with
/// none of those is a plain CKB transfer.
pub fn classify_global_bucket(actions: &TxActions) -> Option<GlobalBucket> {
    if actions.is_cellbase {
        return None;
    }
    // Precedence among item-action protocols follows the enum order (DAO
    // first), independent of the order the actions were emitted in.
    if let Some(bucket) = actions
        .protocol_actions
        .iter()
        .filter_map(|action| protocol_item_bucket(&action.protocol))
        .min()
    {
        return Some(bucket);
    }
    if !actions.protocol_actions.is_empty() {
        return Some(GlobalBucket::Protocol);
    }
    let combined_tags: u16 = actions
        .participants
        .iter()
        .fold(0u16, |acc, p| acc | p.tags);
    if combined_tags & TAG_DAO != 0 {
        return Some(GlobalBucket::Dao);
    }
    if combined_tags & TAG_TOKEN != 0 {
        return Some(GlobalBucket::Token);
    }
    if combined_tags & TAG_OBJECT != 0 {
        return Some(GlobalBucket::Object);
    }
    if combined_tags & TAG_IDENTITY != 0 {
        return Some(GlobalBucket::Identity);
    }
    if combined_tags & (TAG_TYPE_CALL | TAG_LOCK_CALL) != 0
        || !actions.type_calls.is_empty()
        || !actions.lock_calls.is_empty()
    {
        return Some(GlobalBucket::Script);
    }
    Some(GlobalBucket::Ckb)
}

/// The `CF_TX_ACTIONS_BY_BUCKET` key a transaction's `TxActions` row owns, or
/// `None` for a cellbase. The one place both sync paths derive it from.
pub fn tx_actions_bucket_key(
    actions: &TxActions,
) -> Option<[u8; crate::keys::TX_ACTIONS_BY_BUCKET_KEY_SIZE]> {
    classify_global_bucket(actions).map(|bucket| {
        crate::keys::encode_tx_actions_by_bucket_key(bucket, actions.block_number, actions.tx_index)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{
        LockCallEntry, ParticipantDelta, ParticipantId, ProtocolAction, TypeCallEntry, TAG_PROTOCOL,
    };

    /// The classifier as it lived in `crates/api/src/routes/activities.rs`
    /// before it moved here, copied verbatim as the oracle the store copy is
    /// pinned against.
    fn legacy_api_classify(actions: &TxActions) -> &'static str {
        if actions.is_cellbase {
            return "cellbase";
        }
        if actions.protocol_actions.iter().any(|a| a.protocol == "dao") {
            return "dao";
        }
        if !actions.protocol_actions.is_empty() {
            return "protocol";
        }
        let combined_tags: u16 = actions
            .participants
            .iter()
            .fold(0u16, |acc, p| acc | p.tags);
        if combined_tags & TAG_DAO != 0 {
            return "dao";
        }
        if combined_tags & TAG_TOKEN != 0 {
            return "token";
        }
        if combined_tags & TAG_OBJECT != 0 {
            return "object";
        }
        if combined_tags & TAG_IDENTITY != 0 {
            return "identity";
        }
        if combined_tags & (TAG_TYPE_CALL | TAG_LOCK_CALL) != 0
            || !actions.type_calls.is_empty()
            || !actions.lock_calls.is_empty()
        {
            return "script";
        }
        "ckb"
    }

    fn actions(tags: &[u16], protocols: &[&str]) -> TxActions {
        TxActions {
            tx_hash: vec![0x11; 32],
            block_hash: vec![0x22; 32],
            block_number: 100,
            tx_index: 1,
            timestamp: 0,
            is_cellbase: false,
            protocol_actions: protocols
                .iter()
                .map(|p| ProtocolAction::new(*p, "act", serde_json::json!({})))
                .collect(),
            type_calls: vec![],
            lock_calls: vec![],
            participants: tags
                .iter()
                .enumerate()
                .map(|(i, tags)| ParticipantDelta {
                    id: ParticipantId::Lock([i as u8; 32]),
                    ckb_delta: 0,
                    used_delta: 0,
                    item_deltas: vec![],
                    tags: *tags,
                    roles: 0,
                })
                .collect(),
        }
    }

    /// Every fixture the pre-move classifier distinguished, with the bucket
    /// it must keep landing in.
    fn pinned_fixtures() -> Vec<(&'static str, TxActions, Option<GlobalBucket>)> {
        let mut cellbase = actions(&[TAG_TOKEN], &[]);
        cellbase.is_cellbase = true;
        let mut type_call = actions(&[0], &[]);
        type_call.type_calls.push(TypeCallEntry {
            type_code_hash: vec![0x33; 32],
            type_hash_type: 1,
            type_args: vec![],
        });
        let mut lock_call = actions(&[0], &[]);
        lock_call.lock_calls.push(LockCallEntry {
            lock_code_hash: vec![0x44; 32],
            lock_hash_type: 1,
            lock_args: vec![],
        });
        vec![
            ("cellbase", cellbase, None),
            (
                "dao protocol",
                actions(&[TAG_TOKEN], &["dao"]),
                Some(GlobalBucket::Dao),
            ),
            (
                "dao protocol beats other protocols",
                actions(&[0], &["rgbpp", "dao"]),
                Some(GlobalBucket::Dao),
            ),
            (
                "dao tag",
                actions(&[TAG_DAO | TAG_TOKEN], &[]),
                Some(GlobalBucket::Dao),
            ),
            (
                "generic protocol beats item tags",
                actions(&[TAG_TOKEN | TAG_PROTOCOL], &["rgbpp"]),
                Some(GlobalBucket::Protocol),
            ),
            (
                "token",
                actions(&[0, TAG_TOKEN | TAG_OBJECT], &[]),
                Some(GlobalBucket::Token),
            ),
            (
                "object",
                actions(&[TAG_OBJECT | TAG_IDENTITY], &[]),
                Some(GlobalBucket::Object),
            ),
            (
                "identity",
                actions(&[TAG_IDENTITY], &[]),
                Some(GlobalBucket::Identity),
            ),
            (
                "type call tag",
                actions(&[TAG_TYPE_CALL], &[]),
                Some(GlobalBucket::Script),
            ),
            (
                "lock call tag",
                actions(&[TAG_LOCK_CALL], &[]),
                Some(GlobalBucket::Script),
            ),
            ("type call entry", type_call, Some(GlobalBucket::Script)),
            ("lock call entry", lock_call, Some(GlobalBucket::Script)),
            ("plain ckb", actions(&[0, 0], &[]), Some(GlobalBucket::Ckb)),
            (
                "no participants",
                actions(&[], &[]),
                Some(GlobalBucket::Ckb),
            ),
        ]
    }

    #[test]
    fn classification_is_pinned_to_the_pre_move_api_semantics() {
        for (name, fixture, expected) in pinned_fixtures() {
            let got = classify_global_bucket(&fixture);
            assert_eq!(got, expected, "fixture `{name}`");
            let legacy = legacy_api_classify(&fixture);
            match got {
                Some(bucket) => assert_eq!(bucket.as_filter(), legacy, "fixture `{name}`"),
                None => assert_eq!(legacy, "cellbase", "fixture `{name}`"),
            }
        }
    }

    /// The identity-standard protocols — `.bit` (`dotbit:*`) and `.cell`
    /// (`dotcell:*`) — classify a transaction as Identity, not as a generic
    /// protocol: their actions are about one name each. This is the one
    /// deliberate departure from the pre-move classifier, which put every
    /// `.cell` transaction in Protocol (the legacy oracle says "protocol" for
    /// both fixtures below). DAO still takes precedence; any other protocol
    /// beside an identity action does not.
    #[test]
    fn identity_protocol_actions_classify_as_identity_not_protocol() {
        for protocol in ["dotbit", "dotcell"] {
            let with_item = actions(&[TAG_IDENTITY | TAG_PROTOCOL], &[protocol]);
            assert_eq!(legacy_api_classify(&with_item), "protocol", "{protocol}");
            assert_eq!(
                classify_global_bucket(&with_item),
                Some(GlobalBucket::Identity),
                "{protocol}"
            );
            // An in-place edit moves no item, so no TAG_IDENTITY: the action
            // alone decides.
            assert_eq!(
                classify_global_bucket(&actions(&[TAG_PROTOCOL], &[protocol])),
                Some(GlobalBucket::Identity),
                "{protocol}"
            );
            assert_eq!(
                classify_global_bucket(&actions(&[TAG_PROTOCOL], &[protocol, "rgbpp"])),
                Some(GlobalBucket::Identity),
                "{protocol} + another protocol"
            );
            assert_eq!(
                classify_global_bucket(&actions(&[TAG_PROTOCOL], &[protocol, "dao"])),
                Some(GlobalBucket::Dao),
                "{protocol} + dao"
            );
        }
    }

    #[test]
    fn bucket_bytes_and_filters_round_trip_in_key_order() {
        let bytes: Vec<u8> = GlobalBucket::ALL.iter().map(|b| b.as_byte()).collect();
        assert_eq!(bytes, vec![1, 2, 3, 4, 5, 6, 7]);
        for bucket in GlobalBucket::ALL {
            assert_eq!(GlobalBucket::from_byte(bucket.as_byte()).unwrap(), bucket);
            assert_eq!(GlobalBucket::from_filter(bucket.as_filter()), Some(bucket));
        }
        for bad in [0u8, 8, 0xFF] {
            let err = GlobalBucket::from_byte(bad).unwrap_err().to_string();
            assert!(err.contains(&bad.to_string()), "{err}");
        }
        assert_eq!(GlobalBucket::from_filter("cellbase"), None);
        assert_eq!(GlobalBucket::from_filter("all"), None);
    }

    #[test]
    fn bucket_key_is_derived_from_the_tx_actions_position() {
        let fixture = actions(&[TAG_TOKEN], &[]);
        let key = tx_actions_bucket_key(&fixture).expect("non-cellbase tx has a key");
        assert_eq!(
            crate::keys::decode_tx_actions_by_bucket_key(&key).unwrap(),
            (GlobalBucket::Token, 100, 1)
        );
        let mut cellbase = fixture;
        cellbase.is_cellbase = true;
        assert_eq!(tx_actions_bucket_key(&cellbase), None);
    }
}
