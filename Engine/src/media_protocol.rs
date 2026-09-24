//! `kilmedia://` — off-main-thread media streaming for the command-palette
//! audio/video preview.
//!
//! Tauri's built-in `asset:` protocol serves files *synchronously inside the
//! protocol closure* — i.e. on the WebView/UI thread — and for a range-less
//! request it reads the WHOLE file into memory (see tauri 2.11
//! `protocol/asset.rs` ~L217). Previewing a multi-GB movie therefore froze
//! every window until that read finished.
//!
//! This protocol does the same job but:
//!   1. hands the file work to a worker thread, so the protocol closure returns
//!      immediately and the UI thread never blocks; and
//!   2. always responds with a single bounded chunk — a range-less GET is
//!      treated as `bytes=0-`, so we never read or allocate more than `CHUNK`
//!      bytes regardless of file size. The webview range-requests the rest.
//!
//! Scope: the palette's `<video>`/`<audio>`/`<img>` and PDF previews. A
//! range-less GET we can satisfy whole (small file) is served as `200` — what
//! `<img>` and the PDF viewer expect for the initial load — while range
//! requests and capped range-less GETs of large files stream bounded `206`.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use tauri::http::header::{
    ACCEPT_RANGES, ACCESS_CONTROL_EXPOSE_HEADERS, CACHE_CONTROL, CONTENT_LENGTH, CONTENT_RANGE,
    CONTENT_TYPE, ETAG, IF_NONE_MATCH, RANGE,
};
use tauri::http::{Method, Request, Response, StatusCode};
use tauri::{Runtime, UriSchemeContext, UriSchemeResponder};

/// URI scheme name. Frontend builds URLs with `convertFileSrc(path, SCHEME)`.
pub const SCHEME: &str = "kilmedia";

/// Bytes served per response. Mirrors Tauri's asset-protocol `MAX_LEN` so the
/// streaming cadence is identical; the webview fetches the rest via ranges.
const CHUNK: u64 = 1000 * 1024;

/// Async protocol entry point. The actual read runs on a worker thread; this
/// returns instantly so the UI thread is never blocked by a large file.
pub fn handle<R: Runtime>(
    _ctx: UriSchemeContext<'_, R>,
    request: Request<Vec<u8>>,
    responder: UriSchemeResponder,
) {
    std::thread::spawn(move || {
        responder.respond(build_response(&request));
    });
}

fn error(status: u16) -> Response<Vec<u8>> {
    Response::builder()
        .status(status)
        .header("Access-Control-Allow-Origin", "*")
        .body(Vec::new())
        .unwrap()
}

