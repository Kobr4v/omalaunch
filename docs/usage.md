# Omalaunch usage

Omalaunch is an Omarchy-first AppImage library. There is no installer
wizard and no `chmod +x` ritual: double-click any AppImage and omalaunch
takes it from there.

## First run

1. Install the native package (preferred: it wires MIME + binfmt + daemon).
2. Double-click a downloaded AppImage.
3. The Add dialog shows the auto-detected name, comment, destination
   (`~/Applications/<name>_<digest>.AppImage`), and whether an icon and an
   update channel were found.
4. **Integrate and run** moves the file into the library, adds it to your
   app menu, and launches it. **Run once** launches without touching anything.

Coming from AppImageLauncher? Run `omalaunch --migrate` (add `--dry-run`
first to preview, `--force` only if the old package is still installed).
Everything is backed up before anything is rewritten.

## Daily use

- Open **Omalaunch** from the app menu (or `SUPER+A` with the snippet from
  `packaging/bindings.lua`) to browse covers, search (`/`), and launch
  (`Enter` or `p`).
- Toggle **Grid** for the cover wall or keep the list; sort by updates,
  name, recency, or play count; filter by Favorites, Updates, categories,
  tags, or collections.
- Select an app to see details: cover, description, categories, play count,
  update state, and source location. Launch, update, remove, favorite (`f`),
  hide (`h`), or file it into collections from there.
- **Sources** shows every watched folder with scan status; toggle folders
  or Rescan on demand. Copy an AppImage into `~/Applications` and the
  daemon integrates it silently; delete one and the menu entry is cleaned up.
- Right-click is fully supported — every keyboard action has a button.
- Drop an AppImage file onto the window to add it.

## Settings

`omalaunch` → **Settings** (or `s`): applications directory, ask-to-move,
daemon on/off, extra watch directories. Changes apply on Save; the daemon
is restarted automatically.

## Command line

One binary, subcommands (never any GUI init on these paths):

- `omalaunch <file.AppImage>` — open the Add dialog for a file.
- `omalaunch play <name-or-path>` — launch a library app headless.
- `omalaunch quit` — ask a running instance to quit.
- `omalaunch --demo` — explore the UI with a fictional library.
- `omalaunch integrate|unintegrate|would-integrate` — headless helpers
  for scripts (exit codes: 0 ok, 1 failed, 3 bad args).
- `omalaunch update|remove <files...>` — headless update / removal.
- `omalaunch daemon [--list-watched-directories] [--debug]` — run the watcher.
- `omalaunch bypass <file> [args...]` — memfd runner (used by binfmt + menus).
- `omalaunch --migrate [--dry-run] [--force]` — one-shot importer.
- `omalaunch --dump-shortcuts` — regenerate `docs/keyboard.md`.
