// SPDX-License-Identifier: GPL-3.0-or-later
//! Cover pipeline: embedded icon → write-once cache → user override.
//! Nothing downloaded, nothing painted here — paths only.

use oma_core::{Error, Result};
use std::path::{Path, PathBuf};

/// Resolve the cache dir, creating it on demand.
pub fn cache_dir() -> Result<PathBuf> {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/root"));
    let dir = home.join(".cache").join("omalaunch").join("covers");
    std::fs::create_dir_all(&dir).map_err(|e| Error::Io(dir.clone(), e))?;
    Ok(dir)
}

/// User-selected custom covers live here, keyed by app id.
pub fn artwork_dir() -> Result<PathBuf> {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/root"));
    let dir = home
        .join(".local")
        .join("share")
        .join("omalaunch")
        .join("artwork");
    std::fs::create_dir_all(&dir).map_err(|e| Error::Io(dir.clone(), e))?;
    Ok(dir)
}

fn ext_for(bytes: &[u8]) -> &'static str {
    if bytes.len() >= 8 && &bytes[0..8] == b"\x89PNG\r\n\x1a\n" {
        "png"
    } else if bytes.starts_with(b"<svg") || (bytes.len() >= 5 && &bytes[0..5] == b"<?xml") {
        "svg"
    } else {
        "bin"
    }
}

/// Cover for an app: artwork override wins, else write-once cache of the
/// embedded bytes. Returns `None` when there is no image (caller renders a
/// themed placeholder via CSS class).
pub fn cover_for(app_id: i64, icon_bytes: &[u8]) -> Result<Option<PathBuf>> {
    let custom = artwork_dir()?.join(format!("{app_id}"));
    for ext in ["png", "svg"] {
        let candidate = custom.with_extension(ext);
        if candidate.is_file() {
            return Ok(Some(candidate));
        }
    }
    if icon_bytes.is_empty() {
        return Ok(None);
    }
    let digest = hex_md5(icon_bytes);
    let cached = cache_dir()?.join(format!("{digest}.{}", ext_for(icon_bytes)));
    if !cached.is_file() {
        std::fs::write(&cached, icon_bytes).map_err(|e| Error::Io(cached.clone(), e))?;
    }
    Ok(Some(cached))
}

/// Install a user-selected cover file for an app.
pub fn set_custom_cover(app_id: i64, file: &Path) -> Result<PathBuf> {
    let bytes = std::fs::read(file).map_err(|e| Error::Io(file.to_path_buf(), e))?;
    if bytes.is_empty() {
        return Err(Error::Integration("empty cover file".to_string()));
    }
    let dest = artwork_dir()?.join(format!("{app_id}.{}", ext_for(&bytes)));
    std::fs::write(&dest, &bytes).map_err(|e| Error::Io(dest.clone(), e))?;
    Ok(dest)
}

fn hex_md5(bytes: &[u8]) -> String {
    let digest = md5::compute(bytes);
    digest.0.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn override_wins_cache_reused_missing_none() {
        let dir = std::env::temp_dir().join(format!("oma-cov-{}", std::process::id()));
        // Redirect HOME for hermetic paths (restored at the end).
        let old = std::env::var_os("HOME");
        std::env::set_var("HOME", &dir);
        let png = b"\x89PNG\r\n\x1a\nfakepng".to_vec();
        let first = cover_for(7, &png).expect("cover").expect("some");
        let mtime = std::fs::metadata(&first)
            .expect("meta")
            .modified()
            .expect("mtime");
        let second = cover_for(7, &png).expect("cover").expect("some");
        assert_eq!(first, second);
        assert_eq!(
            std::fs::metadata(&second)
                .expect("meta")
                .modified()
                .expect("mtime"),
            mtime,
            "cache write-once"
        );
        assert!(cover_for(8, &[]).expect("empty").is_none());
        let custom_src = dir.join("custom.png");
        std::fs::write(&custom_src, &png).expect("write");
        let custom = set_custom_cover(7, &custom_src).expect("set");
        assert_eq!(cover_for(7, &png).expect("cover"), Some(custom));
        assert!(set_custom_cover(9, &dir.join("missing.png")).is_err());
        match old {
            Some(v) => std::env::set_var("HOME", v),
            None => std::env::remove_var("HOME"),
        }
        std::fs::remove_dir_all(&dir).ok();
    }
}
