-- Compression policy for killmail_attackers (missed in 006_retention_policies.sql)
ALTER TABLE killmail_attackers SET (
    timescaledb.compress,
    timescaledb.compress_segmentby = 'character_id',
    timescaledb.compress_orderby = 'kill_time DESC'
);
SELECT add_compression_policy('killmail_attackers', INTERVAL '30 days', if_not_exists => TRUE);

-- Retention policy for killmail_attackers (1 year, matching other kill tables)
SELECT add_retention_policy('killmail_attackers', INTERVAL '1 year', if_not_exists => TRUE);
