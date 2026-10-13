#!/usr/bin/env python3
import argparse
import base64
from contextlib import ExitStack
import gzip
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path, PurePosixPath
import re
import subprocess
import tarfile
import tempfile
import tomllib


ROOT = Path(__file__).resolve().parents[1]
LIMIT = 256 * 1024 * 1024
MAX_FILES = 20000
PORTABLE_PATHS = ("package.json", "pnpm-lock.yaml", "pnpm-workspace.yaml", ".npmrc", "apps/web", "scripts")
NOISE_PATHS = ("Cargo.toml", "Cargo.lock", "crates/fabrials-remote-noise", "tools/remote-bindgen")
SPEC = importlib.util.spec_from_file_location("remote_assets", ROOT / "scripts/remote-assets.py")
assets = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(assets)


def encoded(value):
    return json.dumps(value, sort_keys=True, separators=(",", ":")).encode()


def sha256(data):
    return hashlib.sha256(data).hexdigest()


def object_hash(kind, data):
    return hashlib.sha1(kind.encode() + b" " + str(len(data)).encode() + b"\0" + data).digest()


def safe_name(name):
    path = PurePosixPath(name)
    if not name or path.is_absolute() or path.as_posix() != name or any(
        part in (".", "..", ".git") for part in path.parts
    ) or "\\" in name or ":" in name or any(ord(char) < 32 for char in name):
        raise ValueError("Unsafe source path")
    return name


def git_tree(entries):
    tree = {}
    for name, (data, mode) in entries.items():
        node = tree
        parts = safe_name(name).split("/")
        for part in parts[:-1]:
            node = node.setdefault(part, {})
            if not isinstance(node, dict):
                raise ValueError("Source file/directory collision")
        if parts[-1] in node or mode not in (0o644, 0o755):
            raise ValueError("Duplicate or nonregular source entry")
        node[parts[-1]] = (data, mode)

    def digest(node):
        records = []
        for name, value in node.items():
            raw = name.encode()
            if isinstance(value, dict):
                records.append((raw + b"/", b"40000 " + raw + b"\0" + digest(value)))
            else:
                data, mode = value
                records.append((raw, f"100{mode:o} ".encode() + raw + b"\0" + object_hash("blob", data)))
        return object_hash("tree", b"".join(value for _, value in sorted(records)))

    return digest(tree).hex()


def git(directory, *args):
    return subprocess.check_output(["git", "-C", str(directory), *args])


def clean(directory):
    if git(directory, "status", "--porcelain", "--untracked-files=all"):
        raise ValueError("Corresponding source requires clean committed checkouts")


def snapshot(directory):
    clean(directory)
    head = git(directory, "rev-parse", "HEAD").decode().strip()
    commit = git(directory, "cat-file", "commit", head)
    entries = {}
    rows = []
    total = 0
    for row in git(directory, "ls-tree", "-rz", "--long", "--full-tree", head).split(b"\0"):
        if not row:
            continue
        meta, name = row.split(b"\t", 1)
        mode, kind, oid, size = meta.split()
        if kind != b"blob" or mode not in (b"100644", b"100755"):
            raise ValueError("Source exports require regular files; no links or submodules")
        total += int(size)
        if total > LIMIT or len(rows) >= MAX_FILES:
            raise ValueError("Oversized source export")
        rows.append((safe_name(name.decode()), int(mode, 8) & 0o777, oid))
    total = 0
    with tempfile.TemporaryFile() as requests, tempfile.TemporaryFile() as output:
        requests.write(b"".join(oid + b"\n" for _, _, oid in rows))
        requests.seek(0)
        subprocess.run(["git", "-C", str(directory), "cat-file", "--batch"], stdin=requests, stdout=output, check=True)
        output.seek(0)
        for name, mode, oid in rows:
            found, kind, size = output.readline().split()
            size = int(size)
            total += size
            if found != oid or kind != b"blob" or total > LIMIT or len(entries) >= MAX_FILES:
                raise ValueError("Invalid or oversized source export")
            data = output.read(size)
            if len(data) != size or output.read(1) != b"\n":
                raise ValueError("Truncated Git object")
            entries[name] = (data, mode)
    identity = {"head": head, "commit": base64.b64encode(commit).decode()}
    verify_commit(entries, identity)
    clean(directory)
    if git(directory, "rev-parse", "HEAD").decode().strip() != head:
        raise ValueError("Source changed during export")
    return entries, identity


