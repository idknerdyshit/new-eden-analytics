use nea_server::routes;
use nea_server::state::AppState;
use std::net::SocketAddr;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use tokio::sync::RwLock;

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let database_url =
        std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let esi_client_id =
        std::env::var("ESI_CLIENT_ID").unwrap_or_default();
    let esi_client_secret =
        std::env::var("ESI_CLIENT_SECRET").unwrap_or_default();
    let esi_callback_url =
        std::env::var("ESI_CALLBACK_URL").unwrap_or_else(|_| "http://localhost:3001/api/auth/callback".to_string());
    let session_secret =
        std::env::var("SESSION_SECRET").unwrap_or_else(|_| "change-me-in-production".to_string());
    let domain = std::env::var("DOMAIN").unwrap_or_else(|_| "localhost".to_string());
    let secure_cookies = domain != "localhost";

    if secure_cookies {
        if session_secret == "change-me-in-production"
            || session_secret == "change-me-to-a-random-string"
            || session_secret.len() < 32
        {
            panic!(
                "SESSION_SECRET must be at least 32 characters and not a default value in production (DOMAIN={domain})"
            );
        }
        if esi_client_id.is_empty() || esi_client_secret.is_empty() {
            panic!(
                "ESI_CLIENT_ID and ESI_CLIENT_SECRET must be set in production (DOMAIN={domain})"
            );
        }
    }

    tracing::info!("connecting to database");
    let pool = nea_db::create_pool(&database_url)
        .await
        .expect("failed to create database pool");

    tracing::info!("running migrations");
    nea_db::run_migrations(&pool)
        .await
        .expect("failed to run migrations");

    let state = AppState {
        pool,
        esi_client_id,
        esi_client_secret,
        esi_callback_url,
        session_secret,
        domain,
        secure_cookies,
        analysis_running: Arc::new(AtomicBool::new(false)),
        jwks_cache: Arc::new(RwLock::new(None)),
    };

    let app = routes::router(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], 3001));
    tracing::info!("nea-server listening on {addr}");

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("failed to bind");

    axum::serve(listener, app)
        .await
        .expect("server error");
}
