# New Eden Analytics - MVP Plan

## Context

EVE Online's economy creates a feedback loop: when ships and items are destroyed in combat, replacement demand drives up raw material prices. This project builds an analytics platform that detects and quantifies these correlations, including the lag time between destruction events and material price movements. The goal is a public-facing dashboard where players and market analysts can explore these relationships using statistical methods (cross-correlation, Granger causality).

## Tech Stack

| Layer | Choice |
|-------|--------|
| Backend | Rust (Axum framework, Cargo workspace) |
| Frontend | SvelteKit + D3.js + Tailwind CSS (dark theme) |
| Database | TimescaleDB (PostgreSQL + time-series extension) |
| Data Sources | ESI (market/industry), zKillboard R2Z2 (kills), Fuzzwork (SDE dumps) |
| Auth | EVE SSO (OAuth2) |
| Deployment | Docker Compose |

## Project Structure

```
new-eden-analytics/
├── docker-compose.yml
├── .env.example
├── PLAN.md
├── Makefile                          # dev commands: up, down, migrate, seed-sde
├── backend/
│   ├── Cargo.toml                    # workspace root
│   ├── crates/
│   │   ├── nea-server/               # Axum HTTP server
│   │   ├── nea-worker/               # Background data ingestion
│   │   ├── nea-db/                   # Shared DB layer (sqlx + migrations)
│   │   ├── nea-esi/                  # ESI API client
│   │   ├── nea-zkill/                # zKillboard R2Z2 client
│   │   └── nea-analysis/             # Statistical analysis (correlation, Granger)
│   └── sde-import/                   # One-shot SDE import CLI tool
├── frontend/
│   ├── src/
│   │   ├── lib/
│   │   │   ├── api/                  # Typed fetch wrappers
│   │   │   ├── charts/               # D3 chart components
│   │   │   ├── components/           # Shared UI components
│   │   │   └── stores/               # Svelte stores (auth, search)
│   │   └── routes/
│   │       ├── +page.svelte          # Dashboard
│   │       ├── search/+page.svelte   # Item search/browse
│   │       ├── items/[typeId]/       # Item detail with charts
│   │       └── auth/                 # Login + callback
└── infra/docker/                     # Dockerfiles
```

**Why separate server + worker binaries?** Worker restart (bug, deploy) doesn't cause API downtime. They share `nea-db` and the same database.

## Database Schema

### SDE Tables (Migration 001)
- `sde_types` — item type definitions (type_id, name, group, category). GIN index on name for full-text search.
- `sde_blueprints` — blueprint -> product mappings
- `sde_blueprint_materials` — blueprint -> required materials + quantities
- `v_product_materials` — convenience view: product -> materials (skips blueprint indirection)

### Market Tables (Migration 002) — TimescaleDB hypertables
- `market_history` — daily OHLCV per type (from ESI `/markets/{region}/history/`). Hypertable on `date`.
- `market_snapshots` — hourly best bid/ask per type (derived from ESI orders endpoint). Hypertable on `ts`.

### Kill Tables (Migration 003) — TimescaleDB hypertables
- `killmails` — kill metadata (id, time, system, value, R2Z2 sequence). Hypertable on `kill_time`.
- `killmail_items` — items destroyed/dropped per killmail. Hypertable on `kill_time`.
- `killmail_victims` — victim ship type per killmail. Hypertable on `kill_time`.
- `daily_destruction` — pre-aggregated daily destruction volume per type (materialized by worker).

### Analysis Tables (Migration 004)
- `correlation_results` — computed correlation data: product_type_id, material_type_id, optimal lag_days, correlation_coeff, granger_p_value, granger_significant, analysis window dates.

### User Tables (Migration 005)
- `users` — EVE character info + encrypted tokens
- `sessions` — session tokens with expiry

## Backend Architecture

