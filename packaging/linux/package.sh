#!/usr/bin/env bash
set -euo pipefail
umask 022
ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"
VERSION="$(python3 packaging/version.py)"
case "$(uname -m)" in
  x86_64) ARCH=x86_64 ;;
  aarch64|arm64) ARCH=aarch64 ;;
  *) echo "Unsupported Linux architecture" >&2; exit 1 ;;
esac
if [[ "${1:-}" != --skip-build ]]; then
  cargo build --locked --release --package craftworks
fi
TARGET="${CARGO_TARGET_DIR:-$ROOT/target}"
mkdir -p "$TARGET"
TARGET="$(cd -- "$TARGET" && pwd -P)"
BASENAME="craftworks-$VERSION-linux-$ARCH"
WORK="$(mktemp -d "$TARGET/craftworks-package.XXXXXX")"
cleanup() {
  case "$WORK" in "$TARGET"/craftworks-package.*) rm -rf -- "$WORK" ;; esac
}
trap cleanup EXIT
STAGE="$WORK/$BASENAME"
DIST="${DIST:-$ROOT/dist/release}"
mkdir -p "$STAGE" "$DIST"
install -m755 "$TARGET/release/craftworks" "$STAGE/craftworks"
install -m755 packaging/linux/install.sh packaging/linux/uninstall.sh "$STAGE/"
install -m644 LICENSE-MIT LICENSE-APACHE README.md "$STAGE/"
ARCHIVE="$DIST/$BASENAME.tar.gz"
SETUP="$DIST/$BASENAME-setup.sh"
tar -C "$WORK" -czf "$ARCHIVE" "$BASENAME"
PAYLOAD_SHA256="$(sha256sum "$ARCHIVE" | awk '{print $1}')"
sed -e "s/@VERSION@/$VERSION/g" -e "s/@ARCH@/$ARCH/g" \
  -e "s/@BASENAME@/$BASENAME/g" -e "s/@PAYLOAD_SHA256@/$PAYLOAD_SHA256/g" \
  packaging/linux/setup-header.sh >"$SETUP"
cat "$ARCHIVE" >>"$SETUP"
chmod 755 "$SETUP"
printf 'Created %s\nCreated %s\n' "$ARCHIVE" "$SETUP"
