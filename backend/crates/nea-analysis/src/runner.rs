use chrono::{NaiveDate, Utc};
use nea_db::CorrelationResult;
use serde::Serialize;
use sqlx::PgPool;

#[derive(Debug, Clone, Serialize)]
pub struct AnalysisStats {
    pub pairs_analyzed: u64,
    pub significant: u64,
}

pub async fn run_analysis(
    pool: &PgPool,
    region_id: i32,
) -> Result<AnalysisStats, Box<dyn std::error::Error + Send + Sync>> {
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
        match analyze_pair(
            pool,
            *product_type_id,
            *material_type_id,
            region_id,
            start,
            end,
        )
        .await
        {
            Ok(Some(is_significant)) => {
                analyzed += 1;
                if is_significant {
                    significant += 1;
                }
            }
            Ok(None) => {
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

    Ok(AnalysisStats {
        pairs_analyzed: analyzed,
        significant,
    })
}

async fn analyze_pair(
    pool: &PgPool,
    product_type_id: i32,
    material_type_id: i32,
    region_id: i32,
    start: NaiveDate,
    end: NaiveDate,
) -> Result<Option<bool>, Box<dyn std::error::Error + Send + Sync>> {
    let destruction_series =
        nea_db::get_destruction_series(pool, product_type_id, start, end).await?;
    let price_series =
        nea_db::get_price_series(pool, material_type_id, region_id, start, end).await?;

    let result = match crate::analyze(&destruction_series, &price_series) {
        Some(r) => r,
        None => return Ok(None),
    };

    let correlation_result = CorrelationResult {
        id: 0,
        product_type_id,
        product_name: String::new(),
        material_type_id,
        material_name: String::new(),
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
