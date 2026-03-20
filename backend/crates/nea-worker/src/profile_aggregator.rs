use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use nea_db::CharacterProfile;
use nea_esi::EsiClient;
use sqlx::PgPool;
use tokio::time;

use crate::fitting_utils::{is_fitted_slot, cluster_fittings, get_type_name};

const WORKER_STATE_KEY: &str = "profile_aggregation_last_run";

pub async fn run(pool: PgPool, esi: Arc<EsiClient>) {
    tracing::info!("profile_aggregator task started");
    let mut interval = time::interval(Duration::from_secs(3600));

    loop {
        interval.tick().await;
        tracing::info!("profile_aggregator: starting cycle");

        if let Err(e) = run_cycle(&pool, &esi).await {
            tracing::error!("profile_aggregator: cycle failed: {e}");
        }
    }
}

async fn run_cycle(
    pool: &PgPool,
    esi: &EsiClient,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Resolve uncached character names
    resolve_character_names(pool, esi).await;

    // Determine which characters need profile recomputation
    let last_run = nea_db::get_worker_state(pool, WORKER_STATE_KEY).await?;
    let since = match last_run {
        Some(val) => val.parse::<chrono::DateTime<Utc>>().unwrap_or(Utc::now() - chrono::Duration::days(7)),
        None => Utc::now() - chrono::Duration::days(7),
    };

    let character_ids = nea_db::get_active_character_ids_since(pool, since).await?;
    tracing::info!(characters = character_ids.len(), "profile_aggregator: recomputing profiles");

    let mut computed = 0u64;
    for character_id in &character_ids {
        match compute_profile(pool, *character_id).await {
            Ok(profile) => {
                if let Err(e) = nea_db::upsert_character_profile(pool, &profile).await {
                    tracing::warn!(character_id, "profile_aggregator: failed to upsert profile: {e}");
                } else {
                    computed += 1;
                }
            }
            Err(e) => {
                tracing::warn!(character_id, "profile_aggregator: failed to compute profile: {e}");
            }
        }
    }

    // Save last run time
    nea_db::set_worker_state(pool, WORKER_STATE_KEY, &Utc::now().to_rfc3339()).await?;

    tracing::info!(computed, total = character_ids.len(), "profile_aggregator: cycle complete");
    Ok(())
}

