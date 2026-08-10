#!/usr/bin/env sh
# Bootstrap spanreed on Linux/macOS, then run interactive setup.
#
#   curl -fsSL …/install.sh | sh
#   ./scripts/install.sh --from-path ./target/release/spanreed
#   ./scripts/install.sh --yes --service
#
# Env:
#   SPANREED_REPO   GitHub owner/repo (default: grok-insider/spanreed)
#   SPANREED_TAG    Release tag (default: latest)

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

case "$os" in
  linux) target="${arch}-unknown-linux-gnu" ;;
  darwin) target="${arch}-apple-darwin" ;;
  *)
    echo "unsupported OS: $os" >&2
    exit 1
    ;;
esac

install_dir="${HOME}/.local/bin"
mkdir -p "$install_dir"
dest="${install_dir}/spanreed"

if [ -n "$FROM_PATH" ]; then
  if [ ! -f "$FROM_PATH" ]; then
    echo "binary not found: $FROM_PATH" >&2
    exit 1
  fi
  cp "$FROM_PATH" "$dest"
  chmod 755 "$dest"
  echo "Installed $dest from $FROM_PATH"
else
  if [ "$TAG" = "latest" ]; then
    base="https://github.com/${REPO}/releases/latest/download"
  else
    base="https://github.com/${REPO}/releases/download/${TAG}"
  fi
  asset="spanreed-${target}.tar.gz"
  url="${base}/${asset}"
  tmp=$(mktemp -d)
  trap 'rm -rf "$tmp"' EXIT
  echo "Downloading $url"
  if command -v curl >/dev/null 2>&1; then
    curl -fsSL "$url" -o "$tmp/$asset"
  else
    wget -qO "$tmp/$asset" "$url"
  fi
  tar -xzf "$tmp/$asset" -C "$tmp"
  bin=$(find "$tmp" -type f -name spanreed | head -n1)
  if [ -z "$bin" ]; then
    echo "archive did not contain spanreed" >&2
    exit 1
  fi
  cp "$bin" "$dest"
  chmod 755 "$dest"
  echo "Installed $dest"
fi

# shellcheck disable=SC2086
exec "$dest" setup --from-current-exe $SETUP_ARGS
