// SPDX-License-Identifier: GPL-3.0-or-later
//! Well-known filesystem locations.

use crate::{Error, Result};
use std::path::PathBuf;

fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            // Fallback mirrors the old C++ `getenv("HOME")` behavior of assuming a root home.
            PathBuf::from("/root")
        })
}

/// Expand a leading `~` or `~/` to `$HOME`.
pub fn expand_tilde(path: &str) -> String {
    if path == "~" || path.starts_with("~/") {
        let mut expanded = home().to_string_lossy().into_owned();
        expanded.push_str(&path[1..]);
        expanded
    } else {
        path.to_string()
    }
}

/// `~/Applications` — default integration destination.
pub fn integration_dir() -> PathBuf {
    home().join("Applications")
}

/// `~/Applications/.trash` — removal staging area.
pub fn trash_dir() -> PathBuf {
    integration_dir().join(".trash")
}

/// `~/.config/omalaunch/omalaunch.toml`.
pub fn config_path() -> PathBuf {
    home()
        .join(".config")
        .join("omalaunch")
        .join("omalaunch.toml")
}

/// `~/.local/share` — freedesktop data dir.
pub fn data_dir() -> PathBuf {
    home().join(".local").join("share")
}

/// `~/.local/state/omarchy/current/theme` — staged, overlay-resolved theme.
pub fn staged_theme_dir() -> PathBuf {
    home()
        .join(".local")
        .join("state")
        .join("omarchy")
        .join("current")
        .join("theme")
}

fn ensure_dir(dir: &PathBuf) -> Result<PathBuf> {
    std::fs::create_dir_all(dir)
        .map_err(|e| Error::Io(dir.clone(), e))
        .map(|()| dir.clone())
}

/// Resolve the integration dir, creating it on demand.
pub fn ensure_integration_dir() -> Result<PathBuf> {
    ensure_dir(&integration_dir())
}

/// Resolve the trash dir, creating it on demand.
pub fn ensure_trash_dir() -> Result<PathBuf> {
    ensure_dir(&trash_dir())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn with_home(home: &str, f: impl FnOnce()) {
        let _guard = ENV_LOCK.lock().unwrap();
        let old = std::env::var_os("HOME");
        std::env::set_var("HOME", home);
        f();
        match old {
            Some(v) => std::env::set_var("HOME", v),
            None => std::env::remove_var("HOME"),
        }
    }

    #[test]
    fn resolves_under_home() {
        with_home("/tmp/oma-test-home", || {
            assert_eq!(
                integration_dir(),
                PathBuf::from("/tmp/oma-test-home/Applications")
            );
            assert_eq!(
                config_path(),
                PathBuf::from("/tmp/oma-test-home/.config/omalaunch/omalaunch.toml")
            );
            assert_eq!(
                staged_theme_dir(),
                PathBuf::from("/tmp/oma-test-home/.local/state/omarchy/current/theme")
            );
        });
    }

    #[test]
    fn creates_on_demand_in_empty_home() {
        let dir = std::env::temp_dir().join(format!("oma-home-{}", std::process::id()));
        let home = dir.to_string_lossy().into_owned();
        with_home(&home, || {
            let created = ensure_integration_dir().expect("must create");
            assert!(created.is_dir());
            let trash = ensure_trash_dir().expect("must create");
            assert!(trash.is_dir());
        });
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn tilde_expansion() {
        with_home("/tmp/oma-test-home", || {
            assert_eq!(
                expand_tilde("~/Applications"),
                "/tmp/oma-test-home/Applications"
            );
            assert_eq!(expand_tilde("~"), "/tmp/oma-test-home");
            assert_eq!(expand_tilde("/abs/path"), "/abs/path");
            assert_eq!(expand_tilde("relative/path"), "relative/path");
        });
    }
}
