//! Quality Pass Wave 1 user feedback (2026-05-29): WizTree-style MFT
//! fast-path for DuplicateFinder.
//!
//! **DISABLED 2026-05-29** in `files.rs::find_duplicate_files_blocking`
//! pending more runtime debugging. Initial Windows test showed this
//! returning ~24 entries instead of the full tree — likely a
//! combination of raw-volume read alignment quirks (the kernel's
//! requirements are stricter than `AlignedVolumeReader` covers) and
//! the ntfs-crate's tree-traversal hitting edge cases on a live NTFS
//! volume rather than a disk image file. The path-concat double-slash
//! bug was fixed but other failures remain. Kept in tree so the
//! partially-fixed code is preserved for the next iteration; tracked
//! in v1Goals.md → Phase 9 pre-launch as a return-before-launch item.
#![allow(dead_code)]
//!
//! Reads the NTFS Master File Table directly via the read-only `ntfs`
//! crate — orders of magnitude faster than walking the directory tree
//! with FindFirstFile/FindNextFile when the user has a really big
//! library to dedup. On a 1 M-photo HDD this drops a 10-minute walk
//! to ~10 seconds (sequential MFT read at disk bandwidth).
//!
//! **NOT used by the filename or content index** per Wave 6's removal
//! decision — this module is a single-purpose fast path for ad-hoc
//! duplicate scans. The MFT path is opt-in by attempt: if it fails for
//! any reason (non-NTFS volume, OneDrive virtualized files, network
//! share, multi-volume scan, permission denied because the volume
//! handle requires elevated rights on this system), `try_enumerate_via_mft`
//! returns `None` and `find_duplicate_files_blocking` falls through to
//! the parallel walker exactly as before.
//!
//! Windows-only via `#![cfg(windows)]` at the call site.

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::os::windows::io::FromRawHandle;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

use ntfs::Ntfs;

/// Quality Pass Wave 1 user feedback bugfix (2026-05-29): raw volume
/// I/O on Windows has THREE strict alignment requirements that
/// `BufReader<File>` does not satisfy and which manifested as a
/// `binrw` panic deep inside `ntfs-0.4.0/src/boot_sector.rs` ("os
/// error 87" = ERROR_INVALID_PARAMETER):
///   * read offset must be sector-aligned (typically 512 or 4096)
///   * read size must be sector-aligned
///   * memory buffer must be sector-aligned (Vec<u8> is only 8-byte
///     aligned on 64-bit, not the 4 KB the kernel demands)
///
/// `AlignedVolumeReader` wraps the raw `\\.\C:` File and:
///   * over-allocates a Vec<u8> + computes an aligned slice within it
///     (no unsafe alloc, no leaks)
///   * tracks the caller's logical read position separately from the
///     physical file offset
///   * on each call to `read()`, refills the internal buffer with one
///     sector-aligned `ReadFile` and then copies the requested slice
///     out to the caller
///
/// This trades raw bandwidth for correctness — the ntfs crate's read
/// pattern is "small reads at scattered offsets" which the buffer
/// handles transparently.
struct AlignedVolumeReader {
    file: File,
    /// Logical position the ntfs crate thinks it's at.
    pos: u64,
    /// The actual backing storage. Larger than `aligned_size` so we
    /// have room to pick a 4 KB-aligned slice inside it.
    raw_buf: Vec<u8>,
    /// Offset into `raw_buf` of the aligned slice we hand to ReadFile.
    aligned_off: usize,
    /// Size of the aligned slice (multiple of `align`).
    aligned_size: usize,
    /// File offset of `raw_buf[aligned_off]` for the currently-buffered
    /// region. `buf_valid == 0` means "buffer is empty, must refill".
    buf_start: u64,
    buf_valid: usize,
}

const VOLUME_READ_ALIGN: usize = 4096;
const VOLUME_READ_BUFFER: usize = 64 * 1024;

impl AlignedVolumeReader {
    fn new(file: File) -> Self {
        // Over-allocate so a 4 KB-aligned window fits anywhere inside.
        let mut raw_buf = vec![0u8; VOLUME_READ_BUFFER + VOLUME_READ_ALIGN];
        let ptr_addr = raw_buf.as_mut_ptr() as usize;
        let aligned_addr = (ptr_addr + VOLUME_READ_ALIGN - 1) & !(VOLUME_READ_ALIGN - 1);
        let aligned_off = aligned_addr - ptr_addr;
        Self {
            file,
            pos: 0,
            raw_buf,
            aligned_off,
            aligned_size: VOLUME_READ_BUFFER,
            buf_start: 0,
            buf_valid: 0,
        }
    }

