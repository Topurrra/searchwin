use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};

use tauri::{AppHandle, Emitter};

use image::{DynamicImage, ImageBuffer, ImageFormat, Rgba, RgbaImage};

static CANCELLED_IMAGE_EXTRA_OPERATIONS: LazyLock<Mutex<HashSet<String>>> =
    LazyLock::new(|| Mutex::new(HashSet::new()));

#[derive(Serialize, Clone)]
pub struct ImageExtraProgress {
    pub operation_id: String,
    pub tool: String,
    pub source_path: String,
    pub file_name: String,
    pub index: usize,
    pub total: usize,
    pub stage: String,
    pub progress: f32,
    pub message: Option<String>,
}

#[tauri::command]
pub fn cancel_image_extra_operation(operation_id: String) -> Result<(), String> {
    if operation_id.trim().is_empty() {
        return Ok(());
    }
    CANCELLED_IMAGE_EXTRA_OPERATIONS
        .lock()
        .map_err(|_| "Cancel registry is unavailable".to_string())?
        .insert(operation_id);
    Ok(())
}

fn is_cancelled(operation_id: &Option<String>) -> bool {
    let Some(id) = operation_id.as_ref() else {
        return false;
    };
    if id.trim().is_empty() {
        return false;
    }
    CANCELLED_IMAGE_EXTRA_OPERATIONS
        .lock()
        .map(|set| set.contains(id))
        .unwrap_or(false)
}

fn clear_cancel(operation_id: &Option<String>) {
    let Some(id) = operation_id.as_ref() else {
        return;
    };
    if let Ok(mut set) = CANCELLED_IMAGE_EXTRA_OPERATIONS.lock() {
        set.remove(id);
    }
}

fn emit_extra_progress(
    app: &AppHandle,
    operation_id: &Option<String>,
    tool: &str,
    source_path: &str,
    index: usize,
    total: usize,
    stage: &str,
    progress: f32,
    message: Option<String>,
) {
    let Some(id) = operation_id.as_ref() else {
        return;
    };
    let _ = app.emit(
        "image-extra-progress",
        ImageExtraProgress {
            operation_id: id.clone(),
            tool: tool.to_string(),
            source_path: source_path.to_string(),
            file_name: file_name(Path::new(source_path)),
            index,
            total,
            stage: stage.to_string(),
            progress: progress.clamp(0.0, 100.0),
            message,
        },
    );
}

// ============================================================
// Shared helpers
// ============================================================

fn file_name(path: &Path) -> String {
    path.file_name()
        .and_then(|v| v.to_str())
        .unwrap_or("image")
        .to_string()
}

fn file_stem(path: &Path) -> String {
    path.file_stem()
        .and_then(|v| v.to_str())
        .unwrap_or("image")
        .to_string()
}

fn ext_lower(path: &Path) -> String {
    path.extension()
        .and_then(|v| v.to_str())
        .unwrap_or("png")
        .to_lowercase()
}

fn ensure_dir(path: &Path) -> Result<(), String> {
    fs::create_dir_all(path).map_err(|e| format!("Failed to create directory: {e}"))
}

fn output_path(
    source: &Path,
    output_dir: Option<&str>,
    suffix: &str,
    ext: Option<&str>,
) -> PathBuf {
    let stem = file_stem(source);
    let out_ext =
        ext.unwrap_or_else(|| source.extension().and_then(|e| e.to_str()).unwrap_or("png"));
    let name = format!("{stem}{suffix}.{out_ext}");

    match output_dir {
        Some(dir) if !dir.trim().is_empty() => PathBuf::from(dir).join(name),
        _ => source.with_file_name(name),
    }
}

fn load_image(path: &Path) -> Result<DynamicImage, String> {
    image::ImageReader::open(path)
        .map_err(|e| format!("Cannot open image: {e}"))?
        .with_guessed_format()
        .map_err(|e| format!("Cannot detect image format: {e}"))?
        .decode()
        .map_err(|e| format!("Cannot decode image: {e}"))
}

