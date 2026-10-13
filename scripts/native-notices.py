import argparse
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import re
import shutil
import subprocess
import tarfile
import tomllib
import urllib.request

MAX_CRATE = 64 * 1024 * 1024
MAX_NOTICE = 4 * 1024 * 1024
NOTICE = re.compile(r'^(LICENSE|LICENCE|COPYING|COPYRIGHT|NOTICE|UNLICENSE)(?:[-_.].*)?$', re.I)
LICENSES = {'0BSD', 'MIT', 'Apache-2.0', 'BSD-2-Clause', 'BSD-3-Clause', 'Unlicense', 'ISC', 'MPL-2.0', 'Unicode-3.0', 'Unicode-DFS-2016', 'Zlib', 'BSL-1.0', 'CC0-1.0', 'MIT-0', 'CDLA-Permissive-2.0', 'LLVM-exception', 'AGPL-3.0-or-later'}
HEADER = b'Spanreed native third-party notices\n\nOriginal license and notice bytes are preserved below. Exact registry source archives accompany this inventory under crate-sources/; they include corresponding source for MPL components. The inventory conservatively includes normal and build dependencies for the selected target and features, including macros. Not every included component survives linking. Spanreed and Fabrials corresponding source are supplied separately with this release.\n\n'


def require(condition, message):
    if not condition:
        raise ValueError(message)


def sha(data):
    return hashlib.sha256(data).hexdigest()


def read(path, limit=MAX_CRATE):
    path = Path(path)
    require(path.is_file() and not path.is_symlink(), f'Expected regular file: {path.name}')
    require(path.stat().st_size <= limit, f'File exceeds limit: {path.name}')
    return path.read_bytes()


def relative(name):
    p = PurePosixPath(name)
    require(name and not p.is_absolute() and str(p) == name and '..' not in p.parts, 'Unsafe archive path')
    return name


def license_ok(value):
    return bool(value) and all(part in LICENSES | {'AND', 'OR', 'WITH'} for part in re.sub(r'[()/]', ' ', value).split())


def closure(metadata, root_name):
    packages = {p['id']: p for p in metadata['packages']}
    nodes = {n['id']: n for n in metadata['resolve']['nodes']}
    roots = [p['id'] for p in packages.values() if p['name'] == root_name and p.get('source') is None]
    require(len(roots) == 1, 'Expected one native root package')
    seen, pending = set(), roots[:]
    while pending:
        current = pending.pop()
        if current in seen:
            continue
        seen.add(current)
        pending.extend(d['pkg'] for d in nodes[current]['deps'] if any(k['kind'] in (None, 'build') for k in d['dep_kinds']))
    return sorted((packages[i] for i in seen), key=lambda p: (p['name'], p['version']))


def extract_notices(archive_path, package, required=True):
    root = f"{package['name']}-{package['version']}"
    found, seen = [], set()
    with tarfile.open(archive_path, 'r:gz') as archive:
        manifest = archive.getmember(root + '/Cargo.toml')
        require(manifest.isfile() and manifest.size < MAX_NOTICE, 'Invalid crate manifest')
        declared = tomllib.loads(archive.extractfile(manifest).read().decode())['package']
        require(declared.get('license') == package.get('license'), 'Crate license differs from Cargo metadata')
        for member in archive:
            path = PurePosixPath(relative(member.name))
            require(path.parts[0] == root, 'Unexpected crate archive root')
            name = str(path.relative_to(root))
            if not NOTICE.fullmatch(path.name) and name != declared.get('license-file'):
                continue
            require(member.isfile() and member.size <= MAX_NOTICE and name not in seen, 'Invalid original notice')
            content = archive.extractfile(member).read()
            require(content and b'\0' not in content, 'Empty or binary license')
            content.decode('utf-8')
            seen.add(name)
            found.append((name, content))
    require(found or not required, f"No original license text for {package['name']}@{package['version']}")
    return sorted(found)


