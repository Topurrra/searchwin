use calamine::{open_workbook_auto, Data, Reader};
use rust_xlsxwriter::{Format, FormatBorder, Workbook};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};
use tauri::{AppHandle, Emitter};
use walkdir::WalkDir;

static CANCELLED_SPREADSHEET: LazyLock<Mutex<HashSet<String>>> =
    LazyLock::new(|| Mutex::new(HashSet::new()));

#[derive(Deserialize)]
pub struct CsvSourcesOptions {
    pub paths: Vec<String>,
    #[serde(default)]
    pub recursive: bool,
}

#[derive(Serialize)]
pub struct CsvSourcesResult {
    pub paths: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Serialize)]
pub struct SpreadsheetSourcesResult {
    pub paths: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Serialize, Clone)]
pub struct SpreadsheetProgressEvent {
    pub operation_id: String,
    pub mode: String,
    pub current_item: String,
    pub items_done: usize,
    pub items_total: usize,
    pub rows_done: usize,
    pub cancelled: bool,
}

#[tauri::command]
pub fn cancel_spreadsheet_operation(operation_id: String) -> Result<(), String> {
    CANCELLED_SPREADSHEET
        .lock()
        .map_err(|_| "Cancel registry lock failed".to_string())?
        .insert(operation_id);
    Ok(())
}

fn spreadsheet_cancelled(operation_id: &Option<String>) -> bool {
    operation_id
        .as_ref()
        .and_then(|id| {
            CANCELLED_SPREADSHEET
                .lock()
                .ok()
                .map(|set| set.contains(id))
        })
        .unwrap_or(false)
}

fn clear_spreadsheet_cancel(operation_id: &Option<String>) {
    if let Some(id) = operation_id {
        if let Ok(mut set) = CANCELLED_SPREADSHEET.lock() {
            set.remove(id);
        }
    }
}

fn emit_spreadsheet_progress(
    app: &AppHandle,
    operation_id: &Option<String>,
    mode: &str,
    current_item: String,
    items_done: usize,
    items_total: usize,
    rows_done: usize,
    cancelled: bool,
) {
    if let Some(operation_id) = operation_id {
        let _ = app.emit(
            "spreadsheet-progress",
            SpreadsheetProgressEvent {
                operation_id: operation_id.clone(),
                mode: mode.to_string(),
                current_item,
                items_done,
                items_total,
                rows_done,
                cancelled,
            },
        );
    }
}

fn is_csv_like_path(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| {
            let ext = ext.to_ascii_lowercase();
            ext == "csv" || ext == "tsv" || ext == "txt"
        })
}

fn is_excel_like_path(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| {
            let ext = ext.to_ascii_lowercase();
            ext == "xlsx" || ext == "xls" || ext == "xlsb" || ext == "xlsm" || ext == "ods"
        })
}

fn normalize_path_key(path: &str) -> String {
    path.to_lowercase()
}

fn collect_csv_paths_from_dir(
    dir: &Path,
    recursive: bool,
    paths: &mut Vec<String>,
    seen: &mut HashSet<String>,
    warnings: &mut Vec<String>,
) {
    if recursive {
        for entry in WalkDir::new(dir) {
            let entry = match entry {
                Ok(entry) => entry,
                Err(err) => {
                    warnings.push(format!(
                        "Skip unreadable entry in '{}': {err}",
                        dir.display()
                    ));
                    continue;
                }
            };

            if !entry.file_type().is_file() {
                continue;
            }

            let entry_path = entry.path();
            if is_csv_like_path(entry_path) {
                let key = entry_path.to_string_lossy().to_lowercase();
                if seen.insert(key) {
                    paths.push(entry_path.to_string_lossy().into());
                }
            }
        }
        return;
    }

    let read_dir = match fs::read_dir(dir) {
        Ok(items) => items,
        Err(err) => {
            warnings.push(format!("Cannot list '{}': {err}", dir.display()));
            return;
        }
    };

    for item in read_dir {
        let item = match item {
            Ok(item) => item,
            Err(err) => {
                warnings.push(format!(
                    "Skip unreadable item in '{}': {err}",
                    dir.display()
                ));
                continue;
            }
        };

        let item_path = item.path();
        if item.file_type().map(|ty| ty.is_file()).unwrap_or(false) && is_csv_like_path(&item_path)
        {
            let key = item_path.to_string_lossy().to_lowercase();
            if seen.insert(key) {
                paths.push(item_path.to_string_lossy().into());
            }
        }
    }
}

fn collect_excel_paths_from_dir(
    dir: &Path,
    recursive: bool,
    paths: &mut Vec<String>,
    seen: &mut HashSet<String>,
    warnings: &mut Vec<String>,
) {
    if recursive {
        for entry in WalkDir::new(dir) {
            let entry = match entry {
                Ok(entry) => entry,
                Err(err) => {
                    warnings.push(format!(
                        "Skip unreadable entry in '{}': {err}",
                        dir.display()
                    ));
                    continue;
                }
            };

            if !entry.file_type().is_file() {
                continue;
            }

            let entry_path = entry.path();
            if is_excel_like_path(entry_path) {
                let key = normalize_path_key(&entry_path.to_string_lossy());
                if seen.insert(key) {
                    paths.push(entry_path.to_string_lossy().into());
                }
            }
        }
        return;
    }

    let read_dir = match fs::read_dir(dir) {
        Ok(items) => items,
        Err(err) => {
            warnings.push(format!("Cannot list '{}': {err}", dir.display()));
            return;
        }
    };

    for item in read_dir {
        let item = match item {
            Ok(item) => item,
            Err(err) => {
                warnings.push(format!(
                    "Skip unreadable item in '{}': {err}",
                    dir.display()
                ));
                continue;
            }
        };

        let item_path = item.path();
        if item.file_type().map(|ty| ty.is_file()).unwrap_or(false)
            && is_excel_like_path(&item_path)
        {
            let key = normalize_path_key(&item_path.to_string_lossy());
            if seen.insert(key) {
                paths.push(item_path.to_string_lossy().into());
            }
        }
    }
}

#[tauri::command]
pub fn collect_excel_sources(
    options: CsvSourcesOptions,
) -> Result<SpreadsheetSourcesResult, String> {
    // Security gate: every input path the user supplied must be real +
    // non-system before we walk it.
    for path in &options.paths {
        crate::core::safe_path::validate_user_path(path)?;
    }

    let mut paths: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut warnings: Vec<String> = Vec::new();

    if options.paths.is_empty() {
        return Ok(SpreadsheetSourcesResult { paths, warnings });
    }

    for root in options.paths {
        let path = Path::new(&root);
        if !path.exists() {
            warnings.push(format!("Path does not exist: {root}"));
            continue;
        }

        if path.is_file() {
            if is_excel_like_path(path) {
                let key = normalize_path_key(&path.to_string_lossy());
                if seen.insert(key) {
                    paths.push(path.to_string_lossy().into());
                }
            } else {
                warnings.push(format!("Ignored non-spreadsheet file: {}", path.display()));
            }
            continue;
        }

        if path.is_dir() {
            collect_excel_paths_from_dir(
                path,
                options.recursive,
                &mut paths,
                &mut seen,
                &mut warnings,
            );
            continue;
        }

        warnings.push(format!("Unsupported path type: {}", path.display()));
    }

    paths.sort();
    Ok(SpreadsheetSourcesResult { paths, warnings })
}

#[tauri::command(async)]
pub fn collect_csv_sources(options: CsvSourcesOptions) -> Result<CsvSourcesResult, String> {
    // Security gate: every input path the user supplied must be real +
    // non-system before we walk it.
    for path in &options.paths {
        crate::core::safe_path::validate_user_path(path)?;
    }

    let mut paths: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut warnings: Vec<String> = Vec::new();

    if options.paths.is_empty() {
        return Ok(CsvSourcesResult { paths, warnings });
    }

    for root in options.paths {
        let path = Path::new(&root);
        if !path.exists() {
            warnings.push(format!("Path does not exist: {root}"));
            continue;
        }

        if path.is_file() {
            if is_csv_like_path(path) {
                let key = path.to_string_lossy().to_lowercase();
                if seen.insert(key) {
                    paths.push(path.to_string_lossy().into());
                }
            } else {
                warnings.push(format!("Ignored non-CSV file: {}", path.display()));
            }
            continue;
        }

        if path.is_dir() {
            collect_csv_paths_from_dir(
                path,
                options.recursive,
                &mut paths,
                &mut seen,
                &mut warnings,
            );
            continue;
        }

        warnings.push(format!("Unsupported path type: {}", path.display()));
    }

    paths.sort();
    Ok(CsvSourcesResult { paths, warnings })
}

#[derive(Deserialize)]
pub struct InspectSpreadsheetOptions {
    pub path: String,
}

#[derive(Serialize, Clone)]
pub struct SheetInfo {
    pub name: String,
    pub rows: usize,
    pub cols: usize,
}

#[derive(Serialize, Clone)]
pub struct SpreadsheetInfo {
    pub format: String,
    pub sheets: Vec<SheetInfo>,
    pub preview_rows: Vec<Vec<String>>,
    pub preview_sheet: String,
}