fn save_image(img: &DynamicImage, path: &Path, quality: u8) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        ensure_dir(parent)?;
    }

    let ext = ext_lower(path);
    match ext.as_str() {
        "jpg" | "jpeg" => {
            let rgb = img.to_rgb8();
            let mut out = fs::File::create(path).map_err(|e| format!("Create failed: {e}"))?;
            let mut encoder =
                image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality.clamp(1, 100));
            encoder
                .encode_image(&rgb)
                .map_err(|e| format!("JPEG encode failed: {e}"))
        }
        "png" => img
            .save_with_format(path, ImageFormat::Png)
            .map_err(|e| format!("PNG save failed: {e}")),
        "webp" => img
            .save_with_format(path, ImageFormat::WebP)
            .map_err(|e| format!("WebP save failed: {e}")),
        "bmp" => img
            .save_with_format(path, ImageFormat::Bmp)
            .map_err(|e| format!("BMP save failed: {e}")),
        "tif" | "tiff" => img
            .save_with_format(path, ImageFormat::Tiff)
            .map_err(|e| format!("TIFF save failed: {e}")),
        _ => Err(format!("Unsupported output format: .{ext}")),
    }
}

fn parse_hex_color(input: &str, fallback: [u8; 4]) -> [u8; 4] {
    let s = input.trim().trim_start_matches('#');
    if s.len() != 6 && s.len() != 8 {
        return fallback;
    }

    let r = u8::from_str_radix(&s[0..2], 16).unwrap_or(fallback[0]);
    let g = u8::from_str_radix(&s[2..4], 16).unwrap_or(fallback[1]);
    let b = u8::from_str_radix(&s[4..6], 16).unwrap_or(fallback[2]);
    let a = if s.len() == 8 {
        u8::from_str_radix(&s[6..8], 16).unwrap_or(fallback[3])
    } else {
        fallback[3]
    };
    [r, g, b, a]
}

fn overlay_with_opacity(base: &mut RgbaImage, overlay: &RgbaImage, x: i64, y: i64, opacity: f32) {
    let opacity = opacity.clamp(0.0, 1.0);
    let bw = base.width() as i64;
    let bh = base.height() as i64;

    for oy in 0..overlay.height() as i64 {
        for ox in 0..overlay.width() as i64 {
            let bx = x + ox;
            let by = y + oy;
            if bx < 0 || by < 0 || bx >= bw || by >= bh {
                continue;
            }

            let src = overlay.get_pixel(ox as u32, oy as u32).0;
            let src_a = (src[3] as f32 / 255.0) * opacity;
            if src_a <= 0.0 {
                continue;
            }

            let dst = base.get_pixel(bx as u32, by as u32).0;
            let inv = 1.0 - src_a;
            let out = [
                (src[0] as f32 * src_a + dst[0] as f32 * inv).round() as u8,
                (src[1] as f32 * src_a + dst[1] as f32 * inv).round() as u8,
                (src[2] as f32 * src_a + dst[2] as f32 * inv).round() as u8,
                ((src_a + (dst[3] as f32 / 255.0) * inv) * 255.0)
                    .round()
                    .clamp(0.0, 255.0) as u8,
            ];
            base.put_pixel(bx as u32, by as u32, Rgba(out));
        }
    }
}

// ============================================================
// Favicon Generator
// ============================================================

#[derive(Deserialize)]
pub struct FaviconOptions {
    pub source_path: String,
    pub output_dir: Option<String>,
    pub app_name: Option<String>,
    pub background: String,
    pub transparent: bool,
    pub padding_percent: u32,
    pub sizes: Vec<u32>,
    pub make_ico: bool,
    pub make_png: bool,
    pub operation_id: Option<String>,
}

#[derive(Serialize, Clone)]
pub struct FaviconOutput {
    pub path: String,
    pub size: u32,
    pub kind: String,
}

#[derive(Serialize, Clone)]
pub struct FaviconResult {
    pub source_path: String,
    pub output_dir: String,
    pub outputs: Vec<FaviconOutput>,
    pub success: bool,
    pub error: Option<String>,
}

