//! Search's engine: KeepItLocal Workspace's Rust core, headless.
//!
//! The commands are Workspace's, unchanged. What changed is around them:
//! there is no Tauri app, no window, no tray. The browser starts this
//! process when it first needs it, talks to it over a pipe (`server`), and
//! owns every window and hotkey itself (`shell`). A small stand-in for the
//! Tauri API the commands use lives in `compat/tauri`.

mod commands;
mod core;
mod server;
mod shell;

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Manager};

pub use shell::{set_tray_listening, BackendInitIssues, BusyFlag, CancelFlag};
pub(crate) use shell::restart_keepitlocal_impl;
use shell::*;
use commands::{
    activity_log::{clear_activity, list_activity, record_activity},
    archive::{cancel_archive_operation, create_archive, extract_archive, inspect_archive},
    automation::{
        add_automation_activity, clear_automation_activity, get_automation_activity,
        get_automation_recipes, open_automation_data_folder, save_automation_recipes,
    },
    browser_search::{clear_browser_snapshot, search_browser, warm_browser_search},
    camscan::{camscan_detect_corners, camscan_warp},
    cleaner::{analyze_system_cleaner, cancel_cleaner_operation, clean_system_cache},
    clipboard_actions::run_clipboard_action,
    clipboard_history::{
        clear_clipboard_history, copy_clipboard_entry_to_clipboard, delete_clipboard_entries,
        delete_clipboard_entry, get_clipboard_exclusions, get_clipboard_history,
        get_clipboard_image_retention_days, get_clipboard_images_enabled, get_clipboard_paused,
        get_clipboard_retention_days, get_clipboard_text, label_clipboard_entry,
        paste_clipboard_entry, paste_snippet_text, pin_clipboard_entries, pin_clipboard_entry,
        reset_clipboard_exclusions_to_defaults, set_clipboard_exclusions,
        set_clipboard_image_retention_days, set_clipboard_images_enabled, set_clipboard_paused,
        set_clipboard_retention_days, start_clipboard_listener, take_clipboard_recovery_notice,
        type_out_text,
    },
    cron_tasks::{create_cron_task, delete_cron_task, list_cron_tasks, run_cron_task_now},
    crypto_tool::{
        cancel_crypto_operation, crypto_decrypt_file, crypto_decrypt_text, crypto_encrypt_file,
        crypto_encrypt_text, crypto_inspect_file,
    },
    diff_merge::{
        cancel_diff_operation, diff_read_text, file_unified_diff, folder_diff, sync_folders,
        three_way_merge,
    },
    doc_metadata::strip_doc_metadata,
    doc_unlock::remove_docx_password,
    duplicate_preview::preview_duplicate_file,
    encoders::encode_decode,
    error_logs::{clear_log_entries, export_log_text, get_log_folder, list_log_entries, log_event},
    ffmpeg::{ffmpeg_set_path, ffmpeg_status},
    file_manager::{
        fm_backup, fm_cancel, fm_copy, fm_delete_to_recycle, fm_dir_size, fm_list_dir, fm_make_dir,
        fm_move, fm_path_suggestions, fm_rename,
    },
    files::{
        apply_bulk_rename, cancel_duplicate_scan, find_duplicate_files, get_file_recovery_state,
        list_folder_children, move_duplicate_files, preview_bulk_rename, undo_bulk_rename,
        undo_duplicate_move,
    },
    format::convert_format,
    frecency::{get_frecency_boosts, get_recent_items, record_frecency_launch},
    halcyon::{halcyon_probe, halcyon_reset},
    hash::{cancel_hash_operation, compute_hashes},
    image_extra_tools::{cancel_image_extra_operation, generate_favicons, watermark_images},
    image_tools::{
        background_removal_model_status, cancel_image_operation, compress_images, convert_images,
        crop_images, download_background_removal_model, images_to_base64, remove_image_background,
        resize_images, run_image_automation,
    },
    launcher_icons::ensure_launcher_icon,
    live_grep::{cancel_live_grep, live_grep_in_folders},
    media_utility::{cancel_media_operation, media_compress_video, media_extract_audio},
    metadata::strip_metadata,
    my_shell::{cancel_my_shell_command, run_my_shell_command},
    notes::{
        copy_note_asset, create_note, create_note_folder, delete_note, delete_note_folder,
        delete_trashed_note, diff_note_revision, empty_note_trash, get_note_attachment,
        get_notes_dir, list_note_folders, list_note_link_sources, list_note_revisions,
        list_note_templates, list_notes, list_trashed_notes, move_note, open_note_templates_folder,
        open_notes_folder, open_or_create_daily_note, read_note, read_note_revision,
        read_note_template, rename_note_folder, restore_note_revision, restore_trashed_note,
        search_note_bodies, write_note,
    },
    notes_pdf::{
        notes_export_docx, notes_export_html, notes_export_markdown, notes_export_styled_pdf,
    },
    preferences::{
        export_app_data, get_app_storage_paths, get_storage_insights, import_app_data,
        load_app_settings, load_enabled_tool_packs, load_onboarding_state, load_profiles_state,
        reset_all_data, save_app_settings, save_enabled_tool_packs, save_onboarding_state,
        save_profiles_state,
    },
    privacy_audit::{
        audit_browser_extensions, audit_dev_secrets, audit_hosts_file, audit_mic_camera,
        audit_outbound_connections, audit_scheduled_tasks, audit_startup_programs,
        audit_unencrypted_pii, open_privacy_setting, privacy_get_acknowledged,
        privacy_set_acknowledged, reveal_in_explorer,
    },
    processes::{kill_process, launch_targets_running, list_processes},
    qr::{generate_qr, save_qr_png, save_qr_svg},
    quick_actions::{
        check_app_available, evaluate_quick_query, execute_system_command, list_system_commands,
        open_external_url,
    },
    redact::redact_image,
    regex_tools::regex_from_examples,
    reminders::{create_reminder_task, delete_reminder_task, reconcile_reminder_tasks},
    screenrec_cmds::{
        screenrec_close_redact_selector, screenrec_close_region_selector, screenrec_close_toolbar,
        screenrec_export_gif, screenrec_list_windows, screenrec_open_redact_selector,
        screenrec_open_region_selector, screenrec_open_toolbar, screenrec_pause, screenrec_resume,
        screenrec_start, screenrec_status, screenrec_stop,
    },
    search::{
        cancel_file_search_index_build, get_file_search_status, launch_cached_target,
        list_logical_drives, open_search_result_path, prewarm_search_engines, read_file_preview,
        refresh_launch_target_cache, save_file_search_index_options,
        save_file_search_rebuild_schedule, search_file_contents, search_launch_targets,
        search_local_files, start_content_search_index, start_file_search_index,
        start_file_search_scheduler, start_filename_search_index, stop_file_search_index_watcher,
        update_notes_index,
    },
    secure_kv::{secure_kv_get, secure_kv_set},
    sensitive_allowlist::{
        count_dismissed_findings, dismiss_finding, is_finding_dismissed, restore_allowlist,
        undismiss_finding,
    },
    sensitive_scan::{preview_file_findings, scan_text_for_findings},
    shredder::{cancel_operation, shred_files, wipe_free_space},
    snippet_expand::{set_snippet_autoexpand_enabled, sync_snippet_expand_data},
    snippets::{
        create_snippet, delete_snippet, list_snippets, preview_snippet_expansion,
        record_snippet_use, update_snippet,
    },
    spreadsheet::{
        cancel_spreadsheet_operation, clean_csv_file, collect_csv_sources, collect_excel_sources,
        csv_to_excel, csv_to_json, excel_to_csv, inspect_spreadsheet, merge_csv_files,
        preview_csv_cleanup, split_csv_files,
    },
    sql_format::{analyze_sql, format_sql},
    ssh_keys::{
        ssh_delete_key, ssh_dir_path, ssh_generate_key, ssh_import_key, ssh_list_keys,
        ssh_read_config, ssh_read_known_hosts, ssh_remove_known_host, ssh_write_config,
    },
    system_info::system_info,
    time_tracker::{
        time_tracker_get_config, time_tracker_pause, time_tracker_range, time_tracker_set_config,
        time_tracker_status, time_tracker_wipe,
    },
    window_control::{focus_window, list_windows, toggle_window},
    windows_hardening::{
        apply_hardening_tweak, export_hardening_backup, read_hardening_state, revert_all_hardening,
        revert_hardening_tweak,
    },
    word_pdf::{
        cancel_word_markdown_operation, cancel_word_text_operation, collect_word_docx_sources,
        read_docx_markdown, word_to_markdown, word_to_text,
    },
};

