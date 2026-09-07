#!/usr/bin/env python3
"""Release orchestration. GitHub writes are confined to explicit subcommands."""

import argparse
import datetime
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import tomllib

REPO = "grok-insider/spanreed"
PREFIXES = ("release-plz-v", "release-plz-manual-v")
CHECK_WORKFLOWS = ("ci.yml", "nix.yml", "desktop.yml")


def run(*args):
    return subprocess.check_output(args, text=True).strip()


def api(path):
    return json.loads(run("gh", "api", f"repos/{REPO}/{path}"))


def git(*args):
    return run("git", *args)


def version_tuple(version):
    if not re.fullmatch(r"\d+\.\d+\.\d+", version):
        raise ValueError(f"Expected a stable x.y.z version, got {version!r}")
    return tuple(map(int, version.split(".")))


def next_version(version, bump):
    major, minor, patch = version_tuple(version)
    return {"patch": f"{major}.{minor}.{patch + 1}",
            "minor": f"{major}.{minor + 1}.0", "major": f"{major + 1}.0.0"}[bump]


def expected_assets(version):
    version_tuple(version)
    names = [f"spanreed-{version}-x86_64-unknown-linux-musl.tar.gz",
             f"spanreed-{version}-x86_64-pc-windows-msvc.zip",
             f"Spanreed_{version}_amd64.deb", f"Spanreed_{version}_x64-setup.exe"]
    return names + [name + ".sha256" for name in names]


def package_version():
    return tomllib.loads(Path("Cargo.toml").read_text())["package"]["version"]


def check_versions():
    version = package_version()
    version_tuple(version)
    for name in ("desktop/package.json", "desktop/src-tauri/tauri.conf.json"):
        if json.loads(Path(name).read_text())["version"] != version:
            raise ValueError(f"Version mismatch: {name}")
    if tomllib.loads(Path("desktop/src-tauri/Cargo.toml").read_text())["package"]["version"] != version:
        raise ValueError("Desktop Cargo version differs from CLI")
    for name, packages in (("Cargo.lock", ("spanreed",)),
                           ("desktop/src-tauri/Cargo.lock", ("spanreed", "spanreed-desktop"))):
        entries = tomllib.loads(Path(name).read_text())["package"]
        for package in packages:
            matches = [p for p in entries if p["name"] == package]
            if len(matches) != 1 or matches[0]["version"] != version:
                raise ValueError(f"Version mismatch: {name}/{package}")
    return version


def bump_files(version):
    version_tuple(version)
    path = Path("Cargo.toml")
    text, count = re.subn(r'(?m)^version = "[^"]+"$', f'version = "{version}"', path.read_text(), count=1)
    if count != 1:
        raise ValueError("Missing package version")
    path.write_text(text)
    path = Path("Cargo.lock")
    text, count = re.subn(r'(\[\[package\]\]\nname = "spanreed"\nversion = ")[^"]+("\n)',
                        lambda m: m[1] + version + m[2], path.read_text())
    if count != 1:
        raise ValueError("Missing local lock entry")
    path.write_text(text)
    run(sys.executable, "scripts/sync-desktop-version.py")
    check_versions()


def release_notes(version):
    text = Path("CHANGELOG.md").read_text()
    match = re.search(rf"(?m)^## \[{re.escape(version)}\][^\n]*\n(.*?)(?=^## |\Z)", text, re.S | re.M)
    if not match or not match[1].strip():
        raise ValueError("Release changelog section missing or empty")
    return match[1].strip() + "\n\nWindows artifacts are unsigned. macOS/ARM distribution is deferred.\n\n" + \
        "Install instructions: https://fabrials.com/install/spanreed.sh (Linux x86_64) and " + \
        "https://fabrials.com/install/spanreed.ps1 (Windows x64).\n"


def validate_pr(number, sha, release_only=False):
    pr = api(f"pulls/{int(number)}")
    if pr["state"] != "open" or pr["head"]["repo"]["full_name"] != REPO or pr["head"]["sha"] != sha:
        raise ValueError("PR is closed, external, or changed SHA")
    base, head = pr["base"]["ref"], pr["head"]["ref"]
    if release_only or base == "master":
        if base != "master" or not head.startswith(PREFIXES):
            raise ValueError("Only release branches may target master")
    elif base != "dev" or not head.startswith(("sync-master-", "release-plz-")):
        raise ValueError("Unexpected dispatched integration PR")
    return pr


