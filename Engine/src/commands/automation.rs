use super::local_db;
use redb::{Database, ReadableTable, TableDefinition};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Manager};

const AUTOMATION_RULES_FILE: &str = "automation_rules.json";
const AUTOMATION_ACTIVITY_DB: &str = "automation_activity.redb";
const RULES_VERSION: u32 = 1;
const ACTIVITY_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("activity");
const MAX_RECIPES: usize = 100;
const MAX_ACTIVITY_ITEMS: usize = 500;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutomationRulesFile {
    pub version: u32,
    pub recipes: Vec<SavedAutomationRecipe>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedAutomationRecipe {
    pub id: String,
    pub name: String,
    pub recipe_id: String,
    pub resize_enabled: bool,
    pub resize_longest_side: u32,
    pub convert_enabled: bool,
    pub convert_format: String,
    pub compress_enabled: bool,
    pub compress_quality: u8,
    pub image_quality: u8,
    pub strip_metadata_enabled: bool,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutomationActivityItem {
    pub id: String,
    pub recipe_name: String,
    pub level: String,
    pub started_at: i64,
    pub finished_at: i64,
    pub input_count: u32,
    pub success_count: u32,
    pub failed_count: u32,
    pub cancelled: bool,
    pub output_dir: Option<String>,
    pub message: String,
}

fn automation_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Cannot resolve app data directory: {e}"))?
        .join("automation");

    fs::create_dir_all(&dir).map_err(|e| format!("Cannot create automation directory: {e}"))?;
    Ok(dir)
}

fn rules_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(automation_dir(app)?.join(AUTOMATION_RULES_FILE))
}

fn rules_db_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(local_db::database_path_for_dir(&automation_dir(app)?))
}

fn activity_db_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(automation_dir(app)?.join(AUTOMATION_ACTIVITY_DB))
}

fn default_rules_file() -> AutomationRulesFile {
    AutomationRulesFile {
        version: RULES_VERSION,
        recipes: Vec::new(),
    }
}

fn read_rules_file(app: &AppHandle) -> Result<AutomationRulesFile, String> {
    let db_path = rules_db_path(app)?;
    if let Some(mut parsed) =
        local_db::read_json::<AutomationRulesFile>(&db_path, AUTOMATION_RULES_FILE)?
    {
        if parsed.version == 0 {
            parsed.version = RULES_VERSION;
        }
        parsed.recipes.truncate(MAX_RECIPES);
        return Ok(parsed);
    }

    let path = rules_path(app)?;
    if !path.exists() {
        let defaults = default_rules_file();
        write_rules_file(app, &defaults)?;
        return Ok(defaults);
    }

    let raw =
        fs::read_to_string(&path).map_err(|e| format!("Cannot read automation rules: {e}"))?;
    let mut parsed: AutomationRulesFile = serde_json::from_str(&raw).map_err(|e| {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let corrupt = path.with_extension(format!("corrupt.{stamp}.json"));
        let _ = fs::rename(&path, &corrupt);
        format!("Automation rules were corrupted and moved aside: {e}")
    })?;

    if parsed.version == 0 {
        parsed.version = RULES_VERSION;
    }

    parsed.recipes.truncate(MAX_RECIPES);
    local_db::write_json(&db_path, AUTOMATION_RULES_FILE, &parsed)?;
    Ok(parsed)
}

fn write_rules_file(app: &AppHandle, rules: &AutomationRulesFile) -> Result<(), String> {
    let db_path = rules_db_path(app)?;
    let mut cleaned = rules.clone();
    cleaned.version = RULES_VERSION;
    cleaned.recipes.truncate(MAX_RECIPES);
    local_db::write_json(&db_path, AUTOMATION_RULES_FILE, &cleaned)
}

static ACTIVITY_DB: std::sync::OnceLock<std::sync::Mutex<Option<Database>>> =
    std::sync::OnceLock::new();

fn with_activity_db<T>(
    app: &AppHandle,
    f: impl FnOnce(&Database) -> Result<T, String>,
) -> Result<T, String> {
    let cell = ACTIVITY_DB.get_or_init(|| std::sync::Mutex::new(None));
    let mut guard = cell
        .lock()
        .map_err(|_| "Automation activity database lock was poisoned".to_string())?;

    if guard.is_none() {
        let path = activity_db_path(app)?;
        let db = Database::create(path)
            .map_err(|e| format!("Cannot open automation activity database: {e}"))?;
        ensure_activity_table(&db)?;
        *guard = Some(db);
    }

    let db = guard
        .as_ref()
        .ok_or_else(|| "Automation activity database is unavailable".to_string())?;

    f(db)
}

fn ensure_activity_table(db: &Database) -> Result<(), String> {
    let write_txn = db
        .begin_write()
        .map_err(|e| format!("Cannot initialize activity database: {e}"))?;
    {
        let _table = write_txn
            .open_table(ACTIVITY_TABLE)
            .map_err(|e| format!("Cannot initialize activity table: {e}"))?;
    }
    write_txn
        .commit()
        .map_err(|e| format!("Cannot commit activity table initialization: {e}"))?;
    Ok(())
}

fn read_activity_items_db(
    db: &Database,
    limit: usize,
) -> Result<Vec<AutomationActivityItem>, String> {
    let read_txn = db
        .begin_read()
        .map_err(|e| format!("Cannot read activity database: {e}"))?;
    let table = read_txn
        .open_table(ACTIVITY_TABLE)
        .map_err(|e| format!("Cannot open activity table: {e}"))?;

    let mut items = Vec::new();
    for entry in table
        .iter()
        .map_err(|e| format!("Cannot iterate activity table: {e}"))?
    {
        let (_key, value) = entry.map_err(|e| format!("Cannot read activity item: {e}"))?;
        if let Ok(item) = serde_json::from_slice::<AutomationActivityItem>(value.value()) {
            items.push(item);
        }
    }

    items.sort_by(|a, b| {
        b.finished_at
            .cmp(&a.finished_at)
            .then_with(|| b.started_at.cmp(&a.started_at))
    });
    items.truncate(limit);
    Ok(items)
}

