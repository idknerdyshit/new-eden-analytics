# New Eden Analytics

An EVE Online market analytics platform that detects and quantifies correlations between ship/item destruction and raw material price movements. Uses cross-correlation and Granger causality to identify lag times between destruction events and market impact.

## Prerequisites

- [Docker](https://docs.docker.com/get-docker/) and Docker Compose
- [Rust](https://rustup.rs/) (1.75+) — for local backend development
- [Node.js](https://nodejs.org/) (20+) — for local frontend development
- An EVE Online account — for SSO API keys

## API Keys

### EVE SSO (required for auth features)

1. Go to [EVE Developer Portal](https://developers.eveonline.com/applications)
2. Create a new application
3. Set the callback URL to `http://localhost:3000/api/auth/callback` (or your deployment URL)
4. No scopes are needed for the MVP (public market data only)
5. Note your **Client ID** and **Secret Key**

### ESI (EVE Swagger Interface)

No API key required. ESI public endpoints are used for market data. The app respects ESI rate limits via the `X-ESI-Error-Limit-Remain` header.

### zKillboard R2Z2

No API key required. The R2Z2 endpoint is public. A User-Agent header is sent per zKillboard's request.

## Setup

### 1. Clone and configure

```bash
git clone https://github.com/your-username/new-eden-analytics.git
cd new-eden-analytics
cp .env.example .env
```

Edit `.env` with your values:

```env
DATABASE_URL=postgres://nea:nea_password@localhost:5432/new_eden_analytics
POSTGRES_USER=nea
POSTGRES_PASSWORD=nea_password
POSTGRES_DB=new_eden_analytics

ESI_CLIENT_ID=your_eve_client_id
ESI_CLIENT_SECRET=your_eve_secret_key
ESI_CALLBACK_URL=http://localhost:3000/api/auth/callback

SESSION_SECRET=generate_a_random_64_char_string_here

RUST_LOG=info
```

### 2. Run with Docker Compose (recommended)

```bash
# Start everything (TimescaleDB, backend server, worker, frontend)
make up

# Or directly:
docker compose up -d
```

This starts:
- **TimescaleDB** on port 5432
- **Backend API server** on port 3001
- **Backend worker** (background data ingestion)
- **Frontend** on port 3000

### 3. Import SDE data

The Static Data Export (ship/item definitions, blueprints) must be imported once:

```bash
make seed-sde

# Or manually:
docker compose exec backend-worker /usr/local/bin/sde-import
```

This downloads ~50MB of CSV data from Fuzzwork and imports ~40k+ item types and blueprint data. Takes 2-5 minutes.

### 4. Backfill historical kill data (optional)

To seed ~90 days of historical killmail data from zKillboard (improves correlation analysis on first run):

```bash
make backfill-kills
```

This is resumable — if interrupted, re-running picks up where it left off. Use `--days N` to control the backfill depth (default 90). Takes a while due to API rate limits.

### 5. Open the app

Visit [http://localhost:3000](http://localhost:3000)

The dashboard will be empty until workers have collected data. Market history starts flowing within the first hour. Kill data streams in real-time. Correlation analysis runs daily at 02:00 UTC (needs several days of data).

## Local Development

### Backend

```bash
cd backend

# Check that it compiles
cargo check

# Run the API server
cargo run -p nea-server

# Run the background worker
cargo run -p nea-worker

# Run the SDE import
cargo run -p sde-import
```

Requires a running TimescaleDB instance (start just the DB with `docker compose up timescaledb -d`).

### Frontend

```bash
cd frontend

# Install dependencies
npm install

# Run dev server (proxies /api to localhost:3001)
npm run dev

# Type check
npx svelte-check

# Production build
npm run build
```

## Architecture

```
┌─────────────┐     ┌──────────────┐     ┌─────────────────┐
│   Frontend   │────▶│  API Server  │────▶│   TimescaleDB   │
│  (SvelteKit) │     │   (Axum)     │     │  (PostgreSQL)   │
└─────────────┘     └──────────────┘     └────────▲────────┘
                                                   │
                    ┌──────────────┐                │
                    │    Worker    │────────────────┘
                    │  (Tokio)    │
                    └──────┬───────┘
                           │
              ┌────────────┼────────────┐
              ▼            ▼            ▼
         ┌────────┐  ┌──────────┐  ┌──────────┐
         │  ESI   │  │   R2Z2   │  │ Fuzzwork │
         │(market)│  │ (kills)  │  │  (SDE)   │
         └────────┘  └──────────┘  └──────────┘
```

- **nea-server**: Axum HTTP API with 12 endpoints (dashboard, search, market data, analysis, auth)
- **nea-worker**: 5 background tasks (market history, order snapshots, killmail polling, daily aggregation, correlation analysis)
- **nea-db**: Shared database layer with sqlx migrations
- **nea-esi**: ESI API client with rate limiting and pagination
- **nea-zkill**: zKillboard R2Z2 sequential polling client
- **nea-analysis**: Cross-correlation and Granger causality statistical analysis
- **kill-backfill**: One-shot CLI tool to backfill historical killmails from zKillboard + ESI

## Makefile Commands

| Command | Description |
|---------|-------------|
| `make up` | Start all services |
| `make down` | Stop all services |
| `make build` | Rebuild Docker images |
| `make logs` | Tail logs from all services |
| `make migrate` | Run database migrations |
| `make seed-sde` | Import EVE static data |
| `make backfill-kills` | Backfill ~90 days of historical killmails from zKillboard |

## Data Flow

1. **SDE Import** (one-time): Item types, blueprints, and material requirements from Fuzzwork CSVs
2. **Market History** (hourly): Daily OHLCV data for The Forge region from ESI
3. **Order Snapshots** (hourly): Best bid/ask prices at Jita 4-4 from ESI
4. **Killmails** (real-time): Ship and item destruction data from zKillboard R2Z2
5. **Daily Aggregation** (hourly): Rolls up killmail data into daily destruction volumes
6. **Correlation Analysis** (daily at 02:00 UTC): Computes cross-correlation and Granger causality for all product-material pairs over 180-day windows
