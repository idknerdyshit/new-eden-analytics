// nea-db: Database models, queries, and connection management for TimescaleDB/PostgreSQL.

pub mod error;
pub mod models;
pub mod pool;
pub mod queries;

pub use error::DbError;
pub use models::*;
pub use pool::{create_pool, run_migrations};
pub use queries::*;
