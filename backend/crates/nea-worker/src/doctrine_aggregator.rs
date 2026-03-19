use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use nea_db::DoctrineProfile;
use nea_esi::EsiClient;
use sqlx::PgPool;
use tokio::time;

use crate::fitting_utils::{cluster_fittings, get_type_name, is_fitted_slot};

const WORKER_STATE_KEY: &str = "doctrine_aggregation_last_run";
const WINDOWS: &[i32] = &[7, 30, 90];
const MIN_KILLS_FOR_PROFILE: i64 = 25;

pub async fn run(pool: PgPool, esi: Arc<EsiClient>) {
    tracing::info!("doctrine_aggregator task started");
    let mut interval = time::interval(Duration::from_secs(3600));

    loop {
        interval.tick().await;
        tracing::info!("doctrine_aggregator: starting cycle");

        if let Err(e) = run_cycle(&pool, &esi).await {
            tracing::error!("doctrine_aggregator: cycle failed: {e}");
        }
    }
}

async fn run_cycle(
    pool: &PgPool,
    esi: &EsiClient,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Step 1: Resolve uncached corp/alliance names
    resolve_corporation_names(pool, esi).await;
    resolve_alliance_names(pool, esi).await;

    // Step 2: Find active entities
    let last_run = nea_db::get_worker_state(pool, WORKER_STATE_KEY).await?;
    let since = match last_run {
        Some(val) => val
            .parse::<chrono::DateTime<Utc>>()
            .unwrap_or(Utc::now() - chrono::Duration::days(7)),
        None => Utc::now() - chrono::Duration::days(7),
    };

    let corp_ids = nea_db::get_active_corporation_ids_since(pool, since).await?;
    let alliance_ids = nea_db::get_active_alliance_ids_since(pool, since).await?;

    tracing::info!(
        corporations = corp_ids.len(),
        alliances = alliance_ids.len(),
        "doctrine_aggregator: found active entities"
    );

    // Step 3: Compute profiles for corporations
    let mut computed = 0u64;
    for &corp_id in &corp_ids {
        // Check 30d kill count to filter low-activity corps
        let kill_count = get_entity_kill_count(pool, "corporation", corp_id, 30).await?;
        if kill_count < MIN_KILLS_FOR_PROFILE {
            continue;
        }

        let corp = nea_db::get_corporation(pool, corp_id).await?;
        let entity_name = corp.as_ref().map(|c| c.name.clone()).unwrap_or_else(|| format!("Corp {}", corp_id));
        let member_count = corp.and_then(|c| c.member_count).unwrap_or(0);

        for &window in WINDOWS {
            match compute_doctrine_profile(pool, "corporation", corp_id, &entity_name, member_count, window).await {
                Ok(profile) => {
                    if let Err(e) = nea_db::upsert_doctrine_profile(pool, &profile).await {
                        tracing::warn!(corp_id, window, "doctrine_aggregator: failed to upsert profile: {e}");
                    } else {
                        computed += 1;
                    }
                }
                Err(e) => {
                    tracing::warn!(corp_id, window, "doctrine_aggregator: failed to compute profile: {e}");
                }
            }
        }
    }

    // Step 3b: Compute profiles for alliances
    for &alliance_id in &alliance_ids {
        let kill_count = get_entity_kill_count(pool, "alliance", alliance_id, 30).await?;
        if kill_count < MIN_KILLS_FOR_PROFILE {
            continue;
        }

        let entity_name = nea_db::get_alliance(pool, alliance_id)
            .await?
            .map(|a| a.name)
            .unwrap_or_else(|| format!("Alliance {}", alliance_id));

        for &window in WINDOWS {
            match compute_doctrine_profile(pool, "alliance", alliance_id, &entity_name, 0, window).await {
                Ok(profile) => {
                    if let Err(e) = nea_db::upsert_doctrine_profile(pool, &profile).await {
                        tracing::warn!(alliance_id, window, "doctrine_aggregator: failed to upsert profile: {e}");
                    } else {
                        computed += 1;
                    }
                }
                Err(e) => {
                    tracing::warn!(alliance_id, window, "doctrine_aggregator: failed to compute profile: {e}");
                }
            }
        }
    }

    // Step 4: Save last run time
    nea_db::set_worker_state(pool, WORKER_STATE_KEY, &Utc::now().to_rfc3339()).await?;

    tracing::info!(computed, "doctrine_aggregator: cycle complete");
    Ok(())
}

