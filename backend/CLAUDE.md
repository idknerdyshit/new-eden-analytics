# Backend — Rust Workspace

## Workspace Structure

```
crates/
  nea-server/      Axum HTTP API server (port 3001)
  nea-worker/      Background data ingestion + analysis tasks
  nea-db/          Database layer (sqlx, migrations, models, queries)
  nea-esi/         ESI API client with rate limiting
  nea-zkill/       zKillboard R2Z2 polling client
  nea-analysis/    Statistical analysis (cross-correlation, Granger causality)
sde-import/        One-shot SDE CSV import utility
kill-backfill/     One-shot historical killmail backfill utility
```

## Crate Dependencies

```
nea-server     → nea-db, nea-esi (for OAuth config only)
nea-worker     → nea-db, nea-esi, nea-zkill, nea-analysis
sde-import     → nea-db
kill-backfill  → nea-db, nea-esi, nea-zkill
```

## Crate Details

### nea-server
- Axum 0.7, binds port 3001
- `AppState`: PgPool + ESI OAuth config (client_id, client_secret, callback_url, session_secret)
- `ApiError` enum: Db, NotFound, BadRequest, Unauthorized, Internal — implements `IntoResponse`
- Middleware stack: request_id (UUID v4), CORS (permissive), TraceLayer
- 6 route modules: dashboard, items, market, analysis, destruction, auth

### nea-worker
- 5 async tasks spawned via `tokio::spawn`, runs until Ctrl+C or task failure:
  - `market_history` — hourly ESI market history fetch
  - `market_orders` — hourly ESI order snapshots
  - `killmail_poller` — continuous R2Z2 sequential polling
  - `aggregation` — hourly rollup of killmails to daily destruction
  - `analyzer` — daily at 02:00 UTC, runs correlation analysis

### nea-db
- sqlx 0.8 with compile-time query checking
- Modules: `pool.rs` (create_pool + run_migrations), `models.rs` (row types), `queries.rs` (all DB functions), `error.rs`
- 5 migration files: 001_sde_tables, 002_market_tables, 003_kill_tables, 004_analysis_tables, 005_user_tables

### nea-esi
- reqwest client with 30s timeout, User-Agent header
- Rate limiting: Semaphore(20) for concurrency + AtomicI32 error budget from `X-ESI-Error-Limit-Remain`
- Methods: `market_history`, `market_orders` (paginated), `get_killmail`, `get_killmail_typed`, `compute_best_bid_ask`
- Killmail types: `EsiKillmail`, `EsiKillmailVictim`, `EsiKillmailItem`
- Constants: `THE_FORGE` (region 10000002), `JITA_STATION` (60003760)

### nea-zkill
- R2Z2 sequential polling client
- Methods: `fetch_sequence`, `fetch_history`
- Response types: `R2z2Response`, `R2z2Victim`, `R2z2Item`

### nea-analysis
- `timeseries` module: align_series (forward-fill prices, zero-fill destruction), differencing, z-normalize
- Cross-correlation function (lags -30..+30)
- Granger causality: OLS via nalgebra, F-test via statrs F-distribution
- `analyze()` pipeline returns `AnalysisResult` (optimal lag, CCF, Granger result)

## API Endpoints

All routes are nested under `/api`:

| Method | Path | Handler | Module |
|--------|------|---------|--------|
| GET | `/api/dashboard` | `dashboard` | dashboard |
| GET | `/api/dashboard/movers` | `movers` | dashboard |
| GET | `/api/items` | `search_items` | items |
| GET | `/api/items/:type_id` | `get_item` | items |
| GET | `/api/market/:type_id/history` | `history` | market |
| GET | `/api/market/:type_id/snapshots` | `snapshots` | market |
| GET | `/api/analysis/:type_id/correlations` | `correlations` | analysis |
| GET | `/api/analysis/:type_id/lag` | `lag` | analysis |
| GET | `/api/analysis/top` | `top` | analysis |
| GET | `/api/destruction/:type_id` | `destruction` | destruction |
| GET | `/api/auth/login` | `login` | auth |
| GET | `/api/auth/callback` | `callback` | auth |
| POST | `/api/auth/logout` | `logout` | auth |
| GET | `/api/auth/me` | `me` | auth |

## Auth Flow

EVE SSO OAuth2 → JWT decode (no signature verification) → session cookie. Login redirects to EVE, callback exchanges code for token, extracts character info from JWT.

## Code Patterns

- Handlers: `#[tracing::instrument(skip(state))]`, return `Result<Json<T>, ApiError>`
- Queries: `Instant` timing, debug-level tracing with elapsed_ms
- Logging: `tracing` crate, `RUST_LOG` env filter, structured fields
- Log levels: info for requests/task lifecycle, debug for queries/ESI calls/analysis steps, warn/error for failures

## Build & Run

```bash
cargo check                    # type-check workspace
cargo test --lib               # unit tests (nea-analysis, nea-zkill)
cargo run -p nea-server        # API server (needs DATABASE_URL)
cargo run -p nea-worker        # worker (needs DATABASE_URL)
cargo run -p sde-import        # one-shot SDE import
cargo run -p kill-backfill     # backfill historical killmails (--days N, default 90)
```

Requires a running TimescaleDB: `docker compose up timescaledb -d`

## Testing

Unit tests exist in nea-analysis, nea-zkill, and nea-esi. No integration tests.
