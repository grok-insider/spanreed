#!/usr/bin/env python3
import argparse
import hashlib
import json
import os
import re
from pathlib import Path
import shutil
import subprocess
import tempfile


ROOT = Path(__file__).resolve().parents[1]
BUNDLE = ROOT / "assets/remote-web"
MANIFEST = ROOT / "assets/remote-web.source.json"
REQUIRED = {"index.html", "manifest.webmanifest", "sw.js"}


def digest(data):
    return hashlib.sha256(data).hexdigest()


def encoded(value):
    return json.dumps(value, sort_keys=True, separators=(",", ":")).encode()


def files(directory):
    if not directory.is_dir() or directory.is_symlink():
        raise ValueError("Bundle must be a regular directory")
    result = {}
    total = 0
    for path in sorted(directory.rglob("*")):
        if path.is_symlink():
            raise ValueError("Bundle must not contain symlinks")
        if path.is_dir():
            continue
        if not path.is_file():
            raise ValueError("Bundle must contain only regular files")
        name = path.relative_to(directory).as_posix()
        if "\\" in name or any(ord(char) < 32 for char in name):
            raise ValueError("Invalid bundle filename")
        total += path.stat().st_size
        if total > 32 * 1024 * 1024:
            raise ValueError("Bundle exceeds 32 MiB")
        result[name] = digest(path.read_bytes())
    return result


def validate_bundle(directory, manifest, release=False):
    actual = files(directory)
    if manifest.get("schema") != 1 or not REQUIRED <= actual.keys():
        raise ValueError("Incomplete Remote bundle")
    if manifest.get("files") != actual or manifest.get("bundleSha256") != digest(encoded(actual)):
        raise ValueError("Remote bundle differs from its source manifest")
    wasm = [name for name in actual if name.startswith("crypto/") and name.endswith("/fabrials_remote_noise_bg.wasm")]
    if len(wasm) != 1 or not re.fullmatch(r"crypto/[0-9a-f]{16,64}/fabrials_remote_noise_bg\.wasm", wasm[0]) or wasm[0].replace("_bg.wasm", ".js") not in actual:
        raise ValueError("Remote crypto pair is missing or ambiguous")
    if (directory / wasm[0]).read_bytes()[:8] != b"\0asm\x01\0\0\0":
        raise ValueError("Remote crypto is not a WASM module")
    pwa = json.loads((directory / "manifest.webmanifest").read_text())
    if pwa.get("start_url") != "/" or pwa.get("scope") != "/":
        raise ValueError("Remote manifest must use the same origin root")
    for name in ("portable", "noise"):
        source = manifest.get("sources", {}).get(name, {})
        if not source.get("files") or source.get("treeSha256") != digest(encoded(source["files"])):
            raise ValueError("Missing or invalid source identity")
        if len(source.get("head", "")) != 40 or not isinstance(source.get("dirty"), bool):
            raise ValueError("Missing source revision")
        if release and source["dirty"]:
            raise ValueError("Release bundle sources must be committed; rebuild after committing Portable and Noise")
    return len(actual)


def run(command, directory, env=None):
    subprocess.run(command, cwd=directory, env=env, check=True)


def source_identity(directory, paths, repository):
    tracked = subprocess.check_output(["git", "ls-files", "--cached", "--others", "--exclude-standard", "-z", "--", *paths], cwd=directory)
    inputs = {}
    for name in sorted(set(tracked.decode().split("\0")) - {""}):
        path = directory / name
        if not path.exists() and not path.is_symlink():
            continue
        if path.is_symlink() or not path.is_file():
            raise ValueError(f"Missing or nonregular build input: {name}")
        inputs[name] = digest(path.read_bytes())
    return {
        "repository": repository,
        "head": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=directory, text=True).strip(),
        "dirty": bool(subprocess.check_output(["git", "status", "--porcelain", "--untracked-files=all", "--", *paths], cwd=directory)),
        "treeSha256": digest(encoded(inputs)),
        "files": inputs,
    }