async fn resolve_corporation_names(pool: &PgPool, esi: &EsiClient) {
    let uncached = match nea_db::get_uncached_corporation_ids(pool, 200).await {
        Ok(ids) => ids,
        Err(e) => {
            tracing::warn!("doctrine_aggregator: failed to get uncached corp IDs: {e}");
            return;
        }
    };

    if uncached.is_empty() {
        return;
    }

    tracing::info!(count = uncached.len(), "doctrine_aggregator: resolving corporation names");

    let mut resolved = 0u64;
    for corp_id in uncached {
        match esi.get_corporation(corp_id).await {
            Ok(info) => {
                let corp = nea_db::Corporation {
                    corporation_id: corp_id,
                    name: info.name,
                    alliance_id: info.alliance_id,
                    member_count: info.member_count,
                    fetched_at: Utc::now(),
                };
                if let Err(e) = nea_db::upsert_corporation(pool, &corp).await {
                    tracing::warn!(corp_id, "doctrine_aggregator: failed to cache corporation: {e}");
                } else {
                    resolved += 1;
                }
            }
            Err(nea_esi::EsiError::Api { status: 404, .. }) => {
                tracing::debug!(corp_id, "doctrine_aggregator: corporation not found on ESI (404), caching placeholder");
                let placeholder = nea_db::Corporation {
                    corporation_id: corp_id,
                    name: format!("Unknown Corp {}", corp_id),
                    alliance_id: None,
                    member_count: None,
                    fetched_at: Utc::now(),
                };
                let _ = nea_db::upsert_corporation(pool, &placeholder).await;
            }
            Err(e) => {
                tracing::debug!(corp_id, "doctrine_aggregator: failed to fetch corporation from ESI: {e}");
            }
        }
        tokio::time::sleep(Duration::from_millis(70)).await;
    }

    tracing::info!(resolved, "doctrine_aggregator: corporation name resolution complete");
}

async fn resolve_alliance_names(pool: &PgPool, esi: &EsiClient) {
    let uncached = match nea_db::get_uncached_alliance_ids(pool, 200).await {
        Ok(ids) => ids,
        Err(e) => {
            tracing::warn!("doctrine_aggregator: failed to get uncached alliance IDs: {e}");
            return;
        }
    };

    if uncached.is_empty() {
        return;
    }

    tracing::info!(count = uncached.len(), "doctrine_aggregator: resolving alliance names");

    let mut resolved = 0u64;
    for alliance_id in uncached {
        match esi.get_alliance(alliance_id).await {
            Ok(info) => {
                let alliance = nea_db::Alliance {
                    alliance_id,
                    name: info.name,
                    ticker: info.ticker,
                    fetched_at: Utc::now(),
                };
                if let Err(e) = nea_db::upsert_alliance(pool, &alliance).await {
                    tracing::warn!(alliance_id, "doctrine_aggregator: failed to cache alliance: {e}");
                } else {
                    resolved += 1;
                }
            }
            Err(nea_esi::EsiError::Api { status: 404, .. }) => {
                tracing::debug!(alliance_id, "doctrine_aggregator: alliance not found on ESI (404), caching placeholder");
                let placeholder = nea_db::Alliance {
                    alliance_id,
                    name: format!("Unknown Alliance {}", alliance_id),
                    ticker: None,
                    fetched_at: Utc::now(),
                };
                let _ = nea_db::upsert_alliance(pool, &placeholder).await;
            }
            Err(e) => {
                tracing::debug!(alliance_id, "doctrine_aggregator: failed to fetch alliance from ESI: {e}");
            }
        }
        tokio::time::sleep(Duration::from_millis(70)).await;
    }

    tracing::info!(resolved, "doctrine_aggregator: alliance name resolution complete");
}

