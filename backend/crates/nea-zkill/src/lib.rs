// nea-zkill: Client for the zKillboard R2Z2 API (RedisQ replacement).

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

#[derive(Debug, Clone, Deserialize)]
pub struct R2z2Response {
    pub killmail_id: i64,
    pub killmail_hash: String,
    #[serde(default)]
    pub killmail_time: String,
    #[serde(default)]
    pub solar_system_id: i32,
    #[serde(default)]
    pub total_value: f64,
    #[serde(default)]
    pub victim: R2z2Victim,
    #[serde(default)]
    pub items: Vec<R2z2Item>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct R2z2Victim {
    #[serde(default)]
    pub ship_type_id: i32,
    #[serde(default)]
    pub character_id: Option<i64>,
    #[serde(default)]
    pub corporation_id: Option<i64>,
    #[serde(default)]
    pub alliance_id: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct R2z2Item {
    pub type_id: i32,
    #[serde(default)]
    pub quantity_destroyed: Option<i64>,
    #[serde(default)]
    pub quantity_dropped: Option<i64>,
    #[serde(default)]
    pub flag: i32,
    #[serde(default)]
    pub singleton: i32,
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
            .user_agent("new-eden-analytics/0.1 (https://github.com/eyedeekay/new-eden-analytics)")
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .expect("failed to build reqwest client");

        Self {
            client,
            base_url: "https://r2z2.zkillboard.com/v1".to_string(),
        }
    }

    /// Fetch a single killmail by its R2Z2 sequence ID.
    ///
    /// Returns `Ok(None)` when the server responds with 404 (no new data yet),
    /// `Ok(Some(response))` on 200, and `Err` for anything else.
    pub async fn fetch_sequence(&self, sequence_id: i64) -> Result<Option<R2z2Response>> {
        let url = format!("{}/{}.json", self.base_url, sequence_id);
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
            "killmail_hash": "abcdef1234567890",
            "killmail_time": "2026-03-17T12:00:00Z",
            "solar_system_id": 30000142,
            "total_value": 1500000.50,
            "victim": {
                "ship_type_id": 587,
                "character_id": 91234567,
                "corporation_id": 98000001,
                "alliance_id": null
            },
            "items": [
                {
                    "type_id": 2032,
                    "quantity_destroyed": 1,
                    "quantity_dropped": null,
                    "flag": 27,
                    "singleton": 0
                }
            ]
        }"#;

        let resp: R2z2Response = serde_json::from_str(json).unwrap();
        assert_eq!(resp.killmail_id, 123456);
        assert_eq!(resp.killmail_hash, "abcdef1234567890");
        assert_eq!(resp.victim.ship_type_id, 587);
        assert_eq!(resp.victim.alliance_id, None);
        assert_eq!(resp.items.len(), 1);
        assert_eq!(resp.items[0].quantity_destroyed, Some(1));
    }

    #[test]
    fn deserialize_response_missing_optional_fields() {
        let json = r#"{
            "killmail_id": 999,
            "killmail_hash": "deadbeef"
        }"#;

        let resp: R2z2Response = serde_json::from_str(json).unwrap();
        assert_eq!(resp.killmail_id, 999);
        assert_eq!(resp.victim.ship_type_id, 0);
        assert!(resp.items.is_empty());
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
