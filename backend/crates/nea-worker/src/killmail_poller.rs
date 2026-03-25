use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use nea_db::{Killmail, KillmailAttacker, KillmailItem, KillmailVictim};
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
                tracing::info!(
                    sequence_id = id,
                    "killmail_poller: resuming from saved sequence"
                );
                id
            }
            Err(e) => {
                tracing::warn!(
                    "killmail_poller: failed to parse saved sequence '{val}': {e}, fetching current from R2Z2"
                );
                fetch_starting_sequence(&r2z2).await
            }
        },
        Ok(None) => {
            tracing::info!("killmail_poller: no saved sequence, fetching current from R2Z2");
            fetch_starting_sequence(&r2z2).await
        }
        Err(e) => {
            tracing::error!(
                "killmail_poller: failed to read worker_state: {e}, fetching current from R2Z2"
            );
            fetch_starting_sequence(&r2z2).await
        }
    };

    loop {
        match r2z2.fetch_sequence(sequence_id).await {
            Ok(Some(response)) => {
                let esi = &response.esi;

                // Parse killmail_time — skip if missing
                let kill_time = match &esi.killmail_time {
                    Some(t) => nea_zkill::parse_killmail_time(t),
                    None => {
                        tracing::warn!(
                            sequence_id,
                            killmail_id = response.killmail_id,
                            "killmail_poller: skipping killmail with missing killmail_time"
                        );
                        sequence_id += 1;
                        continue;
                    }
                };

                // Insert killmail
                let km = Killmail {
                    killmail_id: response.killmail_id,
                    kill_time,
                    solar_system_id: Some(esi.solar_system_id),
                    total_value: Some(response.zkb.total_value),
                    r2z2_sequence_id: Some(sequence_id),
                };

                if let Err(e) = nea_db::insert_killmail(&pool, &km).await {
                    tracing::warn!(
                        killmail_id = response.killmail_id,
                        "killmail_poller: failed to insert killmail: {e}"
                    );
                }

                // Insert items
                // Deduplicate items by (type_id, flag) — ESI can report
                // multiple stacks with the same key (e.g. ammo in cargo).
                let mut item_map: HashMap<(i32, i32), KillmailItem> = HashMap::new();
                for item in &esi.victim.items {
                    let key = (item.item_type_id, item.flag);
                    item_map
                        .entry(key)
                        .and_modify(|e| {
                            e.quantity_destroyed += item.quantity_destroyed.unwrap_or(0);
                            e.quantity_dropped += item.quantity_dropped.unwrap_or(0);
                        })
                        .or_insert(KillmailItem {
                            killmail_id: response.killmail_id,
                            kill_time,
                            type_id: item.item_type_id,
                            quantity_destroyed: item.quantity_destroyed.unwrap_or(0),
                            quantity_dropped: item.quantity_dropped.unwrap_or(0),
                            flag: item.flag,
                        });
                }
                let items: Vec<KillmailItem> = item_map.into_values().collect();

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
                    ship_type_id: esi.victim.ship_type_id,
                    character_id: esi.victim.character_id,
                    corporation_id: esi.victim.corporation_id,
                    alliance_id: esi.victim.alliance_id,
                };

                if let Err(e) = nea_db::insert_killmail_victim(&pool, &victim).await {
                    tracing::warn!(
                        killmail_id = response.killmail_id,
                        "killmail_poller: failed to insert killmail victim: {e}"
                    );
                }

                // Insert attackers
                let attackers: Vec<KillmailAttacker> = esi
                    .attackers
                    .iter()
                    .map(|a| KillmailAttacker {
                        killmail_id: response.killmail_id,
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
                    if let Err(e) = nea_db::insert_killmail_attackers(&pool, &attackers).await {
                        tracing::warn!(
                            killmail_id = response.killmail_id,
                            "killmail_poller: failed to insert killmail attackers: {e}"
                        );
                    }
                }

                // Save sequence to worker_state
                if let Err(e) =
                    nea_db::set_worker_state(&pool, WORKER_STATE_KEY, &sequence_id.to_string())
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
                    attackers = attackers.len(),
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
            Err(nea_zkill::ZkillError::Deserialize(ref msg)) => {
                tracing::warn!(
                    sequence_id,
                    error = %msg,
                    "killmail_poller: skipping unparseable killmail"
                );
                sequence_id += 1;
                time::sleep(Duration::from_millis(100)).await;
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

async fn fetch_starting_sequence(r2z2: &R2z2Client) -> i64 {
    match r2z2.fetch_current_sequence().await {
        Ok(seq) => {
            tracing::info!(sequence_id = seq, "fetched current R2Z2 sequence");
            seq
        }
        Err(e) => {
            tracing::error!("failed to fetch current R2Z2 sequence: {e}, defaulting to 0");
            0
        }
    }
}
