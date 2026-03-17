use axum::{
    extract::{Path, Query, State},
    routing::get,
    Json, Router,
};
use serde::Deserialize;

use tracing::debug;

use crate::error::ApiError;
use crate::state::AppState;
use nea_db::CorrelationResult;

#[derive(Deserialize)]
pub struct TopParams {
    pub limit: Option<i32>,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/analysis/:type_id/correlations", get(correlations))
        .route("/analysis/:type_id/lag", get(lag))
        .route("/analysis/top", get(top))
}

#[tracing::instrument(skip(state))]
async fn correlations(
    State(state): State<AppState>,
    Path(type_id): Path<i32>,
) -> Result<Json<Vec<CorrelationResult>>, ApiError> {
    let rows = nea_db::get_correlations_for_product(&state.pool, type_id).await?;
    debug!(type_id, results = rows.len(), "correlations");
    Ok(Json(rows))
}

#[tracing::instrument(skip(state))]
async fn lag(
    State(state): State<AppState>,
    Path(type_id): Path<i32>,
) -> Result<Json<Vec<CorrelationResult>>, ApiError> {
    // Same data, returned for lag-focused views on the frontend.
    let rows = nea_db::get_correlations_for_product(&state.pool, type_id).await?;
    debug!(type_id, results = rows.len(), "lag");
    Ok(Json(rows))
}

#[tracing::instrument(skip(state, params))]
async fn top(
    State(state): State<AppState>,
    Query(params): Query<TopParams>,
) -> Result<Json<Vec<CorrelationResult>>, ApiError> {
    let limit = params.limit.unwrap_or(20).clamp(1, 100);
    let rows = nea_db::get_top_correlations(&state.pool, limit).await?;
    debug!(limit, results = rows.len(), "top");
    Ok(Json(rows))
}
