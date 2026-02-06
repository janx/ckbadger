use axum::{extract::State, routing::get, Router};
use std::sync::Arc;

use crate::response::{ApiError, ApiResult};
use crate::AppState;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new().route("/tasks/active", get(get_active_tasks))
}

async fn get_active_tasks(State(_state): State<Arc<AppState>>) -> ApiResult<()> {
    Err(ApiError::internal("ClickHouse implementation pending"))
}
