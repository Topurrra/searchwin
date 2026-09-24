//! Offline CamScanner — document perspective-scanner backend.
//!
//! Two commands power the flow, mirroring `redact.rs` for the open/edit/save
//! skeleton and the mandatory `safe_path` gates:
//!
//!   1. `camscan_detect_corners` — decode the source image, find the best
//!      document quadrilateral (Canny edges → contours → polygon approximation,
//!      largest convex 4-gon by area), and return its 4 corners in FULL-RES
//!      source pixels, ordered TL/TR/BR/BL. If nothing convincing is found it
//!      returns the image bounds inset ~2 % with `detected = false`, so the
//!      frontend ALWAYS receives 4 usable corners — this command never fails
//!      hard on a decodable image.
//!
//!   2. `camscan_warp` — given the (possibly user-edited) 4 source corners,
//!      compute the projective homography mapping the OUTPUT rectangle onto the
//!      source quad, bilinear-sample the source into a deskewed rectangle, then
//!      apply an enhancement profile (color | grayscale | bw | magic) and save
//!      (PNG/JPG by extension, or a temp PNG preview when no out_path is given).
//!
//! The optional OCR/text step reuses the EXISTING `ocr` commands (`ocr_image`,
//! `ocr_available`, `ocr_languages`, `cancel_ocr_operation`) — this module does
//! NOT touch OCR.
//!
//! Pure-Rust, on-device, no native build: `image` 0.25 + `imageproc` for edge /
//! contour detection and `nalgebra` for the 4-point DLT homography solve.

use image::{GenericImageView, ImageReader, Rgba, RgbaImage};
use nalgebra::{Matrix3, Vector3};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// A point in FULL-RESOLUTION source-image pixel coordinates.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CamPoint {
    pub x: f64,
    pub y: f64,
}

/// Result of corner detection. `corners` are ordered TL, TR, BR, BL.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CamDetect {
    pub corners: [CamPoint; 4],
    pub detected: bool,
    pub width: u32,
    pub height: u32,
}

// ─── Detection ────────────────────────────────────────────────────────────

