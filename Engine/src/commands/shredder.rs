use rand::{distributions::Alphanumeric, rngs::SmallRng, Rng, RngCore, SeedableRng};
use serde::{Deserialize, Serialize};
use std::fs::{File, OpenOptions};
use std::io::{Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use tauri::{Emitter, State, Window};
use walkdir::WalkDir;

use crate::CancelFlag;

#[derive(Serialize, Clone)]
pub struct ShredResult {
    pub path: String,
    pub size: u64,
    pub passes_completed: u32,
    pub success: bool,
    pub error: Option<String>,
    pub kind: String, // "file" | "directory"
}

#[derive(Serialize, Clone)]
pub struct ProgressEvent {
    pub current_file: String,
    pub current_pass: u32,
    pub total_passes: u32,
    pub bytes_done: u64,
    pub bytes_total: u64,
    pub files_done: usize,
    pub files_total: usize,
}

#[derive(Deserialize)]
pub struct ShredOptions {
    pub paths: Vec<String>,
    pub method: String,        // "quick" | "dod3" | "dod7"
    pub randomize_names: bool, // overwrite filename before unlink
    pub recursive: bool,       // for directories
}

#[derive(Clone, Copy)]
enum Pass {
    Zeros,
    Ones,
    Random,
}

fn passes_for(method: &str) -> Vec<Pass> {
    match method {
        "quick" => vec![Pass::Random],
        "dod3" => vec![Pass::Zeros, Pass::Ones, Pass::Random],
        "dod7" => vec![
            Pass::Random,
            Pass::Zeros,
            Pass::Random,
            Pass::Ones,
            Pass::Random,
            Pass::Zeros,
            Pass::Random,
        ],
        _ => vec![Pass::Random],
    }
}

#[tauri::command]
pub async fn shred_files(
    window: Window,
    flag: State<'_, CancelFlag>,
    options: ShredOptions,
) -> Result<Vec<ShredResult>, String> {
    reset_cancel(&flag);
    let passes = passes_for(&options.method);

    let mut all_files: Vec<PathBuf> = Vec::new();
    let mut dir_results: Vec<ShredResult> = Vec::new();
    let mut dirs_to_remove: Vec<PathBuf> = Vec::new();

    for input_path in &options.paths {
        // Validate before touching the filesystem: shredding is destructive,
        // so reject traversal and OS-critical locations up front. The
        // canonical path returned is what we then operate on.
        let p = match crate::core::safe_path::validate_user_path(input_path) {
            Ok(canonical) => canonical,
            Err(error) => {
                dir_results.push(ShredResult {
                    path: input_path.clone(),
                    size: 0,
                    passes_completed: 0,
                    success: false,
                    error: Some(error),
                    kind: "unknown".into(),
                });
                continue;
            }
        };
        match std::fs::metadata(&p) {
            Ok(m) if m.is_file() => all_files.push(p),
            Ok(m) if m.is_dir() => {
                if !options.recursive {
                    dir_results.push(ShredResult {
                        path: input_path.clone(),
                        size: 0,
                        passes_completed: 0,
                        success: false,
                        error: Some("Directory provided but recursive option is off".into()),
                        kind: "directory".into(),
                    });
                    continue;
                }
                for entry in WalkDir::new(&p).into_iter().filter_map(|e| e.ok()) {
                    if entry.file_type().is_file() {
                        all_files.push(entry.path().to_path_buf());
                    }
                }
                dirs_to_remove.push(p);
            }
            Ok(_) => {
                dir_results.push(ShredResult {
                    path: input_path.clone(),
                    size: 0,
                    passes_completed: 0,
                    success: false,
                    error: Some("Not a regular file or directory".into()),
                    kind: "other".into(),
                });
            }
            Err(e) => {
                dir_results.push(ShredResult {
                    path: input_path.clone(),
                    size: 0,
                    passes_completed: 0,
                    success: false,
                    error: Some(format!("Cannot access: {}", e)),
                    kind: "unknown".into(),
                });
            }
        }
    }

    let total_files = all_files.len();
    let mut results: Vec<ShredResult> = Vec::with_capacity(total_files + dir_results.len());

    for (idx, file_path) in all_files.iter().enumerate() {
        if is_cancelled(&flag) {
            results.push(ShredResult {
                path: file_path.to_string_lossy().to_string(),
                size: 0,
                passes_completed: 0,
                success: false,
                error: Some("Cancelled by user".into()),
                kind: "file".into(),
            });
            break;
        }

        let result = shred_one_file(
            &window,
            &flag,
            file_path,
            &passes,
            options.randomize_names,
            idx,
            total_files,
        );
        results.push(result);
    }

    // Only remove directories if not cancelled
    if !is_cancelled(&flag) {
        dirs_to_remove.sort_by_key(|p| std::cmp::Reverse(p.components().count()));
        for dir in dirs_to_remove {
            let mut subdirs: Vec<PathBuf> = WalkDir::new(&dir)
                .into_iter()
                .filter_map(|e| e.ok())
                .filter(|e| e.file_type().is_dir())
                .map(|e| e.path().to_path_buf())
                .collect();
            subdirs.sort_by_key(|p| std::cmp::Reverse(p.components().count()));

            let mut dir_failed = false;
            for sd in subdirs {
                let final_path = if options.randomize_names {
                    rename_to_random(&sd).unwrap_or(sd.clone())
                } else {
                    sd
                };
                if let Err(e) = std::fs::remove_dir(&final_path) {
                    results.push(ShredResult {
                        path: final_path.to_string_lossy().to_string(),
                        size: 0,
                        passes_completed: 0,
                        success: false,
                        error: Some(format!("Failed to remove subdirectory: {}", e)),
                        kind: "directory".into(),
                    });
                    dir_failed = true;
                }
            }

            if !dir_failed {
                results.push(ShredResult {
                    path: dir.to_string_lossy().to_string(),
                    size: 0,
                    passes_completed: passes.len() as u32,
                    success: true,
                    error: None,
                    kind: "directory".into(),
                });
            }
        }
    }

    results.extend(dir_results);
    reset_cancel(&flag);
    Ok(results)
}

fn shred_one_file(
    window: &Window,
    flag: &CancelFlag,
    path: &Path,
    passes: &[Pass],
    randomize_name: bool,
    file_index: usize,
    total_files: usize,
) -> ShredResult {
    let path_str = path.to_string_lossy().to_string();

    let size = match std::fs::metadata(path) {
        Ok(m) => m.len(),
        Err(e) => {
            return ShredResult {
                path: path_str,
                size: 0,
                passes_completed: 0,
                success: false,
                error: Some(format!("Cannot stat: {}", e)),
                kind: "file".into(),
            };
        }
    };

    let mut completed = 0u32;
    let mut current_path = path.to_path_buf();

    if size > 0 {
        for (pass_idx, pass) in passes.iter().enumerate() {
            if is_cancelled(flag) {
                return ShredResult {
                    path: path_str,
                    size,
                    passes_completed: completed,
                    success: false,
                    error: Some("Cancelled by user".into()),
                    kind: "file".into(),
                };
            }

            let mut file = match OpenOptions::new().write(true).open(&current_path) {
                Ok(f) => f,
                Err(e) => {
                    return ShredResult {
                        path: path_str,
                        size,
                        passes_completed: completed,
                        success: false,
                        error: Some(format!("Cannot open: {}", e)),
                        kind: "file".into(),
                    };
                }
            };

            if let Err(e) = overwrite_pass(
                window,
                flag,
                &mut file,
                size,
                *pass,
                &current_path.to_string_lossy(),
                pass_idx as u32 + 1,
                passes.len() as u32,
                file_index,
                total_files,
            ) {
                let cancelled = is_cancelled(flag);
                return ShredResult {
                    path: path_str,
                    size,
                    passes_completed: completed,
                    success: false,
                    error: Some(if cancelled {
                        "Cancelled by user".into()
                    } else {
                        format!("Pass {} failed: {}", completed + 1, e)
                    }),
                    kind: "file".into(),
                };
            }
            completed += 1;
            drop(file);

            if randomize_name && pass_idx < passes.len() - 1 {
                if let Ok(renamed) = rename_to_random(&current_path) {
                    current_path = renamed;
                }
            }
        }
    }

    let final_path = if randomize_name {
        rename_to_random(&current_path).unwrap_or(current_path)
    } else {
        current_path
    };

    if let Err(e) = std::fs::remove_file(&final_path) {
        return ShredResult {
            path: path_str,
            size,
            passes_completed: completed,
            success: false,
            error: Some(format!("Overwrote but couldn't delete: {}", e)),
            kind: "file".into(),
        };
    }

    ShredResult {
        path: path_str,
        size,
        passes_completed: completed,
        success: true,
        error: None,
        kind: "file".into(),
    }
}

fn rename_to_random(path: &Path) -> std::io::Result<PathBuf> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let mut rng = rand::thread_rng();
    for _ in 0..10 {
        let random_name: String = (&mut rng)
            .sample_iter(Alphanumeric)
            .take(16)
            .map(char::from)
            .collect();
        let candidate = parent.join(random_name);
        if !candidate.exists() {
            std::fs::rename(path, &candidate)?;
            return Ok(candidate);
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        "Could not find unused random name",
    ))
}

