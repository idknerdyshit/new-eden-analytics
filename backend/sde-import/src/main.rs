use std::collections::HashMap;
use std::path::{Path, PathBuf};

use sqlx::PgPool;
use tracing::{info, warn};

const SDE_CACHE_DIR: &str = "/tmp/sde_cache";

const CSV_URLS: &[(&str, &str)] = &[
    ("invTypes.csv", "https://www.fuzzwork.co.uk/dump/latest/invTypes.csv"),
    (
        "industryActivityProducts.csv",
        "https://www.fuzzwork.co.uk/dump/latest/industryActivityProducts.csv",
    ),
    (
        "industryActivityMaterials.csv",
        "https://www.fuzzwork.co.uk/dump/latest/industryActivityMaterials.csv",
    ),
    ("invGroups.csv", "https://www.fuzzwork.co.uk/dump/latest/invGroups.csv"),
    (
        "invCategories.csv",
        "https://www.fuzzwork.co.uk/dump/latest/invCategories.csv",
    ),
];

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    dotenvy::dotenv().ok();

    info!("sde-import starting");

    let database_url =
        std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");

    let pool = nea_db::create_pool(&database_url).await?;
    info!("connected to database");

    nea_db::run_migrations(&pool).await?;
    info!("migrations applied");

    // Download all CSVs
    download_all_csvs().await?;
    info!("all CSVs downloaded / cached");

    // Parse supporting data: groups and categories
    let groups = parse_groups()?;
    info!(count = groups.len(), "parsed invGroups");

    let categories = parse_categories()?;
    info!(count = categories.len(), "parsed invCategories");

    // Import in order: types first, then blueprints, then materials
    let types_count = import_types(&pool, &groups, &categories).await?;
    info!(rows = types_count, "imported sde_types");

    let bp_count = import_blueprints(&pool).await?;
    info!(rows = bp_count, "imported sde_blueprints");

    let mat_count = import_materials(&pool).await?;
    info!(rows = mat_count, "imported sde_blueprint_materials");

    info!(
        types = types_count,
        blueprints = bp_count,
        materials = mat_count,
        "SDE import complete"
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// Download helpers
// ---------------------------------------------------------------------------

async fn download_all_csvs() -> Result<(), Box<dyn std::error::Error>> {
    let cache_dir = Path::new(SDE_CACHE_DIR);
    tokio::fs::create_dir_all(cache_dir).await?;

    let client = reqwest::Client::builder()
        .user_agent("new-eden-analytics (sara@idknerdyshit.com; +https://github.com/idknerdyshit/new-eden-analytics; eve:Eyedeekay)")
        .build()?;

    for (i, (filename, url)) in CSV_URLS.iter().enumerate() {
        let dest = cache_dir.join(filename);
        if dest.exists() {
            info!(file = %filename, "[{}/{}] using cached CSV", i + 1, CSV_URLS.len());
            continue;
        }
        info!(file = %filename, "[{}/{}] downloading CSV (this may take a moment)...", i + 1, CSV_URLS.len());
        let resp = client.get(*url).send().await?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("HTTP {status} downloading {filename}: {body}").into());
        }
        let content_length = resp.content_length();
        if let Some(len) = content_length {
            info!(file = %filename, size_mb = format_args!("{:.1}", len as f64 / 1_048_576.0), "download started");
        }
        let bytes = resp.bytes().await?;
        if bytes.len() < 1000 {
            let preview = String::from_utf8_lossy(&bytes);
            warn!(file = %filename, bytes = bytes.len(), content = %preview, "downloaded file suspiciously small, may not be valid CSV");
            return Err(format!("download of {filename} returned only {} bytes: {preview}", bytes.len()).into());
        }
        tokio::fs::write(&dest, &bytes).await?;
        info!(file = %filename, size_mb = format_args!("{:.1}", bytes.len() as f64 / 1_048_576.0), "download complete");
    }

    Ok(())
}

fn csv_path(filename: &str) -> PathBuf {
    Path::new(SDE_CACHE_DIR).join(filename)
}

fn open_csv(filename: &str) -> Result<csv::Reader<std::fs::File>, Box<dyn std::error::Error>> {
    let rdr = csv::ReaderBuilder::new()
        .flexible(true)
        .from_path(csv_path(filename))?;
    Ok(rdr)
}

// ---------------------------------------------------------------------------
// Groups / categories lookup tables
// ---------------------------------------------------------------------------