/// Best-effort document-quad detection.
///
/// Decodes the image, downscales a working copy (longest side ~1000 px) for
/// speed, runs Canny edge detection, finds contours, approximates each to a
/// polygon, keeps convex 4-vertex polygons, and picks the largest by area.
/// Corners are scaled back to full resolution and ordered TL/TR/BR/BL.
///
/// NEVER fails hard on a decodable image: when no convincing quad is found it
/// returns the image bounds inset ~2 % with `detected = false`.
#[tauri::command(async)]
pub fn camscan_detect_corners(path: String) -> Result<CamDetect, String> {
    // Security gate: source must be a real, non-system file.
    let source = crate::core::safe_path::validate_user_path(&path)?;

    let img = ImageReader::open(&source)
        .map_err(|e| format!("Cannot open: {}", e))?
        .with_guessed_format()
        .map_err(|e| format!("Cannot detect format: {}", e))?
        .decode()
        .map_err(|e| format!("Decode failed: {}", e))?;

    let (full_w, full_h) = img.dimensions();
    if full_w == 0 || full_h == 0 {
        return Err("Image has zero dimensions".to_string());
    }

    // The image-bounds fallback (inset ~2 %), used whenever detection fails.
    let fallback = || CamDetect {
        corners: inset_bounds(full_w, full_h, 0.02),
        detected: false,
        width: full_w,
        height: full_h,
    };

    // Downscale a working copy so edge/contour work is fast and stable.
    let longest = full_w.max(full_h);
    let target = 1000u32;
    let scale = if longest > target {
        target as f32 / longest as f32
    } else {
        1.0
    };
    let work_w = ((full_w as f32) * scale).round().max(1.0) as u32;
    let work_h = ((full_h as f32) * scale).round().max(1.0) as u32;

    let working = image::imageops::resize(
        &img.to_luma8(),
        work_w,
        work_h,
        image::imageops::FilterType::Triangle,
    );

    // Light Gaussian blur to suppress texture/noise before edge detection.
    let blurred = imageproc::filter::gaussian_blur_f32(&working, 1.4);

    // Canny edges → binary edge map. Thresholds tuned for document scans
    // shot against a contrasting background.
    let edges = imageproc::edges::canny(&blurred, 40.0, 100.0);

    // Find contours on the edge map; approximate each to a polygon and keep
    // convex quads. Pick the largest by area that is also a reasonable share
    // of the frame (rejects tiny specks and noise loops).
    let contours = imageproc::contours::find_contours::<u32>(&edges);
    let work_area = (work_w as f64) * (work_h as f64);
    let min_area = work_area * 0.10; // quad must cover ≥10 % of the working frame

    let mut best: Option<(f64, [CamPoint; 4])> = None;
    for contour in &contours {
        if contour.points.len() < 4 {
            continue;
        }
        let pts: Vec<(f64, f64)> = contour
            .points
            .iter()
            .map(|p| (p.x as f64, p.y as f64))
            .collect();

        // Perimeter-relative DP epsilon — the standard approxPolyDP knob.
        let perim = polygon_perimeter(&pts);
        if perim <= 0.0 {
            continue;
        }
        let epsilon = 0.02 * perim;
        let approx = douglas_peucker(&pts, epsilon);

        if approx.len() != 4 || !is_convex(&approx) {
            continue;
        }
        let area = polygon_area(&approx);
        if area < min_area {
            continue;
        }
        if best.as_ref().map(|(a, _)| area > *a).unwrap_or(true) {
            let ordered = order_corners(&[
                CamPoint { x: approx[0].0, y: approx[0].1 },
                CamPoint { x: approx[1].0, y: approx[1].1 },
                CamPoint { x: approx[2].0, y: approx[2].1 },
                CamPoint { x: approx[3].0, y: approx[3].1 },
            ]);
            best = Some((area, ordered));
        }
    }

    let Some((_, ordered)) = best else {
        return Ok(fallback());
    };

    // Scale the working-resolution corners back to full resolution.
    let inv = 1.0 / (scale as f64);
    let corners = [
        CamPoint { x: ordered[0].x * inv, y: ordered[0].y * inv },
        CamPoint { x: ordered[1].x * inv, y: ordered[1].y * inv },
        CamPoint { x: ordered[2].x * inv, y: ordered[2].y * inv },
        CamPoint { x: ordered[3].x * inv, y: ordered[3].y * inv },
    ];

    Ok(CamDetect {
        corners,
        detected: true,
        width: full_w,
        height: full_h,
    })
}

/// Image bounds inset by `frac` (e.g. 0.02 = 2 %), ordered TL/TR/BR/BL.
fn inset_bounds(w: u32, h: u32, frac: f64) -> [CamPoint; 4] {
    let dx = (w as f64) * frac;
    let dy = (h as f64) * frac;
    let x0 = dx;
    let y0 = dy;
    let x1 = (w as f64) - dx;
    let y1 = (h as f64) - dy;
    [
        CamPoint { x: x0, y: y0 }, // TL
        CamPoint { x: x1, y: y0 }, // TR
        CamPoint { x: x1, y: y1 }, // BR
        CamPoint { x: x0, y: y1 }, // BL
    ]
}

/// Order 4 corners TL, TR, BR, BL using the classic sum/diff heuristic:
///   - TL has the smallest (x + y), BR the largest.
///   - TR has the smallest (y − x), BL the largest.
fn order_corners(pts: &[CamPoint; 4]) -> [CamPoint; 4] {
    let mut tl = pts[0];
    let mut br = pts[0];
    let mut tr = pts[0];
    let mut bl = pts[0];
    let mut min_sum = f64::INFINITY;
    let mut max_sum = f64::NEG_INFINITY;
    let mut min_diff = f64::INFINITY;
    let mut max_diff = f64::NEG_INFINITY;
    for p in pts {
        let sum = p.x + p.y;
        let diff = p.y - p.x;
        if sum < min_sum {
            min_sum = sum;
            tl = *p;
        }
        if sum > max_sum {
            max_sum = sum;
            br = *p;
        }
        if diff < min_diff {
            min_diff = diff;
            tr = *p;
        }
        if diff > max_diff {
            max_diff = diff;
            bl = *p;
        }
    }
    [tl, tr, br, bl]
}