fn build_response(request: &Request<Vec<u8>>) -> Response<Vec<u8>> {
    // The path is percent-encoded after the leading `/` (built by
    // `convertFileSrc`). `urlencoding::decode` only resolves `%XX`, which is
    // exactly what convertFileSrc emits (spaces → `%20`).
    let raw = request.uri().path();
    let encoded = raw.strip_prefix('/').unwrap_or(raw);
    let path = match urlencoding::decode(encoded) {
        Ok(decoded) => decoded.into_owned(),
        Err(_) => return error(400),
    };

    let mut file = match File::open(&path) {
        Ok(f) => f,
        Err(e) => {
            return error(if e.kind() == std::io::ErrorKind::NotFound {
                404
            } else {
                403
            })
        }
    };
    let meta = match file.metadata() {
        Ok(m) => m,
        Err(_) => return error(500),
    };
    let len = meta.len();
    if len == 0 {
        return error(404);
    }
    // Cache validator from (mtime, size): cheap, and changes whenever the file
    // does — lets the webview cache previews and revalidate them correctly.
    let mtime_ms = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let etag = etag_for(mtime_ms, len);

    let mime = media_mime(&path);

    // Determine the byte window. A range-less GET is treated as `bytes=0-` so
    // the response is always bounded — no whole-file read, even off-thread.
    let range = request
        .headers()
        .get(RANGE)
        .and_then(|v| v.to_str().ok())
        .and_then(|h| parse_first_range(h, len));
    let (start, requested_end) = range.unwrap_or((0, len - 1));

    if start >= len {
        return Response::builder()
            .status(StatusCode::RANGE_NOT_SATISFIABLE)
            .header(CONTENT_RANGE, format!("bytes */{len}"))
            .header("Access-Control-Allow-Origin", "*")
            .body(Vec::new())
            .unwrap();
    }

    // Conditional GET: if the webview already holds this exact version (same
    // ETag) and isn't asking for a byte range, tell it to reuse its cached copy.
    // Skips re-reading and re-transferring the file — the big win when revisiting
    // a large scanned PDF or image preview.
    if range.is_none() {
        if let Some(matched) = request
            .headers()
            .get(IF_NONE_MATCH)
            .and_then(|v| v.to_str().ok())
        {
            if matched.split(',').any(|tag| tag.trim() == etag) {
                return Response::builder()
                    .status(StatusCode::NOT_MODIFIED)
                    .header(ETAG, &etag)
                    .header(CACHE_CONTROL, "no-cache")
                    .header("Access-Control-Allow-Origin", "*")
                    .body(Vec::new())
                    .unwrap();
            }
        }
    }

    // A HEAD probe just needs the size + range support, never a body.
    if request.method() == Method::HEAD {
        return Response::builder()
            .status(StatusCode::OK)
            .header(CONTENT_TYPE, mime)
            .header(ACCEPT_RANGES, "bytes")
            .header(CONTENT_LENGTH, len)
            .header(ETAG, &etag)
            .header(CACHE_CONTROL, "no-cache")
            .header("Access-Control-Allow-Origin", "*")
            .body(Vec::new())
            .unwrap();
    }

    // Streaming media (video/audio) is always chunked — the player range-requests
    // the rest. Images and the PDF viewer instead expect the COMPLETE resource on
    // a range-less load; a capped chunk yields a truncated file that the viewer
    // reports as "cannot be opened" (notably large scanned PDFs). Serve those
    // whole — off-thread, so even a big scan never blocks the UI.
    let streamable = mime.starts_with("video/") || mime.starts_with("audio/");
    let end = if range.is_none() && !streamable {
        len - 1
    } else {
        requested_end.min(len - 1).min(start + CHUNK - 1)
    };
    let nbytes = end + 1 - start;

    let mut buf = Vec::with_capacity(nbytes as usize);
    if file.seek(SeekFrom::Start(start)).is_err()
        || file.take(nbytes).read_to_end(&mut buf).is_err()
    {
        return error(500);
    }

    let builder = Response::builder()
        .header(CONTENT_TYPE, mime)
        .header(ACCEPT_RANGES, "bytes")
        .header(CONTENT_LENGTH, nbytes)
        .header(ETAG, &etag)
        .header(CACHE_CONTROL, "no-cache")
        .header("Access-Control-Allow-Origin", "*");

    // A range-less GET that we satisfy in full → plain `200` (what `<img>` and
    // the PDF viewer expect). A real range request, or a range-less GET we had
    // to cap (large file), → `206` + Content-Range so the webview learns the
    // full size and streams the remainder via further range requests.
    let serves_whole_file = start == 0 && end + 1 == len;
    if range.is_none() && serves_whole_file {
        builder.status(StatusCode::OK).body(buf).unwrap()
    } else {
        builder
            .status(StatusCode::PARTIAL_CONTENT)
            .header(CONTENT_RANGE, format!("bytes {start}-{end}/{len}"))
            .header(ACCESS_CONTROL_EXPOSE_HEADERS, "content-range")
            .body(buf)
            .unwrap()
    }
}

/// Parse the first range from a `Range: bytes=...` header. Returns
/// `(start, end_inclusive)`. Handles `bytes=start-`, `bytes=start-end`, and the
/// suffix form `bytes=-N` (last N bytes). `len` is needed for the suffix case.
fn parse_first_range(header: &str, len: u64) -> Option<(u64, u64)> {
    let spec = header.trim().strip_prefix("bytes=")?;
    let first = spec.split(',').next()?.trim();
    let (lhs, rhs) = first.split_once('-')?;
    let (lhs, rhs) = (lhs.trim(), rhs.trim());

    if lhs.is_empty() {
        // Suffix range: `bytes=-N` → the last N bytes.
        let n: u64 = rhs.parse().ok()?;
        if n == 0 {
            return None;
        }
        return Some((len.saturating_sub(n), len - 1));
    }

    let start: u64 = lhs.parse().ok()?;
    let end = if rhs.is_empty() {
        len - 1
    } else {
        rhs.parse::<u64>().ok()?
    };
    Some((start, end))
}

