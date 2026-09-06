// SPDX-License-Identifier: GPL-3.0-or-later
//! Icon install, desktop/icon cache refresh, and the integration registry.
//!
//! All locations are explicit parameters (sandbox-friendly); production
//! callers pass `~/.local/share`-derived dirs.

use oma_core::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RegistryEntry {
    pub integrated_path: PathBuf,
    pub desktop_path: PathBuf,
    pub icon_paths: Vec<PathBuf>,
    #[serde(default)]
    pub comment: String,
    #[serde(default)]
    pub categories: Vec<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Registry {
    #[serde(default)]
    pub entries: HashMap<String, RegistryEntry>,
}

impl Registry {
    pub fn load_from(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|t| toml::from_str(&t).ok())
            .unwrap_or_default()
    }

    pub fn save_to(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| Error::Io(parent.to_path_buf(), e))?;
        }
        let text = toml::to_string_pretty(self).map_err(|e| Error::Config(e.to_string()))?;
        std::fs::write(path, text).map_err(|e| Error::Io(path.to_path_buf(), e))
    }

    pub fn is_registered(&self, integrated_path: &Path) -> bool {
        self.entries
            .contains_key(&integrated_path.to_string_lossy().into_owned())
    }

    pub fn insert(&mut self, entry: RegistryEntry) {
        self.entries
            .insert(entry.integrated_path.to_string_lossy().into_owned(), entry);
    }

    pub fn remove(&mut self, integrated_path: &Path) -> Option<RegistryEntry> {
        self.entries
            .remove(&integrated_path.to_string_lossy().into_owned())
    }
}

const STANDARD_SIZES: [u32; 10] = [16, 22, 24, 32, 48, 64, 96, 128, 256, 512];

fn png_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.len() >= 24 && &bytes[0..8] == b"\x89PNG\r\n\x1a\n" && &bytes[12..16] == b"IHDR" {
        let w = u32::from_be_bytes(bytes[16..20].try_into().ok()?);
        let h = u32::from_be_bytes(bytes[20..24].try_into().ok()?);
        Some((w, h))
    } else {
        None
    }
}

fn nearest_size(w: u32, h: u32) -> u32 {
    let want = w.max(h).max(1);
    let mut best = STANDARD_SIZES[0];
    for size in STANDARD_SIZES {
        if (size as i64 - want as i64).abs() < (best as i64 - want as i64).abs() {
            best = size;
        }
    }
    best
}

/// Install icon bytes under `<data_dir>/icons/hicolor/<size>/apps/<name>.<ext>`.
/// Returns the installed path. `ext` is `png`, `svg`, or `xpm`.
pub fn install_icon(data_dir: &Path, icon_name: &str, ext: &str, bytes: &[u8]) -> Result<PathBuf> {
    let size_dir = if ext.eq_ignore_ascii_case("svg") {
        "scalable".to_string()
    } else if ext.eq_ignore_ascii_case("png") {
        match png_dimensions(bytes) {
            Some((w, h)) => format!("{}x{}", nearest_size(w, h), nearest_size(w, h)),
            None => "256x256".to_string(),
        }
    } else {
        "256x256".to_string()
    };
    let dir = data_dir
        .join("icons")
        .join("hicolor")
        .join(size_dir)
        .join("apps");
    std::fs::create_dir_all(&dir).map_err(|e| Error::Io(dir.clone(), e))?;
    let path = dir.join(format!("{icon_name}.{ext}"));
    std::fs::write(&path, bytes).map_err(|e| Error::Io(path.clone(), e))?;
    Ok(path)
}

fn have(cmd: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|paths| std::env::split_paths(&paths).any(|d| d.join(cmd).is_file()))
}

fn run(cmd: &str, arg: &str) {
    if have(cmd) {
        std::process::Command::new(cmd).arg(arg).output().ok();
    }
}

