use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use nea_db::MarketSnapshot;
use nea_esi::{EsiClient, JITA_STATION, THE_FORGE};
use sqlx::PgPool;
use tokio::sync::Semaphore;
use tokio::time;

/// Get the list of tracked type IDs (same as market_history).
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

pub async fn run(pool: PgPool, esi: Arc<EsiClient>) {
    tracing::info!("market_orders snapshotter started");
    let mut interval = time::interval(Duration::from_secs(3600));

    loop {
        interval.tick().await;
        tracing::info!("market_orders: starting snapshot cycle");

        let type_ids = match get_tracked_type_ids(&pool).await {
            Ok(ids) => ids,
            Err(e) => {
                tracing::error!("market_orders: failed to get tracked type IDs: {e}");
                continue;
            }
        };

        tracing::info!(
            "market_orders: snapshotting {} types",
            type_ids.len()
        );

        let semaphore = Arc::new(Semaphore::new(20));
        let mut handles = Vec::with_capacity(type_ids.len());

        for type_id in &type_ids {
            let permit = match semaphore.clone().acquire_owned().await {
                Ok(p) => p,
                Err(_) => {
                    tracing::error!("market_orders: semaphore closed, aborting cycle");
                    break;
                }
            };
            let esi = Arc::clone(&esi);
            let pool = pool.clone();
            let type_id = *type_id;

            handles.push(tokio::spawn(async move {
                let _permit = permit;
                let result = fetch_and_snapshot(&pool, &esi, type_id).await;
                (type_id, result)
            }));
        }

        let mut fetched = 0u64;
        let mut errors = 0u64;

        for handle in handles {
            match handle.await {
                Ok((_type_id, Ok(()))) => {
                    fetched += 1;
                }
                Ok((type_id, Err(e))) => {
                    tracing::warn!(type_id, "market_orders: fetch error: {e}");
                    errors += 1;
                }
                Err(e) => {
                    tracing::warn!("market_orders: task join error: {e}");
                    errors += 1;
                }
            }
        }

        tracing::info!(fetched, errors, "market_orders: cycle complete");
    }
}

fn compute_spread(best_bid: Option<f64>, best_ask: Option<f64>) -> Option<f64> {
    match (best_bid, best_ask) {
        (Some(bid), Some(ask)) if bid > 0.0 => Some((ask - bid) / bid * 100.0),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_spread_normal() {
        let spread = compute_spread(Some(100.0), Some(105.0));
        assert!((spread.unwrap() - 5.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_compute_spread_bid_zero() {
        assert_eq!(compute_spread(Some(0.0), Some(10.0)), None);
    }

    #[test]
    fn test_compute_spread_none_values() {
        assert_eq!(compute_spread(None, Some(10.0)), None);
        assert_eq!(compute_spread(Some(10.0), None), None);
        assert_eq!(compute_spread(None, None), None);
    }
}

async fn fetch_and_snapshot(
    pool: &PgPool,
    esi: &EsiClient,
    type_id: i32,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let orders = esi.market_orders(THE_FORGE, type_id).await?;
    let (best_bid, best_ask, bid_volume, ask_volume) =
        EsiClient::compute_best_bid_ask(&orders, JITA_STATION);

    let spread = compute_spread(best_bid, best_ask);

    let snapshot = MarketSnapshot {
        type_id,
        region_id: THE_FORGE,
        station_id: Some(JITA_STATION),
        ts: Utc::now(),
        best_bid,
        best_ask,
        bid_volume: Some(bid_volume),
        ask_volume: Some(ask_volume),
        spread,
    };

    nea_db::insert_market_snapshot(pool, &snapshot).await?;
    Ok(())
}