### API Routes (nea-server)
```
GET  /api/auth/login                    → redirect to EVE SSO
GET  /api/auth/callback                 → exchange code, create session
POST /api/auth/logout                   → invalidate session

GET  /api/dashboard                     → top correlations, trending destruction
GET  /api/dashboard/movers              → biggest material price movers (24h)

GET  /api/items?q=...&page=             → full-text search over sde_types
GET  /api/items/:type_id                → item detail + blueprint materials

GET  /api/market/:type_id/history?days= → market_history rows
GET  /api/market/:type_id/snapshots?hours= → market_snapshots rows

GET  /api/analysis/:type_id/correlations → correlation results for product
GET  /api/analysis/:type_id/lag          → lag breakdown per material
GET  /api/analysis/top?limit=20          → globally strongest correlations

GET  /api/destruction/:type_id?days=     → daily_destruction for a type
```

Most endpoints are public. Auth only required for future personalized features.

### Background Workers (nea-worker)

| Worker | Frequency | What it does |
|--------|-----------|-------------|
| ESI Market History Poller | Hourly | Fetches daily OHLCV for all tracked types in region 10000002 (The Forge). Respects ESI `Expires` header. Semaphore-limited to 20 concurrent requests, monitors `X-ESI-Error-Limit-Remain`. |
| ESI Orders Snapshotter | Hourly | Fetches orders, filters to Jita 4-4 (location 60003760), computes best bid/ask + volumes. |
| R2Z2 Killmail Poller | Real-time (~100ms) | Sequential polling of R2Z2 endpoint. Parses victim ship + destroyed items. Persists last sequence ID for crash recovery. |
| Daily Aggregation | Hourly | Aggregates killmail_items + killmail_victims into daily_destruction. |
| Correlation Analyzer | Daily (02:00 UTC) | For each product-material pair: fetches 180-day time series, runs cross-correlation + Granger causality, writes results. |

### ESI Client (nea-esi)
- `reqwest` HTTP client with User-Agent header
- Semaphore for concurrency limiting
- Atomic error budget tracker (from `X-ESI-Error-Limit-Remain`)
- Auto-pagination via `X-Pages` header
- Methods: `market_history()`, `market_orders()`

### R2Z2 Client (nea-zkill)
- Sequential polling: fetch `{sequence_id}.json`, increment, 100ms between successes, 6s backoff on 404
- Persisted sequence ID in `worker_state` table

## Statistical Analysis (nea-analysis)

### Data Preparation
1. **Align** destruction and price series to same daily timestamps (forward-fill prices, zero-fill destruction)
2. **Difference** both series (first-order) to achieve stationarity
3. **Z-score normalize** for comparable correlation coefficients
4. **Minimum 60 days** of overlapping data required

### Cross-Correlation
Compute CCF(k) for k in [-30, +30] days. Peak absolute value determines optimal lag. Positive k = destruction leads price (expected). Confidence band at +/- 1.96/sqrt(N).

```rust
pub fn cross_correlation(x: &[f64], y: &[f64], max_lag: i32) -> Vec<(i32, f64)>
```

### Granger Causality
1. Fit restricted model: P[t] = autoregressive on price only
2. Fit unrestricted model: add lagged destruction terms
3. F-test comparing RSS of both models
4. Use `nalgebra` for OLS, `statrs` for F-distribution CDF
5. p < 0.05 = significant

## Frontend Pages

### Dashboard (`/`)
- **Top Correlations** table: item, material, lag days, r-value, significance indicator. Rows link to item detail.
- **Trending Destruction**: items with unusual destruction spikes
- **Biggest Material Movers**: materials with largest 24h price change

### Search (`/search`)
- Debounced full-text search against `/api/items?q=...`
- Results show item name, category, analysis availability
- Click-through to item detail

### Item Detail (`/items/[typeId]`)
- **Item Info**: name, category, blueprint materials list
- **Destruction Volume**: D3 bar chart with 7-day moving average overlay
- **Material Price Impact**: dual Y-axis chart per material — price line + destruction volume area
- **Correlation Analysis**: CCF bar chart (lag on X, correlation on Y) with optimal lag highlighted
- **Lag Visualization**: timeline showing destruction event → material price effect with arrow showing lag days
- **Granger Causality**: table of p-values per material with green/red significance indicators

### D3 Charts
All follow the same pattern: data via Svelte prop → `onMount` + D3 render to SVG → reactive re-render. Tooltip on hover, D3 brush for time range zoom on price charts.

