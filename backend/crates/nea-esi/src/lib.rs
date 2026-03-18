// nea-esi: Client for the EVE Swagger Interface (ESI) API.

use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::Arc;
use std::time::Duration;

use reqwest::header::{HeaderMap, HeaderValue, USER_AGENT as USER_AGENT_HEADER};
use serde::Deserialize;
use thiserror::Error;
use tracing::{debug, warn};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

pub const BASE_URL: &str = "https://esi.evetech.net/latest";
pub const THE_FORGE: i32 = 10000002;
pub const JITA_STATION: i64 = 60003760;
pub const USER_AGENT: &str = "new-eden-analytics (sara@idknerdyshit.com; +https://github.com/idknerdyshit/new-eden-analytics; eve:Eyedeekay)";

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

#[derive(Debug, Error)]
pub enum EsiError {
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("API error (status {status}): {message}")]
    Api { status: u16, message: String },

    #[error("Rate limited – error budget exhausted")]
    RateLimited,

    #[error("Deserialization error: {0}")]
    Deserialize(String),

    #[error("Internal error: {0}")]
    Internal(String),
}

pub type Result<T> = std::result::Result<T, EsiError>;

// ---------------------------------------------------------------------------
// Response types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
pub struct EsiMarketHistoryEntry {
    pub date: String,
    pub average: f64,
    pub highest: f64,
    pub lowest: f64,
    pub volume: i64,
    pub order_count: i64,
}

