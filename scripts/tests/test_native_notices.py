import importlib.util
import argparse
import io
import json
import subprocess
import sys
from pathlib import Path
import tarfile
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location('native_notices', ROOT / 'scripts/native-notices.py')
notices = importlib.util.module_from_spec(spec)
spec.loader.exec_module(notices)


class NativeNoticesTests(unittest.TestCase):
    def test_cargo_utf8_is_independent_of_windows_locale_and_preserves_original_bytes(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            manifest = root / 'Cargo.toml'
            manifest.write_bytes(b'[package]\nname="fixture"\nversion="1.0.0"\n')
            lock = root / 'Cargo.lock'
            lock.write_bytes(b'version=4\n[[package]]\nname="fixture"\nversion="1.0.0"\n')
            metadata = {'packages': [{'id': 'fixture', 'name': 'fixture', 'version': '1.0.0', 'license': 'MIT', 'source': None, 'authors': ['Łukasz']}], 'resolve': {'nodes': [{'id': 'fixture', 'deps': []}]}}
            metadata_bytes = json.dumps(metadata, ensure_ascii=False).encode('utf-8')
            original = 'Copyright Łukasz\r\nOriginal notice\n'.encode('utf-8')
            archive_path = root / 'rust.tar.gz'
            with tarfile.open(archive_path, 'w:gz') as archive:
                files = {name: original for name in ['LICENSE-MIT', 'LICENSE-APACHE', 'COPYRIGHT', 'LICENSES/Unicode-3.0.txt', 'library/compiler-builtins/LICENSE.txt', 'library/compiler-builtins/libm/LICENSE.txt']}
                files['version'] = b'1.2.3 (abcdef123 2026-01-01)\n'
                for name, data in files.items():
                    member = tarfile.TarInfo('rustc-1.2.3-src/' + name)
                    member.size = len(data)
                    archive.addfile(member, io.BytesIO(data))
            archive_hash = notices.sha(archive_path.read_bytes())
            args = argparse.Namespace(manifest=str(manifest), package='fixture', target='x86_64-pc-windows-msvc', output=str(root / 'output'), features='', cargo_home=str(root), rust_notices=None, rust_source_archive=str(archive_path))
            actual_output = subprocess.check_output
            def emitted(data):
                return [sys.executable, '-c', f'import sys; sys.stdout.buffer.write({data!r})']
            def command_output(command, **options):
                data = metadata_bytes if command[0] == 'cargo' else b'rustc 1.2.3 (abcdef123 2026-01-01)\n'
                return actual_output(emitted(data), **options)
            with patch.object(subprocess, '_text_encoding', return_value='cp1252'):
                with self.assertRaises(UnicodeDecodeError):
                    actual_output(emitted(metadata_bytes), text=True)
                with patch.object(notices.subprocess, 'check_output', side_effect=command_output):
                    notices.generate(args)
                    inventory = notices.verify(args.output, lock)
                    content = (Path(args.output) / 'THIRD-PARTY-NOTICES.txt').read_bytes()
                    for section in inventory['sections']:
                        data = content[section['offset']:section['offset'] + section['bytes']]
                        self.assertEqual(data, original)
                        self.assertEqual(notices.sha(data), section['sha256'])
                    metadata_bytes = b'\xff'
                    with self.assertRaises(UnicodeDecodeError):
                        notices.generate(args)
                    with self.assertRaises(UnicodeDecodeError):
                        notices.verify(args.output, lock)
            self.assertEqual(notices.sha(archive_path.read_bytes()), archive_hash)

    def test_original_rust_source_identity_and_nested_bytes_are_verified(self):
        with tempfile.TemporaryDirectory() as directory:
            archive = Path(directory) / 'rust.tar.gz'
            files = {name: b'Original stdlib notice\r\n' for name in ['LICENSE-MIT', 'LICENSE-APACHE', 'COPYRIGHT', 'LICENSES/Unicode-3.0.txt', 'library/compiler-builtins/LICENSE.txt', 'library/compiler-builtins/libm/LICENSE.txt']}
            files['version'] = b'1.2.3 (abcdef123 2026-01-01)\n'
            with tarfile.open(archive, 'w:gz') as stream:
                for name, value in files.items():
                    member = tarfile.TarInfo('rustc-1.2.3-src/' + name)
                    member.size = len(value)
                    stream.addfile(member, io.BytesIO(value))
            sections, provenance = notices.source_toolchain_notices(archive, 'rustc 1.2.3 (abcdef123 2026-01-01)')
            self.assertEqual(len(sections), 6)
            self.assertTrue(all(value == b'Original stdlib notice\r\n' for _, _, value in sections))
            self.assertTrue(all(p['source'] == 'rustc-source-sha256:' + notices.sha(archive.read_bytes()) for p in provenance))
            with self.assertRaisesRegex(ValueError, 'version differs'):
                notices.source_toolchain_notices(archive, 'rustc 1.2.3 (abcdef124 2026-01-01)')

    def test_offline_inputs_never_fetch_and_reject_changed_originals(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            original = root / 'LICENSE'
            original.write_bytes(b'Original\r\n')
            index = root / 'inputs.json'
            index.write_text(json.dumps({'https://example.com/LICENSE': {'file': str(original), 'sha256': notices.sha(original.read_bytes())}}))
            with patch.dict('os.environ', {'SPANREED_NOTICE_INPUTS': str(index)}), patch.object(notices.urllib.request, 'urlopen') as network:
                self.assertEqual(notices.fetch('https://example.com/LICENSE', 100), b'Original\r\n')
                with self.assertRaises(notices.urllib.error.HTTPError) as failure:
                    notices.fetch('https://example.com/missing', 100)
                failure.exception.close()
                original.write_bytes(b'Changed')
                with self.assertRaisesRegex(ValueError, 'checksum'):
                    notices.fetch('https://example.com/LICENSE', 100)
                network.assert_not_called()

    def test_normal_and_build_closure_includes_macros_but_not_dev_only_packages(self):
        packages = [{'id': name, 'name': name, 'version': '1.0.0', 'source': None if name == 'root' else 'registry'} for name in ['root','normal','build','dev']]
        nodes = [{'id': 'root', 'deps': [{'pkg': name, 'dep_kinds': [{'kind': kind}]} for name,kind in [('normal',None),('build','build'),('dev','dev')]]}]
        nodes += [{'id': name, 'deps': []} for name in ['normal','build','dev']]
        selected = notices.closure({'packages':packages,'resolve':{'nodes':nodes}},'root')
        self.assertEqual([p['name'] for p in selected], ['build','normal','root'])

    def archive(self, path, files):
        with tarfile.open(path,'w:gz') as archive:
            for name, value in files.items():
                member = tarfile.TarInfo('fixture-1.0.0/' + name)
                member.size = len(value)
                archive.addfile(member,io.BytesIO(value))

    def test_nested_original_notices_preserve_every_byte_and_detect_spdx_mismatch(self):
        with tempfile.TemporaryDirectory() as directory:
            archive = Path(directory) / 'fixture.crate'
            data = b'Copyright Example\r\nPermission\n\xc2\xa9\n'
            self.archive(archive, {'Cargo.toml':b'[package]\nlicense="MIT"\n','LICENSE':data,'native/NOTICE.txt':b'Native attribution\n'})
            package = {'name':'fixture','version':'1.0.0','license':'MIT'}
            extracted = notices.extract_notices(archive, package)
            self.assertEqual(extracted,[('LICENSE',data),('native/NOTICE.txt',b'Native attribution\n')])
            content,index = notices.render([('fixture@1.0.0',name,value) for name,value in extracted])
            for record in index:
                self.assertEqual(notices.sha(content[record['offset']:record['offset']+record['bytes']]),record['sha256'])
            package['license'] = 'Apache-2.0'
            with self.assertRaisesRegex(ValueError,'differs'):
                notices.extract_notices(archive,package)

    def test_unlicensed_sources_and_unsafe_paths_fail_closed(self):
        with tempfile.TemporaryDirectory() as directory:
            archive = Path(directory) / 'fixture.crate'
            self.archive(archive, {'Cargo.toml':b'[package]\nlicense="MIT"\n','README':b'declaration is not a license'})
            with self.assertRaisesRegex(ValueError,'No original'):
                notices.extract_notices(archive,{'name':'fixture','version':'1.0.0','license':'MIT'})
            self.archive(archive, {'Cargo.toml':b'[package]\nlicense="MIT"\n','../LICENSE':b'outside'})
            with self.assertRaisesRegex(ValueError,'Unsafe'):
                notices.extract_notices(archive,{'name':'fixture','version':'1.0.0','license':'MIT'})
        self.assertFalse(notices.license_ok('Proprietary'))
        self.assertTrue(notices.license_ok('(MIT OR Apache-2.0) AND Unicode-3.0'))

    def test_missing_registry_notice_fallback_is_pinned_to_original_vcs_commit(self):
        with tempfile.TemporaryDirectory() as directory:
            archive = Path(directory) / 'fixture.crate'
            files = {'Cargo.toml':b'[package]\nlicense="MIT"\nrepository="https://github.com/example/fixture"\n', '.cargo_vcs_info.json':json.dumps({'git':{'sha1':'a'*40}}).encode()}
            self.archive(archive, files)
            sources = notices.supplemental_notices(archive,{'name':'fixture','version':'1.0.0'})
            self.assertTrue(all('/'+'a'*40+'/' in url for url,_ in sources))
            files['.cargo_vcs_info.json'] = json.dumps({'git':{'sha1':'main'}}).encode()
            self.archive(archive,files)
            with self.assertRaisesRegex(ValueError,'pinned'):
                notices.supplemental_notices(archive,{'name':'fixture','version':'1.0.0'})


if __name__ == '__main__':
    unittest.main()
