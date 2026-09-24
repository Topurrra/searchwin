//! Notes "Export as PDF" — styled Markdown → DOCX → PDF.
//! Wave 3.1 (2026-05-27) + Wave 4.3b (2026-05-27 polish to pandoc-grade).
//!
//! Pipeline:
//!   1. pulldown-cmark parses the user's Markdown into an event stream.
//!   2. We walk events building a `docx_rs::Docx` whose paragraphs
//!      reference NAMED STYLES (Heading1, BodyText, SourceCode, …),
//!      not direct run formatting. The full style set is registered
//!      once via `register_pandoc_styles` — mirrors pandoc's
//!      `reference.docx`: Heading1-6 with the canonical Word blue
//!      `#0F4761`, BodyText / FirstParagraph / Compact, SourceCode
//!      + VerbatimChar for code, BlockText for blockquotes, and 25+
//!      character styles for syntax highlighting tokens (KeywordTok,
//!      StringTok, FunctionTok, …) with the standard Kate / pandoc
//!      colors (`#007020` bold for keywords, `#4070a0` for strings,
//!      `#60a0b0` italic for comments, etc.).
//!   3. Code blocks get **real syntax highlighting** via `syntect`
//!      (bundled syntax definitions for ~150 languages). Each token
//!      emits a Run whose character-style reference points back to
//!      the per-token style we registered in step 2.
//!   4. The Docx is packed to in-memory bytes via docx-rs.
//!   5. docxide_pdf::convert_docx_bytes_to_pdf renders to PDF with
//!      Word-grade typography.
//!
//! The result is a "real" Word document: Heading1-6 paragraphs are
//! recognized by Word's Navigation Pane + auto-TOC + screen readers;
//! code blocks have per-token color the same way pandoc's reference
//! style produces them.
//!
//! License posture: pulldown-cmark + docx-rs + docxide-pdf + syntect
//! all MIT or Apache-2.0. No GPL surface.

use docx_rs::*;
use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use serde::{Deserialize, Serialize};
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use syntect::parsing::{ParseState, ScopeStack, SyntaxSet};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotesPdfOptions {
    pub markdown: String,
    pub output_path: String,
    #[serde(default)]
    pub title: Option<String>,
    /// Notes folder, so relative image srcs (e.g. `attachments/x.png`) in the
    /// markdown can be resolved and embedded. Optional for back-compat.
    #[serde(default)]
    pub base_dir: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotesPdfResult {
    pub output_path: String,
}

/// Export the portable Markdown form of a note. The editor has a few
/// presentation-only extensions (collapsibles and wiki links), so convert
/// those to ordinary Markdown before writing a document that will be opened
/// outside KeepItLocal.
#[tauri::command(async)]
pub fn notes_export_markdown(options: NotesPdfOptions) -> Result<NotesPdfResult, String> {
    let markdown = normalize_notes_markdown(&options.markdown);
    let title = options
        .title
        .as_deref()
        .map(str::trim)
        .filter(|title| !title.is_empty());
    let content = if body_starts_with_h1(&markdown) || title.is_none() {
        markdown
    } else {
        format!("# {}\n\n{}", title.unwrap(), markdown)
    };

    std::fs::write(&options.output_path, content)
        .map_err(|error| format!("Cannot write Markdown file: {error}"))?;

    Ok(NotesPdfResult {
        output_path: options.output_path,
    })
}
/// Inline style state tracked while walking the Markdown event stream.
#[derive(Clone, Copy, Default)]
struct InlineStyle {
    bold: bool,
    italic: bool,
    code: bool,
}

fn heading_style_id(level: u8) -> &'static str {
    match level {
        1 => "Heading1",
        2 => "Heading2",
        3 => "Heading3",
        4 => "Heading4",
        5 => "Heading5",
        _ => "Heading6",
    }
}

fn body_starts_with_h1(markdown: &str) -> bool {
    let trimmed = markdown.trim_start();
    trimmed.starts_with("# ") || trimmed.starts_with("#\t")
}

fn normalize_notes_markdown(markdown: &str) -> String {
    let normalized = markdown.replace("\r\n", "\n").replace('\r', "\n");
    let lines: Vec<&str> = normalized.split('\n').collect();
    let mut output = Vec::with_capacity(lines.len());
    let mut fence: Option<(char, usize)> = None;
    let mut index = 0;

    while index < lines.len() {
        let line = lines[index];
        if let Some(token) = markdown_fence(line) {
            if let Some(open) = fence {
                if closes_markdown_fence(open, token) {
                    fence = None;
                }
            } else {
                fence = Some(token);
            }
            output.push(line.to_owned());
            index += 1;
            continue;
        }
        if fence.is_some() {
            output.push(line.to_owned());
            index += 1;
            continue;
        }

        if let Some(title) = collapsible_title(line) {
            let mut body = Vec::new();
            let mut nested_fence: Option<(char, usize)> = None;
            let mut cursor = index + 1;
            let mut closed = false;

            while cursor < lines.len() {
                let candidate = lines[cursor];
                if let Some(token) = markdown_fence(candidate) {
                    if let Some(open) = nested_fence {
                        if closes_markdown_fence(open, token) {
                            nested_fence = None;
                        }
                    } else {
                        nested_fence = Some(token);
                    }
                    body.push(candidate);
                    cursor += 1;
                    continue;
                }
                if nested_fence.is_none() && candidate.trim() == ":::" {
                    closed = true;
                    break;
                }
                body.push(candidate);
                cursor += 1;
            }

            if closed {
                output.push(format!("## {title}"));
                output.push(String::new());
                output.extend(
                    normalize_notes_markdown(&body.join("\n"))
                        .split('\n')
                        .map(str::to_owned),
                );
                output.push(String::new());
                index = cursor + 1;
                continue;
            }
        }

        output.push(normalize_notes_line(line));
        index += 1;
    }

    output.join("\n")
}

fn markdown_fence(line: &str) -> Option<(char, usize)> {
    let trimmed = line.trim_start();
    let marker = trimmed.chars().next()?;
    if marker != char::from(96) && marker != '~' {
        return None;
    }
    let length = trimmed.chars().take_while(|character| *character == marker).count();
    (length >= 3).then_some((marker, length))
}

fn closes_markdown_fence(open: (char, usize), close: (char, usize)) -> bool {
    open.0 == close.0 && close.1 >= open.1
}

fn collapsible_title(line: &str) -> Option<String> {
    const PREFIX: &str = ":::details";
    let prefix = line.get(..PREFIX.len())?;
    if !prefix.eq_ignore_ascii_case(PREFIX) {
        return None;
    }

    let remainder = &line[PREFIX.len()..];
    if !remainder.is_empty()
        && !remainder
            .chars()
            .next()
            .map(|character| character.is_whitespace())
            .unwrap_or(false)
    {
        return None;
    }

    let title: String = remainder.trim().chars().take(120).collect();
    Some(if title.is_empty() {
        "Details".to_owned()
    } else {
        title
    })
}

fn normalize_notes_line(line: &str) -> String {
    normalize_wiki_links(&normalize_callout(line))
}

