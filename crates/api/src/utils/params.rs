//! The single parser for user-supplied numeric identifiers and pagination cursors.
//!
//! Companion to [`super::hash`], and a hard API-boundary requirement for the
//! same reason. Store key encoders assert domain invariants that request data
//! must never violate:
//!
//! * `keys::encode_block_num` and `keys::encode_desc_block_num` assert the
//!   block number is non-negative (genesis is block 0).
//! * `keys::encode_desc_tx_idx` asserts the transaction index is non-negative.
//! * `keys::encode_outpoint` asserts the output index is non-negative, and the
//!   outpoint key encodes it in 2 bytes, so it must also fit an `i16`.
//!
//! The release profile builds with `panic = "abort"` and the router installs no
//! catch-panic layer, so a value that violates one of those asserts terminates
//! the whole API process instead of failing the one request that carried it.
//! A single unauthenticated `?cursor=-1:0` is therefore a remote kill switch
//! unless every numeric identifier is checked here first.
//!
//! Silently *repairing* a bad value is equally forbidden (`CLAUDE.md`: "Never
//! hide invariant violations with silent fallbacks, lower-bound clamps, or
//! default-zero repairs"). Two concrete ways that has bitten this API:
//!
//! * A wrapping narrowing cast (`output_index as i16`) turns output 65536 into
//!   output 0 and answers with a *different* cell under the requested label.
//! * A cursor parser returning `Option` and a caller writing
//!   `.and_then(decode_cursor)` turns a malformed cursor into "no cursor", so
//!   the client silently gets page 1 forever instead of an error.
//!
//! So every parser here returns `Result` and every caller must propagate it.

use crate::response::ApiRouteError;
use crate::ApiError;

/// Largest output index an outpoint key can represent.
///
/// `keys::encode_outpoint` stores the index in 2 bytes as an `i16`, so this is
/// a storage-format limit, not a protocol limit: anything wider cannot be
/// addressed and must be rejected rather than narrowed.
pub const MAX_OUTPUT_INDEX: i32 = i16::MAX as i32;

/// Whether `value` is a well-formed block number.
///
/// Genesis is block 0, so the chain never has a negative block number. Callers
/// that must answer "is this arbitrary text a block number?" — free-text search,
/// for one — use this predicate; callers holding a value that is *required* to
/// be a block number use [`validate_block_number`] to get the 400 instead.
pub fn is_valid_block_number(value: i64) -> bool {
    value >= 0
}

/// Reject a block number that the chain cannot have.
pub fn validate_block_number(value: i64, field: &str) -> Result<i64, ApiRouteError> {
    if !is_valid_block_number(value) {
        return Err(ApiError::bad_request(format!(
            "Invalid {field}: expected a non-negative block number (genesis is block 0), got {value}"
        )));
    }
    Ok(value)
}

/// Parse a user-supplied block number from a path segment or query parameter.
pub fn parse_block_number(raw: &str, field: &str) -> Result<i64, ApiRouteError> {
    let value = raw.parse::<i64>().map_err(|_| {
        ApiError::bad_request(format!(
            "Invalid {field}: expected a non-negative block number, got {raw:?}"
        ))
    })?;
    validate_block_number(value, field)
}

/// Narrow a user-supplied output index to the width an outpoint key stores.
///
/// Returns `400` rather than truncating: `65536 as i16` is `0`, which would
/// serve output 0's body as though it were output 65536.
pub fn parse_output_index(value: i32, field: &str) -> Result<i16, ApiRouteError> {
    if !(0..=MAX_OUTPUT_INDEX).contains(&value) {
        return Err(ApiError::bad_request(format!(
            "Invalid {field}: expected an output index in 0..={MAX_OUTPUT_INDEX}, got {value}"
        )));
    }
    Ok(value as i16)
}

