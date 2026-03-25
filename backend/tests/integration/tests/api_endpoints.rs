use axum::Router;
use chrono::NaiveDate;
use http::Request;
use http_body_util::BodyExt;
use sqlx::PgPool;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use tower::ServiceExt;

fn build_test_app(pool: PgPool) -> Router {
    let state = nea_server::state::AppState {
        pool,
        esi_client_id: "test-client-id".to_string(),
        esi_client_secret: "test-client-secret".to_string(),
        esi_callback_url: "http://localhost:3001/api/auth/callback".to_string(),
        session_secret: "test-session-secret".to_string(),
        analysis_running: Arc::new(AtomicBool::new(false)),
    };
    nea_server::routes::router(state)
}

fn date(y: i32, m: u32, d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, d).unwrap()
}

async fn insert_sde_type(pool: &PgPool, type_id: i32, name: &str) {
    sqlx::query("INSERT INTO sde_types (type_id, name) VALUES ($1, $2) ON CONFLICT DO NOTHING")
        .bind(type_id)
        .bind(name)
        .execute(pool)
        .await
        .unwrap();
}

async fn insert_sde_pair(pool: &PgPool, product_id: i32, material_id: i32) {
    let blueprint_id = product_id + 100_000;
    sqlx::query(
        "INSERT INTO sde_types (type_id, name) VALUES ($1, $2), ($3, $4), ($5, $6) ON CONFLICT DO NOTHING",
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

    sqlx::query(
        "INSERT INTO sde_blueprints (blueprint_type_id, product_type_id, quantity) VALUES ($1, $2, 1) ON CONFLICT DO NOTHING",
    )
    .bind(blueprint_id)
    .bind(product_id)
    .execute(pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO sde_blueprint_materials (blueprint_type_id, material_type_id, quantity) VALUES ($1, $2, 100) ON CONFLICT DO NOTHING",
    )
    .bind(blueprint_id)
    .bind(material_id)
    .execute(pool)
    .await
    .unwrap();
}

async fn insert_destruction(pool: &PgPool, type_id: i32, start: NaiveDate, days: i64) {
    for i in 0..days {
        let d = start + chrono::Duration::days(i);
        let qty = ((i as f64 * 0.15).sin() * 500.0 + 1000.0) as i64;
        sqlx::query(
            "INSERT INTO daily_destruction (type_id, date, quantity_destroyed, kill_count) VALUES ($1, $2, $3, $4) ON CONFLICT DO NOTHING",
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

async fn insert_market_history(
    pool: &PgPool,
    type_id: i32,
    region_id: i32,
    start: NaiveDate,
    days: i64,
) {
    for i in 0..days {
        let d = start + chrono::Duration::days(i);
        let avg = 500.0 + (i as f64 * 10.0);
        sqlx::query(
            "INSERT INTO market_history (type_id, region_id, date, average, highest, lowest, volume, order_count) VALUES ($1, $2, $3, $4, $5, $6, $7, $8) ON CONFLICT DO NOTHING",
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

async fn insert_market_snapshot(pool: &PgPool, type_id: i32, region_id: i32) {
    sqlx::query(
        "INSERT INTO market_snapshots (type_id, region_id, station_id, ts, best_bid, best_ask, bid_volume, ask_volume, spread) VALUES ($1, $2, $3, NOW(), $4, $5, $6, $7, $8)",
    )
    .bind(type_id)
    .bind(region_id)
    .bind(60003760i64)
    .bind(100.0)
    .bind(101.0)
    .bind(5000i64)
    .bind(3000i64)
    .bind(1.0)
    .execute(pool)
    .await
    .unwrap();
}

async fn insert_correlation(pool: &PgPool, product_id: i32, material_id: i32) {
    sqlx::query(
        r#"INSERT INTO correlation_results
           (product_type_id, material_type_id, lag_days, correlation_coeff,
            granger_f_stat, granger_p_value, granger_significant,
            window_start, window_end, computed_at)
           VALUES ($1, $2, 5, 0.85, 12.5, 0.001, true, '2024-01-01', '2024-06-01', NOW())
           ON CONFLICT (product_type_id, material_type_id) DO NOTHING"#,
    )
    .bind(product_id)
    .bind(material_id)
    .execute(pool)
    .await
    .unwrap();
}

async fn get_body(response: axum::response::Response) -> serde_json::Value {
    let body = response.into_body();
    let bytes = body.collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

// ── Tests ──────────────────────────────────────────────────────────

#[sqlx::test(migrations = "../../crates/nea-db/migrations")]
async fn test_items_search(pool: PgPool) {
    insert_sde_type(&pool, 587, "Rifter").await;
    insert_sde_type(&pool, 34, "Tritanium").await;

    let app = build_test_app(pool);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/items?q=Rifter")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), 200);
    let body = get_body(response).await;
    let items = body["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["name"], "Rifter");
    assert_eq!(items[0]["type_id"], 587);
}

#[sqlx::test(migrations = "../../crates/nea-db/migrations")]
async fn test_item_detail(pool: PgPool) {
    insert_sde_pair(&pool, 587, 34).await;

    let app = build_test_app(pool);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/items/587")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), 200);
    let body = get_body(response).await;
    assert_eq!(body["item"]["type_id"], 587);
    assert!(body["materials"].as_array().unwrap().len() > 0);
}

#[sqlx::test(migrations = "../../crates/nea-db/migrations")]
async fn test_item_not_found(pool: PgPool) {
    let app = build_test_app(pool);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/items/999999")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), 404);
}

#[sqlx::test(migrations = "../../crates/nea-db/migrations")]
async fn test_market_history(pool: PgPool) {
    insert_market_history(&pool, 34, 10000002, date(2024, 6, 1), 30).await;

    let app = build_test_app(pool);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/market/34/history?days=365")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), 200);
    let body = get_body(response).await;
    let rows = body.as_array().unwrap();
    assert_eq!(rows.len(), 30);
}

#[sqlx::test(migrations = "../../crates/nea-db/migrations")]
async fn test_market_snapshots(pool: PgPool) {
    insert_market_snapshot(&pool, 34, 10000002).await;

    let app = build_test_app(pool);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/market/34/snapshots?hours=24")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), 200);
    let body = get_body(response).await;
    let rows = body.as_array().unwrap();
    assert_eq!(rows.len(), 1);
}

#[sqlx::test(migrations = "../../crates/nea-db/migrations")]
async fn test_destruction(pool: PgPool) {
    insert_destruction(&pool, 587, date(2024, 6, 1), 10).await;

    let app = build_test_app(pool);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/destruction/587?days=365")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), 200);
    let body = get_body(response).await;
    let rows = body.as_array().unwrap();
    assert_eq!(rows.len(), 10);
}

#[sqlx::test(migrations = "../../crates/nea-db/migrations")]
async fn test_dashboard(pool: PgPool) {
    // Insert some correlations and destruction data
    insert_sde_pair(&pool, 587, 34).await;
    insert_correlation(&pool, 587, 34).await;

    let app = build_test_app(pool);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/dashboard")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), 200);
    let body = get_body(response).await;
    assert!(body["top_correlations"].is_array());
    assert!(body["top_destruction"].is_array());
    assert_eq!(body["top_correlations"].as_array().unwrap().len(), 1);
}

#[sqlx::test(migrations = "../../crates/nea-db/migrations")]
async fn test_dashboard_movers(pool: PgPool) {
    // Insert market history for 2 days for one type in The Forge
    insert_sde_type(&pool, 34, "Tritanium").await;
    let today = chrono::Utc::now().date_naive();
    let yesterday = today - chrono::Duration::days(1);

    sqlx::query(
        "INSERT INTO market_history (type_id, region_id, date, average, highest, lowest, volume, order_count) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
    )
    .bind(34)
    .bind(10000002)
    .bind(yesterday)
    .bind(100.0)
    .bind(110.0)
    .bind(90.0)
    .bind(1000i64)
    .bind(50)
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO market_history (type_id, region_id, date, average, highest, lowest, volume, order_count) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
    )
    .bind(34)
    .bind(10000002)
    .bind(today)
    .bind(120.0)
    .bind(130.0)
    .bind(110.0)
    .bind(1200i64)
    .bind(60)
    .execute(&pool)
    .await
    .unwrap();

    let app = build_test_app(pool);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/dashboard/movers")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), 200);
    let body = get_body(response).await;
    let movers = body.as_array().unwrap();
    assert_eq!(movers.len(), 1);
    assert_eq!(movers[0]["type_id"], 34);
    assert_eq!(movers[0]["name"], "Tritanium");
    // 120/100 = 20% change
    let change = movers[0]["change_pct"].as_f64().unwrap();
    assert!((change - 20.0).abs() < 0.01);
}

#[sqlx::test(migrations = "../../crates/nea-db/migrations")]
async fn test_analysis_correlations(pool: PgPool) {
    insert_sde_pair(&pool, 587, 34).await;
    insert_correlation(&pool, 587, 34).await;

    let app = build_test_app(pool);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/analysis/587/correlations")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), 200);
    let body = get_body(response).await;
    let rows = body.as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["product_type_id"], 587);
    assert_eq!(rows[0]["material_type_id"], 34);
}

#[sqlx::test(migrations = "../../crates/nea-db/migrations")]
async fn test_analysis_top(pool: PgPool) {
    insert_sde_pair(&pool, 587, 34).await;
    insert_correlation(&pool, 587, 34).await;

    let app = build_test_app(pool);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/analysis/top?limit=5")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), 200);
    let body = get_body(response).await;
    let rows = body.as_array().unwrap();
    assert!(rows.len() <= 5);
    assert_eq!(rows.len(), 1);
}

#[sqlx::test(migrations = "../../crates/nea-db/migrations")]
async fn test_auth_me_unauthenticated(pool: PgPool) {
    let app = build_test_app(pool);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/auth/me")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), 401);
    let body = get_body(response).await;
    assert_eq!(body["error"], "unauthorized");
}

#[sqlx::test(migrations = "../../crates/nea-db/migrations")]
async fn test_auth_me_valid_session(pool: PgPool) {
    // Insert user and session directly
    let character_id: i64 = 12345678;
    nea_db::upsert_user(
        &pool,
        character_id,
        "Test Pilot",
        b"fake-access-token",
        b"fake-refresh-token",
        chrono::Utc::now() + chrono::Duration::hours(1),
    )
    .await
    .unwrap();

    let session_id = nea_db::create_session(&pool, character_id).await.unwrap();

    let app = build_test_app(pool);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/auth/me")
                .header("cookie", format!("nea_session={session_id}"))
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), 200);
    let body = get_body(response).await;
    assert_eq!(body["character_id"], 12345678);
    assert_eq!(body["character_name"], "Test Pilot");
}

#[sqlx::test(migrations = "../../crates/nea-db/migrations")]
async fn test_request_id_header(pool: PgPool) {
    let app = build_test_app(pool);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/dashboard")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert!(response.headers().contains_key("x-request-id"));
    let request_id = response
        .headers()
        .get("x-request-id")
        .unwrap()
        .to_str()
        .unwrap();
    // Should be a valid UUID
    assert!(uuid::Uuid::parse_str(request_id).is_ok());
}
