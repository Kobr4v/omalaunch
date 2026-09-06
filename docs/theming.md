# Omalaunch theming contract

Omalaunch has **zero hardcoded colors**. Every pixel of styling resolves at
runtime from your current Omarchy theme. If you can see a color in the app,
it came from the theme.

## Source of truth

- The staged directory `~/.local/state/omarchy/current/theme/`:
  `colors.toml` (palette), `theme.name`, `background` symlink.
- `omarchy-theme-set` assembles this directory from the stock theme plus
  your overlay and swaps it atomically. Never read
  `~/.config/omarchy/themes/*` directly, and never edit anything under
  `/usr/share/omarchy/` — both rules are load-bearing.

## How it works

1. `oma-theme::palette::LoadedTheme::load()` parses the staged
   `colors.toml`. Unknown keys (e.g. `hyprland_active_border`) are ignored;
   missing keys resolve to documented neutral fallbacks
   (`oma-theme/src/fallback.rs` — the only non-fixture location allowed to
   contain color literals).
2. `oma-theme::css::to_gtk_css()` maps canonical keys to semantic roles
   (`oma_bg`, `oma_fg`, `oma_accent`, `oma_selection`, danger/warning/…).
   A property test asserts every emitted hex originates from the palette.
3. The GTK app installs the CSS via a `CssProvider`, follows `mode` for the
   dark preference, applies the Omarchy font (`omarchy font current`) to
   `gtk-font-name`, and sets window opacity from staged `shell.toml`
   `background-alpha` (1.0 when absent).
4. A file watcher on the staged dir reloads CSS + font + opacity within ~1 s
   of any `omarchy theme set`, with 500 ms debouncing. The shipped hook
   `hooks/theme-set.d/omalaunch` (install: `omarchy hook install theme-set
   hooks/theme-set.d/omalaunch`) pings `omalaunch --refresh-theme` so the
   subscription is visible in `omarchy hook` listings.

## Fallback table

Missing key → fallback: `accent` → neutral gray, `background` → dark gray,
`foreground` → light gray, `selection`/`muted` → mid grays, `red`/`yellow`/
`green` → muted danger/warning/success. Missing file entirely → full
fallbacks, app stays usable.

## Enforcement

`scripts/no-hardcoded-colors.sh` fails the build on any hex literal outside
`tests/fixtures/`, `fallback.rs`, docs, and `.toml` inputs. Deliberately
add an allowlist entry and the review must name an owner plus expiry.