fn overwrite_pass(
    window: &Window,
    flag: &CancelFlag,
    file: &mut File,
    size: u64,
    pass: Pass,
    current_file: &str,
    pass_num: u32,
    total_passes: u32,
    file_index: usize,
    total_files: usize,
) -> std::io::Result<()> {
    file.seek(SeekFrom::Start(0))?;

    const CHUNK: usize = 1024 * 1024;
    let mut buffer = vec![0u8; CHUNK];

    let mut written: u64 = 0;
    let mut rng = rand::thread_rng();
    let mut last_emit_at: u64 = 0;

    while written < size {
        if is_cancelled(flag) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Interrupted,
                "Cancelled",
            ));
        }

        let to_write = std::cmp::min(CHUNK as u64, size - written) as usize;

        match pass {
            Pass::Zeros => buffer[..to_write].fill(0),
            Pass::Ones => buffer[..to_write].fill(0xFF),
            Pass::Random => rng.fill_bytes(&mut buffer[..to_write]),
        }

        file.write_all(&buffer[..to_write])?;
        written += to_write as u64;

        if written - last_emit_at >= 16 * 1024 * 1024 || written == size {
            let _ = window.emit(
                "shred-progress",
                ProgressEvent {
                    current_file: current_file.to_string(),
                    current_pass: pass_num,
                    total_passes,
                    bytes_done: written,
                    bytes_total: size,
                    files_done: file_index,
                    files_total: total_files,
                },
            );
            last_emit_at = written;
        }
    }

    file.sync_all()?;
    Ok(())
}

