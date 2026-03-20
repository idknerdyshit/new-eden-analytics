-- Index for killmail detail page lookups (PK is composite killmail_id, kill_time)
CREATE INDEX IF NOT EXISTS idx_killmails_id ON killmails (killmail_id);
