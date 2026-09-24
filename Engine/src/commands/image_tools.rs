use base64::{engine::general_purpose::STANDARD, Engine as _};
use image::{imageops::FilterType, DynamicImage, ImageDecoder, ImageFormat, ImageReader};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::fs;
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

const IMAGE_PROGRESS_EVENT: &str = "image-progress";

static CANCELLED_IMAGE_OPERATIONS: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();

fn cancelled_ops() -> &'static Mutex<HashSet<String>> {
    CANCELLED_IMAGE_OPERATIONS.get_or_init(|| Mutex::new(HashSet::new()))
}

fn reset_cancel(operation_id: &str) {
    if let Ok(mut set) = cancelled_ops().lock() {
        set.remove(operation_id);
    }
}

fn request_cancel(operation_id: &str) {
    if let Ok(mut set) = cancelled_ops().lock() {
        set.insert(operation_id.to_string());
    }
}

fn is_cancelled(operation_id: &str) -> bool {
    cancelled_ops()
        .lock()
        .map(|set| set.contains(operation_id))
        .unwrap_or(false)
}

#[tauri::command]
pub fn cancel_image_operation(operation_id: String) -> Result<(), String> {
    if operation_id.trim().is_empty() {
        return Err("Missing image operation id".into());
    }
    request_cancel(&operation_id);
    Ok(())
}

// ===================== SHARED TYPES =====================

#[derive(Serialize, Clone)]
pub struct ImageResult {
    pub source_path: String,
    pub output_path: String,
    pub original_size: u64,
    pub new_size: u64,
    pub original_dims: (u32, u32),
    pub new_dims: (u32, u32),
    pub success: bool,
    pub error: Option<String>,
}

#[derive(Serialize, Clone)]
pub struct ImageProgress {
    pub operation_id: String,
    pub tool: String,
    pub source_path: String,
    pub file_name: String,
    pub index: usize,
    pub total: usize,
    pub stage: String,
    pub progress: u8,
    pub message: Option<String>,
}

fn emit_progress(
    app: &AppHandle,
    operation_id: &str,
    tool: &str,
    source_path: &str,
    index: usize,
    total: usize,
    stage: &str,
    progress: u8,
    message: Option<String>,
) {
    let file_name = Path::new(source_path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(source_path)
        .to_string();

    let _ = app.emit(
        IMAGE_PROGRESS_EVENT,
        ImageProgress {
            operation_id: operation_id.to_string(),
            tool: tool.to_string(),
            source_path: source_path.to_string(),
            file_name,
            index,
            total,
            stage: stage.to_string(),
            progress: progress.min(100),
            message,
        },
    );
}

fn operation_id(value: &Option<String>) -> String {
    value.clone().unwrap_or_else(|| "default".to_string())
}

fn fail_result(
    source_path: String,
    original_size: u64,
    original_dims: (u32, u32),
    error: String,
) -> ImageResult {
    ImageResult {
        source_path,
        output_path: String::new(),
        original_size,
        new_size: 0,
        original_dims,
        new_dims: (0, 0),
        success: false,
        error: Some(error),
    }
}

fn validated_image_source(path: &str) -> Result<PathBuf, String> {
    crate::core::safe_path::validate_user_path(path)
}

fn fail_source_validation(
    app: &AppHandle,
    operation_id: &str,
    tool: &str,
    source_path: String,
    index: usize,
    total: usize,
    error: String,
) -> ImageResult {
    emit_progress(
        app,
        operation_id,
        tool,
        &source_path,
        index,
        total,
        "error",
        100,
        Some(error.clone()),
    );
    fail_result(source_path, 0, (0, 0), error)
}

fn fail_output_path(
    app: &AppHandle,
    operation_id: &str,
    tool: &str,
    source_path: String,
    index: usize,
    total: usize,
    original_size: u64,
    error: String,
) -> ImageResult {
    emit_progress(
        app,
        operation_id,
        tool,
        &source_path,
        index,
        total,
        "error",
        100,
        Some(error.clone()),
    );
    fail_result(source_path, original_size, (0, 0), error)
}

fn build_output_path(
    source: &Path,
    output_dir: Option<&str>,
    suffix: &str,
    new_ext: Option<&str>,
) -> Result<PathBuf, String> {
    validate_output_component(suffix, "suffix")?;
    let stem = source
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("image");
    let ext =
        new_ext.unwrap_or_else(|| source.extension().and_then(|e| e.to_str()).unwrap_or("png"));
    validate_output_component(ext, "extension")?;
    let name = format!("{}{}.{}", stem, suffix, ext.trim_start_matches('.'));

    let output = match output_dir {
        Some(dir) if !dir.trim().is_empty() => PathBuf::from(dir).join(name),
        _ => source.with_file_name(name),
    };
    if output == source {
        return Err("Output path would overwrite the source image".into());
    }

    // The frontend may choose a new output directory. Create it before the
    // write-target check, then keep that check's canonical parent path so a
    // directory symlink cannot redirect the encoder elsewhere.
    ensure_parent_dir(&output)?;
    let output = crate::core::safe_path::validate_user_write_target(&output)?;
    match fs::symlink_metadata(&output) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err("Output path must not be an existing symbolic link".into());
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("Cannot inspect output path: {}", error)),
    }
    let same_file = match (source.canonicalize(), output.canonicalize()) {
        (Ok(source), Ok(output)) => source == output,
        _ => false,
    };
    if same_file {
        return Err("Output path would overwrite the source image".into());
    }

    Ok(output)
}

fn validate_output_component(value: &str, label: &str) -> Result<(), String> {
    let has_path_component = value.contains('/')
        || value.contains('\\')
        || value.contains(':')
        || Path::new(value)
            .components()
            .any(|component| !matches!(component, Component::Normal(_)));
    if has_path_component {
        return Err(format!(
            "Image output {label} must not contain path components"
        ));
    }
    Ok(())
}

/// Image Studio: resolve the desired output extension for a "keep or convert"
/// pass. Empty or "keep" ⇒ `None` (keep the source's own extension); otherwise
/// the normalized lowercase extension (e.g. "webp"), which `save_image` maps to
/// the right encoder.
fn resolve_output_ext(output_format: &str) -> Option<String> {
    let f = output_format.trim().trim_start_matches('.').to_lowercase();
    if f.is_empty() || f == "keep" {
        None
    } else {
        Some(f)
    }
}

fn crop_output_ext(source: &Path, output_format: &str) -> Option<String> {
    if let Some(ext) = resolve_output_ext(output_format) {
        return Some(ext);
    }

    let is_svg = source
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("svg"));
    is_svg.then(|| "png".to_string())
}

fn is_gif(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("gif"))
}

fn ensure_parent_dir(path: &Path) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("Create output directory failed: {}", e))?;
    }
    Ok(())
}

fn load_image(path: &Path) -> Result<DynamicImage, String> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_lowercase())
        .unwrap_or_default();

    if ext == "svg" {
        return load_svg(path);
    }

    let mut decoder = ImageReader::open(path)
        .map_err(|e| format!("Cannot open: {}", e))?
        .with_guessed_format()
        .map_err(|e| format!("Cannot detect format: {}", e))?
        .into_decoder()
        .map_err(|e| format!("Cannot create decoder: {}", e))?;
    let orientation = decoder
        .orientation()
        .map_err(|e| format!("Cannot read EXIF orientation: {}", e))?;
    let mut image =
        DynamicImage::from_decoder(decoder).map_err(|e| format!("Decode failed: {}", e))?;
    image.apply_orientation(orientation);
    Ok(image)
}

fn load_svg(path: &Path) -> Result<DynamicImage, String> {
    use resvg::tiny_skia::{Pixmap, Transform};
    use resvg::usvg::{Options, Tree};

    let data = fs::read(path).map_err(|e| format!("Read SVG failed: {}", e))?;
    let opts = Options::default();
    let tree = Tree::from_data(&data, &opts).map_err(|e| format!("SVG parse failed: {}", e))?;

    let size = tree.size();
    let w = size.width() as u32;
    let h = size.height() as u32;

    if w == 0 || h == 0 {
        return Err("SVG has zero dimensions".into());
    }

    let mut pixmap = Pixmap::new(w, h).ok_or_else(|| "Cannot allocate pixmap".to_string())?;
    resvg::render(&tree, Transform::identity(), &mut pixmap.as_mut());

    let rgba = image::RgbaImage::from_raw(w, h, pixmap.take())
        .ok_or_else(|| "Cannot convert pixmap to image".to_string())?;

    Ok(DynamicImage::ImageRgba8(rgba))
}

fn ext_to_format(ext: &str) -> Option<ImageFormat> {
    match ext.to_lowercase().as_str() {
        "jpg" | "jpeg" => Some(ImageFormat::Jpeg),
        "png" => Some(ImageFormat::Png),
        "webp" => Some(ImageFormat::WebP),
        "bmp" => Some(ImageFormat::Bmp),
        "tiff" | "tif" => Some(ImageFormat::Tiff),
        "ico" => Some(ImageFormat::Ico),
        "gif" => Some(ImageFormat::Gif),
        _ => None,
    }
}

