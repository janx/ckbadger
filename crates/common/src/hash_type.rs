//! CKB script `hash_type`: the ONE table between the JSON-RPC label and the
//! byte that goes into a script's molecule encoding (and so into its hash).
//!
//! The values are consensus, not a local convention: `data` = 0, `type` = 1,
//! `data1` = 2, `data2` = 4. There is no 3 — the byte is a bit field (bit 0 is
//! the "type" flag, the bits above it the VM version), so `data2` is `0b100`.
//! A wrong byte here yields a script hash no real address has, which is why
//! every crate in the workspace maps through this module and none keeps a
//! table of its own.

/// Every `hash_type` this table knows, as `(label, byte)`.
const HASH_TYPES: [(&str, u8); 4] = [("data", 0), ("type", 1), ("data1", 2), ("data2", 4)];

/// The byte for a JSON-RPC `hash_type` label, or `None` for a label CKB does
/// not define. Callers decide what an unknown label means for them (an error
/// with context, or an invariant violation) — it is never defaulted here.
pub fn hash_type_from_label(label: &str) -> Option<u8> {
    HASH_TYPES
        .iter()
        .find(|(known, _)| *known == label)
        .map(|(_, byte)| *byte)
}

/// The JSON-RPC label for a `hash_type` byte, or `None` for a byte CKB does
/// not define.
pub fn hash_type_label(byte: u8) -> Option<&'static str> {
    HASH_TYPES
        .iter()
        .find(|(_, known)| *known == byte)
        .map(|(label, _)| *label)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The upstream types are the oracle: every byte ckb-types accepts has a
    /// label here, that label is the one ckb-jsonrpc-types serializes, and the
    /// two directions invert each other.
    #[test]
    fn hash_type_table_round_trips_every_label() {
        let mut upstream_bytes = Vec::new();
        for byte in 0..=u8::MAX {
            let Ok(upstream) = ckb_types::core::ScriptHashType::try_from(byte) else {
                assert_eq!(
                    hash_type_label(byte),
                    None,
                    "byte {byte} is not a CKB hash_type, so it must have no label"
                );
                continue;
            };
            upstream_bytes.push(byte);

            let label = hash_type_label(byte)
                .unwrap_or_else(|| panic!("CKB hash_type byte {byte} has no label"));
            let json: ckb_jsonrpc_types::ScriptHashType = upstream.into();
            assert_eq!(
                serde_json::to_value(json).unwrap(),
                serde_json::Value::from(label),
                "label for byte {byte} must be the one the node's RPC uses"
            );
            assert_eq!(hash_type_from_label(label), Some(byte));
        }
        assert_eq!(
            upstream_bytes,
            HASH_TYPES.iter().map(|(_, byte)| *byte).collect::<Vec<_>>(),
            "the table must cover exactly the hash_types CKB defines"
        );
    }

    #[test]
    fn data2_is_four_not_three() {
        assert_eq!(hash_type_from_label("data2"), Some(4));
        assert_eq!(hash_type_label(3), None);
    }

    #[test]
    fn unknown_labels_are_none_not_a_default() {
        assert_eq!(hash_type_from_label("bogus"), None);
        assert_eq!(hash_type_from_label("Data"), None);
        assert_eq!(hash_type_from_label(""), None);
    }
}