// ---------------- Free space wiping ----------------

#[derive(Serialize, Clone)]
pub struct WipeProgressEvent {
    pub bytes_written: u64,
    pub estimated_total: u64,
}

#[tauri::command]
pub async fn wipe_free_space(
    window: Window,
    flag: State<'_, CancelFlag>,
    target_dir: String,
    pattern: String,
) -> Result<u64, String> {
    reset_cancel(&flag);

    // Validate before creating the fill file — refuse traversal and
    // OS-critical locations even though we only write a temp file here.
    let dir = crate::core::safe_path::validate_user_path(&target_dir)?;
    if !dir.is_dir() {
        return Err("Target must be an existing directory".into());
    }

    let temp_path = dir.join(format!(".keepitlocal_wipe_{}.tmp", random_suffix()));

    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&temp_path)
        .map_err(|e| format!("Cannot create temp file: {}", e))?;

    let mut total_written: u64 = 0;
    const CHUNK: usize = 4 * 1024 * 1024;
    let mut buffer = vec![0u8; CHUNK];
    let mut last_emit: u64 = 0;
    let estimated_total = available_space(&dir).unwrap_or(0);

    let regenerate_each_chunk = match pattern.as_str() {
        "zeros" => {
            buffer.fill(0);
            false
        }
        "ones" => {
            buffer.fill(0xFF);
            false
        }
        "fast" | "random" => true,
        _ => {
            buffer.fill(0);
            false
        }
    };

    let mut small_rng = SmallRng::from_entropy();
    let mut secure_rng = rand::thread_rng();

    let cancelled = loop {
        if is_cancelled(&flag) {
            break true;
        }

        if regenerate_each_chunk {
            match pattern.as_str() {
                "fast" => small_rng.fill_bytes(&mut buffer),
                "random" => secure_rng.fill_bytes(&mut buffer),
                _ => {}
            }
        }

        match file.write_all(&buffer) {
            Ok(_) => {
                total_written += CHUNK as u64;
                if total_written - last_emit >= 64 * 1024 * 1024 {
                    let _ = window.emit(
                        "wipe-progress",
                        WipeProgressEvent {
                            bytes_written: total_written,
                            estimated_total,
                        },
                    );
                    last_emit = total_written;
                }
            }
            Err(e) => {
                if e.kind() == std::io::ErrorKind::StorageFull
                    || e.raw_os_error() == Some(28)
                    || e.raw_os_error() == Some(112)
                {
                    break false;
                }
                let _ = file.sync_all();
                drop(file);
                let _ = std::fs::remove_file(&temp_path);
                reset_cancel(&flag);
                return Err(format!("Write failed: {}", e));
            }
        }
    };

    let _ = file.sync_all();
    drop(file);

    if let Err(e) = std::fs::remove_file(&temp_path) {
        reset_cancel(&flag);
        return Err(format!(
            "Wrote {} bytes but couldn't remove temp file: {}",
            total_written, e
        ));
    }

    reset_cancel(&flag);

    if cancelled {
        Err(format!("Cancelled after {} bytes written", total_written))
    } else {
        Ok(total_written)
    }
}