fn save_image(img: &DynamicImage, path: &Path, quality: u8) -> Result<(), String> {
    ensure_parent_dir(path)?;

    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_lowercase())
        .unwrap_or_default();
    // AVIF (ravif/rav1e) — handled before the `image`-crate format lookup, since
    // ext_to_format() doesn't map "avif" and the `image` crate's AVIF feature isn't
    // enabled. Uses the same 1–100 quality scale as the other encoders.
    if ext == "avif" {
        let q = quality.clamp(1, 100);
        let rgba = img.to_rgba8();
        let (w, h) = (rgba.width() as usize, rgba.height() as usize);
        let pixels: Vec<ravif::RGBA8> = rgba
            .pixels()
            .map(|p| ravif::RGBA8::new(p[0], p[1], p[2], p[3]))
            .collect();
        let encoded = ravif::Encoder::new()
            .with_quality(q as f32)
            .with_speed(6)
            .encode_rgba(ravif::Img::new(pixels.as_slice(), w, h))
            .map_err(|e| format!("AVIF encode: {}", e))?;
        fs::write(path, encoded.avif_file).map_err(|e| format!("AVIF write: {}", e))?;
        return Ok(());
    }

    let format =
        ext_to_format(&ext).ok_or_else(|| format!("Unsupported output format: .{}", ext))?;
    let quality = quality.clamp(1, 100);

    match format {
        ImageFormat::Jpeg => {
            // MozJPEG (libjpeg-turbo + mozjpeg's trellis quantization) — visibly
            // smaller files than the baseline `image` encoder at the same quality,
            // which is the whole point of a compress tool.
            let rgb = img.to_rgb8();
            let mut comp = mozjpeg::Compress::new(mozjpeg::ColorSpace::JCS_RGB);
            comp.set_size(rgb.width() as usize, rgb.height() as usize);
            comp.set_quality(quality as f32);
            let mut started = comp
                .start_compress(Vec::new())
                .map_err(|e| format!("JPEG start: {}", e))?;
            started
                .write_scanlines(rgb.as_raw())
                .map_err(|e| format!("JPEG encode: {}", e))?;
            let data = started
                .finish()
                .map_err(|e| format!("JPEG finish: {}", e))?;
            fs::write(path, data).map_err(|e| format!("JPEG write: {}", e))?;
        }
        ImageFormat::WebP => {
            // SB-2 (2026-05-29): proper LOSSY WebP via libwebp at the chosen
            // quality. The `image` crate only does lossless WebP — which
            // ignored the quality slider and routinely produced files BIGGER
            // than the source. libwebp's lossy encoder at q is what makes
            // WebP actually compress (often 25-35% under a JPEG of the same
            // visual quality).
            let encoder =
                webp::Encoder::from_image(img).map_err(|e| format!("WebP encode init: {}", e))?;
            let encoded = encoder.encode(quality as f32);
            fs::write(path, &*encoded).map_err(|e| format!("WebP write: {}", e))?;
        }
        ImageFormat::Png => {
            // SB-2 (2026-05-29): encode to memory, then run oxipng (pure-Rust
            // lossless optimizer) for real byte savings the plain `image`
            // deflate path leaves on the table. PNG is lossless, so `quality`
            // doesn't apply — we optimize structure/filters/deflate instead.
            let mut buf = Vec::new();
            img.write_to(&mut std::io::Cursor::new(&mut buf), ImageFormat::Png)
                .map_err(|e| format!("PNG encode: {}", e))?;
            match oxipng::optimize_from_memory(&buf, &oxipng::Options::from_preset(2)) {
                Ok(optimized) => {
                    // Guard: never let optimization make the file larger.
                    let best = if optimized.len() < buf.len() {
                        optimized
                    } else {
                        buf
                    };
                    fs::write(path, best).map_err(|e| format!("PNG write: {}", e))?;
                }
                // oxipng failed (corrupt input, etc.) — fall back to the
                // un-optimized but valid PNG rather than failing the file.
                Err(_) => {
                    fs::write(path, buf).map_err(|e| format!("PNG write: {}", e))?;
                }
            }
        }
        _ => {
            img.save_with_format(path, format)
                .map_err(|e| format!("Save failed: {}", e))?;
        }
    }

    Ok(())
}

fn filter_from_str(value: &str) -> FilterType {
    match value {
        "nearest" => FilterType::Nearest,
        "linear" => FilterType::Triangle,
        "cubic" => FilterType::CatmullRom,
        _ => FilterType::Lanczos3,
    }
}

fn crop_exact(
    image: &DynamicImage,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
) -> Result<DynamicImage, String> {
    if width == 0 || height == 0 {
        return Err("Crop width and height must be at least 1 pixel".into());
    }

    let (image_width, image_height) = (image.width(), image.height());
    if x >= image_width || y >= image_height || width > image_width - x || height > image_height - y
    {
        return Err("Crop rectangle must fit within the source image".into());
    }

    Ok(image.crop_imm(x, y, width, height))
}

#[cfg(feature = "image-background-removal")]
const U2NET_SIDE: u32 = 320;

#[cfg(feature = "image-background-removal")]
fn u2net_input(image: &DynamicImage) -> Vec<f32> {
    const MEAN: [f32; 3] = [0.485, 0.456, 0.406];
    const STD: [f32; 3] = [0.229, 0.224, 0.225];

    let rgb = image
        .resize_exact(U2NET_SIDE, U2NET_SIDE, FilterType::Lanczos3)
        .to_rgb8();
    let plane = (U2NET_SIDE * U2NET_SIDE) as usize;
    let mut input = vec![0.0; 3 * plane];

    for (index, pixel) in rgb.pixels().enumerate() {
        for channel in 0..3 {
            input[channel * plane + index] =
                (pixel[channel] as f32 / 255.0 - MEAN[channel]) / STD[channel];
        }
    }

    input
}

#[cfg(feature = "image-background-removal")]
fn normalize_u2net_mask(values: &[f32]) -> Result<Vec<u8>, String> {
    if values.is_empty() || values.iter().any(|value| !value.is_finite()) {
        return Err("U²-Net returned an invalid foreground mask".into());
    }

    let min = values.iter().copied().fold(f32::INFINITY, f32::min);
    let max = values.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    let range = max - min;
    if range <= f32::EPSILON {
        return Ok(vec![0; values.len()]);
    }

    Ok(values
        .iter()
        .map(|value| (((value - min) / range).clamp(0.0, 1.0) * 255.0).round() as u8)
        .collect())
}

#[cfg(feature = "image-background-removal")]
fn apply_alpha_mask(image: DynamicImage, alpha: &image::GrayImage) -> Result<DynamicImage, String> {
    let mut rgba = image.to_rgba8();
    if (rgba.width(), rgba.height()) != (alpha.width(), alpha.height()) {
        return Err("Background mask dimensions do not match the source image".into());
    }

    for (pixel, alpha) in rgba.pixels_mut().zip(alpha.pixels()) {
        pixel[3] = ((u16::from(pixel[3]) * u16::from(alpha[0]) + 127) / 255) as u8;
    }

    Ok(DynamicImage::ImageRgba8(rgba))
}

fn output_size(path: &Path) -> u64 {
    fs::metadata(path).map(|m| m.len()).unwrap_or(0)
}

// ===================== CONVERTER =====================

#[derive(Deserialize, Clone)]
pub struct ConvertImageOptions {
    pub paths: Vec<String>,
    pub output_format: String,
    pub output_dir: Option<String>,
    pub quality: u8,
    pub suffix: String,
    pub operation_id: Option<String>,
}

#[tauri::command]
pub async fn convert_images(app: AppHandle, options: ConvertImageOptions) -> Vec<ImageResult> {
    // Security gate: every input image must resolve to a real, non-system
    // path; the optional output_dir must not target a forbidden location.
    // Failures come back as per-source ImageResult errors so the frontend
    // renders them like any other rejected file.
    for path in &options.paths {
        if let Err(e) = crate::core::safe_path::validate_user_path(path) {
            return options
                .paths
                .iter()
                .map(|p| fail_result(p.clone(), 0, (0, 0), e.clone()))
                .collect();
        }
    }
    if let Some(ref dir) = options.output_dir {
        if let Err(e) = crate::core::safe_path::forbid_system_path(dir) {
            return options
                .paths
                .iter()
                .map(|p| fail_result(p.clone(), 0, (0, 0), e.clone()))
                .collect();
        }
    }

    tauri::async_runtime::spawn_blocking(move || convert_images_blocking(app, options))
        .await
        .unwrap_or_else(|e| {
            vec![fail_result(
                String::new(),
                0,
                (0, 0),
                format!("Image worker crashed: {}", e),
            )]
        })
}

fn convert_images_blocking(app: AppHandle, options: ConvertImageOptions) -> Vec<ImageResult> {
    let op = operation_id(&options.operation_id);
    reset_cancel(&op);
    let total = options.paths.len();
    let paths = options.paths.clone();
    let mut results = Vec::with_capacity(total);

    for (i, path) in paths.into_iter().enumerate() {
        if is_cancelled(&op) {
            emit_progress(
                &app,
                &op,
                "converter",
                &path,
                i,
                total,
                "cancelled",
                100,
                Some("Operation cancelled".into()),
            );
            break;
        }
        results.push(process_convert_one(&app, &op, i, total, path, &options));
    }

    reset_cancel(&op);
    results
}

fn process_convert_one(
    app: &AppHandle,
    op: &str,
    index: usize,
    total: usize,
    path: String,
    options: &ConvertImageOptions,
) -> ImageResult {
    let src = match validated_image_source(&path) {
        Ok(src) => src,
        Err(e) => return fail_source_validation(app, op, "converter", path, index, total, e),
    };
    let original_size = output_size(&src);
    let out = match build_output_path(
        &src,
        options.output_dir.as_deref(),
        &options.suffix,
        Some(&options.output_format),
    ) {
        Ok(out) => out,
        Err(e) => {
            return fail_output_path(app, op, "converter", path, index, total, original_size, e)
        }
    };

    emit_progress(
        app,
        op,
        "converter",
        &path,
        index,
        total,
        "loading",
        10,
        None,
    );
    if is_cancelled(op) {
        emit_progress(
            app,
            op,
            "converter",
            &path,
            index,
            total,
            "cancelled",
            100,
            Some("Operation cancelled".into()),
        );
        return fail_result(path, original_size, (0, 0), "Cancelled".into());
    }

    let img = match load_image(&src) {
        Ok(img) => img,
        Err(e) => {
            emit_progress(
                app,
                op,
                "converter",
                &path,
                index,
                total,
                "error",
                100,
                Some(e.clone()),
            );
            return fail_result(path, original_size, (0, 0), e);
        }
    };

    let original_dims = (img.width(), img.height());
    emit_progress(
        app,
        op,
        "converter",
        &path,
        index,
        total,
        "encoding",
        75,
        None,
    );
    if is_cancelled(op) {
        emit_progress(
            app,
            op,
            "converter",
            &path,
            index,
            total,
            "cancelled",
            100,
            Some("Operation cancelled".into()),
        );
        return fail_result(path, original_size, original_dims, "Cancelled".into());
    }

    if let Err(e) = save_image(&img, &out, options.quality) {
        emit_progress(
            app,
            op,
            "converter",
            &path,
            index,
            total,
            "error",
            100,
            Some(e.clone()),
        );
        return fail_result(path, original_size, original_dims, e);
    }

    emit_progress(app, op, "converter", &path, index, total, "done", 100, None);

    ImageResult {
        source_path: path,
        output_path: out.to_string_lossy().to_string(),
        original_size,
        new_size: output_size(&out),
        original_dims,
        new_dims: original_dims,
        success: true,
        error: None,
    }
}