/// Refresh desktop and icon caches; missing helpers are skipped silently,
/// matching the old `updateDesktopDatabaseAndIconCaches` behavior.
/// Exit codes are intentionally ignored.
pub fn refresh_caches(data_dir: &Path, apps_dir: &Path) {
    let icons = data_dir.join("icons");
    run("update-desktop-database", &apps_dir.to_string_lossy());
    run(
        "gtk-update-icon-cache-3.0",
        &format!("{} -t", icons.join("hicolor").to_string_lossy()),
    );
    run(
        "gtk-update-icon-cache",
        &format!("{} -t", icons.join("hicolor").to_string_lossy()),
    );
    run("xdg-desktop-menu", "forceupdate");
    run(
        "update-mime-database",
        &data_dir.join("mime").to_string_lossy(),
    );
    run("update-icon-caches", &icons.to_string_lossy());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sandbox(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("oma-icons-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("sandbox");
        dir
    }

    const SVG: &[u8] = b"<svg xmlns=\"http://www.w3.org/2000/svg\"></svg>";

    fn png_1x1() -> Vec<u8> {
        let mut b = vec![0u8; 33];
        b[0..8].copy_from_slice(b"\x89PNG\r\n\x1a\n");
        b[8..12].copy_from_slice(&13u32.to_be_bytes());
        b[12..16].copy_from_slice(b"IHDR");
        b[16..20].copy_from_slice(&1u32.to_be_bytes());
        b[20..24].copy_from_slice(&1u32.to_be_bytes());
        b
    }

    #[test]
    fn svg_goes_scalable_png_sniffs_size() {
        let dir = sandbox("sizes");
        let svg_path = install_icon(&dir, "myapp", "svg", SVG).expect("svg");
        assert!(svg_path.to_string_lossy().contains("scalable"));
        let png_path = install_icon(&dir, "myapp", "png", &png_1x1()).expect("png");
        assert!(png_path.to_string_lossy().contains("16x16"));
        assert!(png_path.is_file());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn registry_round_trip_and_unregister_cleanup() {
        let dir = sandbox("reg");
        let reg_path = dir.join("registry.toml");
        let mut reg = Registry::load_from(&reg_path);
        assert!(!reg.is_registered(Path::new("/a/Test.AppImage")));
        let icon = install_icon(&dir, "test", "svg", SVG).expect("icon");
        let desktop = dir.join("applications").join("test.desktop");
        std::fs::create_dir_all(desktop.parent().expect("parent")).expect("mkdir");
        std::fs::write(&desktop, "[Desktop Entry]\n").expect("write");
        reg.insert(RegistryEntry {
            integrated_path: PathBuf::from("/a/Test.AppImage"),
            desktop_path: desktop.clone(),
            icon_paths: vec![icon.clone()],
            comment: String::new(),
            categories: Vec::new(),
        });
        reg.save_to(&reg_path).expect("save");
        let back = Registry::load_from(&reg_path);
        assert!(back.is_registered(Path::new("/a/Test.AppImage")));
        // Unregister removes tracked files.
        let mut live = back;
        let entry = live.remove(Path::new("/a/Test.AppImage")).expect("present");
        let mut tracked: Vec<&Path> = vec![&entry.desktop_path];
        tracked.extend(entry.icon_paths.iter().map(PathBuf::as_path));
        for p in tracked {
            std::fs::remove_file(p).ok();
        }
        assert!(!desktop.exists());
        assert!(!icon.exists());
        assert!(!live.is_registered(Path::new("/a/Test.AppImage")));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn unregister_unknown_is_none() {
        let mut reg = Registry::default();
        assert!(reg.remove(Path::new("/nope.AppImage")).is_none());
    }

    #[test]
    fn refresh_without_helpers_is_ok() {
        // Empty PATH: every helper missing, must still succeed silently.
        let old = std::env::var_os("PATH");
        std::env::set_var("PATH", "/nonexistent-oma-dir");
        let dir = sandbox("refresh");
        refresh_caches(&dir, &dir.join("applications"));
        match old {
            Some(v) => std::env::set_var("PATH", v),
            None => std::env::remove_var("PATH"),
        }
        std::fs::remove_dir_all(&dir).ok();
    }
}