#[tauri::command]
pub fn inspect_spreadsheet(options: InspectSpreadsheetOptions) -> Result<SpreadsheetInfo, String> {
    // Security gate: the spreadsheet we're about to inspect must be a
    // real, non-system file.
    crate::core::safe_path::validate_user_path(&options.path)?;

    let path = PathBuf::from(&options.path);
    let extension = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_lowercase())
        .unwrap_or_default();

    if extension == "csv" || extension == "tsv" {
        return inspect_csv(&path, &extension);
    }

    let mut workbook =
        open_workbook_auto(&path).map_err(|e| format!("Cannot open spreadsheet: {}", e))?;

    let sheet_names = workbook.sheet_names();
    let mut sheets: Vec<SheetInfo> = Vec::new();

    for name in &sheet_names {
        if let Ok(range) = workbook.worksheet_range(name) {
            sheets.push(SheetInfo {
                name: name.clone(),
                rows: range.height(),
                cols: range.width(),
            });
        }
    }

    let preview_sheet = sheet_names.first().cloned().unwrap_or_default();
    let mut preview_rows: Vec<Vec<String>> = Vec::new();
    if !preview_sheet.is_empty() {
        if let Ok(range) = workbook.worksheet_range(&preview_sheet) {
            for (i, row) in range.rows().enumerate() {
                if i >= 20 {
                    break;
                }
                preview_rows.push(row.iter().map(cell_to_string).collect());
            }
        }
    }

    Ok(SpreadsheetInfo {
        format: extension,
        sheets,
        preview_rows,
        preview_sheet,
    })
}

fn inspect_csv(path: &Path, ext: &str) -> Result<SpreadsheetInfo, String> {
    let delimiter = if ext == "tsv" { b'\t' } else { b',' };
    let file = fs::File::open(path).map_err(|e| format!("Cannot open: {}", e))?;
    let mut rdr = csv::ReaderBuilder::new()
        .has_headers(false)
        .delimiter(delimiter)
        .flexible(true)
        .from_reader(file);

    let mut preview_rows: Vec<Vec<String>> = Vec::new();
    let mut total_rows = 0usize;
    let mut max_cols = 0usize;

    for result in rdr.records() {
        let rec = result.map_err(|e| format!("CSV parse: {}", e))?;
        let row: Vec<String> = rec.iter().map(|s| s.to_string()).collect();
        max_cols = max_cols.max(row.len());
        if total_rows < 50 {
            preview_rows.push(row);
        }
        total_rows += 1;
    }

    let sheet_name = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("data")
        .to_string();

    Ok(SpreadsheetInfo {
        format: ext.to_string(),
        sheets: vec![SheetInfo {
            name: sheet_name.clone(),
            rows: total_rows,
            cols: max_cols,
        }],
        preview_rows,
        preview_sheet: sheet_name,
    })
}

fn cell_to_string(c: &Data) -> String {
    match c {
        Data::Empty => String::new(),
        Data::String(s) => s.clone(),
        Data::Float(f) => {
            if f.fract() == 0.0 && f.abs() < 1e15 {
                format!("{}", *f as i64)
            } else {
                format!("{}", f)
            }
        }
        Data::Int(i) => i.to_string(),
        Data::Bool(b) => b.to_string(),
        Data::DateTime(d) => d.to_string(),
        Data::DateTimeIso(s) => s.clone(),
        Data::DurationIso(s) => s.clone(),
        Data::Error(e) => format!("#{:?}", e),
    }
}

// =================== CSV Cleaner ===================

#[derive(Deserialize, Clone)]
pub struct CsvCleanupOptions {
    pub source_path: String,
    pub input_delimiter: String, // "comma" | "tab" | "semicolon" | "pipe" | "auto"
    pub has_header: bool,
    pub trim_cells: bool,
    pub remove_empty_rows: bool,
    pub remove_empty_columns: bool,
    pub normalize_headers: bool,
    pub output_delimiter: String, // "comma" | "tab" | "semicolon" | "pipe"
}

#[derive(Deserialize)]
pub struct CsvCleanupPreviewOptions {
    pub options: CsvCleanupOptions,
}

#[derive(Deserialize)]
pub struct CleanCsvOptions {
    pub options: CsvCleanupOptions,
    pub output_path: String,
    #[serde(default)]
    pub item_index: usize,
    #[serde(default)]
    pub items_total: usize,
    #[serde(default)]
    pub operation_id: Option<String>,
}

#[derive(Serialize, Clone)]
pub struct CsvCleanupPreviewResult {
    pub source_path: String,
    pub detected_delimiter: String,
    pub original_rows: usize,
    pub cleaned_rows: usize,
    pub original_columns: usize,
    pub cleaned_columns: usize,
    pub removed_empty_rows: usize,
    pub removed_empty_columns: usize,
    pub header_changed: bool,
    pub preview_rows: Vec<Vec<String>>,
}

#[derive(Serialize, Clone)]
pub struct CleanCsvResult {
    pub output_path: String,
    pub original_rows: usize,
    pub cleaned_rows: usize,
    pub original_columns: usize,
    pub cleaned_columns: usize,
    pub removed_empty_rows: usize,
    pub removed_empty_columns: usize,
}

struct CsvCleanupComputed {
    detected_delimiter: u8,
    original_rows: usize,
    cleaned_rows: usize,
    original_columns: usize,
    cleaned_columns: usize,
    removed_empty_rows: usize,
    removed_empty_columns: usize,
    header_changed: bool,
    rows: Vec<Vec<String>>,
}

#[tauri::command(async)]
pub fn preview_csv_cleanup(
    options: CsvCleanupPreviewOptions,
) -> Result<CsvCleanupPreviewResult, String> {
    // Security gate: the CSV we'll inspect must resolve to a real,
    // non-system file.
    crate::core::safe_path::validate_user_path(&options.options.source_path)?;

    let computed = compute_csv_cleanup_preview(&options.options, 25)?;
    Ok(CsvCleanupPreviewResult {
        source_path: options.options.source_path,
        detected_delimiter: delimiter_name(computed.detected_delimiter).into(),
        original_rows: computed.original_rows,
        cleaned_rows: computed.cleaned_rows,
        original_columns: computed.original_columns,
        cleaned_columns: computed.cleaned_columns,
        removed_empty_rows: computed.removed_empty_rows,
        removed_empty_columns: computed.removed_empty_columns,
        header_changed: computed.header_changed,
        preview_rows: computed.rows.into_iter().take(25).collect(),
    })
}

#[tauri::command(async)]
pub fn clean_csv_file(app: AppHandle, options: CleanCsvOptions) -> Result<CleanCsvResult, String> {
    // Security gate: source CSV must be real + non-system; the output
    // file must not land in a forbidden location.
    crate::core::safe_path::validate_user_path(&options.options.source_path)?;
    crate::core::safe_path::validate_user_write_target(&options.output_path)?;

    clear_spreadsheet_cancel(&options.operation_id);
    let source_path = options.options.source_path.clone();
    let output_path = PathBuf::from(&options.output_path);
    let items_total = if options.items_total == 0 {
        1
    } else {
        options.items_total
    };
    let item_index = options.item_index.min(items_total.saturating_sub(1));

    let detected = delimiter_byte(
        &options.options.input_delimiter,
        detect_delimiter(&Path::new(&source_path)).unwrap_or(b','),
    )?;

    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("Cannot create output directory: {e}"))?;
    }

    emit_spreadsheet_progress(
        &app,
        &options.operation_id,
        "csv_cleanup",
        source_path.clone(),
        item_index,
        items_total,
        0,
        false,
    );

    if spreadsheet_cancelled(&options.operation_id) {
        clear_spreadsheet_cancel(&options.operation_id);
        return Err("Cancelled".into());
    }

    let output_delimiter = delimiter_byte(&options.options.output_delimiter, detected)?;

    let result = if options.options.remove_empty_columns {
        clean_csv_file_with_column_removal(
            &app,
            &options.options,
            &Path::new(&source_path),
            &output_path,
            detected,
            output_delimiter,
            item_index,
            items_total,
            &options.operation_id,
        )?
    } else {
        clean_csv_file_without_column_removal(
            &app,
            &options.options,
            &Path::new(&source_path),
            &output_path,
            detected,
            output_delimiter,
            item_index,
            items_total,
            &options.operation_id,
        )?
    };

    clear_spreadsheet_cancel(&options.operation_id);
    Ok(result)
}

fn cleanup_csv_row(record: &csv::StringRecord, trim_cells: bool) -> Vec<String> {
    let mut row: Vec<String> = record.iter().map(|value| value.to_string()).collect();
    if trim_cells {
        for value in &mut row {
            *value = value.trim().to_string();
        }
    }
    row
}

fn is_row_empty(row: &[String]) -> bool {
    !row.iter().any(|value| !value.trim().is_empty())
}