// ===================== COMPRESSOR =====================

#[derive(Deserialize, Clone)]
pub struct CompressImageOptions {
    pub paths: Vec<String>,
    pub output_dir: Option<String>,
    pub quality: u8,
    pub suffix: String,
    pub max_width: Option<u32>,
    pub max_height: Option<u32>,
    pub operation_id: Option<String>,
}

#[tauri::command]
pub async fn compress_images(app: AppHandle, options: CompressImageOptions) -> Vec<ImageResult> {
    // Security gate: every input image must be real + non-system; output_dir
    // must not land in a forbidden location.
    for path in &options.paths {
        if let Err(e) = crate::core::safe_path::validate_user_path(path) {
            return options
                .paths
                .iter()
                .map(|p| fail_result(p.clone(), 0, (0, 0), e.clone()))
                .collect();
        }
    }
    if let Some(ref dir) = options.output_dir {
        if let Err(e) = crate::core::safe_path::forbid_system_path(dir) {
            return options
                .paths
                .iter()
                .map(|p| fail_result(p.clone(), 0, (0, 0), e.clone()))
                .collect();
        }
    }

    tauri::async_runtime::spawn_blocking(move || compress_images_blocking(app, options))
        .await
        .unwrap_or_else(|e| {
            vec![fail_result(
                String::new(),
                0,
                (0, 0),
                format!("Image worker crashed: {}", e),
            )]
        })
}

fn compress_images_blocking(app: AppHandle, options: CompressImageOptions) -> Vec<ImageResult> {
    let op = operation_id(&options.operation_id);
    reset_cancel(&op);
    let total = options.paths.len();
    let paths = options.paths.clone();
    let mut results = Vec::with_capacity(total);

    for (i, path) in paths.into_iter().enumerate() {
        if is_cancelled(&op) {
            emit_progress(
                &app,
                &op,
                "compressor",
                &path,
                i,
                total,
                "cancelled",
                100,
                Some("Operation cancelled".into()),
            );
            break;
        }
        results.push(process_compress_one(&app, &op, i, total, path, &options));
    }

    reset_cancel(&op);
    results
}

fn process_compress_one(
    app: &AppHandle,
    op: &str,
    index: usize,
    total: usize,
    path: String,
    options: &CompressImageOptions,
) -> ImageResult {
    let src = match validated_image_source(&path) {
        Ok(src) => src,
        Err(e) => return fail_source_validation(app, op, "compressor", path, index, total, e),
    };
    let original_size = output_size(&src);
    let ext = src
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_lowercase())
        .unwrap_or_else(|| "jpg".into());

    if ext == "svg" {
        let err = "SVG compression not supported. Convert to PNG/WebP first.".to_string();
        emit_progress(
            app,
            op,
            "compressor",
            &path,
            index,
            total,
            "error",
            100,
            Some(err.clone()),
        );
        return fail_result(path, original_size, (0, 0), err);
    }

    let out = match build_output_path(&src, options.output_dir.as_deref(), &options.suffix, None) {
        Ok(out) => out,
        Err(e) => {
            return fail_output_path(app, op, "compressor", path, index, total, original_size, e)
        }
    };
    emit_progress(
        app,
        op,
        "compressor",
        &path,
        index,
        total,
        "loading",
        10,
        None,
    );
    if is_cancelled(op) {
        emit_progress(
            app,
            op,
            "compressor",
            &path,
            index,
            total,
            "cancelled",
            100,
            Some("Operation cancelled".into()),
        );
        return fail_result(path, original_size, (0, 0), "Cancelled".into());
    }

    let mut img = match load_image(&src) {
        Ok(img) => img,
        Err(e) => {
            emit_progress(
                app,
                op,
                "compressor",
                &path,
                index,
                total,
                "error",
                100,
                Some(e.clone()),
            );
            return fail_result(path, original_size, (0, 0), e);
        }
    };

    let original_dims = (img.width(), img.height());
    let mut new_dims = original_dims;

    emit_progress(
        app,
        op,
        "compressor",
        &path,
        index,
        total,
        "processing",
        45,
        None,
    );

    if let (Some(mw), Some(mh)) = (options.max_width, options.max_height) {
        if img.width() > mw || img.height() > mh {
            img = img.resize(mw, mh, FilterType::Lanczos3);
            new_dims = (img.width(), img.height());
        }
    } else if let Some(mw) = options.max_width {
        if img.width() > mw {
            let ratio = mw as f32 / img.width() as f32;
            let mh = ((img.height() as f32 * ratio).round() as u32).max(1);
            img = img.resize(mw, mh, FilterType::Lanczos3);
            new_dims = (img.width(), img.height());
        }
    } else if let Some(mh) = options.max_height {
        if img.height() > mh {
            let ratio = mh as f32 / img.height() as f32;
            let mw = ((img.width() as f32 * ratio).round() as u32).max(1);
            img = img.resize(mw, mh, FilterType::Lanczos3);
            new_dims = (img.width(), img.height());
        }
    }

    emit_progress(
        app,
        op,
        "compressor",
        &path,
        index,
        total,
        "saving",
        85,
        None,
    );
    if is_cancelled(op) {
        emit_progress(
            app,
            op,
            "compressor",
            &path,
            index,
            total,
            "cancelled",
            100,
            Some("Operation cancelled".into()),
        );
        return fail_result(path, original_size, original_dims, "Cancelled".into());
    }

    if let Err(e) = save_image(&img, &out, options.quality) {
        emit_progress(
            app,
            op,
            "compressor",
            &path,
            index,
            total,
            "error",
            100,
            Some(e.clone()),
        );
        return fail_result(path, original_size, original_dims, e);
    }

    emit_progress(
        app,
        op,
        "compressor",
        &path,
        index,
        total,
        "done",
        100,
        None,
    );

    ImageResult {
        source_path: path,
        output_path: out.to_string_lossy().to_string(),
        original_size,
        new_size: output_size(&out),
        original_dims,
        new_dims,
        success: true,
        error: None,
    }
}

// ===================== RESIZER =====================

#[derive(Deserialize, Clone)]
pub struct ResizeImageOptions {
    pub paths: Vec<String>,
    pub output_dir: Option<String>,
    pub suffix: String,
    /// Image Studio: target output format ("" / "keep" ⇒ keep source extension).
    #[serde(default)]
    pub output_format: String,
    pub mode: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub percent: Option<f32>,
    pub longest_side: Option<u32>,
    pub shortest_side: Option<u32>,
    pub preserve_aspect: bool,
    pub filter: String,
    pub quality: u8,
    pub operation_id: Option<String>,
}

#[tauri::command]
pub async fn resize_images(app: AppHandle, options: ResizeImageOptions) -> Vec<ImageResult> {
    // Security gate: every input image must be real + non-system; output_dir
    // must not land in a forbidden location.
    for path in &options.paths {
        if let Err(e) = crate::core::safe_path::validate_user_path(path) {
            return options
                .paths
                .iter()
                .map(|p| fail_result(p.clone(), 0, (0, 0), e.clone()))
                .collect();
        }
    }
    if let Some(ref dir) = options.output_dir {
        if let Err(e) = crate::core::safe_path::forbid_system_path(dir) {
            return options
                .paths
                .iter()
                .map(|p| fail_result(p.clone(), 0, (0, 0), e.clone()))
                .collect();
        }
    }

    tauri::async_runtime::spawn_blocking(move || resize_images_blocking(app, options))
        .await
        .unwrap_or_else(|e| {
            vec![fail_result(
                String::new(),
                0,
                (0, 0),
                format!("Image worker crashed: {}", e),
            )]
        })
}

fn resize_images_blocking(app: AppHandle, options: ResizeImageOptions) -> Vec<ImageResult> {
    let op = operation_id(&options.operation_id);
    reset_cancel(&op);
    let total = options.paths.len();
    let paths = options.paths.clone();
    let mut results = Vec::with_capacity(total);

    for (i, path) in paths.into_iter().enumerate() {
        if is_cancelled(&op) {
            emit_progress(
                &app,
                &op,
                "resizer",
                &path,
                i,
                total,
                "cancelled",
                100,
                Some("Operation cancelled".into()),
            );
            break;
        }
        results.push(process_resize_one(&app, &op, i, total, path, &options));
    }

    reset_cancel(&op);
    results
}

