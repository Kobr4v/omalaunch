# omalaunch

[![CI](https://github.com/Kobr4v/omalaunch/actions/workflows/ci.yml/badge.svg)](https://github.com/Kobr4v/omalaunch/actions/workflows/ci.yml)
[![License: GPL v3](https://img.shields.io/badge/License-GPLv3-blue.svg)](https://www.gnu.org/licenses/gpl-3.0)

**Your AppImages, beautifully together.** An Omarchy-first AppImage library
and launcher, written in pure Rust — no C, no C++, no CMake.

Double-click any AppImage: omalaunch offers to shelve it into your library
(`~/Applications/<name>_<digest>.AppImage`) with a menu entry, icon, and
one-click update/remove actions — or just run it once and leave no traces.

## Features

- **Cover-grid library** with details pane, or a classic list — your choice,
  remembered across restarts
- **Auto metadata**: name, comment, categories, icon, and update channel
  extracted straight from the AppImage (pure-Rust SquashFS reader, no
  libappimage, no mounting, no execution)
- **Collections, tags, favorites, hidden apps**, smart filters (favorites,
  updates, sources), fuzzy search, and multiple sort orders
- **Sources screen**: every watched folder with scan status, toggles, and
  on-demand rescan; background daemon auto-integrates drops
- **Pure-Rust updater**: zsync update channels with progress, atomic swap,
  length verification, and automatic rollback on failure
- **Keyboard-first** (Omarchy style, `?` overlay) with full mouse parity
- **Zero hardcoded colors**: the entire UI resolves at runtime from your
  active Omarchy theme — palette, font, and transparency — and reloads live
  on `omarchy theme set` (CI fails the build on any hex literal)
- **Single binary**: GUI + `daemon`, `bypass`, `integrate`, `unintegrate`,
  `would-integrate`, `update`, `remove`, `play`, `quit` subcommands;
  `omalaunch --demo` explores the UI with a fictional library
- **One-shot migrator** from AppImageLauncher (`omalaunch --migrate
  --dry-run`), backup-first and refusal-guarded

## Install

### From source (requires Rust 1.85+, GTK4, libadwaita, sqlite)

```bash
git clone https://github.com/Kobr4v/omalaunch
cd omalaunch
cargo build --release
# Binaries land in target/release/omalaunch
./target/release/omalaunch
```

System integration (MIME + binfmt + daemon + menu):

```bash
sudo PREFIX=/usr ./packaging/post-install.sh
omarchy hook install theme-set hooks/theme-set.d/omalaunch
```

### Packages

Release tarballs with checksums are published under
[Releases](https://github.com/Kobr4v/omalaunch/releases) (process described
under [Releasing in CONTRIBUTING.md](CONTRIBUTING.md#releasing)).

## Usage

See [`docs/usage.md`](docs/usage.md) for the full guide,
[`docs/theming.md`](docs/theming.md) for the theming contract,
[`docs/packaging.md`](docs/packaging.md) for install layout, and press `?`
inside the app for the keyboard map.

Quick tour:

```bash
omalaunch                  # library GUI
omalaunch ~/dl/Foo.AppImage  # Add dialog for a file
omalaunch play foo          # launch by fuzzy name, headless
omalaunch update ~/Applications/Foo_*.AppImage
omalaunch remove ~/Applications/Old_*.AppImage
omalaunch daemon            # watcher (also via systemd user unit)
```

## Development

```bash
cargo test --workspace          # 100+ tests, all green required
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
bash scripts/no-hardcoded-colors.sh
```

Quality gates are enforced by CI on every push and pull request. Please
read [`CONTRIBUTING.md`](CONTRIBUTING.md) before your first PR — it covers
the plan-first workflow, commit style, and the no-hardcoded-colors rule.

## Project layout

```text
bins/omalaunch/     single binary: GTK4 shell + subcommands
crates/oma-core/      paths, config, typed errors, run decisions
crates/oma-appimage/  ELF/SquashFS parsing (goblin + backhand)
crates/oma-integrate/ desktop entries, icons, registry, trash, integrate flow
crates/oma-library/   SQLite library: favorites, collections, tags, plays, covers
crates/oma-theme/     staged Omarchy palette loader, GTK CSS, live watcher
crates/oma-update/    pure-Rust zsync update check + atomic apply
crates/oma-daemon/    notify watcher, debounce queue, library sync
packaging/          desktop entry, binfmt, systemd unit, menu, keybinding
hooks/              Omarchy theme-set hook
docs/               usage, theming, packaging guides
```

## License

All Rust code, packaging, hooks, scripts, and docs: **GPL-3.0-or-later**,
Copyright (C) 2026 Ahmed Shafiq — all rights reserved to the copyright
holder. See [`LICENSE`](LICENSE) and [`COPYRIGHT`](COPYRIGHT).

Legacy icons under `resources/` remain MIT, copyright their authors
(see [`LICENSE.txt`](LICENSE.txt)).
