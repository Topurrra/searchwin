//! Quality Pass Wave 1 / DF-4 (2026-05-29): visual preview for
//! DuplicateFinder rows.
//!
//! Before this module, the user looking at a group of "same hash"
//! files had to copy a path out and open it in a separate viewer to
//! decide which copy to keep. That's friction. Instead, every row gets
//! an inline preview pane that renders the actual content right there —
//! image thumbnail full-size, PDF / DOCX / TXT extracted text, audio /
//! video HTML5 player, hex dump for binaries.
//!
//! The principle: zero cloud round-trip, zero subscription, zero
//! third-party renderer. We reuse the same text extraction pipeline
//! the search index already runs (so PDF / DOCX / XLSX / PPTX / ODT /
//! RTF / code files all "just work") and delegate image / audio /
//! video rendering to Tauri's built-in `asset://` protocol — the
//! frontend builds the URL via `convertFileSrc` and the OS-native
//! renderer takes it from there. No extra dependencies, no extra heat.
//!
//! Preview text is capped at 16 KB to keep the UI snappy on huge
//! files. A 16 KB cap on a 200-page PDF still gives the user the first
//! few pages of body text — plenty to recognise "is this the right
//! copy?" without spending 200 ms decoding the rest.

use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use super::text_extract::{extract_text_for_indexing, is_text_or_doc_extension};

/// How much extracted text we ship to the frontend. 16 KB is enough
/// for ~250 lines of code or a few pages of a PDF — enough to identify
/// "yes, this is the contract I'm looking for" without flooding the
/// UI. The frontend then renders into a max-height scroller.
pub const PREVIEW_TEXT_CAP: usize = 16 * 1024;

/// Bytes shown in the hex panel for `Binary` previews. 256 = 16 rows
/// of 16 bytes, a familiar shape from every hex editor.
pub const PREVIEW_HEX_BYTES: usize = 256;

/// What the frontend should render. Tagged in camelCase so the Svelte
/// component pattern-matches cleanly.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum PreviewPayload {
    /// Image file — frontend renders with `<img>` + `convertFileSrc`.
    /// No content payload here; the asset:// URL is built browser-side.
    Image {
        meta: PreviewMeta,
    },
    /// Audio / video — frontend renders with `<audio>` / `<video>`
    /// + `convertFileSrc`. No content payload.
    AudioVideo {
        meta: PreviewMeta,
        /// "audio" or "video" — picks the right HTML5 element.
        media_kind: String,
    },
    /// Plain-text or code file — content is ready to drop into a
    /// `<pre>` block. `language_hint` lets the frontend optionally
    /// pick a syntax highlighter (Shiki / Prism / etc.). Empty when
    /// we can't infer one.
    Text {
        meta: PreviewMeta,
        content: String,
        language_hint: String,
        truncated: bool,
    },
    /// PDF / DOCX / XLSX / PPTX / ODT / RTF — we ran the same
    /// extractor the search index uses and got body text. Same
    /// rendering as `Text` but flagged so the UI can show a
    /// "Extracted text · open original in viewer" affordance.
    Document {
        meta: PreviewMeta,
        content: String,
        truncated: bool,
    },
    /// Unknown binary — show a hex dump of the first 256 bytes plus
    /// the metadata block. Catches archives / executables / raw
    /// camera files / anything we can't decode meaningfully.
    Binary {
        meta: PreviewMeta,
        hex_preview: String,
    },
    /// File disappeared / unreadable between scan and preview. Frontend
    /// renders a soft "File no longer available" message.
    Missing {
        path: String,
        reason: String,
    },
}

/// Metadata block every kind carries — file path, basename, byte size,
/// last-modified ms (epoch), and extension. Lets the preview pane show
/// the same details row whether the body is a thumbnail or a hex dump.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewMeta {
    pub path: String,
    pub file_name: String,
    pub extension: String,
    pub size_bytes: u64,
    pub modified_ms: Option<u128>,
}

impl PreviewMeta {
    fn from_path(path: &Path) -> Option<Self> {
        let metadata = fs::metadata(path).ok()?;
        let modified_ms = metadata
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_millis());
        let extension = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|s| s.to_ascii_lowercase())
            .unwrap_or_default();
        let file_name = path
            .file_name()
            .and_then(|f| f.to_str())
            .map(|s| s.to_string())
            .unwrap_or_default();
        Some(PreviewMeta {
            path: path.to_string_lossy().to_string(),
            file_name,
            extension,
            size_bytes: metadata.len(),
            modified_ms,
        })
    }
}

fn classify(extension: &str) -> Classification {
    match extension {
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "tif" | "tiff" | "ico" | "svg" => {
            Classification::Image
        }
        "mp3" | "wav" | "ogg" | "flac" | "m4a" | "aac" | "opus" => {
            Classification::Audio
        }
        "mp4" | "webm" | "mkv" | "mov" | "avi" | "m4v" => Classification::Video,
        _ => Classification::Other,
    }
}

enum Classification {
    Image,
    Audio,
    Video,
    Other,
}

/// Map a known code/text extension to a Shiki/Prism-friendly language
/// hint. Returns "" when we don't have a confident match — the frontend
/// then falls back to plain `<pre>` rendering.
fn language_hint_for(extension: &str) -> &'static str {
    match extension {
        "rs" => "rust",
        "js" | "mjs" | "cjs" => "javascript",
        "ts" => "typescript",
        "jsx" => "jsx",
        "tsx" => "tsx",
        "py" => "python",
        "go" => "go",
        "java" => "java",
        "kt" | "kts" => "kotlin",
        "c" | "h" => "c",
        "cpp" | "hpp" | "cc" | "hh" | "cxx" => "cpp",
        "swift" => "swift",
        "php" => "php",
        "rb" => "ruby",
        "sh" | "bash" | "zsh" => "bash",
        "ps1" => "powershell",
        "svelte" => "svelte",
        "html" | "htm" => "html",
        "css" => "css",
        "scss" | "sass" => "scss",
        "json" => "json",
        "yaml" | "yml" => "yaml",
        "toml" => "toml",
        "xml" => "xml",
        "md" | "markdown" => "markdown",
        "csv" | "tsv" => "csv",
        "sql" => "sql",
        "ini" | "cfg" | "conf" => "ini",
        _ => "",
    }
}

