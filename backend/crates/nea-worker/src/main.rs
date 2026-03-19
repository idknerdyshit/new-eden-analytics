mod aggregation;
mod analyzer;
mod doctrine_aggregator;
mod fitting_utils;
mod killmail_poller;
mod market_history;
mod market_orders;
mod profile_aggregator;

use std::sync::Arc;

use nea_esi::EsiClient;
use nea_zkill::R2z2Client;
use tokio::signal;

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let args: Vec<String> = std::env::args().collect();
    let run_once = args.get(1).map(|s| s.as_str()) == Some("--run-once");
    let run_once_task = args.get(2).map(|s| s.as_str());

    if run_once {
        match run_once_task {
            Some("analyzer") => {
                tracing::info!("nea-worker: running analyzer once");

                let database_url =
                    std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
                let pool = nea_db::create_pool(&database_url)
                    .await
                    .expect("failed to create database pool");
                nea_db::run_migrations(&pool)
                    .await
                    .expect("failed to run database migrations");

                match nea_analysis::runner::run_analysis(&pool, nea_esi::THE_FORGE).await {
                    Ok(stats) => {
                        tracing::info!(
                            pairs_analyzed = stats.pairs_analyzed,
                            significant = stats.significant,
                            "analyzer: cycle complete"
                        );
                    }
                    Err(e) => {
                        tracing::error!("analyzer: failed: {e}");
                        std::process::exit(1);
                    }
                }
                return;
            }
            Some(other) => {
                tracing::error!("unknown task for --run-once: {other}");
                std::process::exit(1);
            }
            None => {
                tracing::error!("--run-once requires a task name (e.g., 'analyzer')");
                std::process::exit(1);
            }
        }
    }

    tracing::info!("nea-worker starting");

    // Database pool
    let database_url =
        std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let pool = nea_db::create_pool(&database_url)
        .await
        .expect("failed to create database pool");
    tracing::info!("database pool created");

    // Run migrations
    nea_db::run_migrations(&pool)
        .await
        .expect("failed to run database migrations");
    tracing::info!("database migrations complete");

    // Clients
    let esi = Arc::new(EsiClient::new());
    let r2z2 = Arc::new(R2z2Client::new());

    tracing::info!("spawning worker tasks");

    // Spawn all worker tasks
    let pool_mh = pool.clone();
    let esi_mh = Arc::clone(&esi);
    let market_history_handle =
        tokio::spawn(async move { market_history::run(pool_mh, esi_mh).await });

    let pool_mo = pool.clone();
    let esi_mo = Arc::clone(&esi);
    let market_orders_handle =
        tokio::spawn(async move { market_orders::run(pool_mo, esi_mo).await });

    let pool_kp = pool.clone();
    let r2z2_kp = Arc::clone(&r2z2);
    let killmail_poller_handle =
        tokio::spawn(async move { killmail_poller::run(pool_kp, r2z2_kp).await });

    let pool_agg = pool.clone();
    let aggregation_handle =
        tokio::spawn(async move { aggregation::run(pool_agg).await });

    let pool_an = pool.clone();
    let analyzer_handle =
        tokio::spawn(async move { analyzer::run(pool_an).await });

    let pool_pa = pool.clone();
    let esi_pa = Arc::clone(&esi);
    let profile_aggregator_handle =
        tokio::spawn(async move { profile_aggregator::run(pool_pa, esi_pa).await });

    let pool_da = pool.clone();
    let esi_da = Arc::clone(&esi);
    let doctrine_aggregator_handle =
        tokio::spawn(async move { doctrine_aggregator::run(pool_da, esi_da).await });

    let pool_sc = pool.clone();
    let session_cleanup_handle = tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(3600));
        loop {
            interval.tick().await;
            match nea_db::cleanup_expired_sessions(&pool_sc).await {
                Ok(count) => {
                    if count > 0 {
                        tracing::info!(deleted = count, "session_cleanup: removed expired sessions");
                    }
                }
                Err(e) => {
                    tracing::error!("session_cleanup: failed: {e}");
                }
            }
        }
    });

    tracing::info!("all worker tasks spawned, waiting for shutdown signal");

    // Wait for Ctrl+C or any task to finish (which would indicate an unexpected exit)
    tokio::select! {
        _ = signal::ctrl_c() => {
            tracing::info!("received Ctrl+C, shutting down");
        }
        result = market_history_handle => {
            tracing::error!(?result, "market_history task exited unexpectedly");
        }
        result = market_orders_handle => {
            tracing::error!(?result, "market_orders task exited unexpectedly");
        }
        result = killmail_poller_handle => {
            tracing::error!(?result, "killmail_poller task exited unexpectedly");
        }
        result = aggregation_handle => {
            tracing::error!(?result, "aggregation task exited unexpectedly");
        }
        result = analyzer_handle => {
            tracing::error!(?result, "analyzer task exited unexpectedly");
        }
        result = profile_aggregator_handle => {
            tracing::error!(?result, "profile_aggregator task exited unexpectedly");
        }
        result = doctrine_aggregator_handle => {
            tracing::error!(?result, "doctrine_aggregator task exited unexpectedly");
        }
        result = session_cleanup_handle => {
            tracing::error!(?result, "session_cleanup task exited unexpectedly");
        }
    }

    tracing::info!("nea-worker shutting down");
}
