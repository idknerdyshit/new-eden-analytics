# New Eden Analytics

EVE Online market analytics platform that correlates ship and item destruction with raw material price movements. Uses cross-correlation and Granger causality analysis over 180-day windows to identify statistically significant lead/lag relationships between destruction events and price changes in The Forge (Jita).

## Architecture

Five services orchestrated by Docker Compose:

| Service | Port | Description |
|---------|------|-------------|
| `traefik` | 3000 (dev) / 80+443 (prod) | Reverse proxy — plain HTTP locally, TLS via Let's Encrypt in prod |
| `timescaledb` | 5432 (dev) | TimescaleDB (PostgreSQL 16) — all persistent state |
| `backend-server` | 3001 (internal) | Axum HTTP API (routed via Traefik at `/api`) |
| `backend-worker` | — | Background data ingestion + analysis |
| `frontend` | 3000 (internal) | SvelteKit SSR app (routed via Traefik at `/`) |

## Quick Start

```bash
cp .env.example .env   # fill in ESI_CLIENT_ID, ESI_CLIENT_SECRET, SESSION_SECRET
make up                # docker compose up -d
make seed-sde          # one-time SDE import (~2-5 min)
# visit http://localhost:3000
```

## Makefile Commands

| Command | Description |
|---------|-------------|
| `make up` / `make down` | Start/stop all services (local dev, HTTP on :3000) |
| `make up-prod` / `make down-prod` | Start/stop production (TLS on :443 via Let's Encrypt) |
| `make build` | Rebuild Docker images |
| `make logs` | Tail all service logs |
| `make seed-sde` | Import EVE static data (item types, blueprints) |
| `make backfill-kills` | Backfill ~90 days of historical killmails (resumable) |
| `make migrate` | Run DB migrations |
| `make test` | Run backend unit tests |
| `make clean` | Full reset: removes containers, volumes, and images |

## Environment Variables

Defined in `.env` (copy from `.env.example`):

| Variable | Description |
|----------|-------------|
| `DOMAIN` | Public hostname for Traefik routing and TLS cert (default: `localhost`) |
| `ACME_EMAIL` | Email for Let's Encrypt certificate registration |
| `DATABASE_URL` | Postgres connection string |
| `POSTGRES_USER`, `POSTGRES_PASSWORD`, `POSTGRES_DB` | Used by TimescaleDB container |
| `ESI_CLIENT_ID`, `ESI_CLIENT_SECRET`, `ESI_CALLBACK_URL` | EVE SSO OAuth2 credentials |
| `SESSION_SECRET` | Cookie signing key |
| `RUST_LOG` | Tracing filter (default: `info`) |

## Data Flow

1. **SDE Import** (one-time) — item types and blueprints from Fuzzwork CSVs
2. **Market History** (hourly) — daily OHLCV for The Forge from ESI
3. **Order Snapshots** (hourly) — best bid/ask at Jita 4-4 from ESI
4. **Killmails** (continuous) — destruction data from zKillboard R2Z2
5. **Daily Aggregation** (hourly) — rolls up killmails into daily destruction volumes
6. **Correlation Analysis** (daily 02:00 UTC) — cross-correlation + Granger causality over 180-day windows

## Data Retention

TimescaleDB compression and retention policies are applied automatically:

| Table | Compression | Retention |
|-------|-------------|-----------|
| `market_history` | 30 days | None (historically valuable) |
| `market_snapshots` | 30 days | 1 year |
| `killmails` | 30 days | 1 year |
| `killmail_items` | 30 days | 1 year |
| `killmail_victims` | 30 days | 1 year |
| `daily_destruction` | 30 days | None (pre-aggregated, small) |

## Health Check

```
GET /api/health → {"status": "ok"}
```

Returns 200 when the API server and database are healthy, 503 otherwise.

## External APIs

- **ESI** (esi.evetech.net) — no API key, rate-limited via `X-ESI-Error-Limit-Remain` header
- **R2Z2/zKillboard** — no API key, sequential polling with User-Agent header
- **Fuzzwork** — CSV downloads for SDE data, used only during seed

## License

All rights reserved.
