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
BASENAME="craftworks-$VERSION-linux-$ARCH"
WORK="$(mktemp -d "$TARGET/craftworks-package.XXXXXX")"
STAGE="$WORK/$BASENAME"
mkdir -p "$STAGE" "$ROOT/dist/release"
install -m755 "$TARGET/release/craftworks" "$STAGE/craftworks"
install -m755 packaging/linux/install.sh packaging/linux/uninstall.sh "$STAGE/"
install -m644 LICENSE-MIT LICENSE-APACHE README.md "$STAGE/"
tar -C "$WORK" -czf "$ROOT/dist/release/$BASENAME.tar.gz" "$BASENAME"
printf 'Created %s\n' "$ROOT/dist/release/$BASENAME.tar.gz"
