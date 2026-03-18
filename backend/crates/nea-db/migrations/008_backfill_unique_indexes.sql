-- Unique indexes on killmail_victims and killmail_items to enable upserts during backfill.
-- TimescaleDB requires the partition column (kill_time) in unique constraints.

CREATE UNIQUE INDEX IF NOT EXISTS idx_killmail_victims_unique
    ON killmail_victims (killmail_id, kill_time);

CREATE UNIQUE INDEX IF NOT EXISTS idx_killmail_items_unique
    ON killmail_items (killmail_id, kill_time, type_id, flag);