fn trim_activity_log_db(db: &Database) -> Result<(), String> {
    let read_txn = db
        .begin_read()
        .map_err(|e| format!("Cannot read activity database: {e}"))?;
    let table = read_txn
        .open_table(ACTIVITY_TABLE)
        .map_err(|e| format!("Cannot open activity table: {e}"))?;

    let mut keys: Vec<(String, i64)> = Vec::new();
    for entry in table
        .iter()
        .map_err(|e| format!("Cannot iterate activity table: {e}"))?
    {
        let (key, value) = entry.map_err(|e| format!("Cannot read activity item: {e}"))?;
        let finished_at = serde_json::from_slice::<AutomationActivityItem>(value.value())
            .map(|item| item.finished_at)
            .unwrap_or(0);
        keys.push((key.value().to_string(), finished_at));
    }
    drop(table);
    drop(read_txn);

    if keys.len() <= MAX_ACTIVITY_ITEMS {
        return Ok(());
    }

    keys.sort_by(|a, b| b.1.cmp(&a.1));
    let stale: Vec<String> = keys
        .into_iter()
        .skip(MAX_ACTIVITY_ITEMS)
        .map(|(key, _)| key)
        .collect();

    let write_txn = db
        .begin_write()
        .map_err(|e| format!("Cannot trim activity database: {e}"))?;
    {
        let mut table = write_txn
            .open_table(ACTIVITY_TABLE)
            .map_err(|e| format!("Cannot open activity table: {e}"))?;
        for key in stale {
            let _ = table.remove(key.as_str());
        }
    }
    write_txn
        .commit()
        .map_err(|e| format!("Cannot commit activity trim: {e}"))?;
    Ok(())
}

fn clear_activity_db(db: &Database) -> Result<(), String> {
    let read_txn = db
        .begin_read()
        .map_err(|e| format!("Cannot read activity database: {e}"))?;
    let table = read_txn
        .open_table(ACTIVITY_TABLE)
        .map_err(|e| format!("Cannot open activity table: {e}"))?;
    let mut keys = Vec::new();
    for entry in table
        .iter()
        .map_err(|e| format!("Cannot iterate activity table: {e}"))?
    {
        let (key, _value) = entry.map_err(|e| format!("Cannot read activity key: {e}"))?;
        keys.push(key.value().to_string());
    }
    drop(table);
    drop(read_txn);

    let write_txn = db
        .begin_write()
        .map_err(|e| format!("Cannot clear activity database: {e}"))?;
    {
        let mut table = write_txn
            .open_table(ACTIVITY_TABLE)
            .map_err(|e| format!("Cannot open activity table: {e}"))?;
        for key in keys {
            let _ = table.remove(key.as_str());
        }
    }
    write_txn
        .commit()
        .map_err(|e| format!("Cannot commit activity clear: {e}"))?;
    Ok(())
}

#[tauri::command]
pub fn get_automation_recipes(app: AppHandle) -> Result<Vec<SavedAutomationRecipe>, String> {
    Ok(read_rules_file(&app)?.recipes)
}

#[tauri::command]
pub fn save_automation_recipes(
    app: AppHandle,
    recipes: Vec<SavedAutomationRecipe>,
) -> Result<Vec<SavedAutomationRecipe>, String> {
    let mut cleaned = recipes;
    cleaned.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    cleaned.truncate(MAX_RECIPES);
    let rules = AutomationRulesFile {
        version: RULES_VERSION,
        recipes: cleaned.clone(),
    };
    write_rules_file(&app, &rules)?;
    Ok(cleaned)
}

#[tauri::command]
pub fn get_automation_activity(app: AppHandle) -> Result<Vec<AutomationActivityItem>, String> {
    with_activity_db(&app, |db| read_activity_items_db(db, 80))
}

#[tauri::command]
pub fn add_automation_activity(
    app: AppHandle,
    item: AutomationActivityItem,
) -> Result<Vec<AutomationActivityItem>, String> {
    with_activity_db(&app, |db| {
        let key = format!("{:013}_{}", item.finished_at.max(item.started_at), item.id);
        let bytes = serde_json::to_vec(&item)
            .map_err(|e| format!("Cannot serialize activity item: {e}"))?;

        let write_txn = db
            .begin_write()
            .map_err(|e| format!("Cannot write activity database: {e}"))?;
        {
            let mut table = write_txn
                .open_table(ACTIVITY_TABLE)
                .map_err(|e| format!("Cannot open activity table: {e}"))?;
            table
                .insert(key.as_str(), bytes.as_slice())
                .map_err(|e| format!("Cannot insert activity item: {e}"))?;
        }
        write_txn
            .commit()
            .map_err(|e| format!("Cannot commit activity item: {e}"))?;

        let _ = trim_activity_log_db(db);
        read_activity_items_db(db, 80)
    })
}

#[tauri::command]
pub fn clear_automation_activity(app: AppHandle) -> Result<(), String> {
    with_activity_db(&app, clear_activity_db)
}

#[tauri::command]
pub fn open_automation_data_folder(app: AppHandle) -> Result<String, String> {
    let dir = automation_dir(&app)?;
    Ok(dir.to_string_lossy().to_string())
}