def dispatch(number, sha):
    pr = validate_pr(number, sha)
    workflows = list(CHECK_WORKFLOWS)
    if pr["base"]["ref"] == "master":
        workflows.append("guard-master.yml")
    for workflow in workflows:
        run("gh", "workflow", "run", workflow, "--repo", REPO, "--ref", pr["head"]["ref"],
            "-f", f"pr_number={number}", "-f", f"expected_sha={sha}")
    print(f"Dispatched {len(workflows)} workflows for PR #{number} at {sha}")


def validate_source(sha):
    if not re.fullmatch(r"[0-9a-f]{40}", sha):
        raise ValueError("release_sha must be a full commit SHA")
    if git("rev-parse", "HEAD") != sha:
        raise ValueError("Checkout differs from requested release SHA")
    git("merge-base", "--is-ancestor", sha, "origin/master")
    prs = api(f"commits/{sha}/pulls")
    if not any(p.get("merged_at") and p.get("merge_commit_sha") == sha
               and p["base"]["ref"] == "master" and p["head"]["ref"].startswith(PREFIXES) for p in prs):
        raise ValueError("Commit is not the merge of a release PR into master")
    version = check_versions()
    release_notes(version)
    tags = git("tag", "--list", "v" + version)
    if tags and git("rev-parse", f"v{version}^{{commit}}") != sha:
        raise ValueError("Existing tag points to another commit")
    return version


def verify_assets(root, version, sha):
    files = {}
    for name in expected_assets(version):
        matches = list(root.rglob(name))
        if len(matches) != 1 or not matches[0].is_file() or matches[0].is_symlink():
            raise ValueError(f"Expected exactly one regular artifact: {name}")
        files[name] = matches[0]
    manifests = list(root.rglob("release-provenance.json"))
    if len(manifests) != 4:
        raise ValueError("Expected provenance for all four builders")
    claimed = []
    for path in manifests:
        manifest = json.loads(path.read_text())
        if manifest["sha"] != sha or manifest["version"] != version:
            raise ValueError("Artifact provenance differs from release")
        claimed.extend(manifest["assets"])
    if sorted(claimed) != sorted(files):
        raise ValueError("Builder asset manifests overlap or omit required assets")
    for name, path in files.items():
        if not name.endswith(".sha256"):
            expected = files[name + ".sha256"].read_text().split()
            if expected != [hashlib.sha256(path.read_bytes()).hexdigest(), name]:
                raise ValueError(f"Checksum mismatch: {name}")
    return files


def provenance(directory):
    files = sorted(p.name for p in directory.iterdir() if p.is_file() and p.name != "release-provenance.json")
    (directory / "release-provenance.json").write_text(json.dumps({
        "sha": git("rev-parse", "HEAD"), "version": check_versions(), "assets": files,
    }, indent=2) + "\n")


def published_complete(sha, version):
    release = next((r for r in api("releases?per_page=100")
                    if r["tag_name"] == "v" + version), None)
    if not release or release["draft"]:
        return False
    obj = api("git/ref/tags/v" + version)["object"]
    if obj["type"] == "tag":
        obj = api("git/tags/" + obj["sha"])["object"]
    if obj["sha"] != sha:
        raise ValueError("Published release tag points to another SHA")
    if sorted(a["name"] for a in release["assets"]) != sorted(expected_assets(version)):
        raise ValueError("Published release has an incomplete or unexpected asset set")
    with tempfile.TemporaryDirectory() as directory:
        run("gh", "release", "download", "v" + version, "--repo", REPO, "--dir", directory)
        root = Path(directory)
        for name in expected_assets(version)[:4]:
            digest = hashlib.sha256((root / name).read_bytes()).hexdigest()
            if (root / (name + ".sha256")).read_text().split() != [digest, name]:
                raise ValueError("Published release checksum mismatch")
    return True


