// SPDX-License-Identifier: GPL-3.0-or-later
//! Configuration: load/save TOML plus legacy `appimagelauncher.cfg` import.

use crate::paths::{config_path, expand_tilde};
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default = "default_true")]
    pub ask_to_move: bool,
    pub destination: Option<String>,
    #[serde(default = "default_true")]
    pub enable_daemon: bool,
    #[serde(default)]
    pub extra_watch_dirs: Vec<String>,
    #[serde(default)]
    pub monitor_mounted_filesystems: bool,
    /// Cover grid (`true`) vs list (`false`) library view.
    #[serde(default)]
    pub grid_view: bool,
    /// Quit the library window right after launching an app.
    #[serde(default)]
    pub close_after_launch: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            ask_to_move: true,
            destination: None,
            enable_daemon: true,
            extra_watch_dirs: Vec::new(),
            monitor_mounted_filesystems: false,
            grid_view: false,
            close_after_launch: false,
        }
    }
}

fn default_true() -> bool {
    true
}

impl Config {
    /// Load from the standard path; missing file yields defaults.
    pub fn load() -> Result<Self> {
        Self::load_from(&config_path())
    }

    pub fn load_from(path: &Path) -> Result<Self> {
        match std::fs::read_to_string(path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(Error::Io(path.to_path_buf(), e)),
            Ok(text) => toml::from_str(&text).map_err(|e| Error::Config(format!("{path:?}: {e}"))),
        }
    }

    /// Persist to the standard path, creating parent dirs.
    pub fn save(&self) -> Result<()> {
        self.save_to(&config_path())
    }

    pub fn save_to(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| Error::Io(parent.to_path_buf(), e))?;
        }
        let text = toml::to_string_pretty(self).map_err(|e| Error::Config(e.to_string()))?;
        std::fs::write(path, text).map_err(|e| Error::Io(path.to_path_buf(), e))
    }

    /// Effective integration destination after tilde expansion.
    pub fn effective_destination(&self) -> String {
        match &self.destination {
            Some(d) if !d.is_empty() => expand_tilde(d),
            _ => crate::paths::integration_dir()
                .to_string_lossy()
                .into_owned(),
        }
    }

    /// Import a legacy AppImageLauncher INI config (`appimagelauncher.cfg`).
    ///
    /// Understands `[AppImageLauncher] ask_to_move/destination/enable_daemon`
    /// and `[appimagelauncherd] additional_directories_to_watch` (`:`-separated)
    /// plus `monitor_mounted_filesystems`. Commented (`#`-prefixed) keys are
    /// treated as unset, matching the old commented-defaults style.
    pub fn import_legacy(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path).map_err(|e| Error::Io(path.to_path_buf(), e))?;
        let mut sections: HashMap<String, HashMap<String, String>> = HashMap::new();
        let mut current = String::new();
        for raw in text.lines() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
                continue;
            }
            if line.starts_with('[') && line.ends_with(']') {
                current = line[1..line.len() - 1].to_lowercase();
                continue;
            }
            if let Some((k, v)) = line.split_once('=') {
                sections.entry(current.clone()).or_default().insert(
                    k.trim().to_lowercase(),
                    v.trim().trim_matches('"').to_string(),
                );
            }
        }
        let get = |section: &str, key: &str| -> Option<String> {
            sections.get(section).and_then(|s| s.get(key)).cloned()
        };
        let parse_bool =
            |v: &str| -> bool { matches!(v.to_lowercase().as_str(), "true" | "1" | "yes") };

        let mut cfg = Self::default();
        if let Some(v) = get("appimagelauncher", "ask_to_move") {
            cfg.ask_to_move = parse_bool(&v);
        }
        if let Some(v) = get("appimagelauncher", "destination") {
            if !v.is_empty() {
                cfg.destination = Some(v);
            }
        }
        if let Some(v) = get("appimagelauncher", "enable_daemon") {
            cfg.enable_daemon = parse_bool(&v);
        }
        if let Some(v) = get("appimagelauncherd", "additional_directories_to_watch") {
            cfg.extra_watch_dirs = v
                .split(':')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect();
        }
        if let Some(v) = get("appimagelauncherd", "monitor_mounted_filesystems") {
            cfg.monitor_mounted_filesystems = parse_bool(&v);
        }
        Ok(cfg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_yields_defaults() {
        let cfg = Config::load_from(Path::new("/nonexistent-oma-toml-path/config.toml"))
            .expect("must default");
        assert!(cfg.ask_to_move);
        assert!(cfg.enable_daemon);
        assert!(cfg.destination.is_none());
    }

    #[test]
    fn round_trip() {
        let dir = std::env::temp_dir().join(format!("oma-cfg-{}", std::process::id()));
        let path = dir.join("omalaunch.toml");
        let cfg = Config {
            ask_to_move: false,
            destination: Some("~/MyApps".to_string()),
            enable_daemon: true,
            extra_watch_dirs: vec!["~/extra".to_string()],
            monitor_mounted_filesystems: true,
            grid_view: true,
            close_after_launch: true,
        };
        cfg.save_to(&path).expect("save");
        let back = Config::load_from(&path).expect("load");
        assert!(!back.ask_to_move);
        assert_eq!(back.destination.as_deref(), Some("~/MyApps"));
        assert_eq!(back.extra_watch_dirs, vec!["~/extra".to_string()]);
        assert!(back.monitor_mounted_filesystems);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn legacy_import() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("legacy.cfg");
        let cfg = Config::import_legacy(&path).expect("import");
        assert!(!cfg.ask_to_move);
        assert_eq!(cfg.destination.as_deref(), Some("~/MyApplications"));
        assert!(cfg.enable_daemon);
        assert_eq!(
            cfg.extra_watch_dirs,
            vec!["~/otherApps".to_string(), "/media/apps".to_string()]
        );
        assert!(!cfg.monitor_mounted_filesystems);
    }
}
