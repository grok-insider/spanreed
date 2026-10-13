import importlib.util
import io
import json
from pathlib import Path
import tarfile
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location('native_notices', ROOT / 'scripts/native-notices.py')
notices = importlib.util.module_from_spec(spec)
spec.loader.exec_module(notices)


class NativeNoticesTests(unittest.TestCase):
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
