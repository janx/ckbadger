use axum::{http::StatusCode, Json};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize)]
pub struct ApiError {
    pub error: String,
    pub message: String,
}

impl ApiError {
    pub fn new(error: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            error: error.into(),
            message: message.into(),
        }
    }

    pub fn not_found(message: impl Into<String>) -> (StatusCode, Json<Self>) {
        (StatusCode::NOT_FOUND, Json(Self::new("not_found", message)))
    }

    pub fn bad_request(message: impl Into<String>) -> (StatusCode, Json<Self>) {
        (
            StatusCode::BAD_REQUEST,
            Json(Self::new("bad_request", message)),
        )
    }

    pub fn internal(message: impl Into<String>) -> (StatusCode, Json<Self>) {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(Self::new("internal_error", message)),
        )
    }

    /// A state that only exists while the indexer is still starting up, and that
    /// resolves on its own as sync progresses (e.g. the genesis baseline or the
    /// first daily snapshot has not been written yet).
    ///
    /// Must stay 503 + `initializing`: that pair is the contract the pre-sync
    /// router serves and the one the SPA's `isNetworkInitializingError` keys its
    /// retry-with-banner UX on. A 500 here reads as a server fault and gets the
    /// error screen instead.
    pub fn initializing(message: impl Into<String>) -> (StatusCode, Json<Self>) {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(Self::new("initializing", message)),
        )
    }

    pub fn warmup_pending(message: impl Into<String>) -> (StatusCode, Json<Self>) {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(Self::new("warmup_pending", message)),
        )
    }

    /// The resource exists but cannot be served YET: something it is derived
    /// from has not reached the node (e.g. an uncommitted transaction's
    /// parent). Transient by definition, so the caller retries — the response
    /// never carries partial data in its place.
    pub fn service_unavailable(message: impl Into<String>) -> (StatusCode, Json<Self>) {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(Self::new("service_unavailable", message)),
        )
    }

    pub fn unauthorized(message: impl Into<String>) -> (StatusCode, Json<Self>) {
        (
            StatusCode::UNAUTHORIZED,
            Json(Self::new("unauthorized", message)),
        )
    }
}

/// Cursor-based pagination response for efficient large dataset traversal.
/// Uses keyset pagination instead of OFFSET for O(1) page access.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CursorPaginatedResponse<T> {
    pub data: Vec<T>,
    /// Total count of items. Null when count is expensive (large table scans).
    /// Use pre-aggregated counts from sync:status or address_balances when available.
    ///
    /// Always the COMMITTED count. Provisional tx-pool rows are reported beside
    /// it in `pool`, never added to it: chain truth and unconfirmed state are
    /// two different facts.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total: Option<i64>,
    pub limit: i64,
    pub has_more: bool,
    /// Cursor for the next page (null if no more data)
    pub next_cursor: Option<String>,
    /// State of the tx-pool segment on this page. Present only on page one of
    /// the endpoints that serve pool rows; pool rows never enter a cursor.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pool: Option<crate::pool::PoolSummaryResponse>,
}

impl<T: Serialize + Clone> CursorPaginatedResponse<T> {
    pub fn new(data: Vec<T>, total: i64, limit: i64, next_cursor: Option<String>) -> Self {
        let has_more = next_cursor.is_some();
        Self {
            data,
            total: Some(total),
            limit,
            has_more,
            next_cursor,
            pool: None,
        }
    }

    /// Create a response without a total count (avoids expensive COUNT queries).
    /// has_more is determined by fetching limit+1 rows.
    pub fn without_total(data: Vec<T>, limit: i64, next_cursor: Option<String>) -> Self {
        let has_more = next_cursor.is_some();
        Self {
            data,
            total: None,
            limit,
            has_more,
            next_cursor,
            pool: None,
        }
    }

    /// Attach the tx-pool segment's state to a page-one response.
    pub fn with_pool(mut self, pool: crate::pool::PoolSummaryResponse) -> Self {
        self.pool = Some(pool);
        self
    }
}

/// Encode cursor from block_number and index (for transactions, cells, etc.)
pub fn encode_cursor(block_number: i64, index: i32) -> String {
    format!("{}:{}", block_number, index)
}