def supplemental_notices(archive_path, package):
    root = f"{package['name']}-{package['version']}"
    with tarfile.open(archive_path, 'r:gz') as archive:
        manifest = tomllib.loads(archive.extractfile(root + '/Cargo.toml').read().decode())['package']
        pins = json.loads(read(Path(__file__).parent / 'notices/source-pins.json', MAX_NOTICE))
        pinned = pins.get(f"{package['name']}@{package['version']}", {})
        repository = manifest.get('repository')
        if not repository:
            require(pinned.get('crateSha256') == sha(read(archive_path)), 'Missing reviewed repository identity')
            repository = pinned['repository']
        repository = repository.replace('http://github.com/', 'https://github.com/').removesuffix('.git').rstrip('/')
        try:
            commit = json.loads(archive.extractfile(root + '/.cargo_vcs_info.json').read())['git']['sha1']
        except KeyError:
            require(pinned.get('repository') == repository and pinned.get('crateSha256') == sha(read(archive_path)), 'Missing reviewed source pin')
            commit = pinned['commit']
    require(re.fullmatch(r'[a-f0-9]{40}', commit) and re.fullmatch(r'https://github.com/[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+', repository), 'Missing pinned upstream notice source')
    base = repository.replace('https://github.com/', 'https://raw.githubusercontent.com/') + '/' + commit + '/'
    sources = [(base + name, name) for name in ['LICENSE', 'LICENSE-MIT', 'LICENSE-APACHE', 'LICENSE_MIT', 'LICENSE_APACHE', 'COPYING', 'LICENSE.txt', 'LICENSE.md']]
    if package.get('license') == 'MPL-2.0':
        sources.append(('https://www.mozilla.org/media/MPL/2.0/index.txt', 'LICENSE-MPL-2.0'))
    return sources


def render(sections):
    result = bytearray(HEADER)
    index = []
    for component, name, data in sorted(sections):
        relative(name)
        result.extend(f'===== {component} / {name} =====\n'.encode())
        index.append({'component': component, 'path': name, 'offset': len(result), 'bytes': len(data), 'sha256': sha(data)})
        result.extend(data)
        result.extend(b'\n===== END ORIGINAL FILE =====\n\n')
    require(len(result) <= 32 * 1024 * 1024, 'Notices exceed bounded payload')
    return bytes(result), index


def fetch(url, limit):
    offline = os.environ.get('SPANREED_NOTICE_INPUTS')
    if offline:
        inputs = json.loads(read(offline))
        if url not in inputs:
            raise urllib.error.HTTPError(url, 404, 'No pinned offline input', {}, None)
        item = inputs[url]
        content = read(item['file'], limit)
        require(sha(content) == item['sha256'], 'Pinned offline notice checksum differs')
        return content
    with urllib.request.urlopen(url, timeout=45) as response:
        content = response.read(limit + 1)
    require(len(content) <= limit, 'Downloaded input exceeds limit')
    return content


def source_toolchain_notices(archive_path, rustc):
    match = re.fullmatch(r'rustc (\d+\.\d+\.\d+) \(([^)]+)\)(?: \(built from a source tarball\))?', rustc)
    require(match is not None, 'Unsupported Rust toolchain identity')
    expected_version = f'{match[1]} ({match[2]})'
    root = f'rustc-{match[1]}-src/'
    archive_path = Path(archive_path)
    require(archive_path.is_file() and not archive_path.is_symlink() and archive_path.stat().st_size <= 1024 * 1024 * 1024, 'Invalid Rust source archive')
    hasher = hashlib.sha256()
    with archive_path.open('rb') as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b''):
            hasher.update(chunk)
    origin = 'rustc-source-sha256:' + hasher.hexdigest()
    sections, provenance, names, source_version = [], [], set(), None
    with tarfile.open(archive_path, 'r|gz') as archive:
        for member in archive:
            if member.isdir() and member.name.rstrip('/') == root.rstrip('/'):
                continue
            require(member.name.startswith(root), 'Unexpected Rust source root')
            name = member.name[len(root):]
            if name == 'version':
                require(member.isfile() and member.size <= 1024, 'Invalid Rust source version')
                source_version = archive.extractfile(member).read().decode().strip()
            selected = name in {'LICENSE-MIT', 'LICENSE-APACHE', 'COPYRIGHT'} or name.startswith('LICENSES/') or (name.startswith('library/') and NOTICE.fullmatch(PurePosixPath(name).name))
            if not selected or member.isdir():
                continue
            relative(name)
            require(member.isfile() and 0 < member.size <= MAX_NOTICE and name not in names, 'Invalid Rust source notice')
            data = archive.extractfile(member).read()
            data.decode('utf-8')
            require(b'\0' not in data, 'Binary Rust source notice')
            names.add(name)
            sections.append((f'rust-toolchain@{match[1]}', name, data))
            provenance.append({'path': name, 'source': origin, 'sha256': sha(data)})
    require(source_version == expected_version, 'Rust source version differs from artifact toolchain')
    required = {'LICENSE-MIT', 'LICENSE-APACHE', 'COPYRIGHT', 'LICENSES/Unicode-3.0.txt', 'library/compiler-builtins/LICENSE.txt', 'library/compiler-builtins/libm/LICENSE.txt'}
    require(required <= names, 'Incomplete original Rust standard-library notices')
    return sections, provenance


