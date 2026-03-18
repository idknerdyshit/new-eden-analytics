pub mod analysis;
pub mod auth;
pub mod dashboard;
pub mod destruction;
pub mod items;
pub mod market;

use axum::{
    extract::{Request, State},
    http::StatusCode,
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use serde_json::json;
use tower_http::cors::CorsLayer;
use tower_http::trace::{DefaultMakeSpan, DefaultOnResponse, TraceLayer};
use tracing::{Instrument, Level};
use uuid::Uuid;

use crate::state::AppState;

async fn request_id_middleware(request: Request, next: Next) -> Response {
    let request_id = Uuid::new_v4();
    let span = tracing::info_span!("request", request_id = %request_id);

    let mut response = next.run(request).instrument(span).await;

    response.headers_mut().insert(
        "x-request-id",
        request_id
            .to_string()
            .parse()
            .expect("uuid is valid header value"),
    );
    response
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .nest("/api", api_router())
        .layer(CorsLayer::permissive())
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(DefaultMakeSpan::new().level(Level::INFO))
                .on_response(DefaultOnResponse::new().level(Level::INFO)),
        )
        .layer(middleware::from_fn(request_id_middleware))
        .with_state(state)
}

fn api_router() -> Router<AppState> {
    Router::new()
        .route("/health", get(health))
        .merge(dashboard::routes())
        .merge(items::routes())
        .merge(market::routes())
        .merge(analysis::routes())
        .merge(destruction::routes())
        .merge(auth::routes())
}

async fn health(State(state): State<AppState>) -> impl IntoResponse {
    match sqlx::query("SELECT 1").execute(&state.pool).await {
        Ok(_) => (StatusCode::OK, Json(json!({"status": "ok"}))),
        Err(_) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"status": "error", "message": "database unavailable"})),
        ),
    }
}
