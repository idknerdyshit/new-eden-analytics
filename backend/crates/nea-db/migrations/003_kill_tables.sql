-- Kill data tables (TimescaleDB hypertables)
CREATE TABLE IF NOT EXISTS killmails (
    killmail_id BIGINT NOT NULL,
    kill_time TIMESTAMPTZ NOT NULL,
    solar_system_id INTEGER,
    total_value DOUBLE PRECISION,
    r2z2_sequence_id BIGINT,
    PRIMARY KEY (killmail_id, kill_time)
);
SELECT create_hypertable('killmails', 'kill_time', if_not_exists => TRUE);

CREATE TABLE IF NOT EXISTS killmail_items (
    killmail_id BIGINT NOT NULL,
    kill_time TIMESTAMPTZ NOT NULL,
    type_id INTEGER NOT NULL,
    quantity_destroyed BIGINT NOT NULL DEFAULT 0,
    quantity_dropped BIGINT NOT NULL DEFAULT 0
);
SELECT create_hypertable('killmail_items', 'kill_time', if_not_exists => TRUE);
CREATE INDEX idx_killmail_items_type ON killmail_items(type_id, kill_time DESC);
CREATE INDEX idx_killmail_items_killmail ON killmail_items(killmail_id);

CREATE TABLE IF NOT EXISTS killmail_victims (
    killmail_id BIGINT NOT NULL,
    kill_time TIMESTAMPTZ NOT NULL,
    ship_type_id INTEGER NOT NULL
);
SELECT create_hypertable('killmail_victims', 'kill_time', if_not_exists => TRUE);
CREATE INDEX idx_killmail_victims_ship ON killmail_victims(ship_type_id, kill_time DESC);

CREATE TABLE IF NOT EXISTS daily_destruction (
    type_id INTEGER NOT NULL,
    date DATE NOT NULL,
    quantity_destroyed BIGINT NOT NULL,
    kill_count INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (type_id, date)
);
SELECT create_hypertable('daily_destruction', 'date', if_not_exists => TRUE);
CREATE INDEX idx_daily_destruction_type_date ON daily_destruction(type_id, date DESC);

-- Worker state tracking
CREATE TABLE IF NOT EXISTS worker_state (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
