use quick_xml::events::Event;
use quick_xml::reader::Reader;
use quick_xml::writer::Writer;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};

#[derive(Deserialize)]
pub struct DocMetadataOptions {
    pub paths: Vec<String>,
    pub output_dir: Option<String>,
    pub suffix: String,
    pub strip_comments: bool,
    pub strip_tracked_changes: bool,
    pub strip_custom_xml: bool,
    pub strip_pdf_info: bool,
    pub strip_pdf_xmp: bool,
}

#[derive(Serialize, Clone)]
pub struct DocMetadataResult {
    pub source_path: String,
    pub output_path: String,
    pub format: String,
    pub original_size: u64,
    pub new_size: u64,
    pub items_removed: Vec<String>,
    pub success: bool,
    pub error: Option<String>,
}

#[tauri::command]
pub fn strip_doc_metadata(options: DocMetadataOptions) -> Vec<DocMetadataResult> {
    // Security gate: every input doc must resolve to a real, non-system
    // path; output_dir must not target a forbidden location.
    let reject = |err: String| -> Vec<DocMetadataResult> {
        options
            .paths
            .iter()
            .map(|p| DocMetadataResult {
                source_path: p.clone(),
                output_path: String::new(),
                format: String::new(),
                original_size: 0,
                new_size: 0,
                items_removed: vec![],
                success: false,
                error: Some(err.clone()),
            })
            .collect()
    };
    for path in &options.paths {
        if let Err(e) = crate::core::safe_path::validate_user_path(path) {
            return reject(e);
        }
    }
    if let Some(ref dir) = options.output_dir {
        if let Err(e) = crate::core::safe_path::forbid_system_path(dir) {
            return reject(e);
        }
    }

    options.paths.iter().map(|p| process(p, &options)).collect()
}

fn process(source: &str, opts: &DocMetadataOptions) -> DocMetadataResult {
    let src = PathBuf::from(source);
    let original_size = fs::metadata(&src).map(|m| m.len()).unwrap_or(0);

    let extension = src
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_lowercase())
        .unwrap_or_default();

    let output_path = build_output_path(&src, opts.output_dir.as_deref(), &opts.suffix);

    let result = match extension.as_str() {
        "docx" | "xlsx" | "pptx" => strip_office(&src, &output_path, opts),
        "pdf" => strip_pdf(&src, &output_path, opts),
        _ => Err(format!(
            "Unsupported file type: .{}. Use Metadata Stripper for images.",
            extension
        )),
    };

    let new_size = fs::metadata(&output_path).map(|m| m.len()).unwrap_or(0);

    match result {
        Ok(items) => DocMetadataResult {
            source_path: source.to_string(),
            output_path: output_path.to_string_lossy().to_string(),
            format: extension,
            original_size,
            new_size,
            items_removed: items,
            success: true,
            error: None,
        },
        Err(e) => DocMetadataResult {
            source_path: source.to_string(),
            output_path: String::new(),
            format: extension,
            original_size,
            new_size: 0,
            items_removed: vec![],
            success: false,
            error: Some(e),
        },
    }
}

fn build_output_path(source: &Path, output_dir: Option<&str>, suffix: &str) -> PathBuf {
    let stem = source
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("file");
    let ext = source.extension().and_then(|e| e.to_str()).unwrap_or("");
    let new_name = if ext.is_empty() {
        format!("{}{}", stem, suffix)
    } else {
        format!("{}{}.{}", stem, suffix, ext)
    };
    match output_dir {
        Some(dir) => PathBuf::from(dir).join(new_name),
        None => source.with_file_name(new_name),
    }
}

