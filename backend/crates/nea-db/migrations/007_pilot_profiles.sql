-- Pilot profiles: attacker data, victim identity, item flags, character cache, pre-aggregated profiles

-- Add victim identity columns
ALTER TABLE killmail_victims ADD COLUMN IF NOT EXISTS character_id BIGINT;
ALTER TABLE killmail_victims ADD COLUMN IF NOT EXISTS corporation_id BIGINT;
ALTER TABLE killmail_victims ADD COLUMN IF NOT EXISTS alliance_id BIGINT;
CREATE INDEX IF NOT EXISTS idx_killmail_victims_character
    ON killmail_victims (character_id, kill_time DESC)
    WHERE character_id IS NOT NULL;

-- Add unique constraint on victims so backfill can upsert identity fields
-- TimescaleDB requires the partition column (kill_time) in unique constraints
CREATE UNIQUE INDEX IF NOT EXISTS idx_killmail_victims_unique
    ON killmail_victims (killmail_id, kill_time);

-- Add item flag column (EVE slot position)
ALTER TABLE killmail_items ADD COLUMN IF NOT EXISTS flag INTEGER NOT NULL DEFAULT 0;

-- Add unique constraint on items so backfill can upsert flag
CREATE UNIQUE INDEX IF NOT EXISTS idx_killmail_items_unique
    ON killmail_items (killmail_id, kill_time, type_id, flag);

-- Attacker data (hypertable on kill_time)
CREATE TABLE IF NOT EXISTS killmail_attackers (
    killmail_id BIGINT NOT NULL,
    kill_time TIMESTAMPTZ NOT NULL,
    character_id BIGINT,
    corporation_id BIGINT,
    alliance_id BIGINT,
    ship_type_id INTEGER NOT NULL DEFAULT 0,
    weapon_type_id INTEGER NOT NULL DEFAULT 0,
    damage_done INTEGER NOT NULL DEFAULT 0,
    final_blow BOOLEAN NOT NULL DEFAULT FALSE
);
SELECT create_hypertable('killmail_attackers', 'kill_time', if_not_exists => TRUE);
CREATE INDEX IF NOT EXISTS idx_killmail_attackers_character
    ON killmail_attackers (character_id, kill_time DESC)
    WHERE character_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_killmail_attackers_killmail
    ON killmail_attackers (killmail_id);

-- Character name cache (from ESI)
CREATE TABLE IF NOT EXISTS characters (
    character_id BIGINT PRIMARY KEY,
    name TEXT NOT NULL,
    corporation_id BIGINT,
    alliance_id BIGINT,
    fetched_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Pre-aggregated character profiles (computed by worker)
CREATE TABLE IF NOT EXISTS character_profiles (
    character_id BIGINT PRIMARY KEY,
    total_kills INTEGER NOT NULL DEFAULT 0,
    total_losses INTEGER NOT NULL DEFAULT 0,
    solo_kills INTEGER NOT NULL DEFAULT 0,
    solo_losses INTEGER NOT NULL DEFAULT 0,
    top_ships_flown JSONB,
    top_ships_lost JSONB,
    common_fits JSONB,
    active_period JSONB,
    computed_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