fn clean_csv_file_without_column_removal(
    app: &AppHandle,
    options: &CsvCleanupOptions,
    source_path: &Path,
    output_path: &Path,
    input_delimiter: u8,
    output_delimiter: u8,
    item_index: usize,
    items_total: usize,
    operation_id: &Option<String>,
) -> Result<CleanCsvResult, String> {
    let file = fs::File::open(source_path).map_err(|e| format!("Cannot open CSV: {e}"))?;
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(false)
        .delimiter(input_delimiter)
        .flexible(true)
        .from_reader(file);

    let mut writer = csv::WriterBuilder::new()
        .delimiter(output_delimiter)
        .flexible(true)
        .from_path(output_path)
        .map_err(|e| format!("Cannot create output CSV: {e}"))?;

    let mut original_rows = 0usize;
    let mut cleaned_rows = 0usize;
    let mut original_columns = 0usize;
    let mut removed_empty_rows = 0usize;
    let mut cleaned_columns = 0usize;
    let mut header_normalized = false;

    let mut records = reader.records();
    let mut rows_done = 0usize;
    while let Some(record) = records.next() {
        if rows_done.is_multiple_of(256) && spreadsheet_cancelled(operation_id) {
            clear_spreadsheet_cancel(operation_id);
            return Err("Cancelled".into());
        }

        let record = record.map_err(|e| format!("CSV parse failed: {e}"))?;
        let row = cleanup_csv_row(&record, options.trim_cells);
        original_rows += 1;
        original_columns = original_columns.max(row.len());

        if options.remove_empty_rows && is_row_empty(&row) {
            removed_empty_rows += 1;
            continue;
        }

        let mut output_row = row;
        if options.has_header && options.normalize_headers && !header_normalized {
            let normalized = normalize_headers(&output_row);
            output_row = normalized;
            header_normalized = true;
        }

        cleaned_columns = cleaned_columns.max(output_row.len());
        cleaned_rows += 1;
        rows_done = cleaned_rows;
        writer
            .write_record(&output_row)
            .map_err(|e| format!("Cannot write output CSV: {e}"))?;

        if rows_done.is_multiple_of(256) {
            emit_spreadsheet_progress(
                app,
                operation_id,
                "csv_cleanup",
                source_path.to_string_lossy().into(),
                item_index,
                items_total,
                rows_done,
                false,
            );
        }
    }

    writer
        .flush()
        .map_err(|e| format!("Cannot flush output CSV: {e}"))?;

    emit_spreadsheet_progress(
        app,
        operation_id,
        "csv_cleanup",
        source_path.to_string_lossy().into(),
        (item_index + 1).min(items_total),
        items_total,
        rows_done,
        false,
    );

    Ok(CleanCsvResult {
        output_path: output_path.to_string_lossy().to_string(),
        original_rows,
        cleaned_rows,
        original_columns,
        cleaned_columns,
        removed_empty_rows,
        removed_empty_columns: 0,
    })
}

fn clean_csv_file_with_column_removal(
    app: &AppHandle,
    options: &CsvCleanupOptions,
    source_path: &Path,
    output_path: &Path,
    input_delimiter: u8,
    output_delimiter: u8,
    item_index: usize,
    items_total: usize,
    operation_id: &Option<String>,
) -> Result<CleanCsvResult, String> {
    let stats = analyze_csv_keep_columns(source_path, input_delimiter, options, operation_id)?;
    let keep_columns = stats.keep_columns;
    let file = fs::File::open(source_path).map_err(|e| format!("Cannot open CSV: {e}"))?;
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(false)
        .delimiter(input_delimiter)
        .flexible(true)
        .from_reader(file);

    let mut writer = csv::WriterBuilder::new()
        .delimiter(output_delimiter)
        .flexible(true)
        .from_path(output_path)
        .map_err(|e| format!("Cannot create output CSV: {e}"))?;

    let mut cleaned_rows = 0usize;
    let mut rows_done = 0usize;
    let mut header_normalized = false;

    let removed_empty_columns = keep_columns.iter().filter(|value| !**value).count();
    let cleaned_columns = keep_columns.iter().filter(|value| **value).count();

    for record in reader.records() {
        if rows_done.is_multiple_of(256) && spreadsheet_cancelled(operation_id) {
            clear_spreadsheet_cancel(operation_id);
            return Err("Cancelled".into());
        }

        let record = record.map_err(|e| format!("CSV parse failed: {e}"))?;
        let row = cleanup_csv_row(&record, options.trim_cells);

        if options.remove_empty_rows && is_row_empty(&row) {
            continue;
        }

        let mut output_row: Vec<String> = row
            .iter()
            .enumerate()
            .filter_map(|(index, value)| {
                keep_columns
                    .get(index)
                    .and_then(|keep| keep.then_some(value.clone()))
            })
            .collect();

        if options.has_header && options.normalize_headers && !header_normalized {
            let normalized = normalize_headers(&output_row);
            output_row = normalized;
            header_normalized = true;
        }

        writer
            .write_record(&output_row)
            .map_err(|e| format!("Cannot write output CSV: {e}"))?;
        cleaned_rows += 1;
        rows_done = cleaned_rows;

        if rows_done.is_multiple_of(256) {
            emit_spreadsheet_progress(
                app,
                operation_id,
                "csv_cleanup",
                source_path.to_string_lossy().into(),
                item_index,
                items_total,
                rows_done,
                false,
            );
        }
    }

    writer
        .flush()
        .map_err(|e| format!("Cannot flush output CSV: {e}"))?;

    emit_spreadsheet_progress(
        app,
        operation_id,
        "csv_cleanup",
        source_path.to_string_lossy().into(),
        (item_index + 1).min(items_total),
        items_total,
        rows_done,
        false,
    );

    Ok(CleanCsvResult {
        output_path: output_path.to_string_lossy().to_string(),
        original_rows: stats.original_rows,
        original_columns: stats.original_columns,
        cleaned_rows,
        removed_empty_rows: stats.removed_empty_rows,
        cleaned_columns,
        removed_empty_columns,
    })
}

fn analyze_csv_keep_columns(
    source_path: &Path,
    input_delimiter: u8,
    options: &CsvCleanupOptions,
    operation_id: &Option<String>,
) -> Result<CsvCleanupStats, String> {
    let file = fs::File::open(source_path).map_err(|e| format!("Cannot open CSV: {e}"))?;
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(false)
        .delimiter(input_delimiter)
        .flexible(true)
        .from_reader(file);

    let mut keep: Vec<bool> = Vec::new();
    let mut original_rows = 0usize;
    let mut original_columns = 0usize;
    let mut removed_empty_rows = 0usize;

    for record in reader.records() {
        if original_rows.is_multiple_of(256) && spreadsheet_cancelled(operation_id) {
            clear_spreadsheet_cancel(operation_id);
            return Err("Cancelled".into());
        }
        let record = record.map_err(|e| format!("CSV parse failed: {e}"))?;
        let row = cleanup_csv_row(&record, options.trim_cells);

        original_rows += 1;
        original_columns = original_columns.max(row.len());

        if options.remove_empty_rows && is_row_empty(&row) {
            removed_empty_rows += 1;
            continue;
        }

        if keep.len() < row.len() {
            keep.resize(row.len(), false);
        }

        for (index, value) in row.iter().enumerate() {
            if !value.trim().is_empty() {
                keep[index] = true;
            }
        }
    }

    Ok(CsvCleanupStats {
        original_rows,
        original_columns,
        removed_empty_rows,
        keep_columns: keep,
    })
}

struct CsvCleanupStats {
    original_rows: usize,
    original_columns: usize,
    removed_empty_rows: usize,
    keep_columns: Vec<bool>,
}

fn compute_csv_cleanup_preview(
    options: &CsvCleanupOptions,
    preview_limit: usize,
) -> Result<CsvCleanupComputed, String> {
    let path = PathBuf::from(&options.source_path);
    let detected = delimiter_byte(
        &options.input_delimiter,
        detect_delimiter(&path).unwrap_or(b','),
    )?;
    let file = fs::File::open(&path).map_err(|e| format!("Cannot open CSV: {e}"))?;
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(false)
        .delimiter(detected)
        .flexible(true)
        .from_reader(file);

    let mut original_columns = 0usize;
    let mut original_rows = 0usize;
    let mut cleaned_rows = 0usize;
    let mut removed_empty_rows = 0usize;
    let mut cleaned_columns = 0usize;
    let mut keep: Vec<bool> = Vec::new();
    let mut removed_empty_columns = 0usize;
    let mut keep_column_marked = 0usize;

    let mut rows: Vec<Vec<String>> = Vec::new();

    for record in reader.records() {
        let record = record.map_err(|e| format!("CSV parse failed: {e}"))?;
        let row = cleanup_csv_row(&record, options.trim_cells);
        original_rows += 1;
        original_columns = original_columns.max(row.len());

        if options.remove_empty_rows && is_row_empty(&row) {
            removed_empty_rows += 1;
            continue;
        }

        if keep.len() < row.len() {
            keep.resize(row.len(), false);
        }

        if options.remove_empty_columns {
            for (index, value) in row.iter().enumerate() {
                if !value.trim().is_empty() && !keep[index] {
                    keep[index] = true;
                    keep_column_marked += 1;
                }
            }
        } else {
            cleaned_columns = cleaned_columns.max(row.len());
        }

        if rows.len() < preview_limit {
            rows.push(row);
        }

        cleaned_rows += 1;
    }

    if options.remove_empty_columns {
        removed_empty_columns = keep.iter().filter(|value| !**value).count();
        cleaned_columns = keep_column_marked;
        for row in &mut rows {
            let mut next = Vec::new();
            next.reserve_exact(cleaned_columns);
            for (index, value) in row.iter().enumerate() {
                if keep.get(index).copied().unwrap_or(false) {
                    next.push(value.clone());
                }
            }
            *row = next;
        }
    }

    let mut header_changed = false;
    if options.has_header && options.normalize_headers && !rows.is_empty() {
        let normalized = normalize_headers(&rows[0]);
        header_changed = normalized != rows[0];
        rows[0] = normalized;
    }

    Ok(CsvCleanupComputed {
        detected_delimiter: detected,
        original_rows,
        cleaned_rows,
        original_columns,
        cleaned_columns,
        removed_empty_rows,
        removed_empty_columns,
        header_changed,
        rows,
    })
}

fn normalize_headers(headers: &[String]) -> Vec<String> {
    let mut seen: HashMap<String, usize> = HashMap::new();

    headers
        .iter()
        .enumerate()
        .map(|(index, header)| {
            let mut value = header.trim().to_lowercase();
            value = value
                .chars()
                .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
                .collect::<String>();
            while value.contains("__") {
                value = value.replace("__", "_");
            }
            value = value.trim_matches('_').to_string();
            if value.is_empty() {
                value = format!("column_{}", index + 1);
            }

            let count = seen.entry(value.clone()).or_insert(0);
            *count += 1;
            if *count > 1 {
                format!("{}_{}", value, count)
            } else {
                value
            }
        })
        .collect()
}