// Voice-to-text is Windows-only (uses WinRT Media.SpeechRecognition).
// Imported separately so the cfg gate stays clean. On non-Windows
// targets we expose stubs that return a clear error so the frontend
// always has the same command surface to call into.
#[cfg(windows)]
use commands::voice::{
    voice_cancel_recognize, voice_check_availability, voice_download_model,
    voice_list_downloadable_models, voice_list_installed_models, voice_recognize_once,
    voice_release_models, voice_set_vad_enabled, voice_start_continuous, voice_stop_continuous,
};
#[cfg(windows)]
use commands::voice_input::{
    get_foreground_window_info, voice_get_foreground_app, voice_mouse_click, voice_mouse_move,
    voice_mouse_scroll, voice_mouse_warp, voice_send_keystroke, voice_window_action,
};
#[cfg(windows)]
use commands::voice_ui::{voice_get_ui_elements, voice_list_ui_elements};
// Cross-platform — user-authored voice command files (#21).
use commands::voice_scripts::{
    voice_get_command_overrides, voice_open_user_commands_file, voice_read_user_commands,
    voice_set_command_overrides, voice_user_commands_path, voice_watch_user_commands,
    voice_write_user_commands,
};

#[cfg(not(windows))]
#[tauri::command]
async fn voice_check_availability() -> Result<serde_json::Value, String> {
    Ok(serde_json::json!({
        "available": false,
        "hint": "Voice recognition is currently Windows-only.",
        "defaultLanguage": null,
    }))
}

