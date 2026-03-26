# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/),
and this project adheres to [Semantic Versioning](https://semver.org/).

## [0.2.0] - 2026-03-25

### Added

- Pilot profiles: attacker tracking, character search, fitting analysis (f85086d)
- Corporation & alliance doctrine detection (add16be)
- Enriched kill/loss history and killmail detail pages (5d5967f)
- Recent doctrines section to dashboard (51c9ba6)
- Full fleet compositions and grouped multi-ship doctrines (5561eac)
- 5% fleet share threshold for doctrine membership (a935f35)
- Collapsible doctrine cards with responsive grid for fits (72ea520)
- Placeholder for doctrine ships with no fit data (13d9fd0)
- Traefik reverse proxy with dev/prod split (74022f3)
- Production hardening: health checks, error handling, retention policies (48e464e)
- Security hardening: JWT verification, CSRF protection, CORS, cookie flags (4f39f62)
- `make stop-backfill` target to stop the kill backfill worker (a9b18d6)
- Initial EVE Online market analytics platform (e92e710)

### Fixed

- Backfill deserialization errors for NPC/structure killmails (eeff8ce)
- Excessive Unknown pilots by using bulk ESI resolution with retry (2818919)
- Backfill crash on compressed killmail_items chunks (7979885)
- Duplicate Traefik router and missing type_name in aggregation query (643e4c0)
- Backend-worker healthcheck to work without kill binary (0122cc6)
- R2Z2 deserialization to match ESI envelope format (3f1485b)
- Poller stuck on unparseable killmails, reduced ESI rate limit pressure (0004945)
- Killmail_time made optional so missing values don't block the poller (2d8d6de)
- Deduplicate killmail items by (type_id, flag) before insert (c760245)

### Changed

- Optimize solo kill/loss queries and increase TimescaleDB shared memory (6c8963a)
- Tune TimescaleDB: recompress chunks, drop redundant indexes, enable pg_stat_statements (55ba143)
- Optimize aggregation and killmail item lookups (4f0be20)
- Improve doctrine clustering and metadata (7f9811a)
- Improve dashboard data and optimize killmail queries (0856fdc)
- Optimize SQL queries: batch N+1, LATERAL joins, compression (f67bd31)
- Lower doctrine aggregation thresholds for smaller alliances (841b07d)
- Rewrite doctrine detection with engagement-based composition clustering (5696d22)
- Replace collapsible variant fits with diff overlay (c6f005b)
- Rewrite doctrine detection with attack-side grouping and logi detection (d0cb620)
- Skip non-tradable types in market history and order fetches (45b6708)
- Update axum route params from :param to {param} syntax (200639b)
- Switch nea-esi dependency from git to crates.io v0.1.1 (bcaf6f2)
- Upgrade dependencies to latest stable versions (d7f2104)
- Extract nea-esi into standalone repo for reuse across projects (74e86c3)
- Cache placeholder entries for ESI 404s to stop retrying deleted entities (dbfc8b3)
- Reduce market orders request rate to ~2 req/s to ease ESI pressure (91f48f0)
- Address 10 code review fixes: batch inserts, indexes, security, tests, CI (6af6dc3)
- Reduce backfill rate to ~2 req/s to ease ESI pressure during re-backfill (ff71fcb)
- Move unique indexes for victim/item upserts to migration 008 (7c9668f)
- Upsert victims and items during backfill for historical identity and flag fields (c993c7f)
- Rename dashboard labels: "Units Destroyed" and "Killmails" (fa13735)
- Filter trending destruction to only show ships and modules (76bf544)
- Show item names instead of type IDs in trending destruction dashboard (f2d0427)
- Update nea-esi to 0.4.0 and simplify call sites (21a6956)
- Remove redundant delete_killmail_items used before upsert (dd59b25)
- Refactor R2Z2 response model, deduplicate queries/formatters (08b0956)