fn normalize_callout(line: &str) -> String {
    let indent_length = line
        .chars()
        .take_while(|character| character.is_whitespace())
        .map(char::len_utf8)
        .sum();
    let (indent, rest) = line.split_at(indent_length);
    let Some(after_quote) = rest.strip_prefix('>') else {
        return line.to_owned();
    };
    let candidate = after_quote.trim_start();
    let escaped_callout = candidate
        .strip_prefix(r"\[!")
        .map(|rest| format!("[!{}", rest.replacen(r"\]", "]", 1)));
    let candidate = escaped_callout.as_deref().unwrap_or(candidate);

    for (marker, label) in [
        ("[!NOTE]", "Note"),
        ("[!TIP]", "Tip"),
        ("[!WARNING]", "Attention"),
        ("[!FILE]", "Attachment"),
    ] {
        let is_marker = candidate
            .get(..marker.len())
            .map(|value| value.eq_ignore_ascii_case(marker))
            .unwrap_or(false);
        if !is_marker {
            continue;
        }

        let body = candidate[marker.len()..].trim_start();
        return if body.is_empty() {
            format!("{indent}> **{label}**")
        } else {
            format!("{indent}> **{label}** {body}")
        };
    }

    line.to_owned()
}

fn normalize_wiki_links(line: &str) -> String {
    let mut output = String::with_capacity(line.len());
    let mut index = 0;
    let mut code_ticks: Option<usize> = None;

    while index < line.len() {
        let remaining = &line[index..];
        if remaining.starts_with(char::from(96)) {
            let ticks = remaining.bytes().take_while(|byte| *byte == 96).count();
            output.push_str(&remaining[..ticks]);
            if let Some(open_ticks) = code_ticks {
                if ticks >= open_ticks {
                    code_ticks = None;
                }
            } else {
                code_ticks = Some(ticks);
            }
            index += ticks;
            continue;
        }

        if code_ticks.is_none() && remaining.starts_with("[[") {
            if let Some(end) = remaining[2..].find("]]") {
                let raw_target = &remaining[2..2 + end];
                let display = raw_target
                    .split_once('|')
                    .map(|(target, alias)| {
                        if alias.trim().is_empty() {
                            target
                        } else {
                            alias
                        }
                    })
                    .unwrap_or(raw_target)
                    .trim();
                if !display.is_empty() {
                    output.push_str(display);
                    index += end + 4;
                    continue;
                }
            }
        }

        let character = remaining.chars().next().expect("remaining text is not empty");
        output.push(character);
        index += character.len_utf8();
    }

    output
}
/// Resolve a Markdown image `src` to raw bytes: `data:` base64 URIs, `file://`
/// URLs, absolute paths, and notes-relative paths (`attachments/x.png`) against
/// the notes folder. Returns None (image silently skipped) on anything
/// unreadable — a broken image must never fail the whole export.
fn resolve_image_bytes(src: &str, base_dir: Option<&Path>) -> Option<Vec<u8>> {
    use base64::Engine as _;
    if src.starts_with("data:") {
        let b64 = src.split_once("base64,").map(|(_, rest)| rest)?;
        return base64::engine::general_purpose::STANDARD
            .decode(b64.trim())
            .ok();
    }
    let raw = src
        .strip_prefix("file://")
        .map(|s| s.trim_start_matches('/'))
        .unwrap_or(src);
    let candidate = Path::new(raw);
    let full = if candidate.is_absolute() {
        candidate.to_path_buf()
    } else {
        base_dir?.join(candidate)
    };
    std::fs::read(full).ok()
}

/// Read the editor's portable image-width metadata (`title="width=N"`).
fn image_width_from_title(title: &str) -> Option<u32> {
    let width = title.strip_prefix("width=")?.parse::<u32>().ok()?;
    (80..=10_000).contains(&width).then_some(width)
}

fn image_export_dimensions(bytes: &[u8], requested_width: Option<u32>) -> Option<(u32, u32)> {
    use image::GenericImageView;
    let (w, h) = image::load_from_memory(bytes).ok()?.dimensions();
    if w == 0 || h == 0 {
        return None;
    }
    const MAX_W_PX: u32 = 480;
    let dw = requested_width.unwrap_or(w).min(MAX_W_PX);
    let dh = ((h as u64 * dw as u64) / w as u64) as u32;
    Some((dw, dh.max(1)))
}

/// Build a block paragraph holding the image at its requested display width.
fn image_paragraph(bytes: &[u8], requested_width: Option<u32>) -> Option<Paragraph> {
    const EMU_PER_PX: u32 = 9525;
    let (dw, dh) = image_export_dimensions(bytes, requested_width)?;
    let pic = Pic::new(bytes).size(dw * EMU_PER_PX, dh.max(1) * EMU_PER_PX);
    Some(Paragraph::new().add_run(Run::new().add_image(pic)))
}

#[tauri::command]
pub async fn notes_export_styled_pdf(
    options: NotesPdfOptions,
) -> Result<NotesPdfResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let markdown = options.markdown.clone();
        let output_path = PathBuf::from(&options.output_path);
        let base_dir = options.base_dir.as_ref().map(PathBuf::from);

        let docx_bytes =
            render_markdown_to_docx_bytes(&markdown, options.title.as_deref(), base_dir.as_deref())?;
        docxide_pdf::convert_docx_bytes_to_pdf(&docx_bytes, &output_path)
            .map_err(|e| format!("PDF render failed: {e}"))?;

        Ok(NotesPdfResult {
            output_path: output_path.to_string_lossy().to_string(),
        })
    })
    .await
    .map_err(|e| format!("Notes PDF worker failed: {e}"))?
}

/// Markdown → **.docx**, reusing the exact bytes the PDF path already builds.
///
/// `notes_export_styled_pdf` renders Markdown to a `docx_rs::Docx`, packs it to
/// bytes, hands those to `docxide_pdf`, and throws the .docx away. Writing those
/// same bytes to disk is the whole feature — same heading styles, same image
/// embedding, same walker — so Word output costs one `fs::write` rather than a
/// second renderer. Added 2026-07-23 for the Markdown Converter tool.
///
/// Shares `NotesPdfOptions`/`NotesPdfResult` deliberately: the inputs are
/// identical (markdown + output path + optional title + base dir for resolving
/// relative image srcs), and a parallel struct pair would drift.
#[tauri::command]
pub async fn notes_export_docx(options: NotesPdfOptions) -> Result<NotesPdfResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let markdown = options.markdown.clone();
        let output_path = PathBuf::from(&options.output_path);
        let base_dir = options.base_dir.as_ref().map(PathBuf::from);

        let docx_bytes =
            render_markdown_to_docx_bytes(&markdown, options.title.as_deref(), base_dir.as_deref())?;
        std::fs::write(&output_path, &docx_bytes)
            .map_err(|e| format!("Cannot write DOCX: {e}"))?;

        Ok(NotesPdfResult {
            output_path: output_path.to_string_lossy().to_string(),
        })
    })
    .await
    .map_err(|e| format!("Notes DOCX worker failed: {e}"))?
}

// ─── Walker ───────────────────────────────────────────────────────────

