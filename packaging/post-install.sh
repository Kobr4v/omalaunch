#!/bin/bash
# omalaunch post-install: register the binfmt interpreter, MIME handler,
# and user daemon. Idempotent; safe to re-run.
set -euo pipefail
PREFIX="${PREFIX:-/usr}"
SHAREDIR="$PREFIX/share/omalaunch"

if command -v modprobe >/dev/null 2>&1; then
  modprobe -v binfmt_misc || echo "modprobe failed, binfmt_misc might be unavailable"
fi

INTERPRETER="$PREFIX/bin/omalaunch"
sed "s|@BINFMT_INTERPRETER_PATH@|$INTERPRETER|" \
  "$SHAREDIR/binfmt.d/omalaunch.conf.in" > /usr/lib/binfmt.d/omalaunch.conf \
  || sed "s|@BINFMT_INTERPRETER_PATH@|$INTERPRETER|" \
  packaging/binfmt.d/omalaunch.conf.in > /usr/lib/binfmt.d/omalaunch.conf

if command -v systemctl >/dev/null 2>&1; then
  systemctl restart systemd-binfmt || true
  update-desktop-database /usr/share/applications || true
fi
