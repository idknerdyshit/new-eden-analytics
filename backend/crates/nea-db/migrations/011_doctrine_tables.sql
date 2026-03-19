-- Corporation/alliance name caches (same pattern as `characters` table)
CREATE TABLE corporations (
    corporation_id BIGINT PRIMARY KEY,
    name TEXT NOT NULL,
    alliance_id BIGINT,
    member_count INTEGER,
    fetched_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX idx_corporations_name_trgm ON corporations USING gin (name gin_trgm_ops);
CREATE INDEX idx_corporations_alliance ON corporations (alliance_id) WHERE alliance_id IS NOT NULL;

CREATE TABLE alliances (
    alliance_id BIGINT PRIMARY KEY,
    name TEXT NOT NULL,
    ticker TEXT,
    fetched_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX idx_alliances_name_trgm ON alliances USING gin (name gin_trgm_ops);

-- Pre-computed doctrine analysis, one row per (entity_type, entity_id, window_days)
CREATE TABLE doctrine_profiles (
    id SERIAL PRIMARY KEY,
    entity_type TEXT NOT NULL,        -- 'corporation' or 'alliance'
    entity_id BIGINT NOT NULL,
    entity_name TEXT NOT NULL,        -- denormalized for fast reads
    window_days INTEGER NOT NULL,     -- 7, 30, or 90
    member_count INTEGER NOT NULL DEFAULT 0,
    total_kills INTEGER NOT NULL DEFAULT 0,
    total_losses INTEGER NOT NULL DEFAULT 0,
    ship_usage JSONB,                 -- [{type_id, name, count, pct}]
    doctrines JSONB,                  -- [{ship_type_id, ship_name, canonical_fit, occurrences, pilot_count, variant_count}]
    ship_trends JSONB,                -- [{type_id, name, current_count, previous_count, change_pct}]
    fleet_comps JSONB,                -- [{ships: [{type_id, name, avg_count}], occurrence_count}]
    computed_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (entity_type, entity_id, window_days)
);
CREATE INDEX idx_doctrine_profiles_entity ON doctrine_profiles (entity_type, entity_id);

-- Indexes on existing hypertables for corp/alliance GROUP BY queries
CREATE INDEX IF NOT EXISTS idx_attackers_corp ON killmail_attackers (corporation_id, kill_time DESC) WHERE corporation_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_attackers_alliance ON killmail_attackers (alliance_id, kill_time DESC) WHERE alliance_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_victims_corp ON killmail_victims (corporation_id, kill_time DESC) WHERE corporation_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_victims_alliance ON killmail_victims (alliance_id, kill_time DESC) WHERE alliance_id IS NOT NULL;
