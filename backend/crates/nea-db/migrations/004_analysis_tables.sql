-- Analysis results tables
CREATE TABLE IF NOT EXISTS correlation_results (
    id SERIAL PRIMARY KEY,
    product_type_id INTEGER NOT NULL,
    material_type_id INTEGER NOT NULL,
    lag_days INTEGER NOT NULL,
    correlation_coeff DOUBLE PRECISION NOT NULL,
    granger_f_stat DOUBLE PRECISION,
    granger_p_value DOUBLE PRECISION,
    granger_significant BOOLEAN NOT NULL DEFAULT false,
    window_start DATE NOT NULL,
    window_end DATE NOT NULL,
    computed_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (product_type_id, material_type_id)
);
CREATE INDEX idx_correlation_product ON correlation_results(product_type_id);
CREATE INDEX idx_correlation_material ON correlation_results(material_type_id);
CREATE INDEX idx_correlation_coeff ON correlation_results(correlation_coeff DESC);
CREATE INDEX idx_correlation_significant ON correlation_results(granger_significant) WHERE granger_significant = true;
