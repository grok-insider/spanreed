import contextlib
import importlib.util
import json
import os
from pathlib import Path
import shutil
import tempfile
import tarfile
import unittest
from unittest.mock import patch
from source_fixture import SHA, fixtures, write_source

SPEC = importlib.util.spec_from_file_location("release", Path(__file__).parents[1] / "release.py")
release = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(release)
ROOT = Path(__file__).resolve().parents[2]
BUILDER_JOBS = [("cli / cli (", "ubuntu-24.04, x86_64-unknown-linux-musl)"),
                ("cli / cli (", "windows-latest, x86_64-pc-windows-msvc)"),
                ("cli / cli (", "macos-latest, aarch64-apple-darwin)"),
                ("cli / cli (", "macos-latest, x86_64-apple-darwin)"),
                ("cli / macos-universal", ""),
                ("desktop / desktop (", "linux)"), ("desktop / desktop (", "windows)"), ("source", "")]
PRODUCTION_INDEX = '<meta name="grok-app-origin" content="https://desktop.grok.me">'


@contextlib.contextmanager
def checkout():
    previous = Path.cwd()
    with tempfile.TemporaryDirectory() as directory:
        root = Path(directory)
        for name in ("Cargo.toml", "Cargo.lock", "CHANGELOG.md", "desktop/package.json",
                     "desktop/src-tauri/Cargo.toml", "desktop/src-tauri/Cargo.lock",
                     "desktop/src-tauri/tauri.conf.json", "scripts/sync-desktop-version.py"):
            (root / name).parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(ROOT / name, root / name)
        os.chdir(root)
        try:
            yield root
        finally:
            os.chdir(previous)


def artifacts(root, version="0.3.1", sha=SHA, origin="https://desktop.grok.me"):
    import hashlib
    names = release.primary_assets(version)
    for index, name in enumerate(names):
        directory = root / str(index)
        directory.mkdir()
        if name == release.source_asset(version):
            write_source(directory / name, version, origin)
        else:
            (directory / name).write_bytes(name.encode())
            (directory / (name + ".sha256")).write_text(hashlib.sha256(name.encode()).hexdigest() + "  " + name + "\n")
        (directory / "release-provenance.json").write_text(json.dumps({
            "sha": sha, "version": version, "assets": [name, name + ".sha256"],
        }))


class VersionTests(unittest.TestCase):
    def test_version_milestones(self):
        self.assertEqual(release.next_version("0.3.0", "patch"), "0.3.1")
        self.assertEqual(release.next_version("0.3.9", "minor"), "0.4.0")
        self.assertEqual(release.next_version("0.3.9", "major"), "1.0.0")
        with self.assertRaises(ValueError):
            release.next_version("../../other", "patch")

    def test_versions_change_only_local_lock_entries_and_are_idempotent(self):
        with checkout():
            before = {name: release.tomllib.loads(Path(name).read_text())["package"]
                      for name in ("Cargo.lock", "desktop/src-tauri/Cargo.lock")}
            release.bump_files("0.3.1")
            self.assertEqual(release.check_versions(), "0.3.1")
            for name, entries in before.items():
                after = release.tomllib.loads(Path(name).read_text())["package"]
                for entry in entries:
                    if entry["name"] in release.LOCAL_PACKAGES + release.DESKTOP_PACKAGES:
                        entry["version"] = "0.3.1"
                self.assertEqual(entries, after)
            first = Path("desktop/package.json").read_bytes()
            release.bump_files("0.3.1")
            self.assertEqual(first, Path("desktop/package.json").read_bytes())
            Path("desktop/package.json").write_text('{"version":"0.0.0"}')
            with self.assertRaises(ValueError):
                release.check_versions()


