#!/usr/bin/env python3
"""Keep desktop manifests and local lock entries on the CLI release version."""

import json
from pathlib import Path
import re
import tomllib


root = Path(__file__).resolve().parents[1]
version = tomllib.loads((root / "Cargo.toml").read_text())["package"]["version"]
if not re.fullmatch(r"\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?", version):
    raise SystemExit(f"Unsupported release version: {version}")

updates = {}
for name in ("desktop/package.json", "desktop/src-tauri/tauri.conf.json"):
    path = root / name
    data = json.loads(path.read_text())
    data["version"] = version
    updates[path] = json.dumps(data, indent=2) + "\n"

path = root / "desktop/src-tauri/Cargo.toml"
text, count = re.subn(r'(?m)^version = "[^"]+"$', f'version = "{version}"', path.read_text(), count=1)
if count != 1:
    raise SystemExit("Desktop Cargo manifest has no package version")
updates[path] = text

path = root / "desktop/src-tauri/Cargo.lock"
text = path.read_text()
for package in ("spanreed", "spanreed-desktop"):
    pattern = rf'(\[\[package\]\]\nname = "{package}"\nversion = ")[^"]+("\n)'
    text, count = re.subn(pattern, lambda m: m[1] + version + m[2], text)
    if count != 1:
        raise SystemExit(f"Expected one {package} entry in desktop Cargo.lock")
updates[path] = text

for path, text in updates.items():
    path.write_text(text)
print(f"Desktop manifests and lock entries synchronized to {version}")