fn process_resize_one(
    app: &AppHandle,
    op: &str,
    index: usize,
    total: usize,
    path: String,
    options: &ResizeImageOptions,
) -> ImageResult {
    let src = match validated_image_source(&path) {
        Ok(src) => src,
        Err(e) => return fail_source_validation(app, op, "resizer", path, index, total, e),
    };
    let original_size = output_size(&src);
    let out = match build_output_path(
        &src,
        options.output_dir.as_deref(),
        &options.suffix,
        resolve_output_ext(&options.output_format).as_deref(),
    ) {
        Ok(out) => out,
        Err(e) => {
            return fail_output_path(app, op, "resizer", path, index, total, original_size, e)
        }
    };

    emit_progress(app, op, "resizer", &path, index, total, "loading", 10, None);
    if is_cancelled(op) {
        emit_progress(
            app,
            op,
            "resizer",
            &path,
            index,
            total,
            "cancelled",
            100,
            Some("Operation cancelled".into()),
        );
        return fail_result(path, original_size, (0, 0), "Cancelled".into());
    }

    let img = match load_image(&src) {
        Ok(img) => img,
        Err(e) => {
            emit_progress(
                app,
                op,
                "resizer",
                &path,
                index,
                total,
                "error",
                100,
                Some(e.clone()),
            );
            return fail_result(path, original_size, (0, 0), e);
        }
    };

    let original_dims = (img.width(), img.height());
    let orig_w = img.width() as f32;
    let orig_h = img.height() as f32;

    emit_progress(
        app, op, "resizer", &path, index, total, "resizing", 55, None,
    );
    if is_cancelled(op) {
        emit_progress(
            app,
            op,
            "resizer",
            &path,
            index,
            total,
            "cancelled",
            100,
            Some("Operation cancelled".into()),
        );
        return fail_result(path, original_size, original_dims, "Cancelled".into());
    }

    let (target_w, target_h) = match options.mode.as_str() {
        // Image Studio "Keep size": re-encode (convert/compress) without resizing.
        "none" => (img.width(), img.height()),
        "percent" => {
            let pct = options.percent.unwrap_or(50.0).max(1.0) / 100.0;
            ((orig_w * pct).round() as u32, (orig_h * pct).round() as u32)
        }
        "longest_side" => {
            let ls = options.longest_side.unwrap_or(1920).max(1) as f32;
            if orig_w >= orig_h {
                let ratio = ls / orig_w;
                (ls.round() as u32, (orig_h * ratio).round() as u32)
            } else {
                let ratio = ls / orig_h;
                ((orig_w * ratio).round() as u32, ls.round() as u32)
            }
        }
        "shortest_side" => {
            let ss = options.shortest_side.unwrap_or(720).max(1) as f32;
            if orig_w <= orig_h {
                let ratio = ss / orig_w;
                (ss.round() as u32, (orig_h * ratio).round() as u32)
            } else {
                let ratio = ss / orig_h;
                ((orig_w * ratio).round() as u32, ss.round() as u32)
            }
        }
        _ => {
            let tw = options.width.unwrap_or(img.width()).max(1);
            let th = options.height.unwrap_or(img.height()).max(1);
            if options.preserve_aspect {
                let ratio_w = tw as f32 / orig_w;
                let ratio_h = th as f32 / orig_h;
                let ratio = ratio_w.min(ratio_h);
                (
                    (orig_w * ratio).round() as u32,
                    (orig_h * ratio).round() as u32,
                )
            } else {
                (tw, th)
            }
        }
    };

    let target_w = target_w.max(1);
    let target_h = target_h.max(1);
    let filter = filter_from_str(&options.filter);

    let resized = if options.preserve_aspect || options.mode != "px" {
        img.resize(target_w, target_h, filter)
    } else {
        img.resize_exact(target_w, target_h, filter)
    };

    let new_dims = (resized.width(), resized.height());
    emit_progress(app, op, "resizer", &path, index, total, "saving", 85, None);
    if is_cancelled(op) {
        emit_progress(
            app,
            op,
            "resizer",
            &path,
            index,
            total,
            "cancelled",
            100,
            Some("Operation cancelled".into()),
        );
        return fail_result(path, original_size, original_dims, "Cancelled".into());
    }

    if let Err(e) = save_image(&resized, &out, options.quality) {
        emit_progress(
            app,
            op,
            "resizer",
            &path,
            index,
            total,
            "error",
            100,
            Some(e.clone()),
        );
        return fail_result(path, original_size, original_dims, e);
    }

    emit_progress(app, op, "resizer", &path, index, total, "done", 100, None);

    ImageResult {
        source_path: path,
        output_path: out.to_string_lossy().to_string(),
        original_size,
        new_size: output_size(&out),
        original_dims,
        new_dims,
        success: true,
        error: None,
    }
}

// ===================== CROPPER =====================

#[derive(Deserialize, Clone)]
pub struct CropImageOptions {
    pub paths: Vec<String>,
    pub output_dir: Option<String>,
    pub suffix: String,
    /// Target output format ("" / "keep" ⇒ keep source extension).
    #[serde(default)]
    pub output_format: String,
    /// Source-pixel crop origin.
    pub x: u32,
    pub y: u32,
    /// Source-pixel crop dimensions.
    pub width: u32,
    pub height: u32,
    pub quality: u8,
    pub operation_id: Option<String>,
}

#[tauri::command]
pub async fn crop_images(app: AppHandle, options: CropImageOptions) -> Vec<ImageResult> {
    for path in &options.paths {
        if let Err(e) = crate::core::safe_path::validate_user_path(path) {
            return options
                .paths
                .iter()
                .map(|p| fail_result(p.clone(), 0, (0, 0), e.clone()))
                .collect();
        }
    }
    if let Some(ref dir) = options.output_dir {
        if let Err(e) = crate::core::safe_path::forbid_system_path(dir) {
            return options
                .paths
                .iter()
                .map(|p| fail_result(p.clone(), 0, (0, 0), e.clone()))
                .collect();
        }
    }

    tauri::async_runtime::spawn_blocking(move || crop_images_blocking(app, options))
        .await
        .unwrap_or_else(|e| {
            vec![fail_result(
                String::new(),
                0,
                (0, 0),
                format!("Image worker crashed: {}", e),
            )]
        })
}

fn crop_images_blocking(app: AppHandle, options: CropImageOptions) -> Vec<ImageResult> {
    let op = operation_id(&options.operation_id);
    reset_cancel(&op);
    let total = options.paths.len();
    let paths = options.paths.clone();
    let mut results = Vec::with_capacity(total);

    for (i, path) in paths.into_iter().enumerate() {
        if is_cancelled(&op) {
            emit_progress(
                &app,
                &op,
                "cropper",
                &path,
                i,
                total,
                "cancelled",
                100,
                Some("Operation cancelled".into()),
            );
            break;
        }
        results.push(process_crop_one(&app, &op, i, total, path, &options));
    }

    reset_cancel(&op);
    results
}

fn process_crop_one(
    app: &AppHandle,
    op: &str,
    index: usize,
    total: usize,
    path: String,
    options: &CropImageOptions,
) -> ImageResult {
    let src = match validated_image_source(&path) {
        Ok(src) => src,
        Err(e) => return fail_source_validation(app, op, "cropper", path, index, total, e),
    };
    let original_size = output_size(&src);
    if is_gif(&src) {
        let error =
            "Animated GIF crop is not supported; convert to a raster image first".to_string();
        emit_progress(
            app,
            op,
            "cropper",
            &path,
            index,
            total,
            "error",
            100,
            Some(error.clone()),
        );
        return fail_result(path, original_size, (0, 0), error);
    }
    let output_ext = crop_output_ext(&src, &options.output_format);
    let out = match build_output_path(
        &src,
        options.output_dir.as_deref(),
        &options.suffix,
        output_ext.as_deref(),
    ) {
        Ok(out) => out,
        Err(e) => {
            return fail_output_path(app, op, "cropper", path, index, total, original_size, e)
        }
    };

    emit_progress(app, op, "cropper", &path, index, total, "loading", 10, None);
    if is_cancelled(op) {
        emit_progress(
            app,
            op,
            "cropper",
            &path,
            index,
            total,
            "cancelled",
            100,
            Some("Operation cancelled".into()),
        );
        return fail_result(path, original_size, (0, 0), "Cancelled".into());
    }

    let img = match load_image(&src) {
        Ok(img) => img,
        Err(e) => {
            emit_progress(
                app,
                op,
                "cropper",
                &path,
                index,
                total,
                "error",
                100,
                Some(e.clone()),
            );
            return fail_result(path, original_size, (0, 0), e);
        }
    };

    let original_dims = (img.width(), img.height());
    emit_progress(
        app, op, "cropper", &path, index, total, "cropping", 55, None,
    );
    if is_cancelled(op) {
        emit_progress(
            app,
            op,
            "cropper",
            &path,
            index,
            total,
            "cancelled",
            100,
            Some("Operation cancelled".into()),
        );
        return fail_result(path, original_size, original_dims, "Cancelled".into());
    }

    let cropped = match crop_exact(&img, options.x, options.y, options.width, options.height) {
        Ok(cropped) => cropped,
        Err(e) => {
            emit_progress(
                app,
                op,
                "cropper",
                &path,
                index,
                total,
                "error",
                100,
                Some(e.clone()),
            );
            return fail_result(path, original_size, original_dims, e);
        }
    };

    let new_dims = (cropped.width(), cropped.height());
    emit_progress(app, op, "cropper", &path, index, total, "saving", 85, None);
    if is_cancelled(op) {
        emit_progress(
            app,
            op,
            "cropper",
            &path,
            index,
            total,
            "cancelled",
            100,
            Some("Operation cancelled".into()),
        );
        return fail_result(path, original_size, original_dims, "Cancelled".into());
    }

    if let Err(e) = save_image(&cropped, &out, options.quality) {
        emit_progress(
            app,
            op,
            "cropper",
            &path,
            index,
            total,
            "error",
            100,
            Some(e.clone()),
        );
        return fail_result(path, original_size, original_dims, e);
    }

    emit_progress(app, op, "cropper", &path, index, total, "done", 100, None);

    ImageResult {
        source_path: path,
        output_path: out.to_string_lossy().to_string(),
        original_size,
        new_size: output_size(&out),
        original_dims,
        new_dims,
        success: true,
        error: None,
    }
}

// ===================== BACKGROUND REMOVAL =====================

const HIGH_QUALITY_U2NET_URL: &str = "https://huggingface.co/Topurrra/u2net_onnx/resolve/065dc383e4d551f458a30118b7d1570af4d868db/u2net.onnx?download=true";
const HIGH_QUALITY_U2NET_FILE_NAME: &str = "u2net.onnx";
const HIGH_QUALITY_U2NET_SIZE: u64 = 175_997_641;
const HIGH_QUALITY_U2NET_SHA256: &str =
    "8d10d2f3bb75ae3b6d527c77944fc5e7dcd94b29809d47a739a7a728a912b491";
const HIGH_QUALITY_U2NET_UNAVAILABLE: &str =
    "The high-quality background-removal model is unavailable. Download it in Settings > Image models.";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BackgroundRemovalQuality {
    Fast,
    High,
}