#[tauri::command]
pub async fn generate_favicons(app: AppHandle, options: FaviconOptions) -> FaviconResult {
    // Security gate: source image must be real + non-system; output_dir
    // (if specified) must not target a forbidden location.
    if let Err(e) = crate::core::safe_path::validate_user_path(&options.source_path) {
        return FaviconResult {
            source_path: options.source_path.clone(),
            output_dir: String::new(),
            outputs: vec![],
            success: false,
            error: Some(e),
        };
    }
    if let Some(ref dir) = options.output_dir {
        if let Err(e) = crate::core::safe_path::forbid_system_path(dir) {
            return FaviconResult {
                source_path: options.source_path.clone(),
                output_dir: dir.clone(),
                outputs: vec![],
                success: false,
                error: Some(e),
            };
        }
    }

    tauri::async_runtime::spawn_blocking(move || generate_favicons_blocking(app, options))
        .await
        .unwrap_or_else(|e| FaviconResult {
            source_path: String::new(),
            output_dir: String::new(),
            outputs: vec![],
            success: false,
            error: Some(format!("Favicon worker failed: {e}")),
        })
}

fn generate_favicons_blocking(app: AppHandle, options: FaviconOptions) -> FaviconResult {
    let src = PathBuf::from(&options.source_path);
    let source_path = options.source_path.clone();
    if is_cancelled(&options.operation_id) {
        clear_cancel(&options.operation_id);
        return FaviconResult {
            source_path,
            output_dir: String::new(),
            outputs: vec![],
            success: false,
            error: Some("Cancelled".into()),
        };
    }
    emit_extra_progress(
        &app,
        &options.operation_id,
        "favicon",
        &source_path,
        0,
        1,
        "loading",
        5.0,
        Some("Loading source image".into()),
    );
    let base = match load_image(&src) {
        Ok(v) => v.to_rgba8(),
        Err(e) => {
            return FaviconResult {
                source_path,
                output_dir: String::new(),
                outputs: vec![],
                success: false,
                error: Some(e),
            };
        }
    };

    let app_name = options.app_name.unwrap_or_else(|| file_stem(&src));
    let safe_name = app_name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_lowercase();

    let root = match options.output_dir {
        Some(dir) if !dir.trim().is_empty() => PathBuf::from(dir),
        _ => src
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(format!("{safe_name}-favicons")),
    };

    if let Err(e) = ensure_dir(&root) {
        return FaviconResult {
            source_path,
            output_dir: root.to_string_lossy().to_string(),
            outputs: vec![],
            success: false,
            error: Some(e),
        };
    }

    let mut sizes = options.sizes;
    if sizes.is_empty() {
        sizes = vec![16, 32, 48, 64, 128, 180, 192, 256, 512];
    }
    sizes.sort_unstable();
    sizes.dedup();
    sizes.retain(|s| (16..=1024).contains(s));

    let bg = parse_hex_color(&options.background, [255, 255, 255, 255]);
    let mut outputs = Vec::new();
    let mut ico_dir = ico::IconDir::new(ico::ResourceType::Icon);
    let total_steps = sizes.len().max(1);

    for (size_index, size) in sizes.into_iter().enumerate() {
        if is_cancelled(&options.operation_id) {
            clear_cancel(&options.operation_id);
            return FaviconResult {
                source_path,
                output_dir: root.to_string_lossy().to_string(),
                outputs,
                success: false,
                error: Some("Cancelled".into()),
            };
        }
        emit_extra_progress(
            &app,
            &options.operation_id,
            "favicon",
            &source_path,
            size_index,
            total_steps,
            "rendering",
            10.0 + (size_index as f32 / total_steps as f32) * 75.0,
            Some(format!("Rendering {size}×{size}")),
        );
        let canvas = render_square_icon(
            &base,
            size,
            options.padding_percent.clamp(0, 45),
            if options.transparent {
                [0, 0, 0, 0]
            } else {
                bg
            },
        );

        if options.make_png {
            let name = match size {
                180 => "apple-touch-icon.png".to_string(),
                192 => "android-chrome-192x192.png".to_string(),
                512 => "android-chrome-512x512.png".to_string(),
                32 => "favicon-32x32.png".to_string(),
                16 => "favicon-16x16.png".to_string(),
                _ => format!("favicon-{size}x{size}.png"),
            };
            let path = root.join(name);
            if let Err(e) =
                DynamicImage::ImageRgba8(canvas.clone()).save_with_format(&path, ImageFormat::Png)
            {
                return FaviconResult {
                    source_path,
                    output_dir: root.to_string_lossy().to_string(),
                    outputs,
                    success: false,
                    error: Some(format!("PNG save failed: {e}")),
                };
            }
            outputs.push(FaviconOutput {
                path: path.to_string_lossy().to_string(),
                size,
                kind: "png".into(),
            });
        }

        if options.make_ico && [16, 24, 32, 48, 64, 128, 256].contains(&size) {
            let icon = match ico::IconImage::from_rgba_data(size, size, canvas.into_raw()) {
                img => img,
            };
            match ico::IconDirEntry::encode(&icon) {
                Ok(entry) => ico_dir.add_entry(entry),
                Err(e) => {
                    return FaviconResult {
                        source_path,
                        output_dir: root.to_string_lossy().to_string(),
                        outputs,
                        success: false,
                        error: Some(format!("ICO encode failed: {e}")),
                    }
                }
            }
        }
    }

    if options.make_ico {
        if is_cancelled(&options.operation_id) {
            clear_cancel(&options.operation_id);
            return FaviconResult {
                source_path,
                output_dir: root.to_string_lossy().to_string(),
                outputs,
                success: false,
                error: Some("Cancelled".into()),
            };
        }
        emit_extra_progress(
            &app,
            &options.operation_id,
            "favicon",
            &source_path,
            total_steps,
            total_steps,
            "ico",
            90.0,
            Some("Writing favicon.ico".into()),
        );
        let path = root.join("favicon.ico");
        match fs::File::create(&path) {
            Ok(mut file) => {
                if let Err(e) = ico_dir.write(&mut file) {
                    return FaviconResult {
                        source_path,
                        output_dir: root.to_string_lossy().to_string(),
                        outputs,
                        success: false,
                        error: Some(format!("ICO write failed: {e}")),
                    };
                }
                outputs.push(FaviconOutput {
                    path: path.to_string_lossy().to_string(),
                    size: 0,
                    kind: "ico".into(),
                });
            }
            Err(e) => {
                return FaviconResult {
                    source_path,
                    output_dir: root.to_string_lossy().to_string(),
                    outputs,
                    success: false,
                    error: Some(format!("ICO create failed: {e}")),
                }
            }
        }
    }

    let manifest = root.join("site.webmanifest");
    let manifest_text = format!(
        "{{\n  \"name\": \"{}\",\n  \"short_name\": \"{}\",\n  \"icons\": [\n    {{ \"src\": \"/android-chrome-192x192.png\", \"sizes\": \"192x192\", \"type\": \"image/png\" }},\n    {{ \"src\": \"/android-chrome-512x512.png\", \"sizes\": \"512x512\", \"type\": \"image/png\" }}\n  ],\n  \"theme_color\": \"{}\",\n  \"background_color\": \"{}\",\n  \"display\": \"standalone\"\n}}\n",
        escape_json(&safe_name), escape_json(&safe_name), options.background, options.background
    );
    if fs::write(&manifest, manifest_text).is_ok() {
        outputs.push(FaviconOutput {
            path: manifest.to_string_lossy().to_string(),
            size: 0,
            kind: "manifest".into(),
        });
    }

    emit_extra_progress(
        &app,
        &options.operation_id,
        "favicon",
        &source_path,
        total_steps,
        total_steps,
        "done",
        100.0,
        Some("Favicons generated".into()),
    );
    clear_cancel(&options.operation_id);
    FaviconResult {
        source_path,
        output_dir: root.to_string_lossy().to_string(),
        outputs,
        success: true,
        error: None,
    }
}

