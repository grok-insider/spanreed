#!/usr/bin/env bash
set -euo pipefail

remote_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$remote_root"
if [[ ! -f vendor/fabrials-libs/crates/fabrials-remote-host/Cargo.toml ]]; then
  echo 'Sync the reviewed canonical fabrials-libs commit with scripts/sync-fabrials-libs.sh before building Remote.' >&2
  exit 1
fi
scripts/check-fabrials-libs.sh
python3 scripts/remote-assets.py check
export FABRIALS_AGENT_HOST_WEB_DIST="$remote_root/assets/remote-web"
remote_target="$remote_root/target/remote-build"
remote_platform="$(rustc -vV | sed -n 's/^host: //p')"
if [[ -z "$remote_platform" ]]; then
  echo 'Could not determine the native Rust target.' >&2
  exit 1
fi
cargo build --release --locked --target "$remote_platform" --target-dir "$remote_target/cli" --bin spanreed
cargo build --release --locked --target "$remote_platform" --target-dir "$remote_target/relay" --manifest-path crates/spanreed-remote-relay/Cargo.toml
remote_dist="${SPANREED_REMOTE_DIST:-$remote_root/target/remote-dist}"
mkdir -p "$remote_dist"
install -m 755 "$remote_target/cli/$remote_platform/release/spanreed" "$remote_dist/spanreed"
install -m 755 "$remote_target/relay/$remote_platform/release/spanreed-remote-relay" "$remote_dist/spanreed-remote-relay"
"$remote_dist/spanreed" remote doctor
printf '%s\n' "Built $remote_dist/spanreed (Remote included) and the standalone relay; no installation or service changes were made."
