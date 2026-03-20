use axum::{
    extract::{Path, Query, State},
    routing::get,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use tracing::{debug, info};

use crate::error::ApiError;
use crate::state::AppState;
use nea_db::{Character, CharacterProfile, Killmail};

#[derive(Deserialize)]
pub struct SearchParams {
    pub q: Option<String>,
    pub page: Option<i32>,
    pub per_page: Option<i32>,
}

#[derive(Serialize)]
pub struct PaginatedCharacters {
    pub characters: Vec<Character>,
    pub page: i32,
    pub per_page: i32,
    pub total: i64,
}

#[derive(Serialize)]
pub struct CharacterDetail {
    pub character: Character,
    pub profile: Option<CharacterProfile>,
}

#[derive(Deserialize)]
pub struct LimitParams {
    pub limit: Option<i32>,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/characters/search", get(search_characters))
        .route("/characters/{character_id}", get(get_character))
        .route("/characters/{character_id}/kills", get(get_character_kills))
        .route("/characters/{character_id}/losses", get(get_character_losses))
}

#[tracing::instrument(skip(state, params))]
async fn search_characters(
    State(state): State<AppState>,
    Query(params): Query<SearchParams>,
) -> Result<Json<PaginatedCharacters>, ApiError> {
    let page = params.page.unwrap_or(1).max(1);
    let per_page = params.per_page.unwrap_or(20).clamp(1, 100);
    let offset = (page - 1) * per_page;

    let q = params.q.unwrap_or_default();
    if q.is_empty() {
        return Ok(Json(PaginatedCharacters {
            characters: vec![],
            page,
            per_page,
            total: 0,
        }));
    }

    let (characters, total) = tokio::try_join!(
        nea_db::search_characters(&state.pool, &q, per_page, offset),
        nea_db::search_characters_count(&state.pool, &q),
    )?;

    info!(query = %q, results = characters.len(), total, page, "search_characters");
    Ok(Json(PaginatedCharacters {
        characters,
        page,
        per_page,
        total,
    }))
}

#[tracing::instrument(skip(state))]
async fn get_character(
    State(state): State<AppState>,
    Path(character_id): Path<i64>,
) -> Result<Json<CharacterDetail>, ApiError> {
    let character = nea_db::get_character(&state.pool, character_id)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("character_id {character_id} not found")))?;

    let profile = nea_db::get_character_profile(&state.pool, character_id).await?;

    debug!(character_id, has_profile = profile.is_some(), "get_character");
    Ok(Json(CharacterDetail { character, profile }))
}

#[tracing::instrument(skip(state, params))]
async fn get_character_kills(
    State(state): State<AppState>,
    Path(character_id): Path<i64>,
    Query(params): Query<LimitParams>,
) -> Result<Json<Vec<Killmail>>, ApiError> {
    let limit = params.limit.unwrap_or(20).clamp(1, 100);
    let kills = nea_db::get_character_kills(&state.pool, character_id, limit).await?;
    debug!(character_id, kills = kills.len(), "get_character_kills");
    Ok(Json(kills))
}

#[tracing::instrument(skip(state, params))]
async fn get_character_losses(
    State(state): State<AppState>,
    Path(character_id): Path<i64>,
    Query(params): Query<LimitParams>,
) -> Result<Json<Vec<Killmail>>, ApiError> {
    let limit = params.limit.unwrap_or(20).clamp(1, 100);
    let losses = nea_db::get_character_losses(&state.pool, character_id, limit).await?;
    debug!(character_id, losses = losses.len(), "get_character_losses");
    Ok(Json(losses))
}