/// Parse a `"<block_number>:<tx_index>"` pagination cursor.
///
/// This is the one parser for that cursor shape. It previously existed as four
/// near-copies (`activities`, `tokens`, `assets`, `response::decode_cursor`),
/// three of which returned `Option` and none of which checked the sign, so the
/// same `?cursor=-1:0` aborted the process through four different routes.
///
/// `field` names the cursor in the error so a client paging several lists in
/// one view can tell which cursor it corrupted.
pub fn parse_block_tx_cursor(raw: &str, field: &str) -> Result<(i64, i32), ApiRouteError> {
    let invalid = |detail: &str| {
        ApiError::bad_request(format!(
            "Invalid {field}: expected \"<block_number>:<tx_index>\" with non-negative values, {detail}"
        ))
    };

    let (block_str, tx_str) = raw
        .split_once(':')
        .ok_or_else(|| invalid(&format!("got {raw:?}")))?;

    let block_num = block_str
        .parse::<i64>()
        .map_err(|_| invalid(&format!("got a non-numeric block number {block_str:?}")))?;
    let tx_idx = tx_str
        .parse::<i32>()
        .map_err(|_| invalid(&format!("got a non-numeric transaction index {tx_str:?}")))?;

    if block_num < 0 {
        return Err(invalid(&format!("got block number {block_num}")));
    }
    if tx_idx < 0 {
        return Err(invalid(&format!("got transaction index {tx_idx}")));
    }

    Ok((block_num, tx_idx))
}

/// Parse an optional `"<block_number>:<tx_index>"` cursor.
///
/// Absent stays absent (page 1); present-but-malformed is an error. The point
/// of the helper is that `Option<String> -> Option<(i64, i32)>` is exactly the
/// shape that invites `.and_then(...)` and silently swallows the second case.
///
/// An *empty* cursor also means page 1. That is deliberate rather than lenient:
/// `?cursor=` is how a query string spells "absent" when a client interpolates
/// `cursor ?? ''`, it is the value `encode_cursor` never produces, and it
/// cannot reach a key encoder. Routes used to decide this individually — some
/// accepted `Some("")`, others would have rejected it — so it is settled here
/// once instead of drifting per route.
pub fn parse_optional_block_tx_cursor(
    raw: Option<&str>,
    field: &str,
) -> Result<Option<(i64, i32)>, ApiRouteError> {
    match raw {
        None | Some("") => Ok(None),
        Some(cursor) => Ok(Some(parse_block_tx_cursor(cursor, field)?)),
    }
}

/// Parse a `"<block_number>:<0x-object-id>"` pagination cursor.
///
/// This is the one parser for the composite cursor used by the spore list
/// endpoints (`/spore/objects`, `/spore/clusters`,
/// `/spore/clusters/{id}/spores`, `/spore/owner/{lock_hash}`), which page over
/// `(created_at_block DESC, id ASC)`. Their previous cursor was the block
/// number alone, resumed with a strict `created_at_block < cursor` comparison
/// — that skips every remaining entry of the cursor's block, and a mainnet
/// block can hold more spores than the page-size cap, so those rows were
/// irrecoverably unlistable (5,806 of 37,291 live spores). The composite
/// cursor names the exact row instead.
///
/// The id part must be the `0x`-prefixed 32-byte hex id exactly as emitted in
/// `nextCursor`. The legacy numeric-only form is rejected rather than
/// reinterpreted: treating `"100"` as "block 100, first id" would silently
/// resume at a *different* row than the client's stale cursor named.
pub fn parse_block_id_cursor(raw: &str, field: &str) -> Result<(i64, Vec<u8>), ApiRouteError> {
    let invalid = |detail: &str| {
        ApiError::bad_request(format!(
            "Invalid {field}: expected \"<block_number>:<0x-object-id>\" with a non-negative \
             block number and a 0x-prefixed 32-byte hex id, {detail}"
        ))
    };

    let (block_str, id_str) = raw
        .split_once(':')
        .ok_or_else(|| invalid(&format!("got {raw:?}")))?;

    let block_num = block_str
        .parse::<i64>()
        .map_err(|_| invalid(&format!("got a non-numeric block number {block_str:?}")))?;
    if block_num < 0 {
        return Err(invalid(&format!("got block number {block_num}")));
    }

    let id_hex = id_str
        .strip_prefix("0x")
        .ok_or_else(|| invalid(&format!("got an object id without 0x prefix {id_str:?}")))?;
    let id =
        hex::decode(id_hex).map_err(|_| invalid(&format!("got a non-hex object id {id_str:?}")))?;
    if id.len() != super::hash::HASH32_LEN {
        return Err(invalid(&format!(
            "got an object id of {} bytes instead of {}",
            id.len(),
            super::hash::HASH32_LEN
        )));
    }

    Ok((block_num, id))
}

