# nea-worker

Background task runner — 5 long-running async tasks for data ingestion and analysis.

## Responsibilities

- Hourly market history ingestion from ESI
- Hourly order snapshot ingestion from ESI (Jita 4-4 bid/ask)
- Continuous killmail polling from R2Z2
- Hourly aggregation of killmails into daily destruction volumes
- Daily correlation analysis (02:00 UTC)

## Tasks

| Module | Schedule | Description |
|--------|----------|-------------|
| `market_history` | Hourly | Fetches daily OHLCV for all tracked types (materials + products) in The Forge, concurrent via Semaphore(20) |
| `market_orders` | Hourly | Fetches order book for tracked types, computes best bid/ask at Jita 4-4 |
| `killmail_poller` | Continuous | Sequential R2Z2 polling, persists sequence ID in worker_state for resumption |
| `aggregation` | Hourly | Rolls up killmail items + victims into daily_destruction (last 7 days) |
| `analyzer` | Daily 02:00 UTC | Runs full cross-correlation + Granger analysis over 180-day window |

## CLI

- Default: spawns all 5 tasks, runs until Ctrl+C or task failure
- `--run-once [task_name]`: execute a single named task and exit (useful for manual triggers and debugging)

## Patterns

- Each task runs in its own `tokio::spawn`
- Long loops with `tokio::time::interval` or custom duration logic
- Worker state table used for crash recovery (e.g., R2Z2 sequence ID, backfill progress)
- All task starts/completions/errors logged

## Dependencies

External: tokio, tracing, chrono, dotenvy, sqlx
Workspace: nea-db, nea-esi, nea-zkill, nea-analysis
