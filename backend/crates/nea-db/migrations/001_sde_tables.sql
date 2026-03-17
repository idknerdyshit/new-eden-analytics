-- SDE static data tables
CREATE TABLE IF NOT EXISTS sde_types (
    type_id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    group_id INTEGER,
    group_name TEXT,
    category_id INTEGER,
    category_name TEXT,
    market_group_id INTEGER,
    volume DOUBLE PRECISION,
    published BOOLEAN NOT NULL DEFAULT true
);

-- GIN index for full-text search
CREATE INDEX idx_sde_types_name_gin ON sde_types USING gin(to_tsvector('english', name));
CREATE INDEX idx_sde_types_category ON sde_types(category_id);
CREATE INDEX idx_sde_types_group ON sde_types(group_id);

CREATE TABLE IF NOT EXISTS sde_blueprints (
    blueprint_type_id INTEGER PRIMARY KEY REFERENCES sde_types(type_id),
    product_type_id INTEGER NOT NULL REFERENCES sde_types(type_id),
    quantity INTEGER NOT NULL DEFAULT 1
);
CREATE INDEX idx_sde_blueprints_product ON sde_blueprints(product_type_id);

CREATE TABLE IF NOT EXISTS sde_blueprint_materials (
    blueprint_type_id INTEGER NOT NULL REFERENCES sde_types(type_id),
    material_type_id INTEGER NOT NULL REFERENCES sde_types(type_id),
    quantity INTEGER NOT NULL,
    PRIMARY KEY (blueprint_type_id, material_type_id)
);
CREATE INDEX idx_sde_bp_materials_material ON sde_blueprint_materials(material_type_id);

-- Convenience view: product -> materials (skipping blueprint indirection)
CREATE VIEW v_product_materials AS
SELECT
    bp.product_type_id,
    pt.name AS product_name,
    bm.material_type_id,
    mt.name AS material_name,
    bm.quantity
FROM sde_blueprints bp
JOIN sde_blueprint_materials bm ON bp.blueprint_type_id = bm.blueprint_type_id
JOIN sde_types pt ON bp.product_type_id = pt.type_id
JOIN sde_types mt ON bm.material_type_id = mt.type_id;
