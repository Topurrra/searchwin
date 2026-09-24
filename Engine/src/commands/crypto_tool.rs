//! Encrypt / Decrypt — authenticated, streaming file & text encryption.
//!
//! Crypto: **AES-256-GCM** in the **STREAM (BE32)** construction (per-chunk
//! nonces + a last-chunk flag, so truncation/reordering is detected), with the
//! key derived by **Argon2id** from the password (never the raw password). An
//! optional **keyfile** is folded in as the Argon2 secret/pepper. Large files
//! are processed chunk-by-chunk so memory stays bounded.
//!
//! File layout: a small plaintext header (magic, version, KDF params, salt,
//! nonce prefix) followed by a sequence of AEAD chunks. The header stores the
//! KDF params so files stay decryptable if defaults change later.
//!
//! Design vs. 7-Zip / AES Crypt: correct, modern AEAD with a memory-hard KDF and
//! authenticated tamper detection, a keyfile option, and zero foot-guns — wrong
//! password/keyfile or any corruption fails loudly on the first chunk.

use std::collections::HashSet;
use std::fs::File;
use std::io::{BufReader, BufWriter, Cursor, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};

use aes_gcm::aead::generic_array::GenericArray;
use aes_gcm::aead::stream::{DecryptorBE32, EncryptorBE32};
use aes_gcm::Aes256Gcm;
use base64::Engine;
use serde::Serialize;

// ── Cancel registry (Wave 2.5) ─────────────────────────────────────────
//
// AES-GCM-STREAM encrypts/decrypts in 256 KiB chunks; for multi-GB
// files that's hundreds of iterations, each one a natural place to
// check whether the user pressed Cancel. Pattern mirrors hash.rs +
// pdf.rs — a shared HashSet of cancelled operation IDs, with a Tauri
// command to set the flag and a per-loop check that returns
// Err("Cancelled") + cleans up. The half-written output file is
// removed by the existing decrypt-failure cleanup path; the encrypt
// path does the same on cancel (see encrypt commands below).

static CANCELLED_CRYPTO_OPERATIONS: LazyLock<Mutex<HashSet<String>>> =
    LazyLock::new(|| Mutex::new(HashSet::new()));

#[tauri::command]
pub fn cancel_crypto_operation(operation_id: String) -> Result<(), String> {
    CANCELLED_CRYPTO_OPERATIONS
        .lock()
        .map_err(|_| "Cancel registry lock failed".to_string())?
        .insert(operation_id);
    Ok(())
}

fn is_crypto_cancelled(operation_id: &Option<String>) -> bool {
    operation_id
        .as_ref()
        .and_then(|id| CANCELLED_CRYPTO_OPERATIONS.lock().ok().map(|s| s.contains(id)))
        .unwrap_or(false)
}

fn clear_crypto_cancel(operation_id: &Option<String>) {
    if let Some(id) = operation_id {
        if let Ok(mut set) = CANCELLED_CRYPTO_OPERATIONS.lock() {
            set.remove(id);
        }
    }
}

const MAGIC: &[u8; 6] = b"KILENC";
const VERSION: u8 = 1;
const KDF_ARGON2ID: u8 = 1;
const SALT_LEN: usize = 16;
const PREFIX_LEN: usize = 7; // STREAM BE32 prefix for a 12-byte AEAD nonce (12 − 5)
const PLAIN_CHUNK: usize = 256 * 1024;
const TAG_LEN: usize = 16;

// Argon2id defaults (OWASP-grade for a desktop tool). Stored in the header so
// future tweaks don't break old files.
const M_COST: u32 = 65_536; // 64 MiB
const T_COST: u32 = 3;
const P_COST: u32 = 1;

const ENCRYPTED_EXT: &str = "kenc";

struct Header {
    keyfile: bool,
    m_cost: u32,
    t_cost: u32,
    p_cost: u32,
    chunk: u32,
    salt: [u8; SALT_LEN],
    prefix: [u8; PREFIX_LEN],
}

