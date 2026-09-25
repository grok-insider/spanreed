#!/usr/bin/env bash
# Pull fabrials-libs into vendor/fabrials-libs and record the revision.
# Usage: scripts/sync-fabrials-libs.sh [repository] [ref]
set -euo pipefail

root=$(git rev-parse --show-toplevel)
cd "$root"
prefix=vendor/fabrials-libs
source_file=$prefix.source

default_repo=https://github.com/grok-insider/fabrials-libs
if [ -d "$root/../libs/fabrials-libs/.git" ]; then
  default_repo=$root/../libs/fabrials-libs
fi
repo=${1:-${FABRIALS_LIBS_REPO:-$default_repo}}
ref=${2:-master}

git fetch --quiet --no-tags "$repo" "$ref"
commit=$(git rev-parse FETCH_HEAD)
tree=$(git rev-parse "FETCH_HEAD^{tree}")

if [ -d "$prefix" ]; then
  git subtree pull --quiet --prefix="$prefix" "$repo" "$ref" --squash \
    -m "chore: sync fabrials-libs ${commit:0:12}"
else
  git subtree add --quiet --prefix="$prefix" "$repo" "$ref" --squash \
    -m "chore: add fabrials-libs ${commit:0:12}"
fi

cat >"$source_file" <<SOURCE
repository=https://github.com/grok-insider/fabrials-libs
commit=$commit
tree=$tree
SOURCE
git add "$source_file"
git commit --quiet -m "chore: record fabrials-libs ${commit:0:12}" -- "$source_file"
scripts/check-fabrials-libs.sh
