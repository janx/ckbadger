//! Bounded, read-only export of raw entity statistics for the verifier.
//!
//! `POST /api/v1/verify/entity-statistics` answers with the *stored* daily
//! deltas for a small set of named entities, together with the sync anchor and
//! the write-path state that decides whether those numbers mean anything yet.
//!
//! What makes this endpoint different from the public asset endpoints:
//!
//! * **One read view.** Every read happens inside the request's pin (see
//!   [`crate::pin_read_view`]), so the anchor and the rows it labels come from
//!   the same secondary view. A verifier paging across several requests would
//!   have no such guarantee.
//! * **Raw, not derived.** It returns what the writer stored, as exact decimal
//!   strings. It computes no new user-facing statistic and reads no warmup
//!   cache, so a disagreement points at the write path rather than at a second
//!   read path.
//! * **Bounded and honest about it.** Entity count, rows per entity and total
//!   bytes are capped. A request over a hard cap is a `400`; a legal request
//!   that cannot be exported completely comes back with `complete = false`
//!   rather than a silently truncated list.
//! * **Read-only.** It never writes, never triggers the indexer and never calls
//!   the node — the indexer stays the only writer of chain stores.

use axum::{extract::State, routing::post, Json, Router};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::response::{ok, ApiError, ApiResult, ApiRouteError};
use crate::utils::assets::accumulate_owned_capacity;
use crate::utils::hash::parse_hash32;
use crate::AppState;

/// Entities per request. The verifier samples entities and then verifies each
/// one exhaustively, so a wide request is a mistake, not a bigger sample.
const MAX_ENTITIES: usize = 16;
/// Daily rows per entity.
const MAX_DAILY_ROWS: usize = 8_192;
/// Approximate response budget for the daily rows themselves.
const MAX_RESPONSE_BYTES: usize = 4 * 1024 * 1024;
/// Fixed per-row wire overhead used for the byte budget (JSON punctuation, the
/// `date`, and the two field names). Only the budget uses this; no reported
/// number is ever estimated.
const ROW_OVERHEAD_BYTES: usize = 64;

/// The one entity family this delivery exports. Anything else is refused so the
/// verifier reports an uncovered family rather than comparing against silence.
const SUPPORTED_KINDS: &[&str] = &["token"];