fn write_header<W: Write>(w: &mut W, h: &Header) -> Result<(), String> {
    let mut buf = Vec::with_capacity(64);
    buf.extend_from_slice(MAGIC);
    buf.extend_from_slice(&[VERSION, KDF_ARGON2ID, h.keyfile as u8, 0]);
    buf.extend_from_slice(&h.m_cost.to_le_bytes());
    buf.extend_from_slice(&h.t_cost.to_le_bytes());
    buf.extend_from_slice(&h.p_cost.to_le_bytes());
    buf.extend_from_slice(&h.chunk.to_le_bytes());
    buf.extend_from_slice(&h.salt);
    buf.extend_from_slice(&h.prefix);
    w.write_all(&buf).map_err(|e| format!("Write failed: {e}"))
}

fn read_exact_vec<R: Read>(r: &mut R, n: usize) -> Result<Vec<u8>, String> {
    let mut buf = vec![0u8; n];
    r.read_exact(&mut buf).map_err(|_| "File is too short or corrupted.".to_string())?;
    Ok(buf)
}

fn read_header<R: Read>(r: &mut R) -> Result<Header, String> {
    let magic = read_exact_vec(r, 6)?;
    if magic != MAGIC {
        return Err("This isn't a KeepItLocal encrypted file.".to_string());
    }
    let meta = read_exact_vec(r, 4)?;
    if meta[0] != VERSION || meta[1] != KDF_ARGON2ID {
        return Err("Unsupported encrypted-file version.".to_string());
    }
    let keyfile = meta[2] != 0;
    let u32_at = |b: &[u8]| u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    let m = read_exact_vec(r, 4)?;
    let t = read_exact_vec(r, 4)?;
    let p = read_exact_vec(r, 4)?;
    let c = read_exact_vec(r, 4)?;
    let salt_v = read_exact_vec(r, SALT_LEN)?;
    let prefix_v = read_exact_vec(r, PREFIX_LEN)?;
    let mut salt = [0u8; SALT_LEN];
    salt.copy_from_slice(&salt_v);
    let mut prefix = [0u8; PREFIX_LEN];
    prefix.copy_from_slice(&prefix_v);
    Ok(Header {
        keyfile,
        m_cost: u32_at(&m),
        t_cost: u32_at(&t),
        p_cost: u32_at(&p),
        chunk: u32_at(&c),
        salt,
        prefix,
    })
}

/// Derive the 32-byte AES key via Argon2id. A keyfile (if present) is hashed
/// with BLAKE3 and used as the Argon2 secret/pepper.
fn derive_key(
    password: &[u8],
    salt: &[u8],
    keyfile: Option<&[u8]>,
    m_cost: u32,
    t_cost: u32,
    p_cost: u32,
) -> Result<[u8; 32], String> {
    use argon2::{Algorithm, Argon2, Params, Version};
    let params = Params::new(m_cost, t_cost, p_cost, Some(32))
        .map_err(|e| format!("Invalid KDF params: {e}"))?;
    // Argon2 borrows the secret/pepper, so it must outlive `argon` — own it here.
    let pepper: Option<[u8; 32]> = keyfile.map(|bytes| *blake3::hash(bytes).as_bytes());
    let argon = match &pepper {
        Some(secret) => {
            Argon2::new_with_secret(secret, Algorithm::Argon2id, Version::V0x13, params)
                .map_err(|e| format!("KDF init failed: {e}"))?
        }
        None => Argon2::new(Algorithm::Argon2id, Version::V0x13, params),
    };
    let mut key = [0u8; 32];
    argon
        .hash_password_into(password, salt, &mut key)
        .map_err(|e| format!("Key derivation failed: {e}"))?;
    Ok(key)
}

fn read_up_to<R: Read>(r: &mut R, buf: &mut [u8]) -> Result<usize, String> {
    let mut total = 0;
    while total < buf.len() {
        match r.read(&mut buf[total..]) {
            Ok(0) => break,
            Ok(n) => total += n,
            Err(e) => return Err(format!("Read failed: {e}")),
        }
    }
    Ok(total)
}