def verify_commit(entries, identity):
    commit = base64.b64decode(identity["commit"], validate=True)
    if not re.fullmatch(r"[0-9a-f]{40}", identity["head"]) or object_hash("commit", commit).hex() != identity["head"]:
        raise ValueError("Source commit identity mismatch")
    if commit.split(b"\n", 1)[0] != b"tree " + git_tree(entries).encode():
        raise ValueError("Source tree differs from its Git commit")


def selected(entries, paths):
    return {name: sha256(data) for name, (data, _) in entries.items()
            if any(name == prefix or name.startswith(prefix + "/") for prefix in paths)}


def under(entries, prefix):
    return {name[len(prefix):]: value for name, value in entries.items() if name.startswith(prefix)}


def corresponding(spanreed, portable, identities, version, sha):
    if identities["spanreed"]["head"] != sha:
        raise ValueError("Source bundle differs from the release SHA")
    if tomllib.loads(spanreed["Cargo.toml"][0].decode())["package"]["version"] != version:
        raise ValueError("Source bundle version mismatch")
    manifest = json.loads(spanreed["assets/remote-web.source.json"][0])
    with tempfile.TemporaryDirectory(prefix="spanreed-source-check-") as directory:
        root = Path(directory)
        for name, (data, _) in under(spanreed, "assets/remote-web/").items():
            path = root / safe_name(name)
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)
        assets.validate_bundle(root, manifest, release=True)
    source = manifest["sources"]
    vendor = under(spanreed, "vendor/fabrials-libs/")
    pin = dict(line.split("=", 1) for line in spanreed["vendor/fabrials-libs.source"][0].decode().splitlines() if "=" in line)
    if source["noise"]["head"] != pin.get("commit") or git_tree(vendor) != pin.get("tree"):
        raise ValueError("Noise must use the exact committed Spanreed vendor revision; sync and rebuild")
    if source["portable"]["head"] != identities["portable"]["head"]:
        raise ValueError("Portable revision differs from embedded PWA")
    for name, files in (("portable", selected(portable, PORTABLE_PATHS)), ("noise", selected(vendor, NOISE_PATHS))):
        expected_repository = "grok-insider/grok-desktop-portable" if name == "portable" else "grok-insider/fabrials-libs"
        if source[name].get("repository") != expected_repository or source[name]["files"] != files:
            raise ValueError(f"{name} source files differ from embedded build inputs")
    for entries, paths in ((spanreed, ("LICENSE", "vendor/fabrials-libs/LICENSE", "docs/source-distribution.md",
                                     "Cargo.lock", "desktop/bun.lock", "desktop/src-tauri/Cargo.lock",
                                     "vendor/fabrials-libs/tools/remote-bindgen/Cargo.lock")),
                           (portable, ("LICENSE", "LICENSES/AGPL-3.0-or-later.txt", "pnpm-lock.yaml",
                                       "site/install.sh", "site/install.ps1"))):
        if any(not entries.get(name, (b"",))[0] for name in paths):
            raise ValueError("Missing corresponding-source license, build recipe or lockfile")
    return manifest


def notice(entries):
    names = sorted(name for name in entries if re.search(r"(^|/)([^/]*[-_])?(LICENSE[^/]*|NOTICE[^/]*|COPYING[^/]*)(/|$)", name, re.I))
    return ("Spanreed corresponding source\n\n"
            "Spanreed and Fabrials libraries: AGPL-3.0-or-later.\n"
            "Portable: AGPL-3.0-or-later unless separately licensed in writing.\n"
            "Vendored components retain their accompanying notices and license terms.\n"
            "Public registry dependencies and versions are recorded in the included lockfiles.\n"
            "Read BUILDING.md and the following original license/notice files:\n\n" + "\n".join(names) + "\n").encode()


def write_archive(path, entries):
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("xb") as output:
        archive_to(output, entries)


