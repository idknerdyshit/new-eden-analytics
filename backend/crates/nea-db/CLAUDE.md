# nea-db

Database layer for the entire platform. All other crates depend on this for models, queries, and pool management.

## Responsibilities

- Connection pooling (`create_pool`, max 20 connections)
- Embedded migrations (5 files in `./migrations`, run via `run_migrations()`)
- Row types for every table (`models.rs`)
- 50+ query functions (`queries.rs`), all async

## Key Modules

| File | Contents |
|------|----------|
| `pool.rs` | `create_pool()`, `run_migrations()` |
| `models.rs` | All row structs: SDE types/blueprints, market history/snapshots, killmails/items/victims, daily destruction, correlation results, users/sessions, worker state, dashboard movers |
| `queries.rs` | All database queries — search, insert, upsert, get, aggregation |
| `error.rs` | `DbError` enum (Sqlx, NotFound) |

## Patterns

- All queries use `sqlx::query_as!()` for compile-time checked SQL
- Timing: each query logs elapsed_ms at debug level via `Instant::now()`
- Idempotent inserts via `ON CONFLICT DO NOTHING`; upserts via `ON CONFLICT DO UPDATE`
- Full-text search on item names using `to_tsvector`/`plainto_tsquery`
- Worker state table is a simple KV store (`key TEXT PRIMARY KEY, value TEXT`)

## Migration Order

1. `001_sde_tables` — item types, groups, categories, blueprints, materials
2. `002_market_tables` — market history (daily OHLCV), snapshots (bid/ask)
3. `003_kill_tables` — killmails, items, victims, daily_destruction
4. `004_analysis_tables` — correlation_results
5. `005_user_tables` — users, sessions, worker_state

## Dependencies

External: sqlx (Postgres, runtime-tokio), chrono, serde, uuid, thiserror, tracing
