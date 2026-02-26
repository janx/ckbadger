use axum::{http::StatusCode, Json};

use crate::response::ApiError;
use crate::AppState;

pub fn ensure_derived_ready(state: &AppState) -> Result<(), (StatusCode, Json<ApiError>)> {
    let sync = state
        .store
        .get_sync_status()
        .map_err(|e| ApiError::internal(e.to_string()))?;
    if sync.derived_tip_block_number < sync.tip_block_number {
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ApiError::new(
                "derived_syncing",
                format!(
                    "derived store syncing: core_tip={}, derived_tip={}",
                    sync.tip_block_number, sync.derived_tip_block_number
                ),
            )),
        ));
    }
    Ok(())
}
