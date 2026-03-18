# nea-esi

HTTP client for EVE Online's ESI (EVE Swagger Interface) API with built-in rate limiting.

## Responsibilities

- Market history fetching (daily OHLCV per region/type)
- Market order fetching (paginated, with best bid/ask computation)
- Killmail fetching (raw JSON or typed)

## Key Types

- `EsiClient` — holds reqwest::Client, Semaphore(20), AtomicI32 error_budget
- `EsiMarketHistoryEntry` — date, average, highest, lowest, volume, order_count
- `EsiMarketOrder` — order_id, type_id, location_id, price, volume, is_buy_order, etc.
- `EsiKillmail`, `EsiKillmailVictim`, `EsiKillmailItem` — nested killmail structure
- `EsiError` — Http, Api(status, message), RateLimited, Deserialize, Internal

## Rate Limiting

The client implements an error-budget feedback loop based on ESI's `X-ESI-Error-Limit-Remain` header:
- Max 20 concurrent requests (tokio Semaphore)
- Budget < 20 → 1s delay before request
- Budget ≤ 0 → refuse request (return RateLimited error)
- Budget updated after every response

## Public API

- `market_history(region_id, type_id)` → `Vec<EsiMarketHistoryEntry>`
- `market_orders(region_id, type_id)` → `Vec<EsiMarketOrder>` (handles x-pages pagination)
- `get_killmail(id, hash)` → `serde_json::Value`
- `get_killmail_typed(id, hash)` → `EsiKillmail`
- `compute_best_bid_ask(orders, station_id)` → `(best_bid, best_ask, bid_vol, ask_vol)`

## Constants

- `BASE_URL`: `https://esi.evetech.net/latest`
- `THE_FORGE`: 10000002 (region ID)
- `JITA_STATION`: 60003760

## Tests

12 unit tests covering bid/ask computation and deserialization of market history and killmails.

## Dependencies

External: reqwest, serde/serde_json, tokio, tracing, thiserror, chrono