/// Shoelace area of a polygon (absolute value).
fn polygon_area(pts: &[(f64, f64)]) -> f64 {
    let n = pts.len();
    if n < 3 {
        return 0.0;
    }
    let mut acc = 0.0;
    for i in 0..n {
        let (x0, y0) = pts[i];
        let (x1, y1) = pts[(i + 1) % n];
        acc += x0 * y1 - x1 * y0;
    }
    acc.abs() * 0.5
}

/// Polygon perimeter (closed).
fn polygon_perimeter(pts: &[(f64, f64)]) -> f64 {
    let n = pts.len();
    if n < 2 {
        return 0.0;
    }
    let mut acc = 0.0;
    for i in 0..n {
        let (x0, y0) = pts[i];
        let (x1, y1) = pts[(i + 1) % n];
        acc += ((x1 - x0).powi(2) + (y1 - y0).powi(2)).sqrt();
    }
    acc
}

/// Is a (CCW or CW) polygon convex? Checks the cross-product sign is
/// consistent across every consecutive edge triple.
fn is_convex(pts: &[(f64, f64)]) -> bool {
    let n = pts.len();
    if n < 4 {
        return false;
    }
    let mut sign = 0i32;
    for i in 0..n {
        let (ax, ay) = pts[i];
        let (bx, by) = pts[(i + 1) % n];
        let (cx, cy) = pts[(i + 2) % n];
        let cross = (bx - ax) * (cy - by) - (by - ay) * (cx - bx);
        if cross.abs() < 1e-9 {
            continue; // collinear edge — ignore
        }
        let s = if cross > 0.0 { 1 } else { -1 };
        if sign == 0 {
            sign = s;
        } else if s != sign {
            return false;
        }
    }
    true
}

/// Douglas–Peucker polyline simplification (hand-rolled approxPolyDP).
/// Treats the input as a CLOSED polygon: it splits the loop at the two
/// farthest-apart vertices and simplifies each half, which is the standard
/// way to make DP work on closed contours.
fn douglas_peucker(pts: &[(f64, f64)], epsilon: f64) -> Vec<(f64, f64)> {
    let n = pts.len();
    if n < 3 {
        return pts.to_vec();
    }
    // Find the vertex farthest from pts[0]; that pair anchors the open split.
    let mut far_idx = 0usize;
    let mut far_dist = -1.0;
    for (i, p) in pts.iter().enumerate() {
        let d = (p.0 - pts[0].0).powi(2) + (p.1 - pts[0].1).powi(2);
        if d > far_dist {
            far_dist = d;
            far_idx = i;
        }
    }

    // Simplify the two open polylines [0..=far_idx] and [far_idx..=0].
    let mut first: Vec<(f64, f64)> = pts[0..=far_idx].to_vec();
    let mut second: Vec<(f64, f64)> = pts[far_idx..n].to_vec();
    second.push(pts[0]);

    let a = dp_open(&first, epsilon);
    let b = dp_open(&second, epsilon);

    // Stitch: a ends at far_idx, b starts at far_idx — drop the duplicate
    // shared endpoints to form a clean closed ring.
    first.clear();
    let mut result = a;
    if result.last() == b.first() {
        result.extend_from_slice(&b[1..]);
    } else {
        result.extend_from_slice(&b);
    }
    // The closed ring's last point duplicates the first — drop it.
    if result.len() > 1 && result.first() == result.last() {
        result.pop();
    }
    result
}

/// Classic recursive DP on an OPEN polyline.
fn dp_open(pts: &[(f64, f64)], epsilon: f64) -> Vec<(f64, f64)> {
    let n = pts.len();
    if n < 3 {
        return pts.to_vec();
    }
    let start = pts[0];
    let end = pts[n - 1];
    let mut max_dist = 0.0;
    let mut index = 0usize;
    for i in 1..(n - 1) {
        let d = perp_distance(pts[i], start, end);
        if d > max_dist {
            max_dist = d;
            index = i;
        }
    }
    if max_dist > epsilon {
        let mut left = dp_open(&pts[0..=index], epsilon);
        let right = dp_open(&pts[index..n], epsilon);
        left.pop(); // shared vertex at `index`
        left.extend_from_slice(&right);
        left
    } else {
        vec![start, end]
    }
}

