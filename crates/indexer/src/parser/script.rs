use ckb_hash::new_blake2b;

use crate::rpc::{parse_hex_to_bytes, Script};

/// TypeID script code_hash (all-zeros prefix + "TYPE_ID" ASCII).
pub const TYPE_ID_CODE_HASH: &str =
    "00000000000000000000000000000000000000000000000000545950455f4944";

/// The consensus type-id script, compared without allocating on the bulk path.
pub(crate) fn is_type_id(code_hash: &[u8], hash_type: i16) -> bool {
    hash_type == 1
        && code_hash.len() == 32
        && code_hash[..25] == [0; 25]
        && &code_hash[25..] == b"TYPE_ID"
}

pub struct ScriptParser;

impl ScriptParser {
    /// The CKB script hash of an RPC script (blake2b of its Molecule
    /// encoding; see [`ckbadger_common::script_hash`]).
    pub fn compute_script_hash(script: &Script) -> Vec<u8> {
        let code_hash = parse_hex_to_bytes(&script.code_hash);
        let hash_type = Self::parse_hash_type(&script.hash_type);
        let args = parse_hex_to_bytes(&script.args);
        Self::compute_script_hash_raw(&code_hash, hash_type, &args)
    }

    /// Compute script hash from raw byte components (no hex parsing), with
    /// the workspace's one script-hash routine.
    pub fn compute_script_hash_raw(code_hash: &[u8], hash_type: u8, args: &[u8]) -> Vec<u8> {
        ckbadger_common::script_hash(code_hash, hash_type, args).to_vec()
    }

    pub fn compute_data_hash(data: &[u8]) -> [u8; 32] {
        let mut hasher = new_blake2b();
        hasher.update(data);

        let mut hash = [0u8; 32];
        hasher.finalize(&mut hash);
        hash
    }

    /// The node validated this label before handing it over, so an unknown one
    /// is an invariant violation, not input to recover from. The table itself
    /// is the workspace's one: `ckbadger_common::hash_type`.
    pub fn parse_hash_type(hash_type: &str) -> u8 {
        ckbadger_common::hash_type_from_label(hash_type)
            .unwrap_or_else(|| unreachable!("node-validated hash_type: '{}'", hash_type))
    }

    pub fn hash_type_to_i16(hash_type: &str) -> i16 {
        Self::parse_hash_type(hash_type) as i16
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn type_id_filter_is_exact_and_does_not_accept_data_hash_references() {
        let hash = hex::decode(TYPE_ID_CODE_HASH).unwrap();
        assert!(is_type_id(&hash, 1));
        assert!(!is_type_id(&hash, 0));
        assert!(!is_type_id(&hash[1..], 1));
        let mut wrong = hash;
        wrong[0] = 1;
        assert!(!is_type_id(&wrong, 1));
    }

    #[test]
    fn test_secp256k1_blake160_type_script_hash() {
        // The SECP256K1/blake160 code cell (e2fb...#1) has type script:
        // code_hash = TYPE_ID
        // hash_type = type (1)
        // args = 0x8536c9d5d908bd89fc70099e4284870708b6632356aad98734fcf43f6f71c304
        //
        // Its type_script_hash should equal 0x9bd7e06f... (SECP256K1/blake160 code_hash)
        let script = Script {
            code_hash: "0x00000000000000000000000000000000000000000000000000545950455f4944"
                .to_string(),
            hash_type: "type".to_string(),
            args: "0x8536c9d5d908bd89fc70099e4284870708b6632356aad98734fcf43f6f71c304".to_string(),
        };

        let hash = ScriptParser::compute_script_hash(&script);
        let hash_hex = format!("0x{}", hex::encode(&hash));

        assert_eq!(
            hash_hex, "0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8",
            "SECP256K1/blake160 type script hash mismatch"
        );
    }

    #[test]
    fn test_parse_hash_type_known_types() {
        assert_eq!(ScriptParser::parse_hash_type("data"), 0);
        assert_eq!(ScriptParser::parse_hash_type("type"), 1);
        assert_eq!(ScriptParser::parse_hash_type("data1"), 2);
        assert_eq!(ScriptParser::parse_hash_type("data2"), 4);
    }
}
