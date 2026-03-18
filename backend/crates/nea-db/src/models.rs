use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ── SDE ──

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct SdeType {
    pub type_id: i32,
    pub name: String,
    pub group_id: Option<i32>,
    pub group_name: Option<String>,
    pub category_id: Option<i32>,
    pub category_name: Option<String>,
    pub market_group_id: Option<i32>,
    pub volume: Option<f64>,
    pub published: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct SdeBlueprint {
    pub blueprint_type_id: i32,
    pub product_type_id: i32,
    pub quantity: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct SdeBlueprintMaterial {
    pub blueprint_type_id: i32,
    pub material_type_id: i32,
    pub quantity: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct ProductMaterial {
    pub product_type_id: i32,
    pub product_name: String,
    pub material_type_id: i32,
    pub material_name: String,
    pub quantity: i32,
}

// ── Market ──

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct MarketHistory {
    pub type_id: i32,
    pub region_id: i32,
    pub date: NaiveDate,
    pub average: f64,
    pub highest: f64,
    pub lowest: f64,
    pub volume: i64,
    pub order_count: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct MarketSnapshot {
    pub type_id: i32,
    pub region_id: i32,
    pub station_id: Option<i64>,
    pub ts: DateTime<Utc>,
    pub best_bid: Option<f64>,
    pub best_ask: Option<f64>,
    pub bid_volume: Option<i64>,
    pub ask_volume: Option<i64>,
    pub spread: Option<f64>,
}

// ── Kills ──

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Killmail {
    pub killmail_id: i64,
    pub kill_time: DateTime<Utc>,
    pub solar_system_id: Option<i32>,
    pub total_value: Option<f64>,
    pub r2z2_sequence_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct KillmailItem {
    pub killmail_id: i64,
    pub kill_time: DateTime<Utc>,
    pub type_id: i32,
    pub quantity_destroyed: i64,
    pub quantity_dropped: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct KillmailVictim {
    pub killmail_id: i64,
    pub kill_time: DateTime<Utc>,
    pub ship_type_id: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct DailyDestruction {
    pub type_id: i32,
    #[serde(default)]
    pub type_name: Option<String>,
    pub date: NaiveDate,
    pub quantity_destroyed: i64,
    pub kill_count: i32,
}

// ── Dashboard ──

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Mover {
    pub type_id: i32,
    pub name: String,
    pub previous_avg: f64,
    pub current_avg: f64,
    pub change_pct: f64,
}

// ── Analysis ──

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct CorrelationResult {
    pub id: i32,
    pub product_type_id: i32,
    pub product_name: String,
    pub material_type_id: i32,
    pub material_name: String,
    pub lag_days: i32,
    pub correlation_coeff: f64,
    pub granger_f_stat: Option<f64>,
    pub granger_p_value: Option<f64>,
    pub granger_significant: bool,
    pub window_start: NaiveDate,
    pub window_end: NaiveDate,
    pub computed_at: DateTime<Utc>,
}

// ── Auth ──

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct User {
    pub character_id: i64,
    pub character_name: String,
    pub token_expires_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Session {
    pub session_id: Uuid,
    pub character_id: i64,
    pub expires_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}

// ── Worker ──

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct WorkerState {
    pub key: String,
    pub value: String,
    pub updated_at: DateTime<Utc>,
}