/// MIME type for a media path, by extension. Covers every audio/video format
/// the natural-language search categories expand to; unknown → octet-stream
/// (the preview's `onerror` card handles codecs the webview can't decode).
fn media_mime(path: &str) -> &'static str {
    let ext = Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        // video
        "mp4" | "m4v" => "video/mp4",
        "webm" => "video/webm",
        "ogv" => "video/ogg",
        "mkv" => "video/x-matroska",
        "avi" | "divx" => "video/x-msvideo",
        "mov" => "video/quicktime",
        "wmv" => "video/x-ms-wmv",
        "asf" => "video/x-ms-asf",
        "flv" => "video/x-flv",
        "f4v" => "video/x-f4v",
        "mpg" | "mpeg" => "video/mpeg",
        "3gp" => "video/3gpp",
        "ts" | "m2ts" | "mts" => "video/mp2t",
        "vob" => "video/dvd",
        // audio
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "flac" => "audio/flac",
        "m4a" | "m4b" => "audio/mp4",
        "aac" => "audio/aac",
        "ogg" | "oga" => "audio/ogg",
        "opus" => "audio/opus",
        "wma" => "audio/x-ms-wma",
        "aiff" | "aif" => "audio/aiff",
        "mid" | "midi" => "audio/midi",
        // images
        "jpg" | "jpeg" | "jfif" => "image/jpeg",
        "png" => "image/png",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "tif" | "tiff" => "image/tiff",
        "heic" | "heif" => "image/heic",
        "avif" => "image/avif",
        "ico" => "image/x-icon",
        "svg" => "image/svg+xml",
        "jxl" => "image/jxl",
        // documents
        "pdf" => "application/pdf",
        _ => "application/octet-stream",
    }
}

/// Cache validator for a file from its `(mtime_ms, size)`. Quoted per the
/// `ETag` grammar; changes whenever the file is modified or resized, so a
/// cached preview is reused only while the bytes are unchanged.
fn etag_for(mtime_ms: u128, len: u64) -> String {
    format!("\"{mtime_ms}-{len}\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn range_header_forms_parse() {
        assert_eq!(parse_first_range("bytes=0-499", 10_000), Some((0, 499)));
        assert_eq!(parse_first_range("bytes=500-", 10_000), Some((500, 9_999)));
        assert_eq!(parse_first_range("bytes=-200", 10_000), Some((9_800, 9_999)));
        // Multi-range: we serve the first range only.
        assert_eq!(parse_first_range("bytes=0-99,200-299", 10_000), Some((0, 99)));
        assert_eq!(parse_first_range("nonsense", 10_000), None);
    }

    #[test]
    fn mime_maps_common_media() {
        assert_eq!(media_mime("C:/x/clip.mp4"), "video/mp4");
        assert_eq!(media_mime("C:/x/song.mp3"), "audio/mpeg");
        assert_eq!(media_mime("C:/x/voice.OPUS"), "audio/opus");
        assert_eq!(media_mime("C:/x/movie.mkv"), "video/x-matroska");
        assert_eq!(media_mime("C:/x/pic.JPG"), "image/jpeg");
        assert_eq!(media_mime("C:/x/icon.png"), "image/png");
        assert_eq!(media_mime("C:/x/scan.pdf"), "application/pdf");
        assert_eq!(media_mime("C:/x/unknown.zzz"), "application/octet-stream");
    }

    #[test]
    fn etag_changes_with_mtime_or_size() {
        assert_eq!(etag_for(1000, 50), "\"1000-50\"");
        assert_ne!(etag_for(1000, 50), etag_for(1001, 50)); // file modified
        assert_ne!(etag_for(1000, 50), etag_for(1000, 51)); // file resized
    }
}
