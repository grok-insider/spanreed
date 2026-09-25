#!/usr/bin/env bash
# Fails when vendor/fabrials-libs differs from the revision recorded in
# vendor/fabrials-libs.source. With FABRIALS_LIBS_REPO set (path or URL), it
# also checks that the recorded commit really has that tree upstream.
set -euo pipefail

root=$(git rev-parse --show-toplevel)
cd "$root"
prefix=vendor/fabrials-libs
source_file=$prefix.source

field() { sed -n "s/^$1=//p" "$source_file"; }
expected_tree=$(field tree)
commit=$(field commit)
if [ -z "$expected_tree" ] || [ -z "$commit" ]; then
  echo "fabrials-libs: $source_file lacks commit= or tree=" >&2
  exit 1
fi

index=$(mktemp)
trap 'rm -f "$index"' EXIT
rm -f "$index"
GIT_INDEX_FILE=$index git read-tree --empty
GIT_INDEX_FILE=$index git add --all -- "$prefix"
actual_tree=$(GIT_INDEX_FILE=$index git write-tree --prefix="$prefix/")

if [ "$actual_tree" != "$expected_tree" ]; then
  echo "fabrials-libs: $prefix drifted from $commit" >&2
  echo "  expected tree $expected_tree" >&2
  echo "  actual tree   $actual_tree" >&2
  echo "Change the code in fabrials-libs, then run scripts/sync-fabrials-libs.sh." >&2
  exit 1
fi

if [ -n "${FABRIALS_LIBS_REPO:-}" ]; then
  git fetch --quiet --no-tags "$FABRIALS_LIBS_REPO" "$commit"
  upstream_tree=$(git rev-parse "FETCH_HEAD^{tree}")
  if [ "$upstream_tree" != "$expected_tree" ]; then
    echo "fabrials-libs: $commit upstream has tree $upstream_tree, not $expected_tree" >&2
    exit 1
  fi
fi

echo "fabrials-libs: $prefix matches $commit"