#[tauri::command]
pub fn cancel_operation(flag: State<CancelFlag>) {
    flag.0.store(true, Ordering::Relaxed);
}

fn reset_cancel(flag: &CancelFlag) {
    flag.0.store(false, Ordering::Relaxed);
}

fn is_cancelled(flag: &CancelFlag) -> bool {
    flag.0.load(Ordering::Relaxed)
}

fn random_suffix() -> String {
    rand::thread_rng()
        .sample_iter(Alphanumeric)
        .take(8)
        .map(char::from)
        .collect()
}

fn available_space(path: &Path) -> Option<u64> {
    fs2::available_space(path).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quick_method_is_a_single_random_pass() {
        let passes = passes_for("quick");
        assert_eq!(passes.len(), 1);
        assert!(matches!(passes[0], Pass::Random));
    }

    #[test]
    fn dod3_method_is_zeros_then_ones_then_random() {
        let passes = passes_for("dod3");
        assert_eq!(passes.len(), 3);
        assert!(matches!(passes[0], Pass::Zeros));
        assert!(matches!(passes[1], Pass::Ones));
        assert!(matches!(passes[2], Pass::Random));
    }

    #[test]
    fn dod7_method_has_seven_passes_bookended_by_random() {
        let passes = passes_for("dod7");
        assert_eq!(passes.len(), 7);
        assert!(matches!(passes[0], Pass::Random));
        assert!(matches!(passes[6], Pass::Random));
    }

    #[test]
    fn unknown_method_falls_back_to_a_single_random_pass() {
        let passes = passes_for("not-a-real-method");
        assert_eq!(passes.len(), 1);
        assert!(matches!(passes[0], Pass::Random));
    }

    #[test]
    fn random_suffix_is_eight_alphanumeric_characters() {
        let suffix = random_suffix();
        assert_eq!(suffix.len(), 8);
        assert!(suffix.chars().all(|c| c.is_ascii_alphanumeric()));
        assert_ne!(random_suffix(), random_suffix());
    }

    #[test]
    fn rename_to_random_relocates_file_and_preserves_content() {
        let dir = std::env::temp_dir().join("keepitlocal-shredder-test-rename");
        std::fs::create_dir_all(&dir).expect("create test dir");
        let original = dir.join("sensitive.txt");
        std::fs::write(&original, b"classified bytes").expect("write test file");

        let renamed = rename_to_random(&original).expect("rename to random");

        assert!(!original.exists(), "original path is gone after rename");
        assert!(renamed.exists(), "renamed path exists");
        assert_eq!(renamed.parent(), Some(dir.as_path()));
        assert_eq!(
            renamed.file_name().unwrap().to_string_lossy().len(),
            16,
            "random name is 16 characters"
        );
        assert_eq!(
            std::fs::read(&renamed).expect("read renamed file"),
            b"classified bytes".to_vec()
        );

        std::fs::remove_dir_all(&dir).ok();
    }
}