fn escape_json(input: &str) -> String {
    input.replace('\\', "\\\\").replace('"', "\\\"")
}

fn render_square_icon(src: &RgbaImage, size: u32, padding_percent: u32, bg: [u8; 4]) -> RgbaImage {
    let mut canvas = ImageBuffer::from_pixel(size, size, Rgba(bg));
    let pad = ((size as f32) * (padding_percent as f32 / 100.0)).round() as u32;
    let target = size.saturating_sub(pad * 2).max(1);
    let (w, h) = src.dimensions();
    let scale = (target as f32 / w as f32).min(target as f32 / h as f32);
    let nw = (w as f32 * scale).round().max(1.0) as u32;
    let nh = (h as f32 * scale).round().max(1.0) as u32;
    let resized = image::imageops::resize(src, nw, nh, image::imageops::FilterType::Lanczos3);
    let x = ((size - nw) / 2) as i64;
    let y = ((size - nh) / 2) as i64;
    image::imageops::overlay(&mut canvas, &resized, x, y);
    canvas
}

// ============================================================
// Watermark
// ============================================================

#[derive(Deserialize, Clone)]
pub struct WatermarkOptions {
    pub paths: Vec<String>,
    pub output_dir: Option<String>,
    pub suffix: String,
    pub mode: String, // "text" | "image"
    pub text: Option<String>,
    pub watermark_path: Option<String>,
    pub position: String, // top-left, top-right, bottom-left, bottom-right, center
    pub tiled: bool,
    pub opacity: f32,
    pub scale_percent: u32,
    pub margin: u32,
    pub color: String,
    pub quality: u8,
    pub operation_id: Option<String>,
}

