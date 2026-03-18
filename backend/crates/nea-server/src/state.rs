use sqlx::PgPool;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub esi_client_id: String,
    pub esi_client_secret: String,
    pub esi_callback_url: String,
    #[allow(dead_code)]
    pub session_secret: String,
    pub analysis_running: Arc<AtomicBool>,
}
