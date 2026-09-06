# Omalaunch packaging

## Native package (preferred)

Dependencies: `gtk4 >= 4.6`, `libadwaita >= 1.4`, `sqlite` not required,
`systemd` (user units), standard freedesktop helpers
(`update-desktop-database`, `gtk-update-icon-cache`, `xdg-desktop-menu` —
all optional at runtime, skipped silently when absent).

Install layout (single binary):

- `omalaunch` → `$PREFIX/bin/omalaunch` (GUI + all subcommands)
- `packaging/omalaunch.desktop` → `/usr/share/applications/` (owns the
  AppImage MIME types; conflicts with AppImageLauncher by design)
- `packaging/binfmt.d/omalaunch.conf.in` → `/usr/lib/binfmt.d/omalaunch.conf`
  with `@BINFMT_INTERPRETER_PATH@` substituted (see `post-install.sh`)
- `packaging/omalaunchd.service` → user units
- Hook → `omarchy hook install theme-set hooks/theme-set.d/omalaunch`
- Menu row → merge `packaging/omarchy-menu.jsonc` into
  `~/.config/omarchy/extensions/omarchy-menu.jsonc`
- Global key → append `packaging/bindings.lua` to
  `~/.config/hypr/bindings.lua` after checking
  `omarchy menu keybindings --print` (unbind first if taken)

## Lite equivalent

Out of scope for v1: the old Lite edition (rootless AppImage
self-install) has no Rust counterpart yet. Native packages are the only
supported distribution until a portable bundle is planned separately.

## Coexistence

None: omalaunch replaces AppImageLauncher (MIME + binfmt single-owner).
Run `omalaunch --migrate --dry-run` before uninstalling the old package.
