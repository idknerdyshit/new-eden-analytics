use std::time::Instant;

use chrono::{DateTime, NaiveDate, Utc};
use sqlx::{PgPool, QueryBuilder, Postgres};
use tracing::debug;
use uuid::Uuid;

use crate::error::DbError;
use crate::models::*;

const BATCH_SIZE: usize = 500;

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

/// Get the list of tracked type IDs: materials used in manufacturing UNION product types,
/// excluding any types previously marked as non-tradable.
pub async fn get_tracked_type_ids(pool: &PgPool) -> Result<Vec<i32>, DbError> {
    let start = Instant::now();
    let rows: Vec<(i32,)> = sqlx::query_as(
        r#"
        SELECT DISTINCT type_id FROM (
            SELECT DISTINCT material_type_id AS type_id FROM sde_blueprint_materials
            UNION
            SELECT DISTINCT product_type_id AS type_id FROM sde_blueprints
        ) combined
        WHERE type_id NOT IN (SELECT type_id FROM non_tradable_types)
        ORDER BY type_id
        "#,
    )
    .fetch_all(pool)
    .await?;
    let ids: Vec<i32> = rows.into_iter().map(|r| r.0).collect();
    debug!(count = ids.len(), elapsed_ms = start.elapsed().as_millis() as u64, "get_tracked_type_ids");
    Ok(ids)
}

