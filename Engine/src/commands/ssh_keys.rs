//! SSH Key Manager — generate, import, list, and manage OpenSSH keys, plus the
//! `~/.ssh/config` host list and `known_hosts`, all via the pure-Rust `ssh-key`
//! crate (no OpenSSL / native deps).
//!
//! Design vs. PuTTYgen / `ssh-keygen`: a real GUI, modern key types (Ed25519
//! default), one-click fingerprints + public-key copy, a structured config-host
//! editor, and known_hosts cleanup. Private key *material* is never returned to
//! the frontend — only metadata, fingerprints, and the public key.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use ssh_key::private::{Ed25519Keypair, EcdsaKeypair, KeypairData, RsaKeypair};
use ssh_key::{Algorithm, EcdsaCurve, HashAlg, LineEnding, PrivateKey, PublicKey};

// ── Paths ───────────────────────────────────────────────────────────────────

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
}

fn ssh_dir() -> Result<PathBuf, String> {
    Ok(home_dir()
        .ok_or_else(|| "Cannot resolve your home directory.".to_string())?
        .join(".ssh"))
}

fn ensure_ssh_dir() -> Result<PathBuf, String> {
    let dir = ssh_dir()?;
    fs::create_dir_all(&dir).map_err(|error| format!("Cannot create ~/.ssh: {error}"))?;
    Ok(dir)
}

/// Reject path separators / traversal so a key name can only ever be a file
/// inside `~/.ssh`.
fn safe_name(name: &str) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty()
        || name.contains(['/', '\\', ':'])
        || name.contains("..")
        || name.starts_with('.')
    {
        return Err("Invalid key name.".to_string());
    }
    Ok(name.to_string())
}

// ── Model ───────────────────────────────────────────────────────────────────

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SshKeyInfo {
    pub name: String,
    pub path: String,
    pub public_path: Option<String>,
    pub algorithm: String,
    pub bits: Option<u32>,
    pub comment: String,
    pub fingerprint: String,
    pub has_private: bool,
    pub encrypted: bool,
    pub public_key: String,
}

/// Friendly algorithm label from the OpenSSH wire name.
fn friendly_algo(algo: &Algorithm) -> String {
    match algo.as_str() {
        "ssh-ed25519" => "Ed25519".to_string(),
        "ssh-rsa" => "RSA".to_string(),
        "ecdsa-sha2-nistp256" => "ECDSA (P-256)".to_string(),
        "ecdsa-sha2-nistp384" => "ECDSA (P-384)".to_string(),
        "ecdsa-sha2-nistp521" => "ECDSA (P-521)".to_string(),
        other => other.to_string(),
    }
}

fn rsa_bits(public: &PublicKey) -> Option<u32> {
    use ssh_key::public::KeyData;
    if let KeyData::Rsa(rsa) = public.key_data() {
        return rsa
            .n
            .as_positive_bytes()
            .map(|bytes| (bytes.len() as u32) * 8);
    }
    None
}

fn info_from_public(public: &PublicKey, name: &str, path: &Path, has_private: bool, encrypted: bool) -> SshKeyInfo {
    SshKeyInfo {
        name: name.to_string(),
        path: path.to_string_lossy().to_string(),
        public_path: Some(path.with_extension("pub").to_string_lossy().to_string()),
        algorithm: friendly_algo(&public.algorithm()),
        bits: rsa_bits(public),
        comment: public.comment().to_string(),
        fingerprint: public.fingerprint(HashAlg::Sha256).to_string(),
        has_private,
        encrypted,
        public_key: public.to_openssh().unwrap_or_default(),
    }
}

const RESERVED: &[&str] = &["config", "known_hosts", "authorized_keys", "environment", "rc"];