/// groupID -> (groupName, categoryID)
fn parse_groups() -> Result<HashMap<i32, (String, i32)>, Box<dyn std::error::Error>> {
    let mut rdr = open_csv("invGroups.csv")?;
    let mut map = HashMap::new();
    for result in rdr.records() {
        let record = result?;
        // columns: groupID, categoryID, groupName, ...
        let group_id: i32 = record.get(0).unwrap_or("0").parse().unwrap_or(0);
        let category_id: i32 = record.get(1).unwrap_or("0").parse().unwrap_or(0);
        let group_name = record.get(2).unwrap_or("").to_string();
        map.insert(group_id, (group_name, category_id));
    }
    Ok(map)
}

/// categoryID -> categoryName
fn parse_categories() -> Result<HashMap<i32, String>, Box<dyn std::error::Error>> {
    let mut rdr = open_csv("invCategories.csv")?;
    let mut map = HashMap::new();
    for result in rdr.records() {
        let record = result?;
        // columns: categoryID, categoryName, ...
        let category_id: i32 = record.get(0).unwrap_or("0").parse().unwrap_or(0);
        let category_name = record.get(1).unwrap_or("").to_string();
        map.insert(category_id, category_name);
    }
    Ok(map)
}

// ---------------------------------------------------------------------------
// Import: sde_types
// ---------------------------------------------------------------------------

struct TypeRow {
    type_id: i32,
    name: String,
    group_id: Option<i32>,
    group_name: Option<String>,
    category_id: Option<i32>,
    category_name: Option<String>,
    market_group_id: Option<i32>,
    volume: Option<f64>,
}

async fn import_types(
    pool: &PgPool,
    groups: &HashMap<i32, (String, i32)>,
    categories: &HashMap<i32, String>,
) -> Result<usize, Box<dyn std::error::Error>> {
    let mut rdr = open_csv("invTypes.csv")?;
    let mut rows: Vec<TypeRow> = Vec::new();

    for result in rdr.records() {
        let record = result?;
        // columns: typeID, groupID, typeName, description, mass, volume, capacity,
        //          portionSize, raceID, basePrice, published, marketGroupID, iconID, soundID, graphicID
        let published_str = record.get(10).unwrap_or("0");
        let published: i32 = published_str.parse().unwrap_or(0);
        if published != 1 {
            continue;
        }

        let type_id: i32 = match record.get(0).unwrap_or("").parse() {
            Ok(v) => v,
            Err(_) => continue,
        };
        let group_id_raw: Option<i32> = record
            .get(1)
            .and_then(|s| if s.is_empty() || s == "None" { None } else { s.parse().ok() });
        let name = record.get(2).unwrap_or("").to_string();
        let volume: Option<f64> = record
            .get(5)
            .and_then(|s| if s.is_empty() || s == "None" { None } else { s.parse().ok() });
        let market_group_id: Option<i32> = record
            .get(11)
            .and_then(|s| if s.is_empty() || s == "None" { None } else { s.parse().ok() });

        // Resolve group -> category chain
        let (group_name, category_id, category_name) = match group_id_raw {
            Some(gid) => match groups.get(&gid) {
                Some((gname, cat_id)) => {
                    let cname = categories.get(cat_id).cloned();
                    (Some(gname.clone()), Some(*cat_id), cname)
                }
                None => (None, None, None),
            },
            None => (None, None, None),
        };

        rows.push(TypeRow {
            type_id,
            name,
            group_id: group_id_raw,
            group_name,
            category_id,
            category_name,
            market_group_id,
            volume,
        });
    }

    let total = rows.len();
    info!(total, "parsed invTypes (published=1)");

    // Batch insert in chunks of 500
    for (i, chunk) in rows.chunks(500).enumerate() {
        insert_types_batch(pool, chunk).await?;
        let done = ((i + 1) * 500).min(total);
        info!(progress = format_args!("{done}/{total}"), "inserting sde_types");
    }

    Ok(total)
}

