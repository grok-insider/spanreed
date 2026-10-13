import argparse
import hashlib
import importlib.util
import io
from pathlib import Path
import sys
import tempfile
import unittest
from unittest import mock


spec = importlib.util.spec_from_file_location('prepare_native_notices', Path(__file__).resolve().parents[1] / 'prepare-native-notices.py')
prepare = importlib.util.module_from_spec(spec)
spec.loader.exec_module(prepare)


class PrepareNativeNoticesTests(unittest.TestCase):
    def args(self, root):
        return argparse.Namespace(cache=str(root), manifest='Cargo.toml', package='spanreed', target='x86_64-unknown-linux-gnu', output='output', features='tray')

    def test_download_preserves_other_invocations_and_active_interpreter(self):
        payload = b'original source archive'
        checksum = hashlib.sha256(payload).hexdigest().encode()
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            other_download = root / 'rustc-1.98.1-src.tar.download'
            other_download.write_bytes(b'other process')
            with mock.patch.object(prepare.subprocess, 'check_output', return_value='rustc 1.98.1 (abcdef 2026-10-01)'), mock.patch.object(prepare.urllib.request, 'urlopen', side_effect=[io.BytesIO(checksum), io.BytesIO(payload)]), mock.patch.object(prepare.subprocess, 'run') as run:
                prepare.prepare(self.args(root))
            self.assertEqual(other_download.read_bytes(), b'other process')
            self.assertEqual((root / 'rustc-1.98.1-src.tar.gz').read_bytes(), payload)
            self.assertEqual(sorted(path.name for path in root.iterdir()), ['rustc-1.98.1-src.tar.download', 'rustc-1.98.1-src.tar.gz'])
            self.assertEqual(run.call_args.args[0][0], sys.executable)
            self.assertEqual(run.call_args.args[0][-2:], ['--features', 'tray'])
            self.assertTrue(run.call_args.kwargs['check'])

    def test_bad_checksum_cleans_only_own_download_and_never_builds(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            other_download = root / 'rustc-1.98.1-src.tar.download'
            other_download.write_bytes(b'other process')
            with mock.patch.object(prepare.subprocess, 'check_output', return_value='rustc 1.98.1 (abcdef 2026-10-01)'), mock.patch.object(prepare.urllib.request, 'urlopen', side_effect=[io.BytesIO(b'0' * 64), io.BytesIO(b'corrupt')]), mock.patch.object(prepare.subprocess, 'run') as run:
                with self.assertRaisesRegex(ValueError, 'checksum differs'):
                    prepare.prepare(self.args(root))
            self.assertEqual(other_download.read_bytes(), b'other process')
            self.assertEqual(list(root.iterdir()), [other_download])
            run.assert_not_called()

    def test_verified_cache_avoids_archive_download(self):
        payload = b'cached original source'
        checksum = hashlib.sha256(payload).hexdigest().encode()
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / 'rustc-1.98.1-src.tar.gz').write_bytes(payload)
            with mock.patch.object(prepare.subprocess, 'check_output', return_value='rustc 1.98.1 (abcdef 2026-10-01)'), mock.patch.object(prepare.urllib.request, 'urlopen', return_value=io.BytesIO(checksum)) as fetch, mock.patch.object(prepare.subprocess, 'run'):
                prepare.prepare(self.args(root))
            self.assertEqual(fetch.call_count, 1)
            self.assertTrue(fetch.call_args.args[0].endswith('.sha256'))


if __name__ == '__main__':
    unittest.main()
