//! Programs the browser installed as packs (Settings › Packs): FFmpeg now,
//! Tesseract and others later.
//!
//! The browser downloads, verifies and unpacks a pack, then writes where
//! its programs are to `packs.json` in the engine's data folder:
//! `{"ffmpeg": "C:\\…\\Packs\\ffmpeg\\<version>\\bin"}`. Removing the pack
//! takes its line out. The file is read each time a program is looked for,
//! so a pack installed or removed while the engine runs counts at once, and
//! no call can race the browser telling the engine about it.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

static DATA_DIR: OnceLock<PathBuf> = OnceLock::new();

const FILE: &str = "packs.json";

/// Where the browser writes `packs.json`: the engine's `--data-dir`.
pub fn set_data_dir(dir: PathBuf) {
    let _ = DATA_DIR.set(dir);
}

/// The folder holding `pack`'s programs, when the browser has installed it
/// and the folder is still there.
pub fn bin_dir(pack: &str) -> Option<PathBuf> {
    bin_dir_in(DATA_DIR.get()?, pack)
}

fn bin_dir_in(data_dir: &Path, pack: &str) -> Option<PathBuf> {
    let text = std::fs::read_to_string(data_dir.join(FILE)).ok()?;
    let listed: serde_json::Value = serde_json::from_str(&text).ok()?;
    let dir = PathBuf::from(listed.get(pack)?.as_str()?);
    (dir.is_absolute() && dir.is_dir()).then_some(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pack_is_found_only_while_its_listed_folder_exists() {
        let data = std::env::temp_dir().join(format!("kil-packs-{}", std::process::id()));
        let bin = data.join("ffmpeg-bin");
        std::fs::create_dir_all(&bin).unwrap();
        let listed = serde_json::json!({ "ffmpeg": bin.to_string_lossy() });
        std::fs::write(data.join(FILE), listed.to_string()).unwrap();

        assert_eq!(bin_dir_in(&data, "ffmpeg"), Some(bin.clone()));
        assert_eq!(bin_dir_in(&data, "tesseract"), None);

        // Removed by hand (or half-deleted): not a pack to run programs from.
        std::fs::remove_dir(&bin).unwrap();
        assert_eq!(bin_dir_in(&data, "ffmpeg"), None);
        std::fs::remove_dir_all(&data).unwrap();
    }
}
