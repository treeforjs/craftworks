#!/usr/bin/env bash
# Refuse to attach a release to a version tag belonging to another commit.
set -euo pipefail
if [[ "$#" -ne 2 ]]; then
  echo 'Usage: check-version-tag.sh TAG EXPECTED_COMMIT' >&2
  exit 2
fi
tag="$1"
expected="$2"
git check-ref-format "refs/tags/$tag"
[[ "$expected" =~ ^[0-9a-f]{40}$ ]] || { echo 'Expected a full commit SHA.' >&2; exit 2; }
refs="$(git ls-remote --tags origin "refs/tags/$tag" "refs/tags/$tag^{}")"
actual="$(printf '%s\n' "$refs" | awk '
  index($2, "^{}") { peeled = $1; next }
  NF >= 2 { direct = $1 }
  END { if (peeled != "") print peeled; else if (direct != "") print direct }
')"
if [[ -n "$actual" && "$actual" != "$expected" ]]; then
  echo '::error::The version tag already points to another commit; increment the version.' >&2
  exit 1
fi
echo 'Version tag is available for the reviewed commit.'
