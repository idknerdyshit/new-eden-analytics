# market-seed

Utility binary — pre-populates market history for tracked items.

## What It Does

Fetches recent market history from ESI for all tracked material and product types and inserts it into the database. Useful for bootstrapping a fresh database with enough historical data to run analysis.

## Usage

```bash
cargo run -p market-seed
```

## Dependencies

External: tokio, sqlx, chrono, tracing, dotenvy
Workspace: nea-db, nea-esi
