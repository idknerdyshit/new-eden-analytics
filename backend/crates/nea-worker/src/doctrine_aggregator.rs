use std::collections::{HashMap, HashSet};
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

pub async fn run_cycle(
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

/// EVE Online group names for ships that typically don't deal damage and
/// won't appear as attackers on killmails.
const SUPPORT_GROUP_NAMES: &[&str] = &[
    "Logistics",
    "Logistics Frigate",
];

/// Per-ship fitting data extracted from loss killmails.
struct ShipFitData {
    ship_type_id: i32,
    ship_name: String,
    canonical_fit: Vec<serde_json::Value>,
    variants: Vec<Vec<serde_json::Value>>,
    occurrences: usize,
    pilot_count: usize,
}

/// An engagement is a cluster of kills in the same solar system within a
/// short time window, representing a single fleet fight.
struct Engagement {
    kill_ids: Vec<i64>,
    ship_types: HashSet<i32>,
    pilot_count: u32,
    system_id: i32,
}

async fn compute_doctrines(
    pool: &PgPool,
    column: &str,
    entity_id: i64,
    window_days: i32,
) -> Result<Vec<serde_json::Value>, Box<dyn std::error::Error + Send + Sync>> {
    // ── Step 1: Fetch attacker data with location ────────────────────
    let query = format!(
        r#"
        SELECT a.killmail_id, a.ship_type_id, a.kill_time, k.solar_system_id
        FROM killmail_attackers a
        JOIN killmails k ON a.killmail_id = k.killmail_id AND a.kill_time = k.kill_time
        WHERE a.{col} = $1
          AND a.kill_time >= NOW() - $2 * INTERVAL '1 day'
          AND a.ship_type_id > 0
          AND k.solar_system_id IS NOT NULL
        ORDER BY k.solar_system_id, a.kill_time
        "#,
        col = column
    );
    let attack_rows: Vec<(i64, i32, chrono::DateTime<Utc>, i32)> = sqlx::query_as(&query)
        .bind(entity_id)
        .bind(window_days)
        .fetch_all(pool)
        .await?;

    if attack_rows.is_empty() {
        return Ok(Vec::new());
    }

    // ── Step 2: Group kills into engagements ─────────────────────────
    // An engagement = kills in the same system within ±15 min of each other.
    // First, collect per-killmail data.
    struct KillInfo {
        time: chrono::DateTime<Utc>,
        system_id: i32,
        ship_types: HashSet<i32>,
        pilot_count: u32,
    }
    let mut kill_map: HashMap<i64, KillInfo> = HashMap::new();
    for (km_id, ship_type_id, kill_time, system_id) in &attack_rows {
        let entry = kill_map.entry(*km_id).or_insert_with(|| KillInfo {
            time: *kill_time,
            system_id: *system_id,
            ship_types: HashSet::new(),
            pilot_count: 0,
        });
        entry.ship_types.insert(*ship_type_id);
        entry.pilot_count += 1;
    }

    // Sort kills by (system, time) then merge into engagements
    let mut kills_sorted: Vec<(i64, &KillInfo)> = kill_map.iter().map(|(id, info)| (*id, info)).collect();
    kills_sorted.sort_by(|a, b| {
        a.1.system_id.cmp(&b.1.system_id).then(a.1.time.cmp(&b.1.time))
    });

    let engagement_window = chrono::Duration::minutes(15);
    let mut engagements: Vec<Engagement> = Vec::new();

    for (km_id, info) in &kills_sorted {
        let merged = engagements.last_mut().and_then(|eng| {
            if eng.system_id == info.system_id && (info.time - eng.kill_ids.iter()
                .filter_map(|id| kill_map.get(id))
                .map(|k| k.time)
                .max()
                .unwrap_or(info.time)).abs() <= engagement_window
            {
                Some(eng)
            } else {
                None
            }
        });
        if let Some(eng) = merged {
            eng.kill_ids.push(*km_id);
            eng.ship_types.extend(&info.ship_types);
            eng.pilot_count += info.pilot_count;
        } else {
            engagements.push(Engagement {
                kill_ids: vec![*km_id],
                ship_types: info.ship_types.clone(),
                pilot_count: info.pilot_count,
                system_id: info.system_id,
            });
        }
    }

    // Only keep engagements with ≥5 entity pilots (real fleet fights)
    engagements.retain(|e| e.pilot_count >= 5);

    if engagements.is_empty() {
        return Ok(Vec::new());
    }

    tracing::debug!(
        entity_id,
        engagements = engagements.len(),
        "doctrine_aggregator: built engagements"
    );

    // ── Step 3: Cluster engagements by composition similarity ────────
    // Greedy seed-expansion: pick the largest unassigned engagement as seed,
    // find all engagements with Jaccard ≥ 0.4 to the seed's ship-type set,
    // then extract core ship types present in ≥30% of cluster engagements.
    let mut assigned = vec![false; engagements.len()];
    let mut doctrine_groups: Vec<Vec<i32>> = Vec::new();
    // Track which engagements belong to each doctrine for support ship assignment
    let mut doctrine_engagement_kills: Vec<HashSet<i64>> = Vec::new();

    // Sort indices by pilot_count descending for seed selection
    let mut indices: Vec<usize> = (0..engagements.len()).collect();
    indices.sort_by(|a, b| engagements[*b].pilot_count.cmp(&engagements[*a].pilot_count));

    for &seed_idx in &indices {
        if assigned[seed_idx] {
            continue;
        }
        assigned[seed_idx] = true;

        let seed_types = &engagements[seed_idx].ship_types;
        let mut cluster_indices = vec![seed_idx];

        for &j in &indices {
            if assigned[j] {
                continue;
            }
            let jaccard = jaccard_i32(seed_types, &engagements[j].ship_types);
            if jaccard >= 0.4 {
                assigned[j] = true;
                cluster_indices.push(j);
            }
        }

        // Need ≥5 engagements to call it a recurring doctrine
        if cluster_indices.len() < 5 {
            continue;
        }

        // Extract core ship types: present in ≥30% of cluster engagements
        let mut type_counts: HashMap<i32, usize> = HashMap::new();
        for &ci in &cluster_indices {
            for &tid in &engagements[ci].ship_types {
                *type_counts.entry(tid).or_insert(0) += 1;
            }
        }
        let threshold = (cluster_indices.len() as f64 * 0.3).ceil() as usize;
        let core_types: Vec<i32> = type_counts
            .into_iter()
            .filter(|(_, count)| *count >= threshold)
            .map(|(tid, _)| tid)
            .collect();

        if core_types.len() >= 2 {
            // Collect all kill IDs for this doctrine's engagements
            let kills: HashSet<i64> = cluster_indices
                .iter()
                .flat_map(|&ci| engagements[ci].kill_ids.iter().copied())
                .collect();
            doctrine_engagement_kills.push(kills);
            doctrine_groups.push(core_types);
        }
    }

    // ── Step 4: Detect support ships via temporal-spatial correlation ─
    let support_ships = detect_support_ships(pool, column, entity_id, window_days).await;

    for (support_type_id, nearby_kill_ids) in &support_ships {
        if doctrine_groups.iter().any(|g| g.contains(support_type_id)) {
            continue;
        }

        // Find which doctrine group has the most overlap with this support
        // ship's nearby kills
        let mut best_group_idx: Option<usize> = None;
        let mut best_overlap = 0usize;
        for (gi, kills) in doctrine_engagement_kills.iter().enumerate() {
            let overlap = nearby_kill_ids.intersection(kills).count();
            if overlap > best_overlap {
                best_overlap = overlap;
                best_group_idx = Some(gi);
            }
        }

        if let Some(gi) = best_group_idx {
            let group_total = doctrine_engagement_kills[gi].len();
            let threshold = (group_total as f64 * 0.05).max(3.0) as usize;
            if best_overlap >= threshold {
                doctrine_groups[gi].push(*support_type_id);
            }
        }
    }

    // ── Step 5: Compute per-ship fitting data from losses ────────────
    let mut all_doctrine_ship_ids: HashSet<i32> = HashSet::new();
    for group in &doctrine_groups {
        for tid in group {
            all_doctrine_ship_ids.insert(*tid);
        }
    }

    let mut fit_data: HashMap<i32, ShipFitData> = HashMap::new();
    for &ship_type_id in &all_doctrine_ship_ids {
        if let Some(data) = compute_ship_fits(pool, column, entity_id, window_days, ship_type_id).await? {
            fit_data.insert(ship_type_id, data);
        }
    }

    // ── Step 6: Assemble output ──────────────────────────────────────
    let mut result = Vec::new();
    for group in &doctrine_groups {
        let ships: Vec<serde_json::Value> = group
            .iter()
            .map(|&tid| {
                if let Some(d) = fit_data.get(&tid) {
                    serde_json::json!({
                        "ship_type_id": d.ship_type_id,
                        "ship_name": d.ship_name,
                        "canonical_fit": d.canonical_fit,
                        "variants": d.variants,
                        "occurrences": d.occurrences,
                        "pilot_count": d.pilot_count,
                    })
                } else {
                    let name = tid.to_string(); // resolved below
                    serde_json::json!({
                        "ship_type_id": tid,
                        "ship_name": name,
                        "canonical_fit": [],
                        "variants": [],
                        "occurrences": 0,
                        "pilot_count": 0,
                    })
                }
            })
            .collect();
        result.push(serde_json::json!({ "ships": ships }));
    }

    // Resolve names for ships without fit data
    for group_val in &mut result {
        if let Some(ships) = group_val.get_mut("ships").and_then(|s| s.as_array_mut()) {
            for ship in ships.iter_mut() {
                if ship.get("canonical_fit").and_then(|f| f.as_array()).map(|a| a.is_empty()).unwrap_or(true) {
                    if let Some(tid) = ship.get("ship_type_id").and_then(|t| t.as_i64()) {
                        let name = get_type_name(pool, tid as i32).await;
                        ship["ship_name"] = serde_json::Value::String(name);
                    }
                }
            }
        }
    }

    Ok(result)
}

fn jaccard_i32(a: &HashSet<i32>, b: &HashSet<i32>) -> f64 {
    if a.is_empty() && b.is_empty() {
        return 1.0;
    }
    let intersection = a.intersection(b).count();
    let union = a.union(b).count();
    if union == 0 { 0.0 } else { intersection as f64 / union as f64 }
}

/// Compute fitting clusters for a single ship type from entity's loss killmails.
async fn compute_ship_fits(
    pool: &PgPool,
    column: &str,
    entity_id: i64,
    window_days: i32,
    ship_type_id: i32,
) -> Result<Option<ShipFitData>, Box<dyn std::error::Error + Send + Sync>> {
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
    let loss_killmails: Vec<(i64, chrono::DateTime<Utc>)> = sqlx::query_as(&query)
        .bind(entity_id)
        .bind(ship_type_id)
        .bind(window_days)
        .fetch_all(pool)
        .await?;

    if loss_killmails.is_empty() {
        return Ok(None);
    }

    let mut fittings: Vec<Vec<(i32, i32)>> = Vec::new();
    let mut pilot_ids: HashSet<Option<i64>> = HashSet::new();

    for (km_id, km_time) in &loss_killmails {
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
        return Ok(None);
    }

    let clusters = cluster_fittings(&fittings, 0.7);
    let ship_name = get_type_name(pool, ship_type_id).await;

    // Take the largest cluster that qualifies
    let best = clusters.into_iter().filter(|c| c.count >= 3).max_by_key(|c| c.count);

    let Some(cluster) = best else {
        return Ok(None);
    };

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

    let canonical_sorted: Vec<i32> = {
        let mut s: Vec<i32> = canonical.iter().map(|(tid, _)| *tid).collect();
        s.sort();
        s
    };
    let mut seen_variants: HashSet<Vec<i32>> = HashSet::new();
    seen_variants.insert(canonical_sorted);
    let mut variants: Vec<Vec<serde_json::Value>> = Vec::new();

    for &idx in &cluster.member_indices {
        let fit = &fittings[idx];
        let mut sorted_key: Vec<i32> = fit.iter().map(|(tid, _)| *tid).collect();
        sorted_key.sort();
        if seen_variants.insert(sorted_key) {
            let mut variant_modules = Vec::new();
            for (type_id, flag) in fit {
                let name = get_type_name(pool, *type_id).await;
                variant_modules.push(serde_json::json!({
                    "type_id": type_id,
                    "name": name,
                    "flag": flag,
                }));
            }
            variants.push(variant_modules);
        }
    }

    Ok(Some(ShipFitData {
        ship_type_id,
        ship_name,
        canonical_fit: modules,
        variants,
        occurrences: cluster.count,
        pilot_count: pilot_ids.len(),
    }))
}

/// Detect support ships (logistics, etc.) that were lost near fleet engagements.
///
/// Returns a vec of (ship_type_id, nearby_kill_ids) for each support ship type
/// that was lost within ±15 minutes and the same solar system as entity kills.
async fn detect_support_ships(
    pool: &PgPool,
    column: &str,
    entity_id: i64,
    window_days: i32,
) -> Vec<(i32, HashSet<i64>)> {
    // Get entity's kill events with location
    let query = format!(
        r#"
        SELECT DISTINCT a.killmail_id, k.kill_time, k.solar_system_id
        FROM killmail_attackers a
        JOIN killmails k ON a.killmail_id = k.killmail_id AND a.kill_time = k.kill_time
        WHERE a.{} = $1
          AND a.kill_time >= NOW() - $2 * INTERVAL '1 day'
          AND k.solar_system_id IS NOT NULL
        "#,
        column
    );
    let kill_events: Vec<(i64, chrono::DateTime<Utc>, i32)> = match sqlx::query_as(&query)
        .bind(entity_id)
        .bind(window_days)
        .fetch_all(pool)
        .await
    {
        Ok(rows) => rows,
        Err(e) => {
            tracing::warn!(entity_id, "doctrine_aggregator: support ship kill events query failed: {e}");
            return Vec::new();
        }
    };

    if kill_events.is_empty() {
        return Vec::new();
    }

    // Get entity's losses with location, filtered to support ship group names
    let query = format!(
        r#"
        SELECT v.ship_type_id, v.killmail_id, v.kill_time, k.solar_system_id
        FROM killmail_victims v
        JOIN killmails k ON v.killmail_id = k.killmail_id AND v.kill_time = k.kill_time
        JOIN sde_types s ON v.ship_type_id = s.type_id
        WHERE v.{} = $1
          AND v.kill_time >= NOW() - $2 * INTERVAL '1 day'
          AND k.solar_system_id IS NOT NULL
          AND s.group_name = ANY($3)
        "#,
        column
    );
    let support_group_names: Vec<&str> = SUPPORT_GROUP_NAMES.to_vec();
    let loss_events: Vec<(i32, i64, chrono::DateTime<Utc>, i32)> = match sqlx::query_as(&query)
        .bind(entity_id)
        .bind(window_days)
        .bind(&support_group_names)
        .fetch_all(pool)
        .await
    {
        Ok(rows) => rows,
        Err(e) => {
            tracing::warn!(entity_id, "doctrine_aggregator: support ship loss events query failed: {e}");
            return Vec::new();
        }
    };

    // Index kill events by (solar_system_id) for quick lookup
    let mut kills_by_system: HashMap<i32, Vec<(i64, chrono::DateTime<Utc>)>> = HashMap::new();
    for (km_id, kill_time, system_id) in &kill_events {
        kills_by_system
            .entry(*system_id)
            .or_default()
            .push((*km_id, *kill_time));
    }

    // For each support loss, find kills in same system within ±15 min
    let window = chrono::Duration::minutes(15);
    let mut support_map: HashMap<i32, HashSet<i64>> = HashMap::new();

    for (ship_type_id, _loss_km_id, loss_time, system_id) in &loss_events {
        if let Some(system_kills) = kills_by_system.get(system_id) {
            let nearby: HashSet<i64> = system_kills
                .iter()
                .filter(|(_, kt)| {
                    let diff = (*kt - *loss_time).abs();
                    diff <= window
                })
                .map(|(km_id, _)| *km_id)
                .collect();

            if !nearby.is_empty() {
                support_map
                    .entry(*ship_type_id)
                    .or_default()
                    .extend(nearby);
            }
        }
    }

    support_map.into_iter().collect()
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
    // Get the distinct ship types per killmail for this entity's attackers
    let query = format!(
        r#"
        SELECT killmail_id, ship_type_id
        FROM killmail_attackers
        WHERE {} = $1
          AND kill_time >= NOW() - $2 * INTERVAL '1 day'
          AND ship_type_id > 0
        ORDER BY killmail_id
        "#,
        column
    );

    let result = tokio::time::timeout(
        Duration::from_secs(15),
        sqlx::query_as::<_, (i64, i32)>(&query)
            .bind(entity_id)
            .bind(window_days)
            .fetch_all(pool),
    )
    .await;

    let rows = match result {
        Ok(Ok(rows)) => rows,
        Ok(Err(e)) => {
            tracing::warn!(entity_id, "doctrine_aggregator: fleet comp query failed: {e}");
            return None;
        }
        Err(_) => {
            tracing::warn!(entity_id, "doctrine_aggregator: fleet comp query timed out (>15s), skipping");
            return None;
        }
    };

    // Group ship types by killmail, keeping only distinct types per kill
    let mut kills_ships: HashMap<i64, Vec<i32>> = HashMap::new();
    for (killmail_id, ship_type_id) in &rows {
        kills_ships
            .entry(*killmail_id)
            .or_default()
            .push(*ship_type_id);
    }

    // Count occurrences of each sorted ship-type set (only kills with 2+ distinct types)
    let mut comp_counts: HashMap<Vec<i32>, u64> = HashMap::new();
    for ships in kills_ships.values() {
        let mut unique: Vec<i32> = ships.iter().copied().collect::<HashSet<_>>().into_iter().collect();
        if unique.len() < 2 {
            continue;
        }
        unique.sort_unstable();
        *comp_counts.entry(unique).or_insert(0) += 1;
    }

    // Filter to >=5 occurrences, take top 20
    let mut ranked: Vec<(Vec<i32>, u64)> = comp_counts
        .into_iter()
        .filter(|(_, count)| *count >= 5)
        .collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1));
    ranked.truncate(20);

    let mut comps = Vec::new();
    for (ship_ids, count) in ranked {
        let mut ships = Vec::new();
        for type_id in &ship_ids {
            let name = get_type_name(pool, *type_id).await;
            ships.push(serde_json::json!({"type_id": type_id, "name": name}));
        }
        comps.push(serde_json::json!({
            "ships": ships,
            "occurrence_count": count,
        }));
    }
    Some(comps)
}