/// List every key pair (and standalone public key) in `~/.ssh`.
#[tauri::command]
pub fn ssh_list_keys() -> Result<Vec<SshKeyInfo>, String> {
    let dir = ssh_dir()?;
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut out: Vec<SshKeyInfo> = Vec::new();
    let mut handled: std::collections::HashSet<String> = std::collections::HashSet::new();

    let entries = fs::read_dir(&dir).map_err(|error| format!("Cannot read ~/.ssh: {error}"))?;
    let files: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_file())
        .collect();

    // First pass: private keys (the OpenSSH private format stores the public key
    // + algorithm unencrypted, so we can describe encrypted keys without the
    // passphrase).
    for path in &files {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default();
        if name.ends_with(".pub") || RESERVED.contains(&name) {
            continue;
        }
        let Ok(text) = fs::read_to_string(path) else {
            continue;
        };
        if !text.contains("PRIVATE KEY") {
            continue;
        }
        if let Ok(key) = PrivateKey::from_openssh(text.trim()) {
            out.push(info_from_public(key.public_key(), name, path, true, key.is_encrypted()));
            handled.insert(name.to_string());
        }
    }

    // Second pass: standalone public keys with no matching private key.
    for path in &files {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default();
        if !name.ends_with(".pub") {
            continue;
        }
        let base = name.trim_end_matches(".pub");
        if handled.contains(base) {
            continue;
        }
        if let Ok(text) = fs::read_to_string(path) {
            if let Ok(public) = PublicKey::from_openssh(text.trim()) {
                let priv_path = dir.join(base);
                out.push(info_from_public(&public, base, &priv_path, false, false));
            }
        }
    }

    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    Ok(out)
}

#[cfg(windows)]
fn lock_private_perms(path: &Path) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    if let Ok(user) = std::env::var("USERNAME") {
        // Strip inheritance, grant only the current user — what OpenSSH expects.
        let _ = std::process::Command::new("icacls")
            .arg(path)
            .arg("/inheritance:r")
            .arg("/grant:r")
            .arg(format!("{user}:F"))
            .creation_flags(CREATE_NO_WINDOW)
            .output();
    }
}
#[cfg(not(windows))]
fn lock_private_perms(_path: &Path) {}

/// Generate a new key pair into `~/.ssh`. `algorithm` is "ed25519" | "rsa" |
/// "ecdsa"; `bits` applies to RSA only. A non-empty `passphrase` encrypts the
/// private key. Never overwrites an existing file.
#[tauri::command(async)]
pub fn ssh_generate_key(
    name: String,
    algorithm: String,
    bits: Option<u32>,
    comment: String,
    passphrase: String,
) -> Result<SshKeyInfo, String> {
    use rand::rngs::OsRng;
    let name = safe_name(&name)?;
    let dir = ensure_ssh_dir()?;
    let priv_path = dir.join(&name);
    let pub_path = dir.join(format!("{name}.pub"));
    if priv_path.exists() || pub_path.exists() {
        return Err(format!("A key named '{name}' already exists in ~/.ssh."));
    }

    let mut rng = OsRng;
    let comment_str = comment.trim().to_string();
    let keypair: KeypairData = match algorithm.as_str() {
        "ed25519" => KeypairData::Ed25519(Ed25519Keypair::random(&mut rng)),
        "ecdsa" => KeypairData::Ecdsa(
            EcdsaKeypair::random(&mut rng, EcdsaCurve::NistP256)
                .map_err(|error| format!("ECDSA key generation failed: {error}"))?,
        ),
        "rsa" => {
            let size = bits.unwrap_or(4096).clamp(2048, 8192) as usize;
            KeypairData::Rsa(
                RsaKeypair::random(&mut rng, size)
                    .map_err(|error| format!("RSA key generation failed: {error}"))?,
            )
        }
        other => return Err(format!("Unsupported algorithm '{other}'.")),
    };

    let mut key = PrivateKey::new(keypair, comment_str)
        .map_err(|error| format!("Could not build key: {error}"))?;
    if !passphrase.is_empty() {
        key = key
            .encrypt(&mut rng, passphrase.as_bytes())
            .map_err(|error| format!("Could not encrypt key: {error}"))?;
    }

    let private_pem = key
        .to_openssh(LineEnding::LF)
        .map_err(|error| format!("Could not serialize private key: {error}"))?;
    let public_text = key
        .public_key()
        .to_openssh()
        .map_err(|error| format!("Could not serialize public key: {error}"))?;

    fs::write(&priv_path, private_pem.as_bytes())
        .map_err(|error| format!("Cannot write private key: {error}"))?;
    fs::write(&pub_path, format!("{public_text}\n"))
        .map_err(|error| format!("Cannot write public key: {error}"))?;
    lock_private_perms(&priv_path);

    Ok(info_from_public(key.public_key(), &name, &priv_path, true, key.is_encrypted()))
}