def archive_to(output, entries):
    with gzip.GzipFile(filename="", mode="wb", fileobj=output, mtime=0) as compressed:
        with tarfile.open(fileobj=compressed, mode="w", format=tarfile.USTAR_FORMAT) as archive:
            for name, (data, mode) in sorted(entries.items()):
                info = tarfile.TarInfo(safe_name(name))
                info.size = len(data)
                info.mode = mode
                archive.addfile(info, io.BytesIO(data))


def build(spanreed_root, portable_root, output, version, sha):
    spanreed, native_identity = snapshot(spanreed_root)
    portable, portable_identity = snapshot(portable_root)
    identities = {"spanreed": native_identity, "portable": portable_identity}
    manifest = corresponding(spanreed, portable, identities, version, sha)
    entries = {"spanreed/" + name: value for name, value in spanreed.items()}
    entries.update({"portable/" + name: value for name, value in portable.items()})
    entries["BUILDING.md"] = (spanreed["docs/source-distribution.md"][0], 0o644)
    entries["NOTICE.txt"] = (notice(entries), 0o644)
    record = {"schema": 1, "version": version, "sha": sha, "sources": identities,
              "remoteBundleSha256": manifest["bundleSha256"],
              "files": {name: {"sha256": sha256(data), "mode": mode} for name, (data, mode) in entries.items()}}
    entries["source-bundle.json"] = (encoded(record) + b"\n", 0o644)
    if len(entries) > MAX_FILES or sum(len(data) for data, _ in entries.values()) > LIMIT:
        raise ValueError("Oversized combined source bundle")
    output.parent.mkdir(parents=True, exist_ok=True)
    sidecar = output.with_name(output.name + ".sha256")
    owned = {}
    try:
        with ExitStack() as stack:
            archive = stack.enter_context(output.open("xb"))
            owned[output] = os.fstat(archive.fileno())
            checksum = stack.enter_context(sidecar.open("x", encoding="utf-8"))
            owned[sidecar] = os.fstat(checksum.fileno())
            archive_to(archive, entries)
            archive.flush()
            verify(output, version, sha)
            checksum.write(f"{sha256(output.read_bytes())}  {output.name}\n")
    except BaseException:
        for path, created in owned.items():
            try:
                current = path.lstat()
                if (current.st_dev, current.st_ino) == (created.st_dev, created.st_ino):
                    path.unlink()
            except FileNotFoundError:
                pass
        raise


def verify(path, version, sha, include_files=False):
    entries = {}
    total = 0
    tar_limit = LIMIT + MAX_FILES * 2048 + 10240
    if path.stat().st_size > tar_limit:
        raise ValueError("Unsafe or oversized source archive")
    with gzip.open(path, "rb") as compressed:
        data = compressed.read(tar_limit + 1)
    if len(data) > tar_limit:
        raise ValueError("Unsafe or oversized expanded source archive")
    with tarfile.open(fileobj=io.BytesIO(data), mode="r:") as archive:
        for member in archive:
            name = safe_name(member.name)
            total += member.size
            if not member.isfile() or member.mode not in (0o644, 0o755) or name in entries \
                    or total > LIMIT or len(entries) >= MAX_FILES:
                raise ValueError("Unsafe, duplicate or oversized source archive entry")
            entries[name] = (archive.extractfile(member).read(), member.mode)
    record_data, _ = entries.pop("source-bundle.json")
    record = json.loads(record_data)
    inventory = {name: {"sha256": sha256(data), "mode": mode} for name, (data, mode) in entries.items()}
    if record.get("schema") != 1 or record.get("version") != version or record.get("sha") != sha or record.get("files") != inventory:
        raise ValueError("Source archive inventory differs from its manifest")
    if any(name not in ("BUILDING.md", "NOTICE.txt") and not name.startswith(("spanreed/", "portable/")) for name in entries):
        raise ValueError("Unexpected source archive entry")
    spanreed = under(entries, "spanreed/")
    portable = under(entries, "portable/")
    for name, files in (("spanreed", spanreed), ("portable", portable)):
        verify_commit(files, record["sources"][name])
    manifest = corresponding(spanreed, portable, record["sources"], version, sha)
    if record["remoteBundleSha256"] != manifest["bundleSha256"] or entries["BUILDING.md"] != (spanreed["docs/source-distribution.md"][0], 0o644):
        raise ValueError("Source build instructions or Remote identity mismatch")
    if entries["NOTICE.txt"] != (notice({name: value for name, value in entries.items() if name != "NOTICE.txt"}), 0o644):
        raise ValueError("Source notices mismatch")
    return (record, entries) if include_files else record