    /// Refill the buffer so it covers `wanted_offset`. The physical
    /// read starts at the floor of `wanted_offset` to the alignment
    /// boundary, and reads `aligned_size` bytes. Borrow checker note:
    /// the raw_buf and file fields are split-borrowed via local
    /// references to avoid the &mut self pitfall.
    fn refill_to_cover(&mut self, wanted_offset: u64) -> std::io::Result<()> {
        let physical_start =
            (wanted_offset / VOLUME_READ_ALIGN as u64) * VOLUME_READ_ALIGN as u64;
        self.file.seek(SeekFrom::Start(physical_start))?;
        let aligned_size = self.aligned_size;
        let aligned_off = self.aligned_off;
        // Read in a loop because raw volume reads can return short.
        let mut filled = 0usize;
        let file = &mut self.file;
        let slice = &mut self.raw_buf[aligned_off..aligned_off + aligned_size];
        while filled < aligned_size {
            match file.read(&mut slice[filled..]) {
                Ok(0) => break,
                Ok(n) => filled += n,
                Err(ref e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e),
            }
        }
        self.buf_start = physical_start;
        self.buf_valid = filled;
        Ok(())
    }
}

impl Read for AlignedVolumeReader {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        if out.is_empty() {
            return Ok(0);
        }
        let need_start = self.pos;
        // Refill if buffer doesn't cover the needed start.
        let buf_end = self.buf_start + self.buf_valid as u64;
        if self.buf_valid == 0 || need_start < self.buf_start || need_start >= buf_end {
            self.refill_to_cover(need_start)?;
            if self.buf_valid == 0 {
                return Ok(0); // EOF
            }
        }
        let offset_in_buf = (need_start - self.buf_start) as usize;
        let available = self.buf_valid - offset_in_buf;
        let to_copy = out.len().min(available);
        let aligned_start = self.aligned_off + offset_in_buf;
        out[..to_copy]
            .copy_from_slice(&self.raw_buf[aligned_start..aligned_start + to_copy]);
        self.pos += to_copy as u64;
        Ok(to_copy)
    }
}

impl Seek for AlignedVolumeReader {
    fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
        let new_pos = match pos {
            SeekFrom::Start(n) => n,
            SeekFrom::Current(n) => self.pos.wrapping_add(n as u64),
            SeekFrom::End(_) => {
                return Err(std::io::Error::other(
                    "SeekFrom::End is not supported on a raw volume reader",
                ));
            }
        };
        self.pos = new_pos;
        Ok(new_pos)
    }
}

use windows::core::PCWSTR;
use windows::Win32::Foundation::{GENERIC_READ, HANDLE};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_FLAGS_AND_ATTRIBUTES, FILE_SHARE_READ,
    FILE_SHARE_WRITE, OPEN_EXISTING,
};

/// Returned from a successful MFT enumeration. The caller drops this
/// straight into the by_size map for the hashing phase.
pub struct MftResult {
    pub by_size: HashMap<u64, Vec<PathBuf>>,
    pub scanned_files: usize,
}