fn render_markdown_to_docx_bytes(
    markdown: &str,
    title: Option<&str>,
    base_dir: Option<&Path>,
) -> Result<Vec<u8>, String> {
    let markdown = normalize_notes_markdown(markdown);
    let body_starts_with_h1 = body_starts_with_h1(&markdown);
    let mut docx = build_docx_skeleton();

    // Optional title — only when the body doesn't open with its own H1.
    if !body_starts_with_h1 {
        if let Some(title_str) = title.map(str::trim).filter(|s| !s.is_empty()) {
            docx = docx.add_paragraph(
                Paragraph::new()
                    .style("Heading1")
                    .add_run(Run::new().add_text(title_str)),
            );
        }
    }

    // Keep this option set in sync with the HTML export's parser
    // (notes_export_html) — when they diverge, the same note exports as two
    // different documents. Task lists and footnotes were previously off here
    // and on there, so `- [ ] todo` rendered as a checkbox in HTML and as the
    // literal text "[ ] todo" in PDF.
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_TABLES);
    opts.insert(Options::ENABLE_TASKLISTS);
    opts.insert(Options::ENABLE_FOOTNOTES);
    let parser = Parser::new_ext(&markdown, opts);

    let mut style = InlineStyle::default();
    let mut current_paragraph: Option<Paragraph> = None;
    let mut list_stack: Vec<ListContext> = Vec::new();
    let mut in_code_block = false;
    let mut code_block_lang: Option<String> = None;
    let mut code_block_buffer = String::new();
    // True between Tag::Image start/end so the image's alt text isn't emitted
    // as a stray body paragraph.
    let mut in_image = false;
    // Destination of the link currently being walked, so link TEXT can be
    // wrapped in a real hyperlink instead of losing its target.
    let mut link_dest: Option<String> = None;
    // Table accumulation. Tables were previously parsed (ENABLE_TABLES was on)
    // but had no handler at all, so every cell's text fell through the
    // Event::Text arm — which only appends when a paragraph is open — and was
    // SILENTLY DISCARDED. A note's table vanished from the exported PDF.
    let mut table_rows: Vec<TableRow> = Vec::new();
    let mut table_cells: Vec<TableCell> = Vec::new();

    for event in parser {
        match event {
            Event::Start(tag) => match tag {
                Tag::Heading { level, .. } => {
                    let n = heading_level_to_u8(level);
                    current_paragraph = Some(Paragraph::new().style(heading_style_id(n)));
                }
                Tag::Paragraph => {
                    current_paragraph = Some(Paragraph::new().style("BodyText"));
                }
                Tag::Strong => style.bold = true,
                Tag::Emphasis => style.italic = true,
                Tag::Link { dest_url, .. } => {
                    link_dest = Some(dest_url.into_string());
                }
                Tag::Table(_) => {
                    // Flush anything open so the table starts as its own block.
                    if let Some(p) = current_paragraph.take() {
                        docx = docx.add_paragraph(p);
                    }
                    table_rows.clear();
                    table_cells.clear();
                }
                Tag::TableHead | Tag::TableRow => {
                    table_cells.clear();
                }
                Tag::TableCell => {
                    // Open a paragraph so the cell's Event::Text has somewhere
                    // to land — this is the line whose absence lost the data.
                    current_paragraph = Some(Paragraph::new().style("BodyText"));
                }
                Tag::CodeBlock(kind) => {
                    in_code_block = true;
                    code_block_buffer.clear();
                    code_block_lang = match kind {
                        CodeBlockKind::Fenced(info) => {
                            let info = info.into_string();
                            let lang = info
                                .split_whitespace()
                                .next()
                                .map(str::to_string)
                                .filter(|s| !s.is_empty());
                            lang
                        }
                        CodeBlockKind::Indented => None,
                    };
                }
                Tag::List(start) => {
                    list_stack.push(ListContext {
                        ordered: start.is_some(),
                        depth: list_stack.len(),
                    });
                }
                Tag::Item => {
                    if let Some(ctx) = list_stack.last() {
                        current_paragraph = Some(
                            Paragraph::new()
                                .style("Compact")
                                .numbering(
                                    NumberingId::new(if ctx.ordered { 2 } else { 1 }),
                                    IndentLevel::new(ctx.depth.min(8)),
                                ),
                        );
                    }
                }
                Tag::BlockQuote(_) => {
                    current_paragraph = Some(Paragraph::new().style("BlockText"));
                }
                Tag::Image { dest_url, title, .. } => {
                    // Embed the image as its own block. Flush any open paragraph
                    // first; the following alt-text Text event is suppressed.
                    if let Some(p) = current_paragraph.take() {
                        docx = docx.add_paragraph(p);
                    }
                    in_image = true;
                    if let Some(bytes) = resolve_image_bytes(&dest_url, base_dir) {
                        if let Some(p) = image_paragraph(&bytes, image_width_from_title(&title)) {
                            docx = docx.add_paragraph(p);
                        }
                    }
                }
                _ => {}
            },
            Event::End(tag) => match tag {
                TagEnd::Heading(_) => {
                    if let Some(p) = current_paragraph.take() {
                        docx = docx.add_paragraph(p);
                    }
                }
                TagEnd::Paragraph | TagEnd::Item => {
                    if let Some(p) = current_paragraph.take() {
                        docx = docx.add_paragraph(p);
                    }
                }
                TagEnd::Strong => style.bold = false,
                TagEnd::Emphasis => style.italic = false,
                TagEnd::CodeBlock => {
                    in_code_block = false;
                    // Emit a syntax-highlighted SourceCode paragraph per
                    // line. Each line becomes its own paragraph so it
                    // renders on its own row in the PDF; tokens within
                    // the line are emitted as runs with per-token
                    // character styles.
                    let code = std::mem::take(&mut code_block_buffer);
                    let lang_hint = code_block_lang.take();
                    for paragraph in highlight_code_block_paragraphs(&code, lang_hint.as_deref()) {
                        docx = docx.add_paragraph(paragraph);
                    }
                }
                TagEnd::List(_) => {
                    list_stack.pop();
                }
                TagEnd::BlockQuote(_) => {
                }
                TagEnd::Image => {
                    in_image = false;
                }
                TagEnd::Link => {
                    link_dest = None;
                }
                TagEnd::TableCell => {
                    let cell = TableCell::new()
                        .add_paragraph(current_paragraph.take().unwrap_or_else(Paragraph::new));
                    table_cells.push(cell);
                }
                TagEnd::TableHead | TagEnd::TableRow => {
                    if !table_cells.is_empty() {
                        table_rows.push(TableRow::new(std::mem::take(&mut table_cells)));
                    }
                }
                TagEnd::Table => {
                    // A trailing partial row shouldn't be dropped.
                    if !table_cells.is_empty() {
                        table_rows.push(TableRow::new(std::mem::take(&mut table_cells)));
                    }
                    if !table_rows.is_empty() {
                        let columns = table_rows.iter().map(|row| row.cells.len()).max().unwrap_or(1);
                        let column_width = (9_360 / columns).max(1);
                        docx = docx.add_table(Table::new(std::mem::take(&mut table_rows)).set_grid(vec![column_width; columns]));
                    }
                }
                _ => {}
            },
            Event::Text(s) => {
                if in_image {
                    // Image alt text — not rendered as a body paragraph.
                } else if in_code_block {
                    code_block_buffer.push_str(&s);
                } else if let Some(dest) = link_dest.clone() {
                    // Wrap link text in a real hyperlink so the target survives
                    // the export; previously only the text came through and the
                    // href was dropped entirely.
                    let link = Hyperlink::new(dest, HyperlinkType::External)
                        .add_run(styled_inline_run(&s, style).style("Hyperlink"));
                    let p = current_paragraph
                        .take()
                        .unwrap_or_else(|| Paragraph::new().style("BodyText"));
                    current_paragraph = Some(p.add_hyperlink(link));
                } else if let Some(p) = current_paragraph.take() {
                    current_paragraph = Some(p.add_run(styled_inline_run(&s, style)));
                } else if !s.trim().is_empty() {
                    // SAFETY NET: text arriving with no open paragraph used to
                    // be dropped on the floor (that is how every table vanished).
                    // Anything unhandled now becomes its own paragraph — ugly
                    // beats invisible, and it makes the next such gap visible
                    // instead of silent.
                    docx = docx.add_paragraph(
                        Paragraph::new()
                            .style("BodyText")
                            .add_run(styled_inline_run(&s, style)),
                    );
                }
            }
            Event::Code(s) => {
                // Inline code — VerbatimChar character style.
                if let Some(p) = current_paragraph.take() {
                    current_paragraph =
                        Some(p.add_run(Run::new().add_text(s.into_string()).style("VerbatimChar")));
                }
            }
            Event::TaskListMarker(checked) => {
                // Now that ENABLE_TASKLISTS is on, render a real checkbox glyph
                // instead of the literal "[ ]" text the parser used to emit.
                if let Some(p) = current_paragraph.take() {
                    let glyph = if checked { "\u{2611} " } else { "\u{2610} " };
                    current_paragraph = Some(p.add_run(styled_inline_run(glyph, style)));
                }
            }
            Event::SoftBreak | Event::HardBreak => {
                if let Some(p) = current_paragraph.take() {
                    current_paragraph = Some(p.add_run(Run::new().add_break(BreakType::TextWrapping)));
                }
            }
            Event::Rule => {
                docx = docx.add_paragraph(
                    Paragraph::new()
                        .style("BodyText")
                        .add_run(Run::new().add_text("\u{2014}".repeat(40)).color("AAAAAA")),
                );
            }
            _ => {}
        }
    }

    if let Some(p) = current_paragraph.take() {
        docx = docx.add_paragraph(p);
    }

    let mut cursor = Cursor::new(Vec::<u8>::with_capacity(8 * 1024));
    docx.build()
        .pack(&mut cursor)
        .map_err(|e| format!("DOCX pack failed: {e}"))?;
    Ok(cursor.into_inner())
}