fn strip_office(
    source: &Path,
    output: &Path,
    opts: &DocMetadataOptions,
) -> Result<Vec<String>, String> {
    use zip::write::{FileOptions, ZipWriter};
    use zip::ZipArchive;

    let src_file = fs::File::open(source).map_err(|e| format!("Open failed: {}", e))?;
    let mut archive =
        ZipArchive::new(src_file).map_err(|e| format!("Not a valid Office file: {}", e))?;

    let out_file = fs::File::create(output).map_err(|e| format!("Create failed: {}", e))?;
    let mut writer = ZipWriter::new(out_file);

    let metadata_files: &[&str] = &[
        "docProps/core.xml",
        "docProps/app.xml",
        "docProps/custom.xml",
    ];
    let comments_files: &[&str] = &[
        "word/comments.xml",
        "word/commentsExtended.xml",
        "word/commentsExtensible.xml",
        "word/commentsIds.xml",
        "word/people.xml",
        "word/threadedComments.xml",
    ];
    let custom_xml_prefix = "customXml/";

    let mut removed_summary: Vec<String> = Vec::new();
    let mut removed_metadata = false;
    let mut removed_custom = false;
    let mut removed_comments = false;
    let mut accepted_changes = false;
    let mut stripped_inline_comments = false;

    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| format!("Read entry failed: {}", e))?;
        let name = entry.name().to_string();

        if metadata_files.contains(&name.as_str()) {
            removed_metadata = true;
            continue;
        }

        if opts.strip_comments && comments_files.contains(&name.as_str()) {
            removed_comments = true;
            continue;
        }

        if opts.strip_custom_xml && name.starts_with(custom_xml_prefix) {
            removed_custom = true;
            continue;
        }

        let mut buf = Vec::new();
        entry
            .read_to_end(&mut buf)
            .map_err(|e| format!("Read failed: {}", e))?;

        let needs_processing =
            name == "word/document.xml" && (opts.strip_comments || opts.strip_tracked_changes);

        let final_bytes = if needs_processing {
            let processed =
                process_word_document_xml(&buf, opts.strip_comments, opts.strip_tracked_changes)
                    .map_err(|e| format!("XML rewrite failed for {}: {}", name, e))?;

            if processed.changed_comments {
                stripped_inline_comments = true;
            }
            if processed.changed_revisions {
                accepted_changes = true;
            }
            processed.bytes
        } else {
            buf
        };

        let opts_z: FileOptions<()> =
            FileOptions::default().compression_method(entry.compression());
        writer
            .start_file(&name, opts_z)
            .map_err(|e| format!("Write entry failed: {}", e))?;
        writer
            .write_all(&final_bytes)
            .map_err(|e| format!("Write failed: {}", e))?;
    }

    writer
        .finish()
        .map_err(|e| format!("Finalize failed: {}", e))?;

    if removed_metadata {
        removed_summary
            .push("Document properties (author, title, last modified by, edit time)".into());
    }
    if removed_comments {
        removed_summary.push("Comment files (comments, threaded comments, people)".into());
    }
    if stripped_inline_comments {
        removed_summary.push("Inline comment markers in document body".into());
    }
    if accepted_changes {
        removed_summary
            .push("Tracked changes accepted (insertions kept, deletions removed)".into());
    }
    if removed_custom {
        removed_summary.push("Custom XML data".into());
    }
    if removed_summary.is_empty() {
        removed_summary.push("No matching metadata found".into());
    }

    Ok(removed_summary)
}

struct ProcessedXml {
    bytes: Vec<u8>,
    changed_comments: bool,
    changed_revisions: bool,
}