async fn insert_types_batch(
    pool: &PgPool,
    chunk: &[TypeRow],
) -> Result<(), Box<dyn std::error::Error>> {
    if chunk.is_empty() {
        return Ok(());
    }

    // Build a parameterized query with 9 columns per row
    let cols_per_row = 9;
    let mut query = String::from(
        "INSERT INTO sde_types (type_id, name, group_id, group_name, category_id, category_name, market_group_id, volume, published) VALUES ",
    );

    let mut param_idx = 1u32;
    for (i, _) in chunk.iter().enumerate() {
        if i > 0 {
            query.push_str(", ");
        }
        query.push('(');
        for col in 0..cols_per_row {
            if col > 0 {
                query.push_str(", ");
            }
            query.push('$');
            query.push_str(&param_idx.to_string());
            param_idx += 1;
        }
        query.push(')');
    }

    query.push_str(
        " ON CONFLICT (type_id) DO UPDATE SET \
         name = EXCLUDED.name, \
         group_id = EXCLUDED.group_id, \
         group_name = EXCLUDED.group_name, \
         category_id = EXCLUDED.category_id, \
         category_name = EXCLUDED.category_name, \
         market_group_id = EXCLUDED.market_group_id, \
         volume = EXCLUDED.volume, \
         published = EXCLUDED.published",
    );

    let mut q = sqlx::query(&query);
    for row in chunk {
        q = q
            .bind(row.type_id)
            .bind(&row.name)
            .bind(row.group_id)
            .bind(&row.group_name)
            .bind(row.category_id)
            .bind(&row.category_name)
            .bind(row.market_group_id)
            .bind(row.volume)
            .bind(true); // published is always true since we filtered
    }

    let mut tx = pool.begin().await?;
    q.execute(&mut *tx).await?;
    tx.commit().await?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Import: sde_blueprints
// ---------------------------------------------------------------------------

struct BlueprintRow {
    blueprint_type_id: i32,
    product_type_id: i32,
    quantity: i32,
}

async fn import_blueprints(pool: &PgPool) -> Result<usize, Box<dyn std::error::Error>> {
    let mut rdr = open_csv("industryActivityProducts.csv")?;
    let mut rows: Vec<BlueprintRow> = Vec::new();

    for result in rdr.records() {
        let record = result?;
        // columns: typeID, activityID, productTypeID, quantity
        let activity_id: i32 = record.get(1).unwrap_or("0").parse().unwrap_or(0);
        if activity_id != 1 {
            continue;
        }

        let blueprint_type_id: i32 = match record.get(0).unwrap_or("").parse() {
            Ok(v) => v,
            Err(_) => continue,
        };
        let product_type_id: i32 = match record.get(2).unwrap_or("").parse() {
            Ok(v) => v,
            Err(_) => continue,
        };
        let quantity: i32 = record.get(3).unwrap_or("1").parse().unwrap_or(1);

        rows.push(BlueprintRow {
            blueprint_type_id,
            product_type_id,
            quantity,
        });
    }

    let total = rows.len();
    info!(total, "parsed industryActivityProducts (activityID=1)");

    let mut inserted = 0usize;
    for chunk in rows.chunks(500) {
        match insert_blueprints_batch(pool, chunk).await {
            Ok(n) => inserted += n,
            Err(e) => {
                warn!(error = %e, "blueprint batch insert failed (likely FK violation), falling back to row-by-row");
                inserted += insert_blueprints_individually(pool, chunk).await;
            }
        }
    }

    Ok(inserted)
}

async fn insert_blueprints_batch(
    pool: &PgPool,
    chunk: &[BlueprintRow],
) -> Result<usize, Box<dyn std::error::Error>> {
    if chunk.is_empty() {
        return Ok(0);
    }

    let cols_per_row = 3;
    let mut query = String::from(
        "INSERT INTO sde_blueprints (blueprint_type_id, product_type_id, quantity) VALUES ",
    );

    let mut param_idx = 1u32;
    for (i, _) in chunk.iter().enumerate() {
        if i > 0 {
            query.push_str(", ");
        }
        query.push('(');
        for col in 0..cols_per_row {
            if col > 0 {
                query.push_str(", ");
            }
            query.push('$');
            query.push_str(&param_idx.to_string());
            param_idx += 1;
        }
        query.push(')');
    }

    query.push_str(
        " ON CONFLICT (blueprint_type_id) DO UPDATE SET \
         product_type_id = EXCLUDED.product_type_id, \
         quantity = EXCLUDED.quantity",
    );

    let mut q = sqlx::query(&query);
    for row in chunk {
        q = q
            .bind(row.blueprint_type_id)
            .bind(row.product_type_id)
            .bind(row.quantity);
    }

    let mut tx = pool.begin().await?;
    q.execute(&mut *tx).await?;
    tx.commit().await?;

    Ok(chunk.len())
}

async fn insert_blueprints_individually(pool: &PgPool, chunk: &[BlueprintRow]) -> usize {
    let mut count = 0usize;
    for row in chunk {
        let result = sqlx::query(
            "INSERT INTO sde_blueprints (blueprint_type_id, product_type_id, quantity) \
             VALUES ($1, $2, $3) \
             ON CONFLICT (blueprint_type_id) DO UPDATE SET \
             product_type_id = EXCLUDED.product_type_id, \
             quantity = EXCLUDED.quantity",
        )
        .bind(row.blueprint_type_id)
        .bind(row.product_type_id)
        .bind(row.quantity)
        .execute(pool)
        .await;

        match result {
            Ok(_) => count += 1,
            Err(e) => {
                warn!(
                    blueprint_type_id = row.blueprint_type_id,
                    product_type_id = row.product_type_id,
                    error = %e,
                    "skipping blueprint row (FK violation)"
                );
            }
        }
    }
    count
}

// ---------------------------------------------------------------------------
// Import: sde_blueprint_materials
// ---------------------------------------------------------------------------

struct MaterialRow {
    blueprint_type_id: i32,
    material_type_id: i32,
    quantity: i32,
}

async fn import_materials(pool: &PgPool) -> Result<usize, Box<dyn std::error::Error>> {
    let mut rdr = open_csv("industryActivityMaterials.csv")?;
    let mut rows: Vec<MaterialRow> = Vec::new();

    for result in rdr.records() {
        let record = result?;
        // columns: typeID, activityID, materialTypeID, quantity
        let activity_id: i32 = record.get(1).unwrap_or("0").parse().unwrap_or(0);
        if activity_id != 1 {
            continue;
        }

        let blueprint_type_id: i32 = match record.get(0).unwrap_or("").parse() {
            Ok(v) => v,
            Err(_) => continue,
        };
        let material_type_id: i32 = match record.get(2).unwrap_or("").parse() {
            Ok(v) => v,
            Err(_) => continue,
        };
        let quantity: i32 = record.get(3).unwrap_or("1").parse().unwrap_or(1);

        rows.push(MaterialRow {
            blueprint_type_id,
            material_type_id,
            quantity,
        });
    }

    let total = rows.len();
    info!(total, "parsed industryActivityMaterials (activityID=1)");

    let mut inserted = 0usize;
    for chunk in rows.chunks(500) {
        match insert_materials_batch(pool, chunk).await {
            Ok(n) => inserted += n,
            Err(e) => {
                warn!(error = %e, "materials batch insert failed (likely FK violation), falling back to row-by-row");
                inserted += insert_materials_individually(pool, chunk).await;
            }
        }
    }

    Ok(inserted)
}

async fn insert_materials_batch(
    pool: &PgPool,
    chunk: &[MaterialRow],
) -> Result<usize, Box<dyn std::error::Error>> {
    if chunk.is_empty() {
        return Ok(0);
    }

    let cols_per_row = 3;
    let mut query = String::from(
        "INSERT INTO sde_blueprint_materials (blueprint_type_id, material_type_id, quantity) VALUES ",
    );

    let mut param_idx = 1u32;
    for (i, _) in chunk.iter().enumerate() {
        if i > 0 {
            query.push_str(", ");
        }
        query.push('(');
        for col in 0..cols_per_row {
            if col > 0 {
                query.push_str(", ");
            }
            query.push('$');
            query.push_str(&param_idx.to_string());
            param_idx += 1;
        }
        query.push(')');
    }

    query.push_str(
        " ON CONFLICT (blueprint_type_id, material_type_id) DO UPDATE SET \
         quantity = EXCLUDED.quantity",
    );

    let mut q = sqlx::query(&query);
    for row in chunk {
        q = q
            .bind(row.blueprint_type_id)
            .bind(row.material_type_id)
            .bind(row.quantity);
    }

    let mut tx = pool.begin().await?;
    q.execute(&mut *tx).await?;
    tx.commit().await?;

    Ok(chunk.len())
}

async fn insert_materials_individually(pool: &PgPool, chunk: &[MaterialRow]) -> usize {
    let mut count = 0usize;
    for row in chunk {
        let result = sqlx::query(
            "INSERT INTO sde_blueprint_materials (blueprint_type_id, material_type_id, quantity) \
             VALUES ($1, $2, $3) \
             ON CONFLICT (blueprint_type_id, material_type_id) DO UPDATE SET \
             quantity = EXCLUDED.quantity",
        )
        .bind(row.blueprint_type_id)
        .bind(row.material_type_id)
        .bind(row.quantity)
        .execute(pool)
        .await;

        match result {
            Ok(_) => count += 1,
            Err(e) => {
                warn!(
                    blueprint_type_id = row.blueprint_type_id,
                    material_type_id = row.material_type_id,
                    error = %e,
                    "skipping material row (FK violation)"
                );
            }
        }
    }
    count
}
