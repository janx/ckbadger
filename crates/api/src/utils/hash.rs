//! The single parser for user-supplied 32-byte CKB hashes.
//!
//! Every script hash, code hash, transaction hash, block hash and 32-byte asset
//! identifier that arrives from a request (path segment, query parameter or JSON
//! body) must go through [`parse_hash32`] before it reaches the store.
//!
//! This is a hard API-boundary requirement, not a convenience:
//!
//! * **Too short** — store key encoders assert a 32-byte invariant
//!   (`keys::encode_outpoint`, `keys::encode_script_reference_key`,
//!   `keys::encode_cell_code_index_prefix`, …). A short hash reaching one of
//!   them panics, and the release profile builds with `panic = "abort"`, so the
//!   panic terminates the whole API process rather than one request.
//! * **Too long** — key encoders that copy a fixed 32-byte window would
//!   silently truncate the extra bytes and answer for a *different* hash.
//! * **Right length, wrong owner** — prefix-scan readers (`cell_by_lock`,
//!   `addr_txs`, …) take the hash as a raw key prefix, so a shorter value
//!   widens the scan into other addresses' rows.
//!
//! Store-side asserts stay exactly as they are: they encode an internal
//! invariant and must keep failing fast. This parser is what guarantees the
//! invariant is never violated by request data in the first place.

use crate::response::ApiRouteError;
use crate::ApiError;

/// Byte length of every CKB hash exposed through the API (blake2b-256).
pub const HASH32_LEN: usize = 32;

/// Parse a user-supplied 32-byte hash, with or without a `0x` prefix.
///
/// `field` names the request parameter and is echoed in the error so callers
/// can tell which of several hashes in one request was rejected.
///
/// Returns `400 Bad Request` for non-hex input and for any length other than
/// exactly 32 bytes, reporting both the expectation and what was received.
pub fn parse_hash32(raw: &str, field: &str) -> Result<Vec<u8>, ApiRouteError> {
    let stripped = raw.strip_prefix("0x").unwrap_or(raw);

    let bytes = hex::decode(stripped).map_err(|e| {
        ApiError::bad_request(format!(
            "Invalid {field}: expected a 32-byte hex hash (64 hex characters, optional 0x prefix), got invalid hex ({e})"
        ))
    })?;

    if bytes.len() != HASH32_LEN {
        return Err(ApiError::bad_request(format!(
            "Invalid {field}: expected {HASH32_LEN} bytes (64 hex characters, optional 0x prefix), got {} bytes",
            bytes.len()
        )));
    }

    Ok(bytes)
}

/// Parse a user-supplied asset identifier that is *not* a 32-byte hash.
///
/// Some on-chain asset IDs are shorter by construction — an mNFT class ID is 24
/// bytes, an mNFT token ID 28, a `.bit` account ID 20 — and the store keys them
/// by right-padding to a 32-byte window (`keys::pad_id_32`). That padding
/// asserts the ID is at most 32 bytes, because anything longer would be
/// truncated into another collection's key range; the assert would abort the
/// release binary (`panic = "abort"`) if it were ever reached with request data.
///
/// So this is the boundary check for the padded-ID family: hex, non-empty, and
/// never wider than the 32-byte key window. Use [`parse_hash32`] instead for
/// anything that is a real 32-byte hash.
pub fn parse_asset_id_max32(raw: &str, field: &str) -> Result<Vec<u8>, ApiRouteError> {
    let stripped = raw.strip_prefix("0x").unwrap_or(raw);

    let bytes = hex::decode(stripped).map_err(|e| {
        ApiError::bad_request(format!(
            "Invalid {field}: expected a hex identifier of at most {HASH32_LEN} bytes, got invalid hex ({e})"
        ))
    })?;

    if bytes.is_empty() || bytes.len() > HASH32_LEN {
        return Err(ApiError::bad_request(format!(
            "Invalid {field}: expected 1..={HASH32_LEN} bytes of hex, got {} bytes",
            bytes.len()
        )));
    }

    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::StatusCode;

    fn message(err: ApiRouteError) -> String {
        err.1 .0.message.clone()
    }

    #[test]
    fn test_parse_hash32_accepts_32_bytes_with_and_without_prefix() {
        let expected = vec![0xab; 32];
        assert_eq!(
            parse_hash32(&format!("0x{}", "ab".repeat(32)), "code_hash").unwrap(),
            expected
        );
        assert_eq!(
            parse_hash32(&"AB".repeat(32), "code_hash").unwrap(),
            expected
        );
    }

    #[test]
    fn test_parse_hash32_rejects_short_hash_with_actual_length() {
        let err = parse_hash32(&format!("0x{}", "ab".repeat(31)), "code_hash").unwrap_err();
        assert_eq!(err.0, StatusCode::BAD_REQUEST);
        let msg = message(err);
        assert!(msg.contains("code_hash"), "{msg}");
        assert!(msg.contains("expected 32 bytes"), "{msg}");
        assert!(msg.contains("got 31 bytes"), "{msg}");
    }

    #[test]
    fn test_parse_hash32_rejects_long_hash_with_actual_length() {
        let err = parse_hash32(&format!("0x{}", "ab".repeat(33)), "type_script_hash").unwrap_err();
        assert_eq!(err.0, StatusCode::BAD_REQUEST);
        let msg = message(err);
        assert!(msg.contains("type_script_hash"), "{msg}");
        assert!(msg.contains("got 33 bytes"), "{msg}");
    }

    #[test]
    fn test_parse_hash32_rejects_empty_input() {
        let err = parse_hash32("0x", "tx_hash").unwrap_err();
        assert_eq!(err.0, StatusCode::BAD_REQUEST);
        assert!(message(err).contains("got 0 bytes"));
    }

    #[test]
    fn test_parse_hash32_rejects_non_hex() {
        let err = parse_hash32("0xzz", "tx_hash").unwrap_err();
        assert_eq!(err.0, StatusCode::BAD_REQUEST);
        let msg = message(err);
        assert!(msg.contains("tx_hash"), "{msg}");
        assert!(msg.contains("invalid hex"), "{msg}");
    }

    #[test]
    fn test_parse_hash32_rejects_odd_length_hex() {
        let err = parse_hash32("0xabc", "lock_hash").unwrap_err();
        assert_eq!(err.0, StatusCode::BAD_REQUEST);
        assert!(message(err).contains("invalid hex"));
    }

    #[test]
    fn test_parse_asset_id_max32_accepts_short_on_chain_id_widths() {
        // mNFT class (24B), mNFT token (28B), .bit account (20B), padded 32B.
        for width in [20usize, 24, 28, 32] {
            let raw = format!("0x{}", "ab".repeat(width));
            assert_eq!(
                parse_asset_id_max32(&raw, "item ID").unwrap(),
                vec![0xab; width],
                "width={width}"
            );
        }
    }

    #[test]
    fn test_parse_asset_id_max32_rejects_ids_wider_than_the_key_window() {
        let err = parse_asset_id_max32(&format!("0x{}", "ab".repeat(33)), "item ID").unwrap_err();
        assert_eq!(err.0, StatusCode::BAD_REQUEST);
        let msg = message(err);
        assert!(msg.contains("item ID"), "{msg}");
        assert!(msg.contains("got 33 bytes"), "{msg}");
    }

    #[test]
    fn test_parse_asset_id_max32_rejects_empty_and_non_hex() {
        assert!(parse_asset_id_max32("0x", "item ID").is_err());
        assert!(parse_asset_id_max32("0xzz", "item ID").is_err());
    }
}
