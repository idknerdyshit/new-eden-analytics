-- Track item types that ESI reports as not tradable on the market.
-- These are excluded from future market history and order fetches.
CREATE TABLE IF NOT EXISTS non_tradable_types (
    type_id INTEGER PRIMARY KEY REFERENCES sde_types(type_id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
