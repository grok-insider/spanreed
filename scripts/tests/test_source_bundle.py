import base64
import copy
import json
from pathlib import Path
import subprocess
import tarfile
import tempfile
import unittest
from unittest.mock import patch

from source_fixture import SHA, fixtures, identity, source, write_source


class SourceBundleTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.archive = self.root / "source.tar.gz"

    def read(self):
        with tarfile.open(self.archive, "r:gz") as archive:
            return {member.name: (archive.extractfile(member).read(), member.mode) for member in archive}

    def replace(self, entries, resign=False):
        if resign:
            record = json.loads(entries["source-bundle.json"][0])
            record["files"] = {name: {"sha256": source.sha256(data), "mode": mode}
                               for name, (data, mode) in entries.items() if name != "source-bundle.json"}
            entries["source-bundle.json"] = (source.encoded(record), 0o644)
        self.archive.unlink()
        source.write_archive(self.archive, entries)

    def test_deterministic_archive_verifies_complete_sources_and_notices(self):
        write_source(self.archive)
        second = self.root / "different-name.tar.gz"
        write_source(second)
        self.assertEqual(self.archive.read_bytes(), second.read_bytes())
        record = source.verify(self.archive, "0.3.1", SHA)
        self.assertEqual(record["sources"]["spanreed"]["head"], SHA)
        entries = self.read()
        self.assertIn(b"portable/LICENSES/AGPL-3.0-or-later.txt", entries["NOTICE.txt"][0])
        self.assertIn(b"spanreed/vendor/fabrials-libs/LICENSE", entries["NOTICE.txt"][0])
        self.assertFalse(any(".git/" in name for name in entries))

    def test_rehashed_tampering_still_fails_original_git_tree(self):
        write_source(self.archive)
        entries = self.read()
        entries["portable/apps/web/src/main.ts"] = (b"changed", 0o644)
        self.replace(entries, resign=True)
        with self.assertRaisesRegex(ValueError, "Git commit"):
            source.verify(self.archive, "0.3.1", SHA)

    def test_existing_outputs_and_symlink_sidecars_are_never_overwritten(self):
        sidecar = self.archive.with_name(self.archive.name + ".sha256")
        sidecar.write_text("keep checksum")
        with self.assertRaises(FileExistsError):
            write_source(self.archive)
        self.assertFalse(self.archive.exists())
        self.assertEqual(sidecar.read_text(), "keep checksum")
        sidecar.unlink()
        target = self.root / "unrelated.txt"
        target.write_text("keep unrelated")
        try:
            sidecar.symlink_to(target)
        except OSError:
            self.skipTest("Symlinks unavailable for this test account")
        with self.assertRaises(FileExistsError):
            write_source(self.archive)
        self.assertFalse(self.archive.exists())
        self.assertTrue(sidecar.is_symlink())
        self.assertEqual(target.read_text(), "keep unrelated")
        sidecar.unlink()
        self.archive.write_bytes(b"keep archive")
        with self.assertRaises(FileExistsError):
            write_source(self.archive)
        self.assertEqual(self.archive.read_bytes(), b"keep archive")
        self.assertFalse(sidecar.exists())

    def test_failed_verification_removes_only_new_outputs(self):
        with patch.object(source, "verify", side_effect=ValueError("failed verification")):
            with self.assertRaisesRegex(ValueError, "failed verification"):
                write_source(self.archive)
        self.assertFalse(self.archive.exists())
        self.assertFalse(self.archive.with_name(self.archive.name + ".sha256").exists())

    def test_mode_or_inventory_tampering_is_rejected(self):
        write_source(self.archive)
        entries = self.read()
        data, _ = entries["portable/site/install.sh"]
        entries["portable/site/install.sh"] = (data, 0o755)
        self.replace(entries)
        with self.assertRaisesRegex(ValueError, "inventory"):
            source.verify(self.archive, "0.3.1", SHA)

    def test_dirty_bundle_and_vendor_revision_drift_are_rejected(self):
        for mutation, message in (("dirty", "committed"), ("revision", "vendor revision"), ("hash", "build inputs")):
            with self.subTest(mutation=mutation):
                spanreed, portable, identities = fixtures()
                manifest = json.loads(spanreed["assets/remote-web.source.json"][0])
                if mutation == "dirty":
                    manifest["sources"]["portable"]["dirty"] = True
                elif mutation == "revision":
                    manifest["sources"]["noise"]["head"] = "b" * 40
                else:
                    files = manifest["sources"]["portable"]["files"]
                    files["apps/web/src/main.ts"] = "a" * 64
                    manifest["sources"]["portable"]["treeSha256"] = source.sha256(source.encoded(files))
                spanreed["assets/remote-web.source.json"] = (source.encoded(manifest), 0o644)
                identities["spanreed"] = identity(spanreed)
                with self.assertRaisesRegex(ValueError, message):
                    source.corresponding(spanreed, portable, identities, "0.3.1", identities["spanreed"]["head"])

    def test_portable_revision_and_missing_license_fail_closed(self):
        spanreed, portable, identities = fixtures()
        changed = copy.deepcopy(identities)
        changed["portable"]["head"] = "b" * 40
        with self.assertRaisesRegex(ValueError, "Portable revision"):
            source.corresponding(spanreed, portable, changed, "0.3.1", SHA)
        del portable["LICENSE"]
        with self.assertRaisesRegex(ValueError, "license"):
            source.corresponding(spanreed, portable, identities, "0.3.1", SHA)

    def test_traversal_link_and_duplicate_archives_are_rejected(self):
        for kind in ("traversal", "link", "duplicate"):
            with self.subTest(kind=kind):
                with tarfile.open(self.archive, "w:gz") as archive:
                    info = tarfile.TarInfo("../outside" if kind == "traversal" else "file")
                    if kind == "link":
                        info.type = tarfile.SYMTYPE
                        info.linkname = "/outside"
                    archive.addfile(info)
                    if kind == "duplicate":
                        archive.addfile(info)
                with self.assertRaisesRegex(ValueError, "Unsafe"):
                    source.verify(self.archive, "0.3.1", SHA)

    def test_wrong_release_commit_and_oversize_archive_are_rejected(self):
        write_source(self.archive)
        with self.assertRaisesRegex(ValueError, "manifest"):
            source.verify(self.archive, "0.3.1", "b" * 40)
        with patch.object(source, "LIMIT", 1):
            with self.assertRaisesRegex(ValueError, "oversized"):
                source.verify(self.archive, "0.3.1", SHA)

    def test_missing_build_input_or_vendor_tree_drift_cannot_be_resigned(self):
        for kind in ("vendor", "portable"):
            with self.subTest(kind=kind):
                spanreed, portable, identities = fixtures()
                if kind == "vendor":
                    spanreed["vendor/fabrials-libs/crates/fabrials-remote-noise/src/lib.rs"] = (b"changed", 0o644)
                else:
                    del portable["apps/web/src/main.ts"]
                with self.assertRaisesRegex(ValueError, "vendor revision|build inputs"):
                    source.corresponding(spanreed, portable, identities, "0.3.1", SHA)

    def test_snapshot_reconstructs_real_git_trees_and_rejects_dirty_source(self):
        repo = self.root / "repo"
        repo.mkdir()
        subprocess.run(["git", "init", "-q", str(repo)], check=True)
        for name in ("a.b", "a/file", "LICENSE"):
            path = repo / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(name)
        (repo / "a.b").chmod(0o755)
        subprocess.run(["git", "-C", str(repo), "add", "."], check=True)
        subprocess.run(["git", "-C", str(repo), "-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid",
                        "-c", "commit.gpgsign=false", "commit", "-qm", "Synthetic test fixture"], check=True)
        entries, recorded = source.snapshot(repo)
        actual = source.git(repo, "rev-parse", "HEAD^{tree}").decode().strip()
        self.assertEqual(source.git_tree(entries), actual)
        self.assertEqual(entries["a.b"][1], 0o755)
        self.assertEqual(source.object_hash("commit", base64.b64decode(recorded["commit"])).hex(), recorded["head"])
        (repo / "untracked.txt").write_text("new source")
        with self.assertRaisesRegex(ValueError, "clean committed"):
            source.snapshot(repo)

    def test_pwa_proof_binds_every_shell_byte_and_original_source_identities(self):
        write_source(self.archive)
        spanreed, portable, identities = fixtures()
        dist = self.root / "dist"
        for name, (data, _) in source.under(spanreed, "assets/remote-web/").items():
            path = dist / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)
        proof = source.pwa_proof(self.archive, dist, "0.3.1", SHA)
        self.assertEqual(proof["shellId"], "a" * 64)
        self.assertEqual(proof["sources"]["portable"]["commit"], identities["portable"]["head"])
        self.assertEqual(proof["sources"]["portable"]["sha256"], source.input_digest(portable))
        self.assertIn("Synthetic AGPL fixture", proof["licenseText"])
        for name in ("site/install.sh", "site/install.ps1", "deploy/remote/build-output.json",
                     "deploy/remote/qualification-build-output.json"):
            self.assertEqual(proof["distributionFiles"][name], source.sha256(portable[name][0]))
        self.assertEqual(proof["noticesText"], "Synthetic dependency notices\n")
        (dist / "sw.js").write_text("different worker with same HTML")
        with self.assertRaisesRegex(ValueError, "shell differs"):
            source.pwa_proof(self.archive, dist, "0.3.1", SHA)


if __name__ == "__main__":
    unittest.main()
