use axum::{
    extract::{Path, Query, State},
    routing::get,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use tracing::{debug, info};

use crate::error::ApiError;
use crate::state::AppState;
use nea_db::{Alliance, DoctrineProfile};

#[derive(Deserialize)]
pub struct SearchParams {
    pub q: Option<String>,
    pub page: Option<i32>,
    pub per_page: Option<i32>,
}

#[derive(Serialize)]
pub struct PaginatedAlliances {
    pub alliances: Vec<Alliance>,
    pub page: i32,
    pub per_page: i32,
    pub total: i64,
}

#[derive(Serialize)]
pub struct AllianceDetail {
    pub alliance: Alliance,
    pub profiles: Vec<DoctrineProfile>,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/alliances/search", get(search_alliances))
        .route("/alliances/:alliance_id", get(get_alliance))
}

#[tracing::instrument(skip(state, params))]
async fn search_alliances(
    State(state): State<AppState>,
    Query(params): Query<SearchParams>,
) -> Result<Json<PaginatedAlliances>, ApiError> {
    let page = params.page.unwrap_or(1).max(1);
    let per_page = params.per_page.unwrap_or(20).clamp(1, 100);
    let offset = (page - 1) * per_page;

    let q = params.q.unwrap_or_default();
    if q.is_empty() {
        return Ok(Json(PaginatedAlliances {
            alliances: vec![],
            page,
            per_page,
            total: 0,
        }));
    }

    let (alliances, total) = tokio::try_join!(
        nea_db::search_alliances(&state.pool, &q, per_page, offset),
        nea_db::search_alliances_count(&state.pool, &q),
    )?;

    info!(query = %q, results = alliances.len(), total, page, "search_alliances");
    Ok(Json(PaginatedAlliances {
        alliances,
        page,
        per_page,
        total,
    }))
}

#[tracing::instrument(skip(state))]
async fn get_alliance(
    State(state): State<AppState>,
    Path(alliance_id): Path<i64>,
) -> Result<Json<AllianceDetail>, ApiError> {
    let alliance = nea_db::get_alliance(&state.pool, alliance_id)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("alliance_id {alliance_id} not found")))?;

    let profiles = nea_db::get_doctrine_profiles(&state.pool, "alliance", alliance_id).await?;

    debug!(alliance_id, profiles = profiles.len(), "get_alliance");
    Ok(Json(AllianceDetail { alliance, profiles }))
}