/// Import an existing private (or public) key file into `~/.ssh` after
/// validating it parses. Copies a sibling `.pub` when present.
#[tauri::command]
pub fn ssh_import_key(source_path: String) -> Result<SshKeyInfo, String> {
    let source = PathBuf::from(&source_path);
    let text = fs::read_to_string(&source)
        .map_err(|error| format!("Cannot read '{source_path}': {error}"))?;
    let file_name = source
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| "Invalid source file name.".to_string())?;

    let dir = ensure_ssh_dir()?;

    if file_name.ends_with(".pub") || (!text.contains("PRIVATE KEY")) {
        // Public key import.
        let public = PublicKey::from_openssh(text.trim())
            .map_err(|error| format!("Not a valid OpenSSH public key: {error}"))?;
        let name = if file_name.ends_with(".pub") { file_name.to_string() } else { format!("{file_name}.pub") };
        let name = safe_name(name.trim_end_matches(".pub"))?;
        let dest = dir.join(format!("{name}.pub"));
        if dest.exists() {
            return Err(format!("'{name}.pub' already exists in ~/.ssh."));
        }
        fs::write(&dest, format!("{}\n", public.to_openssh().unwrap_or_default()))
            .map_err(|error| format!("Cannot write public key: {error}"))?;
        return Ok(info_from_public(&public, &name, &dir.join(&name), false, false));
    }

    // Private key import (validate it parses).
    let key = PrivateKey::from_openssh(text.trim())
        .map_err(|error| format!("Not a valid OpenSSH private key: {error}"))?;
    let name = safe_name(file_name)?;
    let priv_dest = dir.join(&name);
    if priv_dest.exists() {
        return Err(format!("A key named '{name}' already exists in ~/.ssh."));
    }
    fs::copy(&source, &priv_dest).map_err(|error| format!("Cannot copy private key: {error}"))?;
    lock_private_perms(&priv_dest);

    // Copy a sibling .pub if it exists; otherwise derive one from the (decrypted)
    // public part we already have.
    let sibling_pub = source.with_file_name(format!("{file_name}.pub"));
    let pub_dest = dir.join(format!("{name}.pub"));
    if sibling_pub.is_file() {
        let _ = fs::copy(&sibling_pub, &pub_dest);
    } else if let Ok(public_text) = key.public_key().to_openssh() {
        let _ = fs::write(&pub_dest, format!("{public_text}\n"));
    }

    Ok(info_from_public(key.public_key(), &name, &priv_dest, true, key.is_encrypted()))
}

/// Delete a key's private + public files.
#[tauri::command]
pub fn ssh_delete_key(name: String) -> Result<(), String> {
    let name = safe_name(&name)?;
    let dir = ssh_dir()?;
    let priv_path = dir.join(&name);
    let pub_path = dir.join(format!("{name}.pub"));
    if priv_path.is_file() {
        fs::remove_file(&priv_path).map_err(|error| format!("Cannot delete private key: {error}"))?;
    }
    if pub_path.is_file() {
        fs::remove_file(&pub_path).map_err(|error| format!("Cannot delete public key: {error}"))?;
    }
    Ok(())
}

// ── ~/.ssh/config ───────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SshHost {
    pub host: String,
    pub host_name: String,
    pub user: String,
    pub port: String,
    pub identity_file: String,
    /// Any other directives in the block, preserved verbatim ("Key value").
    pub extra: Vec<String>,
}

#[derive(Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SshConfig {
    /// Lines before the first `Host` block (global options + comments), verbatim.
    pub preamble: String,
    pub hosts: Vec<SshHost>,
}

fn config_path() -> Result<PathBuf, String> {
    Ok(ssh_dir()?.join("config"))
}