/// Perpendicular distance from point `p` to the segment a–b.
fn perp_distance(p: (f64, f64), a: (f64, f64), b: (f64, f64)) -> f64 {
    let dx = b.0 - a.0;
    let dy = b.1 - a.1;
    let len = (dx * dx + dy * dy).sqrt();
    if len < 1e-12 {
        return ((p.0 - a.0).powi(2) + (p.1 - a.1).powi(2)).sqrt();
    }
    ((dx * (a.1 - p.1) - (a.0 - p.0) * dy).abs()) / len
}

// ─── Warp ─────────────────────────────────────────────────────────────────

/// Apply the user-edited corners: deskew the document into a flat rectangle via
/// a projective homography + bilinear sampling, run the chosen enhancement, and
/// save. When `out_path` is `Some`, write there (PNG/JPG by extension) and
/// return that path; when `None`, write a temp PNG (preview) and return it.
///
/// `enhance` ∈ { "color", "grayscale", "bw", "magic" }.
#[tauri::command(async)]
pub fn camscan_warp(
    path: String,
    corners: Vec<CamPoint>,
    enhance: String,
    out_path: Option<String>,
) -> Result<String, String> {
    // Security gate: source must be a real, non-system file. Any user-supplied
    // out_path must pass the write-target gate; the temp preview does not (it's
    // our own file under temp_dir).
    let source = crate::core::safe_path::validate_user_path(&path)?;
    let validated_out: Option<PathBuf> = match &out_path {
        Some(p) => Some(crate::core::safe_path::validate_user_write_target(p)?),
        None => None,
    };

    if corners.len() != 4 {
        return Err(format!("Expected 4 corners, got {}", corners.len()));
    }

    let img = ImageReader::open(&source)
        .map_err(|e| format!("Cannot open: {}", e))?
        .with_guessed_format()
        .map_err(|e| format!("Cannot detect format: {}", e))?
        .decode()
        .map_err(|e| format!("Decode failed: {}", e))?;
    let src: RgbaImage = img.to_rgba8();

    // Source corners arrive already ordered TL/TR/BR/BL: detection orders them
    // (`order_corners` at detect time), the bounds fallback is ordered, and the
    // frontend's editable handles are labeled and never reorder. We TRUST that
    // order here — re-running the sum/diff heuristic would only corrupt a
    // deliberate user arrangement (it mis-classifies strongly-rotated or
    // tall-narrow quads, silently swapping corners → a mirrored/rotated scan
    // from input that was already correct).
    let (tl, tr, br, bl) = (corners[0], corners[1], corners[2], corners[3]);

    // Output size = round(avg of opposite edge lengths).
    let top = dist(tl, tr);
    let bottom = dist(bl, br);
    let left = dist(tl, bl);
    let right = dist(tr, br);
    let out_w = (((top + bottom) * 0.5).round() as i64).max(1) as u32;
    let out_h = (((left + right) * 0.5).round() as i64).max(1) as u32;

    // Clamp the output to a sane ceiling (longest side ≤ MAX_OUT_SIDE, aspect
    // preserved). Output dimensions come from corner *distances*, which are
    // user-editable and unbounded — degenerate or accidentally-scaled corners
    // could otherwise drive a multi-GB allocation. Detection is already bounded
    // by the ~1000 px downscale; this bounds the warp the same way.
    const MAX_OUT_SIDE: u32 = 4000;
    let (out_w, out_h) = {
        let longest = out_w.max(out_h);
        if longest > MAX_OUT_SIDE {
            let s = MAX_OUT_SIDE as f64 / longest as f64;
            (
                ((out_w as f64 * s).round() as u32).max(1),
                ((out_h as f64 * s).round() as u32).max(1),
            )
        } else {
            (out_w, out_h)
        }
    };

    // Homography mapping OUTPUT rectangle corners → SOURCE corners, so for each
    // output pixel we read straight from the source (inverse warp, no holes).
    let dst_rect = [
        CamPoint { x: 0.0, y: 0.0 },
        CamPoint { x: (out_w - 1) as f64, y: 0.0 },
        CamPoint { x: (out_w - 1) as f64, y: (out_h - 1) as f64 },
        CamPoint { x: 0.0, y: (out_h - 1) as f64 },
    ];
    let h = compute_homography(&dst_rect, &[tl, tr, br, bl])
        .ok_or_else(|| "Degenerate corners — cannot compute homography".to_string())?;

    let (src_w, src_h) = (src.width(), src.height());
    let mut out = RgbaImage::new(out_w, out_h);
    for oy in 0..out_h {
        for ox in 0..out_w {
            let (sx, sy) = apply_homography(&h, ox as f64, oy as f64);
            let px = bilinear_sample(&src, sx, sy, src_w, src_h);
            out.put_pixel(ox, oy, px);
        }
    }

    // Enhancement profile.
    let enhanced = match enhance.as_str() {
        "grayscale" => to_grayscale_rgba(&out),
        "bw" => to_bw_rgba(&out),
        "magic" => to_magic_rgba(&out),
        // "color" or anything unknown → leave untouched.
        _ => out,
    };

    // Resolve the output path: validated user target, or a temp PNG preview.
    let output: PathBuf = match validated_out {
        Some(p) => p,
        None => {
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            std::env::temp_dir().join(format!("kil-camscan-{stamp}.png"))
        }
    };

    let ext = output
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_lowercase())
        .unwrap_or_else(|| "png".to_string());

    match ext.as_str() {
        "jpg" | "jpeg" => {
            // JPEG can't store alpha — convert to RGB (mirrors redact.rs).
            let rgb = image::DynamicImage::ImageRgba8(enhanced).to_rgb8();
            rgb.save(&output)
                .map_err(|e| format!("Save failed: {}", e))?;
        }
        _ => {
            enhanced
                .save(&output)
                .map_err(|e| format!("Save failed: {}", e))?;
        }
    }

    Ok(output.to_string_lossy().to_string())
}

