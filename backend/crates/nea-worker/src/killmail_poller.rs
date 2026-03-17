use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use nea_db::{Killmail, KillmailItem, KillmailVictim};
use nea_zkill::R2z2Client;
use sqlx::PgPool;
use tokio::time;

const WORKER_STATE_KEY: &str = "r2z2_last_sequence";

pub async fn run(pool: PgPool, r2z2: Arc<R2z2Client>) {
    tracing::info!("killmail_poller started");

    // Read last sequence ID from worker_state
    let mut sequence_id = match nea_db::get_worker_state(&pool, WORKER_STATE_KEY).await {
        Ok(Some(val)) => match val.parse::<i64>() {
            Ok(id) => {
                tracing::info!(sequence_id = id, "killmail_poller: resuming from saved sequence");
                id
            }
            Err(e) => {
                tracing::warn!("killmail_poller: failed to parse saved sequence '{val}': {e}, starting from 0");
                0
            }
        },
        Ok(None) => {
            tracing::warn!(
                "killmail_poller: no saved sequence found in worker_state, starting from 0. \
                 Set worker_state key '{}' to a recent sequence ID for faster catchup.",
                WORKER_STATE_KEY
            );
            0
        }
        Err(e) => {
            tracing::error!("killmail_poller: failed to read worker_state: {e}, starting from 0");
            0
        }
    };

    loop {
        match r2z2.fetch_sequence(sequence_id).await {
            Ok(Some(response)) => {
                // Parse killmail_time
                let kill_time = parse_killmail_time(&response.killmail_time);

                // Insert killmail
                let km = Killmail {
                    killmail_id: response.killmail_id,
                    kill_time,
                    solar_system_id: Some(response.solar_system_id),
                    total_value: Some(response.total_value),
                    r2z2_sequence_id: Some(sequence_id),
                };

                if let Err(e) = nea_db::insert_killmail(&pool, &km).await {
                    tracing::warn!(
                        killmail_id = response.killmail_id,
                        "killmail_poller: failed to insert killmail: {e}"
                    );
                }

                // Insert items
                let items: Vec<KillmailItem> = response
                    .items
                    .iter()
                    .map(|item| KillmailItem {
                        killmail_id: response.killmail_id,
                        kill_time,
                        type_id: item.type_id,
                        quantity_destroyed: item.quantity_destroyed.unwrap_or(0),
                        quantity_dropped: item.quantity_dropped.unwrap_or(0),
                    })
                    .collect();

                if let Err(e) = nea_db::insert_killmail_items(&pool, &items).await {
                    tracing::warn!(
                        killmail_id = response.killmail_id,
                        "killmail_poller: failed to insert killmail items: {e}"
                    );
                }

                // Insert victim
                let victim = KillmailVictim {
                    killmail_id: response.killmail_id,
                    kill_time,
                    ship_type_id: response.victim.ship_type_id,
                };

                if let Err(e) = nea_db::insert_killmail_victim(&pool, &victim).await {
                    tracing::warn!(
                        killmail_id = response.killmail_id,
                        "killmail_poller: failed to insert killmail victim: {e}"
                    );
                }

                // Save sequence to worker_state
                if let Err(e) = nea_db::set_worker_state(
                    &pool,
                    WORKER_STATE_KEY,
                    &sequence_id.to_string(),
                )
                .await
                {
                    tracing::warn!(
                        sequence_id,
                        "killmail_poller: failed to save sequence to worker_state: {e}"
                    );
                }

                tracing::debug!(
                    sequence_id,
                    killmail_id = response.killmail_id,
                    items = items.len(),
                    "killmail_poller: processed killmail"
                );

                sequence_id += 1;
                time::sleep(Duration::from_millis(100)).await;
            }
            Ok(None) => {
                // 404 - no new data yet
                tracing::debug!(
                    sequence_id,
                    "killmail_poller: no new data, waiting 6 seconds"
                );
                time::sleep(Duration::from_secs(6)).await;
            }
            Err(e) => {
                tracing::warn!(
                    sequence_id,
                    "killmail_poller: error fetching sequence: {e}, retrying in 10 seconds"
                );
                time::sleep(Duration::from_secs(10)).await;
            }
        }
    }
}

// Visible to tests
pub(crate) fn parse_killmail_time(time_str: &str) -> DateTime<Utc> {
    // Try ISO 8601 format first (e.g. "2026-03-17T12:00:00Z")
    if let Ok(dt) = time_str.parse::<DateTime<Utc>>() {
        return dt;
    }
    // Try without timezone suffix
    if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(time_str, "%Y-%m-%dT%H:%M:%S") {
        return dt.and_utc();
    }
    tracing::warn!(
        time_str,
        "killmail_poller: failed to parse killmail_time, using now()"
    );
    Utc::now()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Datelike, Timelike};

    #[test]
    fn test_parse_killmail_time_iso8601_z() {
        let dt = parse_killmail_time("2026-03-17T12:00:00Z");
        assert_eq!(dt.year(), 2026);
        assert_eq!(dt.month(), 3);
        assert_eq!(dt.day(), 17);
        assert_eq!(dt.hour(), 12);
    }

    #[test]
    fn test_parse_killmail_time_no_tz() {
        let dt = parse_killmail_time("2026-03-17T12:00:00");
        assert_eq!(dt.year(), 2026);
        assert_eq!(dt.month(), 3);
        assert_eq!(dt.hour(), 12);
    }

    #[test]
    fn test_parse_killmail_time_invalid_fallback() {
        let before = Utc::now();
        let dt = parse_killmail_time("garbage");
        let after = Utc::now();
        assert!(dt >= before && dt <= after);
    }
}