fn delimiter_byte(value: &str, auto_detected: u8) -> Result<u8, String> {
    Ok(match value {
        "tab" => b'\t',
        "semicolon" => b';',
        "pipe" => b'|',
        "auto" => auto_detected,
        "comma" => b',',
        other => return Err(format!("Unsupported delimiter: {other}")),
    })
}

fn delimiter_name(value: u8) -> &'static str {
    match value {
        b'\t' => "tab",
        b';' => "semicolon",
        b'|' => "pipe",
        _ => "comma",
    }
}

// =================== Excel → CSV ===================

#[derive(Deserialize, Clone)]
pub struct ExcelToCsvOptions {
    pub source_path: String,
    pub output_dir: Option<String>,
    pub sheet_strategy: String, // "all_separate" | "first_only" | "specific" | "merge"
    pub specific_sheet: Option<String>,
    pub delimiter: String, // "comma" | "tab" | "semicolon" | "pipe"
    pub quote_all: bool,
    pub include_sheet_name_column: bool, // for merge mode
    #[serde(default)]
    pub operation_id: Option<String>,
}

/// Reserved for a planned "batch Excel → CSV" command. Kept compiled so
/// the field schema stays validated against incoming JSON when the
/// frontend hook lands; today nothing constructs it.
#[allow(dead_code)]
#[derive(Deserialize, Clone)]
pub struct ExcelToCsvBatchOptions {
    pub sources: Vec<String>,
    pub output_dir: Option<String>,
    pub sheet_strategy: String, // "all_separate" | "first_only" | "specific" | "merge"
    pub specific_sheet: Option<String>,
    pub delimiter: String, // "comma" | "tab" | "semicolon" | "pipe"
    pub quote_all: bool,
    pub include_sheet_name_column: bool, // for merge mode
    #[serde(default)]
    pub operation_id: Option<String>,
}

#[derive(Serialize, Clone)]
pub struct ExcelToCsvResult {
    pub source_path: String,
    pub outputs: Vec<String>,
    pub total_rows: usize,
    pub total_sheets: usize,
    pub success: bool,
    pub error: Option<String>,
}

#[tauri::command(async)]
pub fn excel_to_csv(
    app: AppHandle,
    options: ExcelToCsvOptions,
) -> Result<ExcelToCsvResult, String> {
    // Security gate: source Excel file must be real + non-system; the
    // optional output_dir must not target a forbidden location.
    crate::core::safe_path::validate_user_path(&options.source_path)?;
    if let Some(ref dir) = options.output_dir {
        crate::core::safe_path::forbid_system_path(dir)?;
    }

    clear_spreadsheet_cancel(&options.operation_id);
    let source = PathBuf::from(&options.source_path);
    let stem = source
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("export")
        .to_string();

    let out_dir = match options.output_dir.as_deref() {
        Some(d) => PathBuf::from(d),
        None => source
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| PathBuf::from(".")),
    };

    let mut workbook = open_workbook_auto(&source).map_err(|e| format!("Cannot open: {}", e))?;

    let delimiter_byte = match options.delimiter.as_str() {
        "tab" => b'\t',
        "semicolon" => b';',
        "pipe" => b'|',
        _ => b',',
    };

    let quote_style = if options.quote_all {
        csv::QuoteStyle::Always
    } else {
        csv::QuoteStyle::Necessary
    };

    let sheet_names = workbook.sheet_names();
    let mut outputs: Vec<String> = Vec::new();
    let mut total_rows: usize = 0;
    let mut total_sheets: usize = 0;

    let target_sheets: Vec<String> = match options.sheet_strategy.as_str() {
        "first_only" => sheet_names.iter().take(1).cloned().collect(),
        "specific" => match options.specific_sheet.as_deref() {
            Some(name) if sheet_names.contains(&name.to_string()) => vec![name.to_string()],
            Some(name) => return Err(format!("Sheet '{}' not found", name)),
            None => return Err("Specific sheet mode but no sheet name given".into()),
        },
        _ => sheet_names.clone(), // "all_separate" or "merge"
    };

    emit_spreadsheet_progress(
        &app,
        &options.operation_id,
        "excel_to_csv",
        String::new(),
        0,
        target_sheets.len(),
        0,
        false,
    );

    if spreadsheet_cancelled(&options.operation_id) {
        clear_spreadsheet_cancel(&options.operation_id);
        return Err("Cancelled".into());
    }

    if options.sheet_strategy == "merge" {
        let merged_path = out_dir.join(format!("{}.csv", stem));
        let mut wtr = csv::WriterBuilder::new()
            .delimiter(delimiter_byte)
            .quote_style(quote_style)
            .from_path(&merged_path)
            .map_err(|e| format!("Create CSV: {}", e))?;

        let mut header_emitted = false;

        for (sheet_index, sheet_name) in target_sheets.iter().enumerate() {
            if spreadsheet_cancelled(&options.operation_id) {
                clear_spreadsheet_cancel(&options.operation_id);
                return Err("Cancelled".into());
            }
            emit_spreadsheet_progress(
                &app,
                &options.operation_id,
                "excel_to_csv",
                sheet_name.clone(),
                sheet_index,
                target_sheets.len(),
                total_rows,
                false,
            );
            let range = workbook
                .worksheet_range(sheet_name)
                .map_err(|e| format!("Read sheet '{}': {}", sheet_name, e))?;

            for (row_idx, row) in range.rows().enumerate() {
                if row_idx.is_multiple_of(500) && spreadsheet_cancelled(&options.operation_id) {
                    clear_spreadsheet_cancel(&options.operation_id);
                    return Err("Cancelled".into());
                }
                let mut record: Vec<String> = if options.include_sheet_name_column {
                    vec![sheet_name.clone()]
                } else {
                    Vec::new()
                };

                if row_idx == 0 && options.include_sheet_name_column && !header_emitted {
                    let mut hdr: Vec<String> = vec!["__sheet__".into()];
                    hdr.extend(
                        row.iter()
                            .enumerate()
                            .map(|(i, _)| format!("col_{}", i + 1)),
                    );
                    wtr.write_record(&hdr)
                        .map_err(|e| format!("Write: {}", e))?;
                    header_emitted = true;
                }

                record.extend(row.iter().map(cell_to_string));
                wtr.write_record(&record)
                    .map_err(|e| format!("Write: {}", e))?;
                total_rows += 1;
            }
            total_sheets += 1;
            emit_spreadsheet_progress(
                &app,
                &options.operation_id,
                "excel_to_csv",
                sheet_name.clone(),
                sheet_index + 1,
                target_sheets.len(),
                total_rows,
                false,
            );
        }

        wtr.flush().map_err(|e| format!("Flush: {}", e))?;
        outputs.push(merged_path.to_string_lossy().to_string());
    } else {
        for (sheet_index, sheet_name) in target_sheets.iter().enumerate() {
            if spreadsheet_cancelled(&options.operation_id) {
                clear_spreadsheet_cancel(&options.operation_id);
                return Err("Cancelled".into());
            }
            emit_spreadsheet_progress(
                &app,
                &options.operation_id,
                "excel_to_csv",
                sheet_name.clone(),
                sheet_index,
                target_sheets.len(),
                total_rows,
                false,
            );
            let range = workbook
                .worksheet_range(sheet_name)
                .map_err(|e| format!("Read sheet '{}': {}", sheet_name, e))?;

            let safe_sheet = sanitize_filename_part(sheet_name);
            let out_name = if target_sheets.len() == 1 {
                format!("{}.csv", stem)
            } else {
                format!("{}_{}.csv", stem, safe_sheet)
            };
            let out_path = out_dir.join(out_name);

            let mut wtr = csv::WriterBuilder::new()
                .delimiter(delimiter_byte)
                .quote_style(quote_style)
                .from_path(&out_path)
                .map_err(|e| format!("Create CSV: {}", e))?;

            for (row_idx, row) in range.rows().enumerate() {
                if row_idx.is_multiple_of(500) && spreadsheet_cancelled(&options.operation_id) {
                    clear_spreadsheet_cancel(&options.operation_id);
                    return Err("Cancelled".into());
                }
                let record: Vec<String> = row.iter().map(cell_to_string).collect();
                wtr.write_record(&record)
                    .map_err(|e| format!("Write: {}", e))?;
                total_rows += 1;
            }

            wtr.flush().map_err(|e| format!("Flush: {}", e))?;
            outputs.push(out_path.to_string_lossy().to_string());
            total_sheets += 1;
            emit_spreadsheet_progress(
                &app,
                &options.operation_id,
                "excel_to_csv",
                sheet_name.clone(),
                sheet_index + 1,
                target_sheets.len(),
                total_rows,
                false,
            );
        }
    }

    clear_spreadsheet_cancel(&options.operation_id);

    Ok(ExcelToCsvResult {
        source_path: options.source_path.clone(),
        outputs,
        total_rows,
        total_sheets,
        success: true,
        error: None,
    })
}

fn sanitize_filename_part(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            _ => c,
        })
        .collect()
}

// =================== CSV → Excel ===================

#[derive(Deserialize)]
pub struct CsvToExcelOptions {
    pub sources: Vec<String>, // multiple CSVs → multiple sheets in one xlsx
    pub output_path: String,
    pub delimiter: String, // "comma" | "tab" | "semicolon" | "pipe" | "auto"
    pub has_header: bool,
    pub bold_header: bool,
    pub auto_filter: bool,
    pub auto_width: bool,
    pub freeze_header: bool,
    pub detect_types: bool,
    #[serde(default)]
    pub operation_id: Option<String>,
}

