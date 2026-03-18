// nea-zkill: Client for the zKillboard R2Z2 API (RedisQ replacement).

use chrono::{DateTime, Utc};
use serde::Deserialize;
use thiserror::Error;

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

#[derive(Debug, Error)]
pub enum ZkillError {
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("API error (status {status}): {message}")]
    Api { status: u16, message: String },

    #[error("deserialization error: {0}")]
    Deserialize(String),

    #[error("resource not found")]
    NotFound,
}

pub type Result<T> = std::result::Result<T, ZkillError>;

// ---------------------------------------------------------------------------
// Response types
// ---------------------------------------------------------------------------

/// R2Z2 ephemeral killmail — ESI-format killmail with zkb metadata block.
#[derive(Debug, Clone, Deserialize)]
pub struct R2z2Response {
    pub killmail_id: i64,
    #[serde(default)]
    pub killmail_time: Option<String>,
    #[serde(default)]
    pub solar_system_id: i32,
    pub victim: ZkillVictim,
    pub zkb: ZkillZkb,
    #[serde(default)]
    pub sequence_id: Option<i64>,
    #[serde(default)]
    pub uploaded_at: Option<i64>,
}

/// Response from the R2Z2 `/ephemeral/sequence.json` endpoint.
#[derive(Debug, Clone, Deserialize)]
pub struct R2z2SequenceResponse {
    pub sequence: i64,
}

// ---------------------------------------------------------------------------
// zKillboard kills API types (ESI-format killmail + zkb metadata)
// ---------------------------------------------------------------------------

