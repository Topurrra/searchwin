//! Word Converter — DOCX → Markdown / plain text.
//!
//! Option D (2026-05-29): this tool does ONE thing excellently — convert
//! `.docx` to clean GitHub-Flavored Markdown — and the legacy DOCX→PDF
//! path has been removed (everyone has Save-as-PDF in Word/LibreOffice).
//!
//! The Markdown engine is a genuine OOXML reader, not a lossy text walk.
//! A `.docx` is a zip; we read the relevant parts —
//!   * `word/document.xml`        — body content
//!   * `word/styles.xml`          — style id → name (heading levels, Quote, Code)
//!   * `word/numbering.xml`       — numId → list format (bullet vs ordered)
//!   * `word/_rels/document.xml.rels` — relationship id → target (hyperlinks, images)
//! — parse the body into a small DOM (`Block` / `Inline` trees) and render
//! GFM. Supported features:
//!   * Headings 1–6 (resolved via styles.xml `w:name`, plus `Heading1`-style ids)
//!   * Inline bold / italic / strike / inline-code, combined correctly
//!     (bold+italic → `***`), with adjacent same-format runs collapsed
//!   * Underline → `<u>…</u>`
//!   * Hyperlinks (external via rels `r:id`, internal `w:anchor` → `#anchor`)
//!   * Lists (bullet `-` / ordered `1.`), nested by `w:ilvl` (2 spaces/level)
//!   * Tables → GFM pipe tables (first row = header) with `|`/newline escaping
//!   * Images → extracted to `<stem>_media/imageN.ext`, emitted as `![alt](…)`
//!   * Blockquotes (`Quote` / `IntenseQuote` styles) → `> `
//!   * Hard line breaks (`w:br`), tabs (`w:tab` → space)
//!   * Markdown-significant characters escaped in literal text
//!
//! Deferred / documented edge cases:
//!   * Nested tables render as a flattened single-line cell (GFM cannot
//!     express a table inside a cell); the inner rows are joined with
//!     ` / ` so no content is lost.
//!   * Multi-paragraph table cells join with `<br>`.
//!   * Image extraction failure never aborts the doc — we emit
//!     `![image](unavailable)` and carry on.
//!   * List numbering restarts and `w:lvlOverride` are not tracked; ordered
//!     items render as `1.` (GFM renumbers on render anyway).

use quick_xml::events::{BytesStart, Event};
use quick_xml::reader::Reader;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};
use tauri::{AppHandle, Emitter};
use walkdir::WalkDir;
use zip::ZipArchive;

static CANCELLED_WORD_MARKDOWN: LazyLock<Mutex<HashSet<String>>> =
    LazyLock::new(|| Mutex::new(HashSet::new()));
static CANCELLED_WORD_TEXT: LazyLock<Mutex<HashSet<String>>> =
    LazyLock::new(|| Mutex::new(HashSet::new()));

#[derive(Deserialize, Clone)]
pub struct WordDocxCollectionOptions {
    pub paths: Vec<String>,
    #[serde(default)]
    pub recursive: bool,
}

#[derive(Serialize, Clone)]
pub struct WordDocxCollectionResult {
    pub paths: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Deserialize, Clone)]
pub struct WordToMarkdownOptions {
    pub paths: Vec<String>,
    #[serde(default)]
    pub output_dir: Option<String>,
    #[serde(default)]
    pub operation_id: Option<String>,
}

#[derive(Serialize, Clone)]
pub struct WordToMarkdownResult {
    pub source_path: String,
    pub output_path: String,
    pub paragraphs: usize,
    pub headings: usize,
    pub characters: usize,
    pub success: bool,
    pub error: Option<String>,
    pub warnings: Vec<String>,
}

#[derive(Serialize, Clone)]
pub struct WordMarkdownProgressEvent {
    pub operation_id: String,
    pub current_file: String,
    pub files_done: usize,
    pub files_total: usize,
    pub cancelled: bool,
}

#[derive(Deserialize, Clone)]
pub struct WordToTextOptions {
    pub paths: Vec<String>,
    #[serde(default)]
    pub output_dir: Option<String>,
    #[serde(default)]
    pub operation_id: Option<String>,
}

#[derive(Serialize, Clone)]
pub struct WordToTextResult {
    pub source_path: String,
    pub output_path: String,
    pub paragraphs: usize,
    pub lines: usize,
    pub characters: usize,
    pub success: bool,
    pub error: Option<String>,
    pub warnings: Vec<String>,
}

#[derive(Serialize, Clone)]
pub struct WordTextProgressEvent {
    pub operation_id: String,
    pub current_file: String,
    pub files_done: usize,
    pub files_total: usize,
    pub cancelled: bool,
}

fn is_docx_path(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("docx"))
}

fn normalize_path_key(path: &str) -> String {
    path.to_lowercase()
}

fn collect_docx_paths_from_dir(
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
            if is_docx_path(entry_path) {
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
        if item.file_type().map(|ty| ty.is_file()).unwrap_or(false) && is_docx_path(&item_path) {
            let key = normalize_path_key(&item_path.to_string_lossy());
            if seen.insert(key) {
                paths.push(item_path.to_string_lossy().into());
            }
        }
    }
}

