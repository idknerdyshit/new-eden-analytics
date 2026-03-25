use axum::{Json, Router, extract::State, routing::get};
use serde::Serialize;
use tracing::info;

use crate::error::ApiError;
use crate::state::AppState;
use nea_db::{CorrelationResult, DoctrineProfile, Mover, TrendingDestruction};

#[derive(Serialize)]
pub struct DashboardResponse {
    pub top_correlations: Vec<CorrelationResult>,
    pub top_destruction: Vec<TrendingDestruction>,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/dashboard", get(dashboard))
        .route("/dashboard/movers", get(movers))
        .route("/dashboard/doctrines", get(recent_doctrines))
}

#[tracing::instrument(skip(state))]
async fn dashboard(State(state): State<AppState>) -> Result<Json<DashboardResponse>, ApiError> {
    let top_correlations = nea_db::get_top_correlations(&state.pool, 10).await?;
    let top_destruction = nea_db::get_trending_destruction(&state.pool, 7, 10).await?;

    info!(
        correlations = top_correlations.len(),
        destruction_rows = top_destruction.len(),
        "dashboard"
    );
    Ok(Json(DashboardResponse {
        top_correlations,
        top_destruction,
    }))
}

#[tracing::instrument(skip(state))]
async fn movers(State(state): State<AppState>) -> Result<Json<Vec<Mover>>, ApiError> {
    let rows = nea_db::get_movers(&state.pool, 20).await?;
    info!(movers = rows.len(), "movers");
    Ok(Json(rows))
}

#[tracing::instrument(skip(state))]
async fn recent_doctrines(
    State(state): State<AppState>,
) -> Result<Json<Vec<DoctrineProfile>>, ApiError> {
    let rows = nea_db::get_recent_doctrine_profiles(&state.pool, 8).await?;
    info!(doctrines = rows.len(), "recent_doctrines");
    Ok(Json(rows))
}
