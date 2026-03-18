use chrono::NaiveDate;
use sqlx::PgPool;

const TEST_REGION_ID: i32 = 10000002; // The Forge

fn date(y: i32, m: u32, d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, d).unwrap()
}

/// Insert the minimal SDE data needed: two types, a blueprint linking them, and
/// a blueprint_materials row so `v_product_materials` returns the pair.
async fn insert_sde_pair(pool: &PgPool, product_id: i32, material_id: i32) {
    let blueprint_id = product_id + 100_000;

    // sde_types: product, material, blueprint
    sqlx::query(
        "INSERT INTO sde_types (type_id, name) VALUES ($1, $2), ($3, $4), ($5, $6)
         ON CONFLICT DO NOTHING",
    )
    .bind(product_id)
    .bind(format!("TestProduct{product_id}"))
    .bind(material_id)
    .bind(format!("TestMaterial{material_id}"))
    .bind(blueprint_id)
    .bind(format!("TestBP{blueprint_id}"))
    .execute(pool)
    .await
    .unwrap();

    // sde_blueprints
    sqlx::query(
        "INSERT INTO sde_blueprints (blueprint_type_id, product_type_id, quantity) VALUES ($1, $2, 1)
         ON CONFLICT DO NOTHING",
    )
    .bind(blueprint_id)
    .bind(product_id)
    .execute(pool)
    .await
    .unwrap();

    // sde_blueprint_materials
    sqlx::query(
        "INSERT INTO sde_blueprint_materials (blueprint_type_id, material_type_id, quantity) VALUES ($1, $2, 100)
         ON CONFLICT DO NOTHING",
    )
    .bind(blueprint_id)
    .bind(material_id)
    .execute(pool)
    .await
    .unwrap();
}

/// Insert N days of daily_destruction with a sinusoidal pattern.
async fn insert_destruction(pool: &PgPool, type_id: i32, start: NaiveDate, days: i64) {
    for i in 0..days {
        let d = start + chrono::Duration::days(i);
        let qty = ((i as f64 * 0.15).sin() * 500.0 + 1000.0) as i64;
        sqlx::query(
            "INSERT INTO daily_destruction (type_id, date, quantity_destroyed, kill_count)
             VALUES ($1, $2, $3, $4)
             ON CONFLICT DO NOTHING",
        )
        .bind(type_id)
        .bind(d)
        .bind(qty)
        .bind(10)
        .execute(pool)
        .await
        .unwrap();
    }
}

/// Insert N days of market_history with a lagged-correlated price pattern.
/// The price is correlated with destruction shifted by `lag_days`.
async fn insert_market_history(
    pool: &PgPool,
    type_id: i32,
    region_id: i32,
    start: NaiveDate,
    days: i64,
    lag_days: i64,
) {
    for i in 0..days {
        let d = start + chrono::Duration::days(i);
        // Price follows the same sinusoidal as destruction, but shifted by lag_days
        let lagged_i = (i - lag_days).max(0);
        let avg = (lagged_i as f64 * 0.15).sin() * 50.0 + 500.0;
        sqlx::query(
            "INSERT INTO market_history (type_id, region_id, date, average, highest, lowest, volume, order_count)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
             ON CONFLICT DO NOTHING",
        )
        .bind(type_id)
        .bind(region_id)
        .bind(d)
        .bind(avg)
        .bind(avg * 1.1)
        .bind(avg * 0.9)
        .bind(1000i64)
        .bind(50)
        .execute(pool)
        .await
        .unwrap();
    }
}

#[sqlx::test(migrations = "../../crates/nea-db/migrations")]
async fn test_analysis_pipeline_with_correlated_data(pool: PgPool) {
    let product_id = 587; // arbitrary
    let material_id = 34; // arbitrary (Tritanium-ish)
    let start = date(2024, 1, 1);
    let days = 100i64;
    let lag = 5i64;

    insert_sde_pair(&pool, product_id, material_id).await;
    insert_destruction(&pool, product_id, start, days).await;
    insert_market_history(&pool, material_id, TEST_REGION_ID, start, days, lag).await;

    let stats = nea_analysis::runner::run_analysis(&pool, TEST_REGION_ID)
        .await
        .expect("run_analysis should succeed");

    assert_eq!(stats.pairs_analyzed, 1, "should analyze exactly one pair");

    // Check that a correlation result was inserted
    let rows = nea_db::get_correlations_for_product(&pool, product_id)
        .await
        .expect("query should succeed");

    assert_eq!(rows.len(), 1, "should have one correlation result");
    let row = &rows[0];
    assert_eq!(row.product_type_id, product_id);
    assert_eq!(row.material_type_id, material_id);
    // The lag should be in a reasonable range (the exact value depends on the analysis)
    assert!(
        row.lag_days.abs() <= 30,
        "lag_days should be within max_lag range"
    );
    assert!(
        row.correlation_coeff.abs() > 0.0,
        "correlation should be nonzero"
    );
    assert!(
        row.granger_f_stat.unwrap_or(0.0).is_finite(),
        "f_stat should be finite"
    );
}

#[sqlx::test(migrations = "../../crates/nea-db/migrations")]
async fn test_analysis_pipeline_insufficient_data(pool: PgPool) {
    let product_id = 588;
    let material_id = 35;
    let start = date(2024, 1, 1);
    let days = 30i64; // too few — needs 60+ after differencing

    insert_sde_pair(&pool, product_id, material_id).await;
    insert_destruction(&pool, product_id, start, days).await;
    insert_market_history(&pool, material_id, TEST_REGION_ID, start, days, 3).await;

    let stats = nea_analysis::runner::run_analysis(&pool, TEST_REGION_ID)
        .await
        .expect("run_analysis should succeed");

    // With only 30 days of data, the pair should be skipped (insufficient data)
    assert_eq!(
        stats.pairs_analyzed, 0,
        "should not have analyzed any pairs (insufficient data)"
    );

    // Verify no correlation result was inserted
    let rows = nea_db::get_correlations_for_product(&pool, product_id)
        .await
        .expect("query should succeed");
    assert!(
        rows.is_empty(),
        "should have no correlation results for insufficient data"
    );
}
