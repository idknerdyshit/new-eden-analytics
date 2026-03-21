-- 005_user_tables.sql created idx_sessions_expires on (expires_at)
-- 010_session_retention.sql created idx_sessions_expires_at on (expires_at)
-- These are duplicates; drop the newer one.
DROP INDEX IF EXISTS idx_sessions_expires_at;