fn encrypt_stream<R: Read, W: Write>(
    reader: &mut R,
    writer: &mut W,
    password: &[u8],
    keyfile: Option<&[u8]>,
    operation_id: &Option<String>,
) -> Result<(), String> {
    use rand::RngCore;
    let mut salt = [0u8; SALT_LEN];
    let mut prefix = [0u8; PREFIX_LEN];
    let mut rng = rand::rngs::OsRng;
    rng.fill_bytes(&mut salt);
    rng.fill_bytes(&mut prefix);

    let key = derive_key(password, &salt, keyfile, M_COST, T_COST, P_COST)?;
    write_header(
        writer,
        &Header {
            keyfile: keyfile.is_some(),
            m_cost: M_COST,
            t_cost: T_COST,
            p_cost: P_COST,
            chunk: PLAIN_CHUNK as u32,
            salt,
            prefix,
        },
    )?;

    let mut stream = EncryptorBE32::<Aes256Gcm>::new(
        GenericArray::from_slice(&key),
        GenericArray::from_slice(&prefix),
    );

    let mut buf = vec![0u8; PLAIN_CHUNK];
    let mut pending: Option<Vec<u8>> = None;
    loop {
        if is_crypto_cancelled(operation_id) {
            return Err("Cancelled".to_string());
        }
        let n = read_up_to(reader, &mut buf)?;
        if n == 0 {
            // Flush the held-back chunk as the last (empty input → empty last).
            let last = pending.take().unwrap_or_default();
            let out = stream
                .encrypt_last(last.as_slice())
                .map_err(|_| "Encryption failed.".to_string())?;
            writer.write_all(&out).map_err(|e| format!("Write failed: {e}"))?;
            break;
        }
        let chunk = buf[..n].to_vec();
        if let Some(prev) = pending.take() {
            let out = stream
                .encrypt_next(prev.as_slice())
                .map_err(|_| "Encryption failed.".to_string())?;
            writer.write_all(&out).map_err(|e| format!("Write failed: {e}"))?;
        }
        if n < PLAIN_CHUNK {
            let out = stream
                .encrypt_last(chunk.as_slice())
                .map_err(|_| "Encryption failed.".to_string())?;
            writer.write_all(&out).map_err(|e| format!("Write failed: {e}"))?;
            break;
        }
        pending = Some(chunk);
    }
    Ok(())
}

fn decrypt_stream<R: Read, W: Write>(
    reader: &mut R,
    writer: &mut W,
    password: &[u8],
    keyfile: Option<&[u8]>,
    operation_id: &Option<String>,
) -> Result<(), String> {
    let header = read_header(reader)?;
    if header.keyfile && keyfile.is_none() {
        return Err("This file was encrypted with a keyfile — supply the same keyfile.".to_string());
    }
    // Ignore a stray keyfile if the file wasn't made with one.
    let effective_keyfile = if header.keyfile { keyfile } else { None };
    let key = derive_key(
        password,
        &header.salt,
        effective_keyfile,
        header.m_cost,
        header.t_cost,
        header.p_cost,
    )?;

    let mut stream = DecryptorBE32::<Aes256Gcm>::new(
        GenericArray::from_slice(&key),
        GenericArray::from_slice(&header.prefix),
    );

    let enc_chunk = header.chunk as usize + TAG_LEN;
    let mut buf = vec![0u8; enc_chunk];
    let mut pending: Option<Vec<u8>> = None;
    let bad = || "Incorrect password/keyfile, or the file is corrupted.".to_string();
    loop {
        if is_crypto_cancelled(operation_id) {
            return Err("Cancelled".to_string());
        }
        let n = read_up_to(reader, &mut buf)?;
        if n == 0 {
            if let Some(prev) = pending.take() {
                let out = stream.decrypt_last(prev.as_slice()).map_err(|_| bad())?;
                writer.write_all(&out).map_err(|e| format!("Write failed: {e}"))?;
            }
            break;
        }
        let chunk = buf[..n].to_vec();
        if let Some(prev) = pending.take() {
            let out = stream.decrypt_next(prev.as_slice()).map_err(|_| bad())?;
            writer.write_all(&out).map_err(|e| format!("Write failed: {e}"))?;
        }
        if n < enc_chunk {
            let out = stream.decrypt_last(chunk.as_slice()).map_err(|_| bad())?;
            writer.write_all(&out).map_err(|e| format!("Write failed: {e}"))?;
            break;
        }
        pending = Some(chunk);
    }
    Ok(())
}

// ── Path helpers ────────────────────────────────────────────────────────────

