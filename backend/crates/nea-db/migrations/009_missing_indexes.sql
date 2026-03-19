CREATE INDEX IF NOT EXISTS idx_market_history_region_type_date
    ON market_history (region_id, type_id, date DESC);

CREATE INDEX IF NOT EXISTS idx_market_snapshots_type_region_ts
    ON market_snapshots (type_id, region_id, ts DESC);

-- Trigram index for character name search (ILIKE %query%)
CREATE EXTENSION IF NOT EXISTS pg_trgm;

CREATE INDEX IF NOT EXISTS idx_characters_name_trgm
    ON characters USING gin (name gin_trgm_ops);