async fn get_entity_kill_count(
    pool: &PgPool,
    entity_type: &str,
    entity_id: i64,
    window_days: i32,
) -> Result<i64, Box<dyn std::error::Error + Send + Sync>> {
    let column = match entity_type {
        "corporation" => "corporation_id",
        "alliance" => "alliance_id",
        _ => return Ok(0),
    };
    let query = format!(
        "SELECT COUNT(DISTINCT killmail_id) FROM killmail_attackers WHERE {} = $1 AND kill_time >= NOW() - $2 * INTERVAL '1 day'",
        column
    );
    let (count,): (i64,) = sqlx::query_as(&query)
        .bind(entity_id)
        .bind(window_days)
        .fetch_one(pool)
        .await?;
    Ok(count)
}

async fn compute_doctrine_profile(
    pool: &PgPool,
    entity_type: &str,
    entity_id: i64,
    entity_name: &str,
    member_count: i32,
    window_days: i32,
) -> Result<DoctrineProfile, Box<dyn std::error::Error + Send + Sync>> {
    let column = match entity_type {
        "corporation" => "corporation_id",
        "alliance" => "alliance_id",
        _ => return Err("invalid entity_type".into()),
    };

    // Total kills
    let query = format!(
        "SELECT COUNT(DISTINCT killmail_id) FROM killmail_attackers WHERE {} = $1 AND kill_time >= NOW() - $2 * INTERVAL '1 day'",
        column
    );
    let (total_kills,): (i64,) = sqlx::query_as(&query)
        .bind(entity_id)
        .bind(window_days)
        .fetch_one(pool)
        .await?;

    // Total losses
    let query = format!(
        "SELECT COUNT(DISTINCT killmail_id) FROM killmail_victims WHERE {} = $1 AND kill_time >= NOW() - $2 * INTERVAL '1 day'",
        column
    );
    let (total_losses,): (i64,) = sqlx::query_as(&query)
        .bind(entity_id)
        .bind(window_days)
        .fetch_one(pool)
        .await?;

    // 3a: Ship usage distribution
    let ship_usage = compute_ship_usage(pool, column, entity_id, window_days).await?;

    // 3b: Doctrine detection
    let doctrines = compute_doctrines(pool, column, entity_id, window_days).await?;

    // 3c: Trend detection
    let ship_trends = compute_trends(pool, column, entity_id, window_days).await?;

    // 3d: Fleet compositions
    let fleet_comps = compute_fleet_comps(pool, column, entity_id, window_days).await;

    Ok(DoctrineProfile {
        id: 0,
        entity_type: entity_type.to_string(),
        entity_id,
        entity_name: entity_name.to_string(),
        window_days,
        member_count,
        total_kills: total_kills as i32,
        total_losses: total_losses as i32,
        ship_usage: Some(serde_json::Value::Array(ship_usage)),
        doctrines: Some(serde_json::Value::Array(doctrines)),
        ship_trends: Some(serde_json::Value::Array(ship_trends)),
        fleet_comps: Some(serde_json::Value::Array(fleet_comps.unwrap_or_default())),
        computed_at: Utc::now(),
    })
}

async fn compute_ship_usage(
    pool: &PgPool,
    column: &str,
    entity_id: i64,
    window_days: i32,
) -> Result<Vec<serde_json::Value>, Box<dyn std::error::Error + Send + Sync>> {
    let query = format!(
        r#"
        SELECT ship_type_id, COUNT(*) as cnt
        FROM killmail_attackers
        WHERE {} = $1 AND kill_time >= NOW() - $2 * INTERVAL '1 day'
          AND ship_type_id > 0
        GROUP BY ship_type_id ORDER BY cnt DESC LIMIT 20
        "#,
        column
    );
    let rows: Vec<(i32, i64)> = sqlx::query_as(&query)
        .bind(entity_id)
        .bind(window_days)
        .fetch_all(pool)
        .await?;

    let total: i64 = rows.iter().map(|(_, c)| c).sum();
    let mut result = Vec::new();
    for (type_id, count) in &rows {
        let name = get_type_name(pool, *type_id).await;
        let pct = if total > 0 {
            (*count as f64 / total as f64 * 100.0).round()
        } else {
            0.0
        };
        result.push(serde_json::json!({
            "type_id": type_id,
            "name": name,
            "count": count,
            "pct": pct,
        }));
    }
    Ok(result)
}

