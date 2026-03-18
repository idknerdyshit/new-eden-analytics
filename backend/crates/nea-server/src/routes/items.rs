use axum::{
    extract::{Path, Query, State},
    routing::get,
    Json, Router,
};
use serde::{Deserialize, Serialize};

use tracing::{debug, info};

use crate::error::ApiError;
use crate::state::AppState;
use nea_db::{ProductMaterial, SdeType};

#[derive(Deserialize)]
pub struct SearchParams {
    pub q: Option<String>,
    pub page: Option<i32>,
    pub per_page: Option<i32>,
}

#[derive(Serialize)]
pub struct PaginatedItems {
    pub items: Vec<SdeType>,
    pub page: i32,
    pub per_page: i32,
    pub total: i64,
}

#[derive(Serialize)]
pub struct ItemDetail {
    pub item: SdeType,
    pub materials: Vec<ProductMaterial>,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/items", get(search_items))
        .route("/items/:type_id", get(get_item))
}

#[tracing::instrument(skip(state, params))]
async fn search_items(
    State(state): State<AppState>,
    Query(params): Query<SearchParams>,
) -> Result<Json<PaginatedItems>, ApiError> {
    let page = params.page.unwrap_or(1).max(1);
    let per_page = params.per_page.unwrap_or(20).clamp(1, 100);
    let offset = (page - 1) * per_page;

    let q = params.q.unwrap_or_default();
    if q.is_empty() {
        return Ok(Json(PaginatedItems {
            items: vec![],
            page,
            per_page,
            total: 0,
        }));
    }

    let (items, total) = tokio::try_join!(
        nea_db::search_types(&state.pool, &q, per_page, offset),
        nea_db::search_types_count(&state.pool, &q),
    )?;

    info!(query = %q, results = items.len(), total, page, "search_items");
    Ok(Json(PaginatedItems {
        items,
        page,
        per_page,
        total,
    }))
}

#[tracing::instrument(skip(state))]
async fn get_item(
    State(state): State<AppState>,
    Path(type_id): Path<i32>,
) -> Result<Json<ItemDetail>, ApiError> {
    let item = nea_db::get_type(&state.pool, type_id)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("type_id {type_id} not found")))?;

    let materials = nea_db::get_product_materials(&state.pool, type_id).await?;

    debug!(type_id, materials = materials.len(), "get_item");
    Ok(Json(ItemDetail { item, materials }))
}
