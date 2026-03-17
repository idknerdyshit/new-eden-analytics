use axum::{extract::State, routing::get, Json, Router};
use serde::Serialize;
use sqlx::FromRow;
use tracing::info;

use crate::error::ApiError;
use crate::state::AppState;
use nea_db::{CorrelationResult, DailyDestruction};

#[derive(Serialize)]
pub struct DashboardResponse {
    pub top_correlations: Vec<CorrelationResult>,
    pub top_destruction: Vec<DailyDestruction>,
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct Mover {
    pub type_id: i32,
    pub name: String,
    pub previous_avg: f64,
    pub current_avg: f64,
    pub change_pct: f64,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/dashboard", get(dashboard))
        .route("/dashboard/movers", get(movers))
}

#[tracing::instrument(skip(state))]
async fn dashboard(State(state): State<AppState>) -> Result<Json<DashboardResponse>, ApiError> {
    let top_correlations = nea_db::get_top_correlations(&state.pool, 10).await?;

    // Items with highest recent destruction (last 7 days, top 10)
    let top_destruction: Vec<DailyDestruction> = sqlx::query_as(
        r#"
        SELECT type_id, date, quantity_destroyed, kill_count
        FROM daily_destruction
        WHERE date >= CURRENT_DATE - INTERVAL '7 days'
        ORDER BY quantity_destroyed DESC
        LIMIT 10
        "#,
    )
    .fetch_all(&state.pool)
    .await
    .map_err(nea_db::DbError::from)?;

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
    // Get materials with biggest 24h average price change.
    // Compare the most recent day's average to the previous day's average in market_history.
    let rows: Vec<Mover> = sqlx::query_as(
        r#"
        WITH recent AS (
            SELECT
                mh.type_id,
                st.name,
                mh.date,
                mh.average,
                ROW_NUMBER() OVER (PARTITION BY mh.type_id ORDER BY mh.date DESC) AS rn
            FROM market_history mh
            JOIN sde_types st ON st.type_id = mh.type_id
            WHERE mh.region_id = 10000002
              AND mh.date >= CURRENT_DATE - INTERVAL '3 days'
        ),
        pairs AS (
            SELECT
                r1.type_id,
                r1.name,
                r2.average AS previous_avg,
                r1.average AS current_avg
            FROM recent r1
            JOIN recent r2 ON r1.type_id = r2.type_id AND r2.rn = 2
            WHERE r1.rn = 1 AND r2.average > 0
        )
        SELECT
            type_id,
            name,
            previous_avg,
            current_avg,
            ((current_avg - previous_avg) / previous_avg * 100.0) AS change_pct
        FROM pairs
        ORDER BY ABS((current_avg - previous_avg) / previous_avg) DESC
        LIMIT 20
        "#,
    )
    .fetch_all(&state.pool)
    .await
    .map_err(nea_db::DbError::from)?;

    info!(movers = rows.len(), "movers");
    Ok(Json(rows))
}
