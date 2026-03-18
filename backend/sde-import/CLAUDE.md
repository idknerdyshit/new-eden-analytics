# sde-import

One-time utility binary — imports EVE Online static data (SDE) from Fuzzwork CSV exports.

## What It Does

1. Creates DB pool and runs migrations
2. Downloads 5 CSVs from Fuzzwork (cached in `/tmp/sde_cache`):
   - invTypes.csv, industryActivityProducts.csv, industryActivityMaterials.csv, invGroups.csv, invCategories.csv
3. Parses group/category lookup tables
4. Imports item types (with group/category names joined in)
5. Imports blueprints (product ← blueprint mapping)
6. Imports blueprint materials (raw material requirements, manufacturing activity only)

## Usage

```bash
make seed-sde   # or: cargo run -p sde-import
```

Typical runtime: 2–5 minutes (mostly download time). Safe to re-run — uses ON CONFLICT for idempotent inserts.

## Key Functions

- `download_all_csvs()` — downloads with caching, warns if file < 1KB
- `parse_groups()` / `parse_categories()` — build HashMaps for joining
- `import_types()` / `import_blueprints()` / `import_materials()` — CSV parsing + batch insert

## Dependencies

External: tokio, sqlx, csv, reqwest, serde, tracing, dotenvy
Workspace: nea-db