impl BackgroundRemovalQuality {
    fn parse(value: Option<&str>) -> Result<Self, String> {
        match value {
            None | Some("fast") => Ok(Self::Fast),
            Some("high") => Ok(Self::High),
            Some(_) => Err("Background removal quality must be exactly 'fast' or 'high'".into()),
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackgroundRemovalModelStatus {
    pub available: bool,
    pub size_bytes: u64,
    pub path: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct BackgroundRemovalModelDownloadProgress {
    downloaded: u64,
    total: u64,
}

fn high_quality_u2net_model_path(app_data: &Path) -> PathBuf {
    app_data
        .join("background-removal")
        .join(HIGH_QUALITY_U2NET_FILE_NAME)
}

fn high_quality_u2net_partial_path(model_path: &Path) -> PathBuf {
    model_path.with_file_name(format!("{HIGH_QUALITY_U2NET_FILE_NAME}.partial"))
}

fn high_quality_u2net_path_for_app(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map(|app_data| high_quality_u2net_model_path(&app_data))
        .map_err(|error| format!("Could not resolve the app data directory: {error}"))
}

fn high_quality_u2net_model_size(path: &Path) -> Result<u64, String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("High-quality background-removal model is unreadable: {error}"))?;
    if !metadata.file_type().is_file() {
        return Err("High-quality background-removal model must be a regular file".into());
    }
    if metadata.len() != HIGH_QUALITY_U2NET_SIZE {
        return Err(format!(
            "High-quality background-removal model has an unexpected size (expected {HIGH_QUALITY_U2NET_SIZE} bytes)"
        ));
    }
    Ok(metadata.len())
}

fn sha256_file(path: &Path) -> Result<String, String> {
    let mut file = fs::File::open(path)
        .map_err(|error| format!("Could not open downloaded model for verification: {error}"))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 1024 * 1024];

    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| format!("Could not verify downloaded model: {error}"))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }

    Ok(hex::encode(hasher.finalize()))
}

fn is_pinned_high_quality_u2net_sha256(actual: &str) -> bool {
    actual == HIGH_QUALITY_U2NET_SHA256
}

fn verify_high_quality_u2net_download(path: &Path) -> Result<u64, String> {
    let size = high_quality_u2net_model_size(path)?;
    let actual = sha256_file(path)?;
    if !is_pinned_high_quality_u2net_sha256(&actual) {
        return Err("Downloaded model checksum did not match the pinned U²-Net checksum".into());
    }
    Ok(size)
}

fn available_high_quality_u2net_status(
    path: PathBuf,
    size_bytes: u64,
) -> BackgroundRemovalModelStatus {
    BackgroundRemovalModelStatus {
        available: true,
        size_bytes,
        path: Some(path.to_string_lossy().into_owned()),
    }
}

fn partial_high_quality_u2net_size(path: &Path) -> u64 {
    fs::symlink_metadata(path)
        .ok()
        .filter(|metadata| metadata.file_type().is_file())
        .map(|metadata| metadata.len())
        .unwrap_or(0)
}

fn unavailable_high_quality_u2net_status(size_bytes: u64) -> BackgroundRemovalModelStatus {
    BackgroundRemovalModelStatus {
        available: false,
        size_bytes,
        path: None,
    }
}

#[tauri::command]
pub fn background_removal_model_status(
    app: AppHandle,
) -> Result<BackgroundRemovalModelStatus, String> {
    if !cfg!(feature = "image-background-removal") {
        return Ok(unavailable_high_quality_u2net_status(0));
    }

    let model_path = high_quality_u2net_path_for_app(&app)?;
    let partial_size =
        partial_high_quality_u2net_size(&high_quality_u2net_partial_path(&model_path));
    Ok(match high_quality_u2net_model_size(&model_path) {
        Ok(size_bytes) => available_high_quality_u2net_status(model_path, size_bytes),
        Err(_) => unavailable_high_quality_u2net_status(partial_size),
    })
}

fn promote_high_quality_u2net_download(partial: &Path, target: &Path) -> Result<(), String> {
    match fs::symlink_metadata(target) {
        Ok(metadata) if !metadata.file_type().is_file() => {
            return Err(
                "High-quality background-removal model target must be a regular file".into(),
            );
        }
        Ok(_) => fs::remove_file(target)
            .map_err(|error| format!("Could not replace existing high-quality model: {error}"))?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(format!(
                "Could not inspect high-quality model target: {error}"
            ))
        }
    }

    fs::rename(partial, target)
        .map_err(|error| format!("Could not finalize the high-quality model download: {error}"))
}

#[tauri::command(async)]
pub fn download_background_removal_model(
    app: AppHandle,
) -> Result<BackgroundRemovalModelStatus, String> {
    if !cfg!(feature = "image-background-removal") {
        return Err("High-quality background removal is not included in this build".into());
    }

    let model_path = high_quality_u2net_path_for_app(&app)?;
    if let Ok(size_bytes) = high_quality_u2net_model_size(&model_path) {
        return Ok(available_high_quality_u2net_status(model_path, size_bytes));
    }

    let model_dir = model_path
        .parent()
        .ok_or_else(|| "Could not resolve the high-quality model directory".to_string())?;
    fs::create_dir_all(model_dir)
        .map_err(|error| format!("Could not create the high-quality model directory: {error}"))?;
    let partial = high_quality_u2net_partial_path(&model_path);

    let resume = match fs::symlink_metadata(&partial) {
        Ok(metadata) if !metadata.file_type().is_file() => {
            return Err("High-quality model partial download must be a regular file".into());
        }
        Ok(metadata) if metadata.len() >= HIGH_QUALITY_U2NET_SIZE => {
            if metadata.len() == HIGH_QUALITY_U2NET_SIZE
                && verify_high_quality_u2net_download(&partial).is_ok()
            {
                promote_high_quality_u2net_download(&partial, &model_path)?;
                let _ = app.emit(
                    "background-removal-model-download-progress",
                    BackgroundRemovalModelDownloadProgress {
                        downloaded: HIGH_QUALITY_U2NET_SIZE,
                        total: HIGH_QUALITY_U2NET_SIZE,
                    },
                );
                return Ok(available_high_quality_u2net_status(
                    model_path,
                    HIGH_QUALITY_U2NET_SIZE,
                ));
            }
            fs::remove_file(&partial).map_err(|error| {
                format!("Could not discard an invalid high-quality model partial download: {error}")
            })?;
            false
        }
        Ok(_) => true,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(error) => {
            return Err(format!(
                "Could not inspect high-quality model partial download: {error}"
            ))
        }
    };

    let mut command = Command::new("curl.exe");
    command.args(["-L", "-f", "-sS", "--connect-timeout", "30", "--retry", "2"]);
    if resume {
        command.args(["-C", "-"]);
    }
    command.arg("-o").arg(&partial).arg(HIGH_QUALITY_U2NET_URL);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command.stderr(Stdio::piped());
    let mut child = command.spawn().map_err(|error| {
        format!(
            "Failed to launch curl.exe for the high-quality model download: {error}. curl ships with Windows 10 1803 and later."
        )
    })?;

    let status = loop {
        if let Some(status) = child
            .try_wait()
            .map_err(|error| format!("High-quality model download process error: {error}"))?
        {
            break status;
        }
        let downloaded = fs::metadata(&partial)
            .map(|metadata| metadata.len())
            .unwrap_or(0);
        let _ = app.emit(
            "background-removal-model-download-progress",
            BackgroundRemovalModelDownloadProgress {
                downloaded,
                total: HIGH_QUALITY_U2NET_SIZE,
            },
        );
        std::thread::sleep(Duration::from_millis(400));
    };

    if !status.success() {
        let mut stderr = String::new();
        if let Some(mut handle) = child.stderr.take() {
            let _ = handle.read_to_string(&mut stderr);
        }
        return Err(format!(
            "High-quality model download failed (exit {}): {}. The partial download was kept; press Download again to resume it.",
            status.code().unwrap_or(-1),
            stderr.trim()
        ));
    }

    let downloaded = fs::metadata(&partial)
        .map(|metadata| metadata.len())
        .unwrap_or(HIGH_QUALITY_U2NET_SIZE);
    let _ = app.emit(
        "background-removal-model-download-progress",
        BackgroundRemovalModelDownloadProgress {
            downloaded,
            total: HIGH_QUALITY_U2NET_SIZE,
        },
    );

    let size_bytes = match verify_high_quality_u2net_download(&partial) {
        Ok(size_bytes) => size_bytes,
        Err(error) => {
            let _ = fs::remove_file(&partial);
            return Err(format!(
                "High-quality model download failed integrity verification: {error}. The partial download was removed; try again."
            ));
        }
    };
    promote_high_quality_u2net_download(&partial, &model_path)?;

    Ok(available_high_quality_u2net_status(model_path, size_bytes))
}

#[derive(Deserialize, Clone)]
pub struct RemoveImageBackgroundOptions {
    /// Background removal is intentionally single-image: one U²-Net session
    /// is created and dropped for each operation so it never holds idle RAM.
    pub paths: Vec<String>,
    pub output_dir: Option<String>,
    pub suffix: String,
    #[serde(default)]
    pub quality: Option<String>,
    pub operation_id: Option<String>,
}

#[tauri::command]
pub async fn remove_image_background(
    app: AppHandle,
    options: RemoveImageBackgroundOptions,
) -> Vec<ImageResult> {
    const ONE_IMAGE_ERROR: &str = "Background removal requires exactly one image";

    let quality = match BackgroundRemovalQuality::parse(options.quality.as_deref()) {
        Ok(quality) => quality,
        Err(error) => {
            return if options.paths.is_empty() {
                vec![fail_result(String::new(), 0, (0, 0), error)]
            } else {
                options
                    .paths
                    .iter()
                    .map(|path| fail_result(path.clone(), 0, (0, 0), error.clone()))
                    .collect()
            };
        }
    };

    if options.paths.len() != 1 {
        return if options.paths.is_empty() {
            vec![fail_result(
                String::new(),
                0,
                (0, 0),
                ONE_IMAGE_ERROR.into(),
            )]
        } else {
            options
                .paths
                .iter()
                .map(|path| fail_result(path.clone(), 0, (0, 0), ONE_IMAGE_ERROR.into()))
                .collect()
        };
    }

    let path = &options.paths[0];
    if let Err(e) = crate::core::safe_path::validate_user_path(path) {
        return vec![fail_result(path.clone(), 0, (0, 0), e)];
    }
    if let Some(ref dir) = options.output_dir {
        if let Err(e) = crate::core::safe_path::forbid_system_path(dir) {
            return vec![fail_result(path.clone(), 0, (0, 0), e)];
        }
    }

    #[cfg(feature = "image-background-removal")]
    {
        return tauri::async_runtime::spawn_blocking(move || {
            remove_image_background_blocking(app, options, quality)
        })
        .await
        .unwrap_or_else(|e| {
            vec![fail_result(
                String::new(),
                0,
                (0, 0),
                format!("Image worker crashed: {}", e),
            )]
        });
    }

    #[cfg(not(feature = "image-background-removal"))]
    {
        let _ = app;
        let _ = (&options.suffix, &options.operation_id, quality);
        options
            .paths
            .into_iter()
            .map(|path| {
                fail_result(
                    path,
                    0,
                    (0, 0),
                    "Background removal is not included in this build".into(),
                )
            })
            .collect()
    }
}

