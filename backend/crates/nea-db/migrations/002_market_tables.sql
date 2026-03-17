-- Market data tables (TimescaleDB hypertables)
CREATE TABLE IF NOT EXISTS market_history (
    type_id INTEGER NOT NULL,
    region_id INTEGER NOT NULL,
    date DATE NOT NULL,
    average DOUBLE PRECISION NOT NULL,
    highest DOUBLE PRECISION NOT NULL,
    lowest DOUBLE PRECISION NOT NULL,
    volume BIGINT NOT NULL,
    order_count INTEGER NOT NULL,
    PRIMARY KEY (type_id, region_id, date)
);
SELECT create_hypertable('market_history', 'date', if_not_exists => TRUE);
CREATE INDEX idx_market_history_type_date ON market_history(type_id, date DESC);

CREATE TABLE IF NOT EXISTS market_snapshots (
    type_id INTEGER NOT NULL,
    region_id INTEGER NOT NULL,
    station_id BIGINT,
    ts TIMESTAMPTZ NOT NULL,
    best_bid DOUBLE PRECISION,
    best_ask DOUBLE PRECISION,
    bid_volume BIGINT,
    ask_volume BIGINT,
    spread DOUBLE PRECISION,
    PRIMARY KEY (type_id, region_id, ts)
);
SELECT create_hypertable('market_snapshots', 'ts', if_not_exists => TRUE);
CREATE INDEX idx_market_snapshots_type_ts ON market_snapshots(type_id, ts DESC);
