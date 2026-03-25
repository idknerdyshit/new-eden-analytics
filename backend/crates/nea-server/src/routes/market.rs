use axum::{
    Json, Router,
    extract::{Path, Query, State},
    routing::get,
};
use serde::Deserialize;

use tracing::debug;

use crate::error::ApiError;
use crate::state::AppState;
use nea_db::{MarketHistory, MarketSnapshot};
use nea_esi::THE_FORGE;

#[derive(Deserialize)]
pub struct HistoryParams {
    pub days: Option<i32>,
    pub region_id: Option<i32>,
}

#[derive(Deserialize)]
pub struct SnapshotParams {
    pub hours: Option<i32>,
    pub region_id: Option<i32>,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/market/{type_id}/history", get(history))
        .route("/market/{type_id}/snapshots", get(snapshots))
}

#[tracing::instrument(skip(state, params))]
async fn history(
    State(state): State<AppState>,
    Path(type_id): Path<i32>,
    Query(params): Query<HistoryParams>,
) -> Result<Json<Vec<MarketHistory>>, ApiError> {
    let days = params.days.unwrap_or(90).clamp(1, 365);
    let region_id = params.region_id.unwrap_or(THE_FORGE);

    let rows = nea_db::get_market_history(&state.pool, type_id, region_id, days).await?;
    debug!(type_id, days, rows = rows.len(), "history");
    Ok(Json(rows))
}

#[tracing::instrument(skip(state, params))]
async fn snapshots(
    State(state): State<AppState>,
    Path(type_id): Path<i32>,
    Query(params): Query<SnapshotParams>,
) -> Result<Json<Vec<MarketSnapshot>>, ApiError> {
    let hours = params.hours.unwrap_or(24).clamp(1, 168);
    let region_id = params.region_id.unwrap_or(THE_FORGE);

    let rows = nea_db::get_market_snapshots(&state.pool, type_id, region_id, hours).await?;
    debug!(type_id, hours, rows = rows.len(), "snapshots");
    Ok(Json(rows))
}
