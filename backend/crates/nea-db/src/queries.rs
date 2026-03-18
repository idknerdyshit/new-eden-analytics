use std::time::Instant;

use chrono::{DateTime, NaiveDate, Utc};
use sqlx::PgPool;
use tracing::debug;
use uuid::Uuid;

use crate::error::DbError;
use crate::models::*;

// ═══════════════════════════════════════════════════════════════════
// SDE queries
// ═══════════════════════════════════════════════════════════════════

pub async fn search_types(
    pool: &PgPool,
    query: &str,
    limit: i32,
    offset: i32,
) -> Result<Vec<SdeType>, DbError> {
    let start = Instant::now();
    let rows = sqlx::query_as::<_, SdeType>(
        r#"
        SELECT type_id, name, group_id, group_name, category_id, category_name,
               market_group_id, volume, published
        FROM sde_types
        WHERE to_tsvector('english', name) @@ plainto_tsquery('english', $1)
        ORDER BY name
        LIMIT $2 OFFSET $3
        "#,
    )
    .bind(query)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await?;
    debug!(query, rows = rows.len(), elapsed_ms = start.elapsed().as_millis() as u64, "search_types");
    Ok(rows)
}

pub async fn search_types_count(
    pool: &PgPool,
    query: &str,
) -> Result<i64, DbError> {
    let start = Instant::now();
    let (count,): (i64,) = sqlx::query_as(
        r#"
        SELECT COUNT(*)
        FROM sde_types
        WHERE to_tsvector('english', name) @@ plainto_tsquery('english', $1)
        "#,
    )
    .bind(query)
    .fetch_one(pool)
    .await?;
    debug!(query, count, elapsed_ms = start.elapsed().as_millis() as u64, "search_types_count");
    Ok(count)
}

pub async fn get_type(pool: &PgPool, type_id: i32) -> Result<Option<SdeType>, DbError> {
    let start = Instant::now();
    let row = sqlx::query_as::<_, SdeType>(
        r#"
        SELECT type_id, name, group_id, group_name, category_id, category_name,
               market_group_id, volume, published
        FROM sde_types
        WHERE type_id = $1
        "#,
    )
    .bind(type_id)
    .fetch_optional(pool)
    .await?;
    debug!(type_id, found = row.is_some(), elapsed_ms = start.elapsed().as_millis() as u64, "get_type");
    Ok(row)
}

pub async fn get_product_materials(
    pool: &PgPool,
    product_type_id: i32,
) -> Result<Vec<ProductMaterial>, DbError> {
    let start = Instant::now();
    let rows = sqlx::query_as::<_, ProductMaterial>(
        r#"
        SELECT product_type_id, product_name, material_type_id, material_name, quantity
        FROM v_product_materials
        WHERE product_type_id = $1
        ORDER BY material_name
        "#,
    )
    .bind(product_type_id)
    .fetch_all(pool)
    .await?;
    debug!(product_type_id, rows = rows.len(), elapsed_ms = start.elapsed().as_millis() as u64, "get_product_materials");
    Ok(rows)
}

/// Get the list of tracked type IDs: materials used in manufacturing UNION product types.
pub async fn get_tracked_type_ids(pool: &PgPool) -> Result<Vec<i32>, DbError> {
    let start = Instant::now();
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
    let ids: Vec<i32> = rows.into_iter().map(|r| r.0).collect();
    debug!(count = ids.len(), elapsed_ms = start.elapsed().as_millis() as u64, "get_tracked_type_ids");
    Ok(ids)
}

// ═══════════════════════════════════════════════════════════════════
// Market queries
// ═══════════════════════════════════════════════════════════════════

pub async fn get_market_history(
    pool: &PgPool,
    type_id: i32,
    region_id: i32,
    days: i32,
) -> Result<Vec<MarketHistory>, DbError> {
    let start = Instant::now();
    let rows = sqlx::query_as::<_, MarketHistory>(
        r#"
        SELECT type_id, region_id, date, average, highest, lowest, volume, order_count
        FROM market_history
        WHERE type_id = $1 AND region_id = $2
          AND date >= CURRENT_DATE - $3 * INTERVAL '1 day'
        ORDER BY date
        "#,
    )
    .bind(type_id)
    .bind(region_id)
    .bind(days)
    .fetch_all(pool)
    .await?;
    debug!(type_id, region_id, days, rows = rows.len(), elapsed_ms = start.elapsed().as_millis() as u64, "get_market_history");
    Ok(rows)
}

