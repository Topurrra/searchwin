use img_parts::{DynImage, ImageEXIF, ImageICC};
use serde::Serialize;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

#[derive(Serialize, Clone)]
pub struct StripResult {
    pub source_path: String,
    pub output_path: String,
    pub format: String,
    pub original_size: u64,
    pub new_size: u64,
    pub metadata_removed: Vec<String>,
    pub success: bool,
    pub error: Option<String>,
}

#[tauri::command]
pub fn strip_metadata(
    paths: Vec<String>,
    output_dir: Option<String>,
    suffix: String,
) -> Vec<StripResult> {
    // Security gate: every input image/doc must resolve to a real,
    // non-system path; output_dir (if any) must not target a forbidden
    // location.
    let reject = |err: String| -> Vec<StripResult> {
        paths
            .iter()
            .map(|p| StripResult {
                source_path: p.clone(),
                output_path: String::new(),
                format: String::new(),
                original_size: 0,
                new_size: 0,
                metadata_removed: vec![],
                success: false,
                error: Some(err.clone()),
            })
            .collect()
    };
    for path in &paths {
        if let Err(e) = crate::core::safe_path::validate_user_path(path) {
            return reject(e);
        }
    }
    if let Some(ref dir) = output_dir {
        if let Err(e) = crate::core::safe_path::forbid_system_path(dir) {
            return reject(e);
        }
    }

    paths
        .into_iter()
        .map(|p| process_file(&p, output_dir.as_deref(), &suffix))
        .collect()
}

fn process_file(source: &str, output_dir: Option<&str>, suffix: &str) -> StripResult {
    let src_path = PathBuf::from(source);
    let original_size = fs::metadata(&src_path).map(|m| m.len()).unwrap_or(0);

    let extension = src_path
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_lowercase())
        .unwrap_or_default();

    let output_path = build_output_path(&src_path, output_dir, suffix);

    let result = match extension.as_str() {
        "jpg" | "jpeg" | "png" | "webp" | "tiff" | "tif" | "heic" | "heif" => {
            strip_image(&src_path, &output_path)
        }
        "docx" | "xlsx" | "pptx" => strip_office(&src_path, &output_path),
        "pdf" => Err(format!(
            "PDF metadata stripping isn't available here — use the Privacy Sanitizer for PDFs. Skipped: {}",
            src_path.display()
        )),
        _ => Err(format!("Unsupported file type: .{}", extension)),
    };

    let new_size = fs::metadata(&output_path).map(|m| m.len()).unwrap_or(0);

    match result {
        Ok(removed) => StripResult {
            source_path: source.to_string(),
            output_path: output_path.to_string_lossy().to_string(),
            format: extension,
            original_size,
            new_size,
            metadata_removed: removed,
            success: true,
            error: None,
        },
        Err(e) => StripResult {
            source_path: source.to_string(),
            output_path: String::new(),
            format: extension,
            original_size,
            new_size: 0,
            metadata_removed: vec![],
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

fn strip_image(source: &Path, output: &Path) -> Result<Vec<String>, String> {
    let bytes = fs::read(source).map_err(|e| format!("Read failed: {}", e))?;

    let mut removed = Vec::new();

    let mut img = DynImage::from_bytes(bytes.into())
        .map_err(|e| format!("Parse failed: {}", e))?
        .ok_or_else(|| "Unsupported image format".to_string())?;

    if img.exif().is_some() {
        img.set_exif(None);
        removed.push("EXIF (camera, GPS, timestamps)".to_string());
    }

    if img.icc_profile().is_some() {
        img.set_icc_profile(None);
        removed.push("ICC color profile".to_string());
    }

    let mut out_file = fs::File::create(output).map_err(|e| format!("Create failed: {}", e))?;
    img.encoder()
        .write_to(&mut out_file)
        .map_err(|e| format!("Write failed: {}", e))?;

    if removed.is_empty() {
        removed.push("No metadata found (file already clean)".to_string());
    }

    Ok(removed)
}

fn strip_office(source: &Path, output: &Path) -> Result<Vec<String>, String> {
    // Office files are zip archives. We rebuild the zip without docProps/core.xml,
    // docProps/app.xml, and docProps/custom.xml.
    let src_file = fs::File::open(source).map_err(|e| format!("Open failed: {}", e))?;
    let mut archive =
        zip::ZipArchive::new(src_file).map_err(|e| format!("Not a valid Office file: {}", e))?;

    let out_file = fs::File::create(output).map_err(|e| format!("Create failed: {}", e))?;
    let mut writer = zip::ZipWriter::new(out_file);

    let metadata_files = [
        "docProps/core.xml",   // author, title, last modified by, etc.
        "docProps/app.xml",    // app version, edit time, total time
        "docProps/custom.xml", // custom properties
    ];

    let mut removed = Vec::new();

    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| format!("Read entry failed: {}", e))?;
        let name = entry.name().to_string();

        if metadata_files.contains(&name.as_str()) {
            removed.push(format!("Removed: {}", name));
            continue;
        }

        let options: zip::write::FileOptions<()> =
            zip::write::FileOptions::default().compression_method(entry.compression());

        writer
            .start_file(&name, options)
            .map_err(|e| format!("Write entry failed: {}", e))?;
        let mut buf = Vec::new();
        entry
            .read_to_end(&mut buf)
            .map_err(|e| format!("Read failed: {}", e))?;
        writer
            .write_all(&buf)
            .map_err(|e| format!("Write failed: {}", e))?;
    }

    writer
        .finish()
        .map_err(|e| format!("Finalize failed: {}", e))?;

    if removed.is_empty() {
        removed.push("No metadata files found".to_string());
    } else {
        removed.insert(
            0,
            "Document metadata (author, timestamps, edit history)".to_string(),
        );
    }

    Ok(removed)
}
