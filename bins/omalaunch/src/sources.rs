// SPDX-License-Identifier: GPL-3.0-or-later
//! Sources model: watch directories + library as toggleable sources with
//! persisted enablement and last-scan times. Pure logic, headless-tested.

use oma_core::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    pub id: String,
    pub kind: SourceKind,
    pub path: PathBuf,
    pub enabled: bool,
    pub last_scan: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceKind {
    WatchDir,
    Library,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct SourceState {
    #[serde(default)]
    enabled: HashMap<String, bool>,
    #[serde(default)]
    last_scan: HashMap<String, u64>,
}

fn state_path() -> Result<PathBuf> {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/root"));
    Ok(home
        .join(".local")
        .join("share")
        .join("omalaunch")
        .join("sources.toml"))
}

fn load_state() -> SourceState {
    let Ok(path) = state_path() else {
        return SourceState::default();
    };
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|t| toml::from_str(&t).ok())
        .unwrap_or_default()
}

fn save_state(state: &SourceState) -> Result<()> {
    let path = state_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| Error::Io(parent.to_path_buf(), e))?;
    }
    let text = toml::to_string_pretty(state).map_err(|e| Error::Config(e.to_string()))?;
    std::fs::write(&path, text).map_err(|e| Error::Io(path, e))
}

/// Build the source list from config + watch set.
pub fn list_sources(
    config: &oma_core::config::Config,
    watch_dirs: &[PathBuf],
    library_count: usize,
) -> Vec<Source> {
    let _ = library_count;
    let state = load_state();
    let mut sources = Vec::new();
    let library_dir = PathBuf::from(config.effective_destination());
    let library_id = "library".to_string();
    sources.push(Source {
        id: library_id.clone(),
        kind: SourceKind::Library,
        path: library_dir,
        enabled: state.enabled.get(&library_id).copied().unwrap_or(true),
        last_scan: state.last_scan.get(&library_id).copied(),
    });
    for dir in watch_dirs {
        if dir == &sources[0].path {
            continue;
        }
        let id = format!("dir:{}", dir.to_string_lossy());
        sources.push(Source {
            id: id.clone(),
            kind: SourceKind::WatchDir,
            path: dir.clone(),
            enabled: state.enabled.get(&id).copied().unwrap_or(true),
            last_scan: state.last_scan.get(&id).copied(),
        });
    }
    let _ = config;
    sources.sort_by(|a, b| a.path.cmp(&b.path));
    sources
}

/// Persist an enablement toggle.
pub fn set_enabled(id: &str, enabled: bool) -> Result<()> {
    let mut state = load_state();
    state.enabled.insert(id.to_string(), enabled);
    save_state(&state)
}

/// Record a completed scan; returns the number of AppImages found.
pub fn rescan(dir: &Path) -> usize {
    let found = oma_daemon::scan_dir(dir);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let mut state = load_state();
    state
        .last_scan
        .insert(format!("dir:{}", dir.to_string_lossy()), now);
    save_state(&state).ok();
    found.len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn with_home(home: &Path, f: impl FnOnce()) {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let old = std::env::var_os("HOME");
        std::env::set_var("HOME", home);
        f();
        match old {
            Some(v) => std::env::set_var("HOME", v),
            None => std::env::remove_var("HOME"),
        }
    }

    #[test]
    fn toggle_persists_and_scan_records() {
        let home = std::env::temp_dir().join(format!("oma-src-{}", std::process::id()));
        std::fs::create_dir_all(&home).expect("mkdir");
        with_home(&home, || {
            let config = oma_core::config::Config::default();
            let dirs = vec![home.join("Apps")];
            std::fs::create_dir_all(&dirs[0]).expect("mkdir");
            let sources = list_sources(&config, &dirs, 0);
            assert_eq!(sources.len(), 2);
            assert_eq!(sources[0].kind, SourceKind::Library);
            assert!(sources[1].enabled);
            assert_eq!(sources[1].last_scan, None);
            set_enabled(&sources[1].id, false).expect("toggle");
            let again = list_sources(&config, &dirs, 0);
            assert!(!again[1].enabled);
            let n = rescan(&dirs[0]);
            assert_eq!(n, 0);
            let scanned = list_sources(&config, &dirs, 0);
            assert!(scanned[1].last_scan.is_some());
        });
        std::fs::remove_dir_all(&home).ok();
    }

    #[test]
    fn unreadable_dir_does_not_crash() {
        let home = std::env::temp_dir().join(format!("oma-src-u-{}", std::process::id()));
        std::fs::create_dir_all(&home).expect("mkdir");
        with_home(&home, || {
            assert_eq!(rescan(Path::new("/nonexistent-oma-dir-xyz")), 0);
            let sources = list_sources(
                &oma_core::config::Config::default(),
                &[PathBuf::from("/nonexistent-oma-dir-xyz")],
                0,
            );
            assert_eq!(sources.len(), 2);
        });
        std::fs::remove_dir_all(&home).ok();
    }
}
