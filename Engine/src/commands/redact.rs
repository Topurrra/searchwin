use image::{imageops, GenericImage, GenericImageView, ImageReader, Rgba, RgbaImage};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Deserialize)]
pub struct RedactRegion {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    pub mode: String,  // "black" | "white" | "blur" | "pixelate"
    pub strength: u32, // blur radius or pixelate block size
}

#[derive(Deserialize)]
pub struct RedactInput {
    pub source_path: String,
    pub output_path: String,
    pub regions: Vec<RedactRegion>,
    pub strip_metadata: bool,
}

#[derive(Serialize)]
pub struct RedactResult {
    pub output_path: String,
    pub width: u32,
    pub height: u32,
    pub regions_applied: usize,
}

#[tauri::command]
pub fn redact_image(input: RedactInput) -> Result<RedactResult, String> {
    // Security gate: source image must be real + non-system; output path
    // must not target a forbidden location.
    crate::core::safe_path::validate_user_path(&input.source_path)?;
    crate::core::safe_path::validate_user_write_target(&input.output_path)?;

    let source = PathBuf::from(&input.source_path);

    let img = ImageReader::open(&source)
        .map_err(|e| format!("Cannot open: {}", e))?
        .with_guessed_format()
        .map_err(|e| format!("Cannot detect format: {}", e))?
        .decode()
        .map_err(|e| format!("Decode failed: {}", e))?;

    let (img_w, img_h) = img.dimensions();
    let mut canvas: RgbaImage = img.to_rgba8();

    for region in &input.regions {
        // Clamp to image bounds
        let x = region.x.min(img_w);
        let y = region.y.min(img_h);
        let w = region.width.min(img_w.saturating_sub(x));
        let h = region.height.min(img_h.saturating_sub(y));

        if w == 0 || h == 0 {
            continue;
        }

        match region.mode.as_str() {
            "black" => fill_region(&mut canvas, x, y, w, h, Rgba([0, 0, 0, 255])),
            "white" => fill_region(&mut canvas, x, y, w, h, Rgba([255, 255, 255, 255])),
            "blur" => {
                let strength = region.strength.max(1).min(50) as f32;
                blur_region(&mut canvas, x, y, w, h, strength)?;
            }
            "pixelate" => {
                let block = region.strength.max(2).min(100);
                pixelate_region(&mut canvas, x, y, w, h, block);
            }
            _ => {}
        }
    }

    let output = PathBuf::from(&input.output_path);
    let ext = output
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_lowercase())
        .unwrap_or_else(|| "png".to_string());

    match ext.as_str() {
        "jpg" | "jpeg" => {
            // JPEG can't store alpha — convert to RGB
            let rgb = image::DynamicImage::ImageRgba8(canvas.clone()).to_rgb8();
            rgb.save(&output)
                .map_err(|e| format!("Save failed: {}", e))?;
        }
        _ => {
            canvas
                .save(&output)
                .map_err(|e| format!("Save failed: {}", e))?;
        }
    }

    let _ = input.strip_metadata; // We re-encoded from raw pixels — metadata is gone by definition

    Ok(RedactResult {
        output_path: output.to_string_lossy().to_string(),
        width: img_w,
        height: img_h,
        regions_applied: input.regions.len(),
    })
}

fn fill_region(canvas: &mut RgbaImage, x: u32, y: u32, w: u32, h: u32, color: Rgba<u8>) {
    for py in y..(y + h) {
        for px in x..(x + w) {
            canvas.put_pixel(px, py, color);
        }
    }
}

fn blur_region(
    canvas: &mut RgbaImage,
    x: u32,
    y: u32,
    w: u32,
    h: u32,
    sigma: f32,
) -> Result<(), String> {
    // Crop, blur, paste back
    let sub = canvas.view(x, y, w, h).to_image();
    let blurred = imageops::blur(&sub, sigma);
    canvas
        .copy_from(&blurred, x, y)
        .map_err(|e| format!("Paste failed: {}", e))?;
    Ok(())
}

fn pixelate_region(canvas: &mut RgbaImage, x: u32, y: u32, w: u32, h: u32, block: u32) {
    let block = block.max(1);

    let mut by = 0;
    while by < h {
        let mut bx = 0;
        while bx < w {
            let bw = (block).min(w - bx);
            let bh = (block).min(h - by);

            // Average color in this block
            let mut r = 0u64;
            let mut g = 0u64;
            let mut b = 0u64;
            let mut a = 0u64;
            let count = (bw * bh) as u64;

            for py in 0..bh {
                for px in 0..bw {
                    let p = canvas.get_pixel(x + bx + px, y + by + py);
                    r += p.0[0] as u64;
                    g += p.0[1] as u64;
                    b += p.0[2] as u64;
                    a += p.0[3] as u64;
                }
            }

            let avg = Rgba([
                (r / count) as u8,
                (g / count) as u8,
                (b / count) as u8,
                (a / count) as u8,
            ]);

            for py in 0..bh {
                for px in 0..bw {
                    canvas.put_pixel(x + bx + px, y + by + py, avg);
                }
            }

            bx += block;
        }
        by += block;
    }
}