def toolchain_notices(rustc, target, existing=None, source_archive=None):
    match = re.fullmatch(r'rustc (\d+\.\d+\.\d+) \(([a-f0-9]+) ([0-9-]+)\)(?: \(built from a source tarball\))?', rustc)
    require(match is not None, 'Unsupported Rust toolchain identity')
    version, commit, _ = match.groups()
    sections, provenance = [], []
    if source_archive:
        require(not existing, 'Choose one original Rust notice source')
        sections, provenance = source_toolchain_notices(source_archive, rustc)
        if target.endswith('linux-musl'):
            url = 'https://git.musl-libc.org/cgit/musl/plain/COPYRIGHT?h=v1.2.5'
            content = fetch(url, MAX_NOTICE)
            sections.append((f'rust-toolchain@{version}', 'musl-COPYRIGHT', content))
            provenance.append({'path': 'musl-COPYRIGHT', 'source': url, 'sha256': sha(content)})
        return sections, provenance
    if existing:
        root = Path(existing)
        descriptor = json.loads(read(root / 'wasm-notices.json', MAX_NOTICE))
        require(descriptor['rustc'] == rustc, 'Original runtime bundle used another Rust toolchain')
        supplement = descriptor['supplement']
        blob = read(root / relative(supplement['path'].removeprefix('/')), 32 * 1024 * 1024)
        require(sha(blob) == supplement['sha256'], 'Original runtime bundle checksum differs')
        for record in descriptor['sections']:
            if record['component'] != f'rust-standard-library@{version}':
                continue
            require(type(record['offset']) is int and type(record['bytes']) is int and record['offset'] >= 0 and record['bytes'] > 0, 'Invalid original runtime range')
            content = blob[record['offset']:record['offset'] + record['bytes']]
            require(len(content) == record['bytes'] and sha(content) == record['sha256'], 'Original runtime notice checksum differs')
            sections.append((f'rust-toolchain@{version}', record['path'], content))
            provenance.append({'path': record['path'], 'source': 'rustc-source-sha256:' + descriptor['standardLibrary']['sourceArchiveSha256'], 'sha256': sha(content)})
        require({'LICENSE-MIT', 'LICENSE-APACHE', 'COPYRIGHT', 'COPYRIGHT-library.html'} <= {path for _,path,_ in sections}, 'Incomplete original runtime bundle')
        if not target.endswith('linux-musl'):
            return sections, provenance
    sources = [(f'https://raw.githubusercontent.com/rust-lang/rust/{commit}/{name}', name) for name in ['LICENSE-MIT', 'LICENSE-APACHE', 'COPYRIGHT']]
    sources.append((f'https://doc.rust-lang.org/{version}/copyright.html', 'COPYRIGHT-library.html'))
    if existing:
        sources = []
    if target.endswith('linux-musl'):
        sources.append(('https://git.musl-libc.org/cgit/musl/plain/COPYRIGHT?h=v1.2.5', 'musl-COPYRIGHT'))
    for url, name in sources:
        content = fetch(url, MAX_NOTICE)
        require(content and b'\0' not in content, 'Invalid toolchain notice')
        content.decode('utf-8')
        if name == 'COPYRIGHT-library.html':
            require(b'Copyright notices for The Rust Standard Library' in content, 'Unexpected Rust attribution document')
        sections.append((f'rust-toolchain@{version}', name, content))
        provenance.append({'path': name, 'source': url, 'sha256': sha(content)})
    return sections, provenance


