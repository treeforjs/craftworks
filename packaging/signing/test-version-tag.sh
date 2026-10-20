#!/usr/bin/env bash
set -euo pipefail
CHECK="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)/check-version-tag.sh"
PARENT="$(cd -- "${TMPDIR:-/tmp}" && pwd -P)"
WORK="$(mktemp -d "$PARENT/craftworks-tags.XXXXXXXX")"
cleanup() { case "$WORK" in "$PARENT"/craftworks-tags.*) rm -rf -- "$WORK" ;; esac; }
trap cleanup EXIT
git init --quiet --initial-branch=main "$WORK/source"
git init --quiet --bare "$WORK/remote.git"
cd -- "$WORK/source"
git remote add origin "$WORK/remote.git"
git -c user.name=Fixture -c user.email=fixture@example.invalid commit --quiet --allow-empty -m 'First fixture'
first="$(git rev-parse HEAD)"
git -c user.name=Fixture -c user.email=fixture@example.invalid commit --quiet --allow-empty -m 'Second fixture'
second="$(git rev-parse HEAD)"
git push --quiet origin HEAD:refs/heads/main
bash "$CHECK" missing "$second"
git tag lightweight "$second"
git -c user.name=Fixture -c user.email=fixture@example.invalid tag -a annotated "$second" -m 'Tag fixture'
git push --quiet origin refs/tags/lightweight refs/tags/annotated
bash "$CHECK" lightweight "$second"
bash "$CHECK" annotated "$second"
if bash "$CHECK" lightweight "$first" >/dev/null 2>&1; then
  echo 'A conflicting lightweight tag was accepted.' >&2; exit 1
fi
if bash "$CHECK" annotated "$first" >/dev/null 2>&1; then
  echo 'A conflicting annotated tag was accepted.' >&2; exit 1
fi
git remote remove origin
if bash "$CHECK" missing "$second" >/dev/null 2>&1; then
  echo 'A remote lookup failure was accepted as a missing tag.' >&2; exit 1
fi
echo 'Missing, matching, conflicting and failed-lookup tag checks passed.'