/// Build a `Run` for ordinary inline text with the current bold /
/// italic / inline-code flags applied. Block-level sizing comes from
/// the paragraph's named style; runs just carry inline formatting
/// deltas + the optional VerbatimChar character-style ref.
fn styled_inline_run(text: &str, style: InlineStyle) -> Run {
    let mut run = Run::new().add_text(text);
    if style.code {
        run = run.style("VerbatimChar");
    }
    if style.bold {
        run = run.bold();
    }
    if style.italic {
        run = run.italic();
    }
    run
}

#[derive(Clone, Copy)]
struct ListContext {
    ordered: bool,
    depth: usize,
}

fn heading_level_to_u8(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

// ─── Syntax highlighting ──────────────────────────────────────────────

/// One-time-loaded bundled syntax definitions. ~150 languages out of
/// the box (Rust, JS, Python, Go, C/C++, Java, …). Load cost is
/// non-trivial (~30 ms first call), so we cache it for the process
/// lifetime — every code-block export reuses the same SyntaxSet.
fn syntax_set() -> &'static SyntaxSet {
    static SET: OnceLock<SyntaxSet> = OnceLock::new();
    SET.get_or_init(SyntaxSet::load_defaults_newlines)
}

/// Build a list of `Paragraph`s for a code block. Each line in the
/// source becomes its own SourceCode-styled paragraph; within each
/// line, syntect's scope-stack walk feeds us tokens, and we map each
/// token's innermost scope to a pandoc token style (KeywordTok,
/// StringTok, etc.). The runs reference those character styles via
/// `.style(...)`, so the rendered DOCX (and PDF) shows per-token
/// colors matching pandoc's `reference.docx` palette.
fn highlight_code_block_paragraphs(code: &str, lang_hint: Option<&str>) -> Vec<Paragraph> {
    let ss = syntax_set();
    // Find the syntax by language name. Fallbacks: by extension, then
    // plain-text (no highlighting at all — every token renders as
    // NormalTok / default). The fence info string from pulldown-cmark
    // is usually a language name ("rust") or sometimes an extension
    // ("rs"); try both so common shorthand still highlights.
    let syntax = lang_hint
        .and_then(|name| {
            ss.find_syntax_by_token(name)
                .or_else(|| ss.find_syntax_by_extension(name))
                .or_else(|| ss.find_syntax_by_name(name))
        })
        .unwrap_or_else(|| ss.find_syntax_plain_text());

    let mut parse_state = ParseState::new(syntax);
    let mut scope_stack = ScopeStack::new();

    let mut paragraphs: Vec<Paragraph> = Vec::new();
    // `code.lines()` drops trailing empty strings; that's fine — we
    // also strip a sole trailing newline (parser's quirk). If the
    // user explicitly wanted a blank tail line, they can add one.
    let trimmed = code.trim_end_matches('\n');
    for line in trimmed.split('\n') {
        // Pass the line WITH its newline to syntect so the parser
        // closes scopes properly at end of line.
        let with_newline = format!("{}\n", line);
        let ops = match parse_state.parse_line(&with_newline, ss) {
            Ok(o) => o,
            Err(_) => Vec::new(),
        };

        let mut paragraph = Paragraph::new().style("SourceCode");
        let emit_token = |stack: &ScopeStack, text: &str| {
            if text.is_empty() {
                return None;
            }
            let style_id = scope_to_pandoc_token(stack);
            Some(
                Run::new()
                    .add_text(text)
                    .style(style_id),
            )
        };

        // Walk ops alongside the line bytes, slicing the line into
        // tokens defined by where the scope-stack changes.
        let bytes = with_newline.as_bytes();
        let mut last_byte = 0usize;
        for (byte_pos, op) in ops {
            if byte_pos > last_byte {
                let slice = &with_newline[last_byte..byte_pos];
                // Strip the trailing newline from any final slice — we
                // don't want a literal '\n' in the run (the new
                // paragraph IS the line break).
                let cleaned = slice.trim_end_matches('\n');
                if !cleaned.is_empty() {
                    if let Some(run) = emit_token(&scope_stack, cleaned) {
                        paragraph = paragraph.add_run(run);
                    }
                }
            }
            scope_stack.apply(&op).ok();
            last_byte = byte_pos;
        }
        if last_byte < bytes.len() {
            let slice = &with_newline[last_byte..];
            let cleaned = slice.trim_end_matches('\n');
            if !cleaned.is_empty() {
                if let Some(run) = emit_token(&scope_stack, cleaned) {
                    paragraph = paragraph.add_run(run);
                }
            }
        }
        paragraphs.push(paragraph);
    }
    paragraphs
}

/// Map a syntect ScopeStack to the pandoc-equivalent character style id
/// (KeywordTok / StringTok / CommentTok / …). Walks the stack top-down
/// so the innermost (most-specific) scope wins. Scope prefixes follow
/// the TextMate / Sublime convention, which is what syntect emits.
fn scope_to_pandoc_token(stack: &ScopeStack) -> &'static str {
    // We need the scope as a string to do prefix matching. Format
    // each scope just once and check.
    for scope in stack.as_slice().iter().rev() {
        let s = format!("{}", scope);
        // Order matters — most-specific first so e.g. "keyword.control"
        // hits ControlFlowTok before falling into "keyword" → KeywordTok.
        if s.starts_with("comment.documentation") {
            return "DocumentationTok";
        }
        if s.starts_with("comment") {
            return "CommentTok";
        }
        if s.starts_with("keyword.control") {
            return "ControlFlowTok";
        }
        if s.starts_with("keyword.operator") {
            return "OperatorTok";
        }
        if s.starts_with("keyword") {
            return "KeywordTok";
        }
        if s.starts_with("storage.type") || s.starts_with("entity.name.type") {
            return "DataTypeTok";
        }
        if s.starts_with("storage.modifier") {
            return "KeywordTok";
        }
        if s.starts_with("string.quoted.single") {
            return "CharTok";
        }
        if s.starts_with("string") {
            return "StringTok";
        }
        if s.starts_with("constant.numeric") {
            return "DecValTok";
        }
        if s.starts_with("constant.character") {
            return "CharTok";
        }
        if s.starts_with("constant.language") {
            return "ConstantTok";
        }
        if s.starts_with("constant") {
            return "ConstantTok";
        }
        if s.starts_with("entity.name.function") {
            return "FunctionTok";
        }
        if s.starts_with("entity.name.tag") {
            return "KeywordTok";
        }
        if s.starts_with("entity.other.attribute-name") {
            return "AttributeTok";
        }
        if s.starts_with("support.function") {
            return "BuiltInTok";
        }
        if s.starts_with("support.constant") {
            return "ConstantTok";
        }
        if s.starts_with("support.type") {
            return "DataTypeTok";
        }
        if s.starts_with("support.class") {
            return "DataTypeTok";
        }
        if s.starts_with("variable.parameter") {
            return "VariableTok";
        }
        if s.starts_with("variable.function") {
            return "FunctionTok";
        }
        if s.starts_with("variable") {
            return "VariableTok";
        }
        if s.starts_with("meta.preprocessor") {
            return "PreprocessorTok";
        }
        if s.starts_with("invalid.illegal") {
            return "ErrorTok";
        }
        if s.starts_with("invalid") {
            return "AlertTok";
        }
        if s.starts_with("punctuation.definition.string") {
            return "StringTok";
        }
    }
    "NormalTok"
}