def generate(args):
    manifest = Path(args.manifest).resolve()
    root = manifest.parent
    lock_path = root / 'Cargo.lock'
    lock_bytes = read(lock_path)
    lock = tomllib.loads(lock_bytes.decode())
    command = ['cargo', 'metadata', '--locked', '--format-version=1', '--manifest-path', str(manifest), '--filter-platform', args.target]
    if args.features:
        command += ['--features', args.features]
    metadata = json.loads(subprocess.check_output(command, text=True))
    packages = closure(metadata, args.package)
    locked = {(p['name'], p['version']): p for p in lock['package'] if p.get('source') == 'registry+https://github.com/rust-lang/crates.io-index'}
    out = Path(args.output)
    out.mkdir(parents=True, exist_ok=False)
    sources = out / 'crate-sources'
    sources.mkdir()
    (out / 'upstream-notices').mkdir()
    entries, sections = [], []
    for package in packages:
        name, version = package['name'], package['version']
        identity = f'{name}@{version}'
        require(license_ok(package.get('license')), f'Unreviewed license: {identity}: {package.get("license")}')
        if package.get('source') is None:
            entries.append({'name': name, 'version': version, 'license': package['license'], 'source': 'release-corresponding-source'})
            continue
        require((name, version) in locked, f'Only locked crates.io sources supported: {identity}')
        digest = locked[name, version]['checksum']
        filename = f'{name}-{version}.crate'
        archive_index = getattr(args, 'crate_archives', None)
        archives = [Path(json.loads(read(archive_index))[filename])] if archive_index else sorted((Path(args.cargo_home) / 'registry/cache').glob('*/' + filename))
        archive = next((p for p in archives if sha(read(p)) == digest), None)
        require(archive is not None, f'Original checksum-matched cached crate is required: {identity}')
        notices = extract_notices(archive, package, required=False)
        supplemental = []
        if not notices:
            for url, path in supplemental_notices(archive, package):
                try:
                    content = fetch(url, MAX_NOTICE)
                except urllib.error.HTTPError as error:
                    if error.code == 404:
                        error.close()
                        continue
                    raise
                content.decode('utf-8')
                require(content and b'\0' not in content, 'Invalid pinned upstream license')
                stored = f'upstream-notices/{name}-{version}-{path}'
                (out / stored).write_bytes(content)
                supplemental.append({'path': path, 'file': stored, 'source': url, 'sha256': sha(content)})
                notices.append((path, content))
            notices.sort()
        require(notices, f'No original license texts for {identity}')
        sections.extend((identity, path, content) for path, content in notices)
        shutil.copyfile(archive, sources / filename)
        entries.append({'name': name, 'version': version, 'license': package['license'], 'source': 'crate-sources/' + filename, 'sha256': digest, 'noticePaths': [p for p, _ in notices], 'supplemental': supplemental})
    rustc = subprocess.check_output(['rustc', '--version'], text=True).strip()
    runtime, runtime_provenance = toolchain_notices(rustc, args.target, args.rust_notices, getattr(args, 'rust_source_archive', None))
    content, index = render(sections + runtime)
    (out / 'THIRD-PARTY-NOTICES.txt').write_bytes(content)
    descriptor = {'schema': 1, 'package': args.package, 'target': args.target, 'features': sorted(filter(None, args.features.split(','))), 'cargoLockSha256': sha(lock_bytes), 'rustc': rustc, 'packages': entries, 'runtime': runtime_provenance, 'runtimeSourceArchive': bool(getattr(args, 'rust_source_archive', None)), 'sections': index, 'noticesSha256': sha(content)}
    (out / 'inventory.json').write_text(json.dumps(descriptor, indent=2) + '\n')
    verify(out, lock_path, packages)


