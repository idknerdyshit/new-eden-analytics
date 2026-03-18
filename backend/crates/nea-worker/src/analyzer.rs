use chrono::Utc;
use sqlx::PgPool;
use std::time::Duration;
use tokio::time;

pub async fn run(pool: PgPool) {
    tracing::info!("analyzer task started");

    loop {
        let sleep_duration = duration_until_next_2am();
        tracing::info!(
            sleep_secs = sleep_duration.as_secs(),
            "analyzer: sleeping until next 02:00 UTC"
        );
        time::sleep(sleep_duration).await;

        tracing::info!("analyzer: starting correlation analysis");

        match nea_analysis::runner::run_analysis(&pool, nea_esi::THE_FORGE).await {
            Ok(stats) => {
                tracing::info!(
                    pairs_analyzed = stats.pairs_analyzed,
                    significant_correlations = stats.significant,
                    "analyzer: cycle complete"
                );
            }
            Err(e) => {
                tracing::error!("analyzer: failed: {e}");
            }
        }
    }
}

fn duration_until_next_2am() -> Duration {
    let now = Utc::now();
    let today_2am = now
        .date_naive()
        .and_hms_opt(2, 0, 0)
        .expect("02:00:00 is always a valid time")
        .and_utc();

    let next_2am = if now < today_2am {
        today_2am
    } else {
        today_2am + chrono::Duration::days(1)
    };

    let diff = next_2am - now;
    Duration::from_secs(diff.num_seconds().max(0) as u64)
}
