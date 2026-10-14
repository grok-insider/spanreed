#!/usr/bin/env sh
# Bootstrap spanreed on Linux/macOS, then run interactive setup.
#
# Public one-liner (canonical — served by fabrials.com, not GitHub Releases):
#   curl -fsSL https://fabrials.com/install/spanreed.sh | sh
#
# Local / from a checkout:
#   ./scripts/install.sh --from-path ./target/release/spanreed
#   ./scripts/install.sh --yes --service
#
# Env:
#   SPANREED_REPO   GitHub owner/repo for binary assets (default: grok-insider/spanreed)
#   SPANREED_TAG    Release tag (default: latest)
#
# Binaries always come from GitHub Releases; this script is only the bootstrapper.

set -eu

REPO="${SPANREED_REPO:-grok-insider/spanreed}"
TAG="${SPANREED_TAG:-latest}"
FROM_PATH=""
SETUP_ARGS=""

usage() {
  echo "usage: install.sh [--from-path PATH] [--yes] [--service] [--dry-run] [--no-wire]"
}

while [ $# -gt 0 ]; do
  case "$1" in
    --from-path)
      FROM_PATH="${2:-}"
      shift 2
      ;;
    --yes|-y|--service|--dry-run|--no-wire|--from-current-exe)
      SETUP_ARGS="$SETUP_ARGS $1"
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "unknown arg: $1" >&2
      usage >&2
      exit 1
      ;;
  esac
done

os=$(uname -s | tr '[:upper:]' '[:lower:]')
arch=$(uname -m)
case "$arch" in
  x86_64|amd64) arch="x86_64" ;;
  aarch64|arm64) arch="aarch64" ;;
  *)
    echo "unsupported arch: $arch" >&2
    exit 1
    ;;
esac

# Release assets are static musl on Linux (cargo-zigbuild).
case "$os" in
  linux)
    if [ "$arch" != "x86_64" ] && [ -z "$FROM_PATH" ]; then
      echo "unsupported platform: $os/$arch (no published release asset)" >&2
      exit 1
    fi
    target="${arch}-unknown-linux-musl"
    ;;
  darwin) target="${arch}-apple-darwin" ;;
  *)
    echo "unsupported OS: $os" >&2
    exit 1
    ;;
esac

install_dir="${HOME}/.local/bin"
mkdir -p "$install_dir"
dest="${install_dir}/spanreed"
tmp=""
staged=""
trap '[ -z "$staged" ] || rm -f "$staged"; [ -z "$tmp" ] || rm -rf "$tmp"' EXIT

install_binary() {
  if [ -L "$dest" ] || { [ -e "$dest" ] && [ ! -f "$dest" ]; }; then
    echo "install destination must be a regular file or absent: $dest" >&2
    exit 1
  fi
  staged=$(mktemp "${install_dir}/.spanreed-install.XXXXXX")
  cp "$1" "$staged"
  chmod 755 "$staged"
  if [ -L "$dest" ] || { [ -e "$dest" ] && [ ! -f "$dest" ]; }; then
    echo "install destination must be a regular file or absent: $dest" >&2
    exit 1
  fi
  mv -f "$staged" "$dest"
  staged=""
}

http_get() {
  # $1 url  $2 outfile (optional: stdout)
  if command -v curl >/dev/null 2>&1; then
    if [ -n "${2:-}" ]; then
      curl -fsSL "$1" -o "$2"
    else
      curl -fsSL "$1"
    fi
  else
    if [ -n "${2:-}" ]; then
      wget -qO "$2" "$1"
    else
      wget -qO- "$1"
    fi
  fi
}

resolve_tag() {
  if [ "$TAG" != "latest" ]; then
    echo "$TAG"
    return
  fi
  # Asset names include the semver: spanreed-0.0.1-<target>.tar.gz
  api="https://api.github.com/repos/${REPO}/releases/latest"
  json=$(http_get "$api") || {
    echo "could not resolve latest stable release for ${REPO}" >&2
    return 1
  }
  if printf '%s' "$json" | grep -Eq '"(draft|prerelease)"[[:space:]]*:[[:space:]]*true'; then
    echo "latest release is not a published stable release" >&2
    return 1
  fi
  tag=$(printf '%s' "$json" | sed -n 's/.*"tag_name"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' | head -n1)
  if ! printf '%s\n' "$tag" | grep -Eq '^v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$'; then
    echo "latest release did not provide a stable version tag" >&2
    return 1
  fi
  echo "$tag"
}

if [ -n "$FROM_PATH" ]; then
  if [ ! -f "$FROM_PATH" ]; then
    echo "binary not found: $FROM_PATH" >&2
    exit 1
  fi
  install_binary "$FROM_PATH"
  echo "Installed $dest from $FROM_PATH"
else
  tag=$(resolve_tag)
  version="${tag#v}"
  base="https://github.com/${REPO}/releases/download/${tag}"
  asset="spanreed-${version}-${target}.tar.gz"
  url="${base}/${asset}"
  tmp=$(mktemp -d)
  echo "Downloading $url"
  http_get "$url" "$tmp/$asset"
  http_get "$url.sha256" "$tmp/$asset.sha256"
  expected=$(awk -v name="$asset" 'NR == 1 && NF == 2 && $2 == name {hash = $1} END {if (NR == 1 && length(hash) == 64 && hash !~ /[^0-9a-fA-F]/) print tolower(hash)}' "$tmp/$asset.sha256")
  if [ -z "$expected" ]; then
    echo "checksum file does not describe $asset" >&2
    exit 1
  fi
  if command -v sha256sum >/dev/null 2>&1; then
    actual=$(sha256sum "$tmp/$asset" | awk '{print $1}')
  else
    actual=$(shasum -a 256 "$tmp/$asset" | awk '{print $1}')
  fi
  if [ "$actual" != "$expected" ]; then
    echo "checksum mismatch for $asset" >&2
    exit 1
  fi
  tar -xzf "$tmp/$asset" -C "$tmp" spanreed
  bin="$tmp/spanreed"
  if [ ! -f "$bin" ] || [ -L "$bin" ]; then
    echo "archive did not contain a regular spanreed binary" >&2
    exit 1
  fi
  install_binary "$bin"
  echo "Installed $dest (release ${tag})"
fi

if [ -n "$tmp" ]; then
  rm -rf "$tmp"
  tmp=""
fi

# shellcheck disable=SC2086
exec "$dest" setup --from-current-exe $SETUP_ARGS
