import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import test from 'node:test';
import { frontendNotices } from './frontend-notices.mjs';

function fixture() {
  const root = mkdtempSync(join(tmpdir(), 'spanreed-notices-'));
  mkdirSync(join(root, 'notices'));
  writeFileSync(join(root, 'notices/source-pins.json'), '{}');
  writeFileSync(join(root, 'bun.lock'), 'fixture source lock');
  writeFileSync(join(root, 'package.json'), JSON.stringify({ name: 'spanreed-desktop', dependencies: { example: '1.0.0' } }));
  for (const name of ['example', 'vite']) {
    mkdirSync(join(root, 'node_modules', name), { recursive: true });
    writeFileSync(join(root, 'node_modules', name, 'package.json'), JSON.stringify({ name, version: '1.0.0', license: 'MIT' }));
    writeFileSync(join(root, 'node_modules', name, 'LICENSE'), 'Original author\r\nPermission\n');
  }
  return root;
}

test('original bytes and nested font notices survive rendering', () => {
  const root = fixture();
  try {
    mkdirSync(join(root, 'node_modules/example/fonts'));
    writeFileSync(join(root, 'node_modules/example/fonts/OFL.txt'), 'Original font license\r\n');
    const result = frontendNotices(join(root, 'package.json'));
    assert.equal(result.inventory.packages.length, 2);
    assert.equal(result.inventory.sections.length, 3);
    for (const section of result.inventory.sections) {
      const original = result.content.subarray(section.offset, section.offset + section.bytes);
      assert.ok(original.equals(Buffer.from(section.path === 'fonts/OFL.txt' ? 'Original font license\r\n' : 'Original author\r\nPermission\n')));
    }
  } finally { rmSync(root, { recursive: true, force: true }); }
});

test('missing original notices, unsupported licenses fail closed', () => {
  const root = fixture();
  try {
    rmSync(join(root, 'node_modules/example/LICENSE'));
    assert.throws(() => frontendNotices(join(root, 'package.json')), /Missing original/);
    writeFileSync(join(root, 'node_modules/example/LICENSE'), 'Original');
    writeFileSync(join(root, 'node_modules/example/package.json'), JSON.stringify({ name: 'example', version: '1.0.0', license: 'Proprietary' }));
    assert.throws(() => frontendNotices(join(root, 'package.json')), /Unreviewed/);
  } finally { rmSync(root, { recursive: true, force: true }); }
});