fn read_keyfile(path: &Option<String>) -> Result<Option<Vec<u8>>, String> {
    match path {
        Some(p) if !p.trim().is_empty() => {
            let bytes = std::fs::read(p).map_err(|e| format!("Cannot read keyfile: {e}"))?;
            if bytes.is_empty() {
                return Err("The keyfile is empty.".to_string());
            }
            Ok(Some(bytes))
        }
        _ => Ok(None),
    }
}

/// Don't overwrite: if `path` exists, append " (2)", " (3)", …
fn resolve_output(path: PathBuf) -> PathBuf {
    if !path.exists() {
        return path;
    }
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("output").to_string();
    let ext = path.extension().and_then(|s| s.to_str()).map(|s| s.to_string());
    let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
    for i in 2..1000 {
        let name = match &ext {
            Some(ext) => format!("{stem} ({i}).{ext}"),
            None => format!("{stem} ({i})"),
        };
        let candidate = dir.join(name);
        if !candidate.exists() {
            return candidate;
        }
    }
    path
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CryptoResult {
    pub output_path: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CryptoInspect {
    pub valid: bool,
    pub keyfile_required: bool,
}

// ── Tauri commands ──────────────────────────────────────────────────────────

#[tauri::command]
pub async fn crypto_encrypt_file(
    input_path: String,
    output_path: Option<String>,
    password: String,
    keyfile_path: Option<String>,
    operation_id: Option<String>,
) -> Result<CryptoResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        clear_crypto_cancel(&operation_id);
        if password.is_empty() && keyfile_path.as_deref().unwrap_or("").is_empty() {
            return Err("Provide a password or a keyfile.".to_string());
        }
        let keyfile = read_keyfile(&keyfile_path)?;
        let input = PathBuf::from(&input_path);
        let out = output_path
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(format!("{input_path}.{ENCRYPTED_EXT}")));
        let out = resolve_output(out);

        let mut reader = BufReader::new(
            File::open(&input).map_err(|e| format!("Cannot open input: {e}"))?,
        );
        let mut writer = BufWriter::new(
            File::create(&out).map_err(|e| format!("Cannot create output: {e}"))?,
        );
        let result = encrypt_stream(
            &mut reader,
            &mut writer,
            password.as_bytes(),
            keyfile.as_deref(),
            &operation_id,
        );
        if let Err(e) = result {
            // Half-written ciphertext is useless and confusing; remove it.
            // Same cleanup the decrypt path does on failure.
            drop(writer);
            let _ = std::fs::remove_file(&out);
            clear_crypto_cancel(&operation_id);
            return Err(e);
        }
        writer.flush().map_err(|e| format!("Write failed: {e}"))?;
        clear_crypto_cancel(&operation_id);
        Ok(CryptoResult { output_path: out.to_string_lossy().to_string() })
    })
    .await
    .map_err(|e| format!("Worker failed: {e}"))?
}

#[tauri::command]
pub async fn crypto_decrypt_file(
    input_path: String,
    output_path: Option<String>,
    password: String,
    keyfile_path: Option<String>,
    operation_id: Option<String>,
) -> Result<CryptoResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        clear_crypto_cancel(&operation_id);
        let keyfile = read_keyfile(&keyfile_path)?;
        let input = PathBuf::from(&input_path);
        // Default output: strip the .kenc extension, else append ".dec".
        let default_out = if input.extension().and_then(|e| e.to_str()) == Some(ENCRYPTED_EXT) {
            input.with_extension("")
        } else {
            PathBuf::from(format!("{input_path}.dec"))
        };
        let out = resolve_output(output_path.map(PathBuf::from).unwrap_or(default_out));

        let mut reader = BufReader::new(
            File::open(&input).map_err(|e| format!("Cannot open input: {e}"))?,
        );
        let mut writer = BufWriter::new(
            File::create(&out).map_err(|e| format!("Cannot create output: {e}"))?,
        );
        let result = decrypt_stream(
            &mut reader,
            &mut writer,
            password.as_bytes(),
            keyfile.as_deref(),
            &operation_id,
        );
        if let Err(e) = result {
            // Don't leave a half-written/garbage output file on failure
            // (cancel goes through this branch too — Err("Cancelled")).
            drop(writer);
            let _ = std::fs::remove_file(&out);
            clear_crypto_cancel(&operation_id);
            return Err(e);
        }
        writer.flush().map_err(|e| format!("Write failed: {e}"))?;
        clear_crypto_cancel(&operation_id);
        Ok(CryptoResult { output_path: out.to_string_lossy().to_string() })
    })
    .await
    .map_err(|e| format!("Worker failed: {e}"))?
}