#[tauri::command]
pub fn ssh_read_config() -> Result<SshConfig, String> {
    let path = config_path()?;
    if !path.is_file() {
        return Ok(SshConfig::default());
    }
    let text = fs::read_to_string(&path).map_err(|error| format!("Cannot read config: {error}"))?;
    let mut config = SshConfig::default();
    let mut preamble: Vec<String> = Vec::new();
    let mut current: Option<SshHost> = None;

    for raw in text.lines() {
        let trimmed = raw.trim();
        let lower = trimmed.to_lowercase();
        if lower.starts_with("host ") || lower == "host" {
            if let Some(host) = current.take() {
                config.hosts.push(host);
            }
            let pattern = trimmed[4..].trim().to_string();
            current = Some(SshHost { host: pattern, ..SshHost::default() });
            continue;
        }
        match current.as_mut() {
            None => preamble.push(raw.to_string()),
            Some(host) => {
                let mut parts = trimmed.splitn(2, char::is_whitespace);
                let key = parts.next().unwrap_or("").to_lowercase();
                let value = parts.next().unwrap_or("").trim().to_string();
                match key.as_str() {
                    "hostname" => host.host_name = value,
                    "user" => host.user = value,
                    "port" => host.port = value,
                    "identityfile" => host.identity_file = value,
                    _ if !trimmed.is_empty() => host.extra.push(trimmed.to_string()),
                    _ => {}
                }
            }
        }
    }
    if let Some(host) = current.take() {
        config.hosts.push(host);
    }
    config.preamble = preamble.join("\n");
    Ok(config)
}

#[tauri::command]
pub fn ssh_write_config(config: SshConfig) -> Result<(), String> {
    ensure_ssh_dir()?;
    let path = config_path()?;
    let mut out = String::new();
    let preamble = config.preamble.trim_end();
    if !preamble.is_empty() {
        out.push_str(preamble);
        out.push('\n');
    }
    for host in &config.hosts {
        if host.host.trim().is_empty() {
            continue;
        }
        if !out.is_empty() && !out.ends_with("\n\n") {
            out.push('\n');
        }
        out.push_str(&format!("Host {}\n", host.host.trim()));
        let mut line = |key: &str, value: &str| {
            if !value.trim().is_empty() {
                out.push_str(&format!("    {key} {}\n", value.trim()));
            }
        };
        line("HostName", &host.host_name);
        line("User", &host.user);
        line("Port", &host.port);
        line("IdentityFile", &host.identity_file);
        for extra in &host.extra {
            if !extra.trim().is_empty() {
                out.push_str(&format!("    {}\n", extra.trim()));
            }
        }
    }
    fs::write(&path, out).map_err(|error| format!("Cannot write config: {error}"))
}

// ── ~/.ssh/known_hosts ──────────────────────────────────────────────────────

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KnownHost {
    /// 0-based line index in the file (used for precise removal).
    pub index: usize,
    pub hosts: String,
    pub key_type: String,
    pub fingerprint: String,
}

fn known_hosts_path() -> Result<PathBuf, String> {
    Ok(ssh_dir()?.join("known_hosts"))
}

#[tauri::command]
pub fn ssh_read_known_hosts() -> Result<Vec<KnownHost>, String> {
    let path = known_hosts_path()?;
    if !path.is_file() {
        return Ok(Vec::new());
    }
    let text =
        fs::read_to_string(&path).map_err(|error| format!("Cannot read known_hosts: {error}"))?;
    let mut out = Vec::new();
    for (index, raw) in text.lines().enumerate() {
        let trimmed = raw.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let mut parts = trimmed.split_whitespace();
        let hosts = parts.next().unwrap_or("").to_string();
        let key_type = parts.next().unwrap_or("").to_string();
        let key_b64 = parts.next().unwrap_or("");
        if hosts.is_empty() || key_type.is_empty() {
            continue;
        }
        let fingerprint = PublicKey::from_openssh(&format!("{key_type} {key_b64}"))
            .map(|key| key.fingerprint(HashAlg::Sha256).to_string())
            .unwrap_or_default();
        out.push(KnownHost { index, hosts, key_type, fingerprint });
    }
    Ok(out)
}

#[tauri::command]
pub fn ssh_remove_known_host(index: usize) -> Result<(), String> {
    let path = known_hosts_path()?;
    if !path.is_file() {
        return Ok(());
    }
    let text =
        fs::read_to_string(&path).map_err(|error| format!("Cannot read known_hosts: {error}"))?;
    let kept: Vec<&str> = text
        .lines()
        .enumerate()
        .filter(|(line_index, _)| *line_index != index)
        .map(|(_, line)| line)
        .collect();
    let mut joined = kept.join("\n");
    if !joined.is_empty() {
        joined.push('\n');
    }
    fs::write(&path, joined).map_err(|error| format!("Cannot write known_hosts: {error}"))
}

/// Absolute path to `~/.ssh` (for an "open folder" affordance in the UI).
#[tauri::command]
pub fn ssh_dir_path() -> Result<String, String> {
    Ok(ssh_dir()?.to_string_lossy().to_string())
}