pub fn routes() -> Router<Arc<AppState>> {
    Router::new().route("/verify/entity-statistics", post(export_entity_statistics))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntityStatisticsRequest {
    pub entities: Vec<EntityRef>,
    /// Refuse the export unless the store is still on this anchor.
    pub expected_anchor: Option<AnchorRef>,
    /// Row cap per entity; defaults to (and may not exceed) [`MAX_DAILY_ROWS`].
    pub max_daily_rows: Option<usize>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntityRef {
    pub kind: String,
    pub id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnchorRef {
    pub block_number: i64,
    pub block_hash: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Anchor {
    pub block_number: i64,
    pub block_hash: String,
}

/// The coverage contract for entity-statistics undo, as the write path stored
/// it.
///
/// `coverageFloorBlock` is the lowest block a shallow reorg can still be undone
/// to; `updatedAtBlock` is the committed tip when that floor was last advanced.
/// Both are published because a floor without the tip it was written at cannot
/// be told from a stale one.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntityStatsUndoContract {
    pub version: u32,
    pub coverage_floor_block: i64,
    pub updated_at_block: i64,
}

/// What the primary's hourly-bucket retention boundary is, per family.
///
/// Retention is executed, and recorded, one family at a time. There is
/// deliberately no cross-family verdict here: `token` being settled says
/// nothing about `mnft`, and one collapsed answer would hide exactly the
/// half-swept family a verifier has to look at.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HourlyRetentionReport {
    pub token: HourlyRetentionFamilyReport,
    pub mnft: HourlyRetentionFamilyReport,
}

/// One family's retention evidence.
///
/// A family the store holds no row for answers `"unknown"`: the API's own clock
/// is not a substitute for the deletion the primary actually performed, and a
/// zero boundary would read as "nothing was ever pruned".
#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum HourlyRetentionFamilyReport {
    State(HourlyRetentionState),
    Unknown(&'static str),
}

/// One family's retention state, exactly as the writer stored it.
///
/// `executedCutoffHour` is a boundary a reader may trust **only** when
/// `authoritative` is true. The writer advances it only when a round reaches
/// the end of the family; a round that stopped at `cursor` has deleted just the
/// keys before that cursor, so between `executedCutoffHour` and
/// `roundInProgressCutoffHour` some buckets are gone and some are not. Nothing
/// here is repaired or defaulted — a family whose first round never finished
/// reports the sentinel the store holds, with `authoritative: false` saying not
/// to read it as an hour.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HourlyRetentionState {
    /// True only when the last recorded round ran to the end of this family.
    pub authoritative: bool,
    pub policy_version: u32,
    pub executed_cutoff_hour: i64,
    /// The cutoff an in-flight round is sweeping towards; `null` when no round
    /// is in flight. Diagnostic only — never a retention boundary.
    pub round_in_progress_cutoff_hour: Option<i64>,
    /// Where the in-flight round stopped, hex-encoded; `null` once complete.
    pub cursor: Option<String>,
    pub round_started_at: i64,
    pub round_completed_at: Option<i64>,
}

/// Whether the last recorded round for a family ran to the end of it.
///
/// The writer clears `cursor` and sets `round_completed_at` at the one moment a
/// round reaches the end of the family, which is also the only moment
/// `executed_cutoff_hour` advances. Requiring both is the same condition read
/// twice, and a store where they disagree is one this endpoint must not call
/// settled.
fn round_is_complete(state: &ckbadger_store::types::HourlyRetentionState) -> bool {
    state.cursor.is_none() && state.round_completed_at.is_some()
}

/// Read one family's retention row, or report that there is none.
///
/// A row filed under one family that claims to be another is a corrupt key or a
/// corrupt value; it is an error rather than a relabelled answer, because the
/// whole point of this field is to tell legitimate retention from corruption.
fn read_hourly_retention(
    store: &ckbadger_store::CkbadgerStore,
    family: ckbadger_store::types::HourlyRetentionFamily,
) -> anyhow::Result<HourlyRetentionFamilyReport> {
    let Some(state) = store.get_hourly_retention_state(family)? else {
        return Ok(HourlyRetentionFamilyReport::Unknown("unknown"));
    };
    if state.family != family {
        return Err(anyhow::anyhow!(
            "hourly retention row stored under family '{}' reports family '{}'",
            family.as_str(),
            state.family.as_str()
        ));
    }
    Ok(HourlyRetentionFamilyReport::State(HourlyRetentionState {
        authoritative: round_is_complete(&state),
        policy_version: state.policy_version,
        executed_cutoff_hour: state.executed_cutoff_hour,
        round_in_progress_cutoff_hour: state.round_in_progress_cutoff_hour,
        cursor: state.cursor.as_deref().map(hex0x),
        round_started_at: state.round_started_at,
        round_completed_at: state.round_completed_at,
    }))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportState {
    /// A bulk-build session that has not been closed out.
    pub bulk_session_in_progress: bool,
    pub rollback_cleanup_in_progress: bool,
    pub live_cell_summary_initialized: bool,
    pub deep_fork_detected: bool,
    pub entity_stats_undo_contract: Option<EntityStatsUndoContract>,
    pub hourly_retention: HourlyRetentionReport,
}

impl ExportState {
    /// Whether a write-path phase is still in flight over this view.
    ///
    /// `live_cell_summary_initialized` is reported but does not gate: it says
    /// the first block-end publish has happened, not that one is running.
    fn mid_write(&self) -> bool {
        self.bulk_session_in_progress
            || self.rollback_cleanup_in_progress
            || self.deep_fork_detected
    }
}

/// The script that gives an entity its identity, as the index stores it.
///
/// Published so the verifier can build its chain query from the same pinned
/// read as the rows, instead of asking a public endpoint that also computes
/// aggregates — and therefore fails exactly when the aggregates are the thing
/// under suspicion.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportedScript {
    pub code_hash: String,
    pub hash_type: String,
    pub args: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DailyDeltaRow {
    pub date: u32,
    /// Net live capacity change, in shannons, as an exact decimal string.
    pub capacity_delta: String,
    /// Net live occupied ("knowledge") change, in shannons, exact decimal.
    pub knowledge_delta: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntityStatistics {
    pub kind: String,
    pub id: String,
    /// Whether the entity exists in the index that should retain it. `null`
    /// when the state withheld the export.
    pub present: Option<bool>,
    /// Rows the store holds for this entity. `null` when withheld.
    pub row_count: Option<u64>,
    /// The entity's identifying script. `null` when the entity is absent or
    /// the state withheld the export.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub type_script: Option<ExportedScript>,
    /// True only when every stored row for this entity is in `daily`.
    pub complete: bool,
    /// The index's current live capacity for this entity, in shannons.
    ///
    /// A token has **no separately stored current value**: this is the same
    /// checked accumulation of the same daily rows that the public
    /// `/tokens/{hash}` endpoint reports, read under this request's pin. It is
    /// published so the verifier can compare the index's answer against the
    /// chain's live cell set, which is computed independently; it is not a
    /// second stored figure. It accumulates *every* stored row, not just the
    /// rows returned in `daily`.
    pub current_capacity: Option<String>,
    /// As `currentCapacity`, for occupied ("knowledge") capacity.
    pub current_knowledge: Option<String>,
    /// Why the current totals could not be computed, when they could not.
    ///
    /// The accumulation is checked (no negative live capacity, occupied never
    /// above capacity), so a corrupt row series makes it fail. That is
    /// evidence, not a server fault: it is reported here while the daily rows
    /// are still exported, so the verifier can prove exactly which day is
    /// wrong instead of receiving a 500.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_error: Option<String>,
    pub daily: Vec<DailyDeltaRow>,
}

/// Why an export was refused: the caller pinned an anchor the store has moved
/// past. Structured so a client can re-pin without parsing prose.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnchorMismatch {
    pub expected: Anchor,
    pub actual: Anchor,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntityStatisticsResponse {
    pub anchor: Anchor,
    pub state: ExportState,
    /// True only when every requested entity was exported completely over a
    /// settled view.
    pub complete: bool,
    /// Present only when `expectedAnchor` did not match, in which case no
    /// numbers were exported at all.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub anchor_mismatch: Option<AnchorMismatch>,
    pub entities: Vec<EntityStatistics>,
}

fn hex0x(bytes: &[u8]) -> String {
    format!("0x{}", hex::encode(bytes))
}

/// The wire name of a stored `hash_type` discriminant.
///
/// An unknown value is an error: guessing one would publish a script that
/// hashes to something other than the entity it names.
fn hash_type_name(hash_type: u8) -> anyhow::Result<&'static str> {
    match hash_type {
        0 => Ok("data"),
        1 => Ok("type"),
        2 => Ok("data1"),
        4 => Ok("data2"),
        other => Err(anyhow::anyhow!("unknown stored hash_type {other}")),
    }
}

/// Validate the request against the server's hard caps before any store read.
///
/// Over a hard cap is a request error, not a smaller export: answering a
/// 17-entity request with 16 entities would hand the verifier a silently
/// narrowed scope.
fn validate(request: &EntityStatisticsRequest) -> Result<usize, ApiRouteError> {
    if request.entities.is_empty() {
        return Err(ApiError::bad_request(
            "entities must name at least one entity; an empty selection verifies nothing",
        ));
    }
    if request.entities.len() > MAX_ENTITIES {
        return Err(ApiError::bad_request(format!(
            "too many entities: {} requested, the server exports at most {MAX_ENTITIES} per request",
            request.entities.len()
        )));
    }
    let max_daily_rows = request.max_daily_rows.unwrap_or(MAX_DAILY_ROWS);
    if max_daily_rows == 0 || max_daily_rows > MAX_DAILY_ROWS {
        return Err(ApiError::bad_request(format!(
            "maxDailyRows must be between 1 and {MAX_DAILY_ROWS}, got {max_daily_rows}"
        )));
    }
    for (index, entity) in request.entities.iter().enumerate() {
        if !SUPPORTED_KINDS.contains(&entity.kind.as_str()) {
            return Err(ApiError::bad_request(format!(
                "entities[{index}].kind '{}' is not exported by this endpoint; supported: {}",
                entity.kind,
                SUPPORTED_KINDS.join(", ")
            )));
        }
    }
    Ok(max_daily_rows)
}

/// One entity's identity as the store keys it.
#[derive(Debug)]
struct ResolvedEntity {
    kind: String,
    id: String,
    key: Vec<u8>,
}

fn resolve(request: &EntityStatisticsRequest) -> Result<Vec<ResolvedEntity>, ApiRouteError> {
    request
        .entities
        .iter()
        .enumerate()
        .map(|(index, entity)| {
            let key = parse_hash32(&entity.id, &format!("entities[{index}].id"))?;
            Ok(ResolvedEntity {
                kind: entity.kind.clone(),
                id: hex0x(&key),
                key,
            })
        })
        .collect()
}

async fn export_entity_statistics(
    State(state): State<Arc<AppState>>,
    Json(request): Json<EntityStatisticsRequest>,
) -> ApiResult<EntityStatisticsResponse> {
    let max_daily_rows = validate(&request)?;
    let entities = resolve(&request)?;

    let store = state.store.clone();
    let expected = request.expected_anchor;

    // Every read below runs inside this request's pinned read view, so the
    // anchor, the state flags and the rows are one coherent picture.
    let response = tokio::task::spawn_blocking(move || -> anyhow::Result<Result<EntityStatisticsResponse, ApiRouteError>> {
        let sync = store.get_sync_status()?;
        if sync.tip_block_hash.is_empty() {
            return Ok(Err(ApiError::initializing(
                "the indexer has not published a sync tip yet; there is no anchor to export against",
            )));
        }
        let anchor = Anchor {
            block_number: sync.tip_block_number,
            block_hash: hex0x(&sync.tip_block_hash),
        };

        let export_state = ExportState {
            bulk_session_in_progress: store.get_bulk_build_session_marker()?.is_some(),
            rollback_cleanup_in_progress: store.is_rollback_cleanup_in_progress()?,
            live_cell_summary_initialized: store.is_live_cell_summary_initialized()?,
            deep_fork_detected: sync.deep_fork_detected,
            // Both come from the same pin as the rows they qualify: a coverage
            // floor read from a later view would describe a store the exported
            // numbers never came from.
            entity_stats_undo_contract: store.get_entity_stats_undo_contract()?.map(|contract| {
                EntityStatsUndoContract {
                    version: contract.version,
                    coverage_floor_block: contract.coverage_floor_block,
                    updated_at_block: contract.updated_at_block,
                }
            }),
            hourly_retention: HourlyRetentionReport {
                token: read_hourly_retention(
                    &store,
                    ckbadger_store::types::HourlyRetentionFamily::Token,
                )?,
                mnft: read_hourly_retention(
                    &store,
                    ckbadger_store::types::HourlyRetentionFamily::Mnft,
                )?,
            },
        };

        // A pinned anchor the store has moved past is the chain moving, not a
        // malformed request: nothing is exported, and the caller is handed the
        // actual anchor to re-pin to. Comparing rows from this view against an
        // anchor from another is exactly what the pin exists to prevent.
        let anchor_mismatch = match expected {
            Some(expected) => {
                let expected_hash = expected.block_hash.to_lowercase();
                let expected_hash = if expected_hash.starts_with("0x") {
                    expected_hash
                } else {
                    format!("0x{expected_hash}")
                };
                if expected.block_number != anchor.block_number
                    || expected_hash != anchor.block_hash
                {
                    Some(AnchorMismatch {
                        expected: Anchor {
                            block_number: expected.block_number,
                            block_hash: expected_hash,
                        },
                        actual: Anchor {
                            block_number: anchor.block_number,
                            block_hash: anchor.block_hash.clone(),
                        },
                    })
                } else {
                    None
                }
            }
            None => None,
        };

        let withhold = anchor_mismatch.is_some() || export_state.mid_write();
        let mut budget_bytes = MAX_RESPONSE_BYTES;
        let mut exported = Vec::with_capacity(entities.len());

        for entity in &entities {
            if withhold {
                exported.push(EntityStatistics {
                    kind: entity.kind.clone(),
                    id: entity.id.clone(),
                    present: None,
                    row_count: None,
                    type_script: None,
                    complete: false,
                    current_capacity: None,
                    current_knowledge: None,
                    current_error: None,
                    daily: Vec::new(),
                });
                continue;
            }

            let token = store.get_token(&entity.key)?;
            let present = token.is_some();
            let type_script = token
                .as_ref()
                .map(|info| {
                    Ok::<_, anyhow::Error>(ExportedScript {
                        code_hash: hex0x(&info.type_code_hash),
                        hash_type: hash_type_name(info.hash_type)?.to_string(),
                        args: hex0x(&info.type_args),
                    })
                })
                .transpose()?;
            // One row per day this entity had activity, so the read is bounded
            // by chain age rather than by anything the request controls.
            // Streaming it would need a store-side iterator, which this
            // endpoint does not have; the rows are consumed by value and only
            // the capped page is retained.
            let stored = store.list_token_daily_deltas_in_range(&entity.key, None, None)?;
            let row_count = stored.len() as u64;

            // The index's own answer for "what is live now", accumulated over
            // every stored row with the same checked helper the public token
            // endpoint uses — never over just the page returned below.
            let accumulated = accumulate_owned_capacity(
                stored
                    .iter()
                    .map(|(_, delta)| (delta.owned_capacity_delta, delta.owned_knowledge_delta)),
            );
            let (current_capacity, current_knowledge, current_error) = match accumulated {
                Ok((capacity, knowledge)) => {
                    (Some(capacity.to_string()), Some(knowledge.to_string()), None)
                }
                Err(error) => (None, None, Some(format!("{error:#}"))),
            };

            let mut daily = Vec::with_capacity(stored.len().min(max_daily_rows));
            for (date, delta) in stored {
                if daily.len() >= max_daily_rows {
                    break;
                }
                let capacity_delta = delta.owned_capacity_delta.to_string();
                let knowledge_delta = delta.owned_knowledge_delta.to_string();
                let cost = ROW_OVERHEAD_BYTES + capacity_delta.len() + knowledge_delta.len();
                if cost > budget_bytes {
                    break;
                }
                budget_bytes -= cost;
                daily.push(DailyDeltaRow {
                    date,
                    capacity_delta,
                    knowledge_delta,
                });
            }

            exported.push(EntityStatistics {
                kind: entity.kind.clone(),
                id: entity.id.clone(),
                present: Some(present),
                row_count: Some(row_count),
                type_script,
                complete: daily.len() as u64 == row_count,
                current_capacity,
                current_knowledge,
                current_error,
                daily,
            });
        }

        let complete = !withhold && exported.iter().all(|e| e.complete);
        Ok(Ok(EntityStatisticsResponse {
            anchor,
            state: export_state,
            complete,
            anchor_mismatch,
            entities: exported,
        }))
    })
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?
    .map_err(|e: anyhow::Error| ApiError::internal(e.to_string()))?;

    ok(response?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(
        entities: Vec<(&str, &str)>,
        max_daily_rows: Option<usize>,
    ) -> EntityStatisticsRequest {
        EntityStatisticsRequest {
            entities: entities
                .into_iter()
                .map(|(kind, id)| EntityRef {
                    kind: kind.to_string(),
                    id: id.to_string(),
                })
                .collect(),
            expected_anchor: None,
            max_daily_rows,
        }
    }

    fn hash(byte: u8) -> String {
        format!("0x{}", hex::encode([byte; 32]))
    }

    #[test]
    fn the_default_row_cap_is_the_server_maximum() {
        let id = hash(1);
        assert_eq!(
            validate(&request(vec![("token", &id)], None)).unwrap(),
            MAX_DAILY_ROWS
        );
    }

    #[test]
    fn a_row_cap_over_the_maximum_is_refused_rather_than_clamped() {
        let id = hash(1);
        let error = validate(&request(vec![("token", &id)], Some(MAX_DAILY_ROWS + 1)))
            .expect_err("over the hard cap must be a request error");
        assert_eq!(error.0, axum::http::StatusCode::BAD_REQUEST);
        assert!(error.1 .0.message.contains(&MAX_DAILY_ROWS.to_string()));
    }

    #[test]
    fn a_zero_row_cap_is_refused() {
        let id = hash(1);
        assert!(validate(&request(vec![("token", &id)], Some(0))).is_err());
    }

    #[test]
    fn an_unsupported_kind_names_what_is_supported() {
        let id = hash(1);
        let error = validate(&request(vec![("cluster", &id)], None)).unwrap_err();
        assert!(error.1 .0.message.contains("token"));
    }

    #[test]
    fn mid_write_states_are_exactly_the_in_progress_ones() {
        let base = || ExportState {
            bulk_session_in_progress: false,
            rollback_cleanup_in_progress: false,
            live_cell_summary_initialized: false,
            deep_fork_detected: false,
            entity_stats_undo_contract: None,
            hourly_retention: HourlyRetentionReport {
                token: HourlyRetentionFamilyReport::Unknown("unknown"),
                mnft: HourlyRetentionFamilyReport::Unknown("unknown"),
            },
        };
        assert!(!base().mid_write());
        assert!(ExportState {
            bulk_session_in_progress: true,
            ..base()
        }
        .mid_write());
        assert!(ExportState {
            rollback_cleanup_in_progress: true,
            ..base()
        }
        .mid_write());
        assert!(ExportState {
            deep_fork_detected: true,
            ..base()
        }
        .mid_write());
        assert!(
            !ExportState {
                live_cell_summary_initialized: true,
                ..base()
            }
            .mid_write(),
            "an initialized live-cell summary is reported, not a gate"
        );
    }

    #[test]
    fn an_id_that_is_not_a_32_byte_hash_is_refused() {
        let error = resolve(&request(vec![("token", "0x1234")], None)).unwrap_err();
        assert_eq!(error.0, axum::http::StatusCode::BAD_REQUEST);
        assert!(error.1 .0.message.contains("entities[0].id"));
    }

    #[test]
    fn a_family_with_no_retention_row_serializes_as_a_bare_string() {
        let json = serde_json::to_value(HourlyRetentionFamilyReport::Unknown("unknown")).unwrap();
        assert_eq!(json, serde_json::json!("unknown"));
    }

    /// `authoritative` is the one derived bit this endpoint publishes, so the
    /// exact condition is pinned here rather than only through the store.
    #[test]
    fn only_a_finished_round_is_authoritative() {
        let state = |cursor: Option<Vec<u8>>, completed_at: Option<i64>| {
            ckbadger_store::types::HourlyRetentionState {
                policy_version: 1,
                family: ckbadger_store::types::HourlyRetentionFamily::Token,
                executed_cutoff_hour: 1,
                round_in_progress_cutoff_hour: None,
                cursor,
                round_started_at: 0,
                round_completed_at: completed_at,
            }
        };
        assert!(round_is_complete(&state(None, Some(1))));
        assert!(
            !round_is_complete(&state(Some(vec![0x01]), Some(1))),
            "a cursor means the sweep stopped part-way"
        );
        assert!(
            !round_is_complete(&state(None, None)),
            "a round that never completed has no boundary to offer"
        );
    }
}