def publish(sha, run_id):
    version = validate_source(sha)
    build = api(f"actions/runs/{int(run_id)}")
    current = os.environ.get("GITHUB_RUN_ID") == str(run_id)
    if build["head_sha"] != sha or build["event"] not in ("push", "workflow_dispatch") \
            or build["path"] != ".github/workflows/release.yml" \
            or (not current and build["status"] != "completed"):
        raise ValueError("Recovery artifacts must come from a successful release run at the same SHA")
    if not current:
        jobs = api(f"actions/runs/{int(run_id)}/jobs?per_page=100")["jobs"]
        builders = [j for j in jobs if j["name"].startswith(("cli / cli (", "desktop / desktop ("))]
        if len(builders) != 4 or any(j["conclusion"] != "success" for j in builders):
            raise ValueError("Recovery requires all four successful builder jobs")
    with tempfile.TemporaryDirectory() as temporary:
        root = Path(temporary)
        run("gh", "run", "download", str(run_id), "--repo", REPO, "--pattern", "spanreed-release-*", "--dir", str(root))
        files = verify_assets(root, version, sha)
        tag = "v" + version
        releases = api("releases?per_page=100")
        release = next((r for r in releases if r["tag_name"] == tag), None)
        if release:
            remote = api("git/ref/tags/" + tag)["object"]
            if remote["type"] == "tag":
                remote = api("git/tags/" + remote["sha"])["object"]
            if remote["sha"] != sha:
                raise ValueError("Remote tag conflict")
            existing = {a["name"]: a for a in release["assets"]}
            if set(existing) - set(files):
                raise ValueError("Release has unexpected assets")
            for name in existing:
                destination = root / "existing" / name
                destination.parent.mkdir(exist_ok=True)
                run("gh", "release", "download", tag, "--repo", REPO, "--pattern", name, "--dir", str(destination.parent))
                if destination.read_bytes() != files[name].read_bytes():
                    raise ValueError(f"Existing asset differs; refusing overwrite: {name}")
            if not release["draft"]:
                if set(existing) != set(files):
                    raise ValueError("Published release is incomplete; refusing mutation")
                print("Release already complete; no changes")
                return
        else:
            existing = {}
            notes = root / "notes.md"
            notes.write_text(release_notes(version))
            run("gh", "release", "create", tag, "--repo", REPO, "--target", sha, "--title", tag,
                "--notes-file", str(notes), "--draft")
        for name, path in files.items():
            if name not in existing:
                run("gh", "release", "upload", tag, "--repo", REPO, str(path))
        # Download the complete draft before making it visible/latest.
        check = root / "uploaded"
        run("gh", "release", "download", tag, "--repo", REPO, "--dir", str(check))
        if {p.name for p in check.iterdir()} != set(files):
            raise ValueError("Uploaded asset set is incomplete")
        for name, path in files.items():
            if (check / name).read_bytes() != path.read_bytes():
                raise ValueError(f"Upload verification failed: {name}")
        run("gh", "release", "edit", tag, "--repo", REPO, "--draft=false", "--latest")


