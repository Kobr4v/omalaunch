# Contributing to omalaunch

Thanks for stopping by. This project is intentionally small-team shaped:
a single Rust workspace, strict gates, and boring, reviewable diffs.

## Ground rules

1. **100% Rust.** No C, C++, CMake, or vendored native code in the tree.
   System libraries (GTK4, sqlite) link dynamically; that is the only
   exception, and it needs no new build machinery.
2. **All four gates pass, always.** `cargo test --workspace`,
   `cargo clippy --workspace --all-targets -- -D warnings`,
   `cargo fmt --all -- --check`, `bash scripts/no-hardcoded-colors.sh`.
   CI runs the same commands — a red CI means the PR is not ready.
3. **No hardcoded colors.** Every visible color must resolve from the
   active Omarchy theme at runtime. Read `docs/theming.md` first.
4. **Keyboard + mouse parity.** Every action needs both a shortcut (in the
   single table in `bins/omalaunch/src/shortcuts.rs`) and a clickable
   control. Regenerate `docs/keyboard.md` via
   `./target/debug/omalaunch --dump-shortcuts > docs/keyboard.md`.
5. **`unsafe` lives in one file** (`bins/omalaunch/src/bypass.rs`), every
   block with a `// SAFETY:` comment. No `.unwrap()`/`.expect()` outside
   tests.
6. **Offline, local-first.** No network calls except AppImage updates the
   user explicitly triggers. No accounts, no telemetry, no controllers.

## Workflow

1. Open an issue first for anything non-trivial (bug template asks for
   repro + logs; feature template asks for the scope check).
2. Keep PRs small and single-purpose. Fill in the PR template, including
   the verification checklist — unverified PRs are sent back.
3. Conventional commits (`feat(ui): …`, `fix(daemon): …`, `docs: …`,
   `test: …`, `chore: …`). One logical change per commit.
4. A maintainer (currently just Ahmed) reviews; two approvals are not
   required, but CI must be green and the checklist complete.

## Releasing

1. Bump `version` in the workspace `Cargo.toml` and add a `CHANGELOG.md`
   entry (create the file if this is the first release).
2. Commit as `chore(release): vX.Y.Z`, tag `vX.Y.Z`, push tag.
3. The `release` workflow builds, checksums, and publishes the GitHub
   Release automatically. Verify checksums before announcing.

## Project layout

See the layout section in `README.md`. Engine crates (`crates/`) never
touch GTK; the `bins/omalaunch` shell owns all UI. Headless subcommands
must never initialize GTK.

## License

By contributing you agree your work lands under GPL-3.0-or-later,
copyright retained by its authors, consistent with `LICENSE`/`COPYRIGHT`.
