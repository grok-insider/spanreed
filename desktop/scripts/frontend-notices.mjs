import { createHash } from 'node:crypto';
import { existsSync, lstatSync, readFileSync, readdirSync, realpathSync } from 'node:fs';
import { createRequire } from 'node:module';
import { dirname, join, relative } from 'node:path';

const allowed = new Set(['MIT', 'ISC', 'OFL-1.1', 'Apache-2.0', 'BSD-2-Clause', 'BSD-3-Clause', '0BSD', 'CC0-1.0']);
const notice = /(^|[-_])(license|licence|copying|copyright|notice|ofl)([._-].*)?$/i;
const hash = value => createHash('sha256').update(value).digest('hex');

function regular(path) {
  const stat = lstatSync(path);
  if (!stat.isFile() || stat.isSymbolicLink() || stat.size > 4 * 1024 * 1024) throw new Error('Invalid frontend notice input');
  const bytes = readFileSync(path);
  if (!bytes.length || bytes.includes(0) || !Buffer.from(bytes.toString('utf8')).equals(bytes)) throw new Error('Frontend notices must preserve original UTF-8 text');
  return bytes;
}

function resolvePackage(name, parent, optional = false) {
  if (!/^(@[a-z0-9_.-]+\/)?[a-z0-9_.-]+$/.test(name)) throw new Error('Invalid frontend dependency identity');
  const paths = createRequire(parent).resolve.paths(name) ?? [];
  for (const path of paths) {
    const manifest = join(path, name, 'package.json');
    if (existsSync(manifest)) return realpathSync(manifest);
  }
  if (optional) return null;
  throw new Error(`Missing installed frontend dependency: ${name}`);
}

export function frontendNotices(manifestPath) {
  const root = JSON.parse(regular(manifestPath));
  const lock = readFileSync(join(dirname(manifestPath), 'bun.lock'));
  const pins = JSON.parse(regular(join(dirname(manifestPath), 'notices/source-pins.json')));
  const pending = Object.keys(root.dependencies ?? {}).map(name => resolvePackage(name, manifestPath));
  const bundledBuild = resolvePackage('vite', manifestPath);
  pending.push(bundledBuild);
  const visited = new Set();
  const packages = new Map();
  const sections = [];
  while (pending.length) {
    const manifest = pending.pop();
    if (!manifest || visited.has(manifest)) continue;
    visited.add(manifest);
    const pkg = JSON.parse(regular(manifest));
    if (typeof pkg.license !== 'string' || pkg.license.replace(/[()]/g, ' ').split(/\s+/).filter(Boolean).some(part => !allowed.has(part) && !['OR', 'AND'].includes(part))) throw new Error(`Unreviewed frontend license: ${pkg.name}: ${pkg.license}`);
    const identity = `${pkg.name}@${pkg.version}`;
    const base = dirname(manifest);
    const files = [];
    const folders = [base];
    while (folders.length) {
      const directory = folders.pop();
      for (const item of readdirSync(directory, { withFileTypes: true })) {
        if (item.name === 'node_modules' || item.name === '.git') continue;
        const path = join(directory, item.name);
        if (item.isDirectory()) folders.push(path);
        else if (notice.test(item.name)) files.push([relative(base, path).replaceAll('\\', '/'), regular(path)]);
      }
    }
    files.sort(([a], [b]) => a.localeCompare(b, 'en'));
    if (!files.length && pins[identity]) {
      const pin = pins[identity];
      if (!/^[a-zA-Z0-9._-]+$/.test(pin.path) || !lock.includes(pin.registryIntegrity)) throw new Error('Unpinned upstream frontend notice');
      const bytes = regular(join(dirname(manifestPath), 'notices', pin.path));
      if (hash(bytes) !== pin.sha256) throw new Error('Upstream frontend notice checksum differs');
      files.push(['upstream/' + pin.path, bytes]);
    }
    if (!files.length) throw new Error(`Missing original frontend license: ${identity}`);
    const record = { name: pkg.name, version: pkg.version, license: pkg.license, files: files.map(([path, bytes]) => ({ path, sha256: hash(bytes) })) };
    if (packages.has(identity) && JSON.stringify(packages.get(identity)) !== JSON.stringify(record)) throw new Error(`Conflicting installed frontend copies: ${identity}`);
    if (!packages.has(identity)) {
      packages.set(identity, record);
      for (const [path, bytes] of files) sections.push({ identity, path, bytes });
    }
    if (manifest === bundledBuild) continue;
    for (const name of Object.keys(pkg.dependencies ?? {})) pending.push(resolvePackage(name, manifest, Object.hasOwn(pkg.optionalDependencies ?? {}, name)));
    for (const name of Object.keys(pkg.optionalDependencies ?? {})) pending.push(resolvePackage(name, manifest, true));
    for (const name of Object.keys(pkg.peerDependencies ?? {})) pending.push(resolvePackage(name, manifest, pkg.peerDependenciesMeta?.[name]?.optional === true));
  }
  sections.sort((a, b) => `${a.identity}/${a.path}`.localeCompare(`${b.identity}/${b.path}`, 'en'));
  const chunks = [Buffer.from('Spanreed desktop frontend notices\n\nOriginal notices from installed production dependencies and the Vite runtime are preserved below. The inventory conservatively includes dependency code removed during bundling.\n\n')];
  const index = [];
  let offset = chunks[0].length;
  for (const section of sections) {
    const heading = Buffer.from(`===== ${section.identity} / ${section.path} =====\n`);
    const end = Buffer.from('\n===== END ORIGINAL FILE =====\n\n');
    offset += heading.length;
    index.push({ package: section.identity, path: section.path, offset, bytes: section.bytes.length, sha256: hash(section.bytes) });
    chunks.push(heading, section.bytes, end);
    offset += section.bytes.length + end.length;
  }
  if (offset > 16 * 1024 * 1024) throw new Error('Frontend notices exceed bounded output');
  const content = Buffer.concat(chunks);
  const inventory = { schema: 1, package: root.name, lockSha256: hash(lock), packages: [...packages.values()].sort((a, b) => `${a.name}@${a.version}`.localeCompare(`${b.name}@${b.version}`, 'en')), sections: index, noticesSha256: hash(content) };
  return { content, inventory };
}

export function frontendNoticePlugin(manifestPath) {
  return {
    name: 'spanreed-frontend-notices',
    generateBundle() {
      const result = frontendNotices(manifestPath);
      this.emitFile({ type: 'asset', fileName: 'FRONTEND-NOTICES.txt', source: result.content });
      this.emitFile({ type: 'asset', fileName: 'frontend-notices.json', source: JSON.stringify(result.inventory, null, 2) + '\n' });
    },
  };
}