def prepare(bump, force, dry_run):
    latest = api("releases/latest")
    base = latest["tag_name"].removeprefix("v")
    version = next_version(base, bump)
    git("fetch", "origin", "dev", "master", "--tags")
    git("checkout", "-B", "prepare-release", "origin/dev")
    if subprocess.run(["git", "merge-base", "--is-ancestor", latest["tag_name"], "HEAD"]).returncode:
        raise ValueError("dev lacks the latest release; merge the master-to-dev synchronization PR first")
    if package_version() != base:
        raise ValueError("dev version differs from the latest stable release; resolve version drift first")
    subjects = git("log", f"{latest['tag_name']}..HEAD", "--no-merges", "--format=%s").splitlines()
    if not force and not any(re.match(r"^(feat|fix)(\(.*\))?!?:", s) for s in subjects):
        print("No releasable changes")
        return
    branch = ("release-plz-v" if bump == "patch" else "release-plz-manual-v") + version
    remote = git("ls-remote", "--heads", "origin", branch)
    if remote:
        git("fetch", "origin", branch)
        commits = git("log", f"origin/dev..origin/{branch}", "--no-merges", "--format=%ae%x09%s").splitlines()
        expected = f"41898282+github-actions[bot]@users.noreply.github.com\tchore: prepare release {version}"
        if not commits or any(c != expected for c in commits):
            raise ValueError("Release branch contains non-bot work; review manually")
        git("checkout", "-B", branch, "origin/" + branch)
        git("merge", "--no-edit", "origin/dev")
    else:
        git("checkout", "-B", branch)
    bump_files(version)
    path = Path("CHANGELOG.md")
    text = path.read_text()
    section = f"## [{version}] - {datetime.date.today().isoformat()}\n\n" + "\n".join("- " + s for s in subjects) + "\n\n"
    pattern = rf"(?ms)^## \[{re.escape(version)}\][^\n]*\n.*?(?=^## |\Z)"
    if re.search(pattern, text):
        text = re.sub(pattern, lambda _: section, text)
    else:
        offset = text.index("\n## ") + 1
        text = text[:offset] + section + text[offset:]
    path.write_text(text)
    if dry_run:
        print(git("diff"))
        return
    git("add", "Cargo.toml", "Cargo.lock", "CHANGELOG.md", "desktop/package.json", "desktop/src-tauri/Cargo.toml",
        "desktop/src-tauri/Cargo.lock", "desktop/src-tauri/tauri.conf.json")
    if subprocess.run(["git", "diff", "--cached", "--quiet"]).returncode:
        git("commit", "-m", f"chore: prepare release {version}")
    git("push", "--set-upstream", "origin", branch)
    prs = json.loads(run("gh", "pr", "list", "--repo", REPO, "--head", branch, "--base", "master", "--json", "number"))
    if not prs:
        run("gh", "pr", "create", "--repo", REPO, "--base", "master", "--head", branch,
            "--title", f"chore: release v{version}", "--body", "Release prepared from dev. CLI and desktop versions are synchronized. Windows is unsigned; macOS/ARM deferred.")
        prs = json.loads(run("gh", "pr", "list", "--repo", REPO, "--head", branch, "--base", "master", "--json", "number"))
    dispatch(prs[0]["number"], git("rev-parse", "HEAD"))


def sync_dev():
    git("fetch", "origin", "master", "dev")
    sha = git("rev-parse", "origin/master")
    if subprocess.run(["git", "merge-base", "--is-ancestor", sha, "origin/dev"]).returncode == 0:
        print("dev already includes master")
        return
    branch = "sync-master-" + sha[:12]
    prs = json.loads(run("gh", "pr", "list", "--repo", REPO, "--head", branch, "--base", "dev", "--json", "number"))
    if not prs:
        git("checkout", "-B", branch, "origin/dev")
        git("merge", "--no-edit", "origin/master")
        git("push", "origin", branch)
        run("gh", "pr", "create", "--repo", REPO, "--base", "dev", "--head", branch,
            "--title", "chore: synchronize released master into dev", "--body", "Preserves development changes and includes the published release baseline. Merge after checks pass.")
        prs = json.loads(run("gh", "pr", "list", "--repo", REPO, "--head", branch, "--base", "dev", "--json", "number"))
    dispatch(prs[0]["number"], api(f"pulls/{prs[0]['number']}")["head"]["sha"])


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("command", choices=("prepare", "validate-pr", "validate-source", "publish", "provenance", "sync-dev"))
    parser.add_argument("--sha", default=os.environ.get("GITHUB_SHA", ""))
    parser.add_argument("--pr", type=int)
    parser.add_argument("--release-only", action="store_true")
    parser.add_argument("--bump", choices=("patch", "minor", "major"), default="patch")
    parser.add_argument("--force", action="store_true")
    parser.add_argument("--dry-run", action="store_true")
    parser.add_argument("--run-id", default=os.environ.get("GITHUB_RUN_ID", ""))
    parser.add_argument("--directory", type=Path)
    args = parser.parse_args()
    if args.command == "prepare":
        prepare(args.bump, args.force, args.dry_run)
    elif args.command == "validate-pr":
        validate_pr(args.pr, args.sha, args.release_only)
    elif args.command == "validate-source":
        version = validate_source(args.sha)
        print(version)
        if os.environ.get("GITHUB_OUTPUT"):
            with open(os.environ["GITHUB_OUTPUT"], "a") as output:
                complete = str(published_complete(args.sha, version)).lower()
                output.write(f"version={version}\nsha={args.sha}\ncomplete={complete}\n")
    elif args.command == "publish":
        publish(args.sha, args.run_id)
    elif args.command == "provenance":
        provenance(args.directory)
    else:
        sync_dev()


if __name__ == "__main__":
    main()