class BoundaryTests(unittest.TestCase):
    def test_dispatch_waits_for_pushed_head_without_dispatching_stale_code(self):
        def pr(sha):
            return {"state": "open", "head": {"repo": {"full_name": release.REPO},
                    "sha": sha, "ref": "release-plz-manual-v0.4.0"}, "base": {"ref": "master"}}
        with patch.object(release, "api", side_effect=[pr("b" * 40), pr(SHA)]), \
                patch.object(release.time, "sleep") as sleep, patch.object(release, "run") as run, \
                patch.object(release, "git", return_value=PRODUCTION_INDEX):
            release.dispatch(15, SHA)
            sleep.assert_called_once_with(2)
            self.assertEqual(run.call_count, 4)
            for call in run.call_args_list:
                self.assertIn(f"expected_sha={SHA}", call.args)
        with patch.object(release, "api", return_value=pr("b" * 40)) as api, \
                patch.object(release.time, "sleep") as sleep, patch.object(release, "run") as run:
            with self.assertRaisesRegex(ValueError, "changed SHA"):
                release.dispatch(15, SHA)
            self.assertEqual(api.call_count, 6)
            self.assertEqual(sleep.call_count, 5)
            run.assert_not_called()
        closed = pr(SHA)
        closed["state"] = "closed"
        with patch.object(release, "api", return_value=closed), patch.object(release.time, "sleep") as sleep:
            with self.assertRaisesRegex(ValueError, "closed"):
                release.dispatch(15, SHA)
            sleep.assert_not_called()

    def test_changed_external_or_closed_pr_rejected(self):
        pr = {"state": "open", "head": {"repo": {"full_name": release.REPO},
              "sha": SHA, "ref": "release-plz-v0.3.1"}, "base": {"ref": "master"}}
        with patch.object(release, "api", return_value=pr), \
                patch.object(release, "git", return_value=PRODUCTION_INDEX):
            release.validate_pr(1, SHA, True)
            with self.assertRaises(ValueError):
                release.validate_pr(1, "b" * 40)
            pr["head"]["repo"]["full_name"] = "other/repo"
            with self.assertRaises(ValueError):
                release.validate_pr(1, SHA)
            pr["head"]["repo"]["full_name"] = release.REPO
            pr["state"] = "closed"
            with self.assertRaises(ValueError):
                release.validate_pr(1, SHA)

    def test_existing_tag_conflict(self):
        with checkout():
            version = release.check_versions()
            def git(*args):
                if args == ("rev-parse", "HEAD"):
                    return SHA
                if args == ("show", f"{SHA}:assets/remote-web/index.html"):
                    return PRODUCTION_INDEX
                if args[0] == "merge-base":
                    return ""
                if args[0] == "tag":
                    return "v" + version
                return "b" * 40
            prs = [{"merged_at": "now", "merge_commit_sha": SHA, "base": {"ref": "master"},
                    "head": {"ref": "release-plz-v" + version}}]
            with patch.object(release, "git", side_effect=git), patch.object(release, "api", return_value=prs):
                with self.assertRaisesRegex(ValueError, "another commit"):
                    release.validate_source(SHA)


    def test_qualification_origin_blocks_release_preparation_before_writes(self):
        index = '<meta name="grok-app-origin" content="https://grok.fabrials.com">'
        with patch.object(release, "api", return_value={"tag_name": "v0.3.0"}), \
                patch.object(release, "git", side_effect=["", "", index]) as git, \
                patch.object(release, "run") as run:
            with self.assertRaisesRegex(ValueError, "Stable release requires"):
                release.prepare("patch", False, False)
            self.assertEqual(git.call_count, 3)
            run.assert_not_called()

    def test_qualification_origin_blocks_master_pr_validation_and_dispatch(self):
        pr = {"state": "open", "head": {"repo": {"full_name": release.REPO},
              "sha": SHA, "ref": "release-plz-v0.3.1"}, "base": {"ref": "master"}}
        index = '<meta name="grok-app-origin" content="https://grok.fabrials.com">'
        with patch.object(release, "api", return_value=pr), \
                patch.object(release, "git", return_value=index) as git, patch.object(release, "run") as run:
            with self.assertRaisesRegex(ValueError, "Stable release requires"):
                release.dispatch(15, SHA)
            git.assert_called_once_with("show", f"{SHA}:assets/remote-web/index.html")
            run.assert_not_called()

    def test_stable_release_rejects_qualification_origin_before_github_access(self):
        indexes = [
            '<meta name="grok-app-origin" content="https://grok.fabrials.com">',
            '<meta name="grok-app-origin" content="https://desktop.grok.me/">',
            '<meta name="grok-app-origin" content="https://desktop.grok.me.evil.invalid">',
            '<meta name="grok-app-origin" content="https://desktop.grok.me">' * 2,
            '<!doctype html>',
        ]
        for index in indexes:
            with self.subTest(index=index), patch.object(release, "git", side_effect=[SHA, index]), \
                    patch.object(release, "api") as api:
                with self.assertRaisesRegex(ValueError, "Stable release requires"):
                    release.publish(SHA, "123")
                api.assert_not_called()