/// Parse an optional `"<block_number>:<0x-object-id>"` cursor.
///
/// Same contract as [`parse_optional_block_tx_cursor`]: absent and empty both
/// mean page 1, present-but-malformed is an error — never "no cursor".
pub fn parse_optional_block_id_cursor(
    raw: Option<&str>,
    field: &str,
) -> Result<Option<(i64, Vec<u8>)>, ApiRouteError> {
    match raw {
        None | Some("") => Ok(None),
        Some(cursor) => Ok(Some(parse_block_id_cursor(cursor, field)?)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::StatusCode;

    fn message(err: ApiRouteError) -> String {
        err.1 .0.message.clone()
    }

    #[test]
    fn test_validate_block_number_accepts_genesis_and_positive() {
        assert_eq!(validate_block_number(0, "block number").unwrap(), 0);
        assert_eq!(
            validate_block_number(i64::MAX, "block number").unwrap(),
            i64::MAX
        );
    }

    #[test]
    fn test_validate_block_number_rejects_negative() {
        let err = validate_block_number(-1, "block number").unwrap_err();
        assert_eq!(err.0, StatusCode::BAD_REQUEST);
        let msg = message(err);
        assert!(msg.contains("block number"), "{msg}");
        assert!(msg.contains("got -1"), "{msg}");
    }

    #[test]
    fn test_parse_block_number_rejects_negative_and_non_numeric() {
        assert!(parse_block_number("-1", "block number").is_err());
        assert!(parse_block_number("abc", "block number").is_err());
        assert!(parse_block_number("", "block number").is_err());
        assert_eq!(parse_block_number("0", "block number").unwrap(), 0);
        assert_eq!(parse_block_number("42", "block number").unwrap(), 42);
    }

    #[test]
    fn test_is_valid_block_number_matches_validate() {
        for value in [i64::MIN, -1, 0, 1, i64::MAX] {
            assert_eq!(
                is_valid_block_number(value),
                validate_block_number(value, "b").is_ok(),
                "value={value}"
            );
        }
    }

    #[test]
    fn test_parse_output_index_accepts_the_storable_range() {
        assert_eq!(parse_output_index(0, "output index").unwrap(), 0);
        assert_eq!(
            parse_output_index(MAX_OUTPUT_INDEX, "output index").unwrap(),
            i16::MAX
        );
    }

    /// The wrapping cast this replaces mapped 65536 to 0 and served a different
    /// cell under the requested index.
    #[test]
    fn test_parse_output_index_rejects_values_that_would_alias_another_cell() {
        for value in [MAX_OUTPUT_INDEX + 1, 65536, i32::MAX] {
            let err = parse_output_index(value, "output index").unwrap_err();
            assert_eq!(err.0, StatusCode::BAD_REQUEST, "value={value}");
            let msg = message(err);
            assert!(msg.contains(&value.to_string()), "{msg}");
        }
    }

    #[test]
    fn test_parse_output_index_rejects_negative() {
        let err = parse_output_index(-1, "output index").unwrap_err();
        assert_eq!(err.0, StatusCode::BAD_REQUEST);
        assert!(message(err).contains("got -1"));
    }

    #[test]
    fn test_parse_block_tx_cursor_roundtrips_valid_input() {
        assert_eq!(parse_block_tx_cursor("0:0", "cursor").unwrap(), (0, 0));
        assert_eq!(
            parse_block_tx_cursor("1234:56", "cursor").unwrap(),
            (1234, 56)
        );
    }

    #[test]
    fn test_parse_block_tx_cursor_rejects_negative_components() {
        for raw in ["-1:0", "0:-1", "-1:-1"] {
            let err = parse_block_tx_cursor(raw, "activity cursor").unwrap_err();
            assert_eq!(err.0, StatusCode::BAD_REQUEST, "raw={raw}");
            let msg = message(err);
            assert!(msg.contains("activity cursor"), "{msg}");
            assert!(msg.contains("non-negative"), "{msg}");
        }
    }

    #[test]
    fn test_parse_block_tx_cursor_rejects_malformed_shapes() {
        for raw in ["", "abc", "1", "1:2:3", "abc:def", ":", "1:", ":2"] {
            assert!(
                parse_block_tx_cursor(raw, "cursor").is_err(),
                "expected {raw:?} to be rejected"
            );
        }
    }

    #[test]
    fn test_parse_optional_block_tx_cursor_distinguishes_absent_from_malformed() {
        assert_eq!(
            parse_optional_block_tx_cursor(None, "cursor").unwrap(),
            None
        );
        assert_eq!(
            parse_optional_block_tx_cursor(Some("7:8"), "cursor").unwrap(),
            Some((7, 8))
        );
        assert!(parse_optional_block_tx_cursor(Some("-1:0"), "cursor").is_err());
        assert!(parse_optional_block_tx_cursor(Some("zzz"), "cursor").is_err());
    }

    /// `?cursor=` is how a client spells "page 1"; only the strict parser
    /// rejects it.
    #[test]
    fn test_empty_cursor_means_page_one_but_is_not_a_valid_cursor() {
        assert_eq!(
            parse_optional_block_tx_cursor(Some(""), "cursor").unwrap(),
            None
        );
        assert!(parse_block_tx_cursor("", "cursor").is_err());
    }

    #[test]
    fn test_parse_block_id_cursor_roundtrips_valid_input() {
        let id_hex = "ab".repeat(32);
        assert_eq!(
            parse_block_id_cursor(&format!("0:0x{id_hex}"), "cursor").unwrap(),
            (0, vec![0xAB; 32])
        );
        assert_eq!(
            parse_block_id_cursor(&format!("1234:0x{id_hex}"), "cursor").unwrap(),
            (1234, vec![0xAB; 32])
        );
    }

    #[test]
    fn test_parse_block_id_cursor_rejects_negative_block() {
        let raw = format!("-3:0x{}", "ab".repeat(32));
        let err = parse_block_id_cursor(&raw, "spore list cursor").unwrap_err();
        assert_eq!(err.0, StatusCode::BAD_REQUEST);
        let msg = message(err);
        assert!(msg.contains("spore list cursor"), "{msg}");
        assert!(msg.contains("-3"), "{msg}");
    }

    /// The legacy numeric-only block cursor is rejected, not reinterpreted.
    #[test]
    fn test_parse_block_id_cursor_rejects_legacy_numeric_form() {
        let err = parse_block_id_cursor("100", "cursor").unwrap_err();
        assert_eq!(err.0, StatusCode::BAD_REQUEST);
        assert!(message(err).contains("\"100\""));
    }

    #[test]
    fn test_parse_block_id_cursor_rejects_malformed_shapes() {
        let ok_id = format!("0x{}", "ab".repeat(32));
        let no_prefix_id = "ab".repeat(32);
        for raw in [
            String::new(),
            "abc".to_string(),
            "1:zz".to_string(),
            format!("1:2:{ok_id}"),
            "1:".to_string(),
            ":0xab".to_string(),
            "1:0x".to_string(),
            "1:0xabcd".to_string(),             // 2 bytes, not 32
            format!("1:{no_prefix_id}"),        // missing 0x prefix
            format!("1:0x{}", "ab".repeat(33)), // 33 bytes
        ] {
            let err = parse_block_id_cursor(&raw, "cursor").unwrap_err();
            assert_eq!(err.0, StatusCode::BAD_REQUEST, "raw={raw:?}");
        }
    }

    #[test]
    fn test_parse_optional_block_id_cursor_distinguishes_absent_from_malformed() {
        assert_eq!(
            parse_optional_block_id_cursor(None, "cursor").unwrap(),
            None
        );
        assert_eq!(
            parse_optional_block_id_cursor(Some(""), "cursor").unwrap(),
            None
        );
        let raw = format!("7:0x{}", "cd".repeat(32));
        assert_eq!(
            parse_optional_block_id_cursor(Some(&raw), "cursor").unwrap(),
            Some((7, vec![0xCD; 32]))
        );
        assert!(parse_optional_block_id_cursor(Some("100"), "cursor").is_err());
        assert!(parse_optional_block_id_cursor(Some("zzz"), "cursor").is_err());
    }
}
