// SPDX-License-Identifier: GPL-3.0-or-later
//! Live reload: watch the staged theme dir and notify on change.
//!
//! The callback fires at most once per 500 ms (debounced). Callers must
//! retain the returned [`ThemeWatcher`] or watching stops.

use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

pub struct ThemeWatcher {
    _watcher: RecommendedWatcher,
}

fn interesting(path: &Path) -> bool {
    matches!(
        path.file_name().and_then(|s| s.to_str()),
        Some("colors.toml") | Some("theme.name")
    )
}

/// Watch `dir` (the staged theme dir); send `()` on every debounced change.
pub fn watch_staged(dir: PathBuf, tx: Sender<()>) -> oma_core::Result<ThemeWatcher> {
    let mut last_fire = Instant::now() - Duration::from_secs(3600);
    let mut watcher = RecommendedWatcher::new(
        move |res: Result<notify::Event, notify::Error>| {
            let Ok(event) = res else { return };
            let relevant = matches!(
                event.kind,
                EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_)
            ) && event.paths.iter().any(|p| interesting(p));
            if relevant {
                let now = Instant::now();
                if now.duration_since(last_fire) >= Duration::from_millis(500) {
                    last_fire = now;
                    tx.send(()).ok();
                }
            }
        },
        notify::Config::default(),
    )
    .map_err(|e| oma_core::Error::Config(format!("watch: {e}")))?;
    watcher
        .watch(&dir, RecursiveMode::NonRecursive)
        .map_err(|e| oma_core::Error::Config(format!("watch: {e}")))?;
    Ok(ThemeWatcher { _watcher: watcher })
}

/// Convenience: watch the live staged theme dir.
pub fn watch_live(tx: Sender<()>) -> oma_core::Result<ThemeWatcher> {
    watch_staged(oma_core::paths::staged_theme_dir(), tx)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    fn seed(dir: &Path) {
        // Seed content references fallback constants — no literals here.
        let text = format!(
            "mode = \"dark\"\naccent = \"{}\"\n",
            crate::fallback::FALLBACK_ACCENT
        );
        std::fs::write(dir.join("colors.toml"), text).expect("seed");
    }

    fn modified_seed() -> String {
        format!(
            "mode = \"dark\"\naccent = \"{}\"\n",
            crate::fallback::FALLBACK_SELECTION
        )
    }

    #[test]
    fn fires_on_colors_change() {
        let dir = std::env::temp_dir().join(format!("oma-watch-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("mkdir");
        seed(&dir);
        let (tx, rx) = mpsc::channel();
        let _watcher = watch_staged(dir.clone(), tx).expect("watch");
        std::thread::sleep(Duration::from_millis(100));
        std::fs::write(dir.join("colors.toml"), modified_seed()).expect("modify");
        rx.recv_timeout(Duration::from_secs(5)).expect("fires");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn ignores_unrelated_files() {
        let dir = std::env::temp_dir().join(format!("oma-watch-u-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("mkdir");
        seed(&dir);
        let (tx, rx) = mpsc::channel();
        let _watcher = watch_staged(dir.clone(), tx).expect("watch");
        std::thread::sleep(Duration::from_millis(100));
        std::fs::write(dir.join("notes.txt"), "hello").expect("write");
        assert!(rx.recv_timeout(Duration::from_millis(800)).is_err());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn deleted_theme_falls_back() {
        let theme = crate::palette::LoadedTheme::load_from(Path::new("/nonexistent-oma-theme-xyz"));
        assert!(matches!(
            theme.status,
            crate::palette::ThemeStatus::Unavailable(_)
        ));
        let css = crate::css::to_gtk_css(&theme.palette);
        assert!(css.contains("@define-color oma_bg"));
    }
}
