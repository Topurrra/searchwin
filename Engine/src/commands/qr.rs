use base64::{engine::general_purpose::STANDARD, Engine as _};
use image::Luma;
use qrcode::{EcLevel, QrCode};
use serde::Serialize;

#[derive(Serialize)]
pub struct QrResult {
    png_base64: String,
    svg: String,
    size: usize,
}

#[tauri::command]
pub fn generate_qr(
    text: String,
    error_correction: String,
    scale: u32,
    margin: u32,
) -> Result<QrResult, String> {
    if text.is_empty() {
        return Err("Text is empty".into());
    }

    let ec = match error_correction.as_str() {
        "L" => EcLevel::L,
        "M" => EcLevel::M,
        "Q" => EcLevel::Q,
        "H" => EcLevel::H,
        _ => EcLevel::M,
    };

    let code = QrCode::with_error_correction_level(text.as_bytes(), ec)
        .map_err(|e| format!("QR generation failed: {}", e))?;

    let png_image = code
        .render::<Luma<u8>>()
        .min_dimensions(scale * code.width() as u32, scale * code.width() as u32)
        .quiet_zone(margin > 0)
        .build();

    let mut png_bytes: Vec<u8> = Vec::new();
    png_image
        .write_to(
            &mut std::io::Cursor::new(&mut png_bytes),
            image::ImageFormat::Png,
        )
        .map_err(|e| format!("PNG encode failed: {}", e))?;

    let png_base64 = STANDARD.encode(&png_bytes);

    let svg = code
        .render::<qrcode::render::svg::Color>()
        .min_dimensions(scale * code.width() as u32, scale * code.width() as u32)
        .quiet_zone(margin > 0)
        .build();

    Ok(QrResult {
        png_base64,
        svg,
        size: png_image.width() as usize,
    })
}

#[tauri::command]
pub fn save_qr_png(base64_data: String, path: String) -> Result<(), String> {
    // QR saves are user-driven (file dialog), but we still validate the
    // chosen path so a compromised frontend can't drop a PNG into a
    // Startup folder or overwrite something in `C:\Windows\`.
    let target = crate::core::safe_path::validate_user_write_target(&path)?;
    let bytes = STANDARD
        .decode(&base64_data)
        .map_err(|e| format!("Decode failed: {}", e))?;
    std::fs::write(target, bytes).map_err(|e| format!("Write failed: {}", e))?;
    Ok(())
}

#[tauri::command]
pub fn save_qr_svg(svg: String, path: String) -> Result<(), String> {
    let target = crate::core::safe_path::validate_user_write_target(&path)?;
    std::fs::write(target, svg).map_err(|e| format!("Write failed: {}", e))?;
    Ok(())
}
