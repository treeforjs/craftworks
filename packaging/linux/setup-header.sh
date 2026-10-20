#!/usr/bin/env bash
# Self-contained, per-user Craftworks setup. The release packager appends the archive.
set -euo pipefail
umask 022

VERSION='@VERSION@'
ARCH='@ARCH@'
BASENAME='@BASENAME@'
PAYLOAD_SHA256='@PAYLOAD_SHA256@'
DRY_RUN=false
case "${1:-}" in
  --dry-run) DRY_RUN=true; shift ;;
  -h|--help)
    printf 'Usage: bash %s [--dry-run]\nInstall Craftworks for the current user. No downloads or root access are needed.\n' "$0"
    exit 0 ;;
esac
if [[ "$#" -ne 0 ]]; then
  printf 'Unknown argument. Usage: bash %s [--dry-run]\n' "$0" >&2
  exit 2
fi
fail() { printf 'Craftworks setup: %s\n' "$1" >&2; exit 1; }

USER_HOME="${HOME:-}"
DATA="${XDG_DATA_HOME:-$USER_HOME/.local/share}"
for path in "$USER_HOME" "$DATA"; do
  case "$path" in /*) ;; *) fail 'HOME and XDG_DATA_HOME must be absolute paths.' ;; esac
  case "$path" in *$'\n'*|*$'\r'*) fail 'Installation paths must not contain line breaks.' ;; esac
done
DEST="$DATA/craftworks-launcher"
BIN="$USER_HOME/.local/bin/craftworks"
DESKTOP="$DATA/applications/io.github.treeforjs.craftworks.desktop"
if [[ -L "$DEST" || ( -e "$DEST" && ! -d "$DEST" ) ]]; then
  fail "$DEST must be a regular directory. Move it before installing."
fi
if [[ -e "$BIN" && ! -L "$BIN" ]]; then
  fail "$BIN already exists and is not a symlink. Move it before installing."
fi
if [[ -L "$DESKTOP" || ( -e "$DESKTOP" && ! -f "$DESKTOP" ) ]]; then
  fail "$DESKTOP must be a regular file. Move it before installing."
fi
FILES=(craftworks LICENSE-MIT LICENSE-APACHE README.md THIRD-PARTY-NOTICES.txt uninstall.sh)
HAVE_PREVIOUS=false
for file in "${FILES[@]}"; do
  if [[ -L "$DEST/$file" || ( -e "$DEST/$file" && ! -f "$DEST/$file" ) ]]; then
    fail "$DEST/$file must be a regular file. Move it before installing."
  fi
  if [[ -f "$DEST/$file" ]]; then HAVE_PREVIOUS=true; fi
done
if [[ -L "$BIN" || -f "$DESKTOP" ]]; then HAVE_PREVIOUS=true; fi
BACKUP=''
if [[ "$HAVE_PREVIOUS" == true ]]; then
  BACKUP="$DATA/craftworks-launcher-backups/$VERSION-$(date -u +%Y%m%dT%H%M%SZ)-$$"
  [[ ! -e "$BACKUP" && ! -L "$BACKUP" ]] || fail "Backup path already exists: $BACKUP"
fi

SELF_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)"
SELF="$SELF_DIR/$(basename -- "${BASH_SOURCE[0]}")"
PAYLOAD_LINE="$(awk '/^__CRAFTWORKS_PAYLOAD_BELOW__$/ {print NR + 1; exit}' "$SELF")"
[[ "$PAYLOAD_LINE" =~ ^[0-9]+$ ]] || fail 'The embedded package is missing.'
payload() { tail -n +"$PAYLOAD_LINE" -- "$SELF"; }
ACTUAL_SHA256="$(payload | sha256sum | awk '{print $1}')"
[[ "$ACTUAL_SHA256" == "$PAYLOAD_SHA256" ]] || fail 'The embedded package checksum does not match. Download setup again.'
EXPECTED="$(printf '%s\n' "$BASENAME/" "$BASENAME/craftworks" "$BASENAME/install.sh" \
  "$BASENAME/uninstall.sh" "$BASENAME/LICENSE-MIT" "$BASENAME/LICENSE-APACHE" "$BASENAME/README.md" \
  "$BASENAME/THIRD-PARTY-NOTICES.txt" | LC_ALL=C sort)"
ACTUAL="$(payload | tar -tzf - | LC_ALL=C sort)"
[[ "$ACTUAL" == "$EXPECTED" ]] || fail 'The embedded package contains unexpected or missing files.'
if ! payload | tar -tvzf - | awk 'substr($0, 1, 1) != "-" && substr($0, 1, 1) != "d" {exit 1}'; then
  fail 'The embedded package must contain only regular files and its root directory.'
fi
printf 'Verified Craftworks %s for Linux %s.\n' "$VERSION" "$ARCH"
printf 'Install launcher: %s/craftworks\nCommand: %s\nApplication menu: %s\n' "$DEST" "$BIN" "$DESKTOP"
if [[ -n "$BACKUP" ]]; then printf 'Back up previous launcher: %s\n' "$BACKUP"; fi
if [[ "$DRY_RUN" == true ]]; then
  printf 'Dry run complete. No files were created or changed.\n'
  exit 0
fi
[[ "$(uname -s)" == Linux ]] || fail 'Run this setup on Linux, or use --dry-run to inspect it.'
[[ "$(id -u)" -ne 0 ]] || fail 'Run setup as your normal desktop user, without sudo.'
HOST_ARCH="$(uname -m)"
case "$HOST_ARCH" in arm64) HOST_ARCH=aarch64 ;; esac
[[ "$HOST_ARCH" == "$ARCH" ]] || fail "This package requires $ARCH; this computer is $HOST_ARCH."

TEMP_PARENT="${TMPDIR:-/tmp}"
case "$TEMP_PARENT" in /*) ;; *) fail 'TMPDIR must be an absolute path.' ;; esac
TEMP_PARENT="$(cd -- "$TEMP_PARENT" && pwd -P)"
WORK="$(mktemp -d "$TEMP_PARENT/craftworks-setup.XXXXXXXX")"
cleanup() {
  status=$?
  case "$WORK" in "$TEMP_PARENT"/craftworks-setup.*) rm -rf -- "$WORK" ;; esac
  if [[ "$status" -ne 0 && -n "$BACKUP" && -d "$BACKUP" ]]; then
    printf 'Previous launcher backup kept at %s\n' "$BACKUP" >&2
  fi
  return "$status"
}
trap cleanup EXIT
payload >"$WORK/payload.tar.gz"
ACTUAL_SHA256="$(sha256sum "$WORK/payload.tar.gz" | awk '{print $1}')"
[[ "$ACTUAL_SHA256" == "$PAYLOAD_SHA256" ]] || fail 'The setup file changed while being read. Download setup again.'
tar -xzf "$WORK/payload.tar.gz" --no-same-owner -C "$WORK"
if [[ -n "$BACKUP" ]]; then
  mkdir -p -- "$DATA/craftworks-launcher-backups"
  mkdir -m700 -- "$BACKUP"
  mkdir -- "$BACKUP/launcher"
  for file in "${FILES[@]}"; do
    if [[ -f "$DEST/$file" ]]; then cp -p -- "$DEST/$file" "$BACKUP/launcher/$file"; fi
  done
  if [[ -L "$BIN" ]]; then cp -P -- "$BIN" "$BACKUP/command-link"; fi
  if [[ -f "$DESKTOP" ]]; then cp -p -- "$DESKTOP" "$BACKUP/desktop-entry"; fi
  printf 'Previous launcher backed up to %s\n' "$BACKUP"
fi
bash "$WORK/$BASENAME/install.sh"
printf 'Open Craftworks from your application menu or run:\n  %s\n' "$BIN"
exit 0
__CRAFTWORKS_PAYLOAD_BELOW__
