use sqlx::postgres::{PgPool, PgPoolOptions};
use tracing::info;

use crate::error::DbError;

/// Create a PostgreSQL connection pool with sensible defaults.
pub async fn create_pool(database_url: &str) -> Result<PgPool, DbError> {
    info!(max_connections = 20, "creating database connection pool");
    let pool = PgPoolOptions::new()
        .max_connections(20)
        .connect(database_url)
        .await?;
    info!("database connection pool created");
    Ok(pool)
}

/// Run pending sqlx migrations from the `migrations/` directory embedded at compile time.
pub async fn run_migrations(pool: &PgPool) -> Result<(), DbError> {
    info!("running database migrations");
    sqlx::migrate!("./migrations").run(pool).await.map_err(|e| {
        DbError::Sqlx(sqlx::Error::Configuration(
            format!("migration error: {e}").into(),
        ))
    })?;
    info!("database migrations complete");
    Ok(())
}
