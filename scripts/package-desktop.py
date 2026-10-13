import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import shutil

SPEC = importlib.util.spec_from_file_location('native_notices', Path(__file__).with_name('native-notices.py'))
native = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(native)


def frontend(root):
    content = native.read(root / 'dist/FRONTEND-NOTICES.txt', 16 * 1024 * 1024)
    inventory = json.loads(native.read(root / 'dist/frontend-notices.json'))
    native.require(inventory.get('schema') == 1 and inventory.get('package') == 'spanreed-desktop', 'Invalid frontend inventory')
    native.require(inventory['lockSha256'] == native.sha(native.read(root / 'bun.lock')), 'Frontend source lock differs')
    native.require(inventory['noticesSha256'] == native.sha(content), 'Frontend notice checksum differs')
    expected = {(f"{p['name']}@{p['version']}", f['path'], f['sha256']) for p in inventory['packages'] for f in p['files']}
    actual = set()
    previous_end = 0
    for section in inventory['sections']:
        start, size = section['offset'], section['bytes']
        native.require(type(start) is int and type(size) is int and start >= previous_end and size > 0, 'Invalid frontend original range')
        chunk = content[start:start + size]
        native.require(len(chunk) == size and native.sha(chunk) == section['sha256'], 'Original frontend text differs')
        key = section['package'], section['path'], section['sha256']
        native.require(key not in actual, 'Duplicate frontend notice')
        actual.add(key)
        previous_end = start + size
    native.require(expected == actual and actual, 'Incomplete frontend originals')
    return content


def stage(args):
    root = Path(args.desktop).resolve()
    notices = Path(args.notices).resolve()
    inventory = native.verify(notices, root / 'src-tauri/Cargo.lock')
    native.require(inventory['package'] == 'spanreed-desktop' and inventory['features'] == ['tauri/custom-protocol'], 'Wrong desktop dependency closure')
    frontend(root)
    destination = root / 'distribution-notices'
    native.require(not destination.exists(), 'Refusing to replace staged desktop notices')
    shutil.copytree(notices, destination / 'native')
    shutil.copyfile(root.parent / 'LICENSE', destination / 'LICENSE')
    shutil.copyfile(root / 'dist/FRONTEND-NOTICES.txt', destination / 'FRONTEND-NOTICES.txt')
    shutil.copyfile(root / 'dist/frontend-notices.json', destination / 'frontend-notices.json')
    config = {'bundle': {'licenseFile': '../../LICENSE', 'resources': {'../distribution-notices/': 'licenses/'}}}
    (root / 'src-tauri/notices.conf.json').write_text(json.dumps(config, indent=2) + '\n')


def verify(args):
    root = Path(args.desktop).resolve()
    notices = Path(args.notices).resolve()
    native.verify(notices, root / 'src-tauri/Cargo.lock')
    frontend_bytes = frontend(root)
    binary = Path(args.binary)
    native.require(binary.is_file() and not binary.is_symlink() and binary.stat().st_size < 1024 * 1024 * 1024, 'Invalid desktop executable')
    executable = binary.read_bytes()
    for content in [native.read(root.parent / 'LICENSE'), native.read(notices / 'THIRD-PARTY-NOTICES.txt', 32 * 1024 * 1024), frontend_bytes]:
        native.require(content in executable, 'Desktop executable does not preserve original license bytes')
    print(json.dumps({'verified': True, 'embeddedNativeAndFrontendNotices': True, 'binarySha256': hashlib.sha256(executable).hexdigest()}))


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('action', choices=['stage', 'verify'])
    parser.add_argument('--desktop', default='desktop')
    parser.add_argument('--notices', required=True)
    parser.add_argument('--binary')
    args = parser.parse_args()
    if args.action == 'stage':
        stage(args)
    else:
        native.require(args.binary is not None, 'Desktop verification requires a binary')
        verify(args)