#[cfg(feature = "image-background-removal")]
fn remove_image_background_blocking(
    app: AppHandle,
    options: RemoveImageBackgroundOptions,
    quality: BackgroundRemovalQuality,
) -> Vec<ImageResult> {
    let op = operation_id(&options.operation_id);
    reset_cancel(&op);
    let path = options.paths[0].clone();
    let result = process_background_removal_one(&app, &op, path, &options, quality);
    reset_cancel(&op);
    vec![result]
}

#[cfg(feature = "image-background-removal")]
fn u2netp_model_path(app: &AppHandle) -> Result<PathBuf, String> {
    if let Ok(resource_dir) = app.path().resource_dir() {
        let bundled = resource_dir.join("background-removal").join("u2netp.onnx");
        if bundled.is_file() {
            return Ok(bundled);
        }
    }

    let dev = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("background-removal")
        .join("u2netp.onnx");
    if dev.is_file() {
        return Ok(dev);
    }

    Err(
        "U2NETP model is missing. Reinstall KeepItLocal or restore background-removal/u2netp.onnx"
            .into(),
    )
}

#[cfg(feature = "image-background-removal")]
fn background_removal_model_path(
    app: &AppHandle,
    quality: BackgroundRemovalQuality,
) -> Result<PathBuf, String> {
    match quality {
        BackgroundRemovalQuality::Fast => u2netp_model_path(app),
        BackgroundRemovalQuality::High => {
            let model_path = high_quality_u2net_path_for_app(app)
                .map_err(|_| HIGH_QUALITY_U2NET_UNAVAILABLE.to_string())?;
            high_quality_u2net_model_size(&model_path)
                .map_err(|_| HIGH_QUALITY_U2NET_UNAVAILABLE.to_string())?;
            Ok(model_path)
        }
    }
}

#[cfg(feature = "image-background-removal")]
fn run_u2net(
    app: &AppHandle,
    quality: BackgroundRemovalQuality,
    image: &DynamicImage,
) -> Result<image::GrayImage, String> {
    let model_path = background_removal_model_path(app, quality)?;
    let primary = run_u2net_primary_output(&model_path, image)?;
    let alpha = normalize_u2net_mask(&primary)?;
    image::GrayImage::from_raw(U2NET_SIDE, U2NET_SIDE, alpha)
        .ok_or_else(|| "U²-Net returned an invalid foreground mask".into())
}

#[cfg(feature = "image-background-removal")]
fn run_u2net_primary_output(model_path: &Path, image: &DynamicImage) -> Result<Vec<f32>, String> {
    let input = ort::value::Tensor::<f32>::from_array((
        [1usize, 3, U2NET_SIDE as usize, U2NET_SIDE as usize],
        u2net_input(image).into_boxed_slice(),
    ))
    .map_err(|e| format!("U²-Net input setup failed: {e}"))?;

    // Deliberately per operation: a cached session retains the model's RAM while idle.
    let session = ort::session::Session::builder()
        .map_err(|e| format!("U²-Net session setup failed: {e}"))?
        .with_intra_threads(1)
        .map_err(|e| format!("U²-Net thread setup failed: {e}"))?
        .commit_from_file(model_path)
        .map_err(|e| format!("U²-Net model load failed: {e}"))?;
    let outputs = session
        .run(ort::inputs![input].map_err(|e| format!("U²-Net input failed: {e}"))?)
        .map_err(|e| format!("U²-Net inference failed: {e}"))?;
    if outputs.len() == 0 {
        return Err("U²-Net returned no foreground mask".into());
    }

    let (shape, values) = outputs[0]
        .try_extract_raw_tensor::<f32>()
        .map_err(|e| format!("U²-Net output read failed: {e}"))?;
    let expected_shape = [1_i64, 1, U2NET_SIDE as i64, U2NET_SIDE as i64];
    if shape != expected_shape.as_slice() {
        return Err(format!("U²-Net returned unexpected mask shape: {shape:?}"));
    }
    if values.iter().any(|value| !value.is_finite()) {
        return Err("U²-Net returned an invalid foreground mask".into());
    }

    Ok(values.to_vec())
}

#[cfg(feature = "image-background-removal")]
fn process_background_removal_one(
    app: &AppHandle,
    op: &str,
    path: String,
    options: &RemoveImageBackgroundOptions,
    quality: BackgroundRemovalQuality,
) -> ImageResult {
    let src = match validated_image_source(&path) {
        Ok(src) => src,
        Err(e) => return fail_source_validation(app, op, "background-removal", path, 0, 1, e),
    };
    let original_size = output_size(&src);
    let out = match build_output_path(
        &src,
        options.output_dir.as_deref(),
        &options.suffix,
        Some("png"),
    ) {
        Ok(out) => out,
        Err(e) => {
            return fail_output_path(app, op, "background-removal", path, 0, 1, original_size, e)
        }
    };

    emit_progress(
        app,
        op,
        "background-removal",
        &path,
        0,
        1,
        "loading",
        10,
        None,
    );
    if is_cancelled(op) {
        emit_progress(
            app,
            op,
            "background-removal",
            &path,
            0,
            1,
            "cancelled",
            100,
            Some("Operation cancelled".into()),
        );
        return fail_result(path, original_size, (0, 0), "Cancelled".into());
    }

    let image = match load_image(&src) {
        Ok(image) => image,
        Err(e) => {
            emit_progress(
                app,
                op,
                "background-removal",
                &path,
                0,
                1,
                "error",
                100,
                Some(e.clone()),
            );
            return fail_result(path, original_size, (0, 0), e);
        }
    };

    let original_dims = (image.width(), image.height());
    emit_progress(
        app,
        op,
        "background-removal",
        &path,
        0,
        1,
        "removing-background",
        55,
        None,
    );
    if is_cancelled(op) {
        emit_progress(
            app,
            op,
            "background-removal",
            &path,
            0,
            1,
            "cancelled",
            100,
            Some("Operation cancelled".into()),
        );
        return fail_result(path, original_size, original_dims, "Cancelled".into());
    }

    let mask = match run_u2net(app, quality, &image) {
        Ok(mask) => mask,
        Err(e) => {
            emit_progress(
                app,
                op,
                "background-removal",
                &path,
                0,
                1,
                "error",
                100,
                Some(e.clone()),
            );
            return fail_result(path, original_size, original_dims, e);
        }
    };
    let alpha = DynamicImage::ImageLuma8(mask)
        .resize_exact(original_dims.0, original_dims.1, FilterType::Lanczos3)
        .to_luma8();
    let output = match apply_alpha_mask(image, &alpha) {
        Ok(output) => output,
        Err(e) => {
            emit_progress(
                app,
                op,
                "background-removal",
                &path,
                0,
                1,
                "error",
                100,
                Some(e.clone()),
            );
            return fail_result(path, original_size, original_dims, e);
        }
    };

    emit_progress(
        app,
        op,
        "background-removal",
        &path,
        0,
        1,
        "saving",
        85,
        None,
    );
    if is_cancelled(op) {
        emit_progress(
            app,
            op,
            "background-removal",
            &path,
            0,
            1,
            "cancelled",
            100,
            Some("Operation cancelled".into()),
        );
        return fail_result(path, original_size, original_dims, "Cancelled".into());
    }

    if let Err(e) = save_image(&output, &out, 100) {
        emit_progress(
            app,
            op,
            "background-removal",
            &path,
            0,
            1,
            "error",
            100,
            Some(e.clone()),
        );
        return fail_result(path, original_size, original_dims, e);
    }

    emit_progress(
        app,
        op,
        "background-removal",
        &path,
        0,
        1,
        "done",
        100,
        None,
    );
    ImageResult {
        source_path: path,
        output_path: out.to_string_lossy().to_string(),
        original_size,
        new_size: output_size(&out),
        original_dims,
        new_dims: original_dims,
        success: true,
        error: None,
    }
}

// ===================== IMAGE TO BASE64 =====================

#[derive(Serialize, Clone)]
pub struct Base64ImageResult {
    pub source_path: String,
    pub file_name: String,
    pub mime: String,
    pub data_uri: String,
    pub raw_base64: String,
    pub original_size: u64,
    pub base64_len: usize,
    pub success: bool,
    pub error: Option<String>,
}

#[derive(Deserialize, Clone)]
pub struct ImageToBase64Options {
    pub paths: Vec<String>,
    pub operation_id: Option<String>,
}

#[tauri::command]
pub async fn images_to_base64(
    app: AppHandle,
    options: ImageToBase64Options,
) -> Vec<Base64ImageResult> {
    // Security gate: every input image must resolve to a real, non-system
    // path before we read its bytes. On rejection emit per-source errors.
    for path in &options.paths {
        if let Err(e) = crate::core::safe_path::validate_user_path(path) {
            return options
                .paths
                .iter()
                .map(|p| Base64ImageResult {
                    source_path: p.clone(),
                    file_name: String::new(),
                    mime: String::new(),
                    data_uri: String::new(),
                    raw_base64: String::new(),
                    original_size: 0,
                    base64_len: 0,
                    success: false,
                    error: Some(e.clone()),
                })
                .collect();
        }
    }

    tauri::async_runtime::spawn_blocking(move || images_to_base64_blocking(app, options))
        .await
        .unwrap_or_else(|e| {
            vec![Base64ImageResult {
                source_path: String::new(),
                file_name: String::new(),
                mime: String::new(),
                data_uri: String::new(),
                raw_base64: String::new(),
                original_size: 0,
                base64_len: 0,
                success: false,
                error: Some(format!("Image worker crashed: {}", e)),
            }]
        })
}

