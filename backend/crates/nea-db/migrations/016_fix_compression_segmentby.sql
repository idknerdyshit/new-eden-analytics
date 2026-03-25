-- Fix compression segmentby for killmail_attackers.
-- Was segmented by character_id only, but corp/alliance queries are the hot path.
-- Decompress, change segmentby to include all entity columns, recompress.

-- Decompress all compressed chunks first
SELECT decompress_chunk(c, true)
FROM show_chunks('killmail_attackers') c
WHERE EXISTS (
    SELECT 1 FROM timescaledb_information.chunks
    WHERE hypertable_name = 'killmail_attackers'
      AND chunk_name = split_part(c::text, '.', 2)
      AND is_compressed = true
);

ALTER TABLE killmail_attackers SET (
    timescaledb.compress,
    timescaledb.compress_segmentby = 'killmail_id',
    timescaledb.compress_orderby = 'kill_time DESC'
);

-- Fix compression segmentby for killmail_victims.
-- Was segmented by ship_type_id, but character/corp/alliance queries are the hot path.
SELECT decompress_chunk(c, true)
FROM show_chunks('killmail_victims') c
WHERE EXISTS (
    SELECT 1 FROM timescaledb_information.chunks
    WHERE hypertable_name = 'killmail_victims'
      AND chunk_name = split_part(c::text, '.', 2)
      AND is_compressed = true
);

ALTER TABLE killmail_victims SET (
    timescaledb.compress,
    timescaledb.compress_segmentby = 'killmail_id',
    timescaledb.compress_orderby = 'kill_time DESC'
);

-- Composite index for the LATERAL attacker_count subquery (killmail_id + kill_time)
CREATE INDEX IF NOT EXISTS idx_attackers_killmail_time
    ON killmail_attackers (killmail_id, kill_time);
