use std::time::Duration;

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
    let result = sqlx::query(
        r#"
        WITH item_rows AS (
            SELECT ki.killmail_id,
                   ki.kill_time::date AS date,
                   ki.type_id,
                   SUM(ki.quantity_destroyed)::bigint AS quantity_destroyed
            FROM killmail_items ki
            JOIN sde_types st ON st.type_id = ki.type_id
            WHERE ki.kill_time >= NOW() - INTERVAL '7 days'
              AND st.group_id != 29
            GROUP BY ki.killmail_id, ki.kill_time::date, ki.type_id
        ),
        victim_rows AS (
            SELECT kv.killmail_id,
                   kv.kill_time::date AS date,
                   kv.ship_type_id AS type_id,
                   1::bigint AS quantity_destroyed
            FROM killmail_victims kv
            JOIN sde_types st ON st.type_id = kv.ship_type_id
            WHERE kv.kill_time >= NOW() - INTERVAL '7 days'
              AND st.group_id != 29
        ),
        combined AS (
            SELECT * FROM item_rows
            UNION ALL
            SELECT * FROM victim_rows
        )
        INSERT INTO daily_destruction (type_id, date, quantity_destroyed, kill_count)
        SELECT combined.type_id,
               combined.date,
               SUM(combined.quantity_destroyed)::bigint AS quantity_destroyed,
               COUNT(DISTINCT combined.killmail_id)::int AS kill_count
        FROM combined
        GROUP BY combined.type_id, combined.date
        ON CONFLICT (type_id, date) DO UPDATE
        SET quantity_destroyed = EXCLUDED.quantity_destroyed,
            kill_count = EXCLUDED.kill_count
        "#,
    )
    .execute(pool)
    .await?;

    Ok(result.rows_affected())
}
