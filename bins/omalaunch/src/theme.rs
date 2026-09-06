// SPDX-License-Identifier: GPL-3.0-or-later
//! Runtime Omarchy theme application for the GTK shell.
//! CSS text generation is headless-testable; provider installation needs a display.

/// Current stylesheet + dark-mode flag + font + opacity from the staged theme.
pub struct ThemeState {
    pub css: String,
    pub is_dark: bool,
    pub font: Option<String>,
    pub alpha: f64,
}

pub fn current_theme() -> ThemeState {
    let theme = oma_theme::palette::LoadedTheme::load();
    ThemeState {
        css: oma_theme::css::to_gtk_css(&theme.palette),
        is_dark: theme.is_dark(),
        font: oma_theme::font::system_font(),
        alpha: oma_theme::font::shell_alpha(),
    }
}

/// Install `css` on the default display, sync dark preference and font.
pub fn apply(provider: &gtk::CssProvider, css: &str, is_dark: bool, font: Option<&str>) {
    provider.load_from_data(css);
    if let Some(settings) = gtk::Settings::default() {
        settings.set_gtk_application_prefer_dark_theme(is_dark);
        if let Some(font) = font {
            settings.set_gtk_font_name(Some(font));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn css_always_renders() {
        let state = current_theme();
        assert!(state.css.contains("@define-color oma_bg"));
        assert!(state.css.contains("window"));
    }

    #[test]
    fn theme_state_has_sane_defaults() {
        let state = current_theme();
        assert!(!state.css.is_empty());
        assert!((0.0..=1.0).contains(&state.alpha));
    }
}
