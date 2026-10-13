#!/usr/bin/env bash
set -euo pipefail
root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
output=${1:?usage: build-remote-crypto.sh output-directory}
native_target=$("${RUSTC:-rustc}" -vV | sed -n 's/^host: //p')
test -n "$native_target"
env -u CARGO_ENCODED_RUSTFLAGS RUSTFLAGS='--cfg getrandom_backend="wasm_js"' \
  cargo build --manifest-path "$root/crates/fabrials-remote-noise/Cargo.toml" --target-dir "$root/crates/fabrials-remote-noise/target" --locked --release --target wasm32-unknown-unknown
cargo run --manifest-path "$root/tools/remote-bindgen/Cargo.toml" --target "$native_target" --locked -- \
  "$root/crates/fabrials-remote-noise/target/wasm32-unknown-unknown/release/fabrials_remote_noise.wasm" "$output"
