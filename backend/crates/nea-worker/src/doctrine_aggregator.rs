use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use nea_db::DoctrineProfile;
use nea_esi::EsiClient;
use sqlx::PgPool;
use tokio::time;

use crate::fitting_utils::{cluster_fittings, is_fitted_slot, resolve_type_name};

const WORKER_STATE_KEY: &str = "doctrine_aggregation_last_run";
const WINDOWS: &[i32] = &[7, 30, 90];
const DEFAULT_MIN_KILLS_FOR_PROFILE: i64 = 15;
const ENGAGEMENT_WINDOW_MINUTES: i64 = 15;
const MIN_ENGAGEMENT_PILOTS: usize = 5;
const MIN_DOCTRINE_ENGAGEMENTS: usize = 5;
const MIN_DOCTRINE_DISTINCT_PILOTS: usize = 8;
const CLUSTER_SIMILARITY_THRESHOLD: f64 = 0.5;
const DOCTRINE_CORE_PRESENCE_THRESHOLD: f64 = 0.55;
const POST_MERGE_JACCARD_THRESHOLD: f64 = 0.7;
const POST_MERGE_OVERLAP_THRESHOLD: f64 = 0.85;
const SUPPORT_PRESENCE_THRESHOLD: f64 = 0.25;
const MIN_SUPPORT_ENGAGEMENTS: usize = 2;

fn min_kills_for_profile() -> i64 {
    std::env::var("DOCTRINE_MIN_KILLS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_MIN_KILLS_FOR_PROFILE)
}

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
        if kill_count < min_kills_for_profile() {
            continue;
        }

        let corp = nea_db::get_corporation(pool, corp_id).await?;
        let entity_name = corp
            .as_ref()
            .map(|c| c.name.clone())
            .unwrap_or_else(|| format!("Corp {}", corp_id));
        let member_count = corp.and_then(|c| c.member_count).unwrap_or(0);

        for &window in WINDOWS {
            match compute_doctrine_profile(
                pool,
                "corporation",
                corp_id,
                &entity_name,
                member_count,
                window,
            )
            .await
            {
                Ok(profile) => {
                    if let Err(e) = nea_db::upsert_doctrine_profile(pool, &profile).await {
                        tracing::warn!(
                            corp_id,
                            window,
                            "doctrine_aggregator: failed to upsert profile: {e}"
                        );
                    } else {
                        computed += 1;
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        corp_id,
                        window,
                        "doctrine_aggregator: failed to compute profile: {e}"
                    );
                }
            }
        }
    }

    // Step 3b: Compute profiles for alliances
    for &alliance_id in &alliance_ids {
        let kill_count = get_entity_kill_count(pool, "alliance", alliance_id, 30).await?;
        if kill_count < min_kills_for_profile() {
            continue;
        }

        let entity_name = nea_db::get_alliance(pool, alliance_id)
            .await?
            .map(|a| a.name)
            .unwrap_or_else(|| format!("Alliance {}", alliance_id));

        for &window in WINDOWS {
            match compute_doctrine_profile(pool, "alliance", alliance_id, &entity_name, 0, window)
                .await
            {
                Ok(profile) => {
                    if let Err(e) = nea_db::upsert_doctrine_profile(pool, &profile).await {
                        tracing::warn!(
                            alliance_id,
                            window,
                            "doctrine_aggregator: failed to upsert profile: {e}"
                        );
                    } else {
                        computed += 1;
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        alliance_id,
                        window,
                        "doctrine_aggregator: failed to compute profile: {e}"
                    );
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

    tracing::info!(
        count = uncached.len(),
        "doctrine_aggregator: resolving corporation names"
    );

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
                    tracing::warn!(
                        corp_id,
                        "doctrine_aggregator: failed to cache corporation: {e}"
                    );
                } else {
                    resolved += 1;
                }
            }
            Err(nea_esi::EsiError::Api { status: 404, .. }) => {
                tracing::debug!(
                    corp_id,
                    "doctrine_aggregator: corporation not found on ESI (404), caching placeholder"
                );
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
                tracing::debug!(
                    corp_id,
                    "doctrine_aggregator: failed to fetch corporation from ESI: {e}"
                );
            }
        }
        tokio::time::sleep(Duration::from_millis(70)).await;
    }

    tracing::info!(
        resolved,
        "doctrine_aggregator: corporation name resolution complete"
    );
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

    tracing::info!(
        count = uncached.len(),
        "doctrine_aggregator: resolving alliance names"
    );

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
                    tracing::warn!(
                        alliance_id,
                        "doctrine_aggregator: failed to cache alliance: {e}"
                    );
                } else {
                    resolved += 1;
                }
            }
            Err(nea_esi::EsiError::Api { status: 404, .. }) => {
                tracing::debug!(
                    alliance_id,
                    "doctrine_aggregator: alliance not found on ESI (404), caching placeholder"
                );
                let placeholder = nea_db::Alliance {
                    alliance_id,
                    name: format!("Unknown Alliance {}", alliance_id),
                    ticker: None,
                    fetched_at: Utc::now(),
                };
                let _ = nea_db::upsert_alliance(pool, &placeholder).await;
            }
            Err(e) => {
                tracing::debug!(
                    alliance_id,
                    "doctrine_aggregator: failed to fetch alliance from ESI: {e}"
                );
            }
        }
        tokio::time::sleep(Duration::from_millis(70)).await;
    }

    tracing::info!(
        resolved,
        "doctrine_aggregator: alliance name resolution complete"
    );
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
    let type_ids: Vec<i32> = rows.iter().map(|(id, _)| *id).collect();
    let names = nea_db::get_type_names(pool, &type_ids).await?;
    let result: Vec<serde_json::Value> = rows
        .iter()
        .map(|(type_id, count)| {
            let pct = if total > 0 {
                (*count as f64 / total as f64 * 100.0).round()
            } else {
                0.0
            };
            serde_json::json!({
                "type_id": type_id,
                "name": resolve_type_name(&names, *type_id),
                "count": count,
                "pct": pct,
            })
        })
        .collect();
    Ok(result)
}

