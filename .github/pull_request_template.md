## What this PR does

<!-- One or two sentences. Link issues with Fixes #<n>. -->

## How it was verified

<!-- Exact commands + results. All four gates are required: -->

- [ ] `cargo test --workspace` green
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` clean
- [ ] `cargo fmt --all -- --check` clean
- [ ] `bash scripts/no-hardcoded-colors.sh` clean

## Checklist

- [ ] No `unsafe` outside `bins/omalaunch/src/bypass.rs` (with `// SAFETY:`)
- [ ] No `.unwrap()`/`.expect()` outside tests
- [ ] No hardcoded colors (see `docs/theming.md`)
- [ ] UI changes keep keyboard + mouse parity
- [ ] Docs updated if behavior changed (`docs/`, `docs/keyboard.md` via `--dump-shortcuts`)
- [ ] No unrelated files touched; no secrets committed
