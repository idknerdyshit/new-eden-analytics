-- Retention and compression policies for TimescaleDB hypertables.
-- All calls use if_not_exists => TRUE for idempotent re-runs.

-- ---------------------------------------------------------------------------
-- Compression policies (compress chunks older than 30 days)
-- ---------------------------------------------------------------------------

-- market_history: segmentby type_id + region_id, orderby date
ALTER TABLE market_history SET (
    timescaledb.compress,
    timescaledb.compress_segmentby = 'type_id, region_id',
    timescaledb.compress_orderby = 'date DESC'
);
SELECT add_compression_policy('market_history', INTERVAL '30 days', if_not_exists => TRUE);

-- market_snapshots: segmentby type_id + region_id, orderby ts
ALTER TABLE market_snapshots SET (
    timescaledb.compress,
    timescaledb.compress_segmentby = 'type_id, region_id',
    timescaledb.compress_orderby = 'ts DESC'
);
SELECT add_compression_policy('market_snapshots', INTERVAL '30 days', if_not_exists => TRUE);

-- killmails: segmentby solar_system_id, orderby kill_time
ALTER TABLE killmails SET (
    timescaledb.compress,
    timescaledb.compress_segmentby = 'solar_system_id',
    timescaledb.compress_orderby = 'kill_time DESC'
);
SELECT add_compression_policy('killmails', INTERVAL '30 days', if_not_exists => TRUE);

-- killmail_items: segmentby type_id, orderby kill_time
ALTER TABLE killmail_items SET (
    timescaledb.compress,
    timescaledb.compress_segmentby = 'type_id',
    timescaledb.compress_orderby = 'kill_time DESC'
);
SELECT add_compression_policy('killmail_items', INTERVAL '30 days', if_not_exists => TRUE);

-- killmail_victims: segmentby ship_type_id, orderby kill_time
ALTER TABLE killmail_victims SET (
    timescaledb.compress,
    timescaledb.compress_segmentby = 'ship_type_id',
    timescaledb.compress_orderby = 'kill_time DESC'
);
SELECT add_compression_policy('killmail_victims', INTERVAL '30 days', if_not_exists => TRUE);

-- daily_destruction: segmentby type_id, orderby date
ALTER TABLE daily_destruction SET (
    timescaledb.compress,
    timescaledb.compress_segmentby = 'type_id',
    timescaledb.compress_orderby = 'date DESC'
);
SELECT add_compression_policy('daily_destruction', INTERVAL '30 days', if_not_exists => TRUE);

-- ---------------------------------------------------------------------------
-- Retention policies (auto-drop old chunks)
-- ---------------------------------------------------------------------------
-- market_history: no retention (daily granularity, small volume, historically valuable)
-- daily_destruction: no retention (pre-aggregated, tiny volume)

-- market_snapshots: 1 year retention (hourly, high volume)
SELECT add_retention_policy('market_snapshots', INTERVAL '1 year', if_not_exists => TRUE);

-- killmails: 1 year retention (analysis uses 180-day windows)
SELECT add_retention_policy('killmails', INTERVAL '1 year', if_not_exists => TRUE);

-- killmail_items: 1 year retention
SELECT add_retention_policy('killmail_items', INTERVAL '1 year', if_not_exists => TRUE);

-- killmail_victims: 1 year retention
SELECT add_retention_policy('killmail_victims', INTERVAL '1 year', if_not_exists => TRUE);
