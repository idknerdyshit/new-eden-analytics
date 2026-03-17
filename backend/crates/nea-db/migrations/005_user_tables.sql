-- User and session tables for EVE SSO auth
CREATE TABLE IF NOT EXISTS users (
    character_id BIGINT PRIMARY KEY,
    character_name TEXT NOT NULL,
    access_token_enc BYTEA,
    refresh_token_enc BYTEA,
    token_expires_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS sessions (
    session_id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    character_id BIGINT NOT NULL REFERENCES users(character_id) ON DELETE CASCADE,
    expires_at TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX idx_sessions_character ON sessions(character_id);
CREATE INDEX idx_sessions_expires ON sessions(expires_at);
