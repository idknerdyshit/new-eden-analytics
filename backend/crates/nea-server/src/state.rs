use jsonwebtoken::jwk::JwkSet;
use sqlx::PgPool;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use tokio::sync::RwLock;

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub esi_client_id: String,
    pub esi_client_secret: String,
    pub esi_callback_url: String,
    pub session_secret: String,
    pub domain: String,
    pub secure_cookies: bool,
    pub analysis_running: Arc<AtomicBool>,
    pub jwks_cache: Arc<RwLock<Option<JwkSet>>>,
}