/// EVE Online group names for ships that typically don't deal damage and
/// won't appear as attackers on killmails.
const SUPPORT_GROUP_NAMES: &[&str] = &["Logistics", "Logistics Frigate"];

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
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
enum PilotRef {
    Known(i64),
    Unknown(u64),
}

#[derive(Clone, Debug)]
struct Engagement {
    kill_ids: Vec<i64>,
    signature_ship_types: HashSet<i32>,
    ship_type_counts: HashMap<i32, usize>,
    pilot_ids: HashSet<PilotRef>,
    start_time: chrono::DateTime<Utc>,
}

impl Engagement {
    fn pilot_count(&self) -> usize {
        self.pilot_ids.len()
    }
}

#[derive(Clone)]
struct AttackRow {
    killmail_id: i64,
    ship_type_id: i32,
    kill_time: chrono::DateTime<Utc>,
    system_id: i32,
    character_id: Option<i64>,
}

struct KillInfo {
    time: chrono::DateTime<Utc>,
    system_id: i32,
    ship_pilots: HashMap<i32, HashSet<PilotRef>>,
    pilot_ids: HashSet<PilotRef>,
}

#[derive(Clone, Debug)]
struct DoctrineCluster {
    engagement_indices: Vec<usize>,
    support_ship_types: HashSet<i32>,
}

