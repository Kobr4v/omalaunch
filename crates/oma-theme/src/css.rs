// SPDX-License-Identifier: GPL-3.0-or-later
//! GTK CSS generator: semantic roles only, every color from the palette.

use crate::palette::{LoadedTheme, Palette, ThemeStatus};

/// Build GTK4 CSS where every color value originates from `palette`
/// (or the documented fallbacks). No literal colors appear here by
/// construction — only `@variable` references and structural rules.
pub fn to_gtk_css(palette: &Palette) -> String {
    let theme = LoadedTheme {
        palette: palette.clone(),
        name: None,
        background: None,
        status: ThemeStatus::Available,
    };
    let (bg, fg, accent, sel, muted, danger, warning, success) = (
        theme.background(),
        theme.foreground(),
        theme.accent(),
        theme.selection(),
        theme.muted(),
        theme.danger(),
        theme.warning(),
        theme.success(),
    );
    let (view_bg, header_bg, secondary) = (
        theme.view_background(),
        theme.header_background(),
        theme.secondary_foreground(),
    );
    format!(
        "@define-color oma_bg {bg};\n\
         @define-color oma_view_bg {view_bg};\n\
         @define-color oma_header_bg {header_bg};\n\
         @define-color oma_fg {fg};\n\
         @define-color oma_secondary {secondary};\n\
         @define-color oma_muted {muted};\n\
         @define-color oma_accent {accent};\n\
         @define-color oma_selection {sel};\n\
         @define-color oma_danger {danger};\n\
         @define-color oma_warning {warning};\n\
         @define-color oma_success {success};\n\
         window {{ background-color: @oma_bg; color: @oma_fg; }}\n\
         headerbar {{ background-color: @oma_header_bg; color: @oma_fg; }}\n\
         textview, entry {{ background-color: @oma_view_bg; color: @oma_fg; }}\n\
         button.suggested-action {{ background-color: @oma_accent; color: @oma_bg; }}\n\
         row:selected, .selected {{ background-color: @oma_selection; }}\n\
         .dim-label {{ color: @oma_secondary; }}\n\
         .muted {{ color: @oma_muted; }}\n\
         button.destructive-action {{ background-color: @oma_danger; }}\n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn palette_from_fixture(name: &str) -> Palette {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join(name);
        let text = std::fs::read_to_string(dir.join("colors.toml")).expect("fixture readable");
        toml::from_str(&text).expect("fixture parses")
    }

    fn hex_tokens(css: &str) -> Vec<String> {
        let bytes = css.as_bytes();
        let mut out = Vec::new();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'#' {
                let mut j = i + 1;
                while j < bytes.len() && (bytes[j] as char).is_ascii_hexdigit() && j - i < 9 {
                    j += 1;
                }
                if j - i >= 4 {
                    out.push(css[i..j].to_string());
                }
                i = j;
            } else {
                i += 1;
            }
        }
        out
    }

    fn allowed_values(p: &Palette) -> Vec<String> {
        let opts = [
            &p.accent,
            &p.selection,
            &p.muted,
            &p.background,
            &p.dark_background,
            &p.darker_background,
            &p.lighter_background,
            &p.foreground,
            &p.dark_foreground,
            &p.light_foreground,
            &p.bright_foreground,
            &p.red,
            &p.yellow,
            &p.orange,
            &p.green,
            &p.cyan,
            &p.blue,
            &p.magenta,
            &p.brown,
            &p.bright_red,
            &p.bright_yellow,
            &p.bright_green,
            &p.bright_cyan,
            &p.bright_blue,
            &p.bright_magenta,
        ];
        let mut values: Vec<String> = opts
            .iter()
            .filter_map(|o| o.as_deref())
            .map(str::to_string)
            .collect();
        values.extend(
            [
                crate::fallback::FALLBACK_ACCENT,
                crate::fallback::FALLBACK_BACKGROUND,
                crate::fallback::FALLBACK_FOREGROUND,
                crate::fallback::FALLBACK_SELECTION,
                crate::fallback::FALLBACK_MUTED,
                crate::fallback::FALLBACK_RED,
                crate::fallback::FALLBACK_YELLOW,
                crate::fallback::FALLBACK_GREEN,
            ]
            .iter()
            .map(|s| s.to_string()),
        );
        values
    }

    #[test]
    fn every_emitted_hex_comes_from_palette() {
        for fixture in ["full", "minimal"] {
            let palette = palette_from_fixture(fixture);
            let css = to_gtk_css(&palette);
            let allowed = allowed_values(&palette);
            let tokens = hex_tokens(&css);
            assert!(!tokens.is_empty(), "css must define colors");
            for token in &tokens {
                assert!(
                    allowed.iter().any(|v| v.eq_ignore_ascii_case(token)),
                    "hex {token} not found in palette or fallbacks (fixture {fixture})"
                );
            }
        }
    }

    #[test]
    fn different_palettes_different_css() {
        let a = to_gtk_css(&palette_from_fixture("full"));
        let b = to_gtk_css(&palette_from_fixture("minimal"));
        assert_ne!(a, b, "no theme may be baked in");
        assert!(a.contains("@define-color oma_bg"));
        assert!(a.contains("window"));
    }
}