#[cfg(not(windows))]
#[tauri::command]
async fn voice_recognize_once(
    _model_path: Option<String>,
    _source: Option<String>,
) -> Result<serde_json::Value, String> {
    Err("Voice recognition is currently Windows-only.".to_string())
}

#[cfg(not(windows))]
#[tauri::command]
fn voice_cancel_recognize() {}

#[cfg(not(windows))]
#[tauri::command]
async fn voice_start_continuous(
    _model_path: Option<String>,
    _source: Option<String>,
    _grammar: Option<Vec<String>>,
) -> Result<(), String> {
    Err("Voice recognition is currently Windows-only.".to_string())
}

#[cfg(not(windows))]
#[tauri::command]
fn voice_stop_continuous() {}

#[cfg(not(windows))]
#[tauri::command]
fn voice_set_vad_enabled(_enabled: bool) {}

#[cfg(not(windows))]
#[tauri::command]
fn voice_release_models() {}

#[cfg(not(windows))]
#[tauri::command]
fn voice_list_downloadable_models() -> Vec<serde_json::Value> {
    Vec::new()
}

#[cfg(not(windows))]
#[tauri::command]
async fn voice_download_model(_model_id: String, _target_dir: String) -> Result<String, String> {
    Err("Voice model downloads are currently Windows-only.".to_string())
}

#[cfg(not(windows))]
#[tauri::command]
fn voice_list_installed_models() -> Vec<serde_json::Value> {
    Vec::new()
}

#[cfg(not(windows))]
#[tauri::command]
fn voice_send_keystroke(_key: String, _modifiers: Vec<String>) -> Result<(), String> {
    Err("Voice input emulation is currently Windows-only.".to_string())
}

#[cfg(not(windows))]
#[tauri::command]
fn voice_mouse_click(_button: String, _double: bool) -> Result<(), String> {
    Err("Voice input emulation is currently Windows-only.".to_string())
}

#[cfg(not(windows))]
#[tauri::command]
fn voice_mouse_scroll(_direction: String, _notches: i32) -> Result<(), String> {
    Err("Voice input emulation is currently Windows-only.".to_string())
}

#[cfg(not(windows))]
#[tauri::command]
fn voice_mouse_move(_dx: i32, _dy: i32) -> Result<(), String> {
    Err("Voice input emulation is currently Windows-only.".to_string())
}

#[cfg(not(windows))]
#[tauri::command]
fn voice_mouse_warp(_fx: f64, _fy: f64) -> Result<(), String> {
    Err("Voice input emulation is currently Windows-only.".to_string())
}

#[cfg(not(windows))]
#[tauri::command]
fn voice_window_action(_action: String) -> Result<(), String> {
    Err("Voice window control is currently Windows-only.".to_string())
}

#[cfg(not(windows))]
#[tauri::command]
fn voice_get_foreground_app() -> String {
    String::new()
}

#[cfg(not(windows))]
#[tauri::command]
async fn voice_list_ui_elements() -> Result<usize, String> {
    Err("UI Automation control is currently Windows-only.".to_string())
}

#[cfg(not(windows))]
#[tauri::command]
fn voice_get_ui_elements() -> Vec<serde_json::Value> {
    Vec::new()
}