pub async fn get_market_snapshots(
    pool: &PgPool,
    type_id: i32,
    region_id: i32,
    hours: i32,
) -> Result<Vec<MarketSnapshot>, DbError> {
    let start = Instant::now();
    let rows = sqlx::query_as::<_, MarketSnapshot>(
        r#"
        SELECT type_id, region_id, station_id, ts, best_bid, best_ask,
               bid_volume, ask_volume, spread
        FROM market_snapshots
        WHERE type_id = $1 AND region_id = $2
          AND ts >= NOW() - $3 * INTERVAL '1 hour'
        ORDER BY ts
        "#,
    )
    .bind(type_id)
    .bind(region_id)
    .bind(hours)
    .fetch_all(pool)
    .await?;
    debug!(type_id, region_id, hours, rows = rows.len(), elapsed_ms = start.elapsed().as_millis() as u64, "get_market_snapshots");
    Ok(rows)
}

pub async fn insert_market_history(
    pool: &PgPool,
    rows: &[MarketHistory],
) -> Result<(), DbError> {
    for row in rows {
        sqlx::query(
            r#"
            INSERT INTO market_history (type_id, region_id, date, average, highest, lowest, volume, order_count)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            ON CONFLICT DO NOTHING
            "#,
        )
        .bind(row.type_id)
        .bind(row.region_id)
        .bind(row.date)
        .bind(row.average)
        .bind(row.highest)
        .bind(row.lowest)
        .bind(row.volume)
        .bind(row.order_count)
        .execute(pool)
        .await?;
    }
    Ok(())
}

