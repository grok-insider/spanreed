#!/usr/bin/env bash
#
# update-changelog.sh — splice a generated release section into CHANGELOG.md.
#
# Usage:
#   scripts/update-changelog.sh <version> <section-file>
#
# <section-file> holds a full section beginning with `## <version>` (as produced
# by gen-changelog.sh). If a section for <version> already exists it is replaced;
# otherwise the new section is inserted at the top (newest first). The file's
# preamble (everything before the first `## ` heading) is preserved.
#
# Override the target file with CHANGELOG_FILE (default: CHANGELOG.md).

set -euo pipefail

version="${1:?usage: update-changelog.sh <version> <section-file>}"
section_file="${2:?usage: update-changelog.sh <version> <section-file>}"
file="${CHANGELOG_FILE:-CHANGELOG.md}"

[ -f "$section_file" ] || { echo "section file not found: $section_file" >&2; exit 1; }

if [ ! -f "$file" ]; then
  printf '# Changelog\n\nAll notable, user-facing changes to spanreed are documented here.\n\n' > "$file"
fi

new_file="$(SECTION="$(cat "$section_file")" awk -v version="$version" '
  BEGIN { mode = "pre" }
  # Everything before the first "## " heading is the preamble.
  mode == "pre" && /^## / { mode = "body" }
  mode == "pre" { pre = pre $0 ORS; next }
  # In the body, drop any existing section for this version.
  /^## / { skip = ($2 == version) ? 1 : 0 }
  { if (!skip) body = body $0 ORS }
  END {
    sub(/\n+$/, "", pre)
    section = ENVIRON["SECTION"]
    sub(/\n+$/, "", section)
    sub(/^\n+/, "", body)
    printf "%s\n\n%s\n", pre, section
    if (length(body) > 0) printf "\n%s", body
  }
' "$file")"

printf '%s\n' "$new_file" > "$file"
echo "Updated $file with section for v$version" >&2
