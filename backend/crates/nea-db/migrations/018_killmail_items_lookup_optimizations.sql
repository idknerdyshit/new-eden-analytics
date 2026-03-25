-- Optimize killmail item lookup patterns used by doctrine/profile fitting.
-- Recent lookups benefit from a partial index; older compressed chunks benefit
-- from segmenting by killmail_id instead of type_id.

SELECT decompress_chunk(c, true)
FROM show_chunks('killmail_items') c
WHERE EXISTS (
    SELECT 1
    FROM timescaledb_information.chunks
    WHERE hypertable_name = 'killmail_items'
      AND chunk_name = split_part(c::text, '.', 2)
      AND is_compressed = true
);

ALTER TABLE killmail_items SET (
    timescaledb.compress,
    timescaledb.compress_segmentby = 'killmail_id',
    timescaledb.compress_orderby = 'kill_time DESC'
);

CREATE INDEX IF NOT EXISTS idx_killmail_items_fitted_lookup
    ON killmail_items (killmail_id, kill_time DESC)
    WHERE flag != 0;
