import base64
import importlib.util
from pathlib import Path
from unittest.mock import patch


SPEC = importlib.util.spec_from_file_location("source_bundle_fixture", Path(__file__).parents[1] / "source-bundle.py")
source = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(source)


def identity(entries):
    commit = (f"tree {source.git_tree(entries)}\nauthor Fixture <fixture@example.invalid> 0 +0000\n"
              "committer Fixture <fixture@example.invalid> 0 +0000\n\nSynthetic source fixture\n").encode()
    return {"head": source.object_hash("commit", commit).hex(), "commit": base64.b64encode(commit).decode()}


def fixtures(version="0.3.1", origin="https://desktop.grok.me"):
    portable = {name: (body, 0o644) for name, body in {
        "package.json": b'{"packageManager":"pnpm@10.33.2"}',
        "pnpm-lock.yaml": b"lockfileVersion: 9.0\n",
        "apps/web/src/main.ts": b"export {};\n",
        "LICENSE": b"Synthetic AGPL fixture\n",
        "LICENSES/AGPL-3.0-or-later.txt": b"Synthetic AGPL fixture\n",
        "site/install.sh": b"#!/bin/sh\n",
        "site/install.ps1": b"Write-Output test\n",
        "deploy/remote/build-output.json": b'{"version":3}\n',
        "deploy/remote/qualification-build-output.json": b'{"version":3,"routes":[]}\n',
    }.items()}
    vendor = {name: (body, 0o644) for name, body in {
        "LICENSE": b"Synthetic AGPL fixture\n",
        "Cargo.toml": b'[workspace]\nmembers = ["crates/*"]\n',
        "Cargo.lock": b"version = 4\n",
        "crates/fabrials-remote-noise/Cargo.toml": b'[package]\nname="noise"\n',
        "crates/fabrials-remote-noise/src/lib.rs": b"pub fn noise() {}\n",
        "tools/remote-bindgen/Cargo.lock": b"version = 4\n",
    }.items()}
    portable_identity = identity(portable)
    noise_identity = identity(vendor)
    manifest = {"schema": 1, "sources": {}}
    for name, entries, paths, repo, revision in (
        ("portable", portable, source.PORTABLE_PATHS, "grok-insider/grok-desktop-portable", portable_identity),
        ("noise", vendor, source.NOISE_PATHS, "grok-insider/fabrials-libs", noise_identity),
    ):
        files = source.selected(entries, paths)
        manifest["sources"][name] = {"repository": repo, "head": revision["head"], "dirty": False,
                                     "files": files, "treeSha256": source.sha256(source.encoded(files))}
    shell = {
        "index.html": ('<!doctype html><meta name="grok-app-origin" content="' + origin
                       + '"><meta name="grok-shell-build" content="' + "a" * 64 + '">').encode(),
        "manifest.webmanifest": b'{"start_url":"/","scope":"/"}',
        "sw.js": b"const shell=[];",
        "third-party-notices.txt": b"Synthetic dependency notices\n",
        "crypto/aaaaaaaaaaaaaaaaaaaa/fabrials_remote_noise.js": b"export default function(){}",
        "crypto/aaaaaaaaaaaaaaaaaaaa/fabrials_remote_noise_bg.wasm": b"\0asm\x01\0\0\0",
    }
    manifest["files"] = {name: source.sha256(data) for name, data in shell.items()}
    manifest["bundleSha256"] = source.sha256(source.encoded(manifest["files"]))
    spanreed = {name: (body, 0o644) for name, body in {
        "Cargo.toml": f'[package]\nname="spanreed"\nversion="{version}"\n'.encode(),
        "Cargo.lock": b"version = 4\n",
        "desktop/bun.lock": b"{}\n",
        "desktop/src-tauri/Cargo.lock": b"version = 4\n",
        "LICENSE": b"Synthetic AGPL fixture\n",
        "docs/source-distribution.md": b"Synthetic build instructions\n",
        "assets/remote-web.source.json": source.encoded(manifest),
        "vendor/fabrials-libs.source": f"commit={noise_identity['head']}\ntree={source.git_tree(vendor)}\n".encode(),
    }.items()}
    spanreed.update({"vendor/fabrials-libs/" + name: value for name, value in vendor.items()})
    spanreed.update({"assets/remote-web/" + name: (data, 0o644) for name, data in shell.items()})
    return spanreed, portable, {"spanreed": identity(spanreed), "portable": portable_identity}


SHA = fixtures()[2]["spanreed"]["head"]


def write_source(path, version="0.3.1", origin="https://desktop.grok.me"):
    spanreed, portable, identities = fixtures(version, origin)
    with patch.object(source, "snapshot", side_effect=[(spanreed, identities["spanreed"]), (portable, identities["portable"])]):
        source.build(Path("spanreed"), Path("portable"), path, version, identities["spanreed"]["head"])
