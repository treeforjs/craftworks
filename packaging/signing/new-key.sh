#!/usr/bin/env bash
# Generate an update-signing key locally; never upload or print the private half.
set -euo pipefail

if [[ $# -ne 1 || "$1" != /* ]]; then
    printf 'Usage: bash packaging/signing/new-key.sh /absolute/path/to/new-key-directory\n' >&2
    exit 1
fi
command -v openssl >/dev/null || { printf 'OpenSSL is required.\n' >&2; exit 1; }
destination=${1%/}
parent=$(dirname -- "$destination")
name=$(basename -- "$destination")
if [[ ! -d "$parent" || "$name" == . || "$name" == .. || -z "$name" ]]; then
    printf 'Choose a new directory inside an existing parent directory.\n' >&2
    exit 1
fi
parent=$(cd -- "$parent" && pwd -P)
destination="$parent/$name"
repository=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd -P)
case "$destination" in
    "$repository"|"$repository"/*)
        printf 'Store signing keys outside the repository.\n' >&2
        exit 1
        ;;
esac
if [[ -e "$destination" || -L "$destination" ]]; then
    printf 'The output directory already exists. Refusing to replace any existing key or file.\n' >&2
    exit 1
fi

umask 077
mkdir -m 700 -- "$destination"
openssl genpkey -algorithm ed25519 -out "$destination/craft-update-key.pem"
chmod 600 -- "$destination/craft-update-key.pem"
openssl pkey -in "$destination/craft-update-key.pem" -pubout -outform DER \
    -out "$destination/craft-update-public.der"
openssl base64 -A -in "$destination/craft-update-public.der" \
    -out "$destination/craft-update-public-key.txt"
printf '\n' >>"$destination/craft-update-public-key.txt"

printf 'Private key saved in %s/craft-update-key.pem. Keep it private and back it up securely.\n' "$destination"
printf 'Public key (safe to share):\n'
cat -- "$destination/craft-update-public-key.txt"
printf '%s\n' \
    'In each treeforjs repository: pdfcraft, vectorcraft, photocraft, craftworks:' \
    '  Settings > Environments > release > UPDATE_SIGNING_KEY secret = the whole PEM file.' \
    '  Settings > Secrets and variables > Actions > Variables > CRAFT_UPDATE_PUBLIC_KEY = the public key above.' \
    'See docs/updates.md for release branches, publishing and rebuilding Craftworks.'