class ArtifactTests(unittest.TestCase):
    def test_valid_qualification_source_is_not_a_stable_release_artifact(self):
        origin = "https://grok.fabrials.com"
        sha = fixtures(origin=origin)[2]["spanreed"]["head"]
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            artifacts(root, sha=sha, origin=origin)
            archive = next(root.rglob(release.source_asset("0.3.1")))
            record = release.source_bundle.verify(archive, "0.3.1", sha)
            self.assertEqual(record["sha"], sha)
            with self.assertRaisesRegex(ValueError, "Stable release requires"):
                release.verify_assets(root, "0.3.1", sha)

    def test_complete_public_release_short_circuits_without_mutation(self):
        with tempfile.TemporaryDirectory() as directory:
            fixture = Path(directory)
            artifacts(fixture)
            files = release.verify_assets(fixture, "0.3.1", SHA)
            def api(path):
                if path.startswith("git/ref"):
                    return {"object": {"type": "commit", "sha": SHA}}
                return [{"tag_name": "v0.3.1", "draft": False,
                         "assets": [{"name": name} for name in files]}]
            def run(*args):
                self.assertEqual(args[:3], ("gh", "release", "download"))
                out = Path(args[args.index("--dir") + 1])
                for name, source in files.items():
                    shutil.copy2(source, out / name)
                return ""
            with patch.object(release, "api", side_effect=api), patch.object(release, "run", side_effect=run):
                self.assertTrue(release.published_complete(SHA, "0.3.1"))

    def test_exact_complete_set_and_integrity(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            artifacts(root)
            self.assertEqual(len(release.verify_assets(root, "0.3.1", SHA)), len(release.expected_assets("0.3.1")))
            binary = next(root.rglob("*.deb"))
            binary.write_bytes(b"corrupt")
            with self.assertRaisesRegex(ValueError, "Checksum"):
                release.verify_assets(root, "0.3.1", SHA)

    def test_missing_or_wrong_source_not_publishable(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            artifacts(root)
            with self.assertRaisesRegex(ValueError, "provenance"):
                release.verify_assets(root, "0.3.1", "b" * 40)
            next(root.rglob("*.exe")).unlink()
            with self.assertRaisesRegex(ValueError, "exactly one"):
                release.verify_assets(root, "0.3.1", SHA)

    def test_source_archive_cannot_be_omitted_or_rehashed_after_tampering(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            artifacts(root)
            archive_path = next(root.rglob(release.source_asset("0.3.1")))
            with tarfile.open(archive_path, "r:gz") as archive:
                entries = {member.name: (archive.extractfile(member).read(), member.mode) for member in archive}
            archive_path.unlink()
            with self.assertRaisesRegex(ValueError, "exactly one"):
                release.verify_assets(root, "0.3.1", SHA)
            entries["portable/apps/web/src/main.ts"] = (b"tampered", 0o644)
            record = json.loads(entries["source-bundle.json"][0])
            record["files"] = {name: {"sha256": release.source_bundle.sha256(data), "mode": mode}
                               for name, (data, mode) in entries.items() if name != "source-bundle.json"}
            entries["source-bundle.json"] = (release.source_bundle.encoded(record), 0o644)
            release.source_bundle.write_archive(archive_path, entries)
            archive_path.with_name(archive_path.name + ".sha256").write_text(
                release.source_bundle.sha256(archive_path.read_bytes()) + "  " + archive_path.name + "\n")
            with self.assertRaisesRegex(ValueError, "Git commit"):
                release.verify_assets(root, "0.3.1", SHA)

    def test_recovery_requires_successful_source_producer(self):
        for source_jobs in ([], [{"name": "source", "conclusion": "failure"}]):
            with self.subTest(source_jobs=source_jobs):
                def api(path):
                    if '/jobs?' in path:
                        return {"jobs": [{"name": prefix + target, "conclusion": "success"}
                                         for prefix, target in BUILDER_JOBS if prefix != "source"] + source_jobs}
                    return {"head_sha": SHA, "event": "push", "path": ".github/workflows/release.yml", "status": "completed"}
                with patch.object(release, "validate_source", return_value="0.3.1"), \
                        patch.object(release, "api", side_effect=api), patch.object(release, "run") as run, \
                        patch.dict(os.environ, {"GITHUB_RUN_ID": "other"}):
                    with self.assertRaisesRegex(ValueError, "corresponding-source"):
                        release.publish(SHA, "123")
                    run.assert_not_called()

    def test_draft_resume_and_public_noop_never_clobber(self):
        with tempfile.TemporaryDirectory() as directory:
            fixture = Path(directory)
            artifacts(fixture)
            files = release.verify_assets(fixture, "0.3.1", SHA)
            existing = {next(iter(files)): next(iter(files.values())).read_bytes()}
            remote = {"tag_name": "v0.3.1", "draft": True, "assets": [], "target_commitish": SHA}
            mutations = []
            def api(path, **kwargs):
                if '/jobs?' in path:
                    return {"jobs": [{"name": prefix + os, "conclusion": "success"}
                            for prefix, os in BUILDER_JOBS]}
                if path.startswith("actions/runs/"):
                    return {"head_sha": SHA, "event": "push", "path": ".github/workflows/release.yml", "conclusion": "success", "status": "completed"}
                if path.startswith("git/ref"):
                    if remote["draft"]:
                        self.assertTrue(kwargs.get("missing_ok"))
                        return None
                    return {"object": {"type": "commit", "sha": SHA}}
                remote["assets"] = [{"name": name} for name in existing]
                return [remote]
            def run(*args):
                if args[:3] == ("gh", "run", "download"):
                    target = Path(args[args.index("--dir") + 1])
                    shutil.copytree(fixture, target, dirs_exist_ok=True)
                elif args[:3] == ("gh", "release", "download"):
                    target = Path(args[args.index("--dir") + 1]); target.mkdir(exist_ok=True)
                    selected = [args[args.index("--pattern") + 1]] if "--pattern" in args else existing
                    for name in selected:
                        (target / name).write_bytes(existing[name])
                elif args[:3] == ("gh", "release", "upload"):
                    self.assertNotIn("--clobber", args)
                    path = Path(args[-1]); existing[path.name] = path.read_bytes(); mutations.append("upload")
                elif args[:3] == ("gh", "release", "edit"):
                    self.assertEqual(len(existing), len(release.expected_assets("0.3.1")))
                    remote["draft"] = False; mutations.append("publish")
                else:
                    self.fail(f"Unexpected call: {args}")
                return ""
            with patch.object(release, "validate_source", return_value="0.3.1"), \
                    patch.object(release, "api", side_effect=api), patch.object(release, "run", side_effect=run):
                release.publish(SHA, "123")
                self.assertEqual(mutations.count("upload"), len(release.expected_assets("0.3.1")) - 1)
                self.assertEqual(mutations[-1], "publish")
                mutations.clear()
                release.publish(SHA, "123")
                self.assertEqual(mutations, [])


if __name__ == "__main__":
    unittest.main()
