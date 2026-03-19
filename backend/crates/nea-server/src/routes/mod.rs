pub mod alliances;
pub mod analysis;
pub mod auth;
pub mod characters;
pub mod corporations;
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
use axum::http::{HeaderValue, Method};
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

async fn security_headers_middleware(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    let mut response = next.run(request).await;
    let headers = response.headers_mut();

    headers.insert("x-content-type-options", HeaderValue::from_static("nosniff"));
    headers.insert("x-frame-options", HeaderValue::from_static("DENY"));
    headers.insert(
        "referrer-policy",
        HeaderValue::from_static("strict-origin-when-cross-origin"),
    );

    if state.secure_cookies {
        headers.insert(
            "strict-transport-security",
            HeaderValue::from_static("max-age=31536000; includeSubDomains"),
        );
    }

    // TODO: Add Content-Security-Policy once D3 inline styles are addressed
    response
}

fn cors_layer(state: &AppState) -> CorsLayer {
    if state.domain == "localhost" {
        CorsLayer::permissive()
    } else {
        let origin = format!("https://{}", state.domain);
        CorsLayer::new()
            .allow_origin(origin.parse::<HeaderValue>().expect("valid origin"))
            .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
            .allow_headers([axum::http::header::CONTENT_TYPE])
            .allow_credentials(true)
    }
}

pub fn router(state: AppState) -> Router {
    let cors = cors_layer(&state);
    Router::new()
        .nest("/api", api_router())
        .layer(cors)
        .layer(middleware::from_fn_with_state(
            state.clone(),
            security_headers_middleware,
        ))
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
        .merge(characters::routes())
        .merge(corporations::routes())
        .merge(alliances::routes())
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
