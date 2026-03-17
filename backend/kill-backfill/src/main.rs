use std::sync::Arc;

use chrono::{DateTime, Duration, NaiveDate, Utc};
use nea_db::models::*;
use nea_esi::EsiClient;
use nea_zkill::R2z2Client;
use sqlx::PgPool;
use tracing::{info, warn};

/// Max concurrent ESI requests.
const ESI_CONCURRENCY: usize = 5;

/// Batch size — process this many killmails, then pause briefly.
const BATCH_SIZE: usize = 50;

/// Delay between batches in milliseconds.
const BATCH_DELAY_MS: u64 = 1000;

/// Max retries for a single ESI request.
const MAX_RETRIES: u32 = 3;

/// Parse a killmail timestamp string into a `DateTime<Utc>`.
///
/// Duplicated from nea-worker to avoid a dependency on that crate.
fn parse_killmail_time(time_str: &str) -> DateTime<Utc> {
    if let Ok(dt) = time_str.parse::<DateTime<Utc>>() {
        return dt;
    }
    if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(time_str, "%Y-%m-%dT%H:%M:%S") {
        return dt.and_utc();
    }
    warn!(time_str, "failed to parse killmail_time, using now()");
    Utc::now()
}

/// Parse `--days N` from CLI args, defaulting to 90.
fn parse_days_arg() -> i64 {
    let args: Vec<String> = std::env::args().collect();
    for i in 0..args.len() {
        if args[i] == "--days" {
            if let Some(val) = args.get(i + 1) {
                if let Ok(n) = val.parse::<i64>() {
                    return n;
                }
            }
        }
    }
    90
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

    let days = parse_days_arg();
    info!(days, "kill-backfill starting");

    let database_url = std::env::var("DATABASE_URL")
        .expect("DATABASE_URL must be set");
    let pool = nea_db::create_pool(&database_url).await?;
    let esi = Arc::new(EsiClient::new());
    let r2z2 = R2z2Client::new();

    // Determine start date: resume from last completed date or go back `days` days.
    let default_start = (Utc::now() - Duration::days(days)).date_naive();
    let start_date = match nea_db::get_worker_state(&pool, "backfill_last_completed_date").await? {
        Some(val) => {
            if let Ok(d) = NaiveDate::parse_from_str(&val, "%Y-%m-%d") {
                // Resume from the day after the last completed date.
                d + Duration::days(1)
            } else {
                default_start
            }
        }
        None => default_start,
    };

    let yesterday = (Utc::now() - Duration::days(1)).date_naive();

    if start_date > yesterday {
        info!("backfill already up to date (last completed through yesterday)");
        return Ok(());
    }

    info!(
        start = %start_date,
        end = %yesterday,
        "backfilling killmails"
    );

    let semaphore = Arc::new(tokio::sync::Semaphore::new(ESI_CONCURRENCY));
    let mut total_inserted: u64 = 0;

    let mut current_date = start_date;
    while current_date <= yesterday {
        let date_str = current_date.format("%Y%m%d").to_string();

        // Fetch history for this date from zKillboard.
        let pairs = match r2z2.fetch_history(&date_str).await {
            Ok(p) => p,
            Err(e) => {
                warn!(date = %current_date, error = %e, "failed to fetch history, skipping date");
                current_date += Duration::days(1);
                continue;
            }
        };

        info!(date = %current_date, killmails = pairs.len(), "fetched history");

        let mut date_inserted: u64 = 0;
        let mut date_errors: u64 = 0;

        // Process in batches to avoid overwhelming ESI.
        for (batch_idx, batch) in pairs.chunks(BATCH_SIZE).enumerate() {
            // Check ESI error budget before each batch — pause if low.
            let budget = esi.error_budget();
            if budget < 10 {
                warn!(budget, "ESI error budget critically low, pausing 60s");
                tokio::time::sleep(std::time::Duration::from_secs(60)).await;
            } else if budget < 30 {
                warn!(budget, "ESI error budget low, pausing 10s");
                tokio::time::sleep(std::time::Duration::from_secs(10)).await;
            }

            let mut handles = Vec::with_capacity(batch.len());
            for (km_id, km_hash) in batch {
                let permit = semaphore.clone().acquire_owned().await?;
                let esi = Arc::clone(&esi);
                let pool = pool.clone();
                let km_id = *km_id;
                let km_hash = km_hash.clone();

                handles.push(tokio::spawn(async move {
                    let _permit = permit;
                    process_killmail_with_retry(&esi, &pool, km_id, &km_hash).await
                }));
            }

            for handle in handles {
                match handle.await? {
                    Ok(true) => date_inserted += 1,
                    Ok(false) => {} // already existed or skipped
                    Err(e) => {
                        date_errors += 1;
                        if date_errors <= 5 {
                            warn!(error = %e, "failed to process killmail");
                        }
                    }
                }
            }

            // Brief pause between batches.
            if batch_idx < pairs.len() / BATCH_SIZE {
                tokio::time::sleep(std::time::Duration::from_millis(BATCH_DELAY_MS)).await;
            }
        }

        total_inserted += date_inserted;
        info!(
            date = %current_date,
            inserted = date_inserted,
            errors = date_errors,
            total = total_inserted,
            "date complete"
        );

        // Save progress.
        nea_db::set_worker_state(
            &pool,
            "backfill_last_completed_date",
            &current_date.format("%Y-%m-%d").to_string(),
        )
        .await?;

        current_date += Duration::days(1);

        // Be kind to zKillboard: 1-second delay between dates.
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }

    info!(total = total_inserted, "backfill complete, running aggregation");

    // Run aggregation over the full backfilled range.
    run_aggregation(&pool, start_date, yesterday).await?;

    info!("aggregation complete");
    Ok(())
}

