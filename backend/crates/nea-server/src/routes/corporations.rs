use axum::{
    Json, Router,
    extract::{Path, Query, State},
    routing::get,
};
use serde::{Deserialize, Serialize};
use tracing::{debug, info};

use crate::error::ApiError;
use crate::state::AppState;
use nea_db::{Corporation, DoctrineProfile, KillmailSummary};

#[derive(Deserialize)]
pub struct SearchParams {
    pub q: Option<String>,
    pub page: Option<i32>,
    pub per_page: Option<i32>,
}

#[derive(Serialize)]
pub struct PaginatedCorporations {
    pub corporations: Vec<Corporation>,
    pub page: i32,
    pub per_page: i32,
    pub total: i64,
}

#[derive(Serialize)]
pub struct CorporationDetail {
    pub corporation: Corporation,
    pub profiles: Vec<DoctrineProfile>,
}

#[derive(Serialize)]
pub struct PaginatedKillmails {
    pub killmails: Vec<KillmailSummary>,
    pub page: i32,
    pub per_page: i32,
    pub total: i64,
}

#[derive(Deserialize)]
pub struct PaginationParams {
    pub page: Option<i32>,
    pub per_page: Option<i32>,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/corporations/search", get(search_corporations))
        .route("/corporations/{corp_id}", get(get_corporation))
        .route("/corporations/{corp_id}/kills", get(get_corporation_kills))
        .route(
            "/corporations/{corp_id}/losses",
            get(get_corporation_losses),
        )
}

#[tracing::instrument(skip(state, params))]
async fn search_corporations(
    State(state): State<AppState>,
    Query(params): Query<SearchParams>,
) -> Result<Json<PaginatedCorporations>, ApiError> {
    let page = params.page.unwrap_or(1).max(1);
    let per_page = params.per_page.unwrap_or(20).clamp(1, 100);
    let offset = (page - 1) * per_page;

    let q = params.q.unwrap_or_default();
    if q.is_empty() {
        return Ok(Json(PaginatedCorporations {
            corporations: vec![],
            page,
            per_page,
            total: 0,
        }));
    }

    let (corporations, total) = tokio::try_join!(
        nea_db::search_corporations(&state.pool, &q, per_page, offset),
        nea_db::search_corporations_count(&state.pool, &q),
    )?;

    info!(query = %q, results = corporations.len(), total, page, "search_corporations");
    Ok(Json(PaginatedCorporations {
        corporations,
        page,
        per_page,
        total,
    }))
}

#[tracing::instrument(skip(state))]
async fn get_corporation(
    State(state): State<AppState>,
    Path(corp_id): Path<i64>,
) -> Result<Json<CorporationDetail>, ApiError> {
    let corporation = nea_db::get_corporation(&state.pool, corp_id)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("corporation_id {corp_id} not found")))?;

    let profiles = nea_db::get_doctrine_profiles(&state.pool, "corporation", corp_id).await?;

    debug!(corp_id, profiles = profiles.len(), "get_corporation");
    Ok(Json(CorporationDetail {
        corporation,
        profiles,
    }))
}

#[tracing::instrument(skip(state, params))]
async fn get_corporation_kills(
    State(state): State<AppState>,
    Path(corp_id): Path<i64>,
    Query(params): Query<PaginationParams>,
) -> Result<Json<PaginatedKillmails>, ApiError> {
    let page = params.page.unwrap_or(1).max(1);
    let per_page = params.per_page.unwrap_or(20).clamp(1, 100);
    let offset = (page - 1) * per_page;

    let (killmails, total) = tokio::try_join!(
        nea_db::get_corporation_kills_summary(&state.pool, corp_id, per_page, offset),
        nea_db::count_corporation_kills(&state.pool, corp_id),
    )?;

    debug!(
        corp_id,
        kills = killmails.len(),
        total,
        page,
        "get_corporation_kills"
    );
    Ok(Json(PaginatedKillmails {
        killmails,
        page,
        per_page,
        total,
    }))
}

#[tracing::instrument(skip(state, params))]
async fn get_corporation_losses(
    State(state): State<AppState>,
    Path(corp_id): Path<i64>,
    Query(params): Query<PaginationParams>,
) -> Result<Json<PaginatedKillmails>, ApiError> {
    let page = params.page.unwrap_or(1).max(1);
    let per_page = params.per_page.unwrap_or(20).clamp(1, 100);
    let offset = (page - 1) * per_page;

    let (killmails, total) = tokio::try_join!(
        nea_db::get_corporation_losses_summary(&state.pool, corp_id, per_page, offset),
        nea_db::count_corporation_losses(&state.pool, corp_id),
    )?;

    debug!(
        corp_id,
        losses = killmails.len(),
        total,
        page,
        "get_corporation_losses"
    );
    Ok(Json(PaginatedKillmails {
        killmails,
        page,
        per_page,
        total,
    }))
}
