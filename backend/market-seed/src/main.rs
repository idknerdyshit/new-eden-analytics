use std::sync::Arc;

use chrono::NaiveDate;
use nea_db::MarketHistory;
use nea_esi::{EsiClient, THE_FORGE};
use sqlx::PgPool;
use tokio::sync::Semaphore;
use tracing::{info, warn};

/// Max concurrent ESI requests. ESI allows bursts but tracks error budget;
/// 20 concurrent connections matches the worker's production setting.
const CONCURRENCY: usize = 20;

/// Get the list of tracked type IDs: materials used in manufacturing UNION product types.
async fn get_tracked_type_ids(pool: &PgPool) -> Result<Vec<i32>, sqlx::Error> {
    let rows: Vec<(i32,)> = sqlx::query_as(
        r#"
        SELECT DISTINCT type_id FROM (
            SELECT DISTINCT material_type_id AS type_id FROM sde_blueprint_materials
            UNION
            SELECT DISTINCT product_type_id AS type_id FROM sde_blueprints
        ) combined
        ORDER BY type_id
        "#,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(|r| r.0).collect())
}

/// Convert ESI market history entries into DB models.
fn convert_history(
    type_id: i32,
    region_id: i32,
    entries: &[nea_esi::EsiMarketHistoryEntry],
) -> Vec<MarketHistory> {
    entries
        .iter()
        .filter_map(|e| {
            let date = NaiveDate::parse_from_str(&e.date, "%Y-%m-%d").ok()?;
            Some(MarketHistory {
                type_id,
                region_id,
                date,
                average: e.average,
                highest: e.highest,
                lowest: e.lowest,
                volume: e.volume,
                order_count: e.order_count as i32,
            })
        })
        .collect()
}

async fn fetch_and_store(
    pool: &PgPool,
    esi: &EsiClient,
    type_id: i32,
) -> Result<u64, Box<dyn std::error::Error + Send + Sync>> {
    let entries = esi.market_history(THE_FORGE, type_id).await?;
    let rows = convert_history(type_id, THE_FORGE, &entries);
    let count = rows.len() as u64;
    nea_db::insert_market_history(pool, &rows).await?;
    Ok(count)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    dotenvy::dotenv().ok();

    info!("market-seed starting");

    let database_url = std::env::var("DATABASE_URL")
        .expect("DATABASE_URL must be set");
    let pool = nea_db::create_pool(&database_url).await?;
    let esi = Arc::new(EsiClient::new());

    let type_ids = get_tracked_type_ids(&pool).await?;
    if type_ids.is_empty() {
        warn!("no tracked type IDs found — run `make seed-sde` first");
        return Ok(());
    }

    info!(types = type_ids.len(), "fetching market history from ESI (The Forge)");

    let semaphore = Arc::new(Semaphore::new(CONCURRENCY));
    let mut handles = Vec::with_capacity(type_ids.len());

    for type_id in &type_ids {
        let permit = semaphore.clone().acquire_owned().await.unwrap();
        let esi = Arc::clone(&esi);
        let pool = pool.clone();
        let type_id = *type_id;

        handles.push(tokio::spawn(async move {
            let _permit = permit;
            let result = fetch_and_store(&pool, &esi, type_id).await;
            (type_id, result)
        }));
    }

    let mut fetched = 0u64;
    let mut inserted = 0u64;
    let mut errors = 0u64;

    for handle in handles {
        match handle.await {
            Ok((_type_id, Ok(count))) => {
                fetched += 1;
                inserted += count;
                if fetched % 500 == 0 {
                    info!(fetched, inserted, errors, "progress");
                }
            }
            Ok((type_id, Err(e))) => {
                errors += 1;
                if errors <= 10 {
                    warn!(type_id, error = %e, "fetch error");
                }
            }
            Err(e) => {
                errors += 1;
                if errors <= 10 {
                    warn!(error = %e, "task join error");
                }
            }
        }
    }

    info!(
        fetched,
        inserted,
        errors,
        "market-seed complete"
    );

    Ok(())
}