pub async fn insert_market_snapshot(
    pool: &PgPool,
    snap: &MarketSnapshot,
) -> Result<(), DbError> {
    sqlx::query(
        r#"
        INSERT INTO market_snapshots (type_id, region_id, station_id, ts, best_bid, best_ask,
                                      bid_volume, ask_volume, spread)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
        "#,
    )
    .bind(snap.type_id)
    .bind(snap.region_id)
    .bind(snap.station_id)
    .bind(snap.ts)
    .bind(snap.best_bid)
    .bind(snap.best_ask)
    .bind(snap.bid_volume)
    .bind(snap.ask_volume)
    .bind(snap.spread)
    .execute(pool)
    .await?;
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════
// Kill queries
// ═══════════════════════════════════════════════════════════════════

pub async fn insert_killmail(pool: &PgPool, km: &Killmail) -> Result<(), DbError> {
    sqlx::query(
        r#"
        INSERT INTO killmails (killmail_id, kill_time, solar_system_id, total_value, r2z2_sequence_id)
        VALUES ($1, $2, $3, $4, $5)
        ON CONFLICT DO NOTHING
        "#,
    )
    .bind(km.killmail_id)
    .bind(km.kill_time)
    .bind(km.solar_system_id)
    .bind(km.total_value)
    .bind(km.r2z2_sequence_id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn insert_killmail_items(
    pool: &PgPool,
    items: &[KillmailItem],
) -> Result<(), DbError> {
    for item in items {
        sqlx::query(
            r#"
            INSERT INTO killmail_items (killmail_id, kill_time, type_id, quantity_destroyed, quantity_dropped)
            VALUES ($1, $2, $3, $4, $5)
            ON CONFLICT DO NOTHING
            "#,
        )
        .bind(item.killmail_id)
        .bind(item.kill_time)
        .bind(item.type_id)
        .bind(item.quantity_destroyed)
        .bind(item.quantity_dropped)
        .execute(pool)
        .await?;
    }
    Ok(())
}

pub async fn insert_killmail_victim(
    pool: &PgPool,
    victim: &KillmailVictim,
) -> Result<(), DbError> {
    sqlx::query(
        r#"
        INSERT INTO killmail_victims (killmail_id, kill_time, ship_type_id)
        VALUES ($1, $2, $3)
        ON CONFLICT DO NOTHING
        "#,
    )
    .bind(victim.killmail_id)
    .bind(victim.kill_time)
    .bind(victim.ship_type_id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn get_daily_destruction(
    pool: &PgPool,
    type_id: i32,
    days: i32,
) -> Result<Vec<DailyDestruction>, DbError> {
    let start = Instant::now();
    let rows = sqlx::query_as::<_, DailyDestruction>(
        r#"
        SELECT type_id, date, quantity_destroyed, kill_count
        FROM daily_destruction
        WHERE type_id = $1
          AND date >= CURRENT_DATE - $2 * INTERVAL '1 day'
        ORDER BY date
        "#,
    )
    .bind(type_id)
    .bind(days)
    .fetch_all(pool)
    .await?;
    debug!(type_id, days, rows = rows.len(), elapsed_ms = start.elapsed().as_millis() as u64, "get_daily_destruction");
    Ok(rows)
}

pub async fn upsert_daily_destruction(
    pool: &PgPool,
    rows: &[DailyDestruction],
) -> Result<(), DbError> {
    for row in rows {
        sqlx::query(
            r#"
            INSERT INTO daily_destruction (type_id, date, quantity_destroyed, kill_count)
            VALUES ($1, $2, $3, $4)
            ON CONFLICT (type_id, date) DO UPDATE
            SET quantity_destroyed = EXCLUDED.quantity_destroyed,
                kill_count = EXCLUDED.kill_count
            "#,
        )
        .bind(row.type_id)
        .bind(row.date)
        .bind(row.quantity_destroyed)
        .bind(row.kill_count)
        .execute(pool)
        .await?;
    }
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════
// Dashboard queries
// ═══════════════════════════════════════════════════════════════════

pub async fn get_top_destruction(
    pool: &PgPool,
    days: i32,
    limit: i32,
) -> Result<Vec<DailyDestruction>, DbError> {
    let start = Instant::now();
    let rows = sqlx::query_as::<_, DailyDestruction>(
        r#"
        SELECT type_id, date, quantity_destroyed, kill_count
        FROM daily_destruction
        WHERE date >= CURRENT_DATE - $1 * INTERVAL '1 day'
        ORDER BY quantity_destroyed DESC
        LIMIT $2
        "#,
    )
    .bind(days)
    .bind(limit)
    .fetch_all(pool)
    .await?;
    debug!(days, limit, rows = rows.len(), elapsed_ms = start.elapsed().as_millis() as u64, "get_top_destruction");
    Ok(rows)
}

pub async fn get_movers(
    pool: &PgPool,
    limit: i32,
) -> Result<Vec<Mover>, DbError> {
    let start = Instant::now();
    let rows = sqlx::query_as::<_, Mover>(
        r#"
        WITH recent AS (
            SELECT
                mh.type_id,
                st.name,
                mh.date,
                mh.average,
                ROW_NUMBER() OVER (PARTITION BY mh.type_id ORDER BY mh.date DESC) AS rn
            FROM market_history mh
            JOIN sde_types st ON st.type_id = mh.type_id
            WHERE mh.region_id = 10000002
              AND mh.date >= CURRENT_DATE - INTERVAL '3 days'
        ),
        pairs AS (
            SELECT
                r1.type_id,
                r1.name,
                r2.average AS previous_avg,
                r1.average AS current_avg
            FROM recent r1
            JOIN recent r2 ON r1.type_id = r2.type_id AND r2.rn = 2
            WHERE r1.rn = 1 AND r2.average > 0
        )
        SELECT
            type_id,
            name,
            previous_avg,
            current_avg,
            ((current_avg - previous_avg) / previous_avg * 100.0) AS change_pct
        FROM pairs
        ORDER BY ABS((current_avg - previous_avg) / previous_avg) DESC
        LIMIT $1
        "#,
    )
    .bind(limit)
    .fetch_all(pool)
    .await?;
    debug!(limit, rows = rows.len(), elapsed_ms = start.elapsed().as_millis() as u64, "get_movers");
    Ok(rows)
}

// ═══════════════════════════════════════════════════════════════════
// Analysis queries
// ═══════════════════════════════════════════════════════════════════

pub async fn upsert_correlation(
    pool: &PgPool,
    result: &CorrelationResult,
) -> Result<(), DbError> {
    sqlx::query(
        r#"
        INSERT INTO correlation_results
            (product_type_id, material_type_id, lag_days, correlation_coeff,
             granger_f_stat, granger_p_value, granger_significant,
             window_start, window_end, computed_at)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
        ON CONFLICT (product_type_id, material_type_id) DO UPDATE
        SET lag_days = EXCLUDED.lag_days,
            correlation_coeff = EXCLUDED.correlation_coeff,
            granger_f_stat = EXCLUDED.granger_f_stat,
            granger_p_value = EXCLUDED.granger_p_value,
            granger_significant = EXCLUDED.granger_significant,
            window_start = EXCLUDED.window_start,
            window_end = EXCLUDED.window_end,
            computed_at = EXCLUDED.computed_at
        "#,
    )
    .bind(result.product_type_id)
    .bind(result.material_type_id)
    .bind(result.lag_days)
    .bind(result.correlation_coeff)
    .bind(result.granger_f_stat)
    .bind(result.granger_p_value)
    .bind(result.granger_significant)
    .bind(result.window_start)
    .bind(result.window_end)
    .bind(result.computed_at)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn get_correlations_for_product(
    pool: &PgPool,
    product_type_id: i32,
) -> Result<Vec<CorrelationResult>, DbError> {
    let start = Instant::now();
    let rows = sqlx::query_as::<_, CorrelationResult>(
        r#"
        SELECT cr.id, cr.product_type_id,
               COALESCE(sp.name, 'Type ' || cr.product_type_id) AS product_name,
               cr.material_type_id,
               COALESCE(sm.name, 'Type ' || cr.material_type_id) AS material_name,
               cr.lag_days, cr.correlation_coeff,
               cr.granger_f_stat, cr.granger_p_value, cr.granger_significant,
               cr.window_start, cr.window_end, cr.computed_at
        FROM correlation_results cr
        LEFT JOIN sde_types sp ON sp.type_id = cr.product_type_id
        LEFT JOIN sde_types sm ON sm.type_id = cr.material_type_id
        WHERE cr.product_type_id = $1
        ORDER BY ABS(cr.correlation_coeff) DESC
        "#,
    )
    .bind(product_type_id)
    .fetch_all(pool)
    .await?;
    debug!(product_type_id, rows = rows.len(), elapsed_ms = start.elapsed().as_millis() as u64, "get_correlations_for_product");
    Ok(rows)
}

pub async fn get_top_correlations(
    pool: &PgPool,
    limit: i32,
) -> Result<Vec<CorrelationResult>, DbError> {
    let start = Instant::now();
    let rows = sqlx::query_as::<_, CorrelationResult>(
        r#"
        SELECT cr.id, cr.product_type_id,
               COALESCE(sp.name, 'Type ' || cr.product_type_id) AS product_name,
               cr.material_type_id,
               COALESCE(sm.name, 'Type ' || cr.material_type_id) AS material_name,
               cr.lag_days, cr.correlation_coeff,
               cr.granger_f_stat, cr.granger_p_value, cr.granger_significant,
               cr.window_start, cr.window_end, cr.computed_at
        FROM correlation_results cr
        LEFT JOIN sde_types sp ON sp.type_id = cr.product_type_id
        LEFT JOIN sde_types sm ON sm.type_id = cr.material_type_id
        ORDER BY ABS(cr.correlation_coeff) DESC
        LIMIT $1
        "#,
    )
    .bind(limit)
    .fetch_all(pool)
    .await?;
    debug!(limit, rows = rows.len(), elapsed_ms = start.elapsed().as_millis() as u64, "get_top_correlations");
    Ok(rows)
}

/// Returns (product_type_id, material_type_id) pairs from the product-materials view.
pub async fn get_all_product_material_pairs(
    pool: &PgPool,
) -> Result<Vec<(i32, i32)>, DbError> {
    let start = Instant::now();
    let rows: Vec<(i32, i32)> = sqlx::query_as(
        r#"
        SELECT DISTINCT product_type_id, material_type_id
        FROM v_product_materials
        ORDER BY product_type_id, material_type_id
        "#,
    )
    .fetch_all(pool)
    .await?;
    debug!(rows = rows.len(), elapsed_ms = start.elapsed().as_millis() as u64, "get_all_product_material_pairs");
    Ok(rows)
}

/// Returns (date, quantity_destroyed) series for a given type within a date range.
pub async fn get_destruction_series(
    pool: &PgPool,
    type_id: i32,
    start: NaiveDate,
    end: NaiveDate,
) -> Result<Vec<(NaiveDate, f64)>, DbError> {
    let timer = Instant::now();
    let rows: Vec<(NaiveDate, f64)> = sqlx::query_as(
        r#"
        SELECT date, quantity_destroyed::float8
        FROM daily_destruction
        WHERE type_id = $1 AND date >= $2 AND date <= $3
        ORDER BY date
        "#,
    )
    .bind(type_id)
    .bind(start)
    .bind(end)
    .fetch_all(pool)
    .await?;
    debug!(type_id, rows = rows.len(), elapsed_ms = timer.elapsed().as_millis() as u64, "get_destruction_series");
    Ok(rows)
}

/// Returns (date, average_price) series for a given type/region within a date range.
pub async fn get_price_series(
    pool: &PgPool,
    type_id: i32,
    region_id: i32,
    start: NaiveDate,
    end: NaiveDate,
) -> Result<Vec<(NaiveDate, f64)>, DbError> {
    let timer = Instant::now();
    let rows: Vec<(NaiveDate, f64)> = sqlx::query_as(
        r#"
        SELECT date, average
        FROM market_history
        WHERE type_id = $1 AND region_id = $2
          AND date >= $3 AND date <= $4
        ORDER BY date
        "#,
    )
    .bind(type_id)
    .bind(region_id)
    .bind(start)
    .bind(end)
    .fetch_all(pool)
    .await?;
    debug!(type_id, region_id, rows = rows.len(), elapsed_ms = timer.elapsed().as_millis() as u64, "get_price_series");
    Ok(rows)
}

// ═══════════════════════════════════════════════════════════════════
// Worker state
// ═══════════════════════════════════════════════════════════════════

pub async fn get_worker_state(pool: &PgPool, key: &str) -> Result<Option<String>, DbError> {
    let row: Option<(String,)> = sqlx::query_as(
        r#"
        SELECT value FROM worker_state WHERE key = $1
        "#,
    )
    .bind(key)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|r| r.0))
}

pub async fn set_worker_state(pool: &PgPool, key: &str, value: &str) -> Result<(), DbError> {
    sqlx::query(
        r#"
        INSERT INTO worker_state (key, value, updated_at)
        VALUES ($1, $2, NOW())
        ON CONFLICT (key) DO UPDATE
        SET value = EXCLUDED.value, updated_at = NOW()
        "#,
    )
    .bind(key)
    .bind(value)
    .execute(pool)
    .await?;
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════
// User / Auth
// ═══════════════════════════════════════════════════════════════════

pub async fn upsert_user(
    pool: &PgPool,
    character_id: i64,
    character_name: &str,
    access_token_enc: &[u8],
    refresh_token_enc: &[u8],
    token_expires_at: DateTime<Utc>,
) -> Result<(), DbError> {
    sqlx::query(
        r#"
        INSERT INTO users (character_id, character_name, access_token_enc, refresh_token_enc,
                           token_expires_at, created_at, updated_at)
        VALUES ($1, $2, $3, $4, $5, NOW(), NOW())
        ON CONFLICT (character_id) DO UPDATE
        SET character_name = EXCLUDED.character_name,
            access_token_enc = EXCLUDED.access_token_enc,
            refresh_token_enc = EXCLUDED.refresh_token_enc,
            token_expires_at = EXCLUDED.token_expires_at,
            updated_at = NOW()
        "#,
    )
    .bind(character_id)
    .bind(character_name)
    .bind(access_token_enc)
    .bind(refresh_token_enc)
    .bind(token_expires_at)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn create_session(pool: &PgPool, character_id: i64) -> Result<Uuid, DbError> {
    let session_id = Uuid::new_v4();
    sqlx::query(
        r#"
        INSERT INTO sessions (session_id, character_id, expires_at, created_at)
        VALUES ($1, $2, NOW() + INTERVAL '24 hours', NOW())
        "#,
    )
    .bind(session_id)
    .bind(character_id)
    .execute(pool)
    .await?;
    Ok(session_id)
}

pub async fn get_session(pool: &PgPool, session_id: Uuid) -> Result<Option<Session>, DbError> {
    let row = sqlx::query_as::<_, Session>(
        r#"
        SELECT session_id, character_id, expires_at, created_at
        FROM sessions
        WHERE session_id = $1 AND expires_at > NOW()
        "#,
    )
    .bind(session_id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

pub async fn delete_session(pool: &PgPool, session_id: Uuid) -> Result<(), DbError> {
    sqlx::query("DELETE FROM sessions WHERE session_id = $1")
        .bind(session_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn get_user(pool: &PgPool, character_id: i64) -> Result<Option<User>, DbError> {
    let row = sqlx::query_as::<_, User>(
        r#"
        SELECT character_id, character_name, token_expires_at, created_at, updated_at
        FROM users
        WHERE character_id = $1
        "#,
    )
    .bind(character_id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}
