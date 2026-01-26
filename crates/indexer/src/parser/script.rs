use ckb_hash::new_blake2b;

use crate::rpc::{parse_hex_to_bytes, Script};

pub struct ScriptParser;

impl ScriptParser {
    /// Compute script hash using Molecule serialization.
    ///
    /// Script structure (Molecule table):
    /// ```text
    /// table Script {
    ///     code_hash: Byte32,  // 32 bytes fixed
    ///     hash_type: byte,    // 1 byte
    ///     args: Bytes,        // variable: 4-byte length prefix + data
    /// }
    /// ```
    ///
    /// Molecule table encoding:
    /// - Header: 4 bytes (total size) + 4 bytes per field (offsets)
    /// - Body: fields in declaration order
    pub fn compute_script_hash(script: &Script) -> Vec<u8> {
        let code_hash = parse_hex_to_bytes(&script.code_hash);
        let hash_type = Self::parse_hash_type(&script.hash_type);
        let args = parse_hex_to_bytes(&script.args);

        let encoded = Self::molecule_encode_script(&code_hash, hash_type, &args);

        let mut hasher = new_blake2b();
        hasher.update(&encoded);

        let mut hash = vec![0u8; 32];
        hasher.finalize(&mut hash);
        hash
    }

    fn molecule_encode_script(code_hash: &[u8], hash_type: u8, args: &[u8]) -> Vec<u8> {
        const HEADER_SIZE: u32 = 4 + 3 * 4; // total_size + 3 field offsets
        const CODE_HASH_SIZE: u32 = 32;
        const HASH_TYPE_SIZE: u32 = 1;

        let args_size = 4 + args.len() as u32; // Bytes = length prefix (4) + data
        let total_size = HEADER_SIZE + CODE_HASH_SIZE + HASH_TYPE_SIZE + args_size;

        let offset_code_hash = HEADER_SIZE;
        let offset_hash_type = offset_code_hash + CODE_HASH_SIZE;
        let offset_args = offset_hash_type + HASH_TYPE_SIZE;

        let mut buf = Vec::with_capacity(total_size as usize);

        // Header
        buf.extend_from_slice(&total_size.to_le_bytes());
        buf.extend_from_slice(&offset_code_hash.to_le_bytes());
        buf.extend_from_slice(&offset_hash_type.to_le_bytes());
        buf.extend_from_slice(&offset_args.to_le_bytes());

        // Body
        buf.extend_from_slice(code_hash); // Byte32
        buf.push(hash_type); // byte
        buf.extend_from_slice(&(args.len() as u32).to_le_bytes()); // Bytes length prefix
        buf.extend_from_slice(args); // Bytes data

        buf
    }

    pub fn compute_data_hash(data: &[u8]) -> Vec<u8> {
        let mut hasher = new_blake2b();
        hasher.update(data);

        let mut hash = vec![0u8; 32];
        hasher.finalize(&mut hash);
        hash
    }

    pub fn parse_hash_type(hash_type: &str) -> u8 {
        match hash_type {
            "data" => 0,
            "type" => 1,
            "data1" => 2,
            "data2" => 4,
            _ => 0,
        }
    }

    pub fn hash_type_to_i16(hash_type: &str) -> i16 {
        Self::parse_hash_type(hash_type) as i16
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn test_molecule_encoding_size() {
        let code_hash = [0u8; 32];
        let args = vec![0u8; 32];
        let encoded = ScriptParser::molecule_encode_script(&code_hash, 1, &args);

        // Header: 4 (total) + 4*3 (offsets) = 16 bytes
        // Body: 32 (code_hash) + 1 (hash_type) + 4 (args len) + 32 (args) = 69 bytes
        // Total: 85 bytes
        assert_eq!(encoded.len(), 85);
    }
}