pub fn maybe_run_index_worker_from_args() -> bool {
    commands::search::maybe_run_index_worker_from_args()
}

pub fn maybe_run_archive_worker_from_args() -> bool {
    commands::archive::maybe_run_archive_worker_from_args()
}

/// Where the engine keeps its data, which pipe it answers on, and how long
/// it waits after the browser leaves. The browser passes all three; the
/// defaults are the real (non-test) Search.
struct Options {
    data_dir: PathBuf,
    pipe: String,
    linger: Duration,
}

impl Options {
    fn from_args() -> Self {
        let mut options = Options {
            data_dir: std::env::var_os("LOCALAPPDATA")
                .map(PathBuf::from)
                .unwrap_or_else(std::env::temp_dir)
                .join("Search")
                .join("Engine"),
            pipe: "search-engine".into(),
            linger: Duration::from_secs(15),
        };
        let mut args = std::env::args().skip(1);
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--data-dir" => {
                    if let Some(dir) = args.next() {
                        options.data_dir = PathBuf::from(dir);
                    }
                }
                "--pipe" => {
                    if let Some(pipe) = args.next() {
                        options.pipe = pipe;
                    }
                }
                "--linger" => {
                    if let Some(secs) = args.next().and_then(|s| s.parse().ok()) {
                        options.linger = Duration::from_secs(secs);
                    }
                }
                _ => {}
            }
        }
        options
    }
}

/// The file index's background work: the rebuild scheduler, and readers
/// warmed so the first query doesn't pay the cold open. Workspace started
/// both at launch; Search starts them when file search is switched on.
#[tauri::command]
fn start_search_services(app: AppHandle) {
    start_file_search_scheduler(app.clone());
    prewarm_search_engines(app);
}