## Key Rust Crates

| Crate | Purpose |
|-------|---------|
| `axum` 0.7+ | HTTP framework |
| `tokio` 1 (full) | Async runtime |
| `sqlx` 0.8+ (postgres, chrono, uuid) | Async DB with compile-time query checks |
| `reqwest` 0.12+ (json) | HTTP client for ESI/R2Z2 |
| `serde` / `serde_json` | Serialization |
| `chrono` 0.4 | Date/time |
| `nalgebra` 0.33+ | Linear algebra for Granger OLS |
| `statrs` 0.17+ | F-distribution CDF |
| `tower-http` 0.5+ | CORS, tracing, compression middleware |
| `tracing` + `tracing-subscriber` | Structured logging |
| `oauth2` 4 | EVE SSO OAuth2 flow |
| `dotenvy` | .env loading |
| `csv` 1 | Parsing Fuzzwork SDE dumps |
| `uuid` 1 | Session tokens |
| `thiserror` 1 | Error types |

## SDE Import Strategy

Use Fuzzwork CSV/SQL dumps (not raw SDE YAML — deeply nested and awkward to parse). The `sde-import` CLI tool downloads `invTypes.csv`, `industryActivityMaterials`, and `industryActivityProducts` from Fuzzwork, parses them, and bulk-inserts into the SDE tables. Run once at setup or when EVE has an expansion.

## Implementation Phases

### Phase 1: Foundation (Week 1-2)
- Init monorepo: Cargo workspace, SvelteKit project, Docker Compose
- Write all 5 DB migrations
- Build `sde-import` tool (Fuzzwork CSV → DB)
- Build `nea-esi` crate with rate limiting
- Build ESI market history poller + orders snapshotter
- **Deliverable**: Docker stack ingesting market data

### Phase 2: Kill Data + Aggregation (Week 3)
- Build `nea-zkill` R2Z2 client
- Build R2Z2 killmail poller + daily aggregation worker
- Worker state persistence (last sequence ID)
- Backfill ~90 days of kills via zKillboard history API (`/api/history/{date}.json`) + ESI killmail detail endpoint. ESI market history already provides ~1 year of daily data, so no market backfill needed.
- **Deliverable**: Full data pipeline running

### Phase 3: Statistical Analysis (Week 4)
- Build `nea-analysis` crate (timeseries prep, cross-correlation, Granger)
- Build analyzer worker task (nightly runs)
- Performance test: ~5000 pairs in <15 min
- **Deliverable**: `correlation_results` populated

### Phase 4: API Server (Week 5)
- Build `nea-db` query layer
- Build Axum router with all routes
- EVE SSO OAuth2 flow
- Auth middleware, error handling, request logging
- **Deliverable**: Fully functional API

### Phase 5: Frontend (Week 6-7)
- SvelteKit + Tailwind dark theme setup
- API client layer
- Dashboard, Search, Item Detail pages
- All 4 D3 chart components
- Auth flow (login button, callback, character name display)
- **Deliverable**: Complete working application

### Phase 6: Polish + Deploy (Week 8)
- Docker Compose hardening (health checks, restart policies, volume mounts)
- Error handling audit (no panics in workers)
- TimescaleDB retention policies
- .env.example + README
- Deploy to VPS
- **Deliverable**: Production deployment

## Verification Plan

1. **Data ingestion**: Run Docker Compose, wait 1 hour, query `market_history` and `killmails` tables to confirm rows are flowing
2. **SDE import**: Run `sde-import`, verify `SELECT COUNT(*) FROM sde_types` returns ~40k+ types and `sde_blueprint_materials` has entries
3. **Analysis**: After 1+ days of data, trigger analyzer manually, verify `correlation_results` has rows with plausible values
4. **API**: `curl` all endpoints, verify JSON responses with correct data
5. **Frontend**: Load dashboard, search for "Rifter", navigate to detail page, verify all charts render with data
6. **Auth**: Click login, complete EVE SSO flow, verify session cookie and character name display
7. **End-to-end**: Destroy a popular ship in-game (or wait for natural kills), observe destruction data appear, then check if material price correlation is detected in next analysis run
