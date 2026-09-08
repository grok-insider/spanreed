import contextlib
import importlib.util
import json
import os
from pathlib import Path
import shutil
import tempfile
import unittest
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location("release", Path(__file__).parents[1] / "release.py")
release = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(release)
ROOT = Path(__file__).resolve().parents[2]
SHA = "a" * 40


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


def artifacts(root, version="0.3.1", sha=SHA):
    import hashlib
    names = release.expected_assets(version)[:4]
    for index, name in enumerate(names):
        directory = root / str(index)
        directory.mkdir()
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
                    if entry["name"] in ("spanreed", "spanreed-desktop"):
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
                patch.object(release.time, "sleep") as sleep, patch.object(release, "run") as run:
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
        with patch.object(release, "api", return_value=pr):
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


class ArtifactTests(unittest.TestCase):
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
            self.assertEqual(len(release.verify_assets(root, "0.3.1", SHA)), 8)
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
                            for prefix in ("cli / cli (", "desktop / desktop (") for os in ("linux)", "windows)")]}
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
                    self.assertEqual(len(existing), 8)
                    remote["draft"] = False; mutations.append("publish")
                else:
                    self.fail(f"Unexpected call: {args}")
                return ""
            with patch.object(release, "validate_source", return_value="0.3.1"), \
                    patch.object(release, "api", side_effect=api), patch.object(release, "run", side_effect=run):
                release.publish(SHA, "123")
                self.assertEqual(mutations.count("upload"), 7)
                self.assertEqual(mutations[-1], "publish")
                mutations.clear()
                release.publish(SHA, "123")
                self.assertEqual(mutations, [])


if __name__ == "__main__":
    unittest.main()