def verify(output, lock_path, expected_packages=None):
    output = Path(output)
    descriptor = json.loads(read(output / 'inventory.json', MAX_NOTICE))
    require(descriptor.get('schema') == 1 and descriptor.get('target'), 'Invalid inventory schema')
    lock_bytes = read(lock_path)
    require(sha(lock_bytes) == descriptor['cargoLockSha256'], 'Wrong source lockfile')
    if expected_packages is None:
        command = ['cargo', 'metadata', '--locked', '--format-version=1', '--manifest-path', str(Path(lock_path).parent / 'Cargo.toml'), '--filter-platform', descriptor['target']]
        if descriptor['features']:
            command += ['--features', ','.join(descriptor['features'])]
        expected_packages = closure(json.loads(subprocess.check_output(command, text=True)), descriptor['package'])
    expected = {(p['name'], p['version'], p['license'], p.get('source') is None) for p in expected_packages}
    actual_packages = {(p['name'], p['version'], p['license'], p['source'] == 'release-corresponding-source') for p in descriptor['packages']}
    require(expected == actual_packages, 'Native dependency closure differs from selected target/features')
    lock = tomllib.loads(lock_bytes.decode())
    locked = {(p['name'], p['version']): p for p in lock['package'] if p.get('source') == 'registry+https://github.com/rust-lang/crates.io-index'}
    notices = read(output / 'THIRD-PARTY-NOTICES.txt', 32 * 1024 * 1024)
    require(sha(notices) == descriptor['noticesSha256'], 'Notice checksum differs')
    sections, identities, expected_files = [], set(), {'inventory.json', 'THIRD-PARTY-NOTICES.txt'}
    for package in descriptor['packages']:
        identity = package['name'], package['version']
        require(identity not in identities and license_ok(package['license']), 'Duplicate or unreviewed package')
        identities.add(identity)
        if package['source'] == 'release-corresponding-source':
            continue
        name = relative(package['source'])
        require(name == f'crate-sources/{identity[0]}-{identity[1]}.crate', 'Unexpected source archive path')
        require(identity in locked and package['sha256'] == locked[identity]['checksum'], 'Crate differs from source lock')
        require(sha(read(output / name)) == package['sha256'], 'Registry source checksum differs')
        original = extract_notices(output / name, package, required=False)
        allowed = dict(supplemental_notices(output / name, package)) if package.get('supplemental') else {}
        for extra in package.get('supplemental', []):
            require(extra['source'] in allowed and allowed[extra['source']] == extra['path'], 'Unpinned supplemental source')
            stored = relative(extra['file'])
            require(stored == f"upstream-notices/{identity[0]}-{identity[1]}-{extra['path']}", 'Unsafe supplemental path')
            data = read(output / stored, MAX_NOTICE)
            require(sha(data) == extra['sha256'], 'Supplemental license differs')
            original.append((extra['path'], data))
            expected_files.add(stored)
        original.sort()
        require(original, 'Missing crate attribution')
        require(package['noticePaths'] == [p for p, _ in original], 'Incomplete crate attribution')
        sections.extend((f'{identity[0]}@{identity[1]}', p, content) for p, content in original)
        expected_files.add(name)
    version = descriptor['rustc'].split()[1]
    runtime = {p['path']: p for p in descriptor['runtime']}
    required_runtime = {'LICENSE-MIT', 'LICENSE-APACHE', 'COPYRIGHT'}
    if descriptor.get('runtimeSourceArchive'):
        required_runtime |= {'LICENSES/Unicode-3.0.txt', 'library/compiler-builtins/LICENSE.txt', 'library/compiler-builtins/libm/LICENSE.txt'}
        sources = {item['source'] for item in runtime.values() if item['path'] != 'musl-COPYRIGHT'}
        require(len(sources) == 1 and re.fullmatch(r'rustc-source-sha256:[a-f0-9]{64}', next(iter(sources))), 'Invalid original Rust source identity')
    else:
        required_runtime.add('COPYRIGHT-library.html')
    require(required_runtime <= set(runtime), 'Missing runtime notices')
    if descriptor['target'].endswith('linux-musl'):
        require('musl-COPYRIGHT' in runtime, 'Missing static libc notices')
    for record in descriptor['sections']:
        if record['component'] != f'rust-toolchain@{version}':
            continue
        require(record['path'] in runtime and record['sha256'] == runtime[record['path']]['sha256'], 'Unexpected runtime notice')
        require(type(record['offset']) is int and type(record['bytes']) is int and record['offset'] >= 0 and record['bytes'] > 0, 'Invalid notice range')
        content = notices[record['offset']:record['offset'] + record['bytes']]
        require(len(content) == record['bytes'] and sha(content) == record['sha256'], 'Runtime notice checksum differs')
        sections.append((record['component'], record['path'], content))
    rebuilt, indices = render(sections)
    require(rebuilt == notices and indices == descriptor['sections'], 'Noncanonical or incomplete original notices')
    require(len({(c, p) for c, p, _ in sections}) == len(sections), 'Duplicate notice section')
    actual = set()
    for path in output.rglob('*'):
        require(not path.is_symlink(), 'Notice directory contains a symlink')
        if path.is_file():
            actual.add(path.relative_to(output).as_posix())
    require(actual == expected_files, 'Unexpected or missing redistribution payload')
    print(json.dumps({'verified': True, 'target': descriptor['target'], 'packages': len(identities), 'originalNotices': len(sections), 'registrySources': sum(p['source'].startswith('crate-sources/') for p in descriptor['packages'])}))
    return descriptor


def main():
    parser = argparse.ArgumentParser()
    sub = parser.add_subparsers(dest='action', required=True)
    create = sub.add_parser('generate')
    create.add_argument('--manifest', default='Cargo.toml')
    create.add_argument('--package', default='spanreed')
    create.add_argument('--target', required=True)
    create.add_argument('--features', default='')
    create.add_argument('--cargo-home', default=os.environ.get('CARGO_HOME', str(Path.home() / '.cargo')))
    create.add_argument('--output', required=True)
    create.add_argument('--rust-notices')
    create.add_argument('--rust-source-archive')
    create.add_argument('--crate-archives')
    check = sub.add_parser('verify')
    check.add_argument('--output', required=True)
    check.add_argument('--lockfile', required=True)
    args = parser.parse_args()
    if args.action == 'generate':
        generate(args)
    else:
        verify(args.output, args.lockfile)


if __name__ == '__main__':
    main()
