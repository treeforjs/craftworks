#!/usr/bin/env bash
# Removes only this launcher. Remove managed apps in Craftworks first if desired.
set -euo pipefail
DATA="${XDG_DATA_HOME:-$HOME/.local/share}"
case "$DATA" in /*) ;; *) echo "XDG_DATA_HOME must be an absolute path." >&2; exit 1 ;; esac
case "$HOME" in /*) ;; *) echo "HOME must be an absolute path." >&2; exit 1 ;; esac
DEST="$DATA/craftworks-launcher"
BIN="$HOME/.local/bin/craftworks"
if [[ -L "$BIN" && "$(readlink -- "$BIN")" == "$DEST/craftworks" ]]; then
  rm -- "$BIN"
fi
rm -f -- "$DATA/applications/io.github.treeforjs.craftworks.desktop"
for file in craftworks LICENSE-MIT LICENSE-APACHE README.md uninstall.sh; do
  rm -f -- "$DEST/$file"
done
rmdir -- "$DEST" 2>/dev/null || true
if command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database "$DATA/applications" || true
fi
printf 'Removed the Craftworks launcher. Managed apps and preferences were kept.\n'
