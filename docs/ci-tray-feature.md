# CI / release: enabling the `tray` feature

Desktop release binaries (Windows + macOS) should build with `--features tray`.
Linux **musl** release artifacts stay without tray so the static binary check holds.

Apply these edits to `.github/workflows/` (requires a token with the `workflow` scope):

## `ci.yml` (rust job)

After `cargo test --all` with `SPANREED_OFFLINE=1`:

```yaml
      - run: cargo check --all-targets --features tray
      - run: cargo clippy --all-targets --features tray -- -D warnings
        env:
          SPANREED_OFFLINE: "1"
```

On the informational `cross` matrix (macos/windows), after the offline tests:

```yaml
      - run: cargo check --all-targets --features tray
      - run: cargo test --all --features tray
        env:
          SPANREED_OFFLINE: "1"
```

## `release.yml` (artifacts job)

Replace the native build step with:

```yaml
      # Linux musl stays without `tray` so the static binary check still holds.
      # Windows + macOS release assets include the Behelit system tray.
      - name: Build (zigbuild static)
        if: matrix.cross == 'zig'
        run: cargo zigbuild --release --target "$TARGET"
      - name: Build (native + tray)
        if: matrix.cross != 'zig'
        run: cargo build --release --target ${{ matrix.target }} --features tray
```

Local verification (any platform):

```bash
cargo build --release --features tray
SPANREED_OFFLINE=1 cargo test --all --features tray
```
