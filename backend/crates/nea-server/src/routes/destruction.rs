use axum::{
    extract::{Path, Query, State},
    routing::get,
    Json, Router,
};
use serde::Deserialize;

use tracing::debug;

use crate::error::ApiError;
use crate::state::AppState;
use nea_db::DailyDestruction;

#[derive(Deserialize)]
pub struct DestructionParams {
    pub days: Option<i32>,
}

pub fn routes() -> Router<AppState> {
    Router::new().route("/destruction/:type_id", get(destruction))
}

#[tracing::instrument(skip(state, params))]
async fn destruction(
    State(state): State<AppState>,
    Path(type_id): Path<i32>,
    Query(params): Query<DestructionParams>,
) -> Result<Json<Vec<DailyDestruction>>, ApiError> {
    let days = params.days.unwrap_or(90).clamp(1, 365);
    let rows = nea_db::get_daily_destruction(&state.pool, type_id, days).await?;
    debug!(type_id, days, rows = rows.len(), "destruction");
    Ok(Json(rows))
}
