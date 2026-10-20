#!/usr/bin/env bash
# Run from the unpacked release. No root access or download is needed here.
set -euo pipefail
umask 022
SOURCE="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
DATA="${XDG_DATA_HOME:-$HOME/.local/share}"
case "$DATA" in /*) ;; *) echo "XDG_DATA_HOME must be an absolute path." >&2; exit 1 ;; esac
case "$HOME" in /*) ;; *) echo "HOME must be an absolute path." >&2; exit 1 ;; esac
DEST="$DATA/craftworks-launcher"
BIN="$HOME/.local/bin"
if [[ -e "$BIN/craftworks" && ! -L "$BIN/craftworks" ]]; then
  echo "$BIN/craftworks already exists and is not a symlink. Move it before installing Craftworks." >&2
  exit 1
fi
mkdir -p "$DEST" "$BIN" "$DATA/applications"
# Replacing a running Linux executable by rename lets that process finish normally.
install -m755 "$SOURCE/craftworks" "$DEST/craftworks.new"
mv -f -- "$DEST/craftworks.new" "$DEST/craftworks"
for file in LICENSE-MIT LICENSE-APACHE README.md; do
  install -m644 "$SOURCE/$file" "$DEST/$file"
done
install -m755 "$SOURCE/uninstall.sh" "$DEST/uninstall.sh"
ln -sfn -- "$DEST/craftworks" "$BIN/craftworks"
# A desktop Exec value is quoted and escaped independently from shell arguments.
DESKTOP_EXE="$(printf '%s' "$DEST/craftworks" | sed 's/\\/\\\\/g; s/"/\\"/g; s/`/\\`/g; s/\$/\\$/g; s/%/%%/g')"
cat >"$DATA/applications/io.github.treeforjs.craftworks.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=Craftworks
Comment=Install, update and open your craft apps
Exec="$DESKTOP_EXE"
Terminal=false
Categories=Graphics;Utility;
StartupNotify=true
EOF
if command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database "$DATA/applications" || true
fi
printf 'Installed Craftworks. Open it from your application menu or run %s\n' "$BIN/craftworks"