/// Euclidean distance between two points.
fn dist(a: CamPoint, b: CamPoint) -> f64 {
    ((a.x - b.x).powi(2) + (a.y - b.y).powi(2)).sqrt()
}

/// Compute the 3×3 projective homography H such that, for each i,
/// `H * [src_x_i, src_y_i, 1]ᵀ ≈ [dst_x_i, dst_y_i, w]`.
///
/// 4-point DLT: solve the 8-unknown linear system (h33 fixed to 1) via
/// nalgebra. `src` are the 4 input points, `dst` the 4 corresponding outputs.
/// Returns `None` if the system is singular (degenerate / collinear corners).
fn compute_homography(src: &[CamPoint; 4], dst: &[CamPoint; 4]) -> Option<Matrix3<f64>> {
    // Build the 8×8 system A·h = b where h = [h11..h32] and h33 = 1.
    let mut a = nalgebra::SMatrix::<f64, 8, 8>::zeros();
    let mut b = nalgebra::SVector::<f64, 8>::zeros();
    for i in 0..4 {
        let (x, y) = (src[i].x, src[i].y);
        let (u, v) = (dst[i].x, dst[i].y);
        let r0 = 2 * i;
        let r1 = 2 * i + 1;
        // u row: x*h11 + y*h12 + h13 - u*x*h31 - u*y*h32 = u
        a[(r0, 0)] = x;
        a[(r0, 1)] = y;
        a[(r0, 2)] = 1.0;
        a[(r0, 6)] = -u * x;
        a[(r0, 7)] = -u * y;
        b[r0] = u;
        // v row: x*h21 + y*h22 + h23 - v*x*h31 - v*y*h32 = v
        a[(r1, 3)] = x;
        a[(r1, 4)] = y;
        a[(r1, 5)] = 1.0;
        a[(r1, 6)] = -v * x;
        a[(r1, 7)] = -v * y;
        b[r1] = v;
    }

    let decomp = a.lu();
    let h = decomp.solve(&b)?;
    // Guard against NaN/inf from a near-singular solve.
    if h.iter().any(|v| !v.is_finite()) {
        return None;
    }

    Some(Matrix3::new(
        h[0], h[1], h[2],
        h[3], h[4], h[5],
        h[6], h[7], 1.0,
    ))
}

