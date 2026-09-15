#!/usr/bin/env node
/**
 * Single source of truth for the app version: the git tag.
 *
 * Usage:
 *   node scripts/sync-version.mjs           # derive from `git describe --tags --abbrev=0`
 *   node scripts/sync-version.mjs 1.2.3     # explicit version (CI uses this on tag pushes)
 *
 * It writes the resolved version into package.json, src-tauri/Cargo.toml and
 * src-tauri/tauri.conf.json so every installer carries the tag it was built
 * from. The Rust binary additionally embeds git metadata at compile time in
 * src-tauri/build.rs, so `get_app_info` stays accurate even if these files are
 * stale.
 */
import { execFileSync } from 'node:child_process';
import { readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');

function gitTagVersion() {
  try {
    const tag = execFileSync('git', ['describe', '--tags', '--abbrev=0'], {
      cwd: root,
      encoding: 'utf8',
      stdio: ['ignore', 'pipe', 'ignore'],
    }).trim();
    return tag.replace(/^v/, '');
  } catch {
    return null;
  }
}

const version = (process.argv[2] || gitTagVersion() || '').trim().replace(/^v/, '');

if (!/^\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?$/.test(version)) {
  console.error(
    `[sync-version] Could not determine a valid version (got "${version}").\n` +
      '              Pass one explicitly, e.g. `node scripts/sync-version.mjs 1.2.3`.',
  );
  process.exit(1);
}

/** Replace the first match of `pattern` in a file, failing loudly if nothing matched. */
function patch(relativePath, pattern, replacement) {
  const path = join(root, relativePath);
  const before = readFileSync(path, 'utf8');

  if (!pattern.test(before)) {
    throw new Error(`[sync-version] no version field matched in ${relativePath}`);
  }

  const after = before.replace(pattern, replacement);
  if (after !== before) {
    writeFileSync(path, after);
    console.log(`[sync-version] ${relativePath} -> ${version}`);
  } else {
    console.log(`[sync-version] ${relativePath} already at ${version}`);
  }
}

patch('package.json', /("version"\s*:\s*")[^"]*(")/, `$1${version}$2`);

// Anchor on `[package]` so dependency tables that also have `version = "..."`
// entries are left alone.
patch(
  'src-tauri/Cargo.toml',
  /(^\[package\][\s\S]*?^version\s*=\s*")[^"]*(")/m,
  `$1${version}$2`,
);

patch('src-tauri/tauri.conf.json', /("version"\s*:\s*")[^"]*(")/, `$1${version}$2`);

console.log(`[sync-version] done: ${version}`);