fn images_to_base64_blocking(
    app: AppHandle,
    options: ImageToBase64Options,
) -> Vec<Base64ImageResult> {
    let op = operation_id(&options.operation_id);
    reset_cancel(&op);
    let total = options.paths.len();
    let paths = options.paths.clone();
    let mut results = Vec::with_capacity(total);

    for (i, path) in paths.into_iter().enumerate() {
        if is_cancelled(&op) {
            emit_progress(
                &app,
                &op,
                "base64",
                &path,
                i,
                total,
                "cancelled",
                100,
                Some("Operation cancelled".into()),
            );
            break;
        }
        results.push(process_base64_one(&app, &op, i, total, path));
    }

    reset_cancel(&op);
    results
}

fn process_base64_one(
    app: &AppHandle,
    op: &str,
    index: usize,
    total: usize,
    path: String,
) -> Base64ImageResult {
    let src = match validated_image_source(&path) {
        Ok(src) => src,
        Err(e) => {
            emit_progress(
                app,
                op,
                "base64",
                &path,
                index,
                total,
                "error",
                100,
                Some(e.clone()),
            );
            return Base64ImageResult {
                source_path: path,
                file_name: String::new(),
                mime: String::new(),
                data_uri: String::new(),
                raw_base64: String::new(),
                original_size: 0,
                base64_len: 0,
                success: false,
                error: Some(e),
            };
        }
    };
    let original_size = output_size(&src);
    let ext = src
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_lowercase())
        .unwrap_or_default();
    let file_name = src
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("image")
        .to_string();

    let mime = match ext.as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "bmp" => "image/bmp",
        "tiff" | "tif" => "image/tiff",
        "ico" => "image/x-icon",
        "svg" => "image/svg+xml",
        _ => "application/octet-stream",
    };

    emit_progress(app, op, "base64", &path, index, total, "reading", 25, None);
    if is_cancelled(op) {
        emit_progress(
            app,
            op,
            "base64",
            &path,
            index,
            total,
            "cancelled",
            100,
            Some("Operation cancelled".into()),
        );
        return Base64ImageResult {
            source_path: path,
            file_name,
            mime: mime.into(),
            data_uri: String::new(),
            raw_base64: String::new(),
            original_size,
            base64_len: 0,
            success: false,
            error: Some("Cancelled".into()),
        };
    }

    let bytes = match fs::read(&src) {
        Ok(bytes) => bytes,
        Err(e) => {
            let err = format!("Read failed: {}", e);
            emit_progress(
                app,
                op,
                "base64",
                &path,
                index,
                total,
                "error",
                100,
                Some(err.clone()),
            );
            return Base64ImageResult {
                source_path: path,
                file_name,
                mime: mime.into(),
                data_uri: String::new(),
                raw_base64: String::new(),
                original_size,
                base64_len: 0,
                success: false,
                error: Some(err),
            };
        }
    };

    emit_progress(app, op, "base64", &path, index, total, "encoding", 75, None);
    if is_cancelled(op) {
        emit_progress(
            app,
            op,
            "base64",
            &path,
            index,
            total,
            "cancelled",
            100,
            Some("Operation cancelled".into()),
        );
        return Base64ImageResult {
            source_path: path,
            file_name,
            mime: mime.into(),
            data_uri: String::new(),
            raw_base64: String::new(),
            original_size,
            base64_len: 0,
            success: false,
            error: Some("Cancelled".into()),
        };
    }

    let raw_base64 = STANDARD.encode(&bytes);
    let data_uri = format!("data:{};base64,{}", mime, raw_base64);
    let base64_len = raw_base64.len();

    emit_progress(app, op, "base64", &path, index, total, "done", 100, None);

    Base64ImageResult {
        source_path: path,
        file_name,
        mime: mime.into(),
        data_uri,
        raw_base64,
        original_size,
        base64_len,
        success: true,
        error: None,
    }
}

// ===================== IMAGE AUTOMATION =====================

#[derive(Deserialize, Clone)]
pub struct ImageAutomationOptions {
    pub paths: Vec<String>,
    pub output_dir: Option<String>,
    pub suffix: String,
    pub resize_enabled: bool,
    pub resize_longest_side: u32,
    pub convert_enabled: bool,
    pub output_format: String,
    pub compress_enabled: bool,
    pub compress_quality: u8,
    pub image_quality: u8,
    pub operation_id: Option<String>,
}

#[tauri::command]
pub async fn run_image_automation(
    app: AppHandle,
    options: ImageAutomationOptions,
) -> Vec<ImageResult> {
    // Security gate: every input image must be real + non-system; output_dir
    // must not land in a forbidden location.
    for path in &options.paths {
        if let Err(e) = crate::core::safe_path::validate_user_path(path) {
            return options
                .paths
                .iter()
                .map(|p| fail_result(p.clone(), 0, (0, 0), e.clone()))
                .collect();
        }
    }
    if let Some(ref dir) = options.output_dir {
        if let Err(e) = crate::core::safe_path::forbid_system_path(dir) {
            return options
                .paths
                .iter()
                .map(|p| fail_result(p.clone(), 0, (0, 0), e.clone()))
                .collect();
        }
    }

    tauri::async_runtime::spawn_blocking(move || run_image_automation_blocking(app, options))
        .await
        .unwrap_or_else(|e| {
            vec![fail_result(
                String::new(),
                0,
                (0, 0),
                format!("Image automation worker crashed: {}", e),
            )]
        })
}

fn run_image_automation_blocking(
    app: AppHandle,
    options: ImageAutomationOptions,
) -> Vec<ImageResult> {
    let op = operation_id(&options.operation_id);
    reset_cancel(&op);
    let total = options.paths.len();
    let paths = options.paths.clone();
    let mut results = Vec::with_capacity(total);

    for (i, path) in paths.into_iter().enumerate() {
        if is_cancelled(&op) {
            emit_progress(
                &app,
                &op,
                "automation",
                &path,
                i,
                total,
                "cancelled",
                100,
                Some("Operation cancelled".into()),
            );
            break;
        }
        results.push(process_image_automation_one(
            &app, &op, i, total, path, &options,
        ));
    }

    reset_cancel(&op);
    results
}