/// Apply homography H to an input point (x, y), returning the mapped (x', y')
/// after the perspective divide.
fn apply_homography(h: &Matrix3<f64>, x: f64, y: f64) -> (f64, f64) {
    let v = h * Vector3::new(x, y, 1.0);
    let w = if v.z.abs() < 1e-12 { 1e-12 } else { v.z };
    (v.x / w, v.y / w)
}

/// Bilinear-sample the source RGBA image at floating-point (x, y). Coordinates
/// outside the image are clamped to the edge (no transparent fringe).
fn bilinear_sample(src: &RgbaImage, x: f64, y: f64, w: u32, h: u32) -> Rgba<u8> {
    if w == 0 || h == 0 {
        return Rgba([0, 0, 0, 255]);
    }
    let xc = x.clamp(0.0, (w - 1) as f64);
    let yc = y.clamp(0.0, (h - 1) as f64);
    let x0 = xc.floor() as u32;
    let y0 = yc.floor() as u32;
    let x1 = (x0 + 1).min(w - 1);
    let y1 = (y0 + 1).min(h - 1);
    let fx = xc - x0 as f64;
    let fy = yc - y0 as f64;

    let p00 = src.get_pixel(x0, y0).0;
    let p10 = src.get_pixel(x1, y0).0;
    let p01 = src.get_pixel(x0, y1).0;
    let p11 = src.get_pixel(x1, y1).0;

    let mut out = [0u8; 4];
    for c in 0..4 {
        let top = p00[c] as f64 * (1.0 - fx) + p10[c] as f64 * fx;
        let bot = p01[c] as f64 * (1.0 - fx) + p11[c] as f64 * fx;
        let val = top * (1.0 - fy) + bot * fy;
        out[c] = val.round().clamp(0.0, 255.0) as u8;
    }
    Rgba(out)
}

// ─── Enhancement profiles ──────────────────────────────────────────────────

/// grayscale: luminance, written as RGB (alpha forced opaque).
fn to_grayscale_rgba(img: &RgbaImage) -> RgbaImage {
    RgbaImage::from_fn(img.width(), img.height(), |x, y| {
        let p = img.get_pixel(x, y).0;
        let g = luma(p[0], p[1], p[2]);
        Rgba([g, g, g, 255])
    })
}

/// bw: Otsu threshold on the luminance → black/white (1-bit look as RGB).
fn to_bw_rgba(img: &RgbaImage) -> RgbaImage {
    // Histogram of luma.
    let mut hist = [0u32; 256];
    for p in img.pixels() {
        hist[luma(p.0[0], p.0[1], p.0[2]) as usize] += 1;
    }
    let threshold = otsu_threshold(&hist, (img.width() * img.height()) as u64);
    RgbaImage::from_fn(img.width(), img.height(), |x, y| {
        let p = img.get_pixel(x, y).0;
        let g = luma(p[0], p[1], p[2]);
        let v = if g > threshold { 255 } else { 0 };
        Rgba([v, v, v, 255])
    })
}

/// magic: grayscale + percentile contrast-stretch + a light unsharp mask. The
/// "scanned document" look: bright, even background with crisp text.
fn to_magic_rgba(img: &RgbaImage) -> RgbaImage {
    // 1. To grayscale (GrayImage so we can reuse the histogram-stretch +
    //    Gaussian-blur helpers).
    let mut gray = image::GrayImage::new(img.width(), img.height());
    for (gp, sp) in gray.pixels_mut().zip(img.pixels()) {
        gp.0[0] = luma(sp.0[0], sp.0[1], sp.0[2]);
    }

    // 2. Percentile contrast-stretch (2nd→0, 98th→255), the in-repo pattern
    //    from ocr.rs::normalize_contrast — re-implemented locally to avoid a
    //    cross-module private dependency.
    stretch_contrast(&mut gray);

    // 3. Light unsharp mask: sharpened = gray + amount*(gray − blur(gray)).
    let blurred = imageproc::filter::gaussian_blur_f32(&gray, 1.2);
    let amount = 0.8f32;
    let mut out = RgbaImage::new(img.width(), img.height());
    for ((op, gp), bp) in out
        .pixels_mut()
        .zip(gray.pixels())
        .zip(blurred.pixels())
    {
        let g = gp.0[0] as f32;
        let b = bp.0[0] as f32;
        let sharp = (g + amount * (g - b)).round().clamp(0.0, 255.0) as u8;
        *op = Rgba([sharp, sharp, sharp, 255]);
    }
    out
}