/// Read up to `cap` bytes from the start of the file and format them as
/// a classic hex+ascii dump (16 bytes per line). Falls back to a short
/// error string on IO failure so the panel always has something to
/// render.
fn hex_dump_head(path: &Path, cap: usize) -> String {
    use std::io::Read;
    let mut file = match fs::File::open(path) {
        Ok(f) => f,
        Err(e) => return format!("(cannot open file: {e})"),
    };
    let mut buf = vec![0u8; cap];
    let n = match file.read(&mut buf) {
        Ok(n) => n,
        Err(e) => return format!("(cannot read file: {e})"),
    };
    let mut out = String::with_capacity(n * 4);
    for (i, chunk) in buf[..n].chunks(16).enumerate() {
        let offset = i * 16;
        out.push_str(&format!("{offset:08x}  "));
        // Hex bytes.
        for (j, byte) in chunk.iter().enumerate() {
            out.push_str(&format!("{byte:02x} "));
            if j == 7 {
                out.push(' ');
            }
        }
        // Pad short final row so the ascii column lines up.
        let pad = 16 - chunk.len();
        for _ in 0..pad {
            out.push_str("   ");
        }
        if chunk.len() <= 8 {
            out.push(' ');
        }
        // Printable-ASCII column.
        out.push('|');
        for byte in chunk {
            let ch = if (0x20..0x7f).contains(byte) {
                *byte as char
            } else {
                '.'
            };
            out.push(ch);
        }
        out.push('|');
        out.push('\n');
    }
    out
}

/// Inner worker — takes a borrowed `Path` so the Tauri-command wrapper
/// can validate the input string first. Returns `Missing` (not Err) on
/// any IO failure so the frontend never has to handle a thrown error;
/// the preview pane just shows a friendly "not available" state.
pub fn preview_payload_for(path: &Path) -> PreviewPayload {
    let Some(meta) = PreviewMeta::from_path(path) else {
        return PreviewPayload::Missing {
            path: path.to_string_lossy().to_string(),
            reason: "Cannot read file metadata".to_string(),
        };
    };

    match classify(&meta.extension) {
        Classification::Image => PreviewPayload::Image { meta },
        Classification::Audio => PreviewPayload::AudioVideo {
            meta,
            media_kind: "audio".to_string(),
        },
        Classification::Video => PreviewPayload::AudioVideo {
            meta,
            media_kind: "video".to_string(),
        },
        Classification::Other => {
            // Text / code / document — try the shared extractor first.
            // We pass PREVIEW_TEXT_CAP as the user_max_bytes ceiling, so
            // the extractor's per-format tier limit applies BUT clamped
            // down to our 16 KB preview budget.
            if is_text_or_doc_extension(&meta.extension) {
                if let Some(text) = extract_text_for_indexing(path, PREVIEW_TEXT_CAP) {
                    // Trim to PREVIEW_TEXT_CAP UTF-8 bytes safely — the
                    // extractor already respects the cap, but some tier
                    // implementations may overshoot by a chunk.
                    let truncated = text.len() >= PREVIEW_TEXT_CAP;
                    let content = if truncated {
                        // Walk back to the closest char boundary so we
                        // don't slice in the middle of a multibyte
                        // sequence (PDFs full of em-dashes / accents).
                        let mut end = PREVIEW_TEXT_CAP;
                        while end > 0 && !text.is_char_boundary(end) {
                            end -= 1;
                        }
                        text[..end].to_string()
                    } else {
                        text
                    };

                    // Plain text / code → render with monospace +
                    // optional syntax highlight. Documents → render the
                    // same way but the frontend will badge the panel as
                    // "Extracted text".
                    let is_document = matches!(
                        meta.extension.as_str(),
                        "pdf" | "docx" | "xlsx" | "pptx" | "odt" | "ods" | "odp" | "rtf"
                    );
                    if is_document {
                        return PreviewPayload::Document {
                            meta,
                            content,
                            truncated,
                        };
                    } else {
                        let language_hint = language_hint_for(&meta.extension).to_string();
                        return PreviewPayload::Text {
                            meta,
                            content,
                            language_hint,
                            truncated,
                        };
                    }
                }
                // Extractor returned None → fall through to Binary so the
                // user still sees something useful (hex + metadata).
            }

            // Anything else (archives, executables, raw formats) →
            // hex dump of the head.
            PreviewPayload::Binary {
                meta,
                hex_preview: hex_dump_head(path, PREVIEW_HEX_BYTES),
            }
        }
    }
}

/// Tauri command. Accepts the path as a string (matches what the
/// frontend already has from the scan result), validates it as a real
/// user-path before reading anything off disk.
#[tauri::command]
pub fn preview_duplicate_file(path: String) -> PreviewPayload {
    // Soft-validate the path so the preview command never reads from a
    // system-protected location even if a buggy frontend asks. The
    // duplicate scan itself already gated the roots, but defense in
    // depth — the preview command is reachable from any IPC client.
    if let Err(reason) = crate::core::safe_path::validate_user_path(&path) {
        return PreviewPayload::Missing { path, reason };
    }
    let resolved = PathBuf::from(&path);
    preview_payload_for(&resolved)
}