async fn compute_doctrines(
    pool: &PgPool,
    column: &str,
    entity_id: i64,
    window_days: i32,
) -> Result<Vec<serde_json::Value>, Box<dyn std::error::Error + Send + Sync>> {
    // Get top 10 most-used ship types
    let query = format!(
        r#"
        SELECT ship_type_id, COUNT(*) as cnt
        FROM killmail_attackers
        WHERE {} = $1 AND kill_time >= NOW() - $2 * INTERVAL '1 day'
          AND ship_type_id > 0
        GROUP BY ship_type_id ORDER BY cnt DESC LIMIT 10
        "#,
        column
    );
    let top_ships: Vec<(i32, i64)> = sqlx::query_as(&query)
        .bind(entity_id)
        .bind(window_days)
        .fetch_all(pool)
        .await?;

    let mut all_doctrines = Vec::new();

    for (ship_type_id, _) in &top_ships {
        // Get loss killmails where a member was the victim flying this ship
        let query = format!(
            r#"
            SELECT killmail_id, kill_time
            FROM killmail_victims
            WHERE {} = $1 AND ship_type_id = $2
              AND kill_time >= NOW() - $3 * INTERVAL '1 day'
            ORDER BY kill_time DESC
            LIMIT 200
            "#,
            column
        );
        let loss_killmail_ids: Vec<(i64, chrono::DateTime<Utc>)> = sqlx::query_as(&query)
            .bind(entity_id)
            .bind(ship_type_id)
            .bind(window_days)
            .fetch_all(pool)
            .await?;

        // Reconstruct fittings from killmail_items
        let mut fittings: Vec<Vec<(i32, i32)>> = Vec::new();
        let mut pilot_ids: HashSet<Option<i64>> = HashSet::new();

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

            // Track unique pilots
            let victim: Option<(Option<i64>,)> = sqlx::query_as(
                "SELECT character_id FROM killmail_victims WHERE killmail_id = $1 AND kill_time = $2",
            )
            .bind(km_id)
            .bind(km_time)
            .fetch_optional(pool)
            .await?;
            if let Some((char_id,)) = victim {
                pilot_ids.insert(char_id);
            }
        }

        if fittings.is_empty() {
            continue;
        }

        // Cluster via Jaccard >= 0.7 (lower threshold for corp doctrines)
        let clusters = cluster_fittings(&fittings, 0.7);

        let ship_name = get_type_name(pool, *ship_type_id).await;

        for cluster in clusters {
            // Filter: pilot_count >= 3 to qualify as doctrine
            let cluster_pilot_count = pilot_ids.len(); // approximate
            if cluster.count < 3 {
                continue;
            }

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

            all_doctrines.push(serde_json::json!({
                "ship_type_id": ship_type_id,
                "ship_name": ship_name,
                "canonical_fit": modules,
                "occurrences": cluster.count,
                "pilot_count": cluster_pilot_count,
                "variant_count": cluster.variant_count,
            }));
        }
    }

    Ok(all_doctrines)
}