/// Encode cursor from single i64 value (for tables with simple id ordering)
pub fn encode_cursor_single(id: i64) -> String {
    id.to_string()
}

/// Decode cursor to single i64 value
pub fn decode_cursor_single(cursor: &str) -> Option<i64> {
    cursor.parse().ok()
}

pub type ApiResult<T> = Result<Json<T>, (StatusCode, Json<ApiError>)>;

/// Common error type used by route handlers.
pub type ApiRouteError = (StatusCode, Json<ApiError>);

/// Sync status for WebSocket broadcasts and the statistics endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncStatusResponse {
    pub is_syncing: bool,
    pub synced_block: i64,
    pub tip_block: i64,
    pub progress: f64,
    pub estimated_time: Option<String>,
    pub chart_data_may_be_incomplete: bool,
    pub blocks_per_second: Option<f64>,
    pub ema_blocks_per_second: Option<f64>,
    pub txs_per_second: Option<f64>,
    pub ema_txs_per_second: Option<f64>,
    pub sync_mode: String,
    pub started_at: Option<i64>,
    pub elapsed_time: Option<String>,
    pub total_time: Option<String>,
}

pub fn ok<T: Serialize>(data: T) -> ApiResult<T> {
    Ok(Json(data))
}

/// Default pagination limit shared across all route modules.
pub fn default_limit() -> i64 {
    50
}

/// Map CKB script hash_type integer to its string representation.
/// Returns `None` for unknown hash_type values so callers must handle them explicitly.
pub fn hash_type_to_str(hash_type: i16) -> Option<&'static str> {
    u8::try_from(hash_type)
        .ok()
        .and_then(ckbadger_common::hash_type_label)
}

