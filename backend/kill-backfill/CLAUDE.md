# kill-backfill

Resumable utility binary — backfills historical killmails from zKillboard + ESI.

## What It Does

1. Parses `--days N` CLI arg (default: 90)
2. Reads last completed date from worker_state table (for resumption after crash)
3. For each date from start to yesterday:
   - Fetches killmail (id, hash) pairs from zKillboard history API
   - Fetches full killmail from ESI for each pair
   - Inserts killmail + items + victim into DB
   - Saves progress date to worker_state

## Usage

```bash
make backfill-kills          # default 90 days
cargo run -p kill-backfill -- --days 30
```

Typical runtime: ~6–8 hours for 90 days (sequential, rate-limited).

## Rate Limiting

- Sequential requests (~70ms each ≈ 14 req/s, under ESI's 15 req/s limit)
- ESI budget checks every 20 killmails:
  - Budget < 10 → pause 60s
  - Budget < 30 → pause 10s
- Max 5 retries per ESI request with exponential backoff

## Dependencies

External: tokio, sqlx, chrono, tracing, dotenvy, serde_json
Workspace: nea-db, nea-esi, nea-zkill