// ---------------------------------------------------------------------------
// Killmail types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
pub struct EsiKillmail {
    pub killmail_id: i64,
    pub killmail_time: String,
    #[serde(default)]
    pub solar_system_id: i32,
    pub victim: EsiKillmailVictim,
    #[serde(default)]
    pub attackers: Vec<EsiKillmailAttacker>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EsiKillmailAttacker {
    #[serde(default)]
    pub character_id: Option<i64>,
    #[serde(default)]
    pub corporation_id: Option<i64>,
    #[serde(default)]
    pub alliance_id: Option<i64>,
    #[serde(default)]
    pub ship_type_id: i32,
    #[serde(default)]
    pub weapon_type_id: i32,
    #[serde(default)]
    pub damage_done: i32,
    #[serde(default)]
    pub final_blow: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EsiCharacterInfo {
    pub name: String,
    #[serde(default)]
    pub corporation_id: Option<i64>,
    #[serde(default)]
    pub alliance_id: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EsiKillmailVictim {
    #[serde(default)]
    pub ship_type_id: i32,
    #[serde(default)]
    pub character_id: Option<i64>,
    #[serde(default)]
    pub corporation_id: Option<i64>,
    #[serde(default)]
    pub alliance_id: Option<i64>,
    #[serde(default)]
    pub items: Vec<EsiKillmailItem>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EsiKillmailItem {
    pub item_type_id: i32,
    #[serde(default)]
    pub quantity_destroyed: Option<i64>,
    #[serde(default)]
    pub quantity_dropped: Option<i64>,
    #[serde(default)]
    pub flag: i32,
    #[serde(default)]
    pub singleton: i32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EsiMarketOrder {
    pub order_id: i64,
    pub type_id: i32,
    pub location_id: i64,
    pub price: f64,
    pub volume_remain: i64,
    pub is_buy_order: bool,
    pub issued: String,
    pub duration: i32,
    pub min_volume: i32,
    pub range: String,
}

// ---------------------------------------------------------------------------
// EsiClient
// ---------------------------------------------------------------------------

pub struct EsiClient {
    client: reqwest::Client,
    semaphore: Arc<tokio::sync::Semaphore>,
    error_budget: Arc<AtomicI32>,
}

impl EsiClient {
    /// Create a new ESI client with a User-Agent header and 30-second timeout.
    pub fn new() -> Self {
        let mut headers = HeaderMap::new();
        headers.insert(
            USER_AGENT_HEADER,
            HeaderValue::from_static(USER_AGENT),
        );

        let client = reqwest::Client::builder()
            .default_headers(headers)
            .timeout(Duration::from_secs(30))
            .build()
            .expect("failed to build reqwest client");

        Self {
            client,
            semaphore: Arc::new(tokio::sync::Semaphore::new(20)),
            error_budget: Arc::new(AtomicI32::new(100)),
        }
    }

    /// Return the current error budget value.
    pub fn error_budget(&self) -> i32 {
        self.error_budget.load(Ordering::Relaxed)
    }

    // -----------------------------------------------------------------------
    // Core request helper
    // -----------------------------------------------------------------------

    /// Make a rate-limited GET request to the given URL.
    ///
    /// Acquires a semaphore permit (max 20 concurrent), performs the request,
    /// reads the `X-ESI-Error-Limit-Remain` header to update the error budget,
    /// and returns the response.
    pub async fn request(&self, url: &str) -> Result<reqwest::Response> {
        let _permit = self
            .semaphore
            .acquire()
            .await
            .map_err(|_| EsiError::Internal("rate-limit semaphore closed".into()))?;

        // If the error budget is very low, back off briefly.
        let budget = self.error_budget.load(Ordering::Relaxed);
        if budget < 20 {
            warn!(
                budget,
                "ESI error budget low – adding 1 s delay before request"
            );
            tokio::time::sleep(Duration::from_secs(1)).await;
        }

        // If budget is zero we refuse to make the call.
        if self.error_budget.load(Ordering::Relaxed) <= 0 {
            return Err(EsiError::RateLimited);
        }

        let start = std::time::Instant::now();
        let response = self.client.get(url).send().await?;

        // Update error budget from response header.
        if let Some(val) = response.headers().get("x-esi-error-limit-remain") {
            if let Ok(s) = val.to_str() {
                if let Ok(remain) = s.parse::<i32>() {
                    self.error_budget.store(remain, Ordering::Relaxed);
                }
            }
        }

        // If the response is an error status, return an Api error.
        if !response.status().is_success() {
            let status = response.status().as_u16();
            let message = response.text().await.unwrap_or_default();
            warn!(url, status, "ESI API error");
            return Err(EsiError::Api { status, message });
        }

        debug!(
            url,
            status = response.status().as_u16(),
            elapsed_ms = start.elapsed().as_millis() as u64,
            error_budget = self.error_budget.load(Ordering::Relaxed),
            "ESI request"
        );

        Ok(response)
    }

    // -----------------------------------------------------------------------
    // Market endpoints
    // -----------------------------------------------------------------------

    /// Fetch market history for a type in a region.
    #[tracing::instrument(skip(self))]
    pub async fn market_history(
        &self,
        region_id: i32,
        type_id: i32,
    ) -> Result<Vec<EsiMarketHistoryEntry>> {
        let url = format!(
            "{}/markets/{}/history/?type_id={}",
            BASE_URL, region_id, type_id
        );
        let resp = self.request(&url).await?;
        let entries: Vec<EsiMarketHistoryEntry> = resp
            .json()
            .await
            .map_err(|e| EsiError::Deserialize(e.to_string()))?;
        debug!(entries = entries.len(), "market_history complete");
        Ok(entries)
    }

    /// Fetch all market orders for a type in a region, handling pagination.
    #[tracing::instrument(skip(self))]
    pub async fn market_orders(
        &self,
        region_id: i32,
        type_id: i32,
    ) -> Result<Vec<EsiMarketOrder>> {
        let base_url = format!(
            "{}/markets/{}/orders/?type_id={}&order_type=all",
            BASE_URL, region_id, type_id
        );

        // First request – also tells us how many pages there are.
        let first_url = format!("{}&page=1", base_url);
        let resp = self.request(&first_url).await?;

        let total_pages: i32 = resp
            .headers()
            .get("x-pages")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse().ok())
            .unwrap_or(1);

        let mut orders: Vec<EsiMarketOrder> = resp
            .json()
            .await
            .map_err(|e| EsiError::Deserialize(e.to_string()))?;

        if total_pages > 1 {
            // Fetch remaining pages concurrently.
            let mut handles = Vec::with_capacity((total_pages - 1) as usize);
            for page in 2..=total_pages {
                let url = format!("{}&page={}", base_url, page);
                let this = Self {
                    client: self.client.clone(),
                    semaphore: Arc::clone(&self.semaphore),
                    error_budget: Arc::clone(&self.error_budget),
                };
                handles.push(tokio::spawn(async move {
                    let resp = this.request(&url).await?;
                    let page_orders: Vec<EsiMarketOrder> = resp
                        .json()
                        .await
                        .map_err(|e| EsiError::Deserialize(e.to_string()))?;
                    Ok::<_, EsiError>(page_orders)
                }));
            }

            for handle in handles {
                let page_orders = handle
                    .await
                    .map_err(|e| EsiError::Deserialize(e.to_string()))??;
                orders.extend(page_orders);
            }
        }

        debug!(pages = total_pages, total_orders = orders.len(), "market_orders complete");
        Ok(orders)
    }

    // -----------------------------------------------------------------------
    // Killmail endpoint
    // -----------------------------------------------------------------------

    /// Fetch a single killmail by ID and hash, returning the raw JSON value.
    #[tracing::instrument(skip(self))]
    pub async fn get_killmail(
        &self,
        killmail_id: i64,
        killmail_hash: &str,
    ) -> Result<serde_json::Value> {
        let url = format!(
            "{}/killmails/{}/{}/",
            BASE_URL, killmail_id, killmail_hash
        );
        let resp = self.request(&url).await?;
        let value: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| EsiError::Deserialize(e.to_string()))?;
        debug!("get_killmail complete");
        Ok(value)
    }

    /// Fetch a single killmail by ID and hash, returning a typed struct.
    #[tracing::instrument(skip(self))]
    pub async fn get_killmail_typed(
        &self,
        killmail_id: i64,
        killmail_hash: &str,
    ) -> Result<EsiKillmail> {
        let url = format!(
            "{}/killmails/{}/{}/",
            BASE_URL, killmail_id, killmail_hash
        );
        let resp = self.request(&url).await?;
        let km: EsiKillmail = resp
            .json()
            .await
            .map_err(|e| EsiError::Deserialize(e.to_string()))?;
        debug!("get_killmail_typed complete");
        Ok(km)
    }

    // -----------------------------------------------------------------------
    // Character endpoint
    // -----------------------------------------------------------------------

    /// Fetch character info from ESI.
    #[tracing::instrument(skip(self))]
    pub async fn get_character(&self, character_id: i64) -> Result<EsiCharacterInfo> {
        let url = format!("{}/characters/{}/", BASE_URL, character_id);
        let resp = self.request(&url).await?;
        let info: EsiCharacterInfo = resp
            .json()
            .await
            .map_err(|e| EsiError::Deserialize(e.to_string()))?;
        debug!(character_id, name = %info.name, "get_character complete");
        Ok(info)
    }

    // -----------------------------------------------------------------------
    // Utility
    // -----------------------------------------------------------------------

    /// Given a slice of market orders, filter to a specific station and compute
    /// best bid, best ask, total bid volume, and total ask volume.
    ///
    /// Returns `(best_bid, best_ask, bid_volume, ask_volume)`.
    pub fn compute_best_bid_ask(
        orders: &[EsiMarketOrder],
        station_id: i64,
    ) -> (Option<f64>, Option<f64>, i64, i64) {
        let mut best_bid: Option<f64> = None;
        let mut best_ask: Option<f64> = None;
        let mut bid_volume: i64 = 0;
        let mut ask_volume: i64 = 0;

        for order in orders.iter().filter(|o| o.location_id == station_id) {
            if order.is_buy_order {
                bid_volume += order.volume_remain;
                best_bid = Some(match best_bid {
                    Some(current) => current.max(order.price),
                    None => order.price,
                });
            } else {
                ask_volume += order.volume_remain;
                best_ask = Some(match best_ask {
                    Some(current) => current.min(order.price),
                    None => order.price,
                });
            }
        }

        (best_bid, best_ask, bid_volume, ask_volume)
    }
}

impl Default for EsiClient {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_order(
        order_id: i64,
        location_id: i64,
        price: f64,
        volume_remain: i64,
        is_buy: bool,
    ) -> EsiMarketOrder {
        EsiMarketOrder {
            order_id,
            type_id: 34,
            location_id,
            price,
            volume_remain,
            is_buy_order: is_buy,
            issued: "2026-01-01T00:00:00Z".to_string(),
            duration: 90,
            min_volume: 1,
            range: "station".to_string(),
        }
    }

    #[test]
    fn test_compute_best_bid_ask_empty() {
        let (bid, ask, bv, av) = EsiClient::compute_best_bid_ask(&[], JITA_STATION);
        assert_eq!((bid, ask, bv, av), (None, None, 0, 0));
    }

    #[test]
    fn test_compute_best_bid_ask_wrong_station() {
        let orders = vec![make_order(1, 99999, 10.0, 100, true)];
        let (bid, ask, bv, av) = EsiClient::compute_best_bid_ask(&orders, JITA_STATION);
        assert_eq!((bid, ask, bv, av), (None, None, 0, 0));
    }

    #[test]
    fn test_compute_best_bid_ask_buys_only() {
        let orders = vec![
            make_order(1, JITA_STATION, 10.0, 100, true),
            make_order(2, JITA_STATION, 12.0, 200, true),
        ];
        let (bid, ask, bv, av) = EsiClient::compute_best_bid_ask(&orders, JITA_STATION);
        assert_eq!(bid, Some(12.0));
        assert_eq!(ask, None);
        assert_eq!(bv, 300);
        assert_eq!(av, 0);
    }

    #[test]
    fn test_compute_best_bid_ask_sells_only() {
        let orders = vec![
            make_order(1, JITA_STATION, 15.0, 50, false),
            make_order(2, JITA_STATION, 13.0, 75, false),
        ];
        let (bid, ask, bv, av) = EsiClient::compute_best_bid_ask(&orders, JITA_STATION);
        assert_eq!(bid, None);
        assert_eq!(ask, Some(13.0));
        assert_eq!(bv, 0);
        assert_eq!(av, 125);
    }

    #[test]
    fn test_compute_best_bid_ask_mixed() {
        let orders = vec![
            make_order(1, JITA_STATION, 10.0, 100, true),
            make_order(2, JITA_STATION, 12.0, 200, true),
            make_order(3, JITA_STATION, 15.0, 50, false),
            make_order(4, JITA_STATION, 13.0, 75, false),
        ];
        let (bid, ask, bv, av) = EsiClient::compute_best_bid_ask(&orders, JITA_STATION);
        assert_eq!(bid, Some(12.0));
        assert_eq!(ask, Some(13.0));
        assert_eq!(bv, 300);
        assert_eq!(av, 125);
    }

    #[test]
    fn test_compute_best_bid_ask_multi_station() {
        let amarr: i64 = 60008494;
        let orders = vec![
            make_order(1, JITA_STATION, 10.0, 100, true),
            make_order(2, amarr, 99.0, 999, true),
            make_order(3, JITA_STATION, 15.0, 50, false),
            make_order(4, amarr, 1.0, 999, false),
        ];
        let (bid, ask, bv, av) = EsiClient::compute_best_bid_ask(&orders, JITA_STATION);
        assert_eq!(bid, Some(10.0));
        assert_eq!(ask, Some(15.0));
        assert_eq!(bv, 100);
        assert_eq!(av, 50);
    }

    #[test]
    fn test_deserialize_esi_killmail() {
        let json = r#"{
            "killmail_id": 123456,
            "killmail_time": "2026-03-17T12:00:00Z",
            "solar_system_id": 30000142,
            "victim": {
                "ship_type_id": 587,
                "character_id": 91234567,
                "corporation_id": 98000001,
                "alliance_id": null,
                "items": [
                    {
                        "item_type_id": 2032,
                        "quantity_destroyed": 1,
                        "quantity_dropped": null,
                        "flag": 27,
                        "singleton": 0
                    },
                    {
                        "item_type_id": 3170,
                        "quantity_destroyed": null,
                        "quantity_dropped": 5,
                        "flag": 11,
                        "singleton": 0
                    }
                ]
            },
            "attackers": [
                {
                    "character_id": 95000001,
                    "corporation_id": 98000002,
                    "ship_type_id": 24690,
                    "weapon_type_id": 3170,
                    "damage_done": 5000,
                    "final_blow": true
                },
                {
                    "corporation_id": 1000125,
                    "ship_type_id": 0,
                    "weapon_type_id": 0,
                    "damage_done": 100,
                    "final_blow": false
                }
            ]
        }"#;

        let km: EsiKillmail = serde_json::from_str(json).unwrap();
        assert_eq!(km.killmail_id, 123456);
        assert_eq!(km.killmail_time, "2026-03-17T12:00:00Z");
        assert_eq!(km.solar_system_id, 30000142);
        assert_eq!(km.victim.ship_type_id, 587);
        assert_eq!(km.victim.character_id, Some(91234567));
        assert_eq!(km.victim.alliance_id, None);
        assert_eq!(km.victim.items.len(), 2);
        assert_eq!(km.victim.items[0].item_type_id, 2032);
        assert_eq!(km.victim.items[0].quantity_destroyed, Some(1));
        assert_eq!(km.victim.items[1].item_type_id, 3170);
        assert_eq!(km.victim.items[1].quantity_dropped, Some(5));
        assert_eq!(km.attackers.len(), 2);
        assert_eq!(km.attackers[0].character_id, Some(95000001));
        assert_eq!(km.attackers[0].ship_type_id, 24690);
        assert_eq!(km.attackers[0].damage_done, 5000);
        assert!(km.attackers[0].final_blow);
        assert_eq!(km.attackers[1].character_id, None);
        assert!(!km.attackers[1].final_blow);
    }

    #[test]
    fn test_deserialize_esi_killmail_minimal() {
        let json = r#"{
            "killmail_id": 999,
            "killmail_time": "2026-01-01T00:00:00Z",
            "solar_system_id": 30000001,
            "victim": {
                "ship_type_id": 670
            }
        }"#;

        let km: EsiKillmail = serde_json::from_str(json).unwrap();
        assert_eq!(km.killmail_id, 999);
        assert_eq!(km.victim.ship_type_id, 670);
        assert!(km.victim.items.is_empty());
        assert_eq!(km.victim.character_id, None);
    }

    #[test]
    fn test_deserialize_market_history_entry() {
        let json = r#"{"date":"2026-03-01","average":5.25,"highest":5.27,"lowest":5.11,"volume":72016862,"order_count":2267}"#;
        let entry: EsiMarketHistoryEntry = serde_json::from_str(json).unwrap();
        assert_eq!(entry.date, "2026-03-01");
        assert!((entry.average - 5.25).abs() < f64::EPSILON);
        assert_eq!(entry.volume, 72016862);
        assert_eq!(entry.order_count, 2267);
    }

    #[test]
    fn test_deserialize_market_order() {
        let json = r#"{"order_id":6789012345,"type_id":34,"location_id":60003760,"price":5.13,"volume_remain":250000,"is_buy_order":true,"issued":"2026-03-10T08:15:00Z","duration":90,"min_volume":1,"range":"station"}"#;
        let order: EsiMarketOrder = serde_json::from_str(json).unwrap();
        assert_eq!(order.order_id, 6789012345);
        assert_eq!(order.type_id, 34);
        assert!(order.is_buy_order);
        assert_eq!(order.location_id, JITA_STATION);
    }
}
