use md5::{Digest as Md5Digest, Md5};
use serde::Serialize;
use sha1::Sha1;
use sha2::Sha256;
use std::collections::HashSet;
use std::fs::File;
use std::io::{BufReader, Read};
use std::sync::{LazyLock, Mutex};

#[derive(Serialize)]
pub struct Hashes {
    md5: String,
    sha1: String,
    sha256: String,
    blake3: String,
}

static CANCELLED_HASH_OPERATIONS: LazyLock<Mutex<HashSet<String>>> =
    LazyLock::new(|| Mutex::new(HashSet::new()));

#[tauri::command]
pub fn cancel_hash_operation(operation_id: String) -> Result<(), String> {
    CANCELLED_HASH_OPERATIONS
        .lock()
        .map_err(|_| "Cancel registry lock failed".to_string())?
        .insert(operation_id);
    Ok(())
}

fn is_hash_operation_cancelled(operation_id: &Option<String>) -> bool {
    operation_id
        .as_ref()
        .and_then(|id| {
            CANCELLED_HASH_OPERATIONS
                .lock()
                .ok()
                .map(|set| set.contains(id))
        })
        .unwrap_or(false)
}

fn clear_hash_operation_cancel(operation_id: &Option<String>) {
    if let Some(id) = operation_id {
        if let Ok(mut set) = CANCELLED_HASH_OPERATIONS.lock() {
            set.remove(id);
        }
    }
}

#[tauri::command]
pub fn compute_hashes(path: String, operation_id: Option<String>) -> Result<Hashes, String> {
    // Validate the user-supplied path before opening the file. Rejects
    // traversal + system locations; the canonical path goes straight to
    // File::open without further checks.
    let canonical = crate::core::safe_path::validate_user_path(&path)?;
    let file = File::open(&canonical).map_err(|e| format!("Cannot open file: {}", e))?;
    let mut reader = BufReader::with_capacity(1024 * 1024, file);

    let mut md5 = Md5::new();
    let mut sha1 = Sha1::new();
    let mut sha256 = Sha256::new();
    let mut blake3 = blake3::Hasher::new();

    let op_id = operation_id;
    let mut buffer = [0u8; 65536];
    loop {
        if is_hash_operation_cancelled(&op_id) {
            clear_hash_operation_cancel(&op_id);
            return Err("Cancelled".to_string());
        }

        let n = reader
            .read(&mut buffer)
            .map_err(|e| format!("Read error: {}", e))?;
        if n == 0 {
            break;
        }
        md5.update(&buffer[..n]);
        sha1.update(&buffer[..n]);
        sha256.update(&buffer[..n]);
        blake3.update(&buffer[..n]);
    }

    clear_hash_operation_cancel(&op_id);

    Ok(Hashes {
        md5: hex_encode(&md5.finalize()),
        sha1: hex_encode(&sha1.finalize()),
        sha256: hex_encode(&sha256.finalize()),
        blake3: blake3.finalize().to_hex().to_string(),
    })
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_temp_file(tag: &str, content: &[u8]) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("keepitlocal-hash-test-{tag}"));
        let mut file = File::create(&path).expect("create temp file");
        file.write_all(content).expect("write temp file");
        path
    }

    #[test]
    fn computes_known_hashes_for_abc() {
        let path = write_temp_file("abc", b"abc");
        let hashes =
            compute_hashes(path.to_string_lossy().into_owned(), None).expect("hash abc");
        assert_eq!(hashes.md5, "900150983cd24fb0d6963f7d28e17f72");
        assert_eq!(hashes.sha1, "a9993e364706816aba3e25717850c26c9cd0d89d");
        assert_eq!(
            hashes.sha256,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            hashes.blake3,
            "6437b3ac38465133ffb63b75273a8db548c558465d79db03fd359c6cd5bd9d85"
        );
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn computes_known_hashes_for_empty_file() {
        let path = write_temp_file("empty", b"");
        let hashes =
            compute_hashes(path.to_string_lossy().into_owned(), None).expect("hash empty file");
        assert_eq!(hashes.md5, "d41d8cd98f00b204e9800998ecf8427e");
        assert_eq!(hashes.sha1, "da39a3ee5e6b4b0d3255bfef95601890afd80709");
        assert_eq!(
            hashes.sha256,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            hashes.blake3,
            "af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262"
        );
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn rejects_nonexistent_path() {
        let missing = std::env::temp_dir().join("keepitlocal-hash-test-missing-file");
        std::fs::remove_file(&missing).ok();
        let result = compute_hashes(missing.to_string_lossy().into_owned(), None);
        assert!(result.is_err());
    }

    #[test]
    fn cancelled_operation_returns_cancelled_error() {
        let path = write_temp_file("cancel", b"payload to hash");
        let operation_id = "keepitlocal-hash-test-cancel-op".to_string();
        cancel_hash_operation(operation_id.clone()).expect("register cancel");
        let result = compute_hashes(path.to_string_lossy().into_owned(), Some(operation_id));
        assert_eq!(result.err().as_deref(), Some("Cancelled"));
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn hex_encode_zero_pads_each_byte() {
        assert_eq!(hex_encode(&[0x00, 0x0f, 0xff]), "000fff");
        assert_eq!(hex_encode(&[]), "");
    }
}
