#!/bin/bash
# Fails if a hardcoded color literal appears where it must not.
# Scans: Rust sources, stylesheets, shell hooks, Lua snippets, .desktop files.
# Allowlist (each with owner + reason):
#   - tests/fixtures/** ............ test INPUT palettes (clearly marked dirs)
#   - crates/oma-theme/src/fallback.rs  single choke point (todo 23 owner)
#   - docs/** and *.md ............. human documentation examples
#   - *.toml ........................ theme/config inputs, not code
set -euo pipefail
hits=$(grep -rnE '#[0-9a-fA-F]{3,8}\b' \
  --include='*.rs' \
  --include='*.css' \
  --include='*.qss' \
  --include='*.sh' \
  --include='*.lua' \
  --include='*.desktop' \
  . \
  | grep -v 'tests/fixtures/' \
  | grep -v 'crates/oma-theme/src/fallback.rs' \
  | grep -v '\.md:' \
  | grep -v 'docs/' \
  | grep -v '\.toml:' \
  || true)
if [ -n "$hits" ]; then
  echo "hardcoded color literals found:"
  echo "$hits"
  exit 1
fi
echo "no hardcoded colors OK"
