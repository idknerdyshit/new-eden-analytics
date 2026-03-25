-- Support profile solo kill/loss checks that probe all attackers for one killmail.
CREATE INDEX IF NOT EXISTS idx_killmail_attackers_killmail_time_character
    ON killmail_attackers (killmail_id, kill_time, character_id);