#[derive(Clone, Debug)]
struct DoctrineClusterStats {
    core_ship_types: HashSet<i32>,
    ship_presence: HashMap<i32, usize>,
    ship_weight: HashMap<i32, usize>,
    kill_ids: HashSet<i64>,
    distinct_pilots: HashSet<PilotRef>,
    engagement_count: usize,
    mean_similarity: f64,
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
        SELECT a.killmail_id, a.ship_type_id, a.kill_time, k.solar_system_id, a.character_id
        FROM killmail_attackers a
        JOIN killmails k ON a.killmail_id = k.killmail_id AND a.kill_time = k.kill_time
        JOIN sde_types s ON a.ship_type_id = s.type_id
        WHERE a.{col} = $1
          AND a.kill_time >= NOW() - $2 * INTERVAL '1 day'
          AND a.ship_type_id > 0
          AND k.solar_system_id IS NOT NULL
          AND s.category_id = 6
          AND s.group_id IS DISTINCT FROM 29
          AND s.published = TRUE
        ORDER BY k.solar_system_id, a.kill_time
        "#,
        col = column
    );
    let attack_rows: Vec<(i64, i32, chrono::DateTime<Utc>, i32, Option<i64>)> =
        sqlx::query_as(&query)
            .bind(entity_id)
            .bind(window_days)
            .fetch_all(pool)
            .await?;

    if attack_rows.is_empty() {
        return Ok(Vec::new());
    }

    let attack_rows: Vec<AttackRow> = attack_rows
        .into_iter()
        .map(
            |(killmail_id, ship_type_id, kill_time, system_id, character_id)| AttackRow {
                killmail_id,
                ship_type_id,
                kill_time,
                system_id,
                character_id,
            },
        )
        .collect();

    // ── Step 2: Group kills into engagements ─────────────────────────
    let engagements = build_engagements(&attack_rows);

    if engagements.is_empty() {
        return Ok(Vec::new());
    }

    tracing::debug!(
        entity_id,
        engagements = engagements.len(),
        "doctrine_aggregator: built engagements"
    );

    // ── Step 3: Cluster engagements by composition similarity ────────
    let mut doctrine_clusters = cluster_doctrines(&engagements);
    doctrine_clusters = merge_close_doctrine_clusters(doctrine_clusters, &engagements);

    // ── Step 4: Detect support ships via temporal-spatial correlation ─
    let support_ships = detect_support_ships(pool, column, entity_id, window_days).await;
    assign_support_ships(&mut doctrine_clusters, &engagements, &support_ships);

    // ── Step 5: Compute per-ship fitting data from losses ────────────
    let mut all_doctrine_ship_ids: HashSet<i32> = HashSet::new();
    for cluster in &doctrine_clusters {
        let stats = doctrine_cluster_stats(cluster, &engagements);
        for tid in ordered_doctrine_ship_ids(cluster, &stats) {
            all_doctrine_ship_ids.insert(*tid);
        }
    }

    let mut fit_data: HashMap<i32, ShipFitData> = HashMap::new();
    for &ship_type_id in &all_doctrine_ship_ids {
        if let Some(data) =
            compute_ship_fits(pool, column, entity_id, window_days, ship_type_id).await?
        {
            fit_data.insert(ship_type_id, data);
        }
    }

    // ── Step 6: Assemble output ──────────────────────────────────────
    // Batch resolve names for ships without fit data
    let mut unresolved_ids: Vec<i32> = all_doctrine_ship_ids
        .iter()
        .filter(|tid| !fit_data.contains_key(tid))
        .copied()
        .collect();
    unresolved_ids.sort_unstable();
    unresolved_ids.dedup();
    let extra_names = nea_db::get_type_names(pool, &unresolved_ids).await?;

    let mut result = Vec::new();
    for cluster in &doctrine_clusters {
        let stats = doctrine_cluster_stats(cluster, &engagements);
        let coverage_pct = stats.engagement_count as f64 / engagements.len() as f64 * 100.0;
        let ships: Vec<serde_json::Value> = ordered_doctrine_ship_ids(cluster, &stats)
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
                    serde_json::json!({
                        "ship_type_id": tid,
                        "ship_name": resolve_type_name(&extra_names, *tid),
                        "canonical_fit": [],
                        "variants": [],
                        "occurrences": 0,
                        "pilot_count": 0,
                    })
                }
            })
            .collect();
        result.push(serde_json::json!({
            "ships": ships,
            "engagement_count": stats.engagement_count,
            "distinct_pilot_count": stats.distinct_pilots.len(),
            "coverage_pct": (coverage_pct * 10.0).round() / 10.0,
            "mean_similarity": (stats.mean_similarity * 1000.0).round() / 1000.0,
        }));
    }

    Ok(result)
}

fn jaccard_i32(a: &HashSet<i32>, b: &HashSet<i32>) -> f64 {
    if a.is_empty() && b.is_empty() {
        return 1.0;
    }
    let intersection = a.intersection(b).count();
    let union = a.union(b).count();
    if union == 0 {
        0.0
    } else {
        intersection as f64 / union as f64
    }
}

