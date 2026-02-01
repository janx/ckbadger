use bech32::{Bech32m, Hrp};
use ckb_hash::new_blake2b;

/// Compute lock script hash from a CKB address.
///
/// Per RFC-0022, script hash is `ckbhash(molecule_encode(script))`.
/// This function decodes the address, extracts the lock script components,
/// and computes the hash using proper Molecule encoding.
pub fn address_to_lock_script_hash(address: &str) -> Result<Vec<u8>, String> {
    let (hrp, payload) = bech32::decode(address).map_err(|e| e.to_string())?;

    let hrp_str = hrp.as_str();
    if hrp_str != "ckb" && hrp_str != "ckt" {
        return Err(format!("Invalid address prefix: {}", hrp_str));
    }

    if payload.is_empty() {
        return Err("Empty address payload".to_string());
    }

    let format_type = payload[0];
    if format_type != 0x00 {
        return Err(format!(
            "Only full address format (0x00) is supported, got: 0x{:02x}",
            format_type
        ));
    }

    if payload.len() < 34 {
        return Err(format!(
            "Payload too short: expected at least 34 bytes, got {}",
            payload.len()
        ));
    }

    let code_hash = &payload[1..33];
    let hash_type = payload[33];
    let args = &payload[34..];

    Ok(compute_script_hash(code_hash, hash_type, args))
}

/// Compute script hash using Molecule serialization (RFC-0022).
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
pub fn compute_script_hash(code_hash: &[u8], hash_type: u8, args: &[u8]) -> Vec<u8> {
    let encoded = molecule_encode_script(code_hash, hash_type, args);

    let mut hasher = new_blake2b();
    hasher.update(&encoded);

    let mut hash = vec![0u8; 32];
    hasher.finalize(&mut hash);
    hash
}

fn molecule_encode_script(code_hash: &[u8], hash_type: u8, args: &[u8]) -> Vec<u8> {
    const HEADER_SIZE: u32 = 4 + 3 * 4;
    const CODE_HASH_SIZE: u32 = 32;
    const HASH_TYPE_SIZE: u32 = 1;

    let args_size = 4 + args.len() as u32;
    let total_size = HEADER_SIZE + CODE_HASH_SIZE + HASH_TYPE_SIZE + args_size;

    let offset_code_hash = HEADER_SIZE;
    let offset_hash_type = offset_code_hash + CODE_HASH_SIZE;
    let offset_args = offset_hash_type + HASH_TYPE_SIZE;

    let mut buf = Vec::with_capacity(total_size as usize);

    buf.extend_from_slice(&total_size.to_le_bytes());
    buf.extend_from_slice(&offset_code_hash.to_le_bytes());
    buf.extend_from_slice(&offset_hash_type.to_le_bytes());
    buf.extend_from_slice(&offset_args.to_le_bytes());

    buf.extend_from_slice(code_hash);
    buf.push(hash_type);
    buf.extend_from_slice(&(args.len() as u32).to_le_bytes());
    buf.extend_from_slice(args);

    buf
}

pub fn is_ckb_address(s: &str) -> bool {
    s.starts_with("ckb1") || s.starts_with("ckt1")
}

pub fn script_to_address(
    code_hash: &[u8],
    hash_type: i16,
    args: &[u8],
    network: &str,
) -> Result<String, String> {
    if code_hash.len() != 32 {
        return Err(format!(
            "Invalid code_hash length: expected 32, got {}",
            code_hash.len()
        ));
    }

    let hrp = match network {
        "mainnet" => Hrp::parse("ckb").expect("'ckb' is a valid HRP"),
        _ => Hrp::parse("ckt").expect("'ckt' is a valid HRP"),
    };

    let hash_type_byte = match hash_type {
        0 => 0x00,
        1 => 0x01,
        2 => 0x02,
        4 => 0x04,
        _ => return Err(format!("Unknown hash_type: {}", hash_type)),
    };

    // RFC-0021 full payload: 0x00 | code_hash (32) | hash_type (1) | args
    let mut payload = Vec::with_capacity(1 + 32 + 1 + args.len());
    payload.push(0x00);
    payload.extend_from_slice(code_hash);
    payload.push(hash_type_byte);
    payload.extend_from_slice(args);

    bech32::encode::<Bech32m>(hrp, &payload).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_full_address_encoding() {
        let code_hash =
            hex::decode("9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8")
                .unwrap();
        let args = hex::decode("b39bbc0b3673c7d36450bc14cfcdad2d559c6c64").unwrap();

        let address = script_to_address(&code_hash, 1, &args, "mainnet").unwrap();

        assert_eq!(
            address,
            "ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsqdnnw7qkdnnclfkg59uzn8umtfd2kwxceqxwquc4"
        );
    }

    #[test]
    fn test_testnet_address() {
        let code_hash =
            hex::decode("9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8")
                .unwrap();
        let args = hex::decode("b39bbc0b3673c7d36450bc14cfcdad2d559c6c64").unwrap();

        let address = script_to_address(&code_hash, 1, &args, "testnet").unwrap();

        assert!(address.starts_with("ckt"));
    }

    #[test]
    fn test_address_to_lock_script_hash() {
        let address = "ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsqgvusznxg0wnndrzeqt7w9nya5t8lz09vce7r9rz";
        let hash = address_to_lock_script_hash(address).unwrap();

        assert_eq!(
            hex::encode(&hash),
            "fc72c0f5d59f3191db00b946e0c28e398d954288f54e80f1460ea38a55e269d1"
        );
    }

    #[test]
    fn test_compute_script_hash_matches_indexer() {
        let code_hash =
            hex::decode("9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8")
                .unwrap();
        let hash_type: u8 = 1;
        let args = hex::decode("0ce4053321ee9cda31640bf38b32768b3fc4f2b3").unwrap();

        let hash = compute_script_hash(&code_hash, hash_type, &args);

        assert_eq!(
            hex::encode(&hash),
            "fc72c0f5d59f3191db00b946e0c28e398d954288f54e80f1460ea38a55e269d1"
        );
    }
}
