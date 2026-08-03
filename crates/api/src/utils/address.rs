use bech32::primitives::decode::{CheckedHrpstring, CheckedHrpstringError};
use bech32::{Bech32, Bech32m};
use ckb_hash::new_blake2b;

/// A lock script decoded from an RFC-0021 full CKB address.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddressLockScript {
    pub code_hash: Vec<u8>,
    pub hash_type: u8,
    pub args: Vec<u8>,
}

/// Decode an RFC-0021 full CKB address (`0x00 | code_hash | hash_type | args`)
/// into its lock script components.
///
/// This is the single address-parsing path for every API entry point that
/// accepts an address string. RFC-0021 mandates the Bech32m checksum for the
/// full (0x00) format, so the legacy Bech32 checksum is rejected — a payload
/// that only verifies under Bech32 is either a deprecated pre-2021 encoding or
/// a miscomputed checksum, never a valid full address. Case handling follows
/// the bech32 spec: all-lowercase and all-uppercase decode identically, mixed
/// case is rejected (by the bech32 crate itself).
pub fn parse_address_to_script(address: &str) -> Result<AddressLockScript, String> {
    let checked = match CheckedHrpstring::new::<Bech32m>(address) {
        Ok(checked) => checked,
        Err(err) => return Err(describe_non_bech32m_address(address, &err)),
    };

    let hrp = checked.hrp().to_lowercase();
    if hrp != "ckb" && hrp != "ckt" {
        return Err(format!("Invalid address prefix: {}", hrp));
    }

    let payload: Vec<u8> = checked.byte_iter().collect();

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

    let hash_type = payload[33];
    if !matches!(hash_type, 0x00 | 0x01 | 0x02 | 0x04) {
        return Err(format!(
            "Invalid hash_type byte: 0x{:02x} (RFC-0021 allows 0x00 data, 0x01 type, 0x02 data1, 0x04 data2)",
            hash_type
        ));
    }

    Ok(AddressLockScript {
        code_hash: payload[1..33].to_vec(),
        hash_type,
        args: payload[34..].to_vec(),
    })
}

/// Name the exact reason a non-Bech32m string was rejected. The Bech32 decode
/// below is NOT a fallback acceptance path — every branch returns an error —
/// it only distinguishes "deprecated bech32-checksum encoding" (short 0x01 /
/// pre-2021 full 0x02/0x04, and full-payload strings with a miscomputed
/// checksum) from plain garbage.
fn describe_non_bech32m_address(address: &str, bech32m_err: &CheckedHrpstringError) -> String {
    match CheckedHrpstring::new::<Bech32>(address) {
        Ok(legacy) => match legacy.byte_iter().next() {
            Some(0x00) => "invalid checksum: RFC-0021 full addresses (format 0x00) require the \
                           bech32m checksum, found legacy bech32"
                .to_string(),
            Some(format_type) => format!(
                "Only full address format (0x00) is supported, got: 0x{:02x} (deprecated bech32-checksum encoding)",
                format_type
            ),
            None => "Empty address payload".to_string(),
        },
        Err(_) => bech32m_err.to_string(),
    }
}