async fn resolve_character_names(pool: &PgPool, esi: &EsiClient) {
    // Collect IDs that need resolution: never-seen + stale "Unknown" placeholders.
    let mut ids_to_resolve = match nea_db::get_uncached_character_ids(pool, 500).await {
        Ok(ids) => ids,
        Err(e) => {
            tracing::warn!("profile_aggregator: failed to get uncached character IDs: {e}");
            return;
        }
    };

    match nea_db::get_stale_unknown_character_ids(pool, 500).await {
        Ok(stale) => {
            if !stale.is_empty() {
                tracing::info!(count = stale.len(), "profile_aggregator: retrying stale Unknown characters");
                ids_to_resolve.extend(stale);
            }
        }
        Err(e) => {
            tracing::warn!("profile_aggregator: failed to get stale Unknown character IDs: {e}");
        }
    }

    if ids_to_resolve.is_empty() {
        return;
    }

    tracing::info!(count = ids_to_resolve.len(), "profile_aggregator: resolving character names");

    // Phase 1: Bulk resolve via POST /universe/names/ (up to 1000 per call).
    let mut remaining: std::collections::HashSet<i64> = ids_to_resolve.iter().copied().collect();
    let mut resolved = 0u64;

    for chunk in ids_to_resolve.chunks(1000) {
        match esi.resolve_names(chunk).await {
            Ok(names) => {
                for name in &names {
                    if name.category == "character" {
                        remaining.remove(&name.id);
                        // We only get name from /universe/names/, not corp/alliance.
                        // Do a targeted individual lookup for the full info.
                    }
                }
                // For characters found in bulk, fetch full details individually.
                for name in names {
                    if name.category != "character" {
                        continue;
                    }
                    match esi.get_character(name.id).await {
                        Ok(info) => {
                            let character = nea_db::Character {
                                character_id: name.id,
                                name: info.name,
                                corporation_id: info.corporation_id,
                                alliance_id: info.alliance_id,
                                fetched_at: Utc::now(),
                            };
                            if let Err(e) = nea_db::upsert_character(pool, &character).await {
                                tracing::warn!(character_id = name.id, "profile_aggregator: failed to cache character: {e}");
                            } else {
                                resolved += 1;
                            }
                        }
                        Err(e) => {
                            // Bulk said this ID exists but individual lookup failed —
                            // transient error, skip and retry next cycle.
                            tracing::debug!(character_id = name.id, "profile_aggregator: bulk-confirmed but individual lookup failed: {e}");
                        }
                    }
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
            }
            Err(e) => {
                tracing::warn!("profile_aggregator: bulk resolve_names failed: {e}");
                // Fall through — remaining set still has all IDs from this chunk.
            }
        }
    }

    // Phase 2: IDs not found in bulk are genuinely deleted/biomassed — cache placeholder.
    // But only for IDs that haven't already been resolved above.
    let not_found_count = remaining.len();
    if not_found_count > 0 {
        tracing::info!(count = not_found_count, "profile_aggregator: caching placeholders for IDs not found via bulk resolve");
        for character_id in remaining {
            let placeholder = nea_db::Character {
                character_id,
                name: format!("Unknown {}", character_id),
                corporation_id: None,
                alliance_id: None,
                fetched_at: Utc::now(),
            };
            let _ = nea_db::upsert_character(pool, &placeholder).await;
        }
    }

    tracing::info!(resolved, not_found = not_found_count, "profile_aggregator: character name resolution complete");
}

async fn compute_profile(
    pool: &PgPool,
    character_id: i64,
) -> Result<CharacterProfile, Box<dyn std::error::Error + Send + Sync>> {
    // Total kills (as attacker)
    let (total_kills,): (i64,) = sqlx::query_as(
        "SELECT COUNT(DISTINCT killmail_id) FROM killmail_attackers WHERE character_id = $1",
    )
    .bind(character_id)
    .fetch_one(pool)
    .await?;

    // Total losses (as victim)
    let (total_losses,): (i64,) = sqlx::query_as(
        "SELECT COUNT(DISTINCT killmail_id) FROM killmail_victims WHERE character_id = $1",
    )
    .bind(character_id)
    .fetch_one(pool)
    .await?;

    // Solo kills: killmails where this pilot attacked and only 1 non-NPC attacker
    let (solo_kills,): (i64,) = sqlx::query_as(
        r#"
        SELECT COUNT(*) FROM (
            SELECT a.killmail_id
            FROM killmail_attackers a
            WHERE a.character_id = $1
              AND (SELECT COUNT(*) FROM killmail_attackers a2
                   WHERE a2.killmail_id = a.killmail_id AND a2.kill_time = a.kill_time
                     AND a2.character_id IS NOT NULL) = 1
        ) solo
        "#,
    )
    .bind(character_id)
    .fetch_one(pool)
    .await?;

    // Solo losses: killmails where this pilot died and only 1 non-NPC attacker
    let (solo_losses,): (i64,) = sqlx::query_as(
        r#"
        SELECT COUNT(*) FROM (
            SELECT v.killmail_id
            FROM killmail_victims v
            WHERE v.character_id = $1
              AND (SELECT COUNT(*) FROM killmail_attackers a2
                   WHERE a2.killmail_id = v.killmail_id AND a2.kill_time = v.kill_time
                     AND a2.character_id IS NOT NULL) = 1
        ) solo
        "#,
    )
    .bind(character_id)
    .fetch_one(pool)
    .await?;

    // Top ships flown (as attacker)
    let top_ships_flown: Vec<(i32, i64)> = sqlx::query_as(
        r#"
        SELECT ship_type_id, COUNT(*) as cnt
        FROM killmail_attackers
        WHERE character_id = $1 AND ship_type_id > 0
        GROUP BY ship_type_id
        ORDER BY cnt DESC
        LIMIT 10
        "#,
    )
    .bind(character_id)
    .fetch_all(pool)
    .await?;

    let ships_flown_json: Vec<serde_json::Value> = {
        let mut result = Vec::new();
        for (type_id, count) in &top_ships_flown {
            let name = get_type_name(pool, *type_id).await;
            result.push(serde_json::json!({
                "type_id": type_id,
                "name": name,
                "count": count,
            }));
        }
        result
    };

    // Top ships lost (as victim)
    let top_ships_lost: Vec<(i32, i64)> = sqlx::query_as(
        r#"
        SELECT ship_type_id, COUNT(*) as cnt
        FROM killmail_victims
        WHERE character_id = $1 AND ship_type_id > 0
        GROUP BY ship_type_id
        ORDER BY cnt DESC
        LIMIT 10
        "#,
    )
    .bind(character_id)
    .fetch_all(pool)
    .await?;

    let ships_lost_json: Vec<serde_json::Value> = {
        let mut result = Vec::new();
        for (type_id, count) in &top_ships_lost {
            let name = get_type_name(pool, *type_id).await;
            result.push(serde_json::json!({
                "type_id": type_id,
                "name": name,
                "count": count,
            }));
        }
        result
    };

    // Common fittings (from losses only, last 180 days)
    let common_fits = compute_common_fits(pool, character_id).await?;

    // Active period
    let active_period: Option<(chrono::DateTime<Utc>, chrono::DateTime<Utc>)> = sqlx::query_as(
        r#"
        SELECT MIN(kill_time), MAX(kill_time) FROM (
            SELECT kill_time FROM killmail_attackers WHERE character_id = $1
            UNION ALL
            SELECT kill_time FROM killmail_victims WHERE character_id = $1
        ) times
        "#,
    )
    .bind(character_id)
    .fetch_optional(pool)
    .await?;

    let active_period_json = active_period.map(|(first, last)| {
        serde_json::json!({
            "first_seen": first.to_rfc3339(),
            "last_seen": last.to_rfc3339(),
        })
    });

    Ok(CharacterProfile {
        character_id,
        total_kills: total_kills as i32,
        total_losses: total_losses as i32,
        solo_kills: solo_kills as i32,
        solo_losses: solo_losses as i32,
        top_ships_flown: Some(serde_json::Value::Array(ships_flown_json)),
        top_ships_lost: Some(serde_json::Value::Array(ships_lost_json)),
        common_fits: Some(serde_json::Value::Array(common_fits)),
        active_period: active_period_json,
        computed_at: Utc::now(),
    })
}

/// Compute common fittings from losses.
/// Groups similar fittings (Jaccard similarity >= 0.8) into clusters.
async fn compute_common_fits(
    pool: &PgPool,
    character_id: i64,
) -> Result<Vec<serde_json::Value>, Box<dyn std::error::Error + Send + Sync>> {
    // Get the top 5 most-lost ship types in last 180 days
    let top_lost_ships: Vec<(i32, i64)> = sqlx::query_as(
        r#"
        SELECT ship_type_id, COUNT(*) as cnt
        FROM killmail_victims
        WHERE character_id = $1 AND ship_type_id > 0
          AND kill_time >= NOW() - INTERVAL '180 days'
        GROUP BY ship_type_id
        ORDER BY cnt DESC
        LIMIT 5
        "#,
    )
    .bind(character_id)
    .fetch_all(pool)
    .await?;

    let mut all_fits = Vec::new();

    for (ship_type_id, _) in &top_lost_ships {
        // Get all losses for this ship type
        let loss_killmail_ids: Vec<(i64, chrono::DateTime<Utc>)> = sqlx::query_as(
            r#"
            SELECT killmail_id, kill_time
            FROM killmail_victims
            WHERE character_id = $1 AND ship_type_id = $2
              AND kill_time >= NOW() - INTERVAL '180 days'
            ORDER BY kill_time DESC
            LIMIT 50
            "#,
        )
        .bind(character_id)
        .bind(ship_type_id)
        .fetch_all(pool)
        .await?;

        // For each loss, reconstruct the fitting from killmail_items
        let mut fittings: Vec<Vec<(i32, i32)>> = Vec::new(); // Vec of (type_id, flag) sets

        for (km_id, km_time) in &loss_killmail_ids {
            let items: Vec<(i32, i32)> = sqlx::query_as(
                r#"
                SELECT type_id, flag
                FROM killmail_items
                WHERE killmail_id = $1 AND kill_time = $2
                  AND flag != 0
                "#,
            )
            .bind(km_id)
            .bind(km_time)
            .fetch_all(pool)
            .await?;

            let fitted: Vec<(i32, i32)> = items
                .into_iter()
                .filter(|(_, flag)| is_fitted_slot(*flag))
                .collect();

            if !fitted.is_empty() {
                fittings.push(fitted);
            }
        }

        if fittings.is_empty() {
            continue;
        }

        // Cluster fittings by Jaccard similarity >= 0.8
        let clusters = cluster_fittings(&fittings, 0.8);

        let ship_name = get_type_name(pool, *ship_type_id).await;

        for cluster in clusters {
            // Use the most common exact fit as the canonical one
            let canonical = &fittings[cluster.canonical_idx];
            let mut modules = Vec::new();
            for (type_id, flag) in canonical {
                let name = get_type_name(pool, *type_id).await;
                modules.push(serde_json::json!({
                    "type_id": type_id,
                    "name": name,
                    "flag": flag,
                }));
            }

            all_fits.push(serde_json::json!({
                "ship_type_id": ship_type_id,
                "ship_name": ship_name,
                "modules": modules,
                "count": cluster.count,
                "variant_count": cluster.variant_count,
            }));
        }
    }

    Ok(all_fits)
}

