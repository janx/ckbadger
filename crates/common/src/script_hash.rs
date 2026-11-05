//! The CKB script hash: blake2b-256 (CKB personalization) of the script's
//! Molecule encoding. The one routine in the workspace; the indexer's parser,
//! the API and the store's `LiveCellInfo` all call it. It streams the encoding
//! into the hasher, so it allocates nothing.

use ckb_hash::new_blake2b;

/// CKB script hash of `(code_hash, hash_type, args)`.
///
/// Script structure (Molecule table, RFC-0022):
/// ```text
/// table Script {
///     code_hash: Byte32,  // 32 bytes fixed
///     hash_type: byte,    // 1 byte
///     args: Bytes,        // variable: 4-byte length prefix + data
/// }
/// ```
pub fn script_hash(code_hash: &[u8], hash_type: u8, args: &[u8]) -> [u8; 32] {
    let args_len = u32::try_from(args.len())
        .unwrap_or_else(|_| panic!("script args length exceeds u32 range: {}", args.len()));

    // The Molecule encoding, fed to the hasher field by field instead of
    // being assembled in a buffer first.
    let mut hasher = new_blake2b();
    hasher.update(&molecule_script_header(args_len));
    hasher.update(code_hash); // Byte32
    hasher.update(&[hash_type]); // byte
    hasher.update(&args_len.to_le_bytes()); // Bytes length prefix
    hasher.update(args); // Bytes data

    let mut hash = [0u8; 32];
    hasher.finalize(&mut hash);
    hash
}

/// The Molecule table header of a script: total size, then the offsets of
/// `code_hash`, `hash_type` and `args`, each a little-endian u32.
fn molecule_script_header(args_len: u32) -> [u8; 16] {
    const HEADER_SIZE: u32 = 4 + 3 * 4; // total_size + 3 field offsets
    const CODE_HASH_SIZE: u32 = 32;
    const HASH_TYPE_SIZE: u32 = 1;

    let args_size = 4 + args_len; // Bytes = length prefix (4) + data
    let total_size = HEADER_SIZE + CODE_HASH_SIZE + HASH_TYPE_SIZE + args_size;
    let offset_code_hash = HEADER_SIZE;
    let offset_hash_type = offset_code_hash + CODE_HASH_SIZE;
    let offset_args = offset_hash_type + HASH_TYPE_SIZE;

    let mut header = [0u8; 16];
    header[0..4].copy_from_slice(&total_size.to_le_bytes());
    header[4..8].copy_from_slice(&offset_code_hash.to_le_bytes());
    header[8..12].copy_from_slice(&offset_hash_type.to_le_bytes());
    header[12..16].copy_from_slice(&offset_args.to_le_bytes());
    header
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_molecule_encoding_size() {
        let header = molecule_script_header(32);
        let word = |i: usize| u32::from_le_bytes(header[i..i + 4].try_into().unwrap());

        // Header: 4 (total) + 4*3 (offsets) = 16 bytes
        // Body: 32 (code_hash) + 1 (hash_type) + 4 (args len) + 32 (args) = 69 bytes
        // Total: 85 bytes
        assert_eq!(word(0), 85);
        assert_eq!((word(4), word(8), word(12)), (16, 48, 49));
    }

    /// Streaming the fields into the hasher hashes exactly the assembled
    /// Molecule bytes.
    #[test]
    fn script_hash_streams_the_molecule_encoding() {
        let code_hash = [0x9b; 32];
        let args = [0xab, 0xcd, 0xef];
        let mut encoded = molecule_script_header(3).to_vec();
        encoded.extend_from_slice(&code_hash);
        encoded.push(2);
        encoded.extend_from_slice(&3u32.to_le_bytes());
        encoded.extend_from_slice(&args);
        assert_eq!(encoded.len(), 16 + 32 + 1 + 4 + 3);
        let mut hasher = new_blake2b();
        hasher.update(&encoded);
        let mut expected = [0u8; 32];
        hasher.finalize(&mut expected);
        assert_eq!(script_hash(&code_hash, 2, &args), expected);
    }

    /// The SECP256K1/blake160 code cell's type script (TYPE_ID, `type`, its
    /// args) hashes to the well-known secp256k1 lock code hash, and a mainnet
    /// secp lock hashes to its known lock hash: the same pins the parser and
    /// the API held before this routine was shared.
    #[test]
    fn script_hash_matches_known_mainnet_hashes() {
        let type_id =
            hex::decode("00000000000000000000000000000000000000000000000000545950455f4944")
                .unwrap();
        let args = hex::decode("8536c9d5d908bd89fc70099e4284870708b6632356aad98734fcf43f6f71c304")
            .unwrap();
        assert_eq!(
            hex::encode(script_hash(&type_id, 1, &args)),
            "9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8"
        );

        let secp = hex::decode("9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8")
            .unwrap();
        let lock_args = hex::decode("0ce4053321ee9cda31640bf38b32768b3fc4f2b3").unwrap();
        assert_eq!(
            hex::encode(script_hash(&secp, 1, &lock_args)),
            "fc72c0f5d59f3191db00b946e0c28e398d954288f54e80f1460ea38a55e269d1"
        );
    }
}