/// Compute lock script hash from a CKB address.
///
/// Per RFC-0022, script hash is `ckbhash(molecule_encode(script))`.
/// This function decodes the address through [`parse_address_to_script`] and
/// computes the hash using proper Molecule encoding.
pub fn address_to_lock_script_hash(address: &str) -> Result<Vec<u8>, String> {
    let script = parse_address_to_script(address)?;
    Ok(compute_script_hash(
        &script.code_hash,
        script.hash_type,
        &script.args,
    ))
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

/// Address-vs-hash routing: does `s` carry a CKB address HRP (`ckb1`/`ckt1`)?
///
/// Case-insensitive because the bech32 spec permits all-uppercase encodings;
/// mixed-case strings still route here and are then rejected by the decoder
/// itself, which is the accurate error (a mixed-case address is a broken
/// address, not a hex hash).
pub fn is_ckb_address(s: &str) -> bool {
    matches!(
        s.get(..4),
        Some(prefix) if prefix.eq_ignore_ascii_case("ckb1") || prefix.eq_ignore_ascii_case("ckt1")
    )
}

pub use ckbadger_common::script_to_address;

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

    /// Mainnet burn lock (secp sighash code hash, hash_type `type`, args = 20
    /// zero bytes): the valid RFC-0021 bech32m encoding, and the SAME payload
    /// wrapped in the legacy Bech32 checksum — the audited live probe
    /// (2026-08-01 agent E `format_probe_mainnet.txt`) that the API wrongly
    /// accepted and echoed back.
    const BURN_BECH32M: &str = "ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsqgqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqq5m759c";
    const BURN_WRONG_BECH32_CHECKSUM: &str = "ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsqgqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqp8wcq6";

    #[test]
    fn test_full_address_requires_bech32m_checksum() {
        // Sanity: the canonical bech32m form decodes.
        let hash = address_to_lock_script_hash(BURN_BECH32M).unwrap();
        assert_eq!(hash.len(), 32);

        // RFC-0021 mandates bech32m for the full (0x00) format; the same
        // payload under the legacy bech32 checksum is invalid.
        let err = address_to_lock_script_hash(BURN_WRONG_BECH32_CHECKSUM)
            .expect_err("legacy bech32 checksum on a full (0x00) address must be rejected");
        assert!(
            err.contains("bech32m"),
            "error must name the checksum requirement, got: {err}"
        );
    }

    #[test]
    fn test_uppercase_bech32m_address_decodes_to_same_hash() {
        let lower = address_to_lock_script_hash(BURN_BECH32M).unwrap();
        let upper = address_to_lock_script_hash(&BURN_BECH32M.to_uppercase())
            .expect("all-uppercase bech32m is legal per the bech32 case rules");
        assert_eq!(lower, upper);
    }

    #[test]
    fn test_mixed_case_address_rejected() {
        let mut mixed = BURN_BECH32M.to_string();
        mixed.replace_range(0..1, "C");
        assert!(
            address_to_lock_script_hash(&mixed).is_err(),
            "mixed-case bech32 strings must be rejected"
        );
    }

    #[test]
    fn test_is_ckb_address_case_insensitive_prefix() {
        assert!(is_ckb_address("ckb1abc"));
        assert!(is_ckb_address("ckt1abc"));
        assert!(is_ckb_address("CKB1ABC"));
        assert!(is_ckb_address("CKT1ABC"));
        assert!(!is_ckb_address("0x9bd7e06f"));
        assert!(!is_ckb_address("ck"));
        assert!(!is_ckb_address("bc1qqqq"));
    }

    #[test]
    fn test_parse_address_to_script_decodes_full_address() {
        let script = parse_address_to_script(BURN_BECH32M).unwrap();
        assert_eq!(
            hex::encode(&script.code_hash),
            "9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8"
        );
        assert_eq!(script.hash_type, 1);
        assert_eq!(script.args, vec![0u8; 20]);

        // Round trip: re-encoding the decoded script yields the canonical form.
        assert_eq!(
            script_to_address(
                &script.code_hash,
                script.hash_type as i16,
                &script.args,
                "mainnet"
            )
            .unwrap(),
            BURN_BECH32M
        );
    }

    #[test]
    fn test_parse_address_rejects_invalid_hash_type_byte() {
        // A syntactically valid bech32m string whose payload carries the
        // undefined hash_type 0x03: RFC-0021 allows only 0x00/0x01/0x02/0x04,
        // and no such lock can exist on chain.
        let mut payload = vec![0x00];
        payload.extend_from_slice(&[0x9b; 32]);
        payload.push(0x03);
        payload.extend_from_slice(&[0x11; 20]);
        let address =
            bech32::encode::<bech32::Bech32m>(bech32::Hrp::parse("ckb").unwrap(), &payload)
                .unwrap();

        let err = parse_address_to_script(&address).unwrap_err();
        assert!(
            err.contains("hash_type") && err.contains("0x03"),
            "must name the invalid hash_type byte, got: {err}"
        );
    }

    /// Deprecated encodings keep an error naming their format byte (audit
    /// probe vectors: short 0x01 and pre-2021 full 0x04, both bech32).
    #[test]
    fn test_deprecated_bech32_formats_report_format_byte() {
        let err = address_to_lock_script_hash("ckb1qyqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqp28vuq")
            .unwrap_err();
        assert!(
            err.contains("0x01"),
            "short-format rejection must name the format byte, got: {err}"
        );
        let err = address_to_lock_script_hash(
            "ckb1qjda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqq6ye5ql",
        )
        .unwrap_err();
        assert!(
            err.contains("0x04"),
            "old-full-format rejection must name the format byte, got: {err}"
        );
    }
}
