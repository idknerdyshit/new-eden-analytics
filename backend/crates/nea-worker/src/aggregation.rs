use std::time::Duration;

use nea_db::DailyDestruction;
use sqlx::PgPool;
use tokio::time;

pub async fn run(pool: PgPool) {
    tracing::info!("aggregation task started");
    let mut interval = time::interval(Duration::from_secs(3600));

    loop {
        interval.tick().await;
        tracing::info!("aggregation: starting daily destruction aggregation");

        match aggregate_destruction(&pool).await {
            Ok(count) => {
                tracing::info!(rows_upserted = count, "aggregation: cycle complete");
            }
            Err(e) => {
                tracing::error!("aggregation: failed: {e}");
            }
        }
    }
}

async fn aggregate_destruction(
    pool: &PgPool,
) -> Result<u64, Box<dyn std::error::Error + Send + Sync>> {
    let rows = sqlx::query_as::<_, DailyDestruction>(
        r#"
        SELECT combined.type_id, st.name AS type_name, kill_time::date as date,
               SUM(quantity_destroyed)::bigint as quantity_destroyed,
               COUNT(DISTINCT killmail_id)::int as kill_count
        FROM (
            SELECT ki.killmail_id, ki.kill_time, ki.type_id, ki.quantity_destroyed
            FROM killmail_items ki
            JOIN sde_types st ON st.type_id = ki.type_id
            WHERE ki.kill_time >= NOW() - INTERVAL '7 days'
              AND st.group_id != 29
            UNION ALL
            SELECT kv.killmail_id, kv.kill_time, kv.ship_type_id as type_id, 1::bigint as quantity_destroyed
            FROM killmail_victims kv
            JOIN sde_types st ON st.type_id = kv.ship_type_id
            WHERE kv.kill_time >= NOW() - INTERVAL '7 days'
              AND st.group_id != 29
        ) combined
        JOIN sde_types st ON st.type_id = combined.type_id
        GROUP BY combined.type_id, st.name, kill_time::date
        "#,
    )
    .fetch_all(pool)
    .await?;

    let count = rows.len() as u64;
    nea_db::upsert_daily_destruction(pool, &rows).await?;
    Ok(count)
}