/// Mark a type as non-tradable so it is excluded from future market fetches.
pub async fn mark_type_non_tradable(pool: &PgPool, type_id: i32) -> Result<(), DbError> {
    sqlx::query(
        "INSERT INTO non_tradable_types (type_id) VALUES ($1) ON CONFLICT DO NOTHING",
    )
    .bind(type_id)
    .execute(pool)
    .await?;
    Ok(())
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
    for chunk in rows.chunks(BATCH_SIZE) {
        let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(
            "INSERT INTO market_history (type_id, region_id, date, average, highest, lowest, volume, order_count) ",
        );
        qb.push_values(chunk, |mut b, row| {
            b.push_bind(row.type_id)
                .push_bind(row.region_id)
                .push_bind(row.date)
                .push_bind(row.average)
                .push_bind(row.highest)
                .push_bind(row.lowest)
                .push_bind(row.volume)
                .push_bind(row.order_count);
        });
        qb.push(" ON CONFLICT DO NOTHING");
        qb.build().execute(pool).await?;
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

/// Delete all items for a killmail (used by backfill to replace flag=0 rows with proper flags).
pub async fn delete_killmail_items(
    pool: &PgPool,
    killmail_id: i64,
    kill_time: DateTime<Utc>,
) -> Result<(), DbError> {
    sqlx::query(
        "DELETE FROM killmail_items WHERE killmail_id = $1 AND kill_time = $2",
    )
    .bind(killmail_id)
    .bind(kill_time)
    .execute(pool)
    .await?;
    Ok(())
}

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
    for chunk in items.chunks(BATCH_SIZE) {
        let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(
            "INSERT INTO killmail_items (killmail_id, kill_time, type_id, quantity_destroyed, quantity_dropped, flag) ",
        );
        qb.push_values(chunk, |mut b, item| {
            b.push_bind(item.killmail_id)
                .push_bind(item.kill_time)
                .push_bind(item.type_id)
                .push_bind(item.quantity_destroyed)
                .push_bind(item.quantity_dropped)
                .push_bind(item.flag);
        });
        qb.push(
            " ON CONFLICT (killmail_id, kill_time, type_id, flag) DO UPDATE SET quantity_destroyed = EXCLUDED.quantity_destroyed, quantity_dropped = EXCLUDED.quantity_dropped",
        );
        qb.build().execute(pool).await?;
    }
    Ok(())
}

pub async fn insert_killmail_victim(
    pool: &PgPool,
    victim: &KillmailVictim,
) -> Result<(), DbError> {
    sqlx::query(
        r#"
        INSERT INTO killmail_victims (killmail_id, kill_time, ship_type_id, character_id, corporation_id, alliance_id)
        VALUES ($1, $2, $3, $4, $5, $6)
        ON CONFLICT (killmail_id, kill_time) DO UPDATE
        SET character_id = COALESCE(EXCLUDED.character_id, killmail_victims.character_id),
            corporation_id = COALESCE(EXCLUDED.corporation_id, killmail_victims.corporation_id),
            alliance_id = COALESCE(EXCLUDED.alliance_id, killmail_victims.alliance_id)
        "#,
    )
    .bind(victim.killmail_id)
    .bind(victim.kill_time)
    .bind(victim.ship_type_id)
    .bind(victim.character_id)
    .bind(victim.corporation_id)
    .bind(victim.alliance_id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn insert_killmail_attackers(
    pool: &PgPool,
    attackers: &[KillmailAttacker],
) -> Result<(), DbError> {
    for chunk in attackers.chunks(BATCH_SIZE) {
        let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(
            "INSERT INTO killmail_attackers (killmail_id, kill_time, character_id, corporation_id, alliance_id, ship_type_id, weapon_type_id, damage_done, final_blow) ",
        );
        qb.push_values(chunk, |mut b, a| {
            b.push_bind(a.killmail_id)
                .push_bind(a.kill_time)
                .push_bind(a.character_id)
                .push_bind(a.corporation_id)
                .push_bind(a.alliance_id)
                .push_bind(a.ship_type_id)
                .push_bind(a.weapon_type_id)
                .push_bind(a.damage_done)
                .push_bind(a.final_blow);
        });
        qb.push(" ON CONFLICT DO NOTHING");
        qb.build().execute(pool).await?;
    }
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
    for chunk in rows.chunks(BATCH_SIZE) {
        let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(
            "INSERT INTO daily_destruction (type_id, date, quantity_destroyed, kill_count) ",
        );
        qb.push_values(chunk, |mut b, row| {
            b.push_bind(row.type_id)
                .push_bind(row.date)
                .push_bind(row.quantity_destroyed)
                .push_bind(row.kill_count);
        });
        qb.push(
            " ON CONFLICT (type_id, date) DO UPDATE SET quantity_destroyed = EXCLUDED.quantity_destroyed, kill_count = EXCLUDED.kill_count",
        );
        qb.build().execute(pool).await?;
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
        SELECT dd.type_id, st.name AS type_name, dd.date, dd.quantity_destroyed, dd.kill_count
        FROM daily_destruction dd
        JOIN sde_types st ON st.type_id = dd.type_id
        WHERE dd.date >= CURRENT_DATE - $1 * INTERVAL '1 day'
          AND st.category_id IN (6, 7)
          AND st.group_id != 29
        ORDER BY dd.quantity_destroyed DESC
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

pub async fn cleanup_expired_sessions(pool: &PgPool) -> Result<u64, DbError> {
    let result = sqlx::query("DELETE FROM sessions WHERE expires_at <= NOW()")
        .execute(pool)
        .await?;
    Ok(result.rows_affected())
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

// ═══════════════════════════════════════════════════════════════════
// Character / Profile queries
// ═══════════════════════════════════════════════════════════════════

pub async fn upsert_character(pool: &PgPool, character: &Character) -> Result<(), DbError> {
    sqlx::query(
        r#"
        INSERT INTO characters (character_id, name, corporation_id, alliance_id, fetched_at)
        VALUES ($1, $2, $3, $4, $5)
        ON CONFLICT (character_id) DO UPDATE
        SET name = EXCLUDED.name,
            corporation_id = EXCLUDED.corporation_id,
            alliance_id = EXCLUDED.alliance_id,
            fetched_at = EXCLUDED.fetched_at
        "#,
    )
    .bind(character.character_id)
    .bind(&character.name)
    .bind(character.corporation_id)
    .bind(character.alliance_id)
    .bind(character.fetched_at)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn get_character(pool: &PgPool, character_id: i64) -> Result<Option<Character>, DbError> {
    let row = sqlx::query_as::<_, Character>(
        r#"
        SELECT character_id, name, corporation_id, alliance_id, fetched_at
        FROM characters
        WHERE character_id = $1
        "#,
    )
    .bind(character_id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

pub async fn search_characters(
    pool: &PgPool,
    query: &str,
    limit: i32,
    offset: i32,
) -> Result<Vec<Character>, DbError> {
    let start = Instant::now();
    let pattern = format!("%{}%", query);
    let rows = sqlx::query_as::<_, Character>(
        r#"
        SELECT character_id, name, corporation_id, alliance_id, fetched_at
        FROM characters
        WHERE name ILIKE $1
        ORDER BY name
        LIMIT $2 OFFSET $3
        "#,
    )
    .bind(&pattern)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await?;
    debug!(query, rows = rows.len(), elapsed_ms = start.elapsed().as_millis() as u64, "search_characters");
    Ok(rows)
}

pub async fn search_characters_count(
    pool: &PgPool,
    query: &str,
) -> Result<i64, DbError> {
    let pattern = format!("%{}%", query);
    let (count,): (i64,) = sqlx::query_as(
        r#"
        SELECT COUNT(*)
        FROM characters
        WHERE name ILIKE $1
        "#,
    )
    .bind(&pattern)
    .fetch_one(pool)
    .await?;
    Ok(count)
}

pub async fn get_character_profile(
    pool: &PgPool,
    character_id: i64,
) -> Result<Option<CharacterProfile>, DbError> {
    let row = sqlx::query_as::<_, CharacterProfile>(
        r#"
        SELECT character_id, total_kills, total_losses, solo_kills, solo_losses,
               top_ships_flown, top_ships_lost, common_fits, active_period, computed_at
        FROM character_profiles
        WHERE character_id = $1
        "#,
    )
    .bind(character_id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

pub async fn upsert_character_profile(
    pool: &PgPool,
    profile: &CharacterProfile,
) -> Result<(), DbError> {
    sqlx::query(
        r#"
        INSERT INTO character_profiles
            (character_id, total_kills, total_losses, solo_kills, solo_losses,
             top_ships_flown, top_ships_lost, common_fits, active_period, computed_at)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
        ON CONFLICT (character_id) DO UPDATE
        SET total_kills = EXCLUDED.total_kills,
            total_losses = EXCLUDED.total_losses,
            solo_kills = EXCLUDED.solo_kills,
            solo_losses = EXCLUDED.solo_losses,
            top_ships_flown = EXCLUDED.top_ships_flown,
            top_ships_lost = EXCLUDED.top_ships_lost,
            common_fits = EXCLUDED.common_fits,
            active_period = EXCLUDED.active_period,
            computed_at = EXCLUDED.computed_at
        "#,
    )
    .bind(profile.character_id)
    .bind(profile.total_kills)
    .bind(profile.total_losses)
    .bind(profile.solo_kills)
    .bind(profile.solo_losses)
    .bind(&profile.top_ships_flown)
    .bind(&profile.top_ships_lost)
    .bind(&profile.common_fits)
    .bind(&profile.active_period)
    .bind(profile.computed_at)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn get_character_kills(
    pool: &PgPool,
    character_id: i64,
    limit: i32,
) -> Result<Vec<Killmail>, DbError> {
    let start = Instant::now();
    let rows = sqlx::query_as::<_, Killmail>(
        r#"
        SELECT DISTINCT k.killmail_id, k.kill_time, k.solar_system_id, k.total_value, k.r2z2_sequence_id
        FROM killmail_attackers a
        JOIN killmails k ON k.killmail_id = a.killmail_id AND k.kill_time = a.kill_time
        WHERE a.character_id = $1
        ORDER BY k.kill_time DESC
        LIMIT $2
        "#,
    )
    .bind(character_id)
    .bind(limit)
    .fetch_all(pool)
    .await?;
    debug!(character_id, rows = rows.len(), elapsed_ms = start.elapsed().as_millis() as u64, "get_character_kills");
    Ok(rows)
}

pub async fn get_character_losses(
    pool: &PgPool,
    character_id: i64,
    limit: i32,
) -> Result<Vec<Killmail>, DbError> {
    let start = Instant::now();
    let rows = sqlx::query_as::<_, Killmail>(
        r#"
        SELECT DISTINCT k.killmail_id, k.kill_time, k.solar_system_id, k.total_value, k.r2z2_sequence_id
        FROM killmail_victims v
        JOIN killmails k ON k.killmail_id = v.killmail_id AND k.kill_time = v.kill_time
        WHERE v.character_id = $1
        ORDER BY k.kill_time DESC
        LIMIT $2
        "#,
    )
    .bind(character_id)
    .bind(limit)
    .fetch_all(pool)
    .await?;
    debug!(character_id, rows = rows.len(), elapsed_ms = start.elapsed().as_millis() as u64, "get_character_losses");
    Ok(rows)
}

// ═══════════════════════════════════════════════════════════════════
// Killmail summary / detail queries
// ═══════════════════════════════════════════════════════════════════

pub async fn get_character_kills_summary(
    pool: &PgPool,
    character_id: i64,
    limit: i32,
    offset: i32,
) -> Result<Vec<KillmailSummary>, DbError> {
    let start = Instant::now();
    let rows = sqlx::query_as::<_, KillmailSummary>(
        r#"
        SELECT k.killmail_id, k.kill_time, k.solar_system_id, k.total_value,
               v.ship_type_id AS victim_ship_type_id,
               st.name AS victim_ship_name,
               v.character_id AS victim_character_id,
               c.name AS victim_character_name,
               v.corporation_id AS victim_corporation_id,
               v.alliance_id AS victim_alliance_id,
               (SELECT COUNT(*) FROM killmail_attackers a2
                WHERE a2.killmail_id = k.killmail_id AND a2.kill_time = k.kill_time) AS attacker_count
        FROM killmail_attackers a
        JOIN killmails k ON k.killmail_id = a.killmail_id AND k.kill_time = a.kill_time
        LEFT JOIN killmail_victims v ON v.killmail_id = k.killmail_id AND v.kill_time = k.kill_time
        LEFT JOIN sde_types st ON st.type_id = v.ship_type_id
        LEFT JOIN characters c ON c.character_id = v.character_id
        WHERE a.character_id = $1
        GROUP BY k.killmail_id, k.kill_time, k.solar_system_id, k.total_value, k.r2z2_sequence_id,
                 v.ship_type_id, st.name, v.character_id, c.name, v.corporation_id, v.alliance_id
        ORDER BY k.kill_time DESC
        LIMIT $2 OFFSET $3
        "#,
    )
    .bind(character_id)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await?;
    debug!(character_id, rows = rows.len(), elapsed_ms = start.elapsed().as_millis() as u64, "get_character_kills_summary");
    Ok(rows)
}

pub async fn count_character_kills(pool: &PgPool, character_id: i64) -> Result<i64, DbError> {
    let (count,): (i64,) = sqlx::query_as(
        r#"
        SELECT COUNT(DISTINCT (a.killmail_id, a.kill_time))
        FROM killmail_attackers a
        WHERE a.character_id = $1
        "#,
    )
    .bind(character_id)
    .fetch_one(pool)
    .await?;
    Ok(count)
}

pub async fn get_character_losses_summary(
    pool: &PgPool,
    character_id: i64,
    limit: i32,
    offset: i32,
) -> Result<Vec<KillmailSummary>, DbError> {
    let start = Instant::now();
    let rows = sqlx::query_as::<_, KillmailSummary>(
        r#"
        SELECT k.killmail_id, k.kill_time, k.solar_system_id, k.total_value,
               v.ship_type_id AS victim_ship_type_id,
               st.name AS victim_ship_name,
               v.character_id AS victim_character_id,
               c.name AS victim_character_name,
               v.corporation_id AS victim_corporation_id,
               v.alliance_id AS victim_alliance_id,
               (SELECT COUNT(*) FROM killmail_attackers a2
                WHERE a2.killmail_id = k.killmail_id AND a2.kill_time = k.kill_time) AS attacker_count
        FROM killmail_victims v
        JOIN killmails k ON k.killmail_id = v.killmail_id AND k.kill_time = v.kill_time
        LEFT JOIN sde_types st ON st.type_id = v.ship_type_id
        LEFT JOIN characters c ON c.character_id = v.character_id
        WHERE v.character_id = $1
        ORDER BY k.kill_time DESC
        LIMIT $2 OFFSET $3
        "#,
    )
    .bind(character_id)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await?;
    debug!(character_id, rows = rows.len(), elapsed_ms = start.elapsed().as_millis() as u64, "get_character_losses_summary");
    Ok(rows)
}

pub async fn count_character_losses(pool: &PgPool, character_id: i64) -> Result<i64, DbError> {
    let (count,): (i64,) = sqlx::query_as(
        r#"
        SELECT COUNT(*)
        FROM killmail_victims v
        WHERE v.character_id = $1
        "#,
    )
    .bind(character_id)
    .fetch_one(pool)
    .await?;
    Ok(count)
}

pub async fn get_corporation_kills_summary(
    pool: &PgPool,
    corporation_id: i64,
    limit: i32,
    offset: i32,
) -> Result<Vec<KillmailSummary>, DbError> {
    let start = Instant::now();
    let rows = sqlx::query_as::<_, KillmailSummary>(
        r#"
        SELECT k.killmail_id, k.kill_time, k.solar_system_id, k.total_value,
               v.ship_type_id AS victim_ship_type_id,
               st.name AS victim_ship_name,
               v.character_id AS victim_character_id,
               c.name AS victim_character_name,
               v.corporation_id AS victim_corporation_id,
               v.alliance_id AS victim_alliance_id,
               (SELECT COUNT(*) FROM killmail_attackers a2
                WHERE a2.killmail_id = k.killmail_id AND a2.kill_time = k.kill_time) AS attacker_count
        FROM killmail_attackers a
        JOIN killmails k ON k.killmail_id = a.killmail_id AND k.kill_time = a.kill_time
        LEFT JOIN killmail_victims v ON v.killmail_id = k.killmail_id AND v.kill_time = k.kill_time
        LEFT JOIN sde_types st ON st.type_id = v.ship_type_id
        LEFT JOIN characters c ON c.character_id = v.character_id
        WHERE a.corporation_id = $1
        GROUP BY k.killmail_id, k.kill_time, k.solar_system_id, k.total_value, k.r2z2_sequence_id,
                 v.ship_type_id, st.name, v.character_id, c.name, v.corporation_id, v.alliance_id
        ORDER BY k.kill_time DESC
        LIMIT $2 OFFSET $3
        "#,
    )
    .bind(corporation_id)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await?;
    debug!(corporation_id, rows = rows.len(), elapsed_ms = start.elapsed().as_millis() as u64, "get_corporation_kills_summary");
    Ok(rows)
}

pub async fn count_corporation_kills(pool: &PgPool, corporation_id: i64) -> Result<i64, DbError> {
    let (count,): (i64,) = sqlx::query_as(
        r#"
        SELECT COUNT(DISTINCT (a.killmail_id, a.kill_time))
        FROM killmail_attackers a
        WHERE a.corporation_id = $1
        "#,
    )
    .bind(corporation_id)
    .fetch_one(pool)
    .await?;
    Ok(count)
}

pub async fn get_corporation_losses_summary(
    pool: &PgPool,
    corporation_id: i64,
    limit: i32,
    offset: i32,
) -> Result<Vec<KillmailSummary>, DbError> {
    let start = Instant::now();
    let rows = sqlx::query_as::<_, KillmailSummary>(
        r#"
        SELECT k.killmail_id, k.kill_time, k.solar_system_id, k.total_value,
               v.ship_type_id AS victim_ship_type_id,
               st.name AS victim_ship_name,
               v.character_id AS victim_character_id,
               c.name AS victim_character_name,
               v.corporation_id AS victim_corporation_id,
               v.alliance_id AS victim_alliance_id,
               (SELECT COUNT(*) FROM killmail_attackers a2
                WHERE a2.killmail_id = k.killmail_id AND a2.kill_time = k.kill_time) AS attacker_count
        FROM killmail_victims v
        JOIN killmails k ON k.killmail_id = v.killmail_id AND k.kill_time = v.kill_time
        LEFT JOIN sde_types st ON st.type_id = v.ship_type_id
        LEFT JOIN characters c ON c.character_id = v.character_id
        WHERE v.corporation_id = $1
        ORDER BY k.kill_time DESC
        LIMIT $2 OFFSET $3
        "#,
    )
    .bind(corporation_id)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await?;
    debug!(corporation_id, rows = rows.len(), elapsed_ms = start.elapsed().as_millis() as u64, "get_corporation_losses_summary");
    Ok(rows)
}

pub async fn count_corporation_losses(pool: &PgPool, corporation_id: i64) -> Result<i64, DbError> {
    let (count,): (i64,) = sqlx::query_as(
        r#"
        SELECT COUNT(*)
        FROM killmail_victims v
        WHERE v.corporation_id = $1
        "#,
    )
    .bind(corporation_id)
    .fetch_one(pool)
    .await?;
    Ok(count)
}

pub async fn get_alliance_kills_summary(
    pool: &PgPool,
    alliance_id: i64,
    limit: i32,
    offset: i32,
) -> Result<Vec<KillmailSummary>, DbError> {
    let start = Instant::now();
    let rows = sqlx::query_as::<_, KillmailSummary>(
        r#"
        SELECT k.killmail_id, k.kill_time, k.solar_system_id, k.total_value,
               v.ship_type_id AS victim_ship_type_id,
               st.name AS victim_ship_name,
               v.character_id AS victim_character_id,
               c.name AS victim_character_name,
               v.corporation_id AS victim_corporation_id,
               v.alliance_id AS victim_alliance_id,
               (SELECT COUNT(*) FROM killmail_attackers a2
                WHERE a2.killmail_id = k.killmail_id AND a2.kill_time = k.kill_time) AS attacker_count
        FROM killmail_attackers a
        JOIN killmails k ON k.killmail_id = a.killmail_id AND k.kill_time = a.kill_time
        LEFT JOIN killmail_victims v ON v.killmail_id = k.killmail_id AND v.kill_time = k.kill_time
        LEFT JOIN sde_types st ON st.type_id = v.ship_type_id
        LEFT JOIN characters c ON c.character_id = v.character_id
        WHERE a.alliance_id = $1
        GROUP BY k.killmail_id, k.kill_time, k.solar_system_id, k.total_value, k.r2z2_sequence_id,
                 v.ship_type_id, st.name, v.character_id, c.name, v.corporation_id, v.alliance_id
        ORDER BY k.kill_time DESC
        LIMIT $2 OFFSET $3
        "#,
    )
    .bind(alliance_id)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await?;
    debug!(alliance_id, rows = rows.len(), elapsed_ms = start.elapsed().as_millis() as u64, "get_alliance_kills_summary");
    Ok(rows)
}

pub async fn count_alliance_kills(pool: &PgPool, alliance_id: i64) -> Result<i64, DbError> {
    let (count,): (i64,) = sqlx::query_as(
        r#"
        SELECT COUNT(DISTINCT (a.killmail_id, a.kill_time))
        FROM killmail_attackers a
        WHERE a.alliance_id = $1
        "#,
    )
    .bind(alliance_id)
    .fetch_one(pool)
    .await?;
    Ok(count)
}

pub async fn get_alliance_losses_summary(
    pool: &PgPool,
    alliance_id: i64,
    limit: i32,
    offset: i32,
) -> Result<Vec<KillmailSummary>, DbError> {
    let start = Instant::now();
    let rows = sqlx::query_as::<_, KillmailSummary>(
        r#"
        SELECT k.killmail_id, k.kill_time, k.solar_system_id, k.total_value,
               v.ship_type_id AS victim_ship_type_id,
               st.name AS victim_ship_name,
               v.character_id AS victim_character_id,
               c.name AS victim_character_name,
               v.corporation_id AS victim_corporation_id,
               v.alliance_id AS victim_alliance_id,
               (SELECT COUNT(*) FROM killmail_attackers a2
                WHERE a2.killmail_id = k.killmail_id AND a2.kill_time = k.kill_time) AS attacker_count
        FROM killmail_victims v
        JOIN killmails k ON k.killmail_id = v.killmail_id AND k.kill_time = v.kill_time
        LEFT JOIN sde_types st ON st.type_id = v.ship_type_id
        LEFT JOIN characters c ON c.character_id = v.character_id
        WHERE v.alliance_id = $1
        ORDER BY k.kill_time DESC
        LIMIT $2 OFFSET $3
        "#,
    )
    .bind(alliance_id)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await?;
    debug!(alliance_id, rows = rows.len(), elapsed_ms = start.elapsed().as_millis() as u64, "get_alliance_losses_summary");
    Ok(rows)
}

pub async fn count_alliance_losses(pool: &PgPool, alliance_id: i64) -> Result<i64, DbError> {
    let (count,): (i64,) = sqlx::query_as(
        r#"
        SELECT COUNT(*)
        FROM killmail_victims v
        WHERE v.alliance_id = $1
        "#,
    )
    .bind(alliance_id)
    .fetch_one(pool)
    .await?;
    Ok(count)
}

pub async fn get_killmail_by_id(pool: &PgPool, killmail_id: i64) -> Result<Option<Killmail>, DbError> {
    let start = Instant::now();
    let row = sqlx::query_as::<_, Killmail>(
        r#"
        SELECT killmail_id, kill_time, solar_system_id, total_value, r2z2_sequence_id
        FROM killmails
        WHERE killmail_id = $1
        LIMIT 1
        "#,
    )
    .bind(killmail_id)
    .fetch_optional(pool)
    .await?;
    debug!(killmail_id, found = row.is_some(), elapsed_ms = start.elapsed().as_millis() as u64, "get_killmail_by_id");
    Ok(row)
}

pub async fn get_killmail_victim_detail(
    pool: &PgPool,
    killmail_id: i64,
    kill_time: DateTime<Utc>,
) -> Result<Option<KillmailVictimDetail>, DbError> {
    let row = sqlx::query_as::<_, KillmailVictimDetail>(
        r#"
        SELECT v.ship_type_id,
               st.name AS ship_name,
               v.character_id,
               c.name AS character_name,
               v.corporation_id,
               co.name AS corporation_name,
               v.alliance_id,
               al.name AS alliance_name
        FROM killmail_victims v
        LEFT JOIN sde_types st ON st.type_id = v.ship_type_id
        LEFT JOIN characters c ON c.character_id = v.character_id
        LEFT JOIN corporations co ON co.corporation_id = v.corporation_id
        LEFT JOIN alliances al ON al.alliance_id = v.alliance_id
        WHERE v.killmail_id = $1 AND v.kill_time = $2
        "#,
    )
    .bind(killmail_id)
    .bind(kill_time)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

pub async fn get_killmail_attackers_detail(
    pool: &PgPool,
    killmail_id: i64,
    kill_time: DateTime<Utc>,
) -> Result<Vec<KillmailAttackerDetail>, DbError> {
    let rows = sqlx::query_as::<_, KillmailAttackerDetail>(
        r#"
        SELECT a.character_id,
               c.name AS character_name,
               a.corporation_id,
               co.name AS corporation_name,
               a.alliance_id,
               al.name AS alliance_name,
               a.ship_type_id,
               st.name AS ship_name,
               a.weapon_type_id,
               wt.name AS weapon_name,
               a.damage_done,
               a.final_blow
        FROM killmail_attackers a
        LEFT JOIN characters c ON c.character_id = a.character_id
        LEFT JOIN corporations co ON co.corporation_id = a.corporation_id
        LEFT JOIN alliances al ON al.alliance_id = a.alliance_id
        LEFT JOIN sde_types st ON st.type_id = a.ship_type_id
        LEFT JOIN sde_types wt ON wt.type_id = a.weapon_type_id
        WHERE a.killmail_id = $1 AND a.kill_time = $2
        ORDER BY a.damage_done DESC
        "#,
    )
    .bind(killmail_id)
    .bind(kill_time)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn get_killmail_items_detail(
    pool: &PgPool,
    killmail_id: i64,
    kill_time: DateTime<Utc>,
) -> Result<Vec<KillmailItemDetail>, DbError> {
    let rows = sqlx::query_as::<_, KillmailItemDetail>(
        r#"
        SELECT i.type_id,
               st.name AS type_name,
               i.quantity_destroyed,
               i.quantity_dropped,
               i.flag
        FROM killmail_items i
        LEFT JOIN sde_types st ON st.type_id = i.type_id
        WHERE i.killmail_id = $1 AND i.kill_time = $2
        ORDER BY i.flag, st.name
        "#,
    )
    .bind(killmail_id)
    .bind(kill_time)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Get character IDs that have activity but are not yet in the characters cache.
pub async fn get_uncached_character_ids(
    pool: &PgPool,
    limit: i32,
) -> Result<Vec<i64>, DbError> {
    let rows: Vec<(i64,)> = sqlx::query_as(
        r#"
        SELECT character_id FROM (
            SELECT DISTINCT character_id FROM killmail_attackers WHERE character_id IS NOT NULL
            UNION
            SELECT DISTINCT character_id FROM killmail_victims WHERE character_id IS NOT NULL
        ) all_chars
        WHERE character_id NOT IN (SELECT character_id FROM characters)
        LIMIT $1
        "#,
    )
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(|r| r.0).collect())
}

/// Get character IDs with activity since the given timestamp.
pub async fn get_active_character_ids_since(
    pool: &PgPool,
    since: DateTime<Utc>,
) -> Result<Vec<i64>, DbError> {
    let rows: Vec<(i64,)> = sqlx::query_as(
        r#"
        SELECT DISTINCT character_id FROM (
            SELECT character_id FROM killmail_attackers
            WHERE character_id IS NOT NULL AND kill_time > $1
            UNION
            SELECT character_id FROM killmail_victims
            WHERE character_id IS NOT NULL AND kill_time > $1
        ) active
        "#,
    )
    .bind(since)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(|r| r.0).collect())
}

// ═══════════════════════════════════════════════════════════════════
// Corporation / Alliance / Doctrine queries
// ═══════════════════════════════════════════════════════════════════

pub async fn upsert_corporation(pool: &PgPool, corp: &Corporation) -> Result<(), DbError> {
    sqlx::query(
        r#"
        INSERT INTO corporations (corporation_id, name, alliance_id, member_count, fetched_at)
        VALUES ($1, $2, $3, $4, $5)
        ON CONFLICT (corporation_id) DO UPDATE
        SET name = EXCLUDED.name,
            alliance_id = EXCLUDED.alliance_id,
            member_count = EXCLUDED.member_count,
            fetched_at = EXCLUDED.fetched_at
        "#,
    )
    .bind(corp.corporation_id)
    .bind(&corp.name)
    .bind(corp.alliance_id)
    .bind(corp.member_count)
    .bind(corp.fetched_at)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn get_corporation(pool: &PgPool, corporation_id: i64) -> Result<Option<Corporation>, DbError> {
    let row = sqlx::query_as::<_, Corporation>(
        r#"
        SELECT corporation_id, name, alliance_id, member_count, fetched_at
        FROM corporations
        WHERE corporation_id = $1
        "#,
    )
    .bind(corporation_id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

pub async fn search_corporations(
    pool: &PgPool,
    query: &str,
    limit: i32,
    offset: i32,
) -> Result<Vec<Corporation>, DbError> {
    let start = Instant::now();
    let pattern = format!("%{}%", query);
    let rows = sqlx::query_as::<_, Corporation>(
        r#"
        SELECT corporation_id, name, alliance_id, member_count, fetched_at
        FROM corporations
        WHERE name ILIKE $1
        ORDER BY name
        LIMIT $2 OFFSET $3
        "#,
    )
    .bind(&pattern)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await?;
    debug!(query, rows = rows.len(), elapsed_ms = start.elapsed().as_millis() as u64, "search_corporations");
    Ok(rows)
}

pub async fn search_corporations_count(
    pool: &PgPool,
    query: &str,
) -> Result<i64, DbError> {
    let pattern = format!("%{}%", query);
    let (count,): (i64,) = sqlx::query_as(
        r#"
        SELECT COUNT(*)
        FROM corporations
        WHERE name ILIKE $1
        "#,
    )
    .bind(&pattern)
    .fetch_one(pool)
    .await?;
    Ok(count)
}

pub async fn upsert_alliance(pool: &PgPool, alliance: &Alliance) -> Result<(), DbError> {
    sqlx::query(
        r#"
        INSERT INTO alliances (alliance_id, name, ticker, fetched_at)
        VALUES ($1, $2, $3, $4)
        ON CONFLICT (alliance_id) DO UPDATE
        SET name = EXCLUDED.name,
            ticker = EXCLUDED.ticker,
            fetched_at = EXCLUDED.fetched_at
        "#,
    )
    .bind(alliance.alliance_id)
    .bind(&alliance.name)
    .bind(&alliance.ticker)
    .bind(alliance.fetched_at)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn get_alliance(pool: &PgPool, alliance_id: i64) -> Result<Option<Alliance>, DbError> {
    let row = sqlx::query_as::<_, Alliance>(
        r#"
        SELECT alliance_id, name, ticker, fetched_at
        FROM alliances
        WHERE alliance_id = $1
        "#,
    )
    .bind(alliance_id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

pub async fn search_alliances(
    pool: &PgPool,
    query: &str,
    limit: i32,
    offset: i32,
) -> Result<Vec<Alliance>, DbError> {
    let start = Instant::now();
    let pattern = format!("%{}%", query);
    let rows = sqlx::query_as::<_, Alliance>(
        r#"
        SELECT alliance_id, name, ticker, fetched_at
        FROM alliances
        WHERE name ILIKE $1
        ORDER BY name
        LIMIT $2 OFFSET $3
        "#,
    )
    .bind(&pattern)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await?;
    debug!(query, rows = rows.len(), elapsed_ms = start.elapsed().as_millis() as u64, "search_alliances");
    Ok(rows)
}

pub async fn search_alliances_count(
    pool: &PgPool,
    query: &str,
) -> Result<i64, DbError> {
    let pattern = format!("%{}%", query);
    let (count,): (i64,) = sqlx::query_as(
        r#"
        SELECT COUNT(*)
        FROM alliances
        WHERE name ILIKE $1
        "#,
    )
    .bind(&pattern)
    .fetch_one(pool)
    .await?;
    Ok(count)
}

pub async fn upsert_doctrine_profile(
    pool: &PgPool,
    profile: &DoctrineProfile,
) -> Result<(), DbError> {
    sqlx::query(
        r#"
        INSERT INTO doctrine_profiles
            (entity_type, entity_id, entity_name, window_days, member_count,
             total_kills, total_losses, ship_usage, doctrines, ship_trends, fleet_comps, computed_at)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
        ON CONFLICT (entity_type, entity_id, window_days) DO UPDATE
        SET entity_name = EXCLUDED.entity_name,
            member_count = EXCLUDED.member_count,
            total_kills = EXCLUDED.total_kills,
            total_losses = EXCLUDED.total_losses,
            ship_usage = EXCLUDED.ship_usage,
            doctrines = EXCLUDED.doctrines,
            ship_trends = EXCLUDED.ship_trends,
            fleet_comps = EXCLUDED.fleet_comps,
            computed_at = EXCLUDED.computed_at
        "#,
    )
    .bind(&profile.entity_type)
    .bind(profile.entity_id)
    .bind(&profile.entity_name)
    .bind(profile.window_days)
    .bind(profile.member_count)
    .bind(profile.total_kills)
    .bind(profile.total_losses)
    .bind(&profile.ship_usage)
    .bind(&profile.doctrines)
    .bind(&profile.ship_trends)
    .bind(&profile.fleet_comps)
    .bind(profile.computed_at)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn get_doctrine_profiles(
    pool: &PgPool,
    entity_type: &str,
    entity_id: i64,
) -> Result<Vec<DoctrineProfile>, DbError> {
    let start = Instant::now();
    let rows = sqlx::query_as::<_, DoctrineProfile>(
        r#"
        SELECT id, entity_type, entity_id, entity_name, window_days, member_count,
               total_kills, total_losses, ship_usage, doctrines, ship_trends, fleet_comps, computed_at
        FROM doctrine_profiles
        WHERE entity_type = $1 AND entity_id = $2
        ORDER BY window_days
        "#,
    )
    .bind(entity_type)
    .bind(entity_id)
    .fetch_all(pool)
    .await?;
    debug!(entity_type, entity_id, rows = rows.len(), elapsed_ms = start.elapsed().as_millis() as u64, "get_doctrine_profiles");
    Ok(rows)
}

pub async fn get_active_corporation_ids_since(
    pool: &PgPool,
    since: DateTime<Utc>,
) -> Result<Vec<i64>, DbError> {
    let rows: Vec<(i64,)> = sqlx::query_as(
        r#"
        SELECT DISTINCT corporation_id FROM (
            SELECT corporation_id FROM killmail_attackers
            WHERE corporation_id IS NOT NULL AND kill_time > $1
            UNION
            SELECT corporation_id FROM killmail_victims
            WHERE corporation_id IS NOT NULL AND kill_time > $1
        ) active
        "#,
    )
    .bind(since)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(|r| r.0).collect())
}

pub async fn get_active_alliance_ids_since(
    pool: &PgPool,
    since: DateTime<Utc>,
) -> Result<Vec<i64>, DbError> {
    let rows: Vec<(i64,)> = sqlx::query_as(
        r#"
        SELECT DISTINCT alliance_id FROM (
            SELECT alliance_id FROM killmail_attackers
            WHERE alliance_id IS NOT NULL AND kill_time > $1
            UNION
            SELECT alliance_id FROM killmail_victims
            WHERE alliance_id IS NOT NULL AND kill_time > $1
        ) active
        "#,
    )
    .bind(since)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(|r| r.0).collect())
}

pub async fn get_uncached_corporation_ids(
    pool: &PgPool,
    limit: i32,
) -> Result<Vec<i64>, DbError> {
    let rows: Vec<(i64,)> = sqlx::query_as(
        r#"
        SELECT corporation_id FROM (
            SELECT DISTINCT corporation_id FROM killmail_attackers WHERE corporation_id IS NOT NULL
            UNION
            SELECT DISTINCT corporation_id FROM killmail_victims WHERE corporation_id IS NOT NULL
        ) all_corps
        WHERE corporation_id NOT IN (SELECT corporation_id FROM corporations)
        LIMIT $1
        "#,
    )
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(|r| r.0).collect())
}

pub async fn get_uncached_alliance_ids(
    pool: &PgPool,
    limit: i32,
) -> Result<Vec<i64>, DbError> {
    let rows: Vec<(i64,)> = sqlx::query_as(
        r#"
        SELECT alliance_id FROM (
            SELECT DISTINCT alliance_id FROM killmail_attackers WHERE alliance_id IS NOT NULL
            UNION
            SELECT DISTINCT alliance_id FROM killmail_victims WHERE alliance_id IS NOT NULL
        ) all_alliances
        WHERE alliance_id NOT IN (SELECT alliance_id FROM alliances)
        LIMIT $1
        "#,
    )
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(|r| r.0).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pool::create_pool;

    async fn test_pool() -> PgPool {
        dotenvy::dotenv().ok();
        let url = std::env::var("DATABASE_URL")
            .expect("DATABASE_URL must be set for DB tests");
        create_pool(&url).await.expect("failed to connect to test DB")
    }

    #[tokio::test]
    #[ignore] // requires running TimescaleDB
    async fn test_insert_market_history_batch_and_idempotent() {
        let pool = test_pool().await;

        let rows = vec![
            MarketHistory {
                type_id: 999999,
                region_id: 10000002,
                date: NaiveDate::from_ymd_opt(2020, 1, 1).unwrap(),
                average: 100.0,
                highest: 110.0,
                lowest: 90.0,
                volume: 1000,
                order_count: 50,
            },
            MarketHistory {
                type_id: 999999,
                region_id: 10000002,
                date: NaiveDate::from_ymd_opt(2020, 1, 2).unwrap(),
                average: 105.0,
                highest: 115.0,
                lowest: 95.0,
                volume: 1100,
                order_count: 55,
            },
        ];

        insert_market_history(&pool, &rows).await.unwrap();
        // Re-insert is idempotent (ON CONFLICT DO NOTHING)
        insert_market_history(&pool, &rows).await.unwrap();

        let fetched = get_market_history(&pool, 999999, 10000002, 3650).await.unwrap();
        assert!(fetched.len() >= 2);

        sqlx::query("DELETE FROM market_history WHERE type_id = 999999")
            .execute(&pool).await.unwrap();
    }

    #[tokio::test]
    #[ignore] // requires running TimescaleDB
    async fn test_upsert_daily_destruction() {
        let pool = test_pool().await;
        let date = NaiveDate::from_ymd_opt(2020, 6, 15).unwrap();

        let rows = vec![DailyDestruction {
            type_id: 999998, date, quantity_destroyed: 100, kill_count: 10, type_name: None,
        }];
        upsert_daily_destruction(&pool, &rows).await.unwrap();

        let updated = vec![DailyDestruction {
            type_id: 999998, date, quantity_destroyed: 200, kill_count: 20, type_name: None,
        }];
        upsert_daily_destruction(&pool, &updated).await.unwrap();

        let fetched = get_daily_destruction(&pool, 999998, 3650).await.unwrap();
        let row = fetched.iter().find(|r| r.date == date).expect("row not found");
        assert_eq!(row.quantity_destroyed, 200);
        assert_eq!(row.kill_count, 20);

        sqlx::query("DELETE FROM daily_destruction WHERE type_id = 999998")
            .execute(&pool).await.unwrap();
    }

    #[tokio::test]
    #[ignore] // requires running TimescaleDB
    async fn test_session_lifecycle() {
        let pool = test_pool().await;

        upsert_user(&pool, 999999999, "TestUser", &[], &[], Utc::now() + chrono::Duration::hours(1))
            .await.unwrap();

        let session_id = create_session(&pool, 999999999).await.unwrap();
        let session = get_session(&pool, session_id).await.unwrap();
        assert!(session.is_some());
        assert_eq!(session.unwrap().character_id, 999999999);

        delete_session(&pool, session_id).await.unwrap();
        let session = get_session(&pool, session_id).await.unwrap();
        assert!(session.is_none());

        sqlx::query("DELETE FROM users WHERE character_id = 999999999")
            .execute(&pool).await.unwrap();
    }

    #[tokio::test]
    #[ignore] // requires running TimescaleDB
    async fn test_cleanup_expired_sessions() {
        let pool = test_pool().await;

        upsert_user(&pool, 999999998, "TestUser2", &[], &[], Utc::now() + chrono::Duration::hours(1))
            .await.unwrap();

        let session_id = create_session(&pool, 999999998).await.unwrap();
        sqlx::query("UPDATE sessions SET expires_at = NOW() - INTERVAL '1 hour' WHERE session_id = $1")
            .bind(session_id).execute(&pool).await.unwrap();

        let deleted = cleanup_expired_sessions(&pool).await.unwrap();
        assert!(deleted >= 1);

        let session = get_session(&pool, session_id).await.unwrap();
        assert!(session.is_none());

        sqlx::query("DELETE FROM users WHERE character_id = 999999998")
            .execute(&pool).await.unwrap();
    }
}
