use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use nea_db::MarketSnapshot;
use nea_esi::{compute_best_bid_ask, EsiClient, EsiError, JITA_STATION, THE_FORGE};
use sqlx::PgPool;
use tokio::time;

pub async fn run(pool: PgPool, esi: Arc<EsiClient>) {
    tracing::info!("market_orders snapshotter started");
    let mut interval = time::interval(Duration::from_secs(3600));

    loop {
        interval.tick().await;
        tracing::info!("market_orders: starting snapshot cycle");

        let type_ids = match nea_db::get_tracked_type_ids(&pool).await {
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

        let mut fetched = 0u64;
        let mut errors = 0u64;

        for type_id in &type_ids {
            match fetch_and_snapshot(&pool, &esi, *type_id).await {
                Ok(()) => {
                    fetched += 1;
                }
                Err(e) => {
                    tracing::warn!(type_id, "market_orders: fetch error: {e}");
                    errors += 1;
                }
            }

            // ~2 req/s to ease ESI pressure
            time::sleep(Duration::from_millis(500)).await;

            // If error budget is critically low, pause longer
            if esi.error_budget() < 30 {
                tracing::warn!(
                    budget = esi.error_budget(),
                    "market_orders: ESI error budget low, pausing 30s"
                );
                time::sleep(Duration::from_secs(30)).await;
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
    let orders = match esi.market_orders(THE_FORGE, type_id).await {
        Ok(o) => o,
        Err(EsiError::Api { status: 400, ref message }) if message.contains("not tradable") => {
            nea_db::mark_type_non_tradable(pool, type_id).await?;
            tracing::info!(type_id, "marked type as non-tradable, will skip in future cycles");
            return Ok(());
        }
        Err(e) => return Err(e.into()),
    };
    let (best_bid, best_ask, bid_volume, ask_volume) =
        compute_best_bid_ask(&orders, JITA_STATION);

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
