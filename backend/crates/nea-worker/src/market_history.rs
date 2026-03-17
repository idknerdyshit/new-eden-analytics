use std::sync::Arc;
use std::time::Duration;

use chrono::NaiveDate;
use nea_db::MarketHistory;
use nea_esi::{EsiClient, THE_FORGE};
use sqlx::PgPool;
use tokio::sync::Semaphore;
use tokio::time;

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

pub async fn run(pool: PgPool, esi: Arc<EsiClient>) {
    tracing::info!("market_history poller started");
    let mut interval = time::interval(Duration::from_secs(3600));

    loop {
        interval.tick().await;
        tracing::info!("market_history: starting fetch cycle");

        let type_ids = match get_tracked_type_ids(&pool).await {
            Ok(ids) => ids,
            Err(e) => {
                tracing::error!("market_history: failed to get tracked type IDs: {e}");
                continue;
            }
        };

        tracing::info!(
            "market_history: fetching history for {} types",
            type_ids.len()
        );

        let semaphore = Arc::new(Semaphore::new(20));
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
                }
                Ok((type_id, Err(e))) => {
                    tracing::warn!(type_id, "market_history: fetch error: {e}");
                    errors += 1;
                }
                Err(e) => {
                    tracing::warn!("market_history: task join error: {e}");
                    errors += 1;
                }
            }
        }

        tracing::info!(
            fetched,
            inserted,
            errors,
            "market_history: cycle complete"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nea_esi::EsiMarketHistoryEntry;

    fn entry(date: &str, avg: f64, high: f64, low: f64, vol: i64, orders: i64) -> EsiMarketHistoryEntry {
        EsiMarketHistoryEntry {
            date: date.to_string(),
            average: avg,
            highest: high,
            lowest: low,
            volume: vol,
            order_count: orders,
        }
    }

    #[test]
    fn test_convert_history_valid() {
        let entries = vec![entry("2026-03-01", 5.25, 5.30, 5.10, 1000, 50)];
        let result = convert_history(34, 10000002, &entries);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].type_id, 34);
        assert_eq!(result[0].region_id, 10000002);
        assert_eq!(result[0].date, NaiveDate::from_ymd_opt(2026, 3, 1).unwrap());
        assert!((result[0].average - 5.25).abs() < f64::EPSILON);
        assert_eq!(result[0].order_count, 50);
    }

    #[test]
    fn test_convert_history_invalid_date_filtered() {
        let entries = vec![
            entry("2026-03-01", 5.25, 5.30, 5.10, 1000, 50),
            entry("not-a-date", 1.0, 1.0, 1.0, 1, 1),
            entry("2026-03-02", 6.0, 6.5, 5.5, 2000, 100),
        ];
        let result = convert_history(34, 10000002, &entries);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].date, NaiveDate::from_ymd_opt(2026, 3, 1).unwrap());
        assert_eq!(result[1].date, NaiveDate::from_ymd_opt(2026, 3, 2).unwrap());
    }

    #[test]
    fn test_convert_history_empty() {
        let result = convert_history(34, 10000002, &[]);
        assert!(result.is_empty());
    }
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