// ─── Document skeleton + style registry ──────────────────────────────

fn build_docx_skeleton() -> Docx {
    let docx = Docx::new()
        // Bullet list: abstract id 1, instance id 1.
        .add_abstract_numbering(make_bullet_abstract(1))
        .add_numbering(Numbering::new(1, 1))
        // Decimal list: abstract id 2, instance id 2.
        .add_abstract_numbering(make_decimal_abstract(2))
        .add_numbering(Numbering::new(2, 2));
    register_pandoc_styles(docx)
}

/// Register the full pandoc-equivalent style set on the Docx. Style
/// IDs + values mirror pandoc's `reference.docx` so the rendered
/// output looks like a pandoc-generated document. Word's Navigation
/// Pane and auto-TOC recognize Heading1-6 because each carries the
/// `outlineLvl` property (via `outline_lvl(N)`).
fn register_pandoc_styles(mut docx: Docx) -> Docx {
    // ── Headings (paragraph) ─────────────────────────────────────
    // Sizes are half-points: Heading1 = 40 → 20pt, Heading2 = 32 → 16pt, etc.
    // Color #0F4761 is Word's default accent1@shade-BF (the canonical Heading dark blue).
    let heading_color = "0F4761";
    let heading_font = || RunFonts::new().ascii("Cambria").hi_ansi("Cambria");

    docx = docx.add_style(
        Style::new("Heading1", StyleType::Paragraph)
            .name("heading 1")
            .based_on("Normal")
            .next("BodyText")
            .q_format(true)
            .size(40)
            .color(heading_color)
            .fonts(heading_font())
            .outline_lvl(0),
    );
    docx = docx.add_style(
        Style::new("Heading2", StyleType::Paragraph)
            .name("heading 2")
            .based_on("Normal")
            .next("BodyText")
            .q_format(true)
            .size(32)
            .color(heading_color)
            .fonts(heading_font())
            .outline_lvl(1),
    );
    docx = docx.add_style(
        Style::new("Heading3", StyleType::Paragraph)
            .name("heading 3")
            .based_on("Normal")
            .next("BodyText")
            .q_format(true)
            .size(28)
            .color(heading_color)
            .fonts(heading_font())
            .outline_lvl(2),
    );
    docx = docx.add_style(
        Style::new("Heading4", StyleType::Paragraph)
            .name("heading 4")
            .based_on("Normal")
            .next("BodyText")
            .q_format(true)
            .size(24)
            .color(heading_color)
            .fonts(heading_font())
            .italic()
            .outline_lvl(3),
    );
    docx = docx.add_style(
        Style::new("Heading5", StyleType::Paragraph)
            .name("heading 5")
            .based_on("Normal")
            .next("BodyText")
            .q_format(true)
            .size(22)
            .color(heading_color)
            .fonts(heading_font())
            .outline_lvl(4),
    );
    docx = docx.add_style(
        Style::new("Heading6", StyleType::Paragraph)
            .name("heading 6")
            .based_on("Normal")
            .next("BodyText")
            .q_format(true)
            .size(22)
            .color(heading_color)
            .fonts(heading_font())
            .italic()
            .outline_lvl(5),
    );

    // ── BodyText + variants (paragraph) ──────────────────────────
    docx = docx.add_style(
        Style::new("BodyText", StyleType::Paragraph)
            .name("Body Text")
            .based_on("Normal")
            .size(22),
    );
    docx = docx.add_style(
        Style::new("FirstParagraph", StyleType::Paragraph)
            .name("First Paragraph")
            .based_on("BodyText")
            .next("BodyText")
            .size(22),
    );
    docx = docx.add_style(
        Style::new("Compact", StyleType::Paragraph)
            .name("Compact")
            .based_on("BodyText")
            .size(22),
    );
    docx = docx.add_style(
        Style::new("BlockText", StyleType::Paragraph)
            .name("Block Text")
            .based_on("BodyText")
            .next("BodyText")
            .italic()
            .indent(Some(720), None, None, None),
    );

    // ── SourceCode + VerbatimChar (code block container + base run) ──
    docx = docx.add_style(
        Style::new("SourceCode", StyleType::Paragraph)
            .name("Source Code")
            .based_on("Normal")
            .size(20)
            .fonts(RunFonts::new().ascii("Consolas").hi_ansi("Consolas")),
    );
    docx = docx.add_style(
        Style::new("VerbatimChar", StyleType::Character)
            .name("Verbatim Char")
            .size(22)
            .fonts(RunFonts::new().ascii("Consolas").hi_ansi("Consolas")),
    );

    // ── Syntax-highlighting token character styles ───────────────
    // Colors + bold/italic flags mirror pandoc's `reference.docx`
    // exactly (the standard Kate / syntax-highlighting library
    // palette). Every style is based_on VerbatimChar so they inherit
    // the Consolas font + size; the token style only adds color
    // (and bold/italic where called for).
    let tok = |id: &'static str, color: &'static str, bold: bool, italic: bool| {
        let mut s = Style::new(id, StyleType::Character)
            .name(id)
            .based_on("VerbatimChar")
            .color(color);
        if bold {
            s = s.bold();
        }
        if italic {
            s = s.italic();
        }
        s
    };

    docx = docx.add_style(tok("KeywordTok", "007020", true, false));
    docx = docx.add_style(tok("DataTypeTok", "902000", false, false));
    docx = docx.add_style(tok("DecValTok", "40A070", false, false));
    docx = docx.add_style(tok("BaseNTok", "40A070", false, false));
    docx = docx.add_style(tok("FloatTok", "40A070", false, false));
    docx = docx.add_style(tok("ConstantTok", "880000", false, false));
    docx = docx.add_style(tok("CharTok", "4070A0", false, false));
    docx = docx.add_style(tok("SpecialCharTok", "4070A0", false, false));
    docx = docx.add_style(tok("StringTok", "4070A0", false, false));
    docx = docx.add_style(tok("VerbatimStringTok", "4070A0", false, false));
    docx = docx.add_style(tok("SpecialStringTok", "BB6688", false, false));
    docx = docx.add_style(tok("ImportTok", "008000", true, false));
    docx = docx.add_style(tok("CommentTok", "60A0B0", false, true));
    docx = docx.add_style(tok("DocumentationTok", "BA2121", false, true));
    docx = docx.add_style(tok("AnnotationTok", "60A0B0", true, true));
    docx = docx.add_style(tok("CommentVarTok", "60A0B0", true, true));
    docx = docx.add_style(tok("OtherTok", "007020", false, false));
    docx = docx.add_style(tok("FunctionTok", "06287E", false, false));
    docx = docx.add_style(tok("VariableTok", "19177C", false, false));
    docx = docx.add_style(tok("ControlFlowTok", "007020", true, false));
    docx = docx.add_style(tok("OperatorTok", "666666", false, false));
    docx = docx.add_style(tok("BuiltInTok", "008000", false, false));
    docx = docx.add_style(tok("PreprocessorTok", "BC7A00", false, false));
    docx = docx.add_style(tok("AttributeTok", "7D9029", false, false));
    docx = docx.add_style(tok("AlertTok", "FF0000", true, false));
    docx = docx.add_style(tok("ErrorTok", "FF0000", true, false));
    docx = docx.add_style(
        Style::new("NormalTok", StyleType::Character)
            .name("NormalTok")
            .based_on("VerbatimChar"),
    );

    docx
}

