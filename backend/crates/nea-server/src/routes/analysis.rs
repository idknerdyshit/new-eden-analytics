use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use std::sync::atomic::Ordering;

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
        .route("/analysis/run", post(run_analysis))
        .route("/analysis/status", get(analysis_status))
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

#[tracing::instrument(skip(state))]
async fn run_analysis(
    State(state): State<AppState>,
) -> Result<(StatusCode, Json<serde_json::Value>), (StatusCode, Json<serde_json::Value>)> {
    if state
        .analysis_running
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Err((
            StatusCode::CONFLICT,
            Json(serde_json::json!({"status": "already_running"})),
        ));
    }

    let pool = state.pool.clone();
    let running = state.analysis_running.clone();

    tokio::spawn(async move {
        let result =
            nea_analysis::runner::run_analysis(&pool, nea_esi::THE_FORGE).await;
        running.store(false, Ordering::SeqCst);

        match result {
            Ok(stats) => {
                tracing::info!(
                    pairs_analyzed = stats.pairs_analyzed,
                    significant = stats.significant,
                    "manual analysis run complete"
                );
            }
            Err(e) => {
                tracing::error!("manual analysis run failed: {e}");
            }
        }
    });

    Ok((
        StatusCode::ACCEPTED,
        Json(serde_json::json!({"status": "started"})),
    ))
}

#[tracing::instrument(skip(state))]
async fn analysis_status(
    State(state): State<AppState>,
) -> Json<serde_json::Value> {
    let running = state.analysis_running.load(Ordering::SeqCst);
    Json(serde_json::json!({"running": running}))
}