/// BT.601 luma.
#[inline]
fn luma(r: u8, g: u8, b: u8) -> u8 {
    (0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32)
        .round()
        .clamp(0.0, 255.0) as u8
}

/// Percentile contrast-stretch on a grayscale image in place: map the 2nd
/// percentile to 0 and the 98th to 255. Mirrors `ocr.rs::normalize_contrast`.
fn stretch_contrast(gray: &mut image::GrayImage) {
    let mut hist = [0u32; 256];
    for p in gray.pixels() {
        hist[p.0[0] as usize] += 1;
    }
    let total: u64 = (gray.width() as u64) * (gray.height() as u64);
    if total == 0 {
        return;
    }
    let lo_target = (total as f64 * 0.02) as u64;
    let hi_target = (total as f64 * 0.98) as u64;
    let mut acc = 0u64;
    let mut lo = 0u8;
    for (i, &c) in hist.iter().enumerate() {
        acc += c as u64;
        if acc >= lo_target {
            lo = i as u8;
            break;
        }
    }
    acc = 0;
    let mut hi = 255u8;
    for (i, &c) in hist.iter().enumerate() {
        acc += c as u64;
        if acc >= hi_target {
            hi = i as u8;
            break;
        }
    }
    if hi <= lo {
        return;
    }
    let range = (hi - lo) as f32;
    for p in gray.pixels_mut() {
        let v = p.0[0];
        p.0[0] = if v <= lo {
            0
        } else if v >= hi {
            255
        } else {
            (((v - lo) as f32 / range) * 255.0).round() as u8
        };
    }
}