fn make_bullet_abstract(id: usize) -> AbstractNumbering {
    let mut abs = AbstractNumbering::new(id);
    let bullets = ["\u{2022}", "\u{25E6}", "\u{25AA}"]; // • ◦ ▪
    for level in 0..9usize {
        let glyph = bullets[level % bullets.len()];
        abs = abs.add_level(
            Level::new(
                level,
                Start::new(1),
                NumberFormat::new("bullet"),
                LevelText::new(glyph),
                LevelJc::new("left"),
            )
            .indent(
                Some(((level + 1) * 360) as i32),
                Some(SpecialIndentType::Hanging(360)),
                None,
                None,
            ),
        );
    }
    abs
}

fn make_decimal_abstract(id: usize) -> AbstractNumbering {
    let mut abs = AbstractNumbering::new(id);
    for level in 0..9usize {
        let level_text = format!("%{}.", level + 1);
        abs = abs.add_level(
            Level::new(
                level,
                Start::new(1),
                NumberFormat::new("decimal"),
                LevelText::new(level_text),
                LevelJc::new("left"),
            )
            .indent(
                Some(((level + 1) * 360) as i32),
                Some(SpecialIndentType::Hanging(360)),
                None,
                None,
            ),
        );
    }
    abs
}

// ─── HTML export ─────────────────────────────────────────────────────────
//
// Renders a note to a SELF-CONTAINED .html file: images are inlined as
// base64 data URIs, styles are embedded, so the file opens correctly in any
// browser on any machine with no sidecar folder.
//
// Lives in Rust rather than the frontend (which already has `marked`) for one
// reason: inlining attachments means reading arbitrary files off disk. Doing
// that from the webview would need a broad `fs:allow-read-file` capability;
// here it stays behind the same notes-folder chokepoint the PDF export
// already uses, and reuses `resolve_image_bytes` verbatim.

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotesHtmlOptions {
    pub markdown: String,
    pub output_path: String,
    #[serde(default)]
    pub title: Option<String>,
    /// Notes folder, so relative image srcs (`attachments/x.png`) resolve.
    #[serde(default)]
    pub base_dir: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotesHtmlResult {
    pub output_path: String,
}

/// Minimal, theme-neutral document styling. Deliberately NOT the app's design
/// tokens: this file is read outside KeepItLocal, where those variables don't
/// exist. Honours the reader's dark-mode preference instead.
const HTML_STYLE: &str = r#"
:root { color-scheme: light dark; }
* { box-sizing: border-box; }
body {
  max-width: 46rem; margin: 3rem auto; padding: 0 1.25rem;
  font: 16px/1.65 -apple-system, "Segoe UI", Roboto, Helvetica, Arial, sans-serif;
  color: #1c1e21; background: #fff;
  -webkit-font-smoothing: antialiased;
}
h1, h2, h3, h4 { line-height: 1.25; margin: 2rem 0 .75rem; font-weight: 650; }
h1 { font-size: 1.9rem; margin-top: 0; }
h2 { font-size: 1.45rem; }
h3 { font-size: 1.2rem; }
p, ul, ol, blockquote, pre, table { margin: 0 0 1rem; }
a { color: #0b62d6; }
code {
  font: .9em/1.5 "JetBrains Mono", ui-monospace, SFMono-Regular, Consolas, monospace;
  background: rgba(127,127,127,.14); padding: .15em .4em; border-radius: 4px;
}
pre { background: rgba(127,127,127,.12); padding: 1rem; border-radius: 8px; overflow-x: auto; }
pre code { background: none; padding: 0; }
blockquote {
  margin-left: 0; padding: .25rem 0 .25rem 1rem;
  border-left: 3px solid rgba(127,127,127,.4); color: #555;
}
img { max-width: 100%; height: auto; border-radius: 6px; }
table { border-collapse: collapse; width: 100%; }
th, td { border: 1px solid rgba(127,127,127,.35); padding: .5rem .65rem; text-align: left; }
hr { border: 0; border-top: 1px solid rgba(127,127,127,.3); margin: 2rem 0; }
ul li::marker { color: rgba(127,127,127,.8); }
@media (prefers-color-scheme: dark) {
  body { color: #e6e6e6; background: #17181a; }
  a { color: #6aa8ff; }
  blockquote { color: #aaa; }
}
"#;

fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Guess an image MIME from magic bytes so the data URI is correct regardless
/// of the file extension (a .png that's really a jpeg still renders).
fn image_mime(bytes: &[u8]) -> &'static str {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        "image/png"
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        "image/jpeg"
    } else if bytes.starts_with(b"GIF8") {
        "image/gif"
    } else if bytes.len() > 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        "image/webp"
    } else if bytes.starts_with(b"BM") {
        "image/bmp"
    } else {
        "application/octet-stream"
    }
}
fn apply_image_widths(html: &str) -> String {
    let mut output = String::with_capacity(html.len());
    let mut remaining = html;

    while let Some(start) = remaining.find("<img ") {
        output.push_str(&remaining[..start]);
        let tag_and_rest = &remaining[start..];
        let Some(end) = tag_and_rest.find('>') else {
            output.push_str(tag_and_rest);
            return output;
        };
        let tag = &tag_and_rest[..=end];

        if let Some(width) = html_image_width(tag) {
            let insert_at = if tag.ends_with("/>") {
                tag.len() - 2
            } else {
                tag.len() - 1
            };
            output.push_str(&tag[..insert_at]);
            output.push_str(&format!(
                " style=\"width: {width}px; max-width: 100%; height: auto;\""
            ));
            output.push_str(&tag[insert_at..]);
        } else {
            output.push_str(tag);
        }

        remaining = &tag_and_rest[end + 1..];
    }

    output.push_str(remaining);
    output
}

fn html_image_width(tag: &str) -> Option<u32> {
    const TITLE_PREFIX: &str = r#"title=""#;
    let start = tag.find(TITLE_PREFIX)? + TITLE_PREFIX.len();
    let end = start + tag[start..].find('"')?;
    image_width_from_title(&tag[start..end])
}

/// Export a note's Markdown to a self-contained HTML file.
#[tauri::command(async)]
pub fn notes_export_html(options: NotesHtmlOptions) -> Result<NotesHtmlResult, String> {
    use base64::Engine as _;

    let base_dir = options.base_dir.as_deref().map(Path::new);

    let markdown = normalize_notes_markdown(&options.markdown);
    let mut md_opts = Options::empty();
    md_opts.insert(Options::ENABLE_TABLES);
    md_opts.insert(Options::ENABLE_STRIKETHROUGH);
    md_opts.insert(Options::ENABLE_TASKLISTS);
    md_opts.insert(Options::ENABLE_FOOTNOTES);

    // Rewrite each image's dest_url to an inlined data: URI as the event
    // stream is walked, so push_html emits a self-contained document. An
    // unreadable image keeps its original src rather than vanishing — a
    // broken-image icon is more honest than a silently missing figure.
    //
    // The same pass NEUTRALISES RAW HTML. `push_html` emits Event::Html and
    // Event::InlineHtml verbatim, so a note containing `<script>` or
    // `<img onerror=…>` would carry it into a standalone .html file that gets
    // opened in a REAL browser — where, unlike the app's webview, it actually
    // runs. Notes are routinely pasted in from web pages, and the in-app
    // preview already sanitises for exactly this reason (see
    // src/lib/notes/preview.ts's tag/attribute allowlist). Rather than port
    // that allowlist into a second language and let the two drift, raw HTML is
    // escaped to visible TEXT here: the author still sees what they wrote, and
    // no markup can execute. Markdown-authored constructs are unaffected —
    // they arrive as typed events, not as Html.
    let events: Vec<Event> = Parser::new_ext(&markdown, md_opts)
        .map(|event| match event {
            Event::SoftBreak => Event::HardBreak,
            Event::Html(raw) => Event::Text(raw),
            Event::InlineHtml(raw) => Event::Text(raw),
            Event::Start(Tag::Image {
                link_type,
                dest_url,
                title,
                id,
            }) => {
                let inlined = resolve_image_bytes(&dest_url, base_dir)
                    .map(|bytes| {
                        let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
                        format!("data:{};base64,{}", image_mime(&bytes), b64)
                    })
                    .unwrap_or_else(|| dest_url.to_string());
                Event::Start(Tag::Image {
                    link_type,
                    dest_url: inlined.into(),
                    title,
                    id,
                })
            }
            other => other,
        })
        .collect();

    let mut body = String::new();
    pulldown_cmark::html::push_html(&mut body, events.into_iter());

    let body = apply_image_widths(&body);
    let title = options
        .title
        .as_deref()
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .unwrap_or("Untitled note");
    // Only add an <h1> if the body doesn't already open with one, so a note
    // whose first line is "# Title" doesn't render the title twice.
    let heading = if body.trim_start().starts_with("<h1") {
        String::new()
    } else {
        format!("<h1>{}</h1>\n", escape_html(title))
    };

    let doc = format!(
        "<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n\
         <title>{}</title>\n<style>{}</style>\n</head>\n<body>\n{}{}</body>\n</html>\n",
        escape_html(title),
        HTML_STYLE,
        heading,
        body
    );

    std::fs::write(&options.output_path, doc)
        .map_err(|e| format!("Cannot write HTML file: {e}"))?;

    Ok(NotesHtmlResult {
        output_path: options.output_path,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Render a note to DOCX bytes and return them, or panic with the error.
    fn docx_of(md: &str) -> Vec<u8> {
        render_markdown_to_docx_bytes(md, None, None).expect("docx render should succeed")
    }

    /// A DOCX is a zip; the document body XML is where our content lands.
    /// Reading it back is the only way to prove content SURVIVED the walk —
    /// which is the whole point, since the bug was silent loss.
    fn document_xml(bytes: &[u8]) -> String {
        let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).expect("docx should be a zip");
        let mut file = zip
            .by_name("word/document.xml")
            .expect("docx should contain word/document.xml");
        let mut out = String::new();
        std::io::Read::read_to_string(&mut file, &mut out).expect("document.xml should be utf-8");
        out
    }
    #[test]
    fn rich_notes_extensions_export_as_readable_documents() {
        let rich_markdown = r#"> [!TIP] Keep this handy

:::details Release checklist
- [ ] Export the note
:::

Read [[Product Roadmap|the roadmap]].

first line
second line

~~~text
:::details literal
[[Literal]]
~~~
"#;
        let normalized = normalize_notes_markdown(rich_markdown);
        assert!(normalized.contains("> **Tip** Keep this handy"));
        assert!(normalized.contains("## Release checklist"));
        assert!(normalized.contains("- [ ] Export the note"));
        assert!(normalized.contains("Read the roadmap."));
        assert!(!normalized.contains(":::details Release checklist"));
        assert!(normalized.contains(":::details literal\n[[Literal]]"));

        let dir = std::env::temp_dir().join(format!(
            "kil-rich-export-test-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();

        let markdown_out = dir.join("note.md");
        notes_export_markdown(NotesPdfOptions {
            markdown: rich_markdown.to_owned(),
            output_path: markdown_out.to_string_lossy().to_string(),
            title: Some("Export test".into()),
            base_dir: None,
        })
        .expect("markdown export should succeed");
        let markdown = std::fs::read_to_string(&markdown_out).unwrap();
        assert!(markdown.starts_with("# Export test\n\n"));
        assert!(markdown.contains("## Release checklist"));
        assert!(!markdown.contains(":::details Release checklist"));

        let xml = document_xml(&docx_of(rich_markdown));
        for expected in ["Tip", "Release checklist", "Export the note", "the roadmap"] {
            assert!(
                xml.contains(expected),
                "rich Notes content {expected:?} was lost from the PDF source document"
            );
        }
        assert!(!xml.contains(":::details Release checklist"));
        assert!(!xml.contains("[[Product Roadmap"));
        assert!(xml.contains("<w:br"), "soft line breaks should survive in PDF");

        let pdf_out = dir.join("note.pdf");
        tauri::async_runtime::block_on(notes_export_styled_pdf(NotesPdfOptions {
            markdown: rich_markdown.to_owned(),
            output_path: pdf_out.to_string_lossy().to_string(),
            title: Some("Export test".into()),
            base_dir: None,
        }))
        .expect("pdf export should succeed");
        let pdf = std::fs::read(&pdf_out).unwrap();
        assert!(
            pdf.starts_with(b"%PDF"),
            "PDF export should write a valid PDF header"
        );


        let html_out = dir.join("note.html");
        notes_export_html(NotesHtmlOptions {
            markdown: rich_markdown.to_owned(),
            output_path: html_out.to_string_lossy().to_string(),
            title: Some("Export test".into()),
            base_dir: None,
        })
        .expect("html export should succeed");
        let html = std::fs::read_to_string(&html_out).unwrap();
        assert!(html.contains("<h2>Release checklist</h2>"));
        assert!(html.contains("<strong>Tip</strong> Keep this handy"));
        assert!(html.contains("Read the roadmap."));
        assert!(!html.contains(":::details Release checklist"));
        assert!(!html.contains("[[Product Roadmap"));
        assert!(html.contains("<br"), "soft line breaks should survive in HTML");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn resized_images_keep_their_html_width() {
        let dir = std::env::temp_dir().join(format!(
            "kil-image-width-export-test-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let output = dir.join("note.html");

        notes_export_html(NotesHtmlOptions {
            markdown: r#"![Preview](https://example.com/preview.png "width=240")"#.to_owned(),
            output_path: output.to_string_lossy().to_string(),
            title: None,
            base_dir: None,
        })
        .expect("html export should succeed");

        let html = std::fs::read_to_string(&output).unwrap();
        assert!(html.contains(r#"style="width: 240px; max-width: 100%; height: auto;""#));
        assert_eq!(image_width_from_title("width=240"), Some(240));
        assert_eq!(image_width_from_title("width=79"), None);
        let pixel = image::RgbaImage::from_pixel(1, 1, image::Rgba([0, 0, 0, 255]));
        let mut png = Vec::new();
        image::DynamicImage::ImageRgba8(pixel)
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .expect("test image should encode");

        let _ = std::fs::remove_dir_all(&dir);
    }
    #[test]
    fn writing_blocks_note_exports_to_pdf() {
        let markdown = concat!(
            "# Writing blocks\n\n",
            "This note shows the main writing blocks in one place.\n\n",
            "> \\[!NOTE\\] Callouts keep important context close to the work.\n\n",
            "> \\[!TIP\\] Use the toolbar or slash commands to add blocks while writing.\n\n",
            "> \\[!WARNING\\] This is only example content. You can safely edit or delete this folder.\n\n",
            "## Checklist\n\n",
            "- [x] Try adding a new task\n\n",
            "- [x] Confirm completed tasks are visible\n\n",
            "## Table\n\n",
            "| Block | Try it |\n| --- | --- |\n",
            "| Callout | Add a note, tip, or warning |\n",
            "| Collapsible | Click its title, then write inside it |\n\n",
            ":::details Release checklist\n- [ ] Expand and collapse this section\n\n- [ ] Add a paragraph inside it\n\n:::\n\n",
            "## File cards\n\n",
            "Use the paperclip to attach a real local file. Its card appears here after you add it."
        );
        let normalized = normalize_notes_markdown(markdown);
        assert!(normalized.contains("> **Note** Callouts keep important context close to the work."));
        assert!(normalized.contains("> **Tip** Use the toolbar or slash commands to add blocks while writing."));
        assert!(!normalized.contains(r"\[!NOTE\]"));
        let output = std::env::temp_dir().join(format!(
            "kil-writing-blocks-pdf-test-{}.pdf",
            std::process::id()
        ));
        let markdown_output = output.with_extension("md");
        notes_export_markdown(NotesPdfOptions {
            markdown: markdown.to_owned(),
            output_path: markdown_output.to_string_lossy().to_string(),
            title: Some("Notes example - Writing blocks".into()),
            base_dir: None,
        })
        .expect("the saved Writing blocks note should export as Markdown");
        let markdown_export = std::fs::read_to_string(&markdown_output).unwrap();
        assert!(markdown_export.contains("> **Note** Callouts keep important context close to the work."));
        assert!(!markdown_export.contains(r"\[!NOTE\]"));
        let html_output = output.with_extension("html");
        notes_export_html(NotesHtmlOptions {
            markdown: markdown.to_owned(),
            output_path: html_output.to_string_lossy().to_string(),
            title: Some("Notes example - Writing blocks".into()),
            base_dir: None,
        })
        .expect("the saved Writing blocks note should export as HTML");
        let html_export = std::fs::read_to_string(&html_output).unwrap();
        assert!(html_export.contains("<strong>Note</strong> Callouts keep important context close to the work."));
        assert!(!html_export.contains("[!NOTE]"));
        tauri::async_runtime::block_on(notes_export_styled_pdf(NotesPdfOptions {
            markdown: markdown.to_owned(),
            output_path: output.to_string_lossy().to_string(),
            title: Some("Notes example - Writing blocks".into()),
            base_dir: None,
        }))
        .expect("the saved Writing blocks note should export as PDF");
        assert!(std::fs::read(&output).unwrap().starts_with(b"%PDF"));
        let _ = std::fs::remove_file(output);
        let _ = std::fs::remove_file(markdown_output);
        let _ = std::fs::remove_file(html_output);
    }


    /// THE REGRESSION THIS FILE EXISTS FOR. Tables were parsed but had no
    /// handler, so every cell's text hit an Event::Text arm that only appended
    /// to an OPEN paragraph — and inside a table none was open. The text was
    /// dropped on the floor and the table vanished from the exported PDF with
    /// no error. Silent data loss in a notes app.
    #[test]
    fn table_cell_text_survives_export() {
        let md = "\
| Fruit | Colour |
| ----- | ------ |
| apple | red    |
| plum  | purple |
";
        let xml = document_xml(&docx_of(md));

        for expected in ["Fruit", "Colour", "apple", "red", "plum", "purple"] {
            assert!(
                xml.contains(expected),
                "table cell text {expected:?} was lost from the export"
            );
        }
        // And it should be a real table, not just loose paragraphs.
        assert!(xml.contains("<w:tbl>"), "expected a real DOCX table element");
    }

    /// Link text used to render while its href was dropped entirely, so an
    /// exported note lost every reference it pointed at.
    #[test]
    fn link_target_survives_export() {
        let xml = document_xml(&docx_of("See [the docs](https://example.com/guide) for more."));
        assert!(xml.contains("the docs"), "link text should render");
        assert!(
            xml.contains("hyperlink") || xml.contains("Hyperlink"),
            "link should export as a real hyperlink, not plain text"
        );
    }

    /// The PDF parser had ENABLE_TASKLISTS off while the HTML parser had it on,
    /// so the same note exported as a checkbox in one format and the literal
    /// text "[ ]" in the other.
    #[test]
    fn task_list_renders_a_checkbox_not_literal_brackets() {
        let xml = document_xml(&docx_of("- [ ] todo\n- [x] done\n"));
        assert!(xml.contains('\u{2610}'), "unchecked box glyph expected");
        assert!(xml.contains('\u{2611}'), "checked box glyph expected");
        assert!(
            !xml.contains("[ ]") && !xml.contains("[x]"),
            "literal task-list brackets should not reach the document"
        );
    }

    /// Ordinary prose must not regress while fixing the above.
    #[test]
    fn plain_paragraphs_and_headings_still_export() {
        let xml = document_xml(&docx_of("# Title\n\nSome **bold** body text.\n"));
        assert!(xml.contains("Title"));
        assert!(xml.contains("Some "));
        assert!(xml.contains("bold"));
        assert!(xml.contains("body text."));
    }

    /// Raw HTML embedded in a note must NOT reach the exported .html as live
    /// markup: that file is opened in a real browser, where scripts run. Notes
    /// are routinely pasted from web pages, and the in-app preview already
    /// sanitises for this reason — the export path did not.
    #[test]
    fn raw_html_is_neutralised_in_html_export() {
        let dir = std::env::temp_dir().join(format!("kil-html-export-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let out = dir.join("note.html");

        notes_export_html(NotesHtmlOptions {
            markdown: "Hello <script>alert('xss')</script> and <img src=x onerror=alert(1)>.\n"
                .to_string(),
            output_path: out.to_string_lossy().to_string(),
            title: Some("T".into()),
            base_dir: None,
        })
        .expect("html export should succeed");

        let html = std::fs::read_to_string(&out).unwrap();

        // The test is whether any LIVE tag survives — not whether the attribute
        // TEXT appears. `onerror=alert(1)` still shows up as visible characters
        // inside `&lt;img …&gt;` because `=` and `(` aren't HTML-escaped; that
        // string is inert. What must never appear is an unescaped `<tag`.
        assert!(
            !html.contains("<script"),
            "a live <script> tag must never reach the exported file"
        );
        assert!(
            !html.contains("<img src=x"),
            "a live <img> tag with an event handler must never reach the file"
        );
        // Escaped-and-visible is the intended outcome — the author still sees
        // what they wrote, and nothing can execute.
        assert!(
            html.contains("&lt;script&gt;"),
            "raw HTML should be escaped to visible text"
        );
        assert!(
            html.contains("&lt;img src=x onerror=alert(1)&gt;"),
            "the img tag should survive as escaped text, not vanish"
        );
        assert!(html.contains("Hello"), "surrounding prose must survive");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