#[derive(Serialize, Clone)]
pub struct WatermarkResult {
    pub source_path: String,
    pub output_path: String,
    pub original_size: u64,
    pub new_size: u64,
    pub dimensions: (u32, u32),
    pub success: bool,
    pub error: Option<String>,
}

#[tauri::command]
pub async fn watermark_images(app: AppHandle, options: WatermarkOptions) -> Vec<WatermarkResult> {
    // Security gate: every source image AND the optional watermark image
    // must resolve to a real, non-system path; output_dir must not land
    // in a forbidden location.
    let reject = |err: String| -> Vec<WatermarkResult> {
        options
            .paths
            .iter()
            .map(|p| WatermarkResult {
                source_path: p.clone(),
                output_path: String::new(),
                original_size: 0,
                new_size: 0,
                dimensions: (0, 0),
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
    if let Some(ref wm) = options.watermark_path {
        if !wm.is_empty() {
            if let Err(e) = crate::core::safe_path::validate_user_path(wm) {
                return reject(e);
            }
        }
    }
    if let Some(ref dir) = options.output_dir {
        if let Err(e) = crate::core::safe_path::forbid_system_path(dir) {
            return reject(e);
        }
    }

    tauri::async_runtime::spawn_blocking(move || watermark_images_blocking(app, options))
        .await
        .unwrap_or_else(|e| {
            vec![WatermarkResult {
                source_path: String::new(),
                output_path: String::new(),
                original_size: 0,
                new_size: 0,
                dimensions: (0, 0),
                success: false,
                error: Some(format!("Watermark worker failed: {e}")),
            }]
        })
}

fn watermark_images_blocking(app: AppHandle, options: WatermarkOptions) -> Vec<WatermarkResult> {
    let total = options.paths.len().max(1);
    let mut results = Vec::with_capacity(options.paths.len());

    for (index, path) in options.paths.iter().enumerate() {
        if is_cancelled(&options.operation_id) {
            results.push(WatermarkResult {
                source_path: path.clone(),
                output_path: String::new(),
                original_size: 0,
                new_size: 0,
                dimensions: (0, 0),
                success: false,
                error: Some("Cancelled".into()),
            });
            break;
        }
        emit_extra_progress(
            &app,
            &options.operation_id,
            "watermark",
            path,
            index,
            total,
            "processing",
            (index as f32 / total as f32) * 100.0,
            Some("Applying watermark".into()),
        );
        results.push(watermark_one(path, &options));
        emit_extra_progress(
            &app,
            &options.operation_id,
            "watermark",
            path,
            index + 1,
            total,
            "done",
            ((index + 1) as f32 / total as f32) * 100.0,
            Some("Watermark complete".into()),
        );
    }

    clear_cancel(&options.operation_id);
    results
}

fn watermark_one(path: &str, options: &WatermarkOptions) -> WatermarkResult {
    let src = PathBuf::from(path);
    let original_size = fs::metadata(&src).map(|m| m.len()).unwrap_or(0);
    let out = output_path(&src, options.output_dir.as_deref(), &options.suffix, None);

    if is_cancelled(&options.operation_id) {
        return wm_error(path, original_size, "Cancelled".into());
    }

    let img = match load_image(&src) {
        Ok(v) => v,
        Err(e) => return wm_error(path, original_size, e),
    };

    let mut base = img.to_rgba8();
    let dims = base.dimensions();
    let opacity = (options.opacity / 100.0).clamp(0.0, 1.0);

    let watermark = if options.mode == "image" {
        let wm_path = match &options.watermark_path {
            Some(v) if !v.trim().is_empty() => PathBuf::from(v),
            _ => return wm_error(path, original_size, "Select a watermark image".into()),
        };
        let wm = match load_image(&wm_path) {
            Ok(v) => v.to_rgba8(),
            Err(e) => return wm_error(path, original_size, e),
        };
        scale_watermark(&wm, dims.0, options.scale_percent.clamp(1, 100))
    } else {
        let text = options.text.clone().unwrap_or_default();
        if text.trim().is_empty() {
            return wm_error(path, original_size, "Enter watermark text".into());
        }
        render_text_watermark(&text, options.scale_percent.clamp(1, 100), &options.color)
    };

    if is_cancelled(&options.operation_id) {
        return wm_error(path, original_size, "Cancelled".into());
    }

    if options.tiled {
        apply_tiled(&mut base, &watermark, options.margin, opacity);
    } else {
        let (x, y) = position_xy(
            dims,
            watermark.dimensions(),
            options.margin,
            &options.position,
        );
        overlay_with_opacity(&mut base, &watermark, x, y, opacity);
    }

    let dyn_img = DynamicImage::ImageRgba8(base);
    if let Err(e) = save_image(&dyn_img, &out, options.quality) {
        return wm_error(path, original_size, e);
    }

    let new_size = fs::metadata(&out).map(|m| m.len()).unwrap_or(0);
    WatermarkResult {
        source_path: path.into(),
        output_path: out.to_string_lossy().to_string(),
        original_size,
        new_size,
        dimensions: dims,
        success: true,
        error: None,
    }
}

fn wm_error(path: &str, original_size: u64, error: String) -> WatermarkResult {
    WatermarkResult {
        source_path: path.into(),
        output_path: String::new(),
        original_size,
        new_size: 0,
        dimensions: (0, 0),
        success: false,
        error: Some(error),
    }
}

fn scale_watermark(wm: &RgbaImage, base_width: u32, scale_percent: u32) -> RgbaImage {
    let target_w = ((base_width as f32) * (scale_percent as f32 / 100.0))
        .round()
        .max(1.0) as u32;
    let ratio = target_w as f32 / wm.width().max(1) as f32;
    let target_h = (wm.height() as f32 * ratio).round().max(1.0) as u32;
    image::imageops::resize(
        wm,
        target_w,
        target_h,
        image::imageops::FilterType::Lanczos3,
    )
}

fn position_xy(base: (u32, u32), wm: (u32, u32), margin: u32, position: &str) -> (i64, i64) {
    let bw = base.0 as i64;
    let bh = base.1 as i64;
    let ww = wm.0 as i64;
    let wh = wm.1 as i64;
    let m = margin as i64;

    match position {
        "top-left" => (m, m),
        "top-right" => (bw - ww - m, m),
        "bottom-left" => (m, bh - wh - m),
        "center" => ((bw - ww) / 2, (bh - wh) / 2),
        _ => (bw - ww - m, bh - wh - m),
    }
}

fn apply_tiled(base: &mut RgbaImage, wm: &RgbaImage, margin: u32, opacity: f32) {
    let step_x = (wm.width() + margin.max(16) * 3).max(1) as i64;
    let step_y = (wm.height() + margin.max(16) * 3).max(1) as i64;
    let start_x = -(wm.width() as i64 / 2);
    let start_y = -(wm.height() as i64 / 2);

    let mut y = start_y;
    while y < base.height() as i64 {
        let mut x = start_x;
        while x < base.width() as i64 {
            overlay_with_opacity(base, wm, x, y, opacity);
            x += step_x;
        }
        y += step_y;
    }
}

fn render_text_watermark(text: &str, _scale_percent: u32, color: &str) -> RgbaImage {
    use ab_glyph::{Font, ScaleFont};
    let rgba = parse_hex_color(color, [255, 255, 255, 255]);
    let trimmed: String = text.trim().chars().take(200).collect();
    let text = if trimmed.is_empty() { "WATERMARK" } else { trimmed.as_str() };

    let Some(font) = load_watermark_font() else {
        // No readable system font — emit a transparent 1px image; the caller still
        // composites (a no-op overlay) rather than the whole watermark erroring.
        return RgbaImage::new(1, 1);
    };

    // Render at a fixed, crisp size; `scale_watermark()` then fits it to the image,
    // so the rasterized text stays sharp regardless of the chosen scale.
    let px = ab_glyph::PxScale::from(96.0);
    let scaled = font.as_scaled(px);
    let ascent = scaled.ascent();
    let line_h = (scaled.ascent() - scaled.descent()).ceil().max(1.0);

    // Lay the glyphs out on one baseline, honouring advance width + kerning.
    let mut pen_x = 0.0f32;
    let mut prev: Option<ab_glyph::GlyphId> = None;
    let mut placed: Vec<ab_glyph::Glyph> = Vec::new();
    for ch in text.chars() {
        let id = font.glyph_id(ch);
        if let Some(p) = prev {
            pen_x += scaled.kern(p, id);
        }
        placed.push(id.with_scale_and_position(px, ab_glyph::point(pen_x, ascent)));
        pen_x += scaled.h_advance(id);
        prev = Some(id);
    }

    let pad = (px.y * 0.18).ceil() as i32;
    let width = (pen_x.ceil() as i32 + pad * 2).max(1) as u32;
    let height = (line_h as i32 + pad * 2).max(1) as u32;
    let mut img = ImageBuffer::from_pixel(width, height, Rgba([0, 0, 0, 0]));

    for glyph in placed {
        if let Some(outline) = font.outline_glyph(glyph) {
            let bounds = outline.px_bounds();
            outline.draw(|gx, gy, coverage| {
                if coverage <= 0.0 {
                    return;
                }
                let x = bounds.min.x as i32 + gx as i32 + pad;
                let y = bounds.min.y as i32 + gy as i32 + pad;
                if x < 0 || y < 0 || x as u32 >= width || y as u32 >= height {
                    return;
                }
                let alpha = (coverage * rgba[3] as f32).round().clamp(0.0, 255.0) as u8;
                let cur = img.get_pixel_mut(x as u32, y as u32);
                // Glyphs can overlap when kerned negative — keep the strongest coverage.
                if alpha > cur[3] {
                    *cur = Rgba([rgba[0], rgba[1], rgba[2], alpha]);
                }
            });
        }
    }
    img
}

/// Load a TrueType/OpenType font for the text watermark. Prefers Windows system
/// fonts — broad Latin/Cyrillic/Greek coverage incl. lowercase, always present on
/// the target OS, nothing to bundle. Returns `None` if none can be read (caller
/// then skips text rendering). Replaces the old 5x7 ASCII-uppercase bitmap.
fn load_watermark_font() -> Option<ab_glyph::FontVec> {
    const CANDIDATES: &[&str] = &[
        r"C:\Windows\Fonts\segoeui.ttf",
        r"C:\Windows\Fonts\arial.ttf",
        r"C:\Windows\Fonts\tahoma.ttf",
        r"C:\Windows\Fonts\verdana.ttf",
        r"C:\Windows\Fonts\calibri.ttf",
    ];
    for path in CANDIDATES {
        if let Ok(bytes) = std::fs::read(path) {
            if let Ok(font) = ab_glyph::FontVec::try_from_vec(bytes) {
                return Some(font);
            }
        }
    }
    None
}
