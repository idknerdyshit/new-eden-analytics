use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use nea_db::CharacterProfile;
use nea_esi::EsiClient;
use sqlx::PgPool;
use tokio::time;

const WORKER_STATE_KEY: &str = "profile_aggregation_last_run";

/// Fitted slot flag ranges in EVE Online.
const HIGH_SLOT_START: i32 = 27;
const HIGH_SLOT_END: i32 = 34;
const MID_SLOT_START: i32 = 19;
const MID_SLOT_END: i32 = 26;
const LOW_SLOT_START: i32 = 11;
const LOW_SLOT_END: i32 = 18;
const RIG_SLOT_START: i32 = 92;
const RIG_SLOT_END: i32 = 94;
const SUBSYSTEM_START: i32 = 125;
const SUBSYSTEM_END: i32 = 131;

fn is_fitted_slot(flag: i32) -> bool {
    (flag >= LOW_SLOT_START && flag <= LOW_SLOT_END)
        || (flag >= MID_SLOT_START && flag <= MID_SLOT_END)
        || (flag >= HIGH_SLOT_START && flag <= HIGH_SLOT_END)
        || (flag >= RIG_SLOT_START && flag <= RIG_SLOT_END)
        || (flag >= SUBSYSTEM_START && flag <= SUBSYSTEM_END)
}

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
    let uncached = match nea_db::get_uncached_character_ids(pool, 200).await {
        Ok(ids) => ids,
        Err(e) => {
            tracing::warn!("profile_aggregator: failed to get uncached character IDs: {e}");
            return;
        }
    };

    if uncached.is_empty() {
        return;
    }

    tracing::info!(count = uncached.len(), "profile_aggregator: resolving character names");

    let mut resolved = 0u64;
    for character_id in uncached {
        match esi.get_character(character_id).await {
            Ok(info) => {
                let character = nea_db::Character {
                    character_id,
                    name: info.name,
                    corporation_id: info.corporation_id,
                    alliance_id: info.alliance_id,
                    fetched_at: Utc::now(),
                };
                if let Err(e) = nea_db::upsert_character(pool, &character).await {
                    tracing::warn!(character_id, "profile_aggregator: failed to cache character: {e}");
                } else {
                    resolved += 1;
                }
            }
            Err(e) => {
                tracing::debug!(character_id, "profile_aggregator: failed to fetch character from ESI: {e}");
            }
        }
        // Pace ESI requests
        tokio::time::sleep(Duration::from_millis(70)).await;
    }

    tracing::info!(resolved, "profile_aggregator: character name resolution complete");
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

async fn get_type_name(pool: &PgPool, type_id: i32) -> String {
    match nea_db::get_type(pool, type_id).await {
        Ok(Some(t)) => t.name,
        _ => format!("Type {}", type_id),
    }
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
        let clusters = cluster_fittings(&fittings);

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

struct FittingCluster {
    canonical_idx: usize,
    count: usize,
    variant_count: usize,
}

fn cluster_fittings(fittings: &[Vec<(i32, i32)>]) -> Vec<FittingCluster> {
    let fitting_sets: Vec<std::collections::HashSet<i32>> = fittings
        .iter()
        .map(|f| f.iter().map(|(type_id, _)| *type_id).collect())
        .collect();

    // Track which fittings are assigned to clusters
    let mut assigned = vec![false; fittings.len()];
    let mut clusters: Vec<FittingCluster> = Vec::new();

    // Count exact duplicates for canonical selection
    let mut exact_counts: HashMap<Vec<i32>, (usize, usize)> = HashMap::new(); // sorted type_ids -> (count, first_index)
    for (i, f) in fittings.iter().enumerate() {
        let mut sorted: Vec<i32> = f.iter().map(|(tid, _)| *tid).collect();
        sorted.sort();
        exact_counts
            .entry(sorted)
            .and_modify(|(c, _)| *c += 1)
            .or_insert((1, i));
    }

    for i in 0..fittings.len() {
        if assigned[i] {
            continue;
        }

        assigned[i] = true;
        let mut members = vec![i];

        for j in (i + 1)..fittings.len() {
            if assigned[j] {
                continue;
            }
            let jaccard = jaccard_similarity(&fitting_sets[i], &fitting_sets[j]);
            if jaccard >= 0.8 {
                assigned[j] = true;
                members.push(j);
            }
        }

        // Find the canonical fit (most common exact fit in this cluster)
        let mut best_idx = members[0];
        let mut best_count = 0usize;
        for &m in &members {
            let mut sorted: Vec<i32> = fittings[m].iter().map(|(tid, _)| *tid).collect();
            sorted.sort();
            if let Some((count, _)) = exact_counts.get(&sorted) {
                if *count > best_count {
                    best_count = *count;
                    best_idx = m;
                }
            }
        }

        let unique_fits: std::collections::HashSet<Vec<i32>> = members
            .iter()
            .map(|&m| {
                let mut sorted: Vec<i32> = fittings[m].iter().map(|(tid, _)| *tid).collect();
                sorted.sort();
                sorted
            })
            .collect();

        clusters.push(FittingCluster {
            canonical_idx: best_idx,
            count: members.len(),
            variant_count: unique_fits.len(),
        });
    }

    clusters
}

fn jaccard_similarity(a: &std::collections::HashSet<i32>, b: &std::collections::HashSet<i32>) -> f64 {
    if a.is_empty() && b.is_empty() {
        return 1.0;
    }
    let intersection = a.intersection(b).count();
    let union = a.union(b).count();
    if union == 0 {
        return 0.0;
    }
    intersection as f64 / union as f64
}
