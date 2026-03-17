use chrono::{NaiveDate, Utc};
use nea_db::CorrelationResult;
use nea_esi::THE_FORGE;
use sqlx::PgPool;
use std::time::Duration;
use tokio::time;

pub async fn run(pool: PgPool) {
    tracing::info!("analyzer task started");

    loop {
        // Calculate sleep duration until next 02:00 UTC
        let sleep_duration = duration_until_next_2am();
        tracing::info!(
            sleep_secs = sleep_duration.as_secs(),
            "analyzer: sleeping until next 02:00 UTC"
        );
        time::sleep(sleep_duration).await;

        tracing::info!("analyzer: starting correlation analysis");

        match run_analysis(&pool).await {
            Ok((analyzed, significant)) => {
                tracing::info!(
                    pairs_analyzed = analyzed,
                    significant_correlations = significant,
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
        .unwrap()
        .and_utc();

    let next_2am = if now < today_2am {
        today_2am
    } else {
        today_2am + chrono::Duration::days(1)
    };

    let diff = next_2am - now;
    Duration::from_secs(diff.num_seconds().max(0) as u64)
}

async fn run_analysis(
    pool: &PgPool,
) -> Result<(u64, u64), Box<dyn std::error::Error + Send + Sync>> {
    let pairs = nea_db::get_all_product_material_pairs(pool).await?;
    tracing::info!(
        pair_count = pairs.len(),
        "analyzer: loaded product-material pairs"
    );

    let end = Utc::now().date_naive();
    let start = end - chrono::Duration::days(180);

    let mut analyzed = 0u64;
    let mut significant = 0u64;

    for (product_type_id, material_type_id) in &pairs {
        match analyze_pair(pool, *product_type_id, *material_type_id, start, end).await {
            Ok(Some(is_significant)) => {
                analyzed += 1;
                if is_significant {
                    significant += 1;
                }
            }
            Ok(None) => {
                // Not enough data, skip
                tracing::debug!(
                    product_type_id,
                    material_type_id,
                    "analyzer: insufficient data for pair, skipping"
                );
            }
            Err(e) => {
                tracing::warn!(
                    product_type_id,
                    material_type_id,
                    "analyzer: error analyzing pair: {e}"
                );
            }
        }
    }

    Ok((analyzed, significant))
}

async fn analyze_pair(
    pool: &PgPool,
    product_type_id: i32,
    material_type_id: i32,
    start: NaiveDate,
    end: NaiveDate,
) -> Result<Option<bool>, Box<dyn std::error::Error + Send + Sync>> {
    let destruction_series =
        nea_db::get_destruction_series(pool, product_type_id, start, end).await?;
    let price_series =
        nea_db::get_price_series(pool, material_type_id, THE_FORGE, start, end).await?;

    let result = match nea_analysis::analyze(&destruction_series, &price_series) {
        Some(r) => r,
        None => return Ok(None),
    };

    let correlation_result = CorrelationResult {
        id: 0, // ignored on upsert
        product_type_id,
        material_type_id,
        lag_days: result.optimal_lag.lag,
        correlation_coeff: result.optimal_lag.correlation,
        granger_f_stat: Some(result.granger.f_statistic),
        granger_p_value: Some(result.granger.p_value),
        granger_significant: result.granger.significant,
        window_start: start,
        window_end: end,
        computed_at: Utc::now(),
    };

    nea_db::upsert_correlation(pool, &correlation_result).await?;

    Ok(Some(result.granger.significant))
}
