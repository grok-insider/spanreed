import hashlib
import io
import json
import os
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]
ASSET = "spanreed-9.9.9-x86_64-unknown-linux-musl.tar.gz"


@unittest.skipUnless(shutil.which("bwrap"), "Linux bubblewrap isolation required")
class InstallerIntegrityTests(unittest.TestCase):
    def setUp(self):
        self.scratch = tempfile.TemporaryDirectory(prefix="spanreed-installer-qa-")
        self.directory = Path(self.scratch.name)
        self.fixture = self.directory / "fixture"
        self.private_home = self.directory / "profile"
        self.fixture.mkdir()
        self.private_home.mkdir()
        self.destination = self.private_home / ".local/bin/spanreed"
        self.destination.parent.mkdir(parents=True)
        self.destination.write_bytes(b"previous-binary")
        shutil.copyfile(ROOT / "scripts/install.sh", self.fixture / "install.sh")
        binary = b'#!/bin/sh\nprintf "setup-completed" > "$HOME/setup-ran"\n'
        with tarfile.open(self.fixture / ASSET, "w:gz") as archive:
            entry = tarfile.TarInfo("spanreed")
            entry.size = len(binary)
            entry.mode = 0o755
            archive.addfile(entry, io.BytesIO(binary))
        self.checksum = self.fixture / (ASSET + ".sha256")
        self.checksum.write_text(hashlib.sha256((self.fixture / ASSET).read_bytes()).hexdigest() + "  " + ASSET + "\n")
        (self.fixture / "release.json").write_text(json.dumps({"tag_name": "v9.9.9"}))
        self.stubs = self.fixture / "stubs"
        self.stubs.mkdir()
        (self.stubs / "curl").write_text("""#!/usr/bin/env python3
import pathlib, shutil, sys
args = sys.argv[1:]
url = next((value for value in args if value.startswith('https://')), '')
root = pathlib.Path('/tmp/installer-fixture')
if url == 'https://api.github.com/repos/grok-insider/spanreed/releases/latest':
    source = root / 'release.json'
elif url.startswith('https://github.com/grok-insider/spanreed/releases/download/v9.9.9/'):
    source = root / url.rsplit('/', 1)[-1]
else:
    sys.exit(22)
if not source.is_file():
    sys.exit(22)
if '-o' in args:
    shutil.copyfile(source, args[args.index('-o') + 1])
else:
    sys.stdout.buffer.write(source.read_bytes())
""")
        (self.stubs / "curl").chmod(0o755)
        (self.stubs / "uname").write_text('#!/bin/sh\ncase "$1" in -s) echo Linux ;; -m) echo x86_64 ;; esac\n')
        (self.stubs / "uname").chmod(0o755)

    def tearDown(self):
        self.scratch.cleanup()

    def run_installer(self, args=None):
        environment = dict(os.environ)
        for name in ("SPANREED_TAG", "SPANREED_REPO"):
            environment.pop(name, None)
        environment["PATH"] = "/tmp/installer-fixture/stubs:" + os.environ["PATH"]
        return subprocess.run([
            "bwrap", "--die-with-parent", "--unshare-net", "--ro-bind", "/", "/",
            "--dev", "/dev", "--proc", "/proc", "--tmpfs", "/tmp",
            "--ro-bind", str(self.fixture), "/tmp/installer-fixture",
            "--bind", str(self.private_home), str(Path.home()),
            "--chdir", "/tmp", "/bin/sh", "/tmp/installer-fixture/install.sh", *(args or ["--yes"]),
        ], env=environment, capture_output=True, text=True, timeout=20)

    def assert_rejected(self, message):
        result = self.run_installer()
        self.assertNotEqual(result.returncode, 0, result.stdout)
        self.assertIn(message, result.stderr)
        self.assertEqual(self.destination.read_bytes(), b"previous-binary")
        self.assertFalse((self.private_home / "setup-ran").exists())

    def test_matching_archive_runs_setup_only_after_verification(self):
        result = self.run_installer()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual((self.private_home / "setup-ran").read_text(), "setup-completed")
        self.assertTrue(self.destination.read_bytes().startswith(b"#!/bin/sh"))

    def test_mismatched_checksum_preserves_existing_binary(self):
        self.checksum.write_text("0" * 64 + "  " + ASSET + "\n")
        self.assert_rejected("checksum mismatch")

    def test_missing_checksum_preserves_existing_binary(self):
        self.checksum.unlink()
        result = self.run_installer()
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(self.destination.read_bytes(), b"previous-binary")
        self.assertFalse((self.private_home / "setup-ran").exists())

    def test_wrong_checksum_filename_is_rejected(self):
        self.checksum.write_text(self.checksum.read_text().replace(ASSET, "other.tar.gz"))
        self.assert_rejected("checksum file does not describe")

    def test_ambiguous_checksum_manifest_is_rejected(self):
        self.checksum.write_text(self.checksum.read_text() * 2)
        self.assert_rejected("checksum file does not describe")

    def test_verified_symlink_payload_is_rejected(self):
        with tarfile.open(self.fixture / ASSET, "w:gz") as archive:
            entry = tarfile.TarInfo("spanreed")
            entry.type = tarfile.SYMTYPE
            entry.linkname = "/bin/sh"
            archive.addfile(entry)
        self.checksum.write_text(hashlib.sha256((self.fixture / ASSET).read_bytes()).hexdigest() + "  " + ASSET + "\n")
        self.assert_rejected("regular spanreed binary")

    def test_partial_copy_failure_preserves_existing_binary(self):
        (self.stubs / "cp").write_text('#!/bin/sh\nprintf "partial" > "$2"\necho "synthetic disk full" >&2\nexit 1\n')
        (self.stubs / "cp").chmod(0o755)
        self.assert_rejected("synthetic disk full")
        self.assertEqual(list(self.destination.parent.glob(".spanreed-install.*")), [])

    def test_latest_requires_a_published_stable_tag(self):
        for release in [
            {},
            {"tag_name": "v9.9.9-preview.1"},
            {"tag_name": "v9.9.9", "draft": True},
            {"tag_name": "v9.9.9", "prerelease": True},
            {"tag_name": "../../other"},
            {"tag_name": "9.9.9"},
            {"tag_name": "v09.9.9"},
            {"tag_name": "V9.9.9"},
        ]:
            with self.subTest(release=release):
                (self.fixture / "release.json").write_text(json.dumps(release))
                self.assert_rejected("stable")

    def test_unavailable_release_api_preserves_existing_binary(self):
        (self.fixture / "release.json").unlink()
        self.assert_rejected("could not resolve latest stable release")

    def test_unpublished_linux_architecture_is_refused(self):
        (self.stubs / "uname").write_text('#!/bin/sh\ncase "$1" in -s) echo Linux ;; -m) echo aarch64 ;; esac\n')
        self.assert_rejected("unsupported platform")

    def test_local_binary_remains_installable_on_linux_arm(self):
        (self.stubs / "uname").write_text('#!/bin/sh\ncase "$1" in -s) echo Linux ;; -m) echo aarch64 ;; esac\n')
        (self.fixture / 'local-binary').write_bytes(b'#!/bin/sh\nprintf "setup-completed" > "$HOME/setup-ran"\n')
        result = self.run_installer(['--from-path', '/tmp/installer-fixture/local-binary', '--yes'])
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual((self.private_home / 'setup-ran').read_text(), 'setup-completed')

    def test_directory_destination_is_unchanged(self):
        self.destination.unlink()
        self.destination.mkdir()
        marker = self.destination / "unrelated"
        marker.write_text("preserved")
        result = self.run_installer()
        self.assertNotEqual(result.returncode, 0, result.stdout)
        self.assertIn("install destination must be a regular file", result.stderr)
        self.assertEqual(list(self.destination.iterdir()), [marker])
        self.assertEqual(marker.read_text(), "preserved")
        self.assertFalse((self.private_home / "setup-ran").exists())

    def test_symlink_destinations_are_unchanged(self):
        self.destination.unlink()
        for directory in [False, True]:
            with self.subTest(directory=directory):
                target = self.destination.parent / ("other-directory" if directory else "other-file")
                if directory:
                    target.mkdir()
                else:
                    target.write_text("preserved")
                self.destination.symlink_to(target.name)
                result = self.run_installer()
                self.assertNotEqual(result.returncode, 0, result.stdout)
                self.assertIn("install destination must be a regular file", result.stderr)
                self.assertTrue(self.destination.is_symlink())
                if directory:
                    self.assertEqual(list(target.iterdir()), [])
                else:
                    self.assertEqual(target.read_text(), "preserved")
                self.destination.unlink()
                self.assertFalse((self.private_home / "setup-ran").exists())

    def test_destination_is_rechecked_after_staging(self):
        (self.stubs / "cp").write_text('''#!/usr/bin/env python3
import os, pathlib, shutil, sys
shutil.copyfile(sys.argv[1], sys.argv[2])
profile = pathlib.Path(os.environ['HOME'])
destination = profile / '.local/bin/spanreed'
destination.rename(profile / 'saved-previous')
destination.mkdir()
''')
        (self.stubs / "cp").chmod(0o755)
        result = self.run_installer()
        self.assertNotEqual(result.returncode, 0, result.stdout)
        self.assertIn("install destination must be a regular file", result.stderr)
        self.assertEqual(list(self.destination.iterdir()), [])
        self.assertEqual((self.private_home / "saved-previous").read_bytes(), b"previous-binary")
        self.assertEqual(list(self.destination.parent.glob(".spanreed-install.*")), [])
        self.assertFalse((self.private_home / "setup-ran").exists())


if __name__ == "__main__":
    unittest.main()
