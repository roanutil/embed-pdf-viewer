#!/usr/bin/env node
// Builds the wasm32 engine payload with the Rust core linked in and publishes
// it as a `runtime-payloads-<hash>` release on the runtime fork, where
// packages/engine/runtime/scripts/fetch-payload.sh looks for it.
//
// Upstream's Build PDF Runtime workflow publishes payloads for PDFium alone.
// This branch links crates/epdf-wasm into the same module and builds the fork
// at runtime-src's commit for wasm32-eh, which no release publishes, so its
// payloads have hashes upstream never has.
//
//   node scripts/publish-engine-payload.mjs [--dry-run]
//
// --dry-run builds, verifies and packs the payload and prints the release it
// would create, without creating it.
//
// The fork build and emsdk are cached under perf-compare/.work (PERF_WORK_DIR
// overrides), the same cache the perf harness uses: a cached fork makes a
// publish a few minutes, a cold one several hours. Needs cargo, gh (logged in
// with write access to the fork) and a committed, clean tree, because the
// release names the commit it was built from.

import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { buildFork, linkRustCore } from '../perf-compare/scripts/build-arm.mjs';
import { linkedLocalCrates, listedCrates } from './lib/linked-crates.mjs';

const RELEASE_REPO = 'roanutil/embed-pdf-runtime';
const TARGET = 'wasm32';
const ASSET = `engine-runtime-payload-${TARGET}.tar.gz`;

const repo = fileURLToPath(new URL('..', import.meta.url));
const runtime = path.join(repo, 'packages/engine/runtime');
const libDir = path.join(runtime, 'npm', TARGET, 'lib');
const dryRun = process.argv.includes('--dry-run');

const run = (cmd, args, opts = {}) =>
  execFileSync(cmd, args, { cwd: repo, encoding: 'utf8', ...opts }).trim();
const sh = (cmd, cwd = repo) => {
  console.log(`  $ ${cmd}`);
  execFileSync('bash', ['-euo', 'pipefail', '-c', cmd], { cwd, stdio: 'inherit' });
};
const sha256 = (file) => createHash('sha256').update(readFileSync(file)).digest('hex');

// The release names a commit, so that commit has to be what was built.
const dirty = run('git', ['status', '--porcelain', '--untracked-files=no']);
if (dirty) {
  console.error(`Commit or discard these first; the release records HEAD:\n${dirty}`);
  process.exit(1);
}

// buildFork clones the fork from the runtime-src checkout. Its commit may exist
// only on the fork, which `git submodule update` won't fetch from upstream.
// Untrimmed: the status's first column is a space when the commit matches.
const status = execFileSync('git', ['submodule', 'status', 'packages/engine/runtime/runtime-src'], {
  cwd: repo,
  encoding: 'utf8',
});
if (!status.startsWith(' ')) {
  const sha = status.slice(1).split(' ')[0];
  console.error(
    `runtime-src isn't checked out at ${sha.slice(0, 9)}. From the repo root:\n` +
      `  git submodule init packages/engine/runtime/runtime-src\n` +
      `  git config submodule.packages/engine/runtime/runtime-src.url https://github.com/${RELEASE_REPO}.git\n` +
      `  git submodule update packages/engine/runtime/runtime-src`,
  );
  process.exit(1);
}

// runtime-payload-hash.sh lists the crates epdf-wasm links by hand. A crate
// missing from that list would change the payload without changing the hash.
const metadata = JSON.parse(
  run('cargo', ['metadata', '--format-version', '1'], { maxBuffer: 1 << 28 }),
);
const local = linkedLocalCrates(metadata, 'epdf-wasm');
const hashScript = readFileSync(path.join(runtime, 'scripts/runtime-payload-hash.sh'), 'utf8');
const listed = listedCrates(hashScript, 'RUST_CRATES');
if (local.join() !== listed.join()) {
  console.error(
    `RUST_CRATES in runtime-payload-hash.sh is (${listed.join(' ')}),\n` +
      `but epdf-wasm links (${local.join(' ')}). Update the list first.`,
  );
  process.exit(1);
}

const hash = run('bash', [path.join(runtime, 'scripts/runtime-payload-hash.sh')]);
const tag = `runtime-payloads-${hash}`;
const exists = (() => {
  try {
    run('gh', ['release', 'view', tag, '--repo', RELEASE_REPO], { stdio: 'pipe' });
    return true;
  } catch {
    return false;
  }
})();
if (exists && !dryRun) {
  console.log(`${tag} is already published on ${RELEASE_REPO}; nothing to do.`);
  process.exit(0);
}

const head = run('git', ['rev-parse', 'HEAD']);
console.log(`building payload ${hash} at ${head.slice(0, 9)}`);
rmSync(libDir, { recursive: true, force: true });
const fork = buildFork(repo);
linkRustCore(repo, fork);

// verify-rust-core.mjs imports @embedpdf/engine-runtime, which resolves to dist/.
sh('pnpm --filter @embedpdf/engine-runtime build');
sh('node packages/engine/runtime/scripts/verify-rust-core.mjs');

const out = mkdtempSync(path.join(tmpdir(), 'engine-payload-'));
const archive = path.join(out, ASSET);
sh(`tar -czf ${JSON.stringify(archive)} --exclude .payload-source -C ${JSON.stringify(libDir)} .`);
const digest = sha256(archive);
writeFileSync(`${archive}.sha256`, `${digest}  ${ASSET}\n`);

const rustc = run('rustc', ['--version']);
const notes = [
  `wasm32 engine payload with the Rust core linked in, for fetch-payload.sh.`,
  '',
  `- Payload hash: \`${hash}\``,
  `- Viewer commit: \`${head}\``,
  `- Fork commit (runtime-src): \`${fork.sha}\`, built for wasm32-eh`,
  `- libembedpdf.a sha256: \`${fork.libSha256}\``,
  `- ${rustc}`,
  `- ${ASSET} sha256: \`${digest}\``,
].join('\n');

const args = [
  'release',
  'create',
  tag,
  archive,
  `${archive}.sha256`,
  '--repo',
  RELEASE_REPO,
  // The fork commit the payload's PDFium came from, which the fork has.
  '--target',
  fork.sha,
  '--title',
  tag,
  '--notes',
  notes,
  '--prerelease',
];
if (dryRun) {
  console.log(`\n--dry-run: would run gh ${args.map((a) => JSON.stringify(a)).join(' ')}`);
  console.log(`payload left at ${out}`);
} else {
  execFileSync('gh', args, { cwd: repo, stdio: 'inherit' });
  rmSync(out, { recursive: true, force: true });
}
// The local payload now matches the release, so fetch-payload.sh keeps it.
writeFileSync(path.join(libDir, '.payload-source'), hash);
console.log(`${dryRun ? 'built' : 'published'} ${tag}`);
