# CI / release: enabling the `tray` feature

Desktop release binaries (Windows + macOS) should build with `--features tray`.
Linux **musl** release artifacts stay without tray so the static binary check holds.

## `ci.yml` (rust job on Ubuntu)

Install GTK / AppIndicator before `--features tray`:

```bash
sudo apt-get update
sudo apt-get install -y libgtk-3-dev libxdo-dev libayatana-appindicator3-dev pkg-config
```

Then `cargo check/clippy --features tray`. Win/mac cross jobs need no extra apt packages.

## `release.yml`

Native (non-musl) builds use `--features tray`; musl zigbuild stays without tray.

## Local

```bash
cargo build --release --features tray
SPANREED_OFFLINE=1 cargo test --all --features tray
```
