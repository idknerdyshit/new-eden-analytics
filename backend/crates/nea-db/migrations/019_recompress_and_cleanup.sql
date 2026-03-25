-- Recompress all eligible chunks for killmail_attackers and killmail_victims.
-- Migration 016 decompressed everything to change segmentby but the automatic
-- policy only processes new chunks crossing the 30-day threshold.

SELECT compress_chunk(c)
FROM show_chunks('killmail_attackers', older_than => INTERVAL '30 days') c;

SELECT compress_chunk(c)
FROM show_chunks('killmail_victims', older_than => INTERVAL '30 days') c;

-- Drop redundant indexes on killmail_attackers:
--   idx_killmail_attackers_killmail (killmail_id) is subsumed by
--     idx_killmail_attackers_killmail_time_character (killmail_id, kill_time, character_id)
--   idx_attackers_killmail_time (killmail_id, kill_time) is also subsumed by the same index
DROP INDEX IF EXISTS idx_killmail_attackers_killmail;
DROP INDEX IF EXISTS idx_attackers_killmail_time;