/// Otsu's method: pick the luma threshold that maximizes between-class
/// variance. Returns the threshold value in 0..=255.
fn otsu_threshold(hist: &[u32; 256], total: u64) -> u8 {
    if total == 0 {
        return 127;
    }
    let total_f = total as f64;
    let sum_all: f64 = hist
        .iter()
        .enumerate()
        .map(|(i, &c)| i as f64 * c as f64)
        .sum();

    let mut sum_b = 0.0;
    let mut w_b = 0.0;
    let mut max_var = -1.0;
    let mut threshold = 127u8;
    for (t, &c) in hist.iter().enumerate() {
        w_b += c as f64;
        if w_b == 0.0 {
            continue;
        }
        let w_f = total_f - w_b;
        if w_f == 0.0 {
            break;
        }
        sum_b += t as f64 * c as f64;
        let m_b = sum_b / w_b;
        let m_f = (sum_all - sum_b) / w_f;
        let var_between = w_b * w_f * (m_b - m_f) * (m_b - m_f);
        if var_between > max_var {
            max_var = var_between;
            threshold = t as u8;
        }
    }
    threshold
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx_eq(a: f64, b: f64, eps: f64) -> bool {
        (a - b).abs() <= eps
    }

    #[test]
    fn homography_identity_square_to_square() {
        // Mapping a unit-ish square onto itself must be (numerically) the
        // identity: every input point maps back to itself.
        let sq = [
            CamPoint { x: 0.0, y: 0.0 },
            CamPoint { x: 100.0, y: 0.0 },
            CamPoint { x: 100.0, y: 100.0 },
            CamPoint { x: 0.0, y: 100.0 },
        ];
        let h = compute_homography(&sq, &sq).expect("non-singular");
        for p in &sq {
            let (x, y) = apply_homography(&h, p.x, p.y);
            assert!(approx_eq(x, p.x, 1e-6), "x {} vs {}", x, p.x);
            assert!(approx_eq(y, p.y, 1e-6), "y {} vs {}", y, p.y);
        }
        // An interior point also maps to itself.
        let (cx, cy) = apply_homography(&h, 50.0, 50.0);
        assert!(approx_eq(cx, 50.0, 1e-6));
        assert!(approx_eq(cy, 50.0, 1e-6));
    }

    #[test]
    fn homography_known_skew_maps_corners() {
        // Map a unit output rectangle onto a known skewed quad; verify each
        // output corner lands exactly on its source corner, and the center
        // maps to the quad's centroid-ish interior (finite, inside bbox).
        let dst_rect = [
            CamPoint { x: 0.0, y: 0.0 },
            CamPoint { x: 200.0, y: 0.0 },
            CamPoint { x: 200.0, y: 300.0 },
            CamPoint { x: 0.0, y: 300.0 },
        ];
        // A trapezoid (perspective-skewed document).
        let skew = [
            CamPoint { x: 40.0, y: 30.0 },   // TL
            CamPoint { x: 260.0, y: 60.0 },  // TR
            CamPoint { x: 230.0, y: 350.0 }, // BR
            CamPoint { x: 70.0, y: 320.0 },  // BL
        ];
        let h = compute_homography(&dst_rect, &skew).expect("non-singular");

        for (rect, src) in dst_rect.iter().zip(skew.iter()) {
            let (x, y) = apply_homography(&h, rect.x, rect.y);
            assert!(approx_eq(x, src.x, 1e-6), "corner x {} vs {}", x, src.x);
            assert!(approx_eq(y, src.y, 1e-6), "corner y {} vs {}", y, src.y);
        }

        // Center of the output rect maps inside the skewed quad's bbox.
        let (cx, cy) = apply_homography(&h, 100.0, 150.0);
        assert!(cx.is_finite() && cy.is_finite());
        assert!(cx > 40.0 && cx < 260.0, "center x {} out of bbox", cx);
        assert!(cy > 30.0 && cy < 350.0, "center y {} out of bbox", cy);
    }

    #[test]
    fn order_corners_sorts_tl_tr_br_bl() {
        // Feed the corners scrambled; expect TL/TR/BR/BL out.
        let scrambled = [
            CamPoint { x: 100.0, y: 100.0 }, // BR
            CamPoint { x: 0.0, y: 0.0 },     // TL
            CamPoint { x: 0.0, y: 100.0 },   // BL
            CamPoint { x: 100.0, y: 0.0 },   // TR
        ];
        let o = order_corners(&scrambled);
        assert_eq!((o[0].x, o[0].y), (0.0, 0.0), "TL");
        assert_eq!((o[1].x, o[1].y), (100.0, 0.0), "TR");
        assert_eq!((o[2].x, o[2].y), (100.0, 100.0), "BR");
        assert_eq!((o[3].x, o[3].y), (0.0, 100.0), "BL");
    }

    #[test]
    fn inset_bounds_insets_by_fraction() {
        let c = inset_bounds(1000, 500, 0.02);
        assert!(approx_eq(c[0].x, 20.0, 1e-9));
        assert!(approx_eq(c[0].y, 10.0, 1e-9));
        assert!(approx_eq(c[2].x, 980.0, 1e-9));
        assert!(approx_eq(c[2].y, 490.0, 1e-9));
    }

    #[test]
    fn douglas_peucker_reduces_square_to_four_corners() {
        // A square sampled with many points along each edge must reduce to 4
        // corners under DP. Build a 100×100 closed ring with midpoints.
        let mut pts = Vec::new();
        for i in 0..=10 {
            pts.push((i as f64 * 10.0, 0.0));
        }
        for i in 1..=10 {
            pts.push((100.0, i as f64 * 10.0));
        }
        for i in 1..=10 {
            pts.push((100.0 - i as f64 * 10.0, 100.0));
        }
        for i in 1..10 {
            pts.push((0.0, 100.0 - i as f64 * 10.0));
        }
        let approx = douglas_peucker(&pts, 0.02 * polygon_perimeter(&pts));
        assert_eq!(approx.len(), 4, "square should reduce to 4 corners, got {:?}", approx);
        assert!(is_convex(&approx));
    }
}