def input_digest(entries):
    digest = hashlib.sha256()
    for name in sorted(entries, key=lambda value: value.encode("utf-16be")):
        digest.update(name.encode() + b"\0")
        digest.update(entries[name][0])
    return digest.hexdigest()


def pwa_proof(archive, dist, version, sha):
    record, entries = verify(archive, version, sha, include_files=True)
    spanreed = under(entries, "spanreed/")
    portable = under(entries, "portable/")
    vendor = under(spanreed, "vendor/fabrials-libs/")
    manifest = json.loads(spanreed["assets/remote-web.source.json"][0])
    for name, expected in manifest["files"].items():
        path = dist / safe_name(name)
        if path.is_symlink() or not path.is_file() or sha256(path.read_bytes()) != expected:
            raise ValueError("PWA shell differs from the corresponding-source archive")
    index = (dist / "index.html").read_text()
    matches = re.findall(r'<meta name="grok-shell-build" content="([a-f0-9]{64})">', index)
    if len(matches) != 1:
        raise ValueError("Missing or ambiguous PWA shell build identity")
    return {"schema": 1, "shellId": matches[0], "shellSha256": manifest["bundleSha256"],
            "archiveSha256": sha256(archive.read_bytes()), "archiveBytes": archive.stat().st_size,
            "native": {"version": version, "commit": sha},
            "sources": {
                "portable": {"commit": record["sources"]["portable"]["head"], "sha256": input_digest(portable), "dirty": False},
                "libs": {"commit": manifest["sources"]["noise"]["head"], "sha256": input_digest(vendor), "dirty": False},
            },
            "licenseText": portable["LICENSE"][0].decode() + "\n\n" + portable["LICENSES/AGPL-3.0-or-later.txt"][0].decode(),
            "distributionFiles": {name: sha256(portable[name][0]) for name in (
                "site/install.sh", "site/install.ps1", "deploy/remote/build-output.json",
                "deploy/remote/qualification-build-output.json")},
            "noticesText": spanreed["assets/remote-web/third-party-notices.txt"][0].decode()}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("command", choices=("build", "verify", "inputs", "pwa-proof"))
    parser.add_argument("--portable", type=Path)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--archive", type=Path)
    parser.add_argument("--version")
    parser.add_argument("--sha")
    parser.add_argument("--pwa-dist", type=Path)
    args = parser.parse_args()
    if args.command == "pwa-proof":
        if not args.archive or not args.pwa_dist:
            parser.error("pwa-proof requires --archive and --pwa-dist")
        clean(ROOT)
        version = tomllib.loads((ROOT / "Cargo.toml").read_text())["package"]["version"]
        sha = git(ROOT, "rev-parse", "HEAD").decode().strip()
        print(json.dumps(pwa_proof(args.archive, args.pwa_dist, version, sha)))
        return
    if args.command == "inputs":
        manifest = json.loads((ROOT / "assets/remote-web.source.json").read_text())
        assets.validate_bundle(ROOT / "assets/remote-web", manifest, release=True)
        head = manifest["sources"]["portable"]["head"]
        if not re.fullmatch(r"[0-9a-f]{40}", head):
            raise ValueError("Invalid Portable revision")
        print("portable_sha=" + head)
        return
    if not args.version or not re.fullmatch(r"\d+\.\d+\.\d+", args.version) or not args.sha or not re.fullmatch(r"[0-9a-f]{40}", args.sha):
        parser.error("--version x.y.z and --sha <full commit> are required")
    if args.command == "build":
        if not args.portable or not args.output:
            parser.error("build requires --portable and --output")
        build(ROOT, args.portable.resolve(), args.output.resolve(), args.version, args.sha)
    else:
        if not args.archive:
            parser.error("verify requires --archive")
        verify(args.archive, args.version, args.sha)
    print("Corresponding source verified; nothing published")


if __name__ == "__main__":
    main()
