# nea-zkill

Client for zKillboard's R2Z2 API — used for real-time killmail polling and historical backfill.

## Responsibilities

- Sequential polling of new killmails via R2Z2 sequence endpoint
- Fetching historical killmail ID/hash lists by date (for backfill)

## Key Types

- `R2z2Client` — reqwest::Client with 10s timeout
- `R2z2Response` — killmail_id, killmail_hash, killmail_time, solar_system_id, total_value, victim, items
- `R2z2Victim` — ship_type_id, character_id, corporation_id, alliance_id (all optional)
- `R2z2Item` — type_id, quantity_destroyed, quantity_dropped, flag, singleton
- `ZkillKillmail`, `ZkillVictim`, `ZkillItem`, `ZkillZkb` — zKillboard kills API format
- `ZkillError` — Http, Api, Deserialize, NotFound

## Public API

- `fetch_sequence(sequence_id)` → `Result<Option<R2z2Response>>` (404 → Ok(None) meaning "no new data")
- `fetch_history(date: "YYYYMMDD")` → `Result<Vec<(killmail_id, hash)>>`

## Utility

- `parse_killmail_time(time_str)` → `DateTime<Utc>` — handles both ISO 8601 with Z suffix and bare datetime

## Design Notes

- Polling is intentionally sequential (not concurrent) for stability with zKillboard
- 404 is a normal response meaning "caught up" — handled as `Ok(None)`, not an error

## Dependencies

External: reqwest, serde/serde_json, chrono, tokio, tracing, thiserror