/// Shared script response type used by transaction, cell, and other route modules.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptResponse {
    pub code_hash: String,
    pub hash_type: String,
    pub args: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptFamilyListItemResponse {
    pub family_id: String,
    pub name: String,
    pub description: Option<String>,
    pub script_kind: Option<String>,
    pub deprecated: bool,
    pub website: Option<String>,
    pub live_cells_count: i64,
    pub cells_count: i64,
    pub owned_capacity_sum: String,
    pub owned_knowledge_sum: String,
    pub versions_count: i64,
    /// Classification of the current registered reference forms. A family may
    /// contain both pinned and replaceable references.
    pub code_classes: Vec<ScriptCodeClassTotalResponse>,
    /// The family's earliest code cell over all its versions (the same first
    /// deployment `/hardforks` marks); `None` when no version has an indexed
    /// code cell.
    pub first_deployed_at: Option<ScriptFirstDeploymentResponse>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptFirstDeploymentResponse {
    pub block_number: i64,
    /// Header timestamp of `blockNumber`, ms.
    pub timestamp_ms: i64,
    pub version_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptCodeClassTotalResponse {
    pub class: String,
    pub lock_cells_count: i64,
    pub lock_owned_capacity_sum: String,
    pub reference_count: i64,
    /// `classSummary` only: matching families with at least one reference of
    /// this class.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub family_count: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptFamiliesListResponse {
    #[serde(flatten)]
    pub page: CursorPaginatedResponse<ScriptFamilyListItemResponse>,
    /// All matching families, independent of the current cursor and limit.
    /// Counts and capacity cover lock use of every classified reference.
    pub class_summary: Vec<ScriptCodeClassTotalResponse>,
    pub role_counts: ScriptRoleCountsResponse,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptRoleCountsResponse {
    pub all: i64,
    pub locks: i64,
    pub types: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptCustodyResponse {
    /// `fixed`, `unspendable`, `one_key`, `multisig`, `unsupported`,
    /// `retired`, `mixed`, or `ambiguous`.
    pub class: String,
    pub lock_code_hash: Option<String>,
    pub lock_hash_type: Option<String>,
    pub lock_args: Option<String>,
    pub threshold: Option<u8>,
    pub key_count: Option<u8>,
    pub require_first_n: Option<u8>,
    pub key_hashes: Vec<String>,
    /// The exact spending transaction that supplied verified multisig config.
    pub verified_by_tx: Option<String>,
    /// `never_signed`, `verified`, `unavailable`, or `not_applicable`.
    pub witness_state: String,
    /// Every distinct current custodian for a type reference. Multiple
    /// identical-binary code cells may have different owner locks.
    pub owners: Vec<ScriptCustodyOwnerResponse>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptCustodyOwnerResponse {
    pub lock_code_hash: String,
    pub lock_hash_type: String,
    pub lock_args: String,
    pub class: String,
    pub code_cells: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptObservedReferenceResponse {
    pub reference_hash: String,
    pub hash_type: String,
    pub live_cells_count: i64,
    pub cells_count: i64,
    pub owned_capacity_sum: String,
    pub owned_knowledge_sum: String,
    pub current_version_hash: Option<String>,
    pub custody: ScriptCustodyResponse,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptVersionDeploymentResponse {
    pub hash_type: String,
    pub type_reference_hash: Option<String>,
    pub data_reference_hash: String,
    pub code_cell_tx_hash: String,
    pub code_cell_output_index: i32,
    pub deployed_at: i64,
    pub created_at_block: i64,
    pub status: String,
    pub consumed_at_block: Option<i64>,
    pub consumed_by_tx: Option<String>,
    pub custody: ScriptCustodyResponse,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptVersionDetailResponse {
    pub version_hash: String,
    pub name: String,
    pub description: Option<String>,
    pub script_kind: Option<String>,
    pub website: Option<String>,
    pub deprecated: bool,
    pub canonical_reference_hash: Option<String>,
    pub canonical_hash_type: Option<String>,
    pub deployed_at: Option<i64>,
    pub first_deployed_block: Option<i64>,
    pub first_deployed_tx_index: Option<i32>,
    pub live_cells_count: i64,
    pub cells_count: i64,
    pub owned_capacity_sum: String,
    pub owned_knowledge_sum: String,
    pub code_cells_live_count: i64,
    pub code_cells_total: i64,
    pub deployments: Vec<ScriptVersionDeploymentResponse>,
    pub references: Vec<ScriptObservedReferenceResponse>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptFamilyDetailResponse {
    pub family_id: String,
    pub name: String,
    pub description: Option<String>,
    pub script_kind: Option<String>,
    pub website: Option<String>,
    pub live_cells_count: i64,
    pub cells_count: i64,
    pub owned_capacity_sum: String,
    pub owned_knowledge_sum: String,
    pub versions_count: i64,
    pub versions: Vec<ScriptVersionDetailResponse>,
    /// Includes retired registered references that have no current version.
    pub references: Vec<ScriptObservedReferenceResponse>,
    pub code_classes: Vec<ScriptCodeClassTotalResponse>,
    pub history: Vec<ScriptHistoryMarkerResponse>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptHistoryMarkerResponse {
    pub reference_hash: String,
    pub hash_type: String,
    pub version_hash: Option<String>,
    pub event: String,
    pub block_number: i64,
    pub tx_index: Option<i32>,
    pub timestamp: i64,
    /// Monotonic indexed calendar clock used by the capacity chart.
    pub chain_time_ms: i64,
    pub indexed_day: String,
    pub code_cell_tx_hash: String,
    pub code_cell_output_index: i32,
    pub event_tx_hash: String,
    pub custody: ScriptCustodyResponse,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChartDataPoint {
    pub date: String,
    pub value: String,
    pub value2: Option<String>,
}

/// A chart response's series and labels. Serialized with each series' peak
/// (`peak` for `value`, `peak2` for a second numeric series `value2`),
/// derived from `data` by [`series_peak`] — never stored, so it cannot drift
/// from the points it summarizes (CH-21).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChartResponse {
    pub data: Vec<ChartDataPoint>,
    pub title: String,
    pub y_axis_label: String,
    pub y2_axis_label: Option<String>,
}

/// `y2AxisLabel` of a chart whose `value2` is a timeline marker
/// (`past | current | future`), not a numeric series.
pub const NOMINAL_APC_TIMELINE_LABEL: &str = "nominal-apc-timeline";

impl Serialize for ChartResponse {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::{Error, SerializeStruct};
        let context = |series: &str, e: String| {
            S::Error::custom(format!("chart {:?} {series} peak: {e}", self.title))
        };
        let peak = series_peak(
            self.data
                .iter()
                .map(|point| (point.date.as_str(), point.value.as_str())),
        )
        .map_err(|e| context("value", e))?;
        let peak2 = match self.y2_axis_label.as_deref() {
            Some(label) if label != NOMINAL_APC_TIMELINE_LABEL => series_peak(
                self.data
                    .iter()
                    .filter_map(|point| Some((point.date.as_str(), point.value2.as_deref()?))),
            )
            .map_err(|e| context("value2", e))?,
            _ => None,
        };
        let mut state = serializer.serialize_struct("ChartResponse", 6)?;
        state.serialize_field("data", &self.data)?;
        state.serialize_field("title", &self.title)?;
        state.serialize_field("yAxisLabel", &self.y_axis_label)?;
        state.serialize_field("y2AxisLabel", &self.y2_axis_label)?;
        state.serialize_field("peak", &peak)?;
        state.serialize_field("peak2", &peak2)?;
        state.end()
    }
}

/// The highest point of one chart series: its x label and its exact value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChartPeak {
    pub date: String,
    pub value: String,
}

/// The one peak computation of every chart series: the FIRST point holding
/// the greatest value, compared as exact decimals (no float rounding). `None`
/// for an empty series; a value that is not a plain decimal is an error.
pub fn series_peak<'a>(
    points: impl IntoIterator<Item = (&'a str, &'a str)>,
) -> Result<Option<ChartPeak>, String> {
    let mut best: Option<(&str, &str)> = None;
    for (date, value) in points {
        let greater = match best {
            None => {
                cmp_decimal(value, value)?;
                true
            }
            Some((_, current)) => cmp_decimal(value, current)? == std::cmp::Ordering::Greater,
        };
        if greater {
            best = Some((date, value));
        }
    }
    Ok(best.map(|(date, value)| ChartPeak {
        date: date.to_string(),
        value: value.to_string(),
    }))
}

/// Compare two plain decimal strings (`-?digits(.digits)?`) exactly.
fn cmp_decimal(a: &str, b: &str) -> Result<std::cmp::Ordering, String> {
    use std::cmp::Ordering;
    fn parts(raw: &str) -> Result<(bool, &str, &str), String> {
        let (negative, unsigned) = match raw.strip_prefix('-') {
            Some(rest) => (true, rest),
            None => (false, raw),
        };
        let (int, frac) = unsigned.split_once('.').unwrap_or((unsigned, ""));
        let valid = !int.is_empty()
            && int.bytes().all(|b| b.is_ascii_digit())
            && frac.bytes().all(|b| b.is_ascii_digit())
            && (frac.is_empty() == !unsigned.contains('.'));
        if !valid {
            return Err(format!("{raw:?} is not a plain decimal"));
        }
        let int = int.trim_start_matches('0');
        let frac = frac.trim_end_matches('0');
        let zero = int.is_empty() && frac.is_empty();
        Ok((negative && !zero, int, frac))
    }
    let magnitude = |(_, ai, af): (bool, &str, &str), (_, bi, bf): (bool, &str, &str)| {
        ai.len()
            .cmp(&bi.len())
            .then_with(|| ai.cmp(bi))
            .then_with(|| af.cmp(bf))
    };
    let (a, b) = (parts(a)?, parts(b)?);
    Ok(match (a.0, b.0) {
        (false, true) => Ordering::Greater,
        (true, false) => Ordering::Less,
        (false, false) => magnitude(a, b),
        (true, true) => magnitude(b, a),
    })
}

pub fn chart_response_has_data(response: &ChartResponse) -> bool {
    !response.data.is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn series_peak_is_the_first_greatest_exact_decimal() {
        let peak = |points: &[(&'static str, &'static str)]| {
            series_peak(points.iter().copied()).map(|p| p.map(|p| (p.date, p.value)))
        };
        assert_eq!(peak(&[]), Ok(None));
        // Beyond f64 precision: 9007199254740993 > 9007199254740992.
        assert_eq!(
            peak(&[
                ("a", "9007199254740992"),
                ("b", "9007199254740993"),
                ("c", "2")
            ]),
            Ok(Some(("b".into(), "9007199254740993".into())))
        );
        assert_eq!(
            peak(&[("a", "2.50"), ("b", "2.5"), ("c", "2.4999")]),
            Ok(Some(("a".into(), "2.50".into()))),
            "ties keep the first point"
        );
        assert_eq!(
            peak(&[("a", "-3"), ("b", "-0.5"), ("c", "-1")]),
            Ok(Some(("b".into(), "-0.5".into())))
        );
        assert_eq!(
            peak(&[("a", "0.0"), ("b", "-0")]),
            Ok(Some(("a".into(), "0.0".into())))
        );
        for malformed in ["", "1e9", "NaN", "1.", ".5", "1,000"] {
            assert!(peak(&[("a", malformed)]).is_err(), "{malformed:?}");
        }
    }

    #[test]
    fn test_cursor_paginated_response_with_total() {
        let resp = CursorPaginatedResponse::new(vec![1, 2, 3], 100, 10, Some("5:0".to_string()));
        assert_eq!(resp.total, Some(100));
        assert!(resp.has_more);
        assert_eq!(resp.limit, 10);
        assert_eq!(resp.data, vec![1, 2, 3]);
        assert_eq!(resp.next_cursor, Some("5:0".to_string()));
    }

    #[test]
    fn test_cursor_paginated_response_without_total() {
        let resp =
            CursorPaginatedResponse::without_total(vec![1, 2, 3], 10, Some("5:0".to_string()));
        assert_eq!(resp.total, None);
        assert!(resp.has_more);
    }

    #[test]
    fn test_cursor_paginated_response_without_total_no_more() {
        let resp = CursorPaginatedResponse::<i32>::without_total(vec![1, 2], 10, None);
        assert_eq!(resp.total, None);
        assert!(!resp.has_more);
        assert_eq!(resp.next_cursor, None);
    }

    #[test]
    fn test_without_total_serialization_omits_total() {
        let resp = CursorPaginatedResponse::without_total(vec!["a"], 10, None);
        let json = serde_json::to_value(&resp).unwrap();
        assert!(
            json.get("total").is_none(),
            "total should be omitted from JSON when None"
        );
        assert_eq!(json["hasMore"], false);
        assert_eq!(json["limit"], 10);
    }

    #[test]
    fn test_with_total_serialization_includes_total() {
        let resp = CursorPaginatedResponse::new(vec!["a"], 42, 10, None);
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["total"], 42);
    }

    /// `encode_cursor` writes the shape that `utils::params::parse_block_tx_cursor`
    /// reads. Rejection cases live with that parser; what matters here is that
    /// the two halves still agree.
    #[test]
    fn test_encode_cursor_roundtrips_through_the_shared_parser() {
        let cursor = encode_cursor(12345, 7);
        let (block, idx) = crate::utils::parse_block_tx_cursor(&cursor, "cursor").unwrap();
        assert_eq!(block, 12345);
        assert_eq!(idx, 7);
    }

    #[test]
    fn test_encode_decode_cursor_single_roundtrip() {
        let cursor = encode_cursor_single(99999);
        let id = decode_cursor_single(&cursor).unwrap();
        assert_eq!(id, 99999);
    }

    #[test]
    fn test_decode_cursor_single_invalid() {
        assert_eq!(decode_cursor_single("not_a_number"), None);
    }

    #[test]
    fn test_api_error_serialization() {
        let err = ApiError::new("test_error", "something went wrong");
        let json = serde_json::to_value(&err).unwrap();
        assert_eq!(json["error"], "test_error");
        assert_eq!(json["message"], "something went wrong");
    }

    #[test]
    fn test_default_limit() {
        assert_eq!(default_limit(), 50);
    }

    #[test]
    fn test_hash_type_to_str() {
        assert_eq!(hash_type_to_str(0), Some("data"));
        assert_eq!(hash_type_to_str(1), Some("type"));
        assert_eq!(hash_type_to_str(2), Some("data1"));
        assert_eq!(hash_type_to_str(4), Some("data2"));
        assert_eq!(hash_type_to_str(99), None);
    }

    #[test]
    fn test_script_response_serialization() {
        let script = ScriptResponse {
            code_hash: "0xabc".to_string(),
            hash_type: "type".to_string(),
            args: "0x1234".to_string(),
        };
        let json = serde_json::to_value(&script).unwrap();
        assert_eq!(json["codeHash"], "0xabc");
        assert_eq!(json["hashType"], "type");
        assert_eq!(json["args"], "0x1234");
    }
}
