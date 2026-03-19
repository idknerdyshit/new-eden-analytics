use std::sync::Arc;

use chrono::{Duration, NaiveDate, Utc};
use nea_db::models::{DailyDestruction, Killmail, KillmailAttacker, KillmailItem, KillmailVictim};
use nea_esi::EsiClient;
use nea_zkill::R2z2Client;
use sqlx::PgPool;
use tracing::{info, warn};

/// Check error budget every N killmails.
const BATCH_SIZE: usize = 20;

/// Delay between individual ESI requests in milliseconds (~2 req/s).
const PER_REQUEST_DELAY_MS: u64 = 500;

/// Max retries for a single ESI request.
const MAX_RETRIES: u32 = 5;

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
    let esi = Arc::new(EsiClient::with_user_agent(
        "new-eden-analytics (sara@idknerdyshit.com; +https://github.com/idknerdyshit/new-eden-analytics; eve:Eyedeekay)",
    ));
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

        // Process killmails sequentially with a delay between each request.
        // ESI rate limits are strict; concurrent bursts trigger 429s quickly.
        for (i, (km_id, km_hash)) in pairs.iter().enumerate() {
            // Check ESI error budget periodically — pause longer if low.
            if i % BATCH_SIZE == 0 && i > 0 {
                let budget = esi.error_budget();
                if budget < 10 {
                    warn!(budget, "ESI error budget critically low, pausing 60s");
                    tokio::time::sleep(std::time::Duration::from_secs(60)).await;
                } else if budget < 30 {
                    warn!(budget, "ESI error budget low, pausing 10s");
                    tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                }

                info!(
                    date = %current_date,
                    progress = format!("{}/{}", i, pairs.len()),
                    inserted = date_inserted,
                    errors = date_errors,
                    "batch progress"
                );
            }

            match process_killmail_with_retry(&esi, &pool, *km_id, km_hash).await {
                Ok(true) => date_inserted += 1,
                Ok(false) => {}
                Err(e) => {
                    date_errors += 1;
                    if date_errors <= 5 {
                        warn!(error = %e, "failed to process killmail");
                    }
                }
            }

            // Pace requests: 70ms between each call (~14 req/s, under 15 req/s limit).
            tokio::time::sleep(std::time::Duration::from_millis(PER_REQUEST_DELAY_MS)).await;
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
    let kill_time = nea_zkill::parse_killmail_time(&km.killmail_time);

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
            flag: i.flag,
        })
        .collect();

    if !items.is_empty() {
        // Delete old items first so flag=0 rows are replaced with proper flags
        nea_db::delete_killmail_items(pool, km.killmail_id, kill_time).await?;
        nea_db::insert_killmail_items(pool, &items).await?;
    }

    let victim = KillmailVictim {
        killmail_id: km.killmail_id,
        kill_time,
        ship_type_id: km.victim.ship_type_id,
        character_id: km.victim.character_id,
        corporation_id: km.victim.corporation_id,
        alliance_id: km.victim.alliance_id,
    };

    nea_db::insert_killmail_victim(pool, &victim).await?;

    // Insert attackers
    let attackers: Vec<KillmailAttacker> = km
        .attackers
        .iter()
        .map(|a| KillmailAttacker {
            killmail_id: km.killmail_id,
            kill_time,
            character_id: a.character_id,
            corporation_id: a.corporation_id,
            alliance_id: a.alliance_id,
            ship_type_id: a.ship_type_id,
            weapon_type_id: a.weapon_type_id,
            damage_done: a.damage_done,
            final_blow: a.final_blow,
        })
        .collect();

    if !attackers.is_empty() {
        nea_db::insert_killmail_attackers(pool, &attackers).await?;
    }

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
            type_name: None,
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
            type_name: None,
            date,
            quantity_destroyed: qty,
            kill_count: count,
        })
        .collect();

    info!(rows = ship_daily.len(), "upserting ship destruction");
    nea_db::upsert_daily_destruction(pool, &ship_daily).await?;

    Ok(())
}
