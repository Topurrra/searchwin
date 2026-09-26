pub mod activity_log;
pub mod archive;
pub mod automation;
pub mod cleaner;
pub mod browser_search;
pub mod clipboard_actions;
pub mod clipboard_history;
pub mod cron_tasks;
pub mod crypto_tool;
pub mod diff_merge;
pub mod doc_metadata;
pub mod doc_unlock;
pub mod halcyon;
pub mod encoders;
pub mod error_logs;
pub mod file_manager;
pub mod files;
pub mod ffmpeg;
// Test-covered egress seam staged until the optional BYOK transport exists.
#[allow(dead_code)]
pub mod firewall;
pub mod format;
pub mod frecency;
pub mod hash;
pub mod image_extra_tools;
pub mod image_tools;
pub mod launcher_icons;
pub mod live_grep;
pub mod local_db;
pub mod media_utility;
pub mod metadata;
pub mod my_shell;
pub mod notes;
pub mod notes_pdf;
pub mod duplicate_cache;
// Quality Pass Wave 1 / DF-4 (2026-05-29): inline visual preview for
// every duplicate row — images, audio, video, code, text, PDF, DOCX,
// XLSX, PPTX, ODT, RTF, and a hex fallback for everything else.
pub mod duplicate_preview;
#[cfg(windows)]
pub mod mft_walker;
pub mod ocr;
pub mod ocr_cache;
// Quality Pass Wave 1 / DF-1 (2026-05-29): perceptual image hashing
// powering DuplicateFinder's "Similar images" mode. Pair of modules —
// `perceptual_hash` is the dHash + DSU grouping logic, `perceptual_cache`
// is the (path, mtime) → hash redb cache that mirrors the OCR cache pattern.
pub mod perceptual_cache;
pub mod perceptual_hash;
pub mod preferences;
pub mod privacy_audit;
pub mod packaged_apps;
pub mod processes;
pub mod qr;
pub mod quick_actions;
pub mod rank;
// Semantic search (beta, 2026-07-01): `embedding` = bundled MiniLM ONNX
// encoder (behind the `semantic` feature), `vector_cache` = the per-doc
// vector sidecar store it feeds. Both no-op when the feature/toggle is off.
pub mod embedding;
pub mod vector_cache;
pub mod camscan;
pub mod redact;
pub mod regex_tools;
pub mod reminders;
pub mod search;
// Screen Recorder (WGC capture + ffmpeg encode). Windows-only, opt-in feature.
#[cfg(feature = "screenrec")]
pub mod screen_recorder;
// Always-compiled Tauri command surface for the recorder (errors without the feature).
pub mod screenrec_cmds;
mod scheduler_scope;
pub mod sensitive_allowlist;
pub mod sensitive_scan;
pub mod shredder;
pub mod snippets;
pub mod snippet_expand;
pub mod secure_kv;
pub mod spreadsheet;
pub mod sql_format;
pub mod ssh_keys;
pub mod system_info;
pub mod text_extract;
pub mod time_tracker;
#[cfg(windows)]
pub mod voice;
#[cfg(windows)]
pub mod voice_input;
// voice_scripts is plain file IO + a file watcher — cross-platform,
// unlike the Windows-only voice engine modules above.
pub mod voice_scripts;
#[cfg(windows)]
pub mod voice_ui;
pub mod window_control;
pub mod windows_hardening;
pub mod word_pdf;
