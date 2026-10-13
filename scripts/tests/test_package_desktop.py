import argparse
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location('package_desktop', ROOT / 'scripts/package-desktop.py')
package = importlib.util.module_from_spec(spec)
spec.loader.exec_module(package)


class DesktopNoticesTests(unittest.TestCase):
    def fixture(self, root):
        desktop = root / 'desktop'
        (desktop / 'dist').mkdir(parents=True)
        (desktop / 'src-tauri').mkdir()
        (root / 'LICENSE').write_bytes(b'Original application license\r\n')
        lock = b'Locked frontend dependencies'
        (desktop / 'bun.lock').write_bytes(lock)
        original = b'Original third-party author\r\n'
        (desktop / 'dist/FRONTEND-NOTICES.txt').write_bytes(original)
        sha = package.native.sha(original)
        inventory = {'schema': 1, 'package': 'spanreed-desktop', 'lockSha256': package.native.sha(lock), 'noticesSha256': sha, 'packages': [{'name': 'fixture', 'version': '1.0.0', 'files': [{'path': 'LICENSE', 'sha256': sha}]}], 'sections': [{'package': 'fixture@1.0.0', 'path': 'LICENSE', 'offset': 0, 'bytes': len(original), 'sha256': sha}]}
        (desktop / 'dist/frontend-notices.json').write_text(json.dumps(inventory))
        native = root / 'notices'
        native.mkdir()
        (native / 'THIRD-PARTY-NOTICES.txt').write_bytes(b'Original Rust license\r\n')
        (native / 'fixture.crate').write_bytes(b'Original registry source')
        return desktop, native, inventory

    def test_staging_keeps_sources_and_binary_requires_every_original(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            desktop, native, _ = self.fixture(root)
            binary = root / 'binary'
            binary.write_bytes((root / 'LICENSE').read_bytes() + (native / 'THIRD-PARTY-NOTICES.txt').read_bytes() + (desktop / 'dist/FRONTEND-NOTICES.txt').read_bytes())
            args = argparse.Namespace(desktop=str(desktop), notices=str(native), binary=str(binary))
            with patch.object(package.native, 'verify', return_value={'package': 'spanreed-desktop', 'features': ['tauri/custom-protocol']}):
                package.stage(args)
                self.assertEqual((desktop / 'distribution-notices/native/fixture.crate').read_bytes(), b'Original registry source')
                package.verify(args)
                binary.write_bytes(b'Unqualified binary')
                with self.assertRaisesRegex(ValueError, 'preserve original'):
                    package.verify(args)
                with self.assertRaisesRegex(ValueError, 'replace staged'):
                    package.stage(args)

    def test_inventory_ranges_and_original_bytes_cannot_be_changed(self):
        with tempfile.TemporaryDirectory() as directory:
            desktop, _, inventory = self.fixture(Path(directory))
            self.assertEqual(package.frontend(desktop), b'Original third-party author\r\n')
            inventory['sections'][0]['bytes'] -= 1
            (desktop / 'dist/frontend-notices.json').write_text(json.dumps(inventory))
            with self.assertRaisesRegex(ValueError, 'text differs'):
                package.frontend(desktop)
            (desktop / 'dist/FRONTEND-NOTICES.txt').write_bytes(b'Changed original')
            with self.assertRaisesRegex(ValueError, 'checksum differs'):
                package.frontend(desktop)


if __name__ == '__main__':
    unittest.main()
