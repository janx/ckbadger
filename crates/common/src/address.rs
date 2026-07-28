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
    fn test_testnet_hrp() {
        let code_hash = [0u8; 32];
        let address = script_to_address(&code_hash, 1, &[], "testnet").unwrap();
        assert!(address.starts_with("ckt1"));
    }

    #[test]
    fn test_rejects_bad_code_hash_and_hash_type() {
        assert!(script_to_address(&[0u8; 31], 1, &[], "mainnet").is_err());
        assert!(script_to_address(&[0u8; 32], 3, &[], "mainnet").is_err());
    }
}