pub fn run() {
    let options = Options::from_args();
    let resources = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(PathBuf::from))
        .unwrap_or_default();
    let app = AppHandle::new(options.data_dir.clone(), resources.clone());

    app.manage(CancelFlag(Arc::new(AtomicBool::new(false))));
    app.manage(BusyFlag(Arc::new(AtomicBool::new(false))));
    app.manage(BackendInitIssues(Arc::new(Mutex::new(Vec::new()))));

    // What Workspace's setup did that still belongs to the engine. Everything
    // else it started (clipboard listener, index scheduler, time tracker) is
    // a command, and the browser starts it when the person turns it on.
    commands::ffmpeg::hydrate_override(&app);
    commands::embedding::set_model_dir(resources.join("embedding-runtime"));
    // Copies of other browsers' history left by a crash are swept first, always.
    commands::browser_search::sweep_orphan_snapshots();

    let handler = tauri::generate_handler![            commands::ocr::ocr_available,
            commands::ocr::ocr_languages,
            commands::ocr::ocr_image,
            commands::ocr::cancel_ocr_operation,
            compute_hashes,
            cancel_hash_operation,
            encode_decode,
            load_app_settings,
            save_app_settings,
            load_enabled_tool_packs,
            save_enabled_tool_packs,
            load_onboarding_state,
            save_onboarding_state,
            load_profiles_state,
            save_profiles_state,
            get_app_storage_paths,
            get_storage_insights,
            reset_all_data,
            export_app_data,
            import_app_data,
            get_backend_init_issues,
            take_db_corruption_notice,
            generate_qr,
            save_qr_png,
            save_qr_svg,
            convert_format,
            strip_metadata,
            shred_files,
            wipe_free_space,
            cancel_operation,
            run_clipboard_action,
            record_activity,
            list_activity,
            clear_activity,
            time_tracker_get_config,
            time_tracker_set_config,
            time_tracker_pause,
            time_tracker_status,
            time_tracker_range,
            time_tracker_wipe,
            ssh_list_keys,
            ssh_generate_key,
            ssh_import_key,
            ssh_delete_key,
            ssh_read_config,
            ssh_write_config,
            ssh_read_known_hosts,
            ssh_remove_known_host,
            ssh_dir_path,
            crypto_encrypt_file,
            crypto_decrypt_file,
            crypto_encrypt_text,
            crypto_decrypt_text,
            crypto_inspect_file,
            cancel_crypto_operation,
            folder_diff,
            file_unified_diff,
            three_way_merge,
            diff_read_text,
            cancel_diff_operation,
            sync_folders,
            log_event,
            list_log_entries,
            clear_log_entries,
            get_log_folder,
            export_log_text,
            list_snippets,
            create_snippet,
            update_snippet,
            delete_snippet,
            record_snippet_use,
            preview_snippet_expansion,
            paste_snippet_text,
            set_snippet_autoexpand_enabled,
            sync_snippet_expand_data,
            secure_kv_get,
            secure_kv_set,
            // Notes — local .ki note storage (Documents/KeepItLocal Notes).
            get_notes_dir,
            copy_note_asset,
            get_note_attachment,
            open_notes_folder,
            list_notes,
            search_note_bodies,
            list_note_link_sources,
            search_browser,
            warm_browser_search,
            clear_browser_snapshot,
            list_note_templates,
            read_note_template,
            open_note_templates_folder,
            list_trashed_notes,
            restore_trashed_note,
            delete_trashed_note,
            empty_note_trash,
            read_note,
            write_note,
            list_note_revisions,
            read_note_revision,
            restore_note_revision,
            diff_note_revision,
            create_note,
            open_or_create_daily_note,
            delete_note,
            list_note_folders,
            create_note_folder,
            rename_note_folder,
            delete_note_folder,
            move_note,
            notes_export_styled_pdf,
            notes_export_markdown,
            notes_export_html,
            notes_export_docx,
            run_my_shell_command,
            cancel_my_shell_command,
            type_out_text,
            camscan_detect_corners,
            camscan_warp,
            screenrec_start,
            screenrec_stop,
            screenrec_status,
            screenrec_pause,
            screenrec_resume,
            screenrec_export_gif,
            screenrec_open_region_selector,
            screenrec_close_region_selector,
            screenrec_open_redact_selector,
            screenrec_close_redact_selector,
            screenrec_list_windows,
            screenrec_open_toolbar,
            screenrec_close_toolbar,
            ffmpeg_status,
            ffmpeg_set_path,
            media_extract_audio,
            media_compress_video,
            cancel_media_operation,
            redact_image,
            regex_from_examples,
            scan_text_for_findings,
            preview_file_findings,
            dismiss_finding,
            undismiss_finding,
            restore_allowlist,
            count_dismissed_findings,
            is_finding_dismissed,
            set_busy,
            start_file_search_index,
            start_content_search_index,
            start_filename_search_index,
            start_search_services,
            update_notes_index,
            read_file_preview,
            list_folder_children,
            read_docx_markdown,
            list_logical_drives,
            // Dual-pane File Manager.
            fm_list_dir,
            fm_dir_size,
            fm_path_suggestions,
            fm_copy,
            fm_move,
            fm_delete_to_recycle,
            fm_rename,
            fm_make_dir,
            fm_backup,
            fm_cancel,
            cancel_file_search_index_build,
            stop_file_search_index_watcher,
            get_file_search_status,
            save_file_search_index_options,
            save_file_search_rebuild_schedule,
            search_local_files,
            search_file_contents,
            search_launch_targets,
            refresh_launch_target_cache,
            launch_cached_target,
            ensure_launcher_icon,
            live_grep_in_folders,
            cancel_live_grep,
            evaluate_quick_query,
            execute_system_command,
            list_system_commands,
            list_processes,
            kill_process,
            launch_targets_running,
            list_windows,
            focus_window,
            toggle_window,
            apply_app_hotkeys_config,
            system_info,
            check_app_available,
            open_external_url,
            record_frecency_launch,
            get_frecency_boosts,
            get_recent_items,
            open_search_result_path,
            create_archive,
            extract_archive,
            inspect_archive,
            strip_doc_metadata,
            remove_docx_password,
            audit_mic_camera,
            audit_unencrypted_pii,
            audit_browser_extensions,
            audit_startup_programs,
            audit_scheduled_tasks,
            audit_outbound_connections,
            audit_hosts_file,
            audit_dev_secrets,
            privacy_get_acknowledged,
            privacy_set_acknowledged,
            open_privacy_setting,
            reveal_in_explorer,
            read_hardening_state,
            apply_hardening_tweak,
            revert_hardening_tweak,
            revert_all_hardening,
            export_hardening_backup,
            inspect_spreadsheet,
            preview_csv_cleanup,
            collect_csv_sources,
            collect_excel_sources,
            clean_csv_file,
            merge_csv_files,
            excel_to_csv,
            csv_to_excel,
            csv_to_json,
            split_csv_files,
            cancel_word_markdown_operation,
            word_to_markdown,
            word_to_text,
            cancel_word_text_operation,
            collect_word_docx_sources,
            format_sql,
            analyze_sql,
            create_reminder_task,
            delete_reminder_task,
            reconcile_reminder_tasks,
            create_cron_task,
            delete_cron_task,
            run_cron_task_now,
            list_cron_tasks,
            voice_check_availability,
            voice_recognize_once,
            voice_cancel_recognize,
            voice_start_continuous,
            voice_stop_continuous,
            voice_set_vad_enabled,
            voice_release_models,
            voice_list_downloadable_models,
            voice_send_keystroke,
            voice_mouse_click,
            voice_mouse_scroll,
            voice_mouse_move,
            voice_mouse_warp,
            voice_window_action,
            voice_get_foreground_app,
            get_foreground_window_info,
            voice_list_ui_elements,
            voice_get_ui_elements,
            voice_user_commands_path,
            voice_read_user_commands,
            voice_write_user_commands,
            voice_open_user_commands_file,
            voice_watch_user_commands,
            voice_get_command_overrides,
            voice_set_command_overrides,
            voice_download_model,
            voice_list_installed_models,
            convert_images,
            compress_images,
            resize_images,
            crop_images,
            remove_image_background,
            background_removal_model_status,
            download_background_removal_model,
            images_to_base64,
            generate_favicons,
            watermark_images,
            find_duplicate_files,
            preview_duplicate_file,
            move_duplicate_files,
            analyze_system_cleaner,
            cancel_cleaner_operation,
            clean_system_cache,
            preview_bulk_rename,
            apply_bulk_rename,
            get_file_recovery_state,
            undo_bulk_rename,
            undo_duplicate_move,
            cancel_duplicate_scan,
            cancel_image_operation,
            cancel_image_extra_operation,
            cancel_spreadsheet_operation,
            cancel_archive_operation,
            run_image_automation,
            get_automation_recipes,
            save_automation_recipes,
            get_automation_activity,
            add_automation_activity,
            clear_automation_activity,
            open_automation_data_folder,
            halcyon_reset,
            halcyon_probe,
            apply_overlay_hotkey_config,
            apply_clipboard_overlay_hotkey_config,
            apply_command_overlay_hotkey_config,
            apply_quick_note_hotkey_config,
            apply_screen_recording_hotkey_config,
            apply_voice_overlay_hotkey_config,
            set_push_to_talk_hotkey,
            suspend_global_shortcuts_for_capture,
            resume_global_shortcuts_after_capture,
            show_main_window_command,
            restart_keepitlocal_command,
            show_overlay_window_command,
            hide_overlay_window_command,
            resize_overlay_window_command,
            show_clipboard_overlay_window_command,
            hide_clipboard_overlay_window_command,
            show_command_window_command,
            hide_command_window_command,
            set_command_window_blur,
            show_quick_note,
            set_quick_note_empty,
            supports_window_corner_rounding,
            show_voice_overlay_window_command,
            hide_voice_overlay_window_command,
            show_mouse_grid_window_command,
            hide_mouse_grid_window_command,
            show_ui_elements_window_command,
            hide_ui_elements_window_command,
            show_focus_break_window_command,
            hide_focus_break_window_command,
            show_focus_glow_window_command,
            hide_focus_glow_window_command,
            show_notify_toast_window_command,
            hide_notify_toast_window_command,
            set_ready,
            welcome_finished,
            start_clipboard_listener,
            get_clipboard_history,
            take_clipboard_recovery_notice,
            clear_clipboard_history,
            pin_clipboard_entry,
            pin_clipboard_entries,
            label_clipboard_entry,
            delete_clipboard_entry,
            delete_clipboard_entries,
            copy_clipboard_entry_to_clipboard,
            paste_clipboard_entry,
            get_clipboard_exclusions,
            set_clipboard_exclusions,
            reset_clipboard_exclusions_to_defaults,
            get_clipboard_paused,
            get_clipboard_text,
            set_clipboard_paused,
            get_clipboard_retention_days,
            set_clipboard_retention_days,
            get_clipboard_images_enabled,
            set_clipboard_images_enabled,
            get_clipboard_image_retention_days,
            set_clipboard_image_retention_days
    ];

    let served = server::serve(
        app.clone(),
        handler,
        server::Options { pipe: options.pipe, linger: options.linger },
    );
    commands::search::stop_all_index_workers();
    commands::archive::stop_all_archive_workers();
    if let Err(error) = served {
        eprintln!("kil-engine: {error}");
        // 2: one is already running — the browser just talks to that one.
        let code = if error.kind() == std::io::ErrorKind::AlreadyExists { 2 } else { 1 };
        std::process::exit(code);
    }
}