#[derive(Serialize, Clone)]
pub struct CsvToExcelResult {
    pub output_path: String,
    pub total_sheets: usize,
    pub total_rows: usize,
}

#[derive(Deserialize, Clone)]
pub struct CsvMergeOptions {
    pub sources: Vec<String>,
    pub output_path: String,
    pub source_delimiter: String, // "auto" | "comma" | "tab" | "semicolon" | "pipe"
    pub output_delimiter: String, // "comma" | "tab" | "semicolon" | "pipe"
    pub include_source: bool,
    pub include_headers: bool,
    #[serde(default)]
    pub operation_id: Option<String>,
}

#[derive(Serialize, Clone)]
pub struct CsvMergeFileResult {
    pub source_path: String,
    pub output_path: String,
    pub rows_read: usize,
    pub rows_written: usize,
    pub success: bool,
    pub error: Option<String>,
}

#[derive(Deserialize, Clone)]
pub struct CsvSplitOptions {
    pub sources: Vec<String>,
    pub output_dir: Option<String>,
    pub rows_per_file: usize,
    pub include_header: bool,
    pub delimiter: String, // "auto" | "comma" | "tab" | "semicolon" | "pipe"
    #[serde(default)]
    pub operation_id: Option<String>,
}

#[derive(Serialize, Clone)]
pub struct CsvSplitFileResult {
    pub source_path: String,
    pub output_dir: String,
    pub output_paths: Vec<String>,
    pub total_rows: usize,
    pub split_count: usize,
    pub rows_per_file: usize,
    pub success: bool,
    pub error: Option<String>,
}

#[derive(Deserialize, Clone)]
pub struct CsvToJsonOptions {
    pub sources: Vec<String>,
    pub output_dir: Option<String>,
    pub delimiter: String, // "auto" | "comma" | "tab" | "semicolon" | "pipe"
    pub has_header: bool,
    pub output_format: String, // "jsonl" | "json"
    #[serde(default)]
    pub operation_id: Option<String>,
}

#[derive(Serialize, Clone)]
pub struct CsvToJsonFileResult {
    pub source_path: String,
    pub output_path: String,
    pub rows_written: usize,
    pub columns_written: usize,
    pub success: bool,
    pub error: Option<String>,
}

#[derive(Clone, Copy)]
enum CsvJsonOutputMode {
    Jsonl,
    JsonArray,
}

#[derive(Clone, Copy)]
enum CsvToJsonMode {
    Jsonl,
    JsonArray,
}

impl CsvToJsonMode {
    fn extension(self) -> &'static str {
        match self {
            Self::Jsonl => "jsonl",
            Self::JsonArray => "json",
        }
    }

    fn output_format(self) -> CsvJsonOutputMode {
        match self {
            Self::Jsonl => CsvJsonOutputMode::Jsonl,
            Self::JsonArray => CsvJsonOutputMode::JsonArray,
        }
    }
}

impl CsvToJsonOptions {
    fn resolve_output_mode(&self) -> CsvToJsonMode {
        match self.output_format.as_str() {
            "json" => CsvToJsonMode::JsonArray,
            _ => CsvToJsonMode::Jsonl,
        }
    }
}

fn write_json_object(
    writer: &mut BufWriter<std::fs::File>,
    mode: CsvJsonOutputMode,
    is_first: &mut bool,
    row: &Map<String, Value>,
) -> Result<(), String> {
    let payload = Value::Object(row.clone());
    let line = serde_json::to_string(&payload).map_err(|e| format!("Serialize JSON: {e}"))?;
    match mode {
        CsvJsonOutputMode::Jsonl => {
            writeln!(writer, "{line}").map_err(|e| format!("Write JSONL output: {e}"))?;
        }
        CsvJsonOutputMode::JsonArray => {
            if !*is_first {
                writer
                    .write_all(b",\n")
                    .map_err(|e| format!("Write JSON output: {e}"))?;
            }
            writer
                .write_all(line.as_bytes())
                .map_err(|e| format!("Write JSON output: {e}"))?;
            *is_first = false;
        }
    }

    Ok(())
}

fn open_csv_to_json_writer(
    output_dir: &Path,
    source_stem: &str,
    mode: CsvToJsonMode,
) -> Result<(BufWriter<std::fs::File>, PathBuf), String> {
    let safe_stem = sanitize_filename_part(source_stem);
    let output_path = output_dir.join(format!("{}.{}", safe_stem, mode.extension()));
    let file =
        std::fs::File::create(&output_path).map_err(|e| format!("Create JSON output: {e}"))?;

    let mut writer = BufWriter::new(file);
    if matches!(mode.output_format(), CsvJsonOutputMode::JsonArray) {
        writer
            .write_all(b"[")
            .map_err(|e| format!("Write JSON output: {e}"))?;
    }

    Ok((writer, output_path))
}

fn csv_to_json_source(
    app: &AppHandle,
    source_path: &str,
    output_dir: &Path,
    input_delimiter: u8,
    has_header: bool,
    output_mode: CsvToJsonMode,
    item_index: usize,
    items_total: usize,
    operation_id: &Option<String>,
) -> CsvToJsonFileResult {
    let source = Path::new(source_path);
    if !source.exists() || !source.is_file() {
        return CsvToJsonFileResult {
            source_path: source_path.to_string(),
            output_path: String::new(),
            rows_written: 0,
            columns_written: 0,
            success: false,
            error: Some("Source is missing or not a file".into()),
        };
    }

    let stem = source
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or("converted");
    let (mut writer, output_path) = match open_csv_to_json_writer(output_dir, stem, output_mode) {
        Ok(inner) => inner,
        Err(err) => {
            return CsvToJsonFileResult {
                source_path: source_path.to_string(),
                output_path: String::new(),
                rows_written: 0,
                columns_written: 0,
                success: false,
                error: Some(err),
            };
        }
    };

    let file = match fs::File::open(source) {
        Ok(file) => file,
        Err(e) => {
            let _ = output_path.try_exists();
            return CsvToJsonFileResult {
                source_path: source_path.to_string(),
                output_path: output_path.to_string_lossy().to_string(),
                rows_written: 0,
                columns_written: 0,
                success: false,
                error: Some(format!("Cannot open source: {e}")),
            };
        }
    };

    let mut reader = csv::ReaderBuilder::new()
        .has_headers(false)
        .delimiter(input_delimiter)
        .flexible(true)
        .from_reader(file);

    emit_spreadsheet_progress(
        app,
        operation_id,
        "csv_to_json",
        source_path.to_string(),
        item_index,
        items_total,
        0,
        false,
    );

    let mut records = reader.records();
    let mut headers: Vec<String> = Vec::new();
    let mut inferred_headers = false;
    if has_header {
        let header_record = match records.next() {
            Some(Ok(r)) => r,
            Some(Err(err)) => {
                let _ = output_path.try_exists();
                return CsvToJsonFileResult {
                    source_path: source_path.to_string(),
                    output_path: output_path.to_string_lossy().to_string(),
                    rows_written: 0,
                    columns_written: 0,
                    success: false,
                    error: Some(format!("CSV parse failed: {err}")),
                };
            }
            None => {
                if let Err(err) = match output_mode.output_format() {
                    CsvJsonOutputMode::Jsonl => Ok(()),
                    CsvJsonOutputMode::JsonArray => writer
                        .write_all(b"]")
                        .map_err(|e| format!("Finalize JSON output: {e}")),
                } {
                    return CsvToJsonFileResult {
                        source_path: source_path.to_string(),
                        output_path: output_path.to_string_lossy().to_string(),
                        rows_written: 0,
                        columns_written: 0,
                        success: false,
                        error: Some(err),
                    };
                }
                return CsvToJsonFileResult {
                    source_path: source_path.to_string(),
                    output_path: output_path.to_string_lossy().to_string(),
                    rows_written: 0,
                    columns_written: 0,
                    success: true,
                    error: None,
                };
            }
        };
        headers = normalize_headers(
            &header_record
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>(),
        );
    }

    let mut rows_written = 0usize;
    let mut columns_written = 0usize;
    let mut is_first = true;

    for record in records {
        if rows_written.is_multiple_of(256) && spreadsheet_cancelled(operation_id) {
            clear_spreadsheet_cancel(operation_id);
            if matches!(output_mode.output_format(), CsvJsonOutputMode::JsonArray) && is_first {
                if let Err(err) = writer.write_all(b"]") {
                    return CsvToJsonFileResult {
                        source_path: source_path.to_string(),
                        output_path: output_path.to_string_lossy().to_string(),
                        rows_written,
                        columns_written,
                        success: false,
                        error: Some(format!("Finalize JSON output: {err}")),
                    };
                }
            }
            emit_spreadsheet_progress(
                app,
                operation_id,
                "csv_to_json",
                source_path.to_string(),
                item_index,
                items_total,
                rows_written,
                true,
            );
            return CsvToJsonFileResult {
                source_path: source_path.to_string(),
                output_path: output_path.to_string_lossy().to_string(),
                rows_written,
                columns_written,
                success: false,
                error: Some("Cancelled".into()),
            };
        }

        let record = match record {
            Ok(r) => r,
            Err(err) => {
                if let Err(finalize_error) = match output_mode.output_format() {
                    CsvJsonOutputMode::Jsonl => Ok(()),
                    CsvJsonOutputMode::JsonArray => writer
                        .write_all(b"]")
                        .map_err(|e| format!("Finalize JSON output: {e}")),
                } {
                    return CsvToJsonFileResult {
                        source_path: source_path.to_string(),
                        output_path: output_path.to_string_lossy().to_string(),
                        rows_written,
                        columns_written,
                        success: false,
                        error: Some(format!("Finalize JSON output: {finalize_error}")),
                    };
                }
                return CsvToJsonFileResult {
                    source_path: source_path.to_string(),
                    output_path: output_path.to_string_lossy().to_string(),
                    rows_written,
                    columns_written,
                    success: false,
                    error: Some(format!("CSV parse failed: {err}")),
                };
            }
        };

        if !inferred_headers {
            if !has_header {
                headers = (0..record.len())
                    .map(|index| format!("col_{:03}", index + 1))
                    .collect();
            } else if headers.is_empty() {
                headers = (0..record.len())
                    .map(|index| format!("col_{:03}", index + 1))
                    .collect();
            }
            inferred_headers = true;
        }

        let mut row = Map::new();
        for (index, value) in record.iter().enumerate() {
            let key = if index < headers.len() {
                headers[index].clone()
            } else {
                let generated = format!("col_{:03}", index + 1);
                if headers.len() <= index {
                    headers.push(generated.clone());
                }
                generated
            };
            row.insert(key, Value::String(value.to_string()));
        }

        if let Err(err) = write_json_object(
            &mut writer,
            output_mode.output_format(),
            &mut is_first,
            &row,
        ) {
            return CsvToJsonFileResult {
                source_path: source_path.to_string(),
                output_path: output_path.to_string_lossy().to_string(),
                rows_written,
                columns_written,
                success: false,
                error: Some(err),
            };
        }

        rows_written += 1;
        columns_written = columns_written.max(record.len());

        if rows_written.is_multiple_of(256) {
            emit_spreadsheet_progress(
                app,
                operation_id,
                "csv_to_json",
                source_path.to_string(),
                item_index,
                items_total,
                rows_written,
                false,
            );
        }
    }

    let finalize_error = match output_mode.output_format() {
        CsvJsonOutputMode::Jsonl => Ok(()),
        CsvJsonOutputMode::JsonArray => writer
            .write_all(if is_first { b"]" } else { b"\n]" })
            .map_err(|e| format!("Finalize JSON output: {e}")),
    };

    if let Err(err) = finalize_error {
        return CsvToJsonFileResult {
            source_path: source_path.to_string(),
            output_path: output_path.to_string_lossy().to_string(),
            rows_written,
            columns_written,
            success: false,
            error: Some(err),
        };
    }

    CsvToJsonFileResult {
        source_path: source_path.to_string(),
        output_path: output_path.to_string_lossy().to_string(),
        rows_written,
        columns_written,
        success: true,
        error: None,
    }
}