def build(portable, libs):
    portable_paths = ["package.json", "pnpm-lock.yaml", "pnpm-workspace.yaml", ".npmrc", "apps/web", "scripts"]
    noise_paths = ["Cargo.toml", "Cargo.lock", "crates/fabrials-remote-noise", "tools/remote-bindgen"]
    def identities():
        return {
            "portable": source_identity(portable, portable_paths, "grok-insider/grok-desktop-portable"),
            "noise": source_identity(libs, noise_paths, "grok-insider/fabrials-libs"),
        }
    before = identities()
    compiler = subprocess.check_output([os.environ.get("RUSTC", "rustc"), "-vV"], text=True)
    native_target = next(line.removeprefix("host: ") for line in compiler.splitlines() if line.startswith("host: "))
    target = ROOT / "target/remote-assets"
    target.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="crypto-", dir=target) as temporary:
        crypto = Path(temporary)
        env = dict(os.environ)
        env["RUSTFLAGS"] = '--cfg getrandom_backend="wasm_js"'
        env.pop("CARGO_ENCODED_RUSTFLAGS", None)
        run(["cargo", "build", "--locked", "--release", "--manifest-path", str(libs / "crates/fabrials-remote-noise/Cargo.toml"), "--target", "wasm32-unknown-unknown", "--target-dir", str(target / "wasm")], libs, env)
        run(["cargo", "run", "--locked", "--manifest-path", str(libs / "tools/remote-bindgen/Cargo.toml"), "--target", native_target, "--target-dir", str(target / "bindgen"), "--", str(target / "wasm/wasm32-unknown-unknown/release/fabrials_remote_noise.wasm"), str(crypto)], libs)
        env = dict(os.environ, FABRIALS_REMOTE_CRYPTO_DIST=str(crypto))
        run(["node", "scripts/build-remote.mjs"], portable, env)
        if before != identities():
            raise ValueError("Build inputs changed while building; retry from a stable source tree")
        source = portable / "apps/web/dist"
        inventory = files(source)
        manifest = {"schema": 1, "sources": before, "files": inventory, "bundleSha256": digest(encoded(inventory)), "build": {
            "rustc": compiler.strip(),
            "node": subprocess.check_output(["node", "--version"], text=True).strip(),
            "wasmTarget": "wasm32-unknown-unknown",
            "wasmRustflags": '--cfg getrandom_backend="wasm_js"',
        }}
        validate_bundle(source, manifest)
        BUNDLE.parent.mkdir(parents=True, exist_ok=True)
        staged = Path(tempfile.mkdtemp(prefix=".remote-web-", dir=BUNDLE.parent))
        try:
            shutil.copytree(source, staged, dirs_exist_ok=True)
            validate_bundle(staged, manifest)
            if BUNDLE.exists():
                shutil.rmtree(BUNDLE)
            staged.rename(BUNDLE)
            MANIFEST.write_text(json.dumps(manifest, sort_keys=True, indent=2) + "\n")
        finally:
            if staged.exists():
                shutil.rmtree(staged)
        print(f"Embedded bundle: {len(inventory)} files, {manifest['bundleSha256']}")


def main():
    parser = argparse.ArgumentParser()
    commands = parser.add_subparsers(dest="command", required=True)
    check = commands.add_parser("check")
    check.add_argument("--release", action="store_true")
    refresh = commands.add_parser("build")
    refresh.add_argument("--portable", type=Path, required=True)
    refresh.add_argument("--libs", type=Path, required=True)
    args = parser.parse_args()
    if args.command == "build":
        build(args.portable.resolve(), args.libs.resolve())
    else:
        count = validate_bundle(BUNDLE, json.loads(MANIFEST.read_text()), args.release)
        print(f"Remote bundle verified: {count} files")


if __name__ == "__main__":
    main()
