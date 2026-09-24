//! Word `.docx` edit-restriction removal.
//!
//! `.docx` files can carry two distinct kinds of "password" protection:
//!
//! 1. **Edit restriction** (a.k.a. "Restrict Editing" in Word's menu).
//!    Stored as a `<w:documentProtection w:edit="readOnly" ... />`
//!    element in `word/settings.xml`. The file is **not encrypted** —
//!    Word just refuses to let you edit unless you enter the password
//!    that hashes to the recorded value. Other software ignores it
//!    entirely. We can strip it by editing the XML, no password needed.
//!
//! 2. **Open password** ("Encrypt with Password" in Word's menu). The
//!    file becomes a Compound File Binary container with AES-encrypted
//!    streams. Removing this requires decrypting with the password —
//!    significantly more complex. **Not supported by this command.**
//!    Users with that kind of protection need to open the file in Word
//!    first, then save without password.
//!
//! This command targets case (1) only and surfaces a clear error when it
//! detects case (2) so users aren't confused.
//!
//! Implementation: open the .docx as a ZIP, find `word/settings.xml`,
//! remove the protection element, write the ZIP back to a new file
//! alongside the original (with `_unlocked` suffix by default).

use crate::core::safe_path::{forbid_system_path, validate_user_path};
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use tauri::AppHandle;

const SETTINGS_PATH: &str = "word/settings.xml";

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocxUnlockOptions {
    pub paths: Vec<String>,
    pub output_dir: Option<String>,
    /// Optional suffix appended before the file extension. Defaults to
    /// `_unlocked` so the original file is preserved untouched.
    pub suffix: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocxUnlockResult {
    pub source_path: String,
    pub output_path: Option<String>,
    pub success: bool,
    pub error: Option<String>,
    /// True if the source file had any protection element to remove.
    /// False means the file was already unprotected — we still produce
    /// an unchanged copy so the user has a uniform output.
    pub protection_removed: bool,
}

#[tauri::command]
pub async fn remove_docx_password(
    _app: AppHandle,
    options: DocxUnlockOptions,
) -> Result<Vec<DocxUnlockResult>, String> {
    // Security gate: validate inputs and the output directory if given.
    for path in &options.paths {
        validate_user_path(path)?;
    }
    if let Some(ref dir) = options.output_dir {
        forbid_system_path(dir)?;
    }

    let suffix = options
        .suffix
        .as_deref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .unwrap_or("_unlocked")
        .to_string();

    tauri::async_runtime::spawn_blocking(move || {
        let mut results = Vec::with_capacity(options.paths.len());
        for source in &options.paths {
            let source_path = PathBuf::from(source);
            let result = process_docx(&source_path, options.output_dir.as_deref(), &suffix);
            match result {
                Ok((output_path, removed)) => results.push(DocxUnlockResult {
                    source_path: source.clone(),
                    output_path: Some(output_path.to_string_lossy().to_string()),
                    success: true,
                    error: None,
                    protection_removed: removed,
                }),
                Err(error) => results.push(DocxUnlockResult {
                    source_path: source.clone(),
                    output_path: None,
                    success: false,
                    error: Some(error),
                    protection_removed: false,
                }),
            }
        }
        results
    })
    .await
    .map_err(|error| format!("Worker join failed: {error}"))
}

fn process_docx(
    source_path: &Path,
    output_dir: Option<&str>,
    suffix: &str,
) -> Result<(PathBuf, bool), String> {
    if source_path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("docx"))
        != Some(true)
    {
        return Err("Only .docx files are supported.".to_string());
    }

    // Read source ZIP into memory. We need to walk all entries + write a
    // fresh ZIP at the destination, so streaming would only buy us memory
    // for very large docs. .docx files are usually <10MB; in-memory is fine.
    let source_bytes = std::fs::read(source_path)
        .map_err(|e| format!("Cannot read source: {e}"))?;
    let cursor = std::io::Cursor::new(&source_bytes);
    let mut zip = zip::ZipArchive::new(cursor)
        .map_err(|e| format!("Not a valid .docx (ZIP): {e}"))?;

    // Compute output path. If output_dir is provided we drop the file
    // there; otherwise we sit alongside the source with the suffix
    // appended before the extension.
    let output_path = compute_output_path(source_path, output_dir, suffix)?;

    let output_file =
        File::create(&output_path).map_err(|e| format!("Cannot create output: {e}"))?;
    let mut writer = zip::ZipWriter::new(output_file);
    let options =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    let mut protection_removed = false;
    // Detect cryptographic encryption — if the .docx is actually a
    // CFBF-encrypted container masquerading as a ZIP, ZIP parsing
    // either fails or the entries we expect aren't present. We check
    // for `[Content_Types].xml` (a required entry of every real .docx).
    // Its absence signals encrypted-open-password rather than a
    // restrict-editing case.
    let mut found_content_types = false;

    for i in 0..zip.len() {
        let mut entry = zip
            .by_index(i)
            .map_err(|e| format!("Read entry failed: {e}"))?;
        let name = entry.name().to_string();
        if name == "[Content_Types].xml" {
            found_content_types = true;
        }
        let mut buf = Vec::with_capacity(entry.size() as usize);
        entry
            .read_to_end(&mut buf)
            .map_err(|e| format!("Read entry bytes failed: {e}"))?;

        // For settings.xml, strip the documentProtection + writeReservation
        // elements before re-writing.
        if name == SETTINGS_PATH {
            let original_len = buf.len();
            buf = strip_protection_elements(&buf);
            if buf.len() != original_len {
                protection_removed = true;
            }
        }

        writer
            .start_file(name, options)
            .map_err(|e| format!("Start ZIP entry failed: {e}"))?;
        writer
            .write_all(&buf)
            .map_err(|e| format!("Write ZIP entry failed: {e}"))?;
    }

    writer
        .finish()
        .map_err(|e| format!("Finalize ZIP failed: {e}"))?;

    if !found_content_types {
        // Best-effort cleanup of the partial output before returning.
        let _ = std::fs::remove_file(&output_path);
        return Err(
            "This document uses an open-password (encrypted) — open it in Word and re-save without a password to remove that kind of protection."
                .to_string(),
        );
    }

    Ok((output_path, protection_removed))
}