#[tauri::command]
pub async fn csv_to_json(
    app: AppHandle,
    options: CsvToJsonOptions,
) -> Result<Vec<CsvToJsonFileResult>, String> {
    // Security gate: every input CSV must be real + non-system; the
    // optional output_dir must not target a forbidden location.
    for source in &options.sources {
        crate::core::safe_path::validate_user_path(source)?;
    }
    if let Some(ref dir) = options.output_dir {
        crate::core::safe_path::forbid_system_path(dir)?;
    }

    clear_spreadsheet_cancel(&options.operation_id);
    if options.sources.is_empty() {
        return Ok(Vec::new());
    }

    let output_mode = options.resolve_output_mode();
    let output_dir = match options.output_dir {
        Some(path) => PathBuf::from(path),
        None => Path::new(&options.sources[0])
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from(".")),
    };
    let input_delimiter = delimiter_byte(&options.delimiter, b',')?;

    tauri::async_runtime::spawn_blocking(move || {
        if !output_dir.exists() {
            fs::create_dir_all(&output_dir)
                .map_err(|e| format!("Cannot create output directory: {e}"))?;
        }

        let mut results: Vec<CsvToJsonFileResult> = Vec::with_capacity(options.sources.len());
        for (source_index, source) in options.sources.iter().enumerate() {
            if spreadsheet_cancelled(&options.operation_id) {
                clear_spreadsheet_cancel(&options.operation_id);
                results.push(CsvToJsonFileResult {
                    source_path: source.clone(),
                    output_path: String::new(),
                    rows_written: 0,
                    columns_written: 0,
                    success: false,
                    error: Some("Cancelled".into()),
                });
                break;
            }

            let result = csv_to_json_source(
                &app,
                source,
                &output_dir,
                input_delimiter,
                options.has_header,
                output_mode,
                source_index,
                options.sources.len(),
                &options.operation_id,
            );

            let stop_on_cancel =
                matches!(result.error.as_deref(), Some(error) if error == "Cancelled");
            results.push(result);

            if stop_on_cancel {
                clear_spreadsheet_cancel(&options.operation_id);
                break;
            }
        }

        clear_spreadsheet_cancel(&options.operation_id);
        Ok(results)
    })
    .await
    .map_err(|e| e.to_string())?
}

fn split_csv_file_ext(delimiter: u8) -> &'static str {
    if delimiter == b'\t' {
        "tsv"
    } else {
        "csv"
    }
}

fn open_csv_split_writer(
    output_dir: &Path,
    source_stem: &str,
    part_index: usize,
    delimiter: u8,
    include_header: bool,
    header: Option<&Vec<String>>,
) -> Result<(csv::Writer<std::fs::File>, PathBuf), String> {
    let safe_stem = sanitize_filename_part(source_stem);
    let output_path = output_dir.join(format!(
        "{}_split_{:03}.{}",
        safe_stem,
        part_index,
        split_csv_file_ext(delimiter)
    ));
    let mut writer = csv::WriterBuilder::new()
        .delimiter(delimiter)
        .flexible(true)
        .from_path(&output_path)
        .map_err(|e| format!("Create split output: {e}"))?;

    if include_header {
        if let Some(header) = header {
            writer
                .write_record(header)
                .map_err(|e| format!("Write split header: {e}"))?;
        }
    }

    Ok((writer, output_path))
}

