//! CKB address encoding (RFC-0021 full address format).
//!
//! Single canonical encoder shared by the API (response rendering) and the
//! verify suite (cross-checking rendered addresses against node data).

use bech32::{Bech32m, Hrp};

/// Encode a lock script as an RFC-0021 full CKB address
/// (`0x00 | code_hash | hash_type | args`, bech32m).
///
/// `network` selects the HRP: `mainnet` → `ckb`, anything else → `ckt`.
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

    // The byte comes from the workspace's one hash_type table: a value that
    // table does not know (3, negatives, anything above 4) is not a CKB
    // hash_type and cannot be encoded into an address.
    let hash_type_byte = u8::try_from(hash_type)
        .ok()
        .filter(|byte| crate::hash_type_label(*byte).is_some())
        .ok_or_else(|| format!("Unknown hash_type: {}", hash_type))?;

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
    fn test_testnet_hrp() {
        let code_hash = [0u8; 32];
        let address = script_to_address(&code_hash, 1, &[], "testnet").unwrap();
        assert!(address.starts_with("ckt1"));
    }

    #[test]
    fn test_rejects_bad_code_hash_and_hash_type() {
        assert!(script_to_address(&[0u8; 31], 1, &[], "mainnet").is_err());
        assert!(script_to_address(&[0u8; 32], 3, &[], "mainnet").is_err());
        assert!(script_to_address(&[0u8; 32], -1, &[], "mainnet").is_err());
        assert!(script_to_address(&[0u8; 32], 256, &[], "mainnet").is_err());
    }

    /// Every hash_type the workspace table knows encodes, and the byte in the
    /// payload is the table's byte — `data2` is 4, never 3.
    #[test]
    fn test_encodes_every_known_hash_type_with_its_table_byte() {
        for (label, byte) in [("data", 0u8), ("type", 1), ("data1", 2), ("data2", 4)] {
            assert_eq!(crate::hash_type_from_label(label), Some(byte));
            let address = script_to_address(&[0u8; 32], i16::from(byte), &[], "mainnet")
                .unwrap_or_else(|e| panic!("{label} must encode: {e}"));
            let (_, payload) = bech32::decode(&address).unwrap();
            assert_eq!(payload[33], byte, "{label}");
        }
    }
}