/// Compute the destination path. Either `<output_dir>/<basename><suffix>.docx`
/// or alongside the source with the same naming convention.
fn compute_output_path(
    source_path: &Path,
    output_dir: Option<&str>,
    suffix: &str,
) -> Result<PathBuf, String> {
    let stem = source_path
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| "Source has no filename".to_string())?;
    let ext = source_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("docx");
    let new_name = format!("{stem}{suffix}.{ext}");
    let target_dir: PathBuf = match output_dir {
        Some(dir) => PathBuf::from(dir),
        None => source_path
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| PathBuf::from(".")),
    };
    if !target_dir.exists() {
        std::fs::create_dir_all(&target_dir)
            .map_err(|e| format!("Cannot create output directory: {e}"))?;
    }
    Ok(target_dir.join(new_name))
}

/// Strip `<w:documentProtection .../>` and `<w:writeProtection .../>` from
/// a `settings.xml` byte buffer. We do this with a byte-level search rather
/// than a full XML parse — the settings.xml is small + simple, the elements
/// we target have a fixed prefix, and a regex/parser dependency would be
/// overkill. Returns the cleaned buffer.
///
/// Both element variants are self-closing in practice; we handle that
/// case explicitly. If a future Word version ships with a paired
/// `<w:documentProtection>...</w:documentProtection>`, the simple
/// truncation here might leave a stray closing tag — but every Word
/// version we've seen uses self-closing form.
fn strip_protection_elements(input: &[u8]) -> Vec<u8> {
    let mut out = input.to_vec();
    out = strip_self_closing_tag(&out, b"<w:documentProtection ");
    out = strip_self_closing_tag(&out, b"<w:writeProtection ");
    out
}

/// Remove every `<prefix .../>` substring (where the suffix is `/>`) from
/// the buffer. Case-sensitive byte match — Word writes consistent casing
/// for these elements.
fn strip_self_closing_tag(input: &[u8], prefix: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(input.len());
    let mut i = 0;
    while i < input.len() {
        if i + prefix.len() <= input.len() && &input[i..i + prefix.len()] == prefix {
            // Find the closing `/>` after this prefix.
            let mut j = i + prefix.len();
            let mut found = false;
            while j + 1 < input.len() {
                if input[j] == b'/' && input[j + 1] == b'>' {
                    found = true;
                    break;
                }
                j += 1;
            }
            if found {
                // Skip the entire `<prefix ... />` block.
                i = j + 2;
                continue;
            }
        }
        out.push(input[i]);
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_self_closing_protection_tag() {
        let input = br#"<?xml version="1.0"?><w:settings><w:zoom w:percent="100"/><w:documentProtection w:edit="readOnly" w:enforcement="1"/></w:settings>"#;
        let out = strip_protection_elements(input);
        let s = std::str::from_utf8(&out).unwrap();
        assert!(!s.contains("documentProtection"), "tag should be removed: {s}");
        assert!(s.contains("zoom"), "other tags should be preserved");
    }

    #[test]
    fn strips_write_protection_tag_too() {
        let input =
            br#"<w:settings><w:writeProtection w:cryptProviderType="rsaAES"/></w:settings>"#;
        let out = strip_protection_elements(input);
        let s = std::str::from_utf8(&out).unwrap();
        assert!(!s.contains("writeProtection"));
    }

    #[test]
    fn unprotected_settings_pass_through_unchanged() {
        let input = br#"<w:settings><w:zoom w:percent="100"/></w:settings>"#;
        let out = strip_protection_elements(input);
        assert_eq!(out, input);
    }

    #[test]
    fn handles_empty_input() {
        let out = strip_protection_elements(b"");
        assert_eq!(out, Vec::<u8>::new());
    }
}