/// A single killmail from the zKillboard kills API.
/// Items are nested under `victim` (ESI format), and zkb metadata is separate.
#[derive(Debug, Clone, Deserialize)]
pub struct ZkillKillmail {
    pub killmail_id: i64,
    pub killmail_time: String,
    pub solar_system_id: i32,
    pub victim: ZkillVictim,
    pub zkb: ZkillZkb,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ZkillVictim {
    pub ship_type_id: i32,
    #[serde(default)]
    pub items: Vec<ZkillItem>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ZkillItem {
    pub item_type_id: i32,
    #[serde(default)]
    pub quantity_destroyed: Option<i64>,
    #[serde(default)]
    pub quantity_dropped: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ZkillZkb {
    pub hash: String,
    #[serde(rename = "totalValue")]
    pub total_value: f64,
}

// ---------------------------------------------------------------------------
// Utilities
// ---------------------------------------------------------------------------

/// Parse a killmail timestamp string into a `DateTime<Utc>`.
///
/// Handles both ISO 8601 with Z suffix and bare datetime strings.
pub fn parse_killmail_time(time_str: &str) -> DateTime<Utc> {
    if let Ok(dt) = time_str.parse::<DateTime<Utc>>() {
        return dt;
    }
    if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(time_str, "%Y-%m-%dT%H:%M:%S") {
        return dt.and_utc();
    }
    tracing::warn!(time_str, "failed to parse killmail_time, using now()");
    Utc::now()
}

// ---------------------------------------------------------------------------
// Client
// ---------------------------------------------------------------------------

pub struct R2z2Client {
    client: reqwest::Client,
    base_url: String,
}

impl R2z2Client {
    /// Create a new R2Z2 client with sensible defaults.
    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            .user_agent("new-eden-analytics (sara@idknerdyshit.com; +https://github.com/idknerdyshit/new-eden-analytics; eve:Eyedeekay)")
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .expect("failed to build reqwest client");

        Self {
            client,
            base_url: "https://r2z2.zkillboard.com".to_string(),
        }
    }

    /// Fetch a single killmail by its R2Z2 sequence ID.
    ///
    /// Returns `Ok(None)` when the server responds with 404 (no new data yet),
    /// `Ok(Some(response))` on 200, and `Err` for anything else.
    pub async fn fetch_sequence(&self, sequence_id: i64) -> Result<Option<R2z2Response>> {
        let url = format!("{}/ephemeral/{}.json", self.base_url, sequence_id);
        tracing::debug!(url = %url, "fetching R2Z2 sequence");

        let response = self.client.get(&url).send().await?;
        let status = response.status();

        if status == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }

        if !status.is_success() {
            let message = response
                .text()
                .await
                .unwrap_or_else(|_| "unable to read response body".into());
            return Err(ZkillError::Api {
                status: status.as_u16(),
                message,
            });
        }

        let body = response.text().await?;
        let parsed: R2z2Response =
            serde_json::from_str(&body).map_err(|e| ZkillError::Deserialize(e.to_string()))?;

        Ok(Some(parsed))
    }

    /// Fetch the current R2Z2 sequence ID from `/ephemeral/sequence.json`.
    pub async fn fetch_current_sequence(&self) -> Result<i64> {
        let url = format!("{}/ephemeral/sequence.json", self.base_url);
        tracing::debug!(url = %url, "fetching current R2Z2 sequence");

        let response = self.client.get(&url).send().await?;
        let status = response.status();

        if !status.is_success() {
            let message = response
                .text()
                .await
                .unwrap_or_else(|_| "unable to read response body".into());
            return Err(ZkillError::Api {
                status: status.as_u16(),
                message,
            });
        }

        let body = response.text().await?;
        let parsed: R2z2SequenceResponse =
            serde_json::from_str(&body).map_err(|e| ZkillError::Deserialize(e.to_string()))?;

        Ok(parsed.sequence)
    }

    /// Fetch the list of killmail IDs and hashes for a given date (format: `YYYYMMDD`).
    ///
    /// Uses the zKillboard history API for backfill purposes.
    /// Returns `(killmail_id, killmail_hash)` pairs sorted by ID.
    pub async fn fetch_history(&self, date: &str) -> Result<Vec<(i64, String)>> {
        // History lives at the R2Z2 root, not under /v1.
        let url = format!("https://r2z2.zkillboard.com/history/{}.json", date);
        tracing::debug!(url = %url, "fetching zKillboard history");

        let response = self.client.get(&url).send().await?;
        let status = response.status();

        if status == reqwest::StatusCode::NOT_FOUND {
            return Err(ZkillError::NotFound);
        }

        if !status.is_success() {
            let message = response
                .text()
                .await
                .unwrap_or_else(|_| "unable to read response body".into());
            return Err(ZkillError::Api {
                status: status.as_u16(),
                message,
            });
        }

        // The history endpoint returns a JSON object mapping killmail IDs (as
        // string keys) to killmail hashes, e.g. {"12345": "abc...", ...}.
        let body = response.text().await?;
        let map: std::collections::HashMap<String, String> =
            serde_json::from_str(&body).map_err(|e| ZkillError::Deserialize(e.to_string()))?;

        let mut pairs: Vec<(i64, String)> = map
            .into_iter()
            .filter_map(|(k, v)| k.parse::<i64>().ok().map(|id| (id, v)))
            .collect();
        pairs.sort_unstable_by_key(|(id, _)| *id);

        Ok(pairs)
    }

}

impl Default for R2z2Client {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserialize_response() {
        let json = r#"{
            "killmail_id": 123456,
            "killmail_time": "2026-03-17T12:00:00Z",
            "solar_system_id": 30000142,
            "victim": {
                "ship_type_id": 587,
                "items": [
                    {
                        "item_type_id": 2032,
                        "quantity_destroyed": 1,
                        "quantity_dropped": null
                    }
                ]
            },
            "zkb": {
                "hash": "abcdef1234567890",
                "totalValue": 1500000.50
            },
            "sequence_id": 42,
            "uploaded_at": 1710676800
        }"#;

        let resp: R2z2Response = serde_json::from_str(json).unwrap();
        assert_eq!(resp.killmail_id, 123456);
        assert_eq!(resp.zkb.hash, "abcdef1234567890");
        assert_eq!(resp.zkb.total_value, 1500000.50);
        assert_eq!(resp.victim.ship_type_id, 587);
        assert_eq!(resp.victim.items.len(), 1);
        assert_eq!(resp.victim.items[0].item_type_id, 2032);
        assert_eq!(resp.victim.items[0].quantity_destroyed, Some(1));
        assert_eq!(resp.sequence_id, Some(42));
    }

