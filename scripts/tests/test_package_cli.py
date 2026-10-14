import argparse
import importlib.util
import io
import json
from pathlib import Path
import tarfile
import tempfile
import unittest
from unittest.mock import patch
import zipfile

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location('package_cli', ROOT / 'scripts/package-cli.py')
package = importlib.util.module_from_spec(spec)
spec.loader.exec_module(package)


class NativePackageTests(unittest.TestCase):
    def fixture(self, root, target):
        (root / 'Cargo.toml').write_text('[package]\nversion="1.2.3"\n')
        (root / 'Cargo.lock').write_text('version=4\n')
        (root / 'LICENSE').write_bytes(b'Original application license\n')
        bundle = root / 'bundle'
        bundle.mkdir()
        notice = b'Spanreed native third-party notices\n\nCopyright Example\r\n'
        (bundle / 'THIRD-PARTY-NOTICES.txt').write_bytes(notice)
        inventory = {'schema': 1, 'package': 'spanreed', 'target': target, 'features': package.STABLE_TARGETS[target], 'rustc': 'rustc fixture'}
        (bundle / 'inventory.json').write_text(json.dumps(inventory))
        sources = bundle / 'crate-sources'
        sources.mkdir()
        (sources / 'fixture-1.0.0.crate').write_bytes(b'exact original registry source')
        binary = root / 'binary'
        binary.write_bytes(b'ELF-or-PE\0' + (root / 'LICENSE').read_bytes() + notice)
        args = argparse.Namespace(manifest=str(root / 'Cargo.toml'), target=target, binary=str(binary), notices=str(bundle), output=str(root / 'dist'))
        return args, inventory

    def test_linux_and_windows_roundtrip_preserves_sources_original_bytes_and_embedded_notices(self):
        for target in package.STABLE_TARGETS:
            with self.subTest(target=target), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                args, inventory = self.fixture(root, target)
                with patch.object(package.notices, 'verify', return_value=inventory) as validate, patch.object(package.subprocess, 'check_output', return_value='rustc fixture\n'):
                    package.build(args)
                    archive = next(path for path in (root / 'dist').iterdir() if not path.name.endswith('.sha256'))
                    descriptor = package.verify_archive(archive, target, '1.2.3', root / 'Cargo.toml')
                    self.assertIn('notices/crate-sources/fixture-1.0.0.crate', descriptor['files'])
                    self.assertIn('notices/THIRD-PARTY-NOTICES.txt', descriptor['files'])
                    self.assertGreaterEqual(validate.call_count, 4)
                    self.assertEqual(archive.with_name(archive.name + '.sha256').read_text().split(), [package.digest(archive.read_bytes()), archive.name])

    def test_missing_embedded_notices_and_experimental_targets_refuse_distribution(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            args, inventory = self.fixture(root, 'x86_64-pc-windows-msvc')
            Path(args.binary).write_bytes(b'Executable without original notices')
            with patch.object(package.notices, 'verify', return_value=inventory), patch.object(package.subprocess, 'check_output', return_value='rustc fixture\n'):
                with self.assertRaisesRegex(ValueError, 'does not retain'):
                    package.build(args)
            self.assertFalse((root / 'dist').exists())
            args.target = 'aarch64-apple-darwin'
            with self.assertRaisesRegex(ValueError, 'experimental'):
                package.build(args)

    def test_wrong_target_features_refuse_distribution(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            args, inventory = self.fixture(root, 'x86_64-pc-windows-msvc')
            inventory['features'] = []
            (root / 'bundle/inventory.json').write_text(json.dumps(inventory))
            with patch.object(package.notices, 'verify', return_value=inventory), patch.object(package.subprocess, 'check_output', return_value='rustc fixture\n'):
                with self.assertRaisesRegex(ValueError, 'target or features'):
                    package.build(args)
            self.assertFalse((root / 'dist').exists())

    def test_other_toolchain_is_not_packaged(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            args, inventory = self.fixture(root, 'x86_64-unknown-linux-musl')
            with patch.object(package.notices, 'verify', return_value=inventory), patch.object(package.subprocess, 'check_output', return_value='another toolchain\n'):
                with self.assertRaisesRegex(ValueError, 'another toolchain'):
                    package.build(args)

    def test_archive_traversal_symlinks_and_duplicates_are_rejected_before_extraction(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for name, kind in [('../outside', tarfile.REGTYPE), ('link', tarfile.SYMTYPE)]:
                archive = root / 'unsafe.tar.gz'
                with tarfile.open(archive, 'w:gz') as stream:
                    member = tarfile.TarInfo(name)
                    member.type = kind
                    member.linkname = '../outside' if kind == tarfile.SYMTYPE else ''
                    member.size = 0
                    stream.addfile(member, io.BytesIO())
                with self.assertRaisesRegex(ValueError, 'Unsafe|Nonregular'):
                    package.verify_archive(archive, 'x86_64-unknown-linux-musl', '1.2.3', root / 'Cargo.toml')
            archive = root / 'duplicate.tar.gz'
            with tarfile.open(archive, 'w:gz') as stream:
                for _ in range(2):
                    member = tarfile.TarInfo('same-name')
                    member.size = 0
                    stream.addfile(member, io.BytesIO())
            with self.assertRaisesRegex(ValueError, 'duplicate'):
                package.verify_archive(archive, 'x86_64-unknown-linux-musl', '1.2.3', root / 'Cargo.toml')
            archive = root / 'unsafe.zip'
            with zipfile.ZipFile(archive, 'w') as stream:
                member = zipfile.ZipInfo('link')
                member.external_attr = 0o120777 << 16
                stream.writestr(member, '../outside')
            with self.assertRaisesRegex(ValueError, 'Nonregular'):
                package.verify_archive(archive, 'x86_64-pc-windows-msvc', '1.2.3', root / 'Cargo.toml')


if __name__ == '__main__':
    unittest.main()
