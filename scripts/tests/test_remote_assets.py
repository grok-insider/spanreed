import copy
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest


SPEC = importlib.util.spec_from_file_location("remote_assets", Path(__file__).parents[1] / "remote-assets.py")
assets = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(assets)


class RemoteAssetsTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        for name, data in {
            "index.html": b"<!doctype html>",
            "manifest.webmanifest": b'{"start_url":"/","scope":"/"}',
            "sw.js": b"const shell=[];",
            "crypto/aaaaaaaaaaaaaaaaaaaa/fabrials_remote_noise.js": b"export default function(){}",
            "crypto/aaaaaaaaaaaaaaaaaaaa/fabrials_remote_noise_bg.wasm": b"\0asm\x01\0\0\0",
        }.items():
            path = self.root / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)
        source = {"head": "a" * 40, "dirty": False, "files": {"package.json": assets.digest(b"{}")}}
        source["treeSha256"] = assets.digest(assets.encoded(source["files"]))
        self.manifest = {"schema": 1, "sources": {"portable": source, "noise": copy.deepcopy(source)}}
        self.resign()

    def resign(self):
        self.manifest["files"] = assets.files(self.root)
        self.manifest["bundleSha256"] = assets.digest(assets.encoded(self.manifest["files"]))

    def check(self, release=False):
        return assets.validate_bundle(self.root, self.manifest, release)

    def test_complete_versioned_bundle_is_verified_without_source_checkout(self):
        self.assertEqual(self.check(release=True), 5)

    def test_changed_or_extra_assets_are_rejected(self):
        (self.root / "sw.js").write_text("altered")
        with self.assertRaisesRegex(ValueError, "differs"):
            self.check()
        self.resign()
        (self.root / "unknown.js").write_text("unexpected")
        with self.assertRaisesRegex(ValueError, "differs"):
            self.check()

    def test_missing_pair_or_ambiguous_wasm_is_rejected(self):
        path = self.root / "crypto/aaaaaaaaaaaaaaaaaaaa/fabrials_remote_noise.js"
        path.unlink()
        self.resign()
        with self.assertRaisesRegex(ValueError, "pair"):
            self.check()
        path.write_text("module")
        second = self.root / "crypto/bbbbbbbbbbbbbbbbbbbb/fabrials_remote_noise_bg.wasm"
        second.parent.mkdir()
        second.write_bytes(b"\0asm\x01\0\0\0")
        self.resign()
        with self.assertRaisesRegex(ValueError, "ambiguous"):
            self.check()

    def test_placeholder_wasm_is_rejected_even_with_matching_hash(self):
        (self.root / "crypto/aaaaaaaaaaaaaaaaaaaa/fabrials_remote_noise_bg.wasm").write_text("placeholder")
        self.resign()
        with self.assertRaisesRegex(ValueError, "WASM"):
            self.check()

    def test_release_rejects_uncommitted_source_without_mislabeling_local_build(self):
        self.manifest["sources"]["portable"]["dirty"] = True
        self.assertEqual(self.check(), 5)
        with self.assertRaisesRegex(ValueError, "committed"):
            self.check(release=True)

    def test_source_digest_mismatch_is_rejected(self):
        self.manifest["sources"]["noise"]["files"]["source.rs"] = "b" * 64
        with self.assertRaisesRegex(ValueError, "source identity"):
            self.check()

    def test_incomplete_shell_and_cross_origin_scope_are_rejected(self):
        (self.root / "index.html").unlink()
        self.resign()
        with self.assertRaisesRegex(ValueError, "Incomplete"):
            self.check()
        (self.root / "index.html").write_text("index")
        (self.root / "manifest.webmanifest").write_text(json.dumps({"start_url": "https://other.test", "scope": "/"}))
        self.resign()
        with self.assertRaisesRegex(ValueError, "same origin"):
            self.check()

    def test_symlink_cannot_smuggle_untracked_asset_into_bundle(self):
        try:
            (self.root / "redirect.js").symlink_to(self.root / "sw.js")
        except OSError:
            self.skipTest("This account cannot create symlinks")
        with self.assertRaisesRegex(ValueError, "symlinks"):
            self.check()