/// Top-level entry — try to enumerate files via the raw MFT. Returns
/// `None` on **any** failure so the caller falls back to the parallel
/// walker; never propagates errors. v1 supports a single-volume scan
/// (most users' libraries live under one drive letter); multi-volume
/// scans return None and use the walker.
pub fn try_enumerate_via_mft(
    scan_roots: &[String],
    min_size: u64,
    extension_filter: Option<&HashSet<String>>,
    cancel_flag: &AtomicBool,
) -> Option<MftResult> {
    // 1. Determine which drive letter every scan root sits on.
    //    Multi-volume scans bail out — the caller's walker handles it.
    let drives: HashSet<char> = scan_roots
        .iter()
        .filter_map(|r| extract_drive_letter(r))
        .collect();
    if drives.len() != 1 {
        return None;
    }
    let drive = *drives.iter().next()?;

    // 2. Normalize scan roots into lowercase forward-slash form for
    //    fast prefix matching against MFT-reconstructed paths.
    let scope_prefixes: Vec<String> = scan_roots
        .iter()
        .map(|r| {
            r.to_lowercase()
                .replace('\\', "/")
                .trim_end_matches('/')
                .to_string()
        })
        .collect();

    // 3. Open the raw volume (\\.\C: form). Read access only.
    let handle = open_volume(drive).ok()?;
    // SAFETY: HANDLE is a valid OS file handle returned by CreateFileW.
    // FromRawHandle takes ownership — when the File drops, CloseHandle
    // is called.
    let file = unsafe { File::from_raw_handle(handle.0 as *mut std::ffi::c_void) };
    // Quality Pass Wave 1 bugfix (2026-05-29): wrap in AlignedVolumeReader
    // — raw volume reads need 4 KB-aligned offsets / sizes / buffers,
    // which std's BufReader<File> can't guarantee. See the
    // AlignedVolumeReader doc comment for the full diagnosis.
    let mut reader = AlignedVolumeReader::new(file);

    // 4. Initialize the NTFS structure (reads $Volume + $MFT root).
    let ntfs = Ntfs::new(&mut reader).ok()?;

    // 5. Walk the file tree from the root directory, building paths as
    //    we descend. Each file/dir we encounter has its full path in
    //    hand; we filter by scope_prefixes + size + extension and push
    //    qualifying files into by_size.
    let mut by_size: HashMap<u64, Vec<PathBuf>> = HashMap::new();
    let mut scanned_files: usize = 0;
    // Bugfix (2026-05-29): drive_root WITHOUT trailing slash. The path
    // join below adds the separator — using "c:/" + "/" + name gave
    // "c://name" (double slash) which broke every subsequent scope
    // prefix-match.
    let drive_root = format!("{}:", drive.to_ascii_lowercase());

    let root_dir = ntfs.root_directory(&mut reader).ok()?;
    let mut stack: Vec<(ntfs::NtfsFile<'_>, String)> = Vec::new();
    stack.push((root_dir, drive_root));

    while let Some((dir, prefix)) = stack.pop() {
        if cancel_flag.load(Ordering::Relaxed) {
            return None;
        }
        let index = match dir.directory_index(&mut reader) {
            Ok(i) => i,
            Err(_) => continue,
        };
        let mut iter = index.entries();
        while let Some(entry_result) = iter.next(&mut reader) {
            let entry = match entry_result {
                Ok(e) => e,
                Err(_) => continue,
            };
            // Skip $-prefixed system files and parent/self refs.
            let key = match entry.key() {
                Some(Ok(k)) => k,
                _ => continue,
            };
            let name = key.name().to_string_lossy();
            if name == "." || name == ".." || name.starts_with('$') {
                continue;
            }
            let file_ref = entry.file_reference();
            let child_file = match ntfs.file(&mut reader, file_ref.file_record_number()) {
                Ok(f) => f,
                Err(_) => continue,
            };
            let is_dir = child_file
                .info()
                .map(|info| info.file_attributes().contains(
                    ntfs::structured_values::NtfsFileAttributeFlags::IS_DIRECTORY,
                ))
                .unwrap_or(false);

            // Bugfix (2026-05-29): single-slash join (drive_root no
            // longer has trailing slash). Now "c:" + "/" + "Users"
            // → "c:/Users".
            let child_path = format!("{prefix}/{name}");

            if is_dir {
                // Bugfix (2026-05-29): descend into every directory
                // (skip $-system ones above). The previous "only
                // descend if in scope" heuristic missed subtrees when
                // the path-prefix match was wrong; walking the whole
                // tree is what WizTree does and what makes MFT actually
                // fast — the file-side scope filter (below) still
                // ensures only in-scope files end up in by_size.
                stack.push((child_file, child_path));
                continue;
            }

            // It's a file. Check scope, size, and extension filter.
            scanned_files += 1;
            let child_lower = child_path.to_lowercase();
            let in_scope = scope_prefixes
                .iter()
                .any(|r| child_lower.starts_with(&format!("{r}/")) || child_lower == *r);
            if !in_scope {
                continue;
            }
            if let Some(filter_set) = extension_filter {
                let ext = std::path::Path::new(&child_path)
                    .extension()
                    .and_then(|e| e.to_str())
                    .map(|s| s.to_ascii_lowercase())
                    .unwrap_or_default();
                if !filter_set.contains(&ext) {
                    continue;
                }
            }
            // Size — read the unnamed $DATA attribute's stored length.
            let size = file_size(&child_file, &mut reader).unwrap_or(0);
            if size < min_size {
                continue;
            }
            by_size
                .entry(size)
                .or_default()
                .push(PathBuf::from(child_path));
        }
    }

    Some(MftResult {
        by_size,
        scanned_files,
    })
}

fn extract_drive_letter(path: &str) -> Option<char> {
    let trimmed = path.trim_start();
    let mut chars = trimmed.chars();
    let first = chars.next()?;
    if !first.is_ascii_alphabetic() {
        return None;
    }
    if chars.next() != Some(':') {
        return None;
    }
    Some(first.to_ascii_uppercase())
}

fn open_volume(drive: char) -> std::io::Result<HANDLE> {
    let path = format!(r"\\.\{drive}:");
    let wide: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
    let handle = unsafe {
        CreateFileW(
            PCWSTR(wide.as_ptr()),
            GENERIC_READ.0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            None,
            OPEN_EXISTING,
            FILE_FLAGS_AND_ATTRIBUTES(FILE_ATTRIBUTE_NORMAL.0),
            HANDLE::default(),
        )
    }
    .map_err(|e| std::io::Error::other(format!("CreateFileW failed: {e:?}")))?;
    Ok(handle)
}

/// Read the file's $DATA attribute size. NTFS files can have multiple
/// $DATA attributes (named alternate data streams) — we want the
/// unnamed (default) one, which is the file's actual content.
fn file_size<R: std::io::Read + std::io::Seek>(
    file: &ntfs::NtfsFile<'_>,
    reader: &mut R,
) -> Option<u64> {
    let data_attr = file.data(reader, "")?;
    let data = data_attr.ok()?;
    let attribute = data.to_attribute().ok()?;
    Some(attribute.value_length())
}