fn split_csv_source(
    app: &AppHandle,
    source_path: &str,
    output_dir: &Path,
    rows_per_file: usize,
    include_header: bool,
    input_delimiter: u8,
    output_delimiter: u8,
    item_index: usize,
    items_total: usize,
    operation_id: &Option<String>,
) -> CsvSplitFileResult {
    let source = Path::new(source_path);
    if !source.exists() || !source.is_file() {
        return CsvSplitFileResult {
            source_path: source_path.to_string(),
            output_dir: output_dir.to_string_lossy().to_string(),
            output_paths: Vec::new(),
            total_rows: 0,
            split_count: 0,
            rows_per_file,
            success: false,
            error: Some("Source is missing or not a file".into()),
        };
    }

    let stem = source
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or("split")
        .to_string();

    let file = match fs::File::open(source) {
        Ok(file) => file,
        Err(e) => {
            return CsvSplitFileResult {
                source_path: source_path.to_string(),
                output_dir: output_dir.to_string_lossy().to_string(),
                output_paths: Vec::new(),
                total_rows: 0,
                split_count: 0,
                rows_per_file,
                success: false,
                error: Some(format!("Cannot open source: {e}")),
            };
        }
    };

    let mut reader = csv::ReaderBuilder::new()
        .has_headers(false)
        .delimiter(input_delimiter)
        .flexible(true)
        .from_reader(file);

    emit_spreadsheet_progress(
        app,
        operation_id,
        "csv_split",
        source_path.to_string(),
        item_index,
        items_total,
        0,
        false,
    );

    let mut records = reader.records();
    let mut header: Option<Vec<String>> = None;
    if include_header {
        match records.next() {
            Some(first_record) => match first_record {
                Ok(record) => {
                    header = Some(record.iter().map(ToString::to_string).collect());
                }
                Err(err) => {
                    return CsvSplitFileResult {
                        source_path: source_path.to_string(),
                        output_dir: output_dir.to_string_lossy().to_string(),
                        output_paths: Vec::new(),
                        total_rows: 0,
                        split_count: 0,
                        rows_per_file,
                        success: false,
                        error: Some(format!("CSV parse failed: {err}")),
                    };
                }
            },
            None => {
                return CsvSplitFileResult {
                    source_path: source_path.to_string(),
                    output_dir: output_dir.to_string_lossy().to_string(),
                    output_paths: Vec::new(),
                    total_rows: 0,
                    split_count: 0,
                    rows_per_file,
                    success: false,
                    error: Some("CSV has no rows".into()),
                };
            }
        }
    }

    let mut output_paths: Vec<String> = Vec::new();
    let mut total_rows = 0usize;
    let mut rows_in_part = 0usize;
    let mut split_count = 0usize;
    let mut part_index = 0usize;
    let mut writer: Option<csv::Writer<std::fs::File>> = None;

    if include_header && header.is_some() {
        part_index += 1;
        match open_csv_split_writer(
            output_dir,
            &stem,
            part_index,
            output_delimiter,
            include_header,
            header.as_ref(),
        ) {
            Ok((wtr, output_path)) => {
                writer = Some(wtr);
                output_paths.push(output_path.to_string_lossy().into());
                split_count = 1;
            }
            Err(error) => {
                return CsvSplitFileResult {
                    source_path: source_path.to_string(),
                    output_dir: output_dir.to_string_lossy().to_string(),
                    output_paths,
                    total_rows,
                    split_count,
                    rows_per_file,
                    success: false,
                    error: Some(error),
                };
            }
        }
    }

    for record in records {
        if total_rows.is_multiple_of(256) && spreadsheet_cancelled(operation_id) {
            clear_spreadsheet_cancel(operation_id);
            if let Some(mut output) = writer {
                let _ = output.flush();
            }
            emit_spreadsheet_progress(
                app,
                operation_id,
                "csv_split",
                source_path.to_string(),
                item_index,
                items_total,
                total_rows,
                true,
            );
            return CsvSplitFileResult {
                source_path: source_path.to_string(),
                output_dir: output_dir.to_string_lossy().to_string(),
                output_paths,
                total_rows,
                split_count,
                rows_per_file,
                success: false,
                error: Some("Cancelled".into()),
            };
        }

        let row = match record {
            Ok(record) => record.iter().map(ToString::to_string).collect::<Vec<_>>(),
            Err(err) => {
                if let Some(output) = writer.as_mut() {
                    let _ = output.flush();
                }
                emit_spreadsheet_progress(
                    app,
                    operation_id,
                    "csv_split",
                    source_path.to_string(),
                    item_index,
                    items_total,
                    total_rows,
                    false,
                );
                return CsvSplitFileResult {
                    source_path: source_path.to_string(),
                    output_dir: output_dir.to_string_lossy().to_string(),
                    output_paths,
                    total_rows,
                    split_count,
                    rows_per_file,
                    success: false,
                    error: Some(format!("CSV parse failed: {err}")),
                };
            }
        };

        if writer.is_none() || rows_in_part >= rows_per_file {
            if let Some(output) = writer.as_mut() {
                if let Err(err) = output.flush() {
                    return CsvSplitFileResult {
                        source_path: source_path.to_string(),
                        output_dir: output_dir.to_string_lossy().to_string(),
                        output_paths,
                        total_rows,
                        split_count,
                        rows_per_file,
                        success: false,
                        error: Some(format!("Flush output failed: {err}")),
                    };
                }
            }

            part_index += 1;
            match open_csv_split_writer(
                output_dir,
                &stem,
                part_index,
                output_delimiter,
                include_header,
                header.as_ref(),
            ) {
                Ok((wtr, output_path)) => {
                    writer = Some(wtr);
                    output_paths.push(output_path.to_string_lossy().into());
                    split_count += 1;
                    rows_in_part = 0;
                }
                Err(error) => {
                    if let Some(output) = writer.as_mut() {
                        let _ = output.flush();
                    }
                    return CsvSplitFileResult {
                        source_path: source_path.to_string(),
                        output_dir: output_dir.to_string_lossy().to_string(),
                        output_paths,
                        total_rows,
                        split_count,
                        rows_per_file,
                        success: false,
                        error: Some(error),
                    };
                }
            }
        }

        if let Some(output) = writer.as_mut() {
            if let Err(err) = output.write_record(&row) {
                return CsvSplitFileResult {
                    source_path: source_path.to_string(),
                    output_dir: output_dir.to_string_lossy().to_string(),
                    output_paths,
                    total_rows,
                    split_count,
                    rows_per_file,
                    success: false,
                    error: Some(format!("Write split output: {err}")),
                };
            }
        }

        rows_in_part += 1;
        total_rows += 1;

        if total_rows.is_multiple_of(256) {
            emit_spreadsheet_progress(
                app,
                operation_id,
                "csv_split",
                source_path.to_string(),
                item_index,
                items_total,
                total_rows,
                false,
            );
        }
    }

    if let Some(output) = writer.as_mut() {
        if let Err(err) = output.flush() {
            return CsvSplitFileResult {
                source_path: source_path.to_string(),
                output_dir: output_dir.to_string_lossy().to_string(),
                output_paths,
                total_rows,
                split_count,
                rows_per_file,
                success: false,
                error: Some(format!("Flush output failed: {err}")),
            };
        }
    }

    emit_spreadsheet_progress(
        app,
        operation_id,
        "csv_split",
        source_path.to_string(),
        item_index + 1,
        items_total,
        total_rows,
        false,
    );

    CsvSplitFileResult {
        source_path: source_path.to_string(),
        output_dir: output_dir.to_string_lossy().to_string(),
        output_paths,
        total_rows,
        split_count,
        rows_per_file,
        success: true,
        error: None,
    }
}