/// Fetch a single killmail from ESI with retry on transient errors (429, 502, 503, 504).
async fn process_killmail_with_retry(
    esi: &EsiClient,
    pool: &PgPool,
    killmail_id: i64,
    killmail_hash: &str,
) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
    let mut last_err = None;

    for attempt in 0..MAX_RETRIES {
        match process_killmail(esi, pool, killmail_id, killmail_hash).await {
            Ok(val) => return Ok(val),
            Err(e) => {
                let err_str = e.to_string();
                let is_retryable = err_str.contains("status 429")
                    || err_str.contains("status 502")
                    || err_str.contains("status 503")
                    || err_str.contains("status 504")
                    || err_str.contains("Rate limited");

                if !is_retryable || attempt == MAX_RETRIES - 1 {
                    return Err(e);
                }

                // Exponential backoff: 2s, 4s, 8s...
                let delay = std::time::Duration::from_secs(2u64.pow(attempt + 1));
                warn!(
                    killmail_id,
                    attempt = attempt + 1,
                    delay_secs = delay.as_secs(),
                    error = %err_str,
                    "retryable ESI error, backing off"
                );
                tokio::time::sleep(delay).await;
                last_err = Some(e);
            }
        }
    }

    Err(last_err.unwrap())
}

/// Fetch a single killmail from ESI and insert it into the database.
///
/// Returns `Ok(true)` if a new row was inserted, `Ok(false)` if it already existed.
async fn process_killmail(
    esi: &EsiClient,
    pool: &PgPool,
    killmail_id: i64,
    killmail_hash: &str,
) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
    let km = esi.get_killmail_typed(killmail_id, killmail_hash).await?;
    let kill_time = parse_killmail_time(&km.killmail_time);

    let killmail = Killmail {
        killmail_id: km.killmail_id,
        kill_time,
        solar_system_id: Some(km.solar_system_id),
        total_value: None, // ESI killmail endpoint doesn't include zkb value
        r2z2_sequence_id: None, // not obtained via R2Z2 polling
    };

    nea_db::insert_killmail(pool, &killmail).await?;

    let items: Vec<KillmailItem> = km
        .victim
        .items
        .iter()
        .map(|i| KillmailItem {
            killmail_id: km.killmail_id,
            kill_time,
            type_id: i.item_type_id,
            quantity_destroyed: i.quantity_destroyed.unwrap_or(0),
            quantity_dropped: i.quantity_dropped.unwrap_or(0),
        })
        .collect();

    if !items.is_empty() {
        nea_db::insert_killmail_items(pool, &items).await?;
    }

    let victim = KillmailVictim {
        killmail_id: km.killmail_id,
        kill_time,
        ship_type_id: km.victim.ship_type_id,
    };

    nea_db::insert_killmail_victim(pool, &victim).await?;

    Ok(true)
}

/// Aggregate killmails into daily_destruction for the given date range.
async fn run_aggregation(
    pool: &PgPool,
    start: NaiveDate,
    end: NaiveDate,
) -> Result<(), Box<dyn std::error::Error>> {
    info!(start = %start, end = %end, "aggregating daily destruction");

    // Query raw destruction data and aggregate per (type_id, date).
    let rows: Vec<(i32, NaiveDate, i64, i32)> = sqlx::query_as(
        r#"
        SELECT ki.type_id,
               DATE(ki.kill_time) AS date,
               SUM(ki.quantity_destroyed)::bigint AS quantity_destroyed,
               COUNT(DISTINCT ki.killmail_id)::int AS kill_count
        FROM killmail_items ki
        WHERE DATE(ki.kill_time) >= $1
          AND DATE(ki.kill_time) <= $2
          AND ki.quantity_destroyed > 0
        GROUP BY ki.type_id, DATE(ki.kill_time)
        "#,
    )
    .bind(start)
    .bind(end)
    .fetch_all(pool)
    .await?;

    let daily: Vec<DailyDestruction> = rows
        .into_iter()
        .map(|(type_id, date, qty, count)| DailyDestruction {
            type_id,
            date,
            quantity_destroyed: qty,
            kill_count: count,
        })
        .collect();

    info!(rows = daily.len(), "upserting daily destruction");
    nea_db::upsert_daily_destruction(pool, &daily).await?;

    // Also aggregate victim ships (the ship itself was destroyed).
    let ship_rows: Vec<(i32, NaiveDate, i64, i32)> = sqlx::query_as(
        r#"
        SELECT kv.ship_type_id AS type_id,
               DATE(kv.kill_time) AS date,
               COUNT(*)::bigint AS quantity_destroyed,
               COUNT(*)::int AS kill_count
        FROM killmail_victims kv
        WHERE DATE(kv.kill_time) >= $1
          AND DATE(kv.kill_time) <= $2
          AND kv.ship_type_id > 0
        GROUP BY kv.ship_type_id, DATE(kv.kill_time)
        "#,
    )
    .bind(start)
    .bind(end)
    .fetch_all(pool)
    .await?;

    let ship_daily: Vec<DailyDestruction> = ship_rows
        .into_iter()
        .map(|(type_id, date, qty, count)| DailyDestruction {
            type_id,
            date,
            quantity_destroyed: qty,
            kill_count: count,
        })
        .collect();

    info!(rows = ship_daily.len(), "upserting ship destruction");
    nea_db::upsert_daily_destruction(pool, &ship_daily).await?;

    Ok(())
}