async fn compute_trends(
    pool: &PgPool,
    column: &str,
    entity_id: i64,
    window_days: i32,
) -> Result<Vec<serde_json::Value>, Box<dyn std::error::Error + Send + Sync>> {
    // Current window
    let query = format!(
        r#"
        SELECT ship_type_id, COUNT(*) as cnt
        FROM killmail_attackers
        WHERE {} = $1 AND kill_time >= NOW() - $2 * INTERVAL '1 day'
          AND ship_type_id > 0
        GROUP BY ship_type_id
        "#,
        column
    );
    let current: Vec<(i32, i64)> = sqlx::query_as(&query)
        .bind(entity_id)
        .bind(window_days)
        .fetch_all(pool)
        .await?;

    // Previous window
    let query = format!(
        r#"
        SELECT ship_type_id, COUNT(*) as cnt
        FROM killmail_attackers
        WHERE {} = $1
          AND kill_time >= NOW() - $2 * INTERVAL '1 day'
          AND kill_time < NOW() - $3 * INTERVAL '1 day'
          AND ship_type_id > 0
        GROUP BY ship_type_id
        "#,
        column
    );
    let previous: Vec<(i32, i64)> = sqlx::query_as(&query)
        .bind(entity_id)
        .bind(window_days * 2)
        .bind(window_days)
        .fetch_all(pool)
        .await?;

    let prev_map: std::collections::HashMap<i32, i64> = previous.into_iter().collect();
    let curr_map: std::collections::HashMap<i32, i64> = current.into_iter().collect();

    // Combine all ship type IDs
    let all_ids: HashSet<i32> = curr_map.keys().chain(prev_map.keys()).copied().collect();

    let mut trends: Vec<(i32, i64, i64, f64)> = Vec::new();
    for type_id in all_ids {
        let curr = *curr_map.get(&type_id).unwrap_or(&0);
        let prev = *prev_map.get(&type_id).unwrap_or(&0);

        // Filter out ships with < 3 occurrences in either window
        if curr < 3 && prev < 3 {
            continue;
        }

        let change_pct = if prev > 0 {
            (curr as f64 - prev as f64) / prev as f64 * 100.0
        } else if curr > 0 {
            100.0 // new ship
        } else {
            0.0
        };

        trends.push((type_id, curr, prev, change_pct));
    }

    // Sort by abs(change_pct) desc, take top 20
    trends.sort_by(|a, b| b.3.abs().partial_cmp(&a.3.abs()).unwrap_or(std::cmp::Ordering::Equal));
    trends.truncate(20);

    let mut result = Vec::new();
    for (type_id, current_count, previous_count, change_pct) in trends {
        let name = get_type_name(pool, type_id).await;
        result.push(serde_json::json!({
            "type_id": type_id,
            "name": name,
            "current_count": current_count,
            "previous_count": previous_count,
            "change_pct": (change_pct * 10.0).round() / 10.0,
        }));
    }
    Ok(result)
}

async fn compute_fleet_comps(
    pool: &PgPool,
    column: &str,
    entity_id: i64,
    window_days: i32,
) -> Option<Vec<serde_json::Value>> {
    let query = format!(
        r#"
        SELECT a1.ship_type_id, a2.ship_type_id, COUNT(DISTINCT a1.killmail_id) as cnt
        FROM killmail_attackers a1
        JOIN killmail_attackers a2
          ON a1.killmail_id = a2.killmail_id AND a1.kill_time = a2.kill_time
        WHERE a1.{} = $1
          AND a1.kill_time >= NOW() - $2 * INTERVAL '1 day'
          AND a1.ship_type_id > 0 AND a2.ship_type_id > 0
          AND a1.ship_type_id < a2.ship_type_id
        GROUP BY a1.ship_type_id, a2.ship_type_id
        HAVING COUNT(DISTINCT a1.killmail_id) >= 5
        ORDER BY cnt DESC LIMIT 20
        "#,
        column
    );

    // Use a timeout of 10 seconds for this potentially expensive query
    let result = tokio::time::timeout(
        Duration::from_secs(10),
        sqlx::query_as::<_, (i32, i32, i64)>(&query)
            .bind(entity_id)
            .bind(window_days)
            .fetch_all(pool),
    )
    .await;

    match result {
        Ok(Ok(rows)) => {
            let mut comps = Vec::new();
            for (ship1, ship2, count) in rows {
                let name1 = get_type_name(pool, ship1).await;
                let name2 = get_type_name(pool, ship2).await;
                comps.push(serde_json::json!({
                    "ships": [
                        {"type_id": ship1, "name": name1},
                        {"type_id": ship2, "name": name2}
                    ],
                    "occurrence_count": count,
                }));
            }
            Some(comps)
        }
        Ok(Err(e)) => {
            tracing::warn!(entity_id, "doctrine_aggregator: fleet comp query failed: {e}");
            None
        }
        Err(_) => {
            tracing::warn!(entity_id, "doctrine_aggregator: fleet comp query timed out (>10s), skipping");
            None
        }
    }
}