fn build_engagements(rows: &[AttackRow]) -> Vec<Engagement> {
    let mut kill_map: HashMap<i64, KillInfo> = HashMap::new();
    let mut unknown_seq = 0u64;

    for row in rows {
        let pilot_ref = match row.character_id {
            Some(character_id) => PilotRef::Known(character_id),
            None => {
                unknown_seq += 1;
                PilotRef::Unknown(unknown_seq)
            }
        };
        let entry = kill_map.entry(row.killmail_id).or_insert_with(|| KillInfo {
            time: row.kill_time,
            system_id: row.system_id,
            ship_pilots: HashMap::new(),
            pilot_ids: HashSet::new(),
        });
        entry.pilot_ids.insert(pilot_ref);
        entry
            .ship_pilots
            .entry(row.ship_type_id)
            .or_default()
            .insert(pilot_ref);
    }

    let mut kills_sorted: Vec<(i64, &KillInfo)> =
        kill_map.iter().map(|(id, info)| (*id, info)).collect();
    kills_sorted.sort_by(|a, b| {
        a.1.system_id
            .cmp(&b.1.system_id)
            .then(a.1.time.cmp(&b.1.time))
    });

    let engagement_window = chrono::Duration::minutes(ENGAGEMENT_WINDOW_MINUTES);
    let mut raw_engagements: Vec<KillInfoEngagement> = Vec::new();

    for (killmail_id, kill_info) in kills_sorted {
        let can_merge = raw_engagements.last().is_some_and(|engagement| {
            engagement.system_id == kill_info.system_id
                && kill_info.time - engagement.start_time <= engagement_window
        });

        if can_merge {
            let engagement = raw_engagements.last_mut().expect("last engagement exists");
            engagement.kill_ids.push(killmail_id);
            engagement.end_time = kill_info.time;
            engagement
                .pilot_ids
                .extend(kill_info.pilot_ids.iter().copied());
            for (&ship_type_id, pilots) in &kill_info.ship_pilots {
                engagement
                    .ship_pilots
                    .entry(ship_type_id)
                    .or_default()
                    .extend(pilots.iter().copied());
            }
            continue;
        }

        raw_engagements.push(KillInfoEngagement {
            kill_ids: vec![killmail_id],
            system_id: kill_info.system_id,
            start_time: kill_info.time,
            end_time: kill_info.time,
            ship_pilots: kill_info.ship_pilots.clone(),
            pilot_ids: kill_info.pilot_ids.clone(),
        });
    }

    raw_engagements
        .into_iter()
        .filter_map(finalize_engagement)
        .collect()
}

struct KillInfoEngagement {
    kill_ids: Vec<i64>,
    system_id: i32,
    start_time: chrono::DateTime<Utc>,
    end_time: chrono::DateTime<Utc>,
    ship_pilots: HashMap<i32, HashSet<PilotRef>>,
    pilot_ids: HashSet<PilotRef>,
}

fn finalize_engagement(raw: KillInfoEngagement) -> Option<Engagement> {
    if raw.pilot_ids.len() < MIN_ENGAGEMENT_PILOTS {
        return None;
    }

    let signature_threshold = ((raw.pilot_ids.len() as f64) * 0.1).ceil() as usize;
    let signature_threshold = signature_threshold.max(2);

    let ship_type_counts: HashMap<i32, usize> = raw
        .ship_pilots
        .iter()
        .map(|(&ship_type_id, pilots)| (ship_type_id, pilots.len()))
        .collect();
    let signature_ship_types: HashSet<i32> = ship_type_counts
        .iter()
        .filter(|(_, count)| **count >= signature_threshold)
        .map(|(&ship_type_id, _)| ship_type_id)
        .collect();

    if signature_ship_types.len() < 2 {
        return None;
    }

    Some(Engagement {
        kill_ids: raw.kill_ids,
        signature_ship_types,
        ship_type_counts,
        pilot_ids: raw.pilot_ids,
        start_time: raw.start_time,
    })
}

fn cluster_doctrines(engagements: &[Engagement]) -> Vec<DoctrineCluster> {
    let mut clusters: Vec<DoctrineCluster> = Vec::new();
    let mut indices: Vec<usize> = (0..engagements.len()).collect();
    indices.sort_by(|a, b| {
        engagements[*b]
            .pilot_count()
            .cmp(&engagements[*a].pilot_count())
            .then(engagements[*a].start_time.cmp(&engagements[*b].start_time))
    });

    for engagement_idx in indices {
        let signature = &engagements[engagement_idx].signature_ship_types;
        let mut best_cluster_idx: Option<usize> = None;
        let mut best_similarity = 0.0;

        for (cluster_idx, cluster) in clusters.iter().enumerate() {
            let stats = doctrine_cluster_stats(cluster, engagements);
            let similarity = jaccard_i32(signature, &stats.core_ship_types);
            if similarity >= CLUSTER_SIMILARITY_THRESHOLD && similarity > best_similarity {
                best_similarity = similarity;
                best_cluster_idx = Some(cluster_idx);
            }
        }

        if let Some(cluster_idx) = best_cluster_idx {
            clusters[cluster_idx]
                .engagement_indices
                .push(engagement_idx);
        } else {
            clusters.push(DoctrineCluster {
                engagement_indices: vec![engagement_idx],
                support_ship_types: HashSet::new(),
            });
        }
    }

    let mut qualified: Vec<DoctrineCluster> = clusters
        .into_iter()
        .filter(|cluster| doctrine_cluster_qualifies(cluster, engagements))
        .collect();
    qualified.sort_by(|a, b| compare_clusters(a, b, engagements));
    qualified
}

fn doctrine_cluster_qualifies(cluster: &DoctrineCluster, engagements: &[Engagement]) -> bool {
    let stats = doctrine_cluster_stats(cluster, engagements);
    stats.engagement_count >= MIN_DOCTRINE_ENGAGEMENTS
        && stats.distinct_pilots.len() >= MIN_DOCTRINE_DISTINCT_PILOTS
        && stats.core_ship_types.len() >= 2
}