#[tauri::command]
pub async fn merge_csv_files(
    app: AppHandle,
    options: CsvMergeOptions,
) -> Result<Vec<CsvMergeFileResult>, String> {
    // Security gate: every input CSV must be real + non-system; the
    // output file must not land in a forbidden location.
    for source in &options.sources {
        crate::core::safe_path::validate_user_path(source)?;
    }
    crate::core::safe_path::validate_user_write_target(&options.output_path)?;

    clear_spreadsheet_cancel(&options.operation_id);
    if options.sources.is_empty() {
        return Ok(Vec::new());
    }

    let output_path = options.output_path.clone();
    let output_delimiter = delimiter_byte(&options.output_delimiter, b',')?;

    tauri::async_runtime::spawn_blocking(move || {
        if let Some(parent) = Path::new(&output_path).parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("Cannot create output directory: {e}"))?;
        }

        let mut results: Vec<CsvMergeFileResult> = Vec::with_capacity(options.sources.len());
        let mut rows_written: usize = 0usize;
        let mut source_header_written = false;
        let mut output = csv::WriterBuilder::new()
            .delimiter(output_delimiter)
            .flexible(true)
            .from_path(&output_path)
            .map_err(|e| format!("Cannot create output CSV: {e}"))?;

        emit_spreadsheet_progress(
            &app,
            &options.operation_id,
            "csv_merge",
            String::new(),
            0,
            options.sources.len(),
            0,
            false,
        );

        for (source_index, source) in options.sources.iter().enumerate() {
            if spreadsheet_cancelled(&options.operation_id) {
                clear_spreadsheet_cancel(&options.operation_id);
                emit_spreadsheet_progress(
                    &app,
                    &options.operation_id,
                    "csv_merge",
                    source.clone(),
                    source_index,
                    options.sources.len(),
                    rows_written,
                    true,
                );
                break;
            }

            emit_spreadsheet_progress(
                &app,
                &options.operation_id,
                "csv_merge",
                source.clone(),
                source_index,
                options.sources.len(),
                rows_written,
                false,
            );

            let source_path = Path::new(source);
            let file_name = source_path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("source")
                .to_string();

            if !source_path.exists() || !source_path.is_file() {
                results.push(CsvMergeFileResult {
                    source_path: source.clone(),
                    output_path: output_path.clone(),
                    rows_read: 0,
                    rows_written: 0,
                    success: false,
                    error: Some("Source is missing or not a file".into()),
                });
                emit_spreadsheet_progress(
                    &app,
                    &options.operation_id,
                    "csv_merge",
                    source.clone(),
                    source_index + 1,
                    options.sources.len(),
                    rows_written,
                    false,
                );
                continue;
            }

            let reader_delimiter = match options.source_delimiter.as_str() {
                "tab" => b'\t',
                "semicolon" => b';',
                "pipe" => b'|',
                "auto" => detect_delimiter(source_path).unwrap_or(b','),
                "comma" => b',',
                _ => {
                    return Err(format!(
                        "Unsupported source delimiter: {}",
                        options.source_delimiter
                    ))
                }
            };

            let file = match fs::File::open(source_path) {
                Ok(file) => file,
                Err(err) => {
                    results.push(CsvMergeFileResult {
                        source_path: source.clone(),
                        output_path: output_path.clone(),
                        rows_read: 0,
                        rows_written: 0,
                        success: false,
                        error: Some(format!("Open failed: {err}")),
                    });
                    emit_spreadsheet_progress(
                        &app,
                        &options.operation_id,
                        "csv_merge",
                        source.clone(),
                        source_index + 1,
                        options.sources.len(),
                        rows_written,
                        false,
                    );
                    continue;
                }
            };

            let mut reader = csv::ReaderBuilder::new()
                .has_headers(false)
                .delimiter(reader_delimiter)
                .flexible(true)
                .from_reader(file);

            let mut source_rows_read = 0usize;
            let mut source_rows_written = 0usize;
            let mut source_record_index = 0usize;

            for record in reader.records() {
                if source_record_index.is_multiple_of(1000) && spreadsheet_cancelled(&options.operation_id) {
                    clear_spreadsheet_cancel(&options.operation_id);
                    break;
                }
                let record = record.map_err(|err| format!("CSV parse {}: {err}", source))?;
                source_record_index += 1;
                source_rows_read += 1;

                if options.include_headers && source_record_index == 1 {
                    if !source_header_written {
                        let mut header =
                            Vec::with_capacity(record.len() + usize::from(options.include_source));
                        if options.include_source {
                            header.push("source_path".to_string());
                        }
                        header.extend(record.iter().map(ToString::to_string));
                        output
                            .write_record(&header)
                            .map_err(|err| format!("Write output failed: {err}"))?;
                        source_header_written = true;
                    }
                    continue;
                }

                let mut output_record: Vec<String> =
                    Vec::with_capacity(record.len() + usize::from(options.include_source));
                if options.include_source {
                    output_record.push(file_name.clone());
                }
                output_record.extend(record.iter().map(ToString::to_string));
                output
                    .write_record(&output_record)
                    .map_err(|err| format!("Write output failed: {err}"))?;
                source_rows_written += 1;
                rows_written += 1;
            }

            results.push(CsvMergeFileResult {
                source_path: source.clone(),
                output_path: output_path.clone(),
                rows_read: source_rows_read,
                rows_written: source_rows_written,
                success: source_rows_written > 0
                    || (!options.include_headers && source_rows_read > 0),
                error: if spreadsheet_cancelled(&options.operation_id) {
                    Some("Cancelled".into())
                } else {
                    None
                },
            });

            emit_spreadsheet_progress(
                &app,
                &options.operation_id,
                "csv_merge",
                source.clone(),
                source_index + 1,
                options.sources.len(),
                rows_written,
                false,
            );
        }

        output.flush().map_err(|e| format!("Flush output: {e}"))?;
        clear_spreadsheet_cancel(&options.operation_id);

        Ok(results)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn split_csv_files(
    app: AppHandle,
    options: CsvSplitOptions,
) -> Result<Vec<CsvSplitFileResult>, String> {
    // Security gate: every input CSV must be real + non-system; the
    // optional output_dir must not target a forbidden location.
    for source in &options.sources {
        crate::core::safe_path::validate_user_path(source)?;
    }
    if let Some(ref dir) = options.output_dir {
        crate::core::safe_path::forbid_system_path(dir)?;
    }

    clear_spreadsheet_cancel(&options.operation_id);
    if options.sources.is_empty() {
        return Ok(Vec::new());
    }

    if options.rows_per_file == 0 {
        return Err("rows_per_file must be at least 1".into());
    }

    let input_delimiter = delimiter_byte(&options.delimiter, b',')?;
    let output_delimiter = input_delimiter;
    let output_dir = match options.output_dir {
        Some(path) => PathBuf::from(path),
        None => Path::new(&options.sources[0])
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from(".")),
    };

    tauri::async_runtime::spawn_blocking(move || {
        if !output_dir.exists() {
            fs::create_dir_all(&output_dir)
                .map_err(|e| format!("Cannot create output directory: {e}"))?;
        }

        let mut results: Vec<CsvSplitFileResult> = Vec::with_capacity(options.sources.len());

        for (source_index, source) in options.sources.iter().enumerate() {
            if spreadsheet_cancelled(&options.operation_id) {
                clear_spreadsheet_cancel(&options.operation_id);
                results.push(CsvSplitFileResult {
                    source_path: source.clone(),
                    output_dir: output_dir.to_string_lossy().to_string(),
                    output_paths: Vec::new(),
                    total_rows: 0,
                    split_count: 0,
                    rows_per_file: options.rows_per_file,
                    success: false,
                    error: Some("Cancelled".into()),
                });
                break;
            }

            let result = split_csv_source(
                &app,
                source,
                &output_dir,
                options.rows_per_file,
                options.include_header,
                input_delimiter,
                output_delimiter,
                source_index,
                options.sources.len(),
                &options.operation_id,
            );

            let stop_on_cancel =
                matches!(result.error.as_deref(), Some(error) if error == "Cancelled");
            results.push(result);

            if stop_on_cancel {
                clear_spreadsheet_cancel(&options.operation_id);
                break;
            }
        }

        clear_spreadsheet_cancel(&options.operation_id);

        Ok(results)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command(async)]
pub fn csv_to_excel(
    app: AppHandle,
    options: CsvToExcelOptions,
) -> Result<CsvToExcelResult, String> {
    // Security gate: every input CSV must be real + non-system; the
    // output Excel file must not land in a forbidden location.
    for source in &options.sources {
        crate::core::safe_path::validate_user_path(source)?;
    }
    crate::core::safe_path::validate_user_write_target(&options.output_path)?;

    clear_spreadsheet_cancel(&options.operation_id);
    if options.sources.is_empty() {
        return Err("No CSV files provided".into());
    }

    let mut wb = Workbook::new();

    let header_format = Format::new()
        .set_bold()
        .set_background_color("D9E1F2")
        .set_border(FormatBorder::Thin);

    let mut total_rows = 0usize;
    let mut total_sheets = 0usize;

    emit_spreadsheet_progress(
        &app,
        &options.operation_id,
        "csv_to_excel",
        String::new(),
        0,
        options.sources.len(),
        0,
        false,
    );

    for (source_index, source) in options.sources.iter().enumerate() {
        if spreadsheet_cancelled(&options.operation_id) {
            clear_spreadsheet_cancel(&options.operation_id);
            return Err("Cancelled".into());
        }
        emit_spreadsheet_progress(
            &app,
            &options.operation_id,
            "csv_to_excel",
            source.clone(),
            source_index,
            options.sources.len(),
            total_rows,
            false,
        );
        let path = PathBuf::from(source);
        let sheet_name_raw = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Sheet")
            .to_string();
        let sheet_name = truncate_sheet_name(&sanitize_filename_part(&sheet_name_raw));

        let delimiter_byte = match options.delimiter.as_str() {
            "tab" => b'\t',
            "semicolon" => b';',
            "pipe" => b'|',
            "auto" => detect_delimiter(&path).unwrap_or(b','),
            _ => b',',
        };

        let file = fs::File::open(&path).map_err(|e| format!("Cannot open {}: {}", source, e))?;
        let mut rdr = csv::ReaderBuilder::new()
            .has_headers(false)
            .delimiter(delimiter_byte)
            .flexible(true)
            .from_reader(file);

        let sheet = wb
            .add_worksheet()
            .set_name(&sheet_name)
            .map_err(|e| format!("Add sheet: {}", e))?;

        let mut row_idx: u32 = 0;
        let mut max_cols: usize = 0;
        let mut col_widths: Vec<usize> = Vec::new();

        for record_result in rdr.records() {
            if row_idx.is_multiple_of(500) && spreadsheet_cancelled(&options.operation_id) {
                clear_spreadsheet_cancel(&options.operation_id);
                return Err("Cancelled".into());
            }
            let record = record_result.map_err(|e| format!("CSV parse: {}", e))?;

            for (col_idx, field) in record.iter().enumerate() {
                let col = col_idx as u16;

                if options.auto_width {
                    if col_widths.len() <= col_idx {
                        col_widths.resize(col_idx + 1, 0);
                    }
                    let w = field.chars().count();
                    if w > col_widths[col_idx] {
                        col_widths[col_idx] = w;
                    }
                }

                let is_header = options.has_header && row_idx == 0;

                if is_header && options.bold_header {
                    sheet
                        .write_string_with_format(row_idx, col, field, &header_format)
                        .map_err(|e| format!("Write: {}", e))?;
                } else if options.detect_types && !is_header {
                    write_typed_cell(sheet, row_idx, col, field)
                        .map_err(|e| format!("Write: {}", e))?;
                } else {
                    sheet
                        .write_string(row_idx, col, field)
                        .map_err(|e| format!("Write: {}", e))?;
                }
            }

            max_cols = max_cols.max(record.len());
            row_idx += 1;
        }

        if options.freeze_header && options.has_header && row_idx > 0 {
            sheet
                .set_freeze_panes(1, 0)
                .map_err(|e| format!("Freeze: {}", e))?;
        }

        if options.auto_filter && options.has_header && row_idx > 0 && max_cols > 0 {
            let last_col = (max_cols - 1) as u16;
            sheet
                .autofilter(0, 0, row_idx - 1, last_col)
                .map_err(|e| format!("Autofilter: {}", e))?;
        }

        if options.auto_width {
            for (i, w) in col_widths.iter().enumerate() {
                let width = ((*w as f64) * 1.1).clamp(8.0, 60.0);
                sheet
                    .set_column_width(i as u16, width)
                    .map_err(|e| format!("Set width: {}", e))?;
            }
        }

        total_rows += row_idx as usize;
        total_sheets += 1;
        emit_spreadsheet_progress(
            &app,
            &options.operation_id,
            "csv_to_excel",
            source.clone(),
            source_index + 1,
            options.sources.len(),
            total_rows,
            false,
        );
    }

    if spreadsheet_cancelled(&options.operation_id) {
        clear_spreadsheet_cancel(&options.operation_id);
        return Err("Cancelled".into());
    }

    wb.save(&options.output_path)
        .map_err(|e| format!("Save xlsx: {}", e))?;

    clear_spreadsheet_cancel(&options.operation_id);

    Ok(CsvToExcelResult {
        output_path: options.output_path,
        total_sheets,
        total_rows,
    })
}

fn truncate_sheet_name(s: &str) -> String {
    // Excel limits sheet names to 31 chars
    if s.chars().count() <= 31 {
        s.to_string()
    } else {
        s.chars().take(31).collect()
    }
}

fn detect_delimiter(path: &Path) -> Option<u8> {
    use std::io::{BufRead, BufReader};
    let file = fs::File::open(path).ok()?;
    let mut reader = BufReader::new(file);
    let mut line = String::new();
    for _ in 0..3 {
        line.clear();
        if reader.read_line(&mut line).ok()? == 0 {
            break;
        }
        let counts = [
            (b',', line.matches(',').count()),
            (b';', line.matches(';').count()),
            (b'\t', line.matches('\t').count()),
            (b'|', line.matches('|').count()),
        ];
        let best = counts.iter().max_by_key(|(_, c)| *c)?;
        if best.1 > 0 {
            return Some(best.0);
        }
    }
    None
}

fn write_typed_cell(
    sheet: &mut rust_xlsxwriter::Worksheet,
    row: u32,
    col: u16,
    field: &str,
) -> Result<(), rust_xlsxwriter::XlsxError> {
    if field.is_empty() {
        sheet.write_blank(row, col, &Format::default())?;
        return Ok(());
    }

    // Try integer
    if let Ok(i) = field.parse::<i64>() {
        sheet.write_number(row, col, i as f64)?;
        return Ok(());
    }

    // Try float
    if let Ok(f) = field.parse::<f64>() {
        sheet.write_number(row, col, f)?;
        return Ok(());
    }

    // Bool-ish
    let lower = field.to_ascii_lowercase();
    if lower == "true" || lower == "false" {
        sheet.write_boolean(row, col, lower == "true")?;
        return Ok(());
    }

    // Default: string
    sheet.write_string(row, col, field)?;
    Ok(())
}
