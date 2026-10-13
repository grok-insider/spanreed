import argparse
import hashlib
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import urllib.request


def prepare(args):
    rustc = subprocess.check_output(['rustc', '--version'], text=True).strip()
    version = re.fullmatch(r'rustc (\d+\.\d+\.\d+) \([^)]+\)(?: \(built from a source tarball\))?', rustc)
    if version is None:
        raise ValueError('Unsupported distribution toolchain')
    cache = Path(args.cache).resolve()
    cache.mkdir(parents=True, exist_ok=True)
    archive = cache / f'rustc-{version[1]}-src.tar.gz'
    url = f'https://static.rust-lang.org/dist/{archive.name}'
    with urllib.request.urlopen(url + '.sha256', timeout=45) as response:
        checksum = response.read(512).decode().split()[0]
    if re.fullmatch('[a-f0-9]{64}', checksum) is None:
        raise ValueError('Invalid official Rust source checksum')
    def digest(path):
        result = hashlib.sha256()
        with path.open('rb') as stream:
            for chunk in iter(lambda: stream.read(1024 * 1024), b''):
                result.update(chunk)
        return result.hexdigest()
    if not archive.is_file() or archive.is_symlink() or digest(archive) != checksum:
        temporary = None
        total = 0
        try:
            with tempfile.NamedTemporaryFile(dir=cache, prefix=f'.{archive.name}.', suffix='.download', delete=False) as output:
                temporary = Path(output.name)
                with urllib.request.urlopen(url, timeout=60) as response:
                    while chunk := response.read(1024 * 1024):
                        total += len(chunk)
                        if total > 1024 * 1024 * 1024:
                            raise ValueError('Rust source exceeds distribution input limit')
                        output.write(chunk)
            if digest(temporary) != checksum:
                raise ValueError('Official Rust source checksum differs')
            os.replace(temporary, archive)
        finally:
            if temporary is not None:
                temporary.unlink(missing_ok=True)
    command = [sys.executable, str(Path(__file__).with_name('native-notices.py')), 'generate', '--manifest', args.manifest, '--package', args.package, '--target', args.target, '--output', args.output, '--rust-source-archive', str(archive)]
    if args.features:
        command += ['--features', args.features]
    subprocess.run(command, check=True)


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--manifest', default='Cargo.toml')
    parser.add_argument('--package', default='spanreed')
    parser.add_argument('--target', required=True)
    parser.add_argument('--features', default='')
    parser.add_argument('--output', required=True)
    parser.add_argument('--cache', default='native-rust-sources')
    prepare(parser.parse_args())