#[tauri::command]
pub fn collect_word_docx_sources(
    options: WordDocxCollectionOptions,
) -> Result<WordDocxCollectionResult, String> {
    // Security gate: every input path the user supplied must be real +
    // non-system before we walk it.
    for path in &options.paths {
        crate::core::safe_path::validate_user_path(path)?;
    }

    let mut paths: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut warnings: Vec<String> = Vec::new();

    if options.paths.is_empty() {
        return Ok(WordDocxCollectionResult { paths, warnings });
    }

    for root in options.paths {
        let path = Path::new(&root);
        if !path.exists() {
            warnings.push(format!("Path does not exist: {root}"));
            continue;
        }

        if path.is_file() {
            if is_docx_path(path) {
                let key = normalize_path_key(&path.to_string_lossy());
                if seen.insert(key) {
                    paths.push(path.to_string_lossy().into());
                }
            } else {
                warnings.push(format!("Ignored non-DOCX file: {}", path.display()));
            }
            continue;
        }

        if path.is_dir() {
            collect_docx_paths_from_dir(
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
    Ok(WordDocxCollectionResult { paths, warnings })
}

#[tauri::command]
pub fn cancel_word_markdown_operation(operation_id: String) -> Result<(), String> {
    CANCELLED_WORD_MARKDOWN
        .lock()
        .map_err(|_| "Cancel registry lock failed".to_string())?
        .insert(operation_id);
    Ok(())
}

#[tauri::command]
pub fn cancel_word_text_operation(operation_id: String) -> Result<(), String> {
    CANCELLED_WORD_TEXT
        .lock()
        .map_err(|_| "Cancel registry lock failed".to_string())?
        .insert(operation_id);
    Ok(())
}

fn is_markdown_cancelled(operation_id: &Option<String>) -> bool {
    operation_id
        .as_ref()
        .and_then(|id| {
            CANCELLED_WORD_MARKDOWN
                .lock()
                .ok()
                .map(|set| set.contains(id))
        })
        .unwrap_or(false)
}

fn is_text_cancelled(operation_id: &Option<String>) -> bool {
    operation_id
        .as_ref()
        .and_then(|id| CANCELLED_WORD_TEXT.lock().ok().map(|set| set.contains(id)))
        .unwrap_or(false)
}

fn clear_markdown_cancel(operation_id: &Option<String>) {
    if let Some(id) = operation_id {
        if let Ok(mut set) = CANCELLED_WORD_MARKDOWN.lock() {
            set.remove(id);
        }
    }
}

fn clear_text_cancel(operation_id: &Option<String>) {
    if let Some(id) = operation_id {
        if let Ok(mut set) = CANCELLED_WORD_TEXT.lock() {
            set.remove(id);
        }
    }
}

fn emit_markdown_progress(
    app: &AppHandle,
    opts: &WordToMarkdownOptions,
    current_file: String,
    files_done: usize,
    cancelled: bool,
) {
    if let Some(operation_id) = &opts.operation_id {
        let _ = app.emit(
            "word-markdown-progress",
            WordMarkdownProgressEvent {
                operation_id: operation_id.clone(),
                current_file,
                files_done,
                files_total: opts.paths.len(),
                cancelled,
            },
        );
    }
}

fn emit_text_progress(
    app: &AppHandle,
    opts: &WordToTextOptions,
    current_file: String,
    files_done: usize,
    cancelled: bool,
) {
    if let Some(operation_id) = &opts.operation_id {
        let _ = app.emit(
            "word-text-progress",
            WordTextProgressEvent {
                operation_id: operation_id.clone(),
                current_file,
                files_done,
                files_total: opts.paths.len(),
                cancelled,
            },
        );
    }
}

#[tauri::command]
pub async fn word_to_markdown(
    app: AppHandle,
    options: WordToMarkdownOptions,
) -> Result<Vec<WordToMarkdownResult>, String> {
    // Security gate: every input Word doc must be real + non-system; the
    // optional output_dir must not target a forbidden location.
    for path in &options.paths {
        crate::core::safe_path::validate_user_path(path)?;
    }
    if let Some(ref dir) = options.output_dir {
        crate::core::safe_path::forbid_system_path(dir)?;
    }

    clear_markdown_cancel(&options.operation_id);

    tauri::async_runtime::spawn_blocking(move || {
        let mut results = Vec::with_capacity(options.paths.len());

        for (idx, p) in options.paths.iter().enumerate() {
            if is_markdown_cancelled(&options.operation_id) {
                emit_markdown_progress(&app, &options, String::new(), idx, true);
                break;
            }

            emit_markdown_progress(&app, &options, p.clone(), idx, false);
            let result = convert_one_markdown(p, &options);
            results.push(result);
            emit_markdown_progress(&app, &options, p.clone(), idx + 1, false);
        }

        clear_markdown_cancel(&options.operation_id);
        Ok(results)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn word_to_text(
    app: AppHandle,
    options: WordToTextOptions,
) -> Result<Vec<WordToTextResult>, String> {
    // Security gate: every input Word doc must be real + non-system; the
    // optional output_dir must not target a forbidden location.
    for path in &options.paths {
        crate::core::safe_path::validate_user_path(path)?;
    }
    if let Some(ref dir) = options.output_dir {
        crate::core::safe_path::forbid_system_path(dir)?;
    }

    clear_text_cancel(&options.operation_id);

    tauri::async_runtime::spawn_blocking(move || {
        let mut results = Vec::with_capacity(options.paths.len());

        for (idx, p) in options.paths.iter().enumerate() {
            if is_text_cancelled(&options.operation_id) {
                emit_text_progress(&app, &options, String::new(), idx, true);
                break;
            }

            emit_text_progress(&app, &options, p.clone(), idx, false);
            let result = convert_one_text(p, &options);
            results.push(result);
            emit_text_progress(&app, &options, p.clone(), idx + 1, false);
        }

        clear_text_cancel(&options.operation_id);
        Ok(results)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Render a single `.docx` to Markdown for the command-palette preview.
/// Best-effort: returns `Ok(None)` for non-`.docx` inputs (`.doc`/`.rtf`/
/// `.odt` aren't the OOXML zip the converter reads — the frontend falls
/// back to its flat-text view) and `Ok(None)` on any converter error, so
/// the preview degrades gracefully instead of surfacing an error. Images
/// are not extracted here (`media = None`), so inline image refs may be
/// blank — acceptable for a preview.
#[tauri::command(async)]
pub fn read_docx_markdown(path: String) -> Result<Option<String>, String> {
    let is_docx = Path::new(&path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("docx"))
        .unwrap_or(false);
    if !is_docx {
        return Ok(None);
    }

    match docx_to_markdown(Path::new(&path), None) {
        Ok(conv) => Ok(Some(conv.markdown)),
        Err(_) => Ok(None),
    }
}

fn convert_one_markdown(source: &str, opts: &WordToMarkdownOptions) -> WordToMarkdownResult {
    let src = PathBuf::from(source);

    let output_path = match &opts.output_dir {
        Some(dir) => {
            let dir_path = Path::new(dir);
            if let Err(e) = fs::create_dir_all(dir_path) {
                return error_markdown_result(source, e.to_string());
            }

            dir_path.join(
                src.file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string()
                    + ".md",
            )
        }
        None => src.with_extension("md"),
    };

    // The media folder is a sibling of the output .md named "<stem>_media".
    let output_stem = output_path
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    let media_dir_name = format!("{output_stem}_media");
    let media_dir = output_path
        .parent()
        .map(|p| p.join(&media_dir_name))
        .unwrap_or_else(|| PathBuf::from(&media_dir_name));

    let Conversion {
        markdown, warnings, ..
    } = match docx_to_markdown(&src, Some((&media_dir, &media_dir_name))) {
        Ok(conv) => conv,
        Err(e) => return error_markdown_result(source, e),
    };

    let mut out = match fs::File::create(&output_path) {
        Ok(file) => file,
        Err(e) => return error_markdown_result(source, e.to_string()),
    };

    if let Err(e) = out.write_all(markdown.as_bytes()) {
        return error_markdown_result(source, e.to_string());
    }

    let mut paragraphs = 0usize;
    let mut headings = 0usize;
    for line in markdown.lines() {
        let text = line.trim();
        if text.is_empty() {
            continue;
        }
        paragraphs += 1;
        if text.starts_with('#') {
            headings += 1;
        }
    }

    WordToMarkdownResult {
        source_path: source.into(),
        output_path: output_path.to_string_lossy().into(),
        paragraphs,
        headings,
        characters: markdown.chars().count(),
        success: true,
        error: None,
        warnings,
    }
}

fn convert_one_text(source: &str, opts: &WordToTextOptions) -> WordToTextResult {
    let src = PathBuf::from(source);

    let output_path = match &opts.output_dir {
        Some(dir) => {
            let dir_path = Path::new(dir);
            if let Err(e) = fs::create_dir_all(dir_path) {
                return error_text_result(source, e.to_string());
            }

            dir_path.join(
                src.file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string()
                    + ".txt",
            )
        }
        None => src.with_extension("txt"),
    };

    let plain_text = match docx_to_text(&src) {
        Ok(text) => text,
        Err(e) => return error_text_result(source, e),
    };

    let mut out = match fs::File::create(&output_path) {
        Ok(file) => file,
        Err(e) => return error_text_result(source, e.to_string()),
    };

    if let Err(e) = out.write_all(plain_text.as_bytes()) {
        return error_text_result(source, e.to_string());
    }

    let mut paragraphs = 0usize;
    let mut lines = 0usize;
    for line in plain_text.lines() {
        lines += 1;
        if !line.trim().is_empty() {
            paragraphs += 1;
        }
    }

    WordToTextResult {
        source_path: source.into(),
        output_path: output_path.to_string_lossy().into(),
        paragraphs,
        lines,
        characters: plain_text.chars().count(),
        success: true,
        error: None,
        warnings: vec![],
    }
}

fn error_markdown_result(source: &str, err: String) -> WordToMarkdownResult {
    WordToMarkdownResult {
        source_path: source.into(),
        output_path: "".into(),
        paragraphs: 0,
        headings: 0,
        characters: 0,
        success: false,
        error: Some(err),
        warnings: vec![],
    }
}

fn error_text_result(source: &str, err: String) -> WordToTextResult {
    WordToTextResult {
        source_path: source.into(),
        output_path: "".into(),
        paragraphs: 0,
        lines: 0,
        characters: 0,
        success: false,
        error: Some(err),
        warnings: vec![],
    }
}

// ════════════════════════════════════════════════════════════════════════
//  OOXML → Markdown engine
// ════════════════════════════════════════════════════════════════════════

/// Result of converting one document.
struct Conversion {
    markdown: String,
    warnings: Vec<String>,
    /// Plain-text rendering (used by the text exporter so it doesn't have to
    /// strip markdown). Populated only when requested.
    plain_text: String,
}

/// How a `numId` formats each indent level: `true` = ordered, `false` = bullet.
#[derive(Default, Clone)]
struct NumFormat {
    /// Per-level ordered flag. Index = ilvl. Missing levels default to bullet.
    ordered_levels: HashMap<u8, bool>,
}

impl NumFormat {
    fn is_ordered(&self, ilvl: u8) -> bool {
        // Fall back to level 0's format if the exact level is unknown, then
        // bullet if nothing is known at all.
        self.ordered_levels
            .get(&ilvl)
            .or_else(|| self.ordered_levels.get(&0))
            .copied()
            .unwrap_or(false)
    }
}

/// Everything we resolve from the auxiliary parts before walking the body.
#[derive(Default)]
struct DocxContext {
    /// style id → heading level (1..=6)
    heading_styles: HashMap<String, u8>,
    /// style ids that mean "blockquote"
    quote_styles: HashSet<String>,
    /// style ids that mean "inline code" (paragraph or character)
    code_styles: HashSet<String>,
    /// relationship id → target string (URL or media path)
    rels: HashMap<String, RelTarget>,
    /// numId → abstractNumId
    num_to_abstract: HashMap<String, String>,
    /// abstractNumId → format-per-level
    abstract_formats: HashMap<String, NumFormat>,
}

#[derive(Clone)]
struct RelTarget {
    target: String,
    /// true when `TargetMode="External"` (hyperlinks); media is internal.
    external: bool,
}

impl DocxContext {
    fn num_format(&self, num_id: &str) -> Option<&NumFormat> {
        let abstract_id = self.num_to_abstract.get(num_id)?;
        self.abstract_formats.get(abstract_id)
    }
}

/// Inline formatting flags carried by a run.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
struct RunFmt {
    bold: bool,
    italic: bool,
    strike: bool,
    code: bool,
    underline: bool,
}

/// A leaf of inline content.
enum Inline {
    /// Literal text + its formatting.
    Text { text: String, fmt: RunFmt },
    /// A hyperlink: display children + destination URL/anchor.
    Link { children: Vec<Inline>, href: String },
    /// An image reference: alt text + path relative to the .md file.
    Image { alt: String, path: String },
    /// A hard line break (`w:br`).
    Break,
}

/// What kind of block a paragraph is.
enum BlockKind {
    Paragraph,
    Heading(u8),
    Quote,
    /// List item: (ordered, indent level).
    ListItem { ordered: bool, level: u8 },
}

struct Paragraph {
    kind: BlockKind,
    inlines: Vec<Inline>,
}

enum Block {
    Para(Paragraph),
    Table(Table),
}

struct Table {
    /// Each row is a list of cells; each cell is a list of blocks (usually
    /// paragraphs, but a cell can hold a nested table).
    rows: Vec<Vec<Vec<Block>>>,
}

// ─── Top-level entry points ───────────────────────────────────────────────

/// Convert a DOCX into Markdown. When `media` is `Some((dir, rel_name))`,
/// referenced images are extracted into `dir` and emitted with paths
/// prefixed by `rel_name` (the media folder's name relative to the .md).
fn docx_to_markdown(
    source: &Path,
    media: Option<(&Path, &str)>,
) -> Result<Conversion, String> {
    let file = fs::File::open(source).map_err(|e| format!("Cannot open: {e}"))?;
    let mut archive =
        ZipArchive::new(file).map_err(|e| format!("Not a valid DOCX file: {e}"))?;

    let document_xml = read_zip_text(&mut archive, "word/document.xml")
        .map_err(|_| "DOCX document.xml missing".to_string())?;

    // Auxiliary parts are best-effort: a doc without them still converts.
    let styles_xml = read_zip_text(&mut archive, "word/styles.xml").ok();
    let numbering_xml = read_zip_text(&mut archive, "word/numbering.xml").ok();
    let rels_xml = read_zip_text(&mut archive, "word/_rels/document.xml.rels").ok();

    let mut ctx = DocxContext::default();
    if let Some(xml) = &styles_xml {
        parse_styles(xml, &mut ctx);
    }
    if let Some(xml) = &numbering_xml {
        parse_numbering(xml, &mut ctx);
    }
    if let Some(xml) = &rels_xml {
        parse_rels(xml, &mut ctx);
    }

    let mut warnings: Vec<String> = Vec::new();

    // Parse the body into a block tree.
    let blocks = parse_body(&document_xml, &ctx, media, &mut archive, &mut warnings)?;

    // Render.
    let markdown = render_blocks(&blocks);
    let plain_text = render_plain(&blocks);

    Ok(Conversion {
        markdown,
        warnings,
        plain_text,
    })
}

fn docx_to_text(source: &Path) -> Result<String, String> {
    // No media extraction for plain text; images become their alt text.
    docx_to_markdown(source, None).map(|c| c.plain_text)
}

fn read_zip_text<R: Read + std::io::Seek>(
    archive: &mut ZipArchive<R>,
    name: &str,
) -> Result<String, String> {
    let mut entry = archive
        .by_name(name)
        .map_err(|_| format!("{name} missing"))?;
    let mut xml = String::new();
    entry
        .read_to_string(&mut xml)
        .map_err(|e| format!("Cannot read {name}: {e}"))?;
    Ok(xml)
}

// ─── styles.xml ────────────────────────────────────────────────────────────

/// Resolve style ids → heading level / quote / code by reading each
/// `w:style`'s `w:styleId` + `w:name`. We accept both the human name
/// ("heading 1", "Quote") and common ids ("Heading1", "Heading2").
fn parse_styles(xml: &str, ctx: &mut DocxContext) {
    let mut reader = Reader::from_reader(Cursor::new(xml.as_bytes()));
    reader.config_mut().trim_text(false);
    let mut buf = Vec::new();

    let mut cur_id: Option<String> = None;
    let mut cur_name: Option<String> = None;

    loop {
        match reader.read_event_into(&mut buf) {
            Err(_) => break,
            Ok(Event::Eof) => break,
            Ok(Event::Start(e)) if e.name().as_ref() == b"w:style" => {
                cur_id = attr_val(&e, b"w:styleId");
                cur_name = None;
            }
            Ok(Event::Empty(e)) if e.name().as_ref() == b"w:name" => {
                if let Some(v) = attr_val(&e, b"w:val") {
                    cur_name = Some(v);
                }
            }
            Ok(Event::Start(e)) if e.name().as_ref() == b"w:name" => {
                if let Some(v) = attr_val(&e, b"w:val") {
                    cur_name = Some(v);
                }
            }
            Ok(Event::End(e)) if e.name().as_ref() == b"w:style" => {
                if let Some(id) = cur_id.take() {
                    classify_style(&id, cur_name.as_deref(), ctx);
                }
                cur_name = None;
            }
            Ok(_) => {}
        }
        buf.clear();
    }
}

/// Decide what a style id means based on its id and human name.
fn classify_style(id: &str, name: Option<&str>, ctx: &mut DocxContext) {
    let id_lc = id.to_ascii_lowercase();
    let name_lc = name.map(|n| n.to_ascii_lowercase());

    // Heading level — prefer the human name ("heading 1"), fall back to id.
    if let Some(level) = heading_level_from_label(name_lc.as_deref())
        .or_else(|| heading_level_from_label(Some(&id_lc)))
    {
        ctx.heading_styles.insert(id.to_string(), level);
        return;
    }

    let matches = |needle: &str| {
        id_lc.contains(needle) || name_lc.as_deref().is_some_and(|n| n.contains(needle))
    };

    if matches("quote") {
        ctx.quote_styles.insert(id.to_string());
    }
    // "Code", "SourceCode", "HTMLCode", "VerbatimChar" → inline code.
    if matches("code") || matches("verbatim") {
        ctx.code_styles.insert(id.to_string());
    }
}

/// Map a style label like "heading 1", "heading1", "title", "subtitle" to a
/// Markdown heading level. Returns `None` for non-heading labels.
fn heading_level_from_label(label: Option<&str>) -> Option<u8> {
    let label = label?;
    let normalized = label.replace(' ', "");
    if normalized == "title" {
        return Some(1);
    }
    if normalized == "subtitle" {
        return Some(2);
    }
    let rest = normalized.strip_prefix("heading")?;
    let level = rest.parse::<u8>().ok()?;
    (1..=6).contains(&level).then_some(level)
}

// ─── numbering.xml ───────────────────────────────────────────────────────

/// Resolve `numId → abstractNumId` and `abstractNumId → per-level format`.
/// A level is "ordered" unless its `w:numFmt` is `bullet` (or `none`).
fn parse_numbering(xml: &str, ctx: &mut DocxContext) {
    let mut reader = Reader::from_reader(Cursor::new(xml.as_bytes()));
    reader.config_mut().trim_text(false);
    let mut buf = Vec::new();

    let mut cur_abstract: Option<String> = None;
    let mut cur_abstract_fmt = NumFormat::default();
    let mut cur_level: Option<u8> = None;

    // For <w:num w:numId> → <w:abstractNumId w:val>.
    let mut cur_num_id: Option<String> = None;

    loop {
        match reader.read_event_into(&mut buf) {
            Err(_) => break,
            Ok(Event::Eof) => break,
            Ok(Event::Start(e)) => match e.name().as_ref() {
                b"w:abstractNum" => {
                    cur_abstract = attr_val(&e, b"w:abstractNumId");
                    cur_abstract_fmt = NumFormat::default();
                }
                b"w:lvl" => {
                    cur_level = attr_val(&e, b"w:ilvl").and_then(|v| v.parse::<u8>().ok());
                }
                b"w:num" => {
                    cur_num_id = attr_val(&e, b"w:numId");
                }
                _ => {}
            },
            Ok(Event::Empty(e)) => match e.name().as_ref() {
                b"w:numFmt" => {
                    if let (Some(level), Some(fmt)) = (cur_level, attr_val(&e, b"w:val")) {
                        let fmt_lc = fmt.to_ascii_lowercase();
                        let ordered = fmt_lc != "bullet" && fmt_lc != "none";
                        cur_abstract_fmt.ordered_levels.insert(level, ordered);
                    }
                }
                b"w:abstractNumId" => {
                    // Inside <w:num>: links numId → abstractNumId.
                    if let (Some(num_id), Some(abs)) =
                        (cur_num_id.as_ref(), attr_val(&e, b"w:val"))
                    {
                        ctx.num_to_abstract.insert(num_id.clone(), abs);
                    }
                }
                _ => {}
            },
            Ok(Event::End(e)) => match e.name().as_ref() {
                b"w:lvl" => {
                    cur_level = None;
                }
                b"w:abstractNum" => {
                    if let Some(id) = cur_abstract.take() {
                        ctx.abstract_formats
                            .insert(id, std::mem::take(&mut cur_abstract_fmt));
                    }
                }
                b"w:num" => {
                    cur_num_id = None;
                }
                _ => {}
            },
            Ok(_) => {}
        }
        buf.clear();
    }
}

// ─── document.xml.rels ─────────────────────────────────────────────────────

/// Parse relationship id → target. Hyperlinks carry `TargetMode="External"`;
/// images point at internal media parts.
fn parse_rels(xml: &str, ctx: &mut DocxContext) {
    let mut reader = Reader::from_reader(Cursor::new(xml.as_bytes()));
    reader.config_mut().trim_text(false);
    let mut buf = Vec::new();

    let mut handle = |e: &BytesStart| {
        if e.name().as_ref() != b"Relationship" {
            return;
        }
        let id = attr_val(e, b"Id");
        let target = attr_val(e, b"Target");
        let mode = attr_val(e, b"TargetMode");
        if let (Some(id), Some(target)) = (id, target) {
            let external = mode.as_deref() == Some("External");
            ctx.rels.insert(id, RelTarget { target, external });
        }
    };

    loop {
        match reader.read_event_into(&mut buf) {
            Err(_) => break,
            Ok(Event::Eof) => break,
            Ok(Event::Empty(e)) => handle(&e),
            Ok(Event::Start(e)) => handle(&e),
            Ok(_) => {}
        }
        buf.clear();
    }
}

// ─── Body parser (document.xml) ────────────────────────────────────────────

/// Walk `document.xml` into a `Vec<Block>`. Uses depth tracking so a `w:tbl`
/// inside a `w:tc` is parsed recursively into a nested table block.
fn parse_body<R: Read + std::io::Seek>(
    xml: &str,
    ctx: &DocxContext,
    media: Option<(&Path, &str)>,
    archive: &mut ZipArchive<R>,
    warnings: &mut Vec<String>,
) -> Result<Vec<Block>, String> {
    // Find <w:body>…</w:body> and parse its children recursively.
    let mut reader = Reader::from_reader(Cursor::new(xml.as_bytes()));
    reader.config_mut().trim_text(false);
    let mut buf = Vec::new();

    // Skip to <w:body>.
    loop {
        match reader.read_event_into(&mut buf) {
            Err(e) => return Err(format!("XML parse failed: {e}")),
            Ok(Event::Eof) => return Ok(Vec::new()),
            Ok(Event::Start(e)) if e.name().as_ref() == b"w:body" => break,
            Ok(_) => {}
        }
        buf.clear();
    }

    let mut state = BodyState {
        ctx,
        media,
        archive,
        warnings,
        media_seq: 0,
    };
    parse_block_sequence(&mut reader, &mut buf, b"w:body", &mut state)
}

/// Mutable state threaded through the recursive body walk.
struct BodyState<'a, R: Read + std::io::Seek> {
    ctx: &'a DocxContext,
    media: Option<(&'a Path, &'a str)>,
    archive: &'a mut ZipArchive<R>,
    warnings: &'a mut Vec<String>,
    /// De-dupes media extraction so the same image isn't written twice.
    media_seq: usize,
}

/// Parse a run of block-level elements (`w:p` and `w:tbl`) until `end_tag`
/// closes. Used for the body and for table-cell contents.
fn parse_block_sequence<R: Read + std::io::Seek>(
    reader: &mut Reader<Cursor<&[u8]>>,
    buf: &mut Vec<u8>,
    end_tag: &[u8],
    state: &mut BodyState<R>,
) -> Result<Vec<Block>, String> {
    let mut blocks: Vec<Block> = Vec::new();

    loop {
        match reader.read_event_into(buf) {
            Err(e) => return Err(format!("XML parse failed: {e}")),
            Ok(Event::Eof) => break,
            Ok(Event::Start(e)) => match e.name().as_ref() {
                b"w:p" => {
                    let para = parse_paragraph(reader, buf, state)?;
                    blocks.push(Block::Para(para));
                }
                b"w:tbl" => {
                    let table = parse_table(reader, buf, state)?;
                    blocks.push(Block::Table(table));
                }
                _ => {}
            },
            Ok(Event::End(e)) if e.name().as_ref() == end_tag => break,
            Ok(_) => {}
        }
        buf.clear();
    }

    Ok(blocks)
}

/// Parse one `<w:p>…</w:p>` (cursor is positioned just after the start tag).
fn parse_paragraph<R: Read + std::io::Seek>(
    reader: &mut Reader<Cursor<&[u8]>>,
    buf: &mut Vec<u8>,
    state: &mut BodyState<R>,
) -> Result<Paragraph, String> {
    let mut pstyle: Option<String> = None;
    let mut num_id: Option<String> = None;
    let mut ilvl: u8 = 0;
    let mut has_num = false;

    let mut inlines: Vec<Inline> = Vec::new();

    // Accumulator for collapsing adjacent same-format runs.
    let mut run_acc = RunAccumulator::default();

    loop {
        match reader.read_event_into(buf) {
            Err(e) => return Err(format!("XML parse failed: {e}")),
            Ok(Event::Eof) => break,
            Ok(Event::Start(e)) => match e.name().as_ref() {
                b"w:pPr" => {
                    // Paragraph properties — parse into pstyle/num.
                    parse_ppr(reader, buf, &mut pstyle, &mut num_id, &mut ilvl, &mut has_num)?;
                }
                b"w:hyperlink" => {
                    run_acc.flush(&mut inlines);
                    let (rid, anchor) = hyperlink_attrs(&e);
                    let href = resolve_href(state.ctx, rid.as_deref(), anchor.as_deref());
                    if let Some(link) = parse_hyperlink_inner(reader, buf, state, href)? {
                        inlines.push(link);
                    }
                }
                b"w:r" => {
                    parse_run(reader, buf, state, &mut run_acc, &mut inlines)?;
                }
                _ => {}
            },
            Ok(Event::End(e)) if e.name().as_ref() == b"w:p" => break,
            Ok(_) => {}
        }
        buf.clear();
    }

    run_acc.flush(&mut inlines);

    // Decide the block kind.
    let kind = classify_paragraph(state.ctx, pstyle.as_deref(), has_num, num_id.as_deref(), ilvl);

    Ok(Paragraph { kind, inlines })
}

fn classify_paragraph(
    ctx: &DocxContext,
    pstyle: Option<&str>,
    has_num: bool,
    num_id: Option<&str>,
    ilvl: u8,
) -> BlockKind {
    if has_num {
        // numId 0 conventionally means "no numbering" (a removed list).
        let ordered = match num_id {
            Some("0") | None => false,
            Some(id) => ctx
                .num_format(id)
                .map(|f| f.is_ordered(ilvl))
                .unwrap_or(false),
        };
        return BlockKind::ListItem { ordered, level: ilvl };
    }
    if let Some(style) = pstyle {
        if let Some(&level) = ctx.heading_styles.get(style) {
            return BlockKind::Heading(level);
        }
        // Also accept bare "Heading1"-style ids even if styles.xml was absent.
        if let Some(level) = heading_level_from_label(Some(&style.to_ascii_lowercase())) {
            return BlockKind::Heading(level);
        }
        if ctx.quote_styles.contains(style) {
            return BlockKind::Quote;
        }
    }
    BlockKind::Paragraph
}

/// Parse `<w:pPr>` for style + numbering. Cursor is after the start tag.
fn parse_ppr(
    reader: &mut Reader<Cursor<&[u8]>>,
    buf: &mut Vec<u8>,
    pstyle: &mut Option<String>,
    num_id: &mut Option<String>,
    ilvl: &mut u8,
    has_num: &mut bool,
) -> Result<(), String> {
    loop {
        match reader.read_event_into(buf) {
            Err(e) => return Err(format!("XML parse failed: {e}")),
            Ok(Event::Eof) => break,
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => match e.name().as_ref() {
                b"w:pStyle" => {
                    if let Some(v) = attr_val(&e, b"w:val") {
                        *pstyle = Some(v);
                    }
                }
                b"w:numPr" => {
                    *has_num = true;
                }
                b"w:ilvl" => {
                    if let Some(v) = attr_val(&e, b"w:val").and_then(|v| v.parse::<u8>().ok()) {
                        *ilvl = v;
                    }
                }
                b"w:numId" => {
                    if let Some(v) = attr_val(&e, b"w:val") {
                        *num_id = Some(v);
                    }
                }
                _ => {}
            },
            Ok(Event::End(e)) if e.name().as_ref() == b"w:pPr" => break,
            Ok(_) => {}
        }
        buf.clear();
    }
    Ok(())
}

/// Accumulates run text under a single format so consecutive same-format runs
/// merge into one `Inline::Text` (avoids `**a****b**`).
#[derive(Default)]
struct RunAccumulator {
    text: String,
    fmt: RunFmt,
    active: bool,
}

impl RunAccumulator {
    fn push(&mut self, text: &str, fmt: RunFmt, out: &mut Vec<Inline>) {
        if self.active && self.fmt == fmt {
            self.text.push_str(text);
        } else {
            self.flush(out);
            self.text.push_str(text);
            self.fmt = fmt;
            self.active = true;
        }
    }

    fn flush(&mut self, out: &mut Vec<Inline>) {
        if self.active && !self.text.is_empty() {
            out.push(Inline::Text {
                text: std::mem::take(&mut self.text),
                fmt: self.fmt,
            });
        }
        self.text.clear();
        self.active = false;
    }

    fn push_inline(&mut self, inline: Inline, out: &mut Vec<Inline>) {
        self.flush(out);
        out.push(inline);
    }
}

/// Parse one `<w:r>…</w:r>` run: reads `<w:rPr>` for formatting, then text /
/// breaks / tabs / drawings. Appends to the accumulator + inline list.
fn parse_run<R: Read + std::io::Seek>(
    reader: &mut Reader<Cursor<&[u8]>>,
    buf: &mut Vec<u8>,
    state: &mut BodyState<R>,
    run_acc: &mut RunAccumulator,
    inlines: &mut Vec<Inline>,
) -> Result<(), String> {
    let mut fmt = RunFmt::default();

    loop {
        match reader.read_event_into(buf) {
            Err(e) => return Err(format!("XML parse failed: {e}")),
            Ok(Event::Eof) => break,
            Ok(Event::Start(e)) => match e.name().as_ref() {
                b"w:rPr" => {
                    fmt = parse_rpr(reader, buf, state.ctx)?;
                }
                b"w:t" => {
                    let text = read_text_element(reader, buf, b"w:t")?;
                    run_acc.push(&text, fmt, inlines);
                }
                b"w:drawing" | b"w:pict" => {
                    let end = e.name().as_ref().to_vec();
                    if let Some(img) = parse_drawing(reader, buf, &end, state)? {
                        run_acc.push_inline(img, inlines);
                    }
                }
                _ => {}
            },
            Ok(Event::Empty(e)) => match e.name().as_ref() {
                b"w:br" | b"w:cr" => {
                    run_acc.push_inline(Inline::Break, inlines);
                }
                b"w:tab" => {
                    // Tab → single space (collapsed by paragraph join anyway).
                    run_acc.push(" ", fmt, inlines);
                }
                _ => {}
            },
            Ok(Event::End(e)) if e.name().as_ref() == b"w:r" => break,
            Ok(_) => {}
        }
        buf.clear();
    }

    Ok(())
}

/// Parse `<w:rPr>` into a `RunFmt`. Cursor is after the start tag.
fn parse_rpr(
    reader: &mut Reader<Cursor<&[u8]>>,
    buf: &mut Vec<u8>,
    ctx: &DocxContext,
) -> Result<RunFmt, String> {
    let mut fmt = RunFmt::default();

    loop {
        match reader.read_event_into(buf) {
            Err(e) => return Err(format!("XML parse failed: {e}")),
            Ok(Event::Eof) => break,
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => match e.name().as_ref() {
                b"w:b" | b"w:bCs" => {
                    if toggle_is_on(&e) {
                        fmt.bold = true;
                    }
                }
                b"w:i" | b"w:iCs" => {
                    if toggle_is_on(&e) {
                        fmt.italic = true;
                    }
                }
                b"w:strike" | b"w:dstrike" => {
                    if toggle_is_on(&e) {
                        fmt.strike = true;
                    }
                }
                b"w:u" => {
                    // Underline is "on" unless explicitly val="none".
                    if attr_val(&e, b"w:val").as_deref() != Some("none") {
                        fmt.underline = true;
                    }
                }
                b"w:rStyle" => {
                    if let Some(style) = attr_val(&e, b"w:val") {
                        if ctx.code_styles.contains(&style)
                            || style.to_ascii_lowercase().contains("code")
                        {
                            fmt.code = true;
                        }
                    }
                }
                b"w:rFonts" => {
                    if is_monospace_font(&e) {
                        fmt.code = true;
                    }
                }
                _ => {}
            },
            Ok(Event::End(e)) if e.name().as_ref() == b"w:rPr" => break,
            Ok(_) => {}
        }
        buf.clear();
    }

    Ok(fmt)
}

/// A boolean run property (`w:b`, `w:i`, …) is ON unless it carries
/// `w:val="false"`/`"0"`/`"off"`.
fn toggle_is_on(e: &BytesStart) -> bool {
    match attr_val(e, b"w:val") {
        None => true,
        Some(v) => !matches!(v.as_str(), "false" | "0" | "off"),
    }
}

/// Detect a monospace font on `<w:rFonts>` (Consolas, Courier, …) → inline code.
fn is_monospace_font(e: &BytesStart) -> bool {
    const MONO: [&str; 6] = [
        "consolas",
        "courier",
        "courier new",
        "lucida console",
        "monaco",
        "menlo",
    ];
    for key in [b"w:ascii".as_ref(), b"w:hAnsi".as_ref(), b"w:cs".as_ref()] {
        if let Some(font) = attr_val(e, key) {
            let f = font.to_ascii_lowercase();
            if MONO.iter().any(|m| f.contains(m)) {
                return true;
            }
        }
    }
    false
}

/// Read the text content of a simple element like `<w:t>…</w:t>`. Handles
/// `xml:space="preserve"` (text is taken verbatim) and CDATA.
fn read_text_element(
    reader: &mut Reader<Cursor<&[u8]>>,
    buf: &mut Vec<u8>,
    end_tag: &[u8],
) -> Result<String, String> {
    let mut text = String::new();
    loop {
        match reader.read_event_into(buf) {
            Err(e) => return Err(format!("XML decode failed: {e}")),
            Ok(Event::Eof) => break,
            Ok(Event::Text(t)) => {
                let decoded = t
                    .unescape()
                    .map_err(|err| format!("XML decode failed: {err}"))?;
                text.push_str(decoded.as_ref());
            }
            Ok(Event::CData(c)) => {
                text.push_str(&String::from_utf8_lossy(c.as_ref()));
            }
            Ok(Event::End(e)) if e.name().as_ref() == end_tag => break,
            Ok(_) => {}
        }
        buf.clear();
    }
    Ok(text)
}

/// Parse `<w:hyperlink>` children — receives the resolved destination
/// (external URL via rels `r:id`, or `#anchor`), walks child runs into an
/// `Inline::Link`. With no destination, the children collapse to plain text.
fn parse_hyperlink_inner<R: Read + std::io::Seek>(
    reader: &mut Reader<Cursor<&[u8]>>,
    buf: &mut Vec<u8>,
    state: &mut BodyState<R>,
    href: Option<String>,
) -> Result<Option<Inline>, String> {
    let mut children: Vec<Inline> = Vec::new();
    let mut run_acc = RunAccumulator::default();

    loop {
        match reader.read_event_into(buf) {
            Err(e) => return Err(format!("XML parse failed: {e}")),
            Ok(Event::Eof) => break,
            Ok(Event::Start(e)) => match e.name().as_ref() {
                b"w:r" => {
                    parse_run(reader, buf, state, &mut run_acc, &mut children)?;
                }
                // Nested hyperlink is unusual; flatten its runs in-place.
                b"w:hyperlink" => {
                    let (rid, anchor) = hyperlink_attrs(&e);
                    let nested_href = resolve_href(state.ctx, rid.as_deref(), anchor.as_deref());
                    if let Some(inner) =
                        parse_hyperlink_inner(reader, buf, state, nested_href)?
                    {
                        run_acc.push_inline(inner, &mut children);
                    }
                }
                _ => {}
            },
            Ok(Event::End(e)) if e.name().as_ref() == b"w:hyperlink" => break,
            Ok(_) => {}
        }
        buf.clear();
    }

    run_acc.flush(&mut children);

    if children.is_empty() {
        return Ok(None);
    }

    match href {
        Some(href) => Ok(Some(Inline::Link { children, href })),
        // No destination → emit the children as plain inline text.
        None => {
            // Wrap children in a transparent link-less container by returning
            // a single text join is lossy for formatting; instead emit the
            // first child and append the rest by flattening into text.
            Ok(Some(flatten_inlines_to_text(children)))
        }
    }
}

/// Extract `r:id` + `w:anchor` from a `<w:hyperlink>` start tag.
fn hyperlink_attrs(e: &BytesStart) -> (Option<String>, Option<String>) {
    let rid = attr_val(e, b"r:id").or_else(|| attr_val(e, b"r:embed"));
    let anchor = attr_val(e, b"w:anchor");
    (rid, anchor)
}

/// Resolve a hyperlink destination: external rels target, or `#anchor`.
fn resolve_href(
    ctx: &DocxContext,
    rid: Option<&str>,
    anchor: Option<&str>,
) -> Option<String> {
    if let Some(rid) = rid {
        if let Some(rel) = ctx.rels.get(rid) {
            if rel.external {
                return Some(rel.target.clone());
            }
            // Internal target that isn't an anchor — still emit it raw.
            return Some(rel.target.clone());
        }
    }
    anchor.map(|a| format!("#{a}"))
}

/// Collapse a list of inlines into a single plain `Inline::Text` (used when a
/// hyperlink has no destination — we keep the words, drop the link wrapper).
fn flatten_inlines_to_text(inlines: Vec<Inline>) -> Inline {
    let mut text = String::new();
    collect_plain_inlines(&inlines, &mut text);
    Inline::Text {
        text,
        fmt: RunFmt::default(),
    }
}

/// Parse a `<w:drawing>` / `<w:pict>` looking for the embedded image's
/// relationship id + alt text, resolve it via rels, extract the media file,
/// and return an `Inline::Image`. Never fails the document.
fn parse_drawing<R: Read + std::io::Seek>(
    reader: &mut Reader<Cursor<&[u8]>>,
    buf: &mut Vec<u8>,
    end_tag: &[u8],
    state: &mut BodyState<R>,
) -> Result<Option<Inline>, String> {
    let mut rid: Option<String> = None;
    let mut alt: Option<String> = None;

    loop {
        match reader.read_event_into(buf) {
            Err(e) => return Err(format!("XML parse failed: {e}")),
            Ok(Event::Eof) => break,
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                let name = e.name();
                let local = local_name(name.as_ref());
                match local {
                    // <a:blip r:embed="rIdN"> (DrawingML) or VML <v:imagedata r:id="…">
                    b"blip" | b"imagedata" => {
                        if let Some(v) =
                            attr_val(&e, b"r:embed").or_else(|| attr_val(&e, b"r:id"))
                        {
                            rid = Some(v);
                        }
                    }
                    // <wp:docPr descr="…" title="…"> carries alt text.
                    b"docPr" => {
                        alt = attr_val(&e, b"descr")
                            .filter(|s| !s.trim().is_empty())
                            .or_else(|| {
                                attr_val(&e, b"title").filter(|s| !s.trim().is_empty())
                            });
                    }
                    _ => {}
                }
            }
            Ok(Event::End(e)) if e.name().as_ref() == end_tag => break,
            Ok(_) => {}
        }
        buf.clear();
    }

    let alt_text = alt.unwrap_or_else(|| "image".to_string());

    let Some(rid) = rid else {
        // A drawing without a blip (e.g. a shape) — nothing to extract.
        return Ok(None);
    };

    // Resolve the media part path from rels.
    let Some(rel) = state.ctx.rels.get(&rid) else {
        state
            .warnings
            .push(format!("Image relationship '{rid}' not found"));
        return Ok(Some(Inline::Image {
            alt: alt_text,
            path: "unavailable".to_string(),
        }));
    };

    let media_part = normalize_media_part(&rel.target);

    // Extract the image if a media dir was requested.
    if let Some((media_dir, media_rel_name)) = state.media {
        match extract_media(state.archive, &media_part, media_dir, state.media_seq) {
            Ok(file_name) => {
                state.media_seq += 1;
                let rel_path = format!("{media_rel_name}/{file_name}");
                return Ok(Some(Inline::Image {
                    alt: alt_text,
                    path: rel_path,
                }));
            }
            Err(err) => {
                state
                    .warnings
                    .push(format!("Could not extract image '{media_part}': {err}"));
                return Ok(Some(Inline::Image {
                    alt: alt_text,
                    path: "unavailable".to_string(),
                }));
            }
        }
    }

    // No extraction (plain-text mode) — reference the in-zip name.
    let name = Path::new(&media_part)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "image".to_string());
    Ok(Some(Inline::Image {
        alt: alt_text,
        path: name,
    }))
}

/// Relationship targets for media are usually `media/imageN.ext` (relative to
/// `word/`) but may be `../media/...` or absolute. Normalize to a zip path
/// rooted at `word/`.
fn normalize_media_part(target: &str) -> String {
    let t = target.trim_start_matches('/');
    if let Some(rest) = t.strip_prefix("word/") {
        return format!("word/{rest}");
    }
    if let Some(rest) = t.strip_prefix("../") {
        // "../media/image1.png" is relative to word/_rels → resolves to word/media/...
        return format!("word/{rest}");
    }
    format!("word/{t}")
}

/// Extract a media entry from the zip into `media_dir`, returning the written
/// file's name. The output name keeps the original extension and uses a
/// stable sequence prefix so collisions can't happen.
fn extract_media<R: Read + std::io::Seek>(
    archive: &mut ZipArchive<R>,
    media_part: &str,
    media_dir: &Path,
    seq: usize,
) -> Result<String, String> {
    // Read the bytes first (immutable borrow ends before we touch the FS).
    let mut bytes = Vec::new();
    {
        let mut entry = archive
            .by_name(media_part)
            .map_err(|_| format!("'{media_part}' not in archive"))?;
        entry
            .read_to_end(&mut bytes)
            .map_err(|e| format!("read failed: {e}"))?;
    }

    let ext = Path::new(media_part)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("png");
    let stem = Path::new(media_part)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("image");
    let file_name = format!("{stem}{}.{ext}", if seq == 0 { String::new() } else { format!("_{seq}") });

    fs::create_dir_all(media_dir).map_err(|e| format!("mkdir failed: {e}"))?;
    let dest = media_dir.join(&file_name);
    fs::write(&dest, &bytes).map_err(|e| format!("write failed: {e}"))?;

    Ok(file_name)
}

/// Parse a `<w:tbl>` into a `Table`. Cursor is after the start tag.
fn parse_table<R: Read + std::io::Seek>(
    reader: &mut Reader<Cursor<&[u8]>>,
    buf: &mut Vec<u8>,
    state: &mut BodyState<R>,
) -> Result<Table, String> {
    let mut rows: Vec<Vec<Vec<Block>>> = Vec::new();

    loop {
        match reader.read_event_into(buf) {
            Err(e) => return Err(format!("XML parse failed: {e}")),
            Ok(Event::Eof) => break,
            Ok(Event::Start(e)) if e.name().as_ref() == b"w:tr" => {
                let row = parse_table_row(reader, buf, state)?;
                rows.push(row);
            }
            Ok(Event::End(e)) if e.name().as_ref() == b"w:tbl" => break,
            Ok(_) => {}
        }
        buf.clear();
    }

    Ok(Table { rows })
}

fn parse_table_row<R: Read + std::io::Seek>(
    reader: &mut Reader<Cursor<&[u8]>>,
    buf: &mut Vec<u8>,
    state: &mut BodyState<R>,
) -> Result<Vec<Vec<Block>>, String> {
    let mut cells: Vec<Vec<Block>> = Vec::new();

    loop {
        match reader.read_event_into(buf) {
            Err(e) => return Err(format!("XML parse failed: {e}")),
            Ok(Event::Eof) => break,
            Ok(Event::Start(e)) if e.name().as_ref() == b"w:tc" => {
                // A cell holds block-level content (paragraphs and/or nested
                // tables) until </w:tc>.
                let cell = parse_block_sequence(reader, buf, b"w:tc", state)?;
                cells.push(cell);
            }
            Ok(Event::End(e)) if e.name().as_ref() == b"w:tr" => break,
            Ok(_) => {}
        }
        buf.clear();
    }

    Ok(cells)
}

/// Read a `w:val`-style attribute (or any named attribute) off a start tag.
fn attr_val(e: &BytesStart, key: &[u8]) -> Option<String> {
    for attr in e.attributes().flatten() {
        if attr.key.as_ref() == key {
            return std::str::from_utf8(attr.value.as_ref())
                .ok()
                .map(|s| s.to_string());
        }
    }
    None
}

/// Strip the `w:`/`a:`/`wp:` namespace prefix from a tag's qualified name.
fn local_name(name: &[u8]) -> &[u8] {
    match name.iter().position(|&b| b == b':') {
        Some(idx) => &name[idx + 1..],
        None => name,
    }
}

// ─── Markdown rendering ────────────────────────────────────────────────────

fn render_blocks(blocks: &[Block]) -> String {
    let mut out: Vec<String> = Vec::new();
    for block in blocks {
        match block {
            Block::Para(p) => {
                if let Some(rendered) = render_paragraph(p) {
                    out.push(rendered);
                }
            }
            Block::Table(t) => {
                if let Some(rendered) = render_table(t) {
                    out.push(rendered);
                }
            }
        }
    }
    // Join with blank lines, then trim leading/trailing blank lines.
    let body = out.join("\n\n");
    let trimmed = body.trim_matches('\n');
    let mut result = trimmed.to_string();
    if !result.is_empty() {
        result.push('\n');
    }
    result
}

/// Render one paragraph to a Markdown line (or `None` if it's empty).
/// List items are returned with their indentation + marker so that
/// `render_blocks` keeps them as separate blocks — GFM tolerates blank lines
/// between list items, and this keeps the renderer simple.
fn render_paragraph(p: &Paragraph) -> Option<String> {
    let inline_md = render_inlines(&p.inlines, false);

    match &p.kind {
        BlockKind::ListItem { ordered, level } => {
            // Even an "empty" list item should render as a bullet so structure
            // survives, but skip truly contentless ones to avoid noise.
            let indent = "  ".repeat(*level as usize);
            let marker = if *ordered { "1." } else { "-" };
            if inline_md.trim().is_empty() {
                return None;
            }
            Some(format!("{indent}{marker} {inline_md}"))
        }
        _ => {
            // Non-list paragraphs collapse to nothing when empty.
            if inline_md.trim().is_empty() {
                return None;
            }
            match &p.kind {
                BlockKind::Heading(level) => {
                    Some(format!("{} {}", "#".repeat(*level as usize), inline_md.trim()))
                }
                BlockKind::Quote => Some(format!("> {}", inline_md.trim())),
                _ => Some(inline_md),
            }
        }
    }
}

/// Render a list of inlines to Markdown. `in_table` escapes pipes + replaces
/// hard breaks with `<br>` so the text stays on one table row.
fn render_inlines(inlines: &[Inline], in_table: bool) -> String {
    let mut out = String::new();
    for inline in inlines {
        match inline {
            Inline::Text { text, fmt } => {
                out.push_str(&render_formatted_text(text, *fmt, in_table));
            }
            Inline::Link { children, href } => {
                let label = render_inlines(children, in_table);
                let label = if label.trim().is_empty() {
                    escape_md(href, in_table)
                } else {
                    label
                };
                out.push_str(&format!("[{label}]({})", escape_link_url(href)));
            }
            Inline::Image { alt, path } => {
                let alt = escape_md(alt, in_table);
                out.push_str(&format!("![{alt}]({})", escape_link_url(path)));
            }
            Inline::Break => {
                if in_table {
                    out.push_str("<br>");
                } else {
                    // Markdown hard line break: two trailing spaces + newline.
                    out.push_str("  \n");
                }
            }
        }
    }
    out
}

/// Apply bold/italic/strike/code/underline wrappers around escaped text.
/// Inline code is NOT markdown-escaped (it's literal), but its backticks are
/// fenced so backtick-containing code still renders.
fn render_formatted_text(text: &str, fmt: RunFmt, in_table: bool) -> String {
    if text.is_empty() {
        return String::new();
    }

    // Inline code path: don't escape, fence with enough backticks.
    if fmt.code {
        let code = if in_table {
            text.replace('|', "\\|").replace('\n', " ")
        } else {
            text.to_string()
        };
        return wrap_inline_code(&code);
    }

    // Preserve leading/trailing spaces *outside* the emphasis markers —
    // `** bold **` is invalid; `**bold**` with the spaces hoisted out is right.
    let leading_len = text.bytes().take_while(|b| *b == b' ').count();
    if leading_len == text.len() {
        // All spaces — emit as-is, no emphasis (and no overlapping slice).
        return text.to_string();
    }
    let trailing_len = text.bytes().rev().take_while(|b| *b == b' ').count();
    let leading = &text[..leading_len];
    let trailing = &text[text.len() - trailing_len..];
    let core = &text[leading_len..text.len() - trailing_len];

    let mut body = escape_md(core, in_table);

    if fmt.underline {
        body = format!("<u>{body}</u>");
    }
    if fmt.strike {
        body = format!("~~{body}~~");
    }
    // bold+italic → ***…***
    match (fmt.bold, fmt.italic) {
        (true, true) => body = format!("***{body}***"),
        (true, false) => body = format!("**{body}**"),
        (false, true) => body = format!("*{body}*"),
        (false, false) => {}
    }

    format!("{leading}{body}{trailing}")
}

/// Wrap text in inline code, choosing a backtick run longer than any inside.
fn wrap_inline_code(text: &str) -> String {
    let max_run = text
        .split(|c| c != '`')
        .map(|s| s.len())
        .max()
        .unwrap_or(0);
    let fence = "`".repeat(max_run + 1);
    // Pad with a space when the text starts/ends with a backtick.
    let needs_pad = text.starts_with('`') || text.ends_with('`');
    if needs_pad {
        format!("{fence} {text} {fence}")
    } else {
        format!("{fence}{text}{fence}")
    }
}

/// Render a table to a GFM pipe table. The first row becomes the header.
fn render_table(table: &Table) -> Option<String> {
    if table.rows.is_empty() {
        return None;
    }

    // Determine column count from the widest row.
    let cols = table.rows.iter().map(|r| r.len()).max().unwrap_or(0);
    if cols == 0 {
        return None;
    }

    let mut lines: Vec<String> = Vec::new();

    let render_row = |cells: &[Vec<Block>]| -> String {
        let mut rendered: Vec<String> = cells.iter().map(|c| render_cell(c)).collect();
        // Pad short rows so the pipe table stays rectangular.
        while rendered.len() < cols {
            rendered.push(String::new());
        }
        format!("| {} |", rendered.join(" | "))
    };

    // Header row.
    lines.push(render_row(&table.rows[0]));
    // Separator.
    lines.push(format!(
        "| {} |",
        std::iter::repeat("---")
            .take(cols)
            .collect::<Vec<_>>()
            .join(" | ")
    ));
    // Body rows.
    for row in &table.rows[1..] {
        lines.push(render_row(row));
    }

    Some(lines.join("\n"))
}

/// Render the contents of a single table cell to one pipe-safe line. Multiple
/// paragraphs join with `<br>`; a nested table flattens to `r1c1 / r1c2 / …`.
fn render_cell(blocks: &[Block]) -> String {
    let mut parts: Vec<String> = Vec::new();
    for block in blocks {
        match block {
            Block::Para(p) => {
                let line = render_inlines(&p.inlines, true);
                let line = line.trim();
                if !line.is_empty() {
                    parts.push(line.to_string());
                }
            }
            Block::Table(t) => {
                // GFM has no nested tables — flatten so content survives.
                let flat = flatten_table_inline(t);
                if !flat.is_empty() {
                    parts.push(flat);
                }
            }
        }
    }
    parts.join("<br>")
}

/// Flatten a (nested) table into a single inline string for cell embedding.
fn flatten_table_inline(table: &Table) -> String {
    let mut rows: Vec<String> = Vec::new();
    for row in &table.rows {
        let cells: Vec<String> = row.iter().map(|c| render_cell(c)).collect();
        let joined = cells.join(", ");
        if !joined.trim().is_empty() {
            rows.push(joined);
        }
    }
    rows.join(" / ")
}

// ─── Markdown escaping ─────────────────────────────────────────────────────

/// Escape characters that are markdown-significant in literal text. In a
/// table context we additionally escape `|`. Newlines are normalized to a
/// space (paragraph joining happens elsewhere).
fn escape_md(text: &str, in_table: bool) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '*' => out.push_str("\\*"),
            '_' => out.push_str("\\_"),
            '`' => out.push_str("\\`"),
            '[' => out.push_str("\\["),
            ']' => out.push_str("\\]"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '|' if in_table => out.push_str("\\|"),
            '\n' | '\r' => out.push(' '),
            other => out.push(other),
        }
    }
    out
}

/// Lightly sanitize a URL/path for use inside `(...)`. Spaces and parens are
/// percent-ish-escaped (`%20`, `\(`) so the link target parses cleanly.
fn escape_link_url(url: &str) -> String {
    let mut out = String::with_capacity(url.len());
    for ch in url.chars() {
        match ch {
            ' ' => out.push_str("%20"),
            '(' => out.push_str("%28"),
            ')' => out.push_str("%29"),
            '\n' | '\r' => {}
            other => out.push(other),
        }
    }
    out
}

// ─── Plain-text rendering (for the .txt exporter) ──────────────────────────

/// Render blocks to plain text: drop all markdown markers, keep words.
fn render_plain(blocks: &[Block]) -> String {
    let mut out: Vec<String> = Vec::new();
    for block in blocks {
        match block {
            Block::Para(p) => {
                let mut text = String::new();
                collect_plain_inlines(&p.inlines, &mut text);
                let text = text.trim();
                if !text.is_empty() {
                    out.push(text.to_string());
                }
            }
            Block::Table(t) => {
                for row in &t.rows {
                    let cells: Vec<String> = row
                        .iter()
                        .map(|c| {
                            let mut s = String::new();
                            for b in c {
                                if let Block::Para(p) = b {
                                    collect_plain_inlines(&p.inlines, &mut s);
                                    s.push(' ');
                                }
                            }
                            s.trim().to_string()
                        })
                        .filter(|s| !s.is_empty())
                        .collect();
                    if !cells.is_empty() {
                        out.push(cells.join("\t"));
                    }
                }
            }
        }
    }
    out.join("\n\n")
}

/// Append the plain words of a list of inlines (no formatting markers).
fn collect_plain_inlines(inlines: &[Inline], out: &mut String) {
    for inline in inlines {
        match inline {
            Inline::Text { text, .. } => out.push_str(text),
            Inline::Link { children, .. } => collect_plain_inlines(children, out),
            Inline::Image { alt, .. } => out.push_str(alt),
            Inline::Break => out.push(' '),
        }
    }
}

// ════════════════════════════════════════════════════════════════════════
//  Tests — build small DOCX zips in memory and assert the Markdown output.
// ════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use zip::write::SimpleFileOptions;
    use zip::ZipWriter;

    const DOC_OPEN: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"
  xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"
  xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"
  xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing"><w:body>"#;
    const DOC_CLOSE: &str = "</w:body></w:document>";

    /// Build a .docx at a unique temp path from the given parts.
    /// `parts` is a list of (zip-path, contents).
    fn write_docx(parts: &[(&str, &[u8])]) -> PathBuf {
        let mut path = std::env::temp_dir();
        let unique = format!(
            "kil_docx_test_{}_{}.docx",
            std::process::id(),
            COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        );
        path.push(unique);

        let file = fs::File::create(&path).expect("create temp docx");
        let mut zip = ZipWriter::new(file);
        let opts: SimpleFileOptions =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        for (name, data) in parts {
            zip.start_file(*name, opts).expect("start_file");
            zip.write_all(data).expect("write part");
        }
        zip.finish().expect("finish zip");
        path
    }

    static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

    /// Wrap a body fragment in a full document.xml.
    fn document(body: &str) -> String {
        format!("{DOC_OPEN}{body}{DOC_CLOSE}")
    }

    /// Convert a body-only fragment (no aux parts) and return the markdown.
    fn md_from_body(body: &str) -> String {
        let doc = document(body);
        let path = write_docx(&[("word/document.xml", doc.as_bytes())]);
        let conv = docx_to_markdown(&path, None).expect("convert");
        let _ = fs::remove_file(&path);
        conv.markdown
    }

    #[test]
    fn headings_via_style_ids() {
        let body = r#"
          <w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Title</w:t></w:r></w:p>
          <w:p><w:pPr><w:pStyle w:val="Heading3"/></w:pPr><w:r><w:t>Sub</w:t></w:r></w:p>
        "#;
        let md = md_from_body(body);
        assert!(md.contains("# Title"), "got: {md}");
        assert!(md.contains("### Sub"), "got: {md}");
    }

    #[test]
    fn headings_resolved_via_styles_xml_name() {
        // Style id "MyH" is unknown by id; styles.xml maps it to "heading 2".
        let body = r#"<w:p><w:pPr><w:pStyle w:val="MyH"/></w:pPr><w:r><w:t>Named</w:t></w:r></w:p>"#;
        let styles = r#"<?xml version="1.0"?>
        <w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
          <w:style w:type="paragraph" w:styleId="MyH"><w:name w:val="heading 2"/></w:style>
        </w:styles>"#;
        let doc = document(body);
        let path = write_docx(&[
            ("word/document.xml", doc.as_bytes()),
            ("word/styles.xml", styles.as_bytes()),
        ]);
        let conv = docx_to_markdown(&path, None).expect("convert");
        let _ = fs::remove_file(&path);
        assert!(conv.markdown.contains("## Named"), "got: {}", conv.markdown);
    }

    #[test]
    fn inline_bold_italic_combined_and_collapsed() {
        // Two adjacent bold runs must collapse to one **...** span.
        let body = r#"<w:p>
          <w:r><w:rPr><w:b/></w:rPr><w:t>Hel</w:t></w:r>
          <w:r><w:rPr><w:b/></w:rPr><w:t>lo</w:t></w:r>
          <w:r><w:t> </w:t></w:r>
          <w:r><w:rPr><w:b/><w:i/></w:rPr><w:t>both</w:t></w:r>
        </w:p>"#;
        let md = md_from_body(body);
        assert!(md.contains("**Hello**"), "collapse failed: {md}");
        assert!(!md.contains("****"), "double-marker leak: {md}");
        assert!(md.contains("***both***"), "bold+italic failed: {md}");
    }

    #[test]
    fn strike_and_inline_code() {
        let body = r#"<w:p>
          <w:r><w:rPr><w:strike/></w:rPr><w:t>gone</w:t></w:r>
          <w:r><w:t> </w:t></w:r>
          <w:r><w:rPr><w:rFonts w:ascii="Consolas"/></w:rPr><w:t>code()</w:t></w:r>
        </w:p>"#;
        let md = md_from_body(body);
        assert!(md.contains("~~gone~~"), "strike failed: {md}");
        assert!(md.contains("`code()`"), "inline code failed: {md}");
    }

    #[test]
    fn underline_emits_u_tag() {
        let body = r#"<w:p><w:r><w:rPr><w:u w:val="single"/></w:rPr><w:t>under</w:t></w:r></w:p>"#;
        let md = md_from_body(body);
        assert!(md.contains("<u>under</u>"), "underline failed: {md}");
    }

    #[test]
    fn hyperlink_external_resolves_url() {
        let body = r#"<w:p><w:hyperlink r:id="rId5"><w:r><w:t>click</w:t></w:r></w:hyperlink></w:p>"#;
        let rels = r#"<?xml version="1.0"?>
        <Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
          <Relationship Id="rId5" Type="http://x/hyperlink" Target="https://example.com" TargetMode="External"/>
        </Relationships>"#;
        let doc = document(body);
        let path = write_docx(&[
            ("word/document.xml", doc.as_bytes()),
            ("word/_rels/document.xml.rels", rels.as_bytes()),
        ]);
        let conv = docx_to_markdown(&path, None).expect("convert");
        let _ = fs::remove_file(&path);
        assert!(
            conv.markdown.contains("[click](https://example.com)"),
            "got: {}",
            conv.markdown
        );
    }

    #[test]
    fn hyperlink_anchor_internal() {
        let body =
            r#"<w:p><w:hyperlink w:anchor="sec1"><w:r><w:t>jump</w:t></w:r></w:hyperlink></w:p>"#;
        let md = md_from_body(body);
        assert!(md.contains("[jump](#sec1)"), "got: {md}");
    }

    #[test]
    fn bullet_and_nested_list() {
        // numId 1 → abstract 1 → bullet (level 0 & 1).
        let body = r#"
          <w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="1"/></w:numPr></w:pPr><w:r><w:t>One</w:t></w:r></w:p>
          <w:p><w:pPr><w:numPr><w:ilvl w:val="1"/><w:numId w:val="1"/></w:numPr></w:pPr><w:r><w:t>Nested</w:t></w:r></w:p>
        "#;
        let numbering = r#"<?xml version="1.0"?>
        <w:numbering xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
          <w:abstractNum w:abstractNumId="7">
            <w:lvl w:ilvl="0"><w:numFmt w:val="bullet"/></w:lvl>
            <w:lvl w:ilvl="1"><w:numFmt w:val="bullet"/></w:lvl>
          </w:abstractNum>
          <w:num w:numId="1"><w:abstractNumId w:val="7"/></w:num>
        </w:numbering>"#;
        let doc = document(body);
        let path = write_docx(&[
            ("word/document.xml", doc.as_bytes()),
            ("word/numbering.xml", numbering.as_bytes()),
        ]);
        let conv = docx_to_markdown(&path, None).expect("convert");
        let _ = fs::remove_file(&path);
        assert!(conv.markdown.contains("- One"), "bullet: {}", conv.markdown);
        assert!(
            conv.markdown.contains("  - Nested"),
            "nested indent: {}",
            conv.markdown
        );
    }

    #[test]
    fn ordered_list_decimal() {
        let body = r#"<w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="3"/></w:numPr></w:pPr><w:r><w:t>First</w:t></w:r></w:p>"#;
        let numbering = r#"<?xml version="1.0"?>
        <w:numbering xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
          <w:abstractNum w:abstractNumId="2">
            <w:lvl w:ilvl="0"><w:numFmt w:val="decimal"/></w:lvl>
          </w:abstractNum>
          <w:num w:numId="3"><w:abstractNumId w:val="2"/></w:num>
        </w:numbering>"#;
        let doc = document(body);
        let path = write_docx(&[
            ("word/document.xml", doc.as_bytes()),
            ("word/numbering.xml", numbering.as_bytes()),
        ]);
        let conv = docx_to_markdown(&path, None).expect("convert");
        let _ = fs::remove_file(&path);
        assert!(conv.markdown.contains("1. First"), "got: {}", conv.markdown);
    }

    #[test]
    fn list_falls_back_to_bullet_without_numbering_xml() {
        let body = r#"<w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="9"/></w:numPr></w:pPr><w:r><w:t>Item</w:t></w:r></w:p>"#;
        let md = md_from_body(body);
        assert!(md.contains("- Item"), "fallback bullet: {md}");
    }

    #[test]
    fn table_to_gfm_pipe_table() {
        let body = r#"
          <w:tbl>
            <w:tr><w:tc><w:p><w:r><w:t>H1</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>H2</w:t></w:r></w:p></w:tc></w:tr>
            <w:tr><w:tc><w:p><w:r><w:t>a</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>b</w:t></w:r></w:p></w:tc></w:tr>
          </w:tbl>
        "#;
        let md = md_from_body(body);
        assert!(md.contains("| H1 | H2 |"), "header: {md}");
        assert!(md.contains("| --- | --- |"), "separator: {md}");
        assert!(md.contains("| a | b |"), "row: {md}");
    }

    #[test]
    fn table_cell_escapes_pipe() {
        let body = r#"<w:tbl><w:tr><w:tc><w:p><w:r><w:t>a|b</w:t></w:r></w:p></w:tc></w:tr></w:tbl>"#;
        let md = md_from_body(body);
        assert!(md.contains("a\\|b"), "pipe not escaped: {md}");
    }

    #[test]
    fn quote_style_blockquote() {
        let body = r#"<w:p><w:pPr><w:pStyle w:val="Quote"/></w:pPr><w:r><w:t>wise words</w:t></w:r></w:p>"#;
        let styles = r#"<?xml version="1.0"?>
        <w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
          <w:style w:type="paragraph" w:styleId="Quote"><w:name w:val="Quote"/></w:style>
        </w:styles>"#;
        let doc = document(body);
        let path = write_docx(&[
            ("word/document.xml", doc.as_bytes()),
            ("word/styles.xml", styles.as_bytes()),
        ]);
        let conv = docx_to_markdown(&path, None).expect("convert");
        let _ = fs::remove_file(&path);
        assert!(
            conv.markdown.contains("> wise words"),
            "got: {}",
            conv.markdown
        );
    }

    #[test]
    fn hard_break_emits_two_spaces() {
        let body = r#"<w:p><w:r><w:t>line1</w:t><w:br/><w:t>line2</w:t></w:r></w:p>"#;
        let md = md_from_body(body);
        assert!(md.contains("line1  \nline2"), "hard break: {md:?}");
    }

    #[test]
    fn special_chars_escaped() {
        let body = r#"<w:p><w:r><w:t>a*b_c[d]</w:t></w:r></w:p>"#;
        let md = md_from_body(body);
        assert!(md.contains("a\\*b\\_c\\[d\\]"), "escape: {md}");
    }

    #[test]
    fn image_extracted_to_media_folder() {
        // A drawing referencing rId10 → word/media/image1.png.
        let body = r#"<w:p><w:r><w:drawing><wp:inline>
          <wp:docPr id="1" name="Pic" descr="my alt"/>
          <a:graphic><a:graphicData><pic:pic xmlns:pic="http://x">
            <pic:blipFill><a:blip r:embed="rId10"/></pic:blipFill>
          </pic:pic></a:graphicData></a:graphic>
        </wp:inline></w:drawing></w:r></w:p>"#;
        let rels = r#"<?xml version="1.0"?>
        <Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
          <Relationship Id="rId10" Type="http://x/image" Target="media/image1.png"/>
        </Relationships>"#;
        let png_bytes: &[u8] = &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 1, 2, 3];
        let doc = document(body);
        let path = write_docx(&[
            ("word/document.xml", doc.as_bytes()),
            ("word/_rels/document.xml.rels", rels.as_bytes()),
            ("word/media/image1.png", png_bytes),
        ]);

        let media_dir = std::env::temp_dir().join(format!(
            "kil_media_{}_{}",
            std::process::id(),
            COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let conv =
            docx_to_markdown(&path, Some((&media_dir, "out_media"))).expect("convert");

        // Markdown references the extracted file with alt text.
        assert!(
            conv.markdown.contains("![my alt](out_media/image1.png)"),
            "img md: {}",
            conv.markdown
        );
        // The file actually landed on disk with the original bytes.
        let extracted = media_dir.join("image1.png");
        assert!(extracted.exists(), "image not extracted");
        assert_eq!(fs::read(&extracted).unwrap(), png_bytes);

        let _ = fs::remove_file(&path);
        let _ = fs::remove_dir_all(&media_dir);
    }

    #[test]
    fn missing_image_does_not_fail_document() {
        // rId99 has no rels entry → emit ![alt](unavailable), keep going.
        let body = r#"<w:p><w:r><w:drawing><wp:inline>
          <wp:docPr id="1" name="Pic"/>
          <a:blip r:embed="rId99"/>
        </wp:inline></w:drawing></w:r></w:p>
        <w:p><w:r><w:t>after</w:t></w:r></w:p>"#;
        let md = md_from_body(body);
        assert!(md.contains("![image](unavailable)"), "fallback: {md}");
        assert!(md.contains("after"), "doc continued: {md}");
    }

    #[test]
    fn plain_text_strips_markdown() {
        let body = r#"
          <w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Title</w:t></w:r></w:p>
          <w:p><w:r><w:rPr><w:b/></w:rPr><w:t>bold</w:t></w:r><w:r><w:t> word</w:t></w:r></w:p>
        "#;
        let doc = document(body);
        let path = write_docx(&[("word/document.xml", doc.as_bytes())]);
        let text = docx_to_text(&path).expect("text");
        let _ = fs::remove_file(&path);
        assert!(text.contains("Title"), "title: {text}");
        assert!(text.contains("bold word"), "body: {text}");
        assert!(!text.contains('#'), "no markdown markers: {text}");
        assert!(!text.contains('*'), "no asterisks: {text}");
    }

    #[test]
    fn empty_body_yields_empty_markdown() {
        let md = md_from_body("");
        assert!(md.trim().is_empty(), "expected empty, got: {md:?}");
    }
}