    #[test]
    fn deserialize_response_missing_optional_fields() {
        let json = r#"{
            "killmail_id": 999,
            "killmail_time": "2026-03-17T00:00:00Z",
            "victim": {
                "ship_type_id": 0,
                "items": []
            },
            "zkb": {
                "hash": "deadbeef",
                "totalValue": 0.0
            }
        }"#;

        let resp: R2z2Response = serde_json::from_str(json).unwrap();
        assert_eq!(resp.killmail_id, 999);
        assert_eq!(resp.victim.ship_type_id, 0);
        assert!(resp.victim.items.is_empty());
        assert_eq!(resp.sequence_id, None);
        assert_eq!(resp.uploaded_at, None);
    }

    #[test]
    fn deserialize_sequence_response() {
        let json = r#"{"sequence": 987654}"#;
        let resp: R2z2SequenceResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.sequence, 987654);
    }

    #[test]
    fn deserialize_zkill_killmail() {
        let json = r#"[{
            "killmail_id": 123456,
            "killmail_time": "2026-03-17T12:00:00Z",
            "solar_system_id": 30000142,
            "victim": {
                "ship_type_id": 587,
                "items": [
                    {
                        "item_type_id": 2032,
                        "quantity_destroyed": 1,
                        "quantity_dropped": null
                    },
                    {
                        "item_type_id": 2048,
                        "quantity_destroyed": null,
                        "quantity_dropped": 5
                    }
                ]
            },
            "zkb": {
                "hash": "abcdef1234567890",
                "totalValue": 1500000.50
            }
        }]"#;

        let kills: Vec<ZkillKillmail> = serde_json::from_str(json).unwrap();
        assert_eq!(kills.len(), 1);
        let km = &kills[0];
        assert_eq!(km.killmail_id, 123456);
        assert_eq!(km.solar_system_id, 30000142);
        assert_eq!(km.victim.ship_type_id, 587);
        assert_eq!(km.victim.items.len(), 2);
        assert_eq!(km.victim.items[0].item_type_id, 2032);
        assert_eq!(km.victim.items[0].quantity_destroyed, Some(1));
        assert_eq!(km.victim.items[1].quantity_dropped, Some(5));
        assert_eq!(km.zkb.hash, "abcdef1234567890");
        assert_eq!(km.zkb.total_value, 1500000.50);
    }

    #[test]
    fn test_parse_killmail_time_iso8601_z() {
        use chrono::{Datelike, Timelike};
        let dt = parse_killmail_time("2026-03-17T12:00:00Z");
        assert_eq!(dt.year(), 2026);
        assert_eq!(dt.month(), 3);
        assert_eq!(dt.day(), 17);
        assert_eq!(dt.hour(), 12);
    }

    #[test]
    fn test_parse_killmail_time_no_tz() {
        use chrono::{Datelike, Timelike};
        let dt = parse_killmail_time("2026-03-17T12:00:00");
        assert_eq!(dt.year(), 2026);
        assert_eq!(dt.month(), 3);
        assert_eq!(dt.hour(), 12);
    }

    #[test]
    fn test_parse_killmail_time_invalid_fallback() {
        let before = chrono::Utc::now();
        let dt = parse_killmail_time("garbage");
        let after = chrono::Utc::now();
        assert!(dt >= before && dt <= after);
    }

    #[test]
    fn deserialize_history() {
        let json = r#"{"100001": "hash1", "100002": "hash2", "100003": "hash3"}"#;
        let map: std::collections::HashMap<String, String> =
            serde_json::from_str(json).unwrap();
        let mut pairs: Vec<(i64, String)> = map
            .into_iter()
            .filter_map(|(k, v)| k.parse::<i64>().ok().map(|id| (id, v)))
            .collect();
        pairs.sort_unstable_by_key(|(id, _)| *id);
        assert_eq!(pairs.len(), 3);
        assert_eq!(pairs[0].0, 100001);
        assert_eq!(pairs[0].1, "hash1");
        assert_eq!(pairs[2].0, 100003);
        assert_eq!(pairs[2].1, "hash3");
    }
}