fn process_image_automation_one(
    app: &AppHandle,
    op: &str,
    index: usize,
    total: usize,
    path: String,
    options: &ImageAutomationOptions,
) -> ImageResult {
    let src = match validated_image_source(&path) {
        Ok(src) => src,
        Err(e) => return fail_source_validation(app, op, "automation", path, index, total, e),
    };
    let original_size = output_size(&src);
    let source_ext = src
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_lowercase())
        .unwrap_or_else(|| "png".into());

    let output_ext = if options.convert_enabled {
        options.output_format.trim_start_matches('.').to_lowercase()
    } else {
        source_ext.clone()
    };

    if output_ext == "svg" {
        let err = "SVG output is not supported for image automation".to_string();
        emit_progress(
            app,
            op,
            "automation",
            &path,
            index,
            total,
            "error",
            100,
            Some(err.clone()),
        );
        return fail_result(path, original_size, (0, 0), err);
    }

    if source_ext == "svg" && !options.convert_enabled {
        let err = "SVG input must be converted to a raster format in automation".to_string();
        emit_progress(
            app,
            op,
            "automation",
            &path,
            index,
            total,
            "error",
            100,
            Some(err.clone()),
        );
        return fail_result(path, original_size, (0, 0), err);
    }

    let out = match build_output_path(
        &src,
        options.output_dir.as_deref(),
        &options.suffix,
        Some(&output_ext),
    ) {
        Ok(out) => out,
        Err(e) => {
            return fail_output_path(app, op, "automation", path, index, total, original_size, e)
        }
    };

    emit_progress(
        app,
        op,
        "automation",
        &path,
        index,
        total,
        "loading",
        10,
        None,
    );
    if is_cancelled(op) {
        emit_progress(
            app,
            op,
            "automation",
            &path,
            index,
            total,
            "cancelled",
            100,
            Some("Operation cancelled".into()),
        );
        return fail_result(path, original_size, (0, 0), "Cancelled".into());
    }

    let mut img = match load_image(&src) {
        Ok(img) => img,
        Err(e) => {
            emit_progress(
                app,
                op,
                "automation",
                &path,
                index,
                total,
                "error",
                100,
                Some(e.clone()),
            );
            return fail_result(path, original_size, (0, 0), e);
        }
    };

    let original_dims = (img.width(), img.height());
    let mut new_dims = original_dims;

    if options.resize_enabled {
        emit_progress(
            app,
            op,
            "automation",
            &path,
            index,
            total,
            "resizing",
            40,
            None,
        );
        if is_cancelled(op) {
            emit_progress(
                app,
                op,
                "automation",
                &path,
                index,
                total,
                "cancelled",
                100,
                Some("Operation cancelled".into()),
            );
            return fail_result(path, original_size, original_dims, "Cancelled".into());
        }

        let limit = options.resize_longest_side.max(1);
        if img.width() > limit || img.height() > limit {
            img = img.resize(limit, limit, FilterType::Lanczos3);
            new_dims = (img.width(), img.height());
        }
    }

    emit_progress(
        app,
        op,
        "automation",
        &path,
        index,
        total,
        "saving",
        85,
        None,
    );
    if is_cancelled(op) {
        emit_progress(
            app,
            op,
            "automation",
            &path,
            index,
            total,
            "cancelled",
            100,
            Some("Operation cancelled".into()),
        );
        return fail_result(path, original_size, original_dims, "Cancelled".into());
    }

    let quality = if options.compress_enabled {
        options.compress_quality
    } else {
        options.image_quality
    };

    if let Err(e) = save_image(&img, &out, quality) {
        emit_progress(
            app,
            op,
            "automation",
            &path,
            index,
            total,
            "error",
            100,
            Some(e.clone()),
        );
        return fail_result(path, original_size, original_dims, e);
    }

    emit_progress(
        app,
        op,
        "automation",
        &path,
        index,
        total,
        "done",
        100,
        None,
    );

    ImageResult {
        source_path: path,
        output_path: out.to_string_lossy().to_string(),
        original_size,
        new_size: output_size(&out),
        original_dims,
        new_dims,
        success: true,
        error: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::GenericImageView;

    #[test]
    fn crop_exact_returns_only_the_requested_pixels() {
        let source =
            image::RgbaImage::from_fn(4, 3, |x, y| image::Rgba([x as u8, y as u8, 0, 255]));
        let cropped = crop_exact(&DynamicImage::ImageRgba8(source), 1, 1, 2, 1)
            .expect("crop within the source image");

        assert_eq!(cropped.dimensions(), (2, 1));
        assert_eq!(cropped.get_pixel(0, 0), image::Rgba([1, 1, 0, 255]));
        assert_eq!(cropped.get_pixel(1, 0), image::Rgba([2, 1, 0, 255]));
    }

    #[test]
    fn crop_exact_rejects_a_rectangle_outside_the_source() {
        let source = DynamicImage::new_rgba8(4, 3);

        assert!(crop_exact(&source, 3, 0, 2, 1).is_err());
    }

    #[test]
    fn build_output_path_rejects_navigation_and_source_overwrites() {
        let source = std::path::Path::new("photo.png");

        let suffix_error = build_output_path(source, None, "../escape", None)
            .expect_err("suffix must not create a path component");
        assert!(suffix_error.contains("suffix"));

        let overwrite_error = build_output_path(source, None, "", None)
            .expect_err("output must not overwrite the source");
        assert!(overwrite_error.contains("overwrite"));
    }

    #[test]
    fn build_output_path_returns_the_validated_write_target() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock after epoch")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "swisskit-image-output-path-{}-{}",
            std::process::id(),
            unique
        ));
        fs::create_dir_all(&dir).expect("create fixture directory");
        let source = dir.join("photo.jpg");
        fs::write(&source, b"fixture").expect("write source fixture");

        let output = build_output_path(
            &source,
            Some(dir.to_str().expect("UTF-8 temp directory")),
            "_edited",
            Some("png"),
        )
        .expect("valid output target");

        assert_eq!(
            output,
            dir.canonicalize()
                .expect("canonical fixture directory")
                .join("photo_edited.png")
        );

        fs::remove_file(&source).expect("remove source fixture");
        fs::remove_dir(&dir).expect("remove fixture directory");
    }

    #[test]
    fn validated_image_source_returns_a_canonical_path() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock after epoch")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "swisskit-image-source-path-{}-{}",
            std::process::id(),
            unique
        ));
        fs::create_dir_all(&dir).expect("create fixture directory");
        let source = dir.join("photo.png");
        fs::write(&source, b"fixture").expect("write source fixture");

        let canonical = validated_image_source(
            dir.join(".")
                .join("photo.png")
                .to_str()
                .expect("UTF-8 fixture path"),
        )
        .expect("valid source path");

        assert_eq!(
            canonical,
            source.canonicalize().expect("canonical source fixture")
        );

        fs::remove_file(&source).expect("remove source fixture");
        fs::remove_dir(&dir).expect("remove fixture directory");
    }

    #[test]
    fn crop_output_uses_png_when_keeping_an_svg_source_format() {
        assert_eq!(
            crop_output_ext(Path::new("illustration.svg"), "keep").as_deref(),
            Some("png")
        );
        assert_eq!(
            crop_output_ext(Path::new("illustration.svg"), "webp").as_deref(),
            Some("webp")
        );
    }

    #[test]
    fn crop_identifies_gif_sources_case_insensitively() {
        assert!(is_gif(Path::new("animation.GIF")));
        assert!(!is_gif(Path::new("still.png")));
    }

    #[test]
    fn load_image_applies_jpeg_exif_orientation() {
        use image::ImageEncoder;

        let source = image::RgbImage::from_fn(2, 3, |x, y| {
            image::Rgb([(x * 100) as u8, (y * 80) as u8, 0])
        });
        let mut jpeg = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 100)
            .write_image(
                source.as_raw(),
                source.width(),
                source.height(),
                image::ExtendedColorType::Rgb8,
            )
            .expect("encode fixture JPEG");

        // A minimal little-endian EXIF APP1 segment with orientation 6 (90 degrees CW).
        let exif_orientation_6: [u8; 32] = [
            b'E', b'x', b'i', b'f', 0, 0, // Exif signature
            b'I', b'I', 42, 0, 8, 0, 0, 0, // TIFF header
            1, 0, // one IFD entry
            0x12, 0x01, 3, 0, 1, 0, 0, 0, 6, 0, 0, 0, // Orientation = 6
            0, 0, 0, 0, // no next IFD
        ];
        let mut oriented_jpeg = Vec::with_capacity(jpeg.len() + 36);
        oriented_jpeg.extend_from_slice(&jpeg[..2]); // SOI
        oriented_jpeg.extend_from_slice(&[0xff, 0xe1, 0, 34]); // APP1 length incl. its two bytes
        oriented_jpeg.extend_from_slice(&exif_orientation_6);
        oriented_jpeg.extend_from_slice(&jpeg[2..]);

        let path = std::env::temp_dir().join(format!(
            "swisskit-image-orientation-{}.jpg",
            std::process::id()
        ));
        fs::write(&path, oriented_jpeg).expect("write EXIF JPEG fixture");
        let decoded = load_image(&path);
        let _ = fs::remove_file(&path);

        assert_eq!(decoded.expect("decode EXIF JPEG").dimensions(), (3, 2));
    }

    #[cfg(feature = "image-background-removal")]
    #[test]
    fn u2net_input_is_channel_first_imagenet_normalized() {
        let source =
            DynamicImage::ImageRgb8(image::RgbImage::from_pixel(1, 1, image::Rgb([0, 127, 255])));
        let input = u2net_input(&source);
        let plane = 320 * 320;

        assert_eq!(input.len(), 3 * plane);
        assert!((input[0] - (-0.485 / 0.229)).abs() < 1e-6);
        assert!((input[plane] - ((127.0 / 255.0 - 0.456) / 0.224)).abs() < 1e-6);
        assert!((input[2 * plane] - ((1.0 - 0.406) / 0.225)).abs() < 1e-6);
    }

    #[cfg(feature = "image-background-removal")]
    #[test]
    fn apply_alpha_mask_preserves_dimensions_and_rgb() {
        let source = DynamicImage::ImageRgba8(
            image::RgbaImage::from_raw(2, 1, vec![10, 20, 30, 255, 40, 50, 60, 255])
                .expect("valid RGBA pixels"),
        );
        let mask = image::GrayImage::from_raw(2, 1, vec![0, 128]).expect("valid mask pixels");
        let output = apply_alpha_mask(source, &mask).expect("matching image and mask dimensions");
        let rgba = output.to_rgba8();

        assert_eq!(rgba.dimensions(), (2, 1));
        assert_eq!(rgba.get_pixel(0, 0).0, [10, 20, 30, 0]);
        assert_eq!(rgba.get_pixel(1, 0).0, [40, 50, 60, 128]);
    }

    #[cfg(feature = "image-background-removal")]
    #[test]
    fn normalize_u2net_mask_scales_primary_output_to_alpha() {
        assert_eq!(
            normalize_u2net_mask(&[0.2, 0.5, 0.8]).expect("finite model output"),
            vec![0, 128, 255],
        );
    }

    #[test]
    fn background_removal_quality_accepts_only_fast_or_high() {
        assert_eq!(
            BackgroundRemovalQuality::parse(None).expect("missing quality defaults to fast"),
            BackgroundRemovalQuality::Fast
        );
        assert_eq!(
            BackgroundRemovalQuality::parse(Some("fast")).expect("fast is supported"),
            BackgroundRemovalQuality::Fast
        );
        assert_eq!(
            BackgroundRemovalQuality::parse(Some("high")).expect("high is supported"),
            BackgroundRemovalQuality::High
        );
        assert!(BackgroundRemovalQuality::parse(Some("Fast")).is_err());
        assert!(BackgroundRemovalQuality::parse(Some(" high")).is_err());
    }

    #[test]
    fn high_quality_model_path_is_fixed_below_app_data() {
        let app_data = Path::new(r"C:\\KeepItLocalData");

        assert_eq!(
            high_quality_u2net_model_path(app_data),
            app_data.join("background-removal").join("u2net.onnx")
        );
    }

    #[test]
    fn partial_high_quality_model_size_is_available_for_resume_progress() {
        let path = std::env::temp_dir().join(format!(
            "swisskit-u2net-partial-{}-{}.onnx.partial",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system clock after epoch")
                .as_nanos()
        ));
        fs::write(&path, b"partial").expect("write partial fixture");

        let size = partial_high_quality_u2net_size(&path);
        let _ = fs::remove_file(&path);

        assert_eq!(size, 7);
    }

    #[test]
    fn high_quality_model_hash_accepts_only_the_pinned_digest() {
        assert!(is_pinned_high_quality_u2net_sha256(
            "8d10d2f3bb75ae3b6d527c77944fc5e7dcd94b29809d47a739a7a728a912b491"
        ));
        assert!(!is_pinned_high_quality_u2net_sha256(
            "8d10d2f3bb75ae3b6d527c77944fc5e7dcd94b29809d47a739a7a728a912b490"
        ));
    }

    #[test]
    fn sha256_file_hashes_downloaded_bytes() {
        let path = std::env::temp_dir().join(format!(
            "swisskit-u2net-hash-{}-{}.bin",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system clock after epoch")
                .as_nanos()
        ));
        fs::write(&path, b"abc").expect("write hash fixture");

        let actual = sha256_file(&path).expect("hash fixture");
        let _ = fs::remove_file(&path);

        assert_eq!(
            actual,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[cfg(feature = "image-background-removal")]
    #[test]
    #[ignore = "loads a U²-Net ONNX model; optionally set KEEPITLOCAL_U2NET_SMOKE_MODEL"]
    fn u2net_smoke() {
        let model = std::env::var_os("KEEPITLOCAL_U2NET_SMOKE_MODEL")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("background-removal")
                    .join("u2netp.onnx")
            });
        assert!(model.is_file(), "no U²-Net model at {}", model.display());

        let primary = run_u2net_primary_output(&model, &DynamicImage::new_rgb8(320, 320))
            .expect("U²-Net model should run");
        assert_eq!(primary.len(), 320 * 320);
        assert!(primary.iter().all(|value| value.is_finite()));
    }
}