fn doctrine_cluster_stats(
    cluster: &DoctrineCluster,
    engagements: &[Engagement],
) -> DoctrineClusterStats {
    let mut ship_presence: HashMap<i32, usize> = HashMap::new();
    let mut ship_weight: HashMap<i32, usize> = HashMap::new();
    let mut kill_ids: HashSet<i64> = HashSet::new();
    let mut distinct_pilots: HashSet<PilotRef> = HashSet::new();

    for &engagement_idx in &cluster.engagement_indices {
        let engagement = &engagements[engagement_idx];
        kill_ids.extend(engagement.kill_ids.iter().copied());
        distinct_pilots.extend(engagement.pilot_ids.iter().copied());
        for &ship_type_id in &engagement.signature_ship_types {
            *ship_presence.entry(ship_type_id).or_insert(0) += 1;
            *ship_weight.entry(ship_type_id).or_insert(0) += engagement
                .ship_type_counts
                .get(&ship_type_id)
                .copied()
                .unwrap_or(0);
        }
    }

    let engagement_count = cluster.engagement_indices.len();
    let threshold = ((engagement_count as f64) * DOCTRINE_CORE_PRESENCE_THRESHOLD).ceil() as usize;
    let threshold = threshold.max(1);
    let core_ship_types: HashSet<i32> = ship_presence
        .iter()
        .filter(|(_, count)| **count >= threshold)
        .map(|(&ship_type_id, _)| ship_type_id)
        .collect();
    let mean_similarity = if engagement_count == 0 || core_ship_types.is_empty() {
        0.0
    } else {
        cluster
            .engagement_indices
            .iter()
            .map(|&engagement_idx| {
                jaccard_i32(
                    &engagements[engagement_idx].signature_ship_types,
                    &core_ship_types,
                )
            })
            .sum::<f64>()
            / engagement_count as f64
    };

    DoctrineClusterStats {
        core_ship_types,
        ship_presence,
        ship_weight,
        kill_ids,
        distinct_pilots,
        engagement_count,
        mean_similarity,
    }
}

