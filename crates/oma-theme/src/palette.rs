// SPDX-License-Identifier: GPL-3.0-or-later
//! Staged Omarchy palette loader.
//!
//! Source of truth is the *staged* theme directory
//! (`~/.local/state/omarchy/current/theme/colors.toml`), which
//! `omarchy-theme-set` assembles from the stock theme plus the user overlay
//! and swaps atomically. We never read `~/.config/omarchy/themes/*` directly
//! and never hardcode per-theme branches.

use crate::fallback;
use oma_core::{Error, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Canonical Omarchy palette. Every field is optional at parse time;
/// missing keys resolve via [`fallback`].
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Palette {
    pub mode: Option<String>,
    pub accent: Option<String>,
    pub selection: Option<String>,
    pub muted: Option<String>,
    pub background: Option<String>,
    pub dark_background: Option<String>,
    pub darker_background: Option<String>,
    pub lighter_background: Option<String>,
    pub foreground: Option<String>,
    pub dark_foreground: Option<String>,
    pub light_foreground: Option<String>,
    pub bright_foreground: Option<String>,
    pub red: Option<String>,
    pub yellow: Option<String>,
    pub orange: Option<String>,
    pub green: Option<String>,
    pub cyan: Option<String>,
    pub blue: Option<String>,
    pub magenta: Option<String>,
    pub brown: Option<String>,
    pub bright_red: Option<String>,
    pub bright_yellow: Option<String>,
    pub bright_green: Option<String>,
    pub bright_cyan: Option<String>,
    pub bright_blue: Option<String>,
    pub bright_magenta: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ThemeStatus {
    Available,
    Unavailable(String),
}

#[derive(Debug, Clone)]
pub struct LoadedTheme {
    pub palette: Palette,
    pub name: Option<String>,
    pub background: Option<PathBuf>,
    pub status: ThemeStatus,
}

impl LoadedTheme {
    /// Load from the live staged theme directory.
    pub fn load() -> Self {
        Self::load_from(&oma_core::paths::staged_theme_dir())
    }

    /// Load from an explicit staged-theme-shaped directory
    /// (`colors.toml` + optional `theme.name` + optional `background` link).
    pub fn load_from(dir: &Path) -> Self {
        let colors = dir.join("colors.toml");
        let text = match std::fs::read_to_string(&colors) {
            Ok(t) => t,
            Err(e) => {
                return Self {
                    palette: Palette::default(),
                    name: None,
                    background: None,
                    status: ThemeStatus::Unavailable(format!("{colors:?}: {e}")),
                };
            }
        };
        let palette: Palette = match toml::from_str(&text) {
            Ok(p) => p,
            Err(e) => {
                return Self {
                    palette: Palette::default(),
                    name: None,
                    background: None,
                    status: ThemeStatus::Unavailable(format!("{colors:?}: {e}")),
                };
            }
        };
        // Unknown extra keys (e.g. `hyprland_active_border`) are ignored by
        // serde's default behavior — only canonical keys are consumed.
        let name = std::fs::read_to_string(dir.join("theme.name"))
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        let background = std::fs::read_link(dir.join("background")).ok();
        Self {
            palette,
            name,
            background,
            status: ThemeStatus::Available,
        }
    }

    /// Load, mapping I/O failure to a typed error instead of a fallback.
    /// Prefer [`LoadedTheme::load`] for UI startup (never fails).
    pub fn load_strict(dir: &Path) -> Result<Self> {
        let loaded = Self::load_from(dir);
        match &loaded.status {
            ThemeStatus::Available => Ok(loaded),
            ThemeStatus::Unavailable(msg) => Err(Error::Theme(msg.clone())),
        }
    }

    pub fn is_dark(&self) -> bool {
        self.palette
            .mode
            .as_deref()
            .map(|m| m.eq_ignore_ascii_case("dark"))
            .unwrap_or(true)
    }

    // Resolved accessors — every one falls back, never panics.
    pub fn accent(&self) -> &str {
        self.palette
            .accent
            .as_deref()
            .unwrap_or(fallback::FALLBACK_ACCENT)
    }
    pub fn selection(&self) -> &str {
        self.palette
            .selection
            .as_deref()
            .unwrap_or(fallback::FALLBACK_SELECTION)
    }
    pub fn muted(&self) -> &str {
        self.palette
            .muted
            .as_deref()
            .unwrap_or(fallback::FALLBACK_MUTED)
    }
    pub fn background(&self) -> &str {
        self.palette
            .background
            .as_deref()
            .unwrap_or(fallback::FALLBACK_BACKGROUND)
    }
    pub fn foreground(&self) -> &str {
        self.palette
            .foreground
            .as_deref()
            .unwrap_or(fallback::FALLBACK_FOREGROUND)
    }
    pub fn view_background(&self) -> &str {
        self.palette
            .dark_background
            .as_deref()
            .unwrap_or(self.background())
    }
    pub fn header_background(&self) -> &str {
        self.palette
            .lighter_background
            .as_deref()
            .unwrap_or(self.background())
    }
    pub fn secondary_foreground(&self) -> &str {
        self.palette
            .dark_foreground
            .as_deref()
            .unwrap_or(self.muted())
    }
    pub fn danger(&self) -> &str {
        self.palette
            .red
            .as_deref()
            .unwrap_or(fallback::FALLBACK_RED)
    }
    pub fn warning(&self) -> &str {
        self.palette
            .yellow
            .as_deref()
            .unwrap_or(fallback::FALLBACK_YELLOW)
    }
    pub fn success(&self) -> &str {
        self.palette
            .green
            .as_deref()
            .unwrap_or(fallback::FALLBACK_GREEN)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join(name)
            .join("colors.toml")
            .parent()
            .expect("fixture dir")
            .to_path_buf()
    }

    #[test]
    fn parses_full_palette() {
        let dir = fixture("full");
        let theme = LoadedTheme::load_from(&dir);
        assert_eq!(theme.status, ThemeStatus::Available);
        assert_eq!(theme.name.as_deref(), Some("tokyo-night"));
        // Expected values come from the fixture file itself — no literals here.
        let raw: Palette =
            toml::from_str(&std::fs::read_to_string(dir.join("colors.toml")).expect("fixture"))
                .expect("parses");
        assert_eq!(theme.accent(), raw.accent.as_deref().expect("accent"));
        assert_eq!(
            theme.view_background(),
            raw.dark_background.as_deref().expect("dark_background")
        );
        assert!(theme.is_dark());
        assert!(theme.danger().starts_with('#'));
    }

    #[test]
    fn missing_keys_fall_back() {
        let theme = LoadedTheme::load_from(&fixture("minimal"));
        assert_eq!(theme.status, ThemeStatus::Available);
        assert_eq!(theme.accent(), fallback::FALLBACK_ACCENT);
        assert_eq!(theme.background(), fallback::FALLBACK_BACKGROUND);
        assert_eq!(theme.foreground(), fallback::FALLBACK_FOREGROUND);
        // Present keys still win; expected value from the fixture file.
        let raw: Palette = toml::from_str(
            &std::fs::read_to_string(fixture("minimal").join("colors.toml")).expect("fixture"),
        )
        .expect("parses");
        assert_eq!(theme.muted(), raw.muted.as_deref().expect("muted"));
        assert!(theme.is_dark());
    }

    #[test]
    fn empty_file_falls_back() {
        let dir = std::env::temp_dir().join(format!("oma-theme-empty-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("mkdir");
        std::fs::write(dir.join("colors.toml"), "").expect("write");
        let theme = LoadedTheme::load_from(&dir);
        // Empty TOML parses to all-None, which is Available with full
        // fallbacks — renderable, never a crash.
        assert_eq!(theme.status, ThemeStatus::Available);
        assert_eq!(theme.accent(), fallback::FALLBACK_ACCENT);
        let css = crate::css::to_gtk_css(&theme.palette);
        assert!(css.contains("@define-color oma_bg"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn light_mode_detected() {
        let dir = std::env::temp_dir().join(format!("oma-theme-light-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("mkdir");
        std::fs::write(dir.join("colors.toml"), "mode = \"light\"\n").expect("write");
        let theme = LoadedTheme::load_from(&dir);
        assert_eq!(theme.status, ThemeStatus::Available);
        assert!(!theme.is_dark());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn absent_dir_is_unavailable_not_panic() {
        let theme = LoadedTheme::load_from(Path::new("/nonexistent-oma-theme-dir-xyz"));
        assert!(matches!(theme.status, ThemeStatus::Unavailable(_)));
        // Fallbacks keep the UI renderable.
        assert_eq!(theme.accent(), fallback::FALLBACK_ACCENT);
        assert!(LoadedTheme::load_strict(Path::new("/nonexistent-oma-theme-dir-xyz")).is_err());
    }
}
