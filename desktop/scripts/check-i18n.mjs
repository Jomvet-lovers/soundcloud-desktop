#!/usr/bin/env node
/**
 * Fail-fast check for static `t('key')` usages that have no entry in the
 * English locale. Missing keys make i18next return the raw key (visible in
 * tooltips, titles and labels), which is how the `player.play` /
 * `player.prevTrack` regressions slipped through.
 *
 * Dynamic keys (`t(`track.status.${x}`)`) are skipped on purpose.
 * Plural bases are accepted when `_one` / `_other` variants exist.
 *
 * Usage: node scripts/check-i18n.mjs
 */
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('../src', import.meta.url));
const en = JSON.parse(fs.readFileSync(path.join(root, 'i18n/locales/en.json'), 'utf8'));

function flatten(obj, prefix = '', out = new Set()) {
  for (const [k, v] of Object.entries(obj)) {
    const key = prefix ? `${prefix}.${k}` : k;
    if (v && typeof v === 'object' && !Array.isArray(v)) flatten(v, key, out);
    else out.add(key);
  }
  return out;
}
const keys = flatten(en);

const files = [];
(function walk(dir) {
  for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
    const p = path.join(dir, e.name);
    if (e.isDirectory()) walk(p);
    else if (/\.(ts|tsx)$/.test(e.name)) files.push(p);
  }
})(root);

const re = /\bt\(\s*(['"])((?:\\.|(?!\1).)*?)\1/g;
const missing = new Map();
for (const f of files) {
  const text = fs.readFileSync(f, 'utf8');
  for (const m of text.matchAll(re)) {
    const key = m[2];
    if (key.includes('${')) continue;
    const has = keys.has(key) || keys.has(`${key}_one`) || keys.has(`${key}_other`);
    if (!has) {
      const list = missing.get(key) ?? [];
      list.push(path.relative(root, f).replace(/\\/g, '/'));
      missing.set(key, list);
    }
  }
}

for (const [k, list] of [...missing.entries()].sort()) {
  console.log(`${k} -> ${[...new Set(list)].join(', ')}`);
}
if (missing.size > 0) {
  console.error(`\n${missing.size} missing i18n key(s)`);
  process.exit(1);
}
console.log('i18n keys OK');