fn compare_clusters(
    a: &DoctrineCluster,
    b: &DoctrineCluster,
    engagements: &[Engagement],
) -> std::cmp::Ordering {
    let stats_a = doctrine_cluster_stats(a, engagements);
    let stats_b = doctrine_cluster_stats(b, engagements);
    stats_b
        .engagement_count
        .cmp(&stats_a.engagement_count)
        .then(
            stats_b
                .distinct_pilots
                .len()
                .cmp(&stats_a.distinct_pilots.len()),
        )
        .then_with(|| {
            stats_b
                .mean_similarity
                .partial_cmp(&stats_a.mean_similarity)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .then(
            stats_b
                .core_ship_types
                .len()
                .cmp(&stats_a.core_ship_types.len()),
        )
}

fn merge_close_doctrine_clusters(
    mut clusters: Vec<DoctrineCluster>,
    engagements: &[Engagement],
) -> Vec<DoctrineCluster> {
    let mut merged_any = true;
    while merged_any {
        merged_any = false;
        'outer: for i in 0..clusters.len() {
            for j in (i + 1)..clusters.len() {
                let a_stats = doctrine_cluster_stats(&clusters[i], engagements);
                let b_stats = doctrine_cluster_stats(&clusters[j], engagements);
                if should_merge_cluster_stats(&a_stats, &b_stats) {
                    let mut merged_indices: Vec<usize> = clusters[i]
                        .engagement_indices
                        .iter()
                        .chain(clusters[j].engagement_indices.iter())
                        .copied()
                        .collect();
                    merged_indices.sort_unstable();
                    merged_indices.dedup();

                    let mut merged_support = clusters[i].support_ship_types.clone();
                    merged_support.extend(clusters[j].support_ship_types.iter().copied());

                    clusters[i] = DoctrineCluster {
                        engagement_indices: merged_indices,
                        support_ship_types: merged_support,
                    };
                    clusters.remove(j);
                    merged_any = true;
                    break 'outer;
                }
            }
        }
    }

    let mut qualified: Vec<DoctrineCluster> = clusters
        .into_iter()
        .filter(|cluster| doctrine_cluster_qualifies(cluster, engagements))
        .collect();
    qualified.sort_by(|a, b| compare_clusters(a, b, engagements));
    qualified
}

fn should_merge_cluster_stats(a: &DoctrineClusterStats, b: &DoctrineClusterStats) -> bool {
    if a.core_ship_types.is_empty() || b.core_ship_types.is_empty() {
        return false;
    }
    let jaccard = jaccard_i32(&a.core_ship_types, &b.core_ship_types);
    let intersection = a.core_ship_types.intersection(&b.core_ship_types).count();
    let min_len = a.core_ship_types.len().min(b.core_ship_types.len());
    let overlap = if min_len == 0 {
        0.0
    } else {
        intersection as f64 / min_len as f64
    };
    jaccard >= POST_MERGE_JACCARD_THRESHOLD || overlap >= POST_MERGE_OVERLAP_THRESHOLD
}

fn assign_support_ships(
    clusters: &mut [DoctrineCluster],
    engagements: &[Engagement],
    support_ships: &[(i32, HashSet<i64>)],
) {
    for (support_type_id, nearby_kill_ids) in support_ships {
        let mut best_cluster_idx: Option<usize> = None;
        let mut best_presence = 0usize;
        let mut best_overlap = 0usize;

        for (cluster_idx, cluster) in clusters.iter().enumerate() {
            let stats = doctrine_cluster_stats(cluster, engagements);
            if stats.core_ship_types.contains(support_type_id) {
                best_cluster_idx = None;
                best_presence = 0;
                break;
            }

            let engagement_presence = cluster
                .engagement_indices
                .iter()
                .filter(|&&engagement_idx| {
                    engagements[engagement_idx]
                        .kill_ids
                        .iter()
                        .any(|killmail_id| nearby_kill_ids.contains(killmail_id))
                })
                .count();
            let overlap_kills = stats.kill_ids.intersection(nearby_kill_ids).count();

            if engagement_presence > best_presence
                || (engagement_presence == best_presence && overlap_kills > best_overlap)
            {
                best_presence = engagement_presence;
                best_overlap = overlap_kills;
                best_cluster_idx = Some(cluster_idx);
            }
        }

        let Some(cluster_idx) = best_cluster_idx else {
            continue;
        };

        let cluster_size = clusters[cluster_idx].engagement_indices.len();
        let threshold = ((cluster_size as f64) * SUPPORT_PRESENCE_THRESHOLD).ceil() as usize;
        let threshold = threshold.max(MIN_SUPPORT_ENGAGEMENTS);
        if best_presence >= threshold {
            clusters[cluster_idx]
                .support_ship_types
                .insert(*support_type_id);
        }
    }
}

fn ordered_doctrine_ship_ids<'a>(
    cluster: &'a DoctrineCluster,
    stats: &'a DoctrineClusterStats,
) -> Vec<&'a i32> {
    let mut core_ship_ids: Vec<&i32> = stats.core_ship_types.iter().collect();
    core_ship_ids.sort_by(|a, b| {
        stats.ship_presence[*b]
            .cmp(&stats.ship_presence[*a])
            .then(stats.ship_weight[*b].cmp(&stats.ship_weight[*a]))
            .then(a.cmp(b))
    });

    let mut support_ship_ids: Vec<&i32> = cluster.support_ship_types.iter().collect();
    support_ship_ids.sort();

    core_ship_ids.extend(support_ship_ids);
    core_ship_ids
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};

    fn ts(total_minutes: u32) -> chrono::DateTime<Utc> {
        let hour = total_minutes / 60;
        let minute = total_minutes % 60;
        Utc.with_ymd_and_hms(2026, 1, 1, hour, minute, 0)
            .single()
            .expect("valid timestamp")
    }

    fn attack_rows(
        killmail_id: i64,
        minute: u32,
        system_id: i32,
        ships: &[(i64, i32)],
    ) -> Vec<AttackRow> {
        ships
            .iter()
            .map(|(character_id, ship_type_id)| AttackRow {
                killmail_id,
                ship_type_id: *ship_type_id,
                kill_time: ts(minute),
                system_id,
                character_id: Some(*character_id),
            })
            .collect()
    }

    fn engagement(
        killmail_id: i64,
        minute: u32,
        system_id: i32,
        ships: &[(i64, i32)],
    ) -> Engagement {
        build_engagements(&attack_rows(killmail_id, minute, system_id, ships))
            .into_iter()
            .next()
            .expect("expected engagement")
    }

    #[test]
    fn repeated_small_gang_does_not_create_engagement() {
        let mut rows = Vec::new();
        rows.extend(attack_rows(
            1,
            0,
            30000142,
            &[(11, 1001), (12, 1002), (13, 1003), (14, 1004)],
        ));
        rows.extend(attack_rows(
            2,
            5,
            30000142,
            &[(11, 1001), (12, 1002), (13, 1003), (14, 1004)],
        ));

        let engagements = build_engagements(&rows);
        assert!(engagements.is_empty());
    }

    #[test]
    fn engagement_window_does_not_chain_indefinitely() {
        let mut rows = Vec::new();
        rows.extend(attack_rows(
            1,
            0,
            30000142,
            &[
                (11, 1001),
                (12, 1001),
                (13, 1002),
                (14, 1002),
                (15, 1003),
                (16, 1003),
            ],
        ));
        rows.extend(attack_rows(
            2,
            10,
            30000142,
            &[
                (21, 1001),
                (22, 1001),
                (23, 1002),
                (24, 1002),
                (25, 1003),
                (26, 1003),
            ],
        ));
        rows.extend(attack_rows(
            3,
            20,
            30000142,
            &[
                (31, 1001),
                (32, 1001),
                (33, 1002),
                (34, 1002),
                (35, 1003),
                (36, 1003),
            ],
        ));

        let engagements = build_engagements(&rows);
        assert_eq!(engagements.len(), 2);
        assert_eq!(engagements[0].kill_ids, vec![1, 2]);
        assert_eq!(engagements[1].kill_ids, vec![3]);
    }

    #[test]
    fn hull_swaps_merge_into_one_doctrine_cluster() {
        let engagements = vec![
            engagement(
                1,
                0,
                30000142,
                &[
                    (11, 1001),
                    (12, 1001),
                    (13, 1002),
                    (14, 1002),
                    (15, 1003),
                    (16, 1003),
                ],
            ),
            engagement(
                2,
                20,
                30000142,
                &[
                    (21, 1001),
                    (22, 1001),
                    (23, 1002),
                    (24, 1002),
                    (25, 1003),
                    (26, 1003),
                ],
            ),
            engagement(
                3,
                40,
                30000142,
                &[
                    (31, 1001),
                    (32, 1001),
                    (33, 1002),
                    (34, 1002),
                    (35, 1003),
                    (36, 1003),
                ],
            ),
            engagement(
                4,
                60,
                30000142,
                &[
                    (41, 1001),
                    (42, 1001),
                    (43, 1002),
                    (44, 1002),
                    (45, 1004),
                    (46, 1004),
                ],
            ),
            engagement(
                5,
                80,
                30000142,
                &[
                    (51, 1001),
                    (52, 1001),
                    (53, 1002),
                    (54, 1002),
                    (55, 1004),
                    (56, 1004),
                ],
            ),
            engagement(
                6,
                100,
                30000142,
                &[
                    (61, 1001),
                    (62, 1001),
                    (63, 1002),
                    (64, 1002),
                    (65, 1004),
                    (66, 1004),
                ],
            ),
        ];

        let clusters = merge_close_doctrine_clusters(cluster_doctrines(&engagements), &engagements);
        assert_eq!(clusters.len(), 1);

        let stats = doctrine_cluster_stats(&clusters[0], &engagements);
        assert_eq!(stats.engagement_count, 6);
        assert!(stats.core_ship_types.contains(&1001));
        assert!(stats.core_ship_types.contains(&1002));
        assert_eq!(stats.core_ship_types.len(), 2);
    }
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
        SELECT killmail_id, kill_time, character_id
        FROM killmail_victims
        WHERE {} = $1 AND ship_type_id = $2
          AND kill_time >= NOW() - $3 * INTERVAL '1 day'
        ORDER BY kill_time DESC
        LIMIT 200
        "#,
        column
    );
    let loss_killmails: Vec<(i64, chrono::DateTime<Utc>, Option<i64>)> = sqlx::query_as(&query)
        .bind(entity_id)
        .bind(ship_type_id)
        .bind(window_days)
        .fetch_all(pool)
        .await?;

    if loss_killmails.is_empty() {
        return Ok(None);
    }

    // Collect pilot IDs directly from the loss query (Issue 3: no separate fetch)
    let pilot_ids: HashSet<Option<i64>> = loss_killmails.iter().map(|(_, _, cid)| *cid).collect();

    // Batch fetch all killmail items (Issue 2: no per-killmail fetch)
    let km_keys: Vec<(i64, chrono::DateTime<Utc>)> =
        loss_killmails.iter().map(|(id, t, _)| (*id, *t)).collect();
    let items_map = nea_db::get_killmail_items_batch(pool, &km_keys).await?;

    let mut fittings: Vec<Vec<(i32, i32)>> = Vec::new();
    for (km_id, _, _) in &loss_killmails {
        if let Some(items) = items_map.get(km_id) {
            let fitted: Vec<(i32, i32)> = items
                .iter()
                .filter(|(_, flag)| is_fitted_slot(*flag))
                .cloned()
                .collect();
            if !fitted.is_empty() {
                fittings.push(fitted);
            }
        }
    }

    if fittings.is_empty() {
        return Ok(None);
    }

    let clusters = cluster_fittings(&fittings, 0.7);

    // Take the largest cluster that qualifies
    let best = clusters
        .into_iter()
        .filter(|c| c.count >= 3)
        .max_by_key(|c| c.count);

    let Some(cluster) = best else {
        return Ok(None);
    };

    // Batch resolve all type names (ship + all modules in canonical + variants)
    let mut all_type_ids: Vec<i32> = vec![ship_type_id];
    let canonical = &fittings[cluster.canonical_idx];
    for (type_id, _) in canonical {
        all_type_ids.push(*type_id);
    }
    for &idx in &cluster.member_indices {
        for (type_id, _) in &fittings[idx] {
            all_type_ids.push(*type_id);
        }
    }
    all_type_ids.sort_unstable();
    all_type_ids.dedup();
    let type_names = nea_db::get_type_names(pool, &all_type_ids).await?;

    let ship_name = resolve_type_name(&type_names, ship_type_id);

    let modules: Vec<serde_json::Value> = canonical
        .iter()
        .map(|(type_id, flag)| {
            serde_json::json!({
                "type_id": type_id,
                "name": resolve_type_name(&type_names, *type_id),
                "flag": flag,
            })
        })
        .collect();

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
            let variant_modules: Vec<serde_json::Value> = fit
                .iter()
                .map(|(type_id, flag)| {
                    serde_json::json!({
                        "type_id": type_id,
                        "name": resolve_type_name(&type_names, *type_id),
                        "flag": flag,
                    })
                })
                .collect();
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
            tracing::warn!(
                entity_id,
                "doctrine_aggregator: support ship kill events query failed: {e}"
            );
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
            tracing::warn!(
                entity_id,
                "doctrine_aggregator: support ship loss events query failed: {e}"
            );
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
                support_map.entry(*ship_type_id).or_default().extend(nearby);
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
    trends.sort_by(|a, b| {
        b.3.abs()
            .partial_cmp(&a.3.abs())
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    trends.truncate(20);

    let trend_type_ids: Vec<i32> = trends.iter().map(|(tid, _, _, _)| *tid).collect();
    let trend_names = nea_db::get_type_names(pool, &trend_type_ids).await?;

    let result: Vec<serde_json::Value> = trends
        .iter()
        .map(|(type_id, current_count, previous_count, change_pct)| {
            serde_json::json!({
                "type_id": type_id,
                "name": resolve_type_name(&trend_names, *type_id),
                "current_count": current_count,
                "previous_count": previous_count,
                "change_pct": (change_pct * 10.0).round() / 10.0,
            })
        })
        .collect();
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
            tracing::warn!(
                entity_id,
                "doctrine_aggregator: fleet comp query failed: {e}"
            );
            return None;
        }
        Err(_) => {
            tracing::warn!(
                entity_id,
                "doctrine_aggregator: fleet comp query timed out (>15s), skipping"
            );
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
        let mut unique: Vec<i32> = ships
            .iter()
            .copied()
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        if unique.len() < 2 {
            continue;
        }
        unique.sort_unstable();
        *comp_counts.entry(unique).or_insert(0) += 1;
    }

    // Filter to >=5 occurrences, take top 20
    let mut ranked: Vec<(Vec<i32>, u64)> = comp_counts
        .into_iter()
        .filter(|(_, count)| *count >= 2)
        .collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1));
    ranked.truncate(20);

    let all_comp_type_ids: Vec<i32> = ranked
        .iter()
        .flat_map(|(ids, _)| ids.iter())
        .copied()
        .collect();
    let comp_names = match nea_db::get_type_names(pool, &all_comp_type_ids).await {
        Ok(n) => n,
        Err(e) => {
            tracing::warn!(
                entity_id,
                "doctrine_aggregator: fleet comp name resolution failed: {e}"
            );
            return None;
        }
    };

    let comps: Vec<serde_json::Value> = ranked
        .iter()
        .map(|(ship_ids, count)| {
            let ships: Vec<serde_json::Value> = ship_ids
                .iter()
                .map(|type_id| {
                    serde_json::json!({"type_id": type_id, "name": resolve_type_name(&comp_names, *type_id)})
                })
                .collect();
            serde_json::json!({
                "ships": ships,
                "occurrence_count": count,
            })
        })
        .collect();
    Some(comps)
}