#[tauri::command]
pub fn crypto_encrypt_text(
    text: String,
    password: String,
    keyfile_path: Option<String>,
) -> Result<String, String> {
    if password.is_empty() && keyfile_path.as_deref().unwrap_or("").is_empty() {
        return Err("Provide a password or a keyfile.".to_string());
    }
    let keyfile = read_keyfile(&keyfile_path)?;
    let mut reader = Cursor::new(text.into_bytes());
    let mut out: Vec<u8> = Vec::new();
    encrypt_stream(&mut reader, &mut out, password.as_bytes(), keyfile.as_deref(), &None)?;
    Ok(base64::engine::general_purpose::STANDARD.encode(out))
}

#[tauri::command]
pub fn crypto_decrypt_text(
    payload: String,
    password: String,
    keyfile_path: Option<String>,
) -> Result<String, String> {
    let keyfile = read_keyfile(&keyfile_path)?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(payload.trim())
        .map_err(|_| "Not valid encrypted text (expected Base64).".to_string())?;
    let mut reader = Cursor::new(bytes);
    let mut out: Vec<u8> = Vec::new();
    decrypt_stream(&mut reader, &mut out, password.as_bytes(), keyfile.as_deref(), &None)?;
    String::from_utf8(out).map_err(|_| "Decrypted data is not valid UTF-8 text.".to_string())
}

/// Peek at a file's header so the UI can tell whether it's ours and whether a
/// keyfile is required — without needing the password.
#[tauri::command]
pub fn crypto_inspect_file(input_path: String) -> Result<CryptoInspect, String> {
    let mut reader = BufReader::new(
        File::open(&input_path).map_err(|e| format!("Cannot open input: {e}"))?,
    );
    match read_header(&mut reader) {
        Ok(header) => Ok(CryptoInspect { valid: true, keyfile_required: header.keyfile }),
        Err(_) => Ok(CryptoInspect { valid: false, keyfile_required: false }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn enc(data: &[u8], pw: &[u8], kf: Option<&[u8]>) -> Vec<u8> {
        let mut out = Vec::new();
        encrypt_stream(&mut Cursor::new(data.to_vec()), &mut out, pw, kf, &None).unwrap();
        out
    }
    fn dec(blob: Vec<u8>, pw: &[u8], kf: Option<&[u8]>) -> Result<Vec<u8>, String> {
        let mut out = Vec::new();
        decrypt_stream(&mut Cursor::new(blob), &mut out, pw, kf, &None)?;
        Ok(out)
    }

    #[test]
    fn roundtrip_small_empty_and_multichunk() {
        for data in [
            b"hello, KeepItLocal".to_vec(),
            Vec::new(),
            vec![7u8; PLAIN_CHUNK + 1],         // spans 2 chunks (boundary + 1)
            vec![9u8; PLAIN_CHUNK * 2 + 1234],  // spans 3 chunks
        ] {
            let blob = enc(&data, b"correct horse", None);
            assert_eq!(dec(blob, b"correct horse", None).unwrap(), data);
        }
    }

    #[test]
    fn wrong_password_is_rejected() {
        let blob = enc(b"top secret", b"right-password", None);
        assert!(dec(blob, b"wrong-password", None).is_err());
    }

    #[test]
    fn tampering_is_detected() {
        let mut blob = enc(b"integrity matters", b"pw", None);
        let last = blob.len() - 1;
        blob[last] ^= 0x01; // flip a ciphertext/tag bit
        assert!(dec(blob, b"pw", None).is_err());
    }

    #[test]
    fn keyfile_required_and_must_match() {
        let kf = b"keyfile-contents";
        let data = b"protected by two factors";
        let blob = enc(data, b"pw", Some(kf));
        // correct keyfile decrypts
        assert_eq!(dec(blob.clone(), b"pw", Some(kf)).unwrap(), data.to_vec());
        // wrong keyfile fails
        assert!(dec(blob.clone(), b"pw", Some(b"different")).is_err());
        // missing keyfile fails with the explicit "needs a keyfile" guard
        assert!(dec(blob, b"pw", None).is_err());
    }
}