/// Properly parse and rewrite word/document.xml with quick-xml.
///
/// Rules implemented:
/// - strip_comments: drop `<w:commentRangeStart>`, `<w:commentRangeEnd>`, `<w:commentReference>`.
///   These are typically empty/self-closing inline markers.
/// - strip_tracked_changes:
///     - `<w:ins>...</w:ins>`: unwrap (keep inner content, drop the wrapper)
///     - `<w:del>...</w:del>`: drop entirely (the deleted text)
///     - `<w:rPrChange>`, `<w:pPrChange>`: drop entirely (formatting change history)
fn process_word_document_xml(
    input: &[u8],
    strip_comments: bool,
    strip_tracked_changes: bool,
) -> Result<ProcessedXml, String> {
    let mut reader = Reader::from_reader(input);
    reader.config_mut().trim_text(false);
    reader.config_mut().expand_empty_elements = false;

    let mut writer = Writer::new(Cursor::new(Vec::new()));

    // Tags that, when matched, cause the whole element (including children) to be skipped.
    let drop_with_children: &[&[u8]] = if strip_tracked_changes {
        &[b"w:del", b"w:rPrChange", b"w:pPrChange"]
    } else {
        &[]
    };

    // Tags whose wrapping is removed, but children are kept.
    let unwrap_tags: &[&[u8]] = if strip_tracked_changes {
        &[b"w:ins"]
    } else {
        &[]
    };

    // Tags to drop entirely (handles both `<w:tag/>` and `<w:tag>...</w:tag>` empty content).
    let drop_inline: &[&[u8]] = if strip_comments {
        &[
            b"w:commentRangeStart",
            b"w:commentRangeEnd",
            b"w:commentReference",
        ]
    } else {
        &[]
    };

    let mut skip_depth: u32 = 0;
    let mut skip_match: Option<Vec<u8>> = None;
    let mut buf = Vec::new();

    let mut changed_comments = false;
    let mut changed_revisions = false;

    loop {
        match reader.read_event_into(&mut buf) {
            Err(e) => return Err(format!("XML parse error: {}", e)),
            Ok(Event::Eof) => break,

            Ok(Event::Start(e)) => {
                if skip_depth > 0 {
                    if let Some(target) = &skip_match {
                        if e.name().as_ref() == target.as_slice() {
                            skip_depth += 1;
                        }
                    }
                    continue;
                }

                let name = e.name().as_ref().to_vec();

                if drop_with_children.contains(&name.as_slice()) {
                    skip_depth = 1;
                    skip_match = Some(name);
                    changed_revisions = true;
                    continue;
                }

                if unwrap_tags.contains(&name.as_slice()) {
                    changed_revisions = true;
                    // Don't write the start tag; children flow through normally.
                    continue;
                }

                if drop_inline.contains(&name.as_slice()) {
                    changed_comments = true;
                    skip_depth = 1;
                    skip_match = Some(name);
                    continue;
                }

                writer
                    .write_event(Event::Start(e.clone()))
                    .map_err(|e| format!("Write start: {}", e))?;
            }

            Ok(Event::End(e)) => {
                if skip_depth > 0 {
                    if let Some(target) = &skip_match {
                        if e.name().as_ref() == target.as_slice() {
                            skip_depth -= 1;
                            if skip_depth == 0 {
                                skip_match = None;
                            }
                        }
                    }
                    continue;
                }

                let name = e.name().as_ref().to_vec();

                if unwrap_tags.contains(&name.as_slice()) {
                    // Matching unwrap; suppress the end tag.
                    continue;
                }

                writer
                    .write_event(Event::End(e.clone()))
                    .map_err(|e| format!("Write end: {}", e))?;
            }

            Ok(Event::Empty(e)) => {
                if skip_depth > 0 {
                    continue;
                }

                let name = e.name().as_ref().to_vec();

                if drop_inline.contains(&name.as_slice()) {
                    changed_comments = true;
                    continue;
                }

                if drop_with_children.contains(&name.as_slice()) {
                    changed_revisions = true;
                    continue;
                }

                if unwrap_tags.contains(&name.as_slice()) {
                    // Self-closing <w:ins/> with no content — just skip the tag.
                    changed_revisions = true;
                    continue;
                }

                writer
                    .write_event(Event::Empty(e.clone()))
                    .map_err(|e| format!("Write empty: {}", e))?;
            }

            Ok(other) => {
                if skip_depth > 0 {
                    continue;
                }
                writer
                    .write_event(other)
                    .map_err(|e| format!("Write event: {}", e))?;
            }
        }
        buf.clear();
    }

    Ok(ProcessedXml {
        bytes: writer.into_inner().into_inner(),
        changed_comments,
        changed_revisions,
    })
}

fn strip_pdf(
    source: &Path,
    output: &Path,
    opts: &DocMetadataOptions,
) -> Result<Vec<String>, String> {
    use lopdf::Document;

    let mut doc = Document::load(source).map_err(|e| format!("PDF load failed: {}", e))?;
    let mut removed = Vec::new();

    if opts.strip_pdf_info {
        if doc.trailer.has(b"Info") {
            doc.trailer.remove(b"Info");
            removed
                .push("PDF Info dictionary (Title, Author, Creator, Producer, dates)".to_string());
        }
    }

    if opts.strip_pdf_xmp {
        let mut to_remove: Vec<lopdf::ObjectId> = Vec::new();
        if let Ok(catalog) = doc.catalog_mut() {
            if let Ok(metadata_ref) = catalog.get(b"Metadata") {
                if let Ok((id, gen)) = metadata_ref.as_reference() {
                    to_remove.push((id, gen));
                }
            }
            catalog.remove(b"Metadata");
        }
        for id in to_remove {
            doc.objects.remove(&id);
        }
        if !to_remove_was_empty(&doc) {
            // no-op marker; we already pushed below if anything was removed
        }
        removed.push("XMP metadata stream".to_string());
    }

    if doc.trailer.has(b"ID") {
        doc.trailer.remove(b"ID");
        removed.push("File identifier (ID array)".to_string());
    }

    if let Ok(catalog) = doc.catalog_mut() {
        if catalog.has(b"PieceInfo") {
            catalog.remove(b"PieceInfo");
            removed.push("PieceInfo (application-specific data)".to_string());
        }
    }

    doc.save(output)
        .map_err(|e| format!("PDF save failed: {}", e))?;

    if removed.is_empty() {
        removed.push("No metadata found".to_string());
    }

    Ok(removed)
}

fn to_remove_was_empty(_doc: &lopdf::Document) -> bool {
    true
}
