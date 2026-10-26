#!/usr/bin/env node
// Builds the engine's two binaries and publishes each as a release on the
// runtime fork:
//
// - the wasm32 payload with the Rust core linked in, as `runtime-payloads-<hash>`,
//   where packages/engine/runtime/scripts/fetch-payload.sh looks for it;
// - the iOS EmbedPDFBindings.xcframework.zip, as `apple-engine-<hash>`, which
//   platforms/apple/EmbedPDF/Package.swift downloads.
//
// Each half has its own hash and is skipped when its release already exists,
// so a change that only reaches the iOS engine publishes only that one.
//
// Upstream's Build PDF Runtime workflow publishes payloads for PDFium alone.
// This branch links crates/epdf-wasm into the same module and builds the fork
// at runtime-src's commit for wasm32-eh, which no release publishes, so its
// payloads have hashes upstream never has.
//
//   node scripts/publish-engine-payload.mjs [--dry-run] [--only wasm|apple]
//
// --dry-run builds, verifies and packs each payload and prints the release it
// would create, without creating it or touching Package.swift. --only runs one
// half.
//
// The Apple half rewrites engineRelease and engineChecksum in Package.swift
// after it publishes, and leaves that edit for you to commit: the release names
// the commit it was built from, which can't also contain its checksum.
//
// The fork build and emsdk are cached under perf-compare/.work (PERF_WORK_DIR
// overrides), the same cache the perf harness uses: a cached fork makes a
// publish a few minutes, a cold one several hours. Needs cargo, gh (logged in
// with write access to the fork) and a committed, clean tree, because the
// release names the commit it was built from. The Apple half also needs Xcode
// and an iOS simulator; EMBEDPDF_TEST_DESTINATION overrides the default one
// when two simulators share its name.

import { execFileSync, spawnSync } from 'node:child_process';
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
const apple = path.join(repo, 'platforms/apple');
const manifest = path.join(apple, 'EmbedPDF/Package.swift');
const dryRun = process.argv.includes('--dry-run');
const onlyAt = process.argv.indexOf('--only');
const only = onlyAt === -1 ? null : process.argv[onlyAt + 1];
if (only !== null && only !== 'wasm' && only !== 'apple') {
  console.error('--only takes wasm or apple');
  process.exit(64);
}

const run = (cmd, args, opts = {}) =>
  execFileSync(cmd, args, { cwd: repo, encoding: 'utf8', ...opts }).trim();
const sh = (cmd, cwd = repo) => {
  console.log(`  $ ${cmd}`);
  execFileSync('bash', ['-euo', 'pipefail', '-c', cmd], { cwd, stdio: 'inherit' });
};
const sha256 = (file) => createHash('sha256').update(readFileSync(file)).digest('hex');
const releaseExists = (tag) => {
  try {
    run('gh', ['release', 'view', tag, '--repo', RELEASE_REPO], { stdio: 'pipe' });
    return true;
  } catch {
    return false;
  }
};

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

// Each hash script lists the crates its binary links by hand. A crate missing
// from a list would change that binary without changing its hash.
const metadata = JSON.parse(
  run('cargo', ['metadata', '--format-version', '1'], { maxBuffer: 1 << 28 }),
);
const checkCrates = (root, script, variable) => {
  const local = linkedLocalCrates(metadata, root);
  const listed = listedCrates(readFileSync(script, 'utf8'), variable);
  if (local.join() !== listed.join()) {
    console.error(
      `${variable} in ${path.basename(script)} is (${listed.join(' ')}),\n` +
        `but ${root} links (${local.join(' ')}). Update the list first.`,
    );
    process.exit(1);
  }
};
if (only !== 'apple') {
  checkCrates('epdf-wasm', path.join(runtime, 'scripts/runtime-payload-hash.sh'), 'RUST_CRATES');
}
if (only !== 'wasm') {
  checkCrates('epdf-viewer-host', path.join(apple, 'apple-engine-hash.sh'), 'APPLE_CRATES');
}

const head = run('git', ['rev-parse', 'HEAD']);
const rustc = run('rustc', ['--version']);
if (only !== 'apple') publishWasm();
if (only !== 'wasm') publishApple();

function publishWasm() {
  const hash = run('bash', [path.join(runtime, 'scripts/runtime-payload-hash.sh')]);
  const tag = `runtime-payloads-${hash}`;
  if (releaseExists(tag) && !dryRun) {
    console.log(`${tag} is already published on ${RELEASE_REPO}; nothing to do.`);
    return;
  }

  console.log(`building payload ${hash} at ${head.slice(0, 9)}`);
  rmSync(libDir, { recursive: true, force: true });
  const fork = buildFork(repo);
  linkRustCore(repo, fork);

  // verify-rust-core.mjs imports @embedpdf/engine-runtime, which resolves to dist/.
  sh('pnpm --filter @embedpdf/engine-runtime build');
  sh('node packages/engine/runtime/scripts/verify-rust-core.mjs');

  const out = mkdtempSync(path.join(tmpdir(), 'engine-payload-'));
  const archive = path.join(out, ASSET);
  sh(
    `tar -czf ${JSON.stringify(archive)} --exclude .payload-source -C ${JSON.stringify(libDir)} .`,
  );
  const digest = sha256(archive);
  writeFileSync(`${archive}.sha256`, `${digest}  ${ASSET}\n`);

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
}

function publishApple() {
  const ZIP = 'EmbedPDFBindings.xcframework.zip';
  // check-apple-engine.sh hashes the tree and compares it with the pin. Only
  // its verdicts (0, 1, 2) say what to do; anything else is gh, the manifest
  // or the hash failing, and building on a guess could upload over a release.
  const check = spawnSync('bash', [path.join(apple, 'check-apple-engine.sh')], {
    cwd: repo,
    encoding: 'utf8',
  });
  process.stdout.write(check.stdout);
  if (check.status === 0) {
    console.log('Package.swift already pins this engine; skipping the Apple half.');
    return;
  }
  if (check.status !== 1 && check.status !== 2) {
    console.error(
      `check-apple-engine.sh failed (exit ${check.status ?? check.signal}):\n${check.stderr}`,
    );
    process.exit(1);
  }

  const hash = run('bash', [path.join(apple, 'apple-engine-hash.sh')]);
  const tag = `apple-engine-${hash}`;

  // Published but not pinned: the checksum is in the release, so pin it from
  // there. Never upload to a tag that exists.
  if (check.status === 2) {
    const out = mkdtempSync(path.join(tmpdir(), 'apple-engine-'));
    run('gh', [
      'release',
      'download',
      tag,
      '--repo',
      RELEASE_REPO,
      '--pattern',
      '*.sha256',
      '--dir',
      out,
    ]);
    const line = readFileSync(path.join(out, `${ZIP}.sha256`), 'utf8').trim();
    rmSync(out, { recursive: true, force: true });
    const digest = line.match(new RegExp(`^([0-9a-f]{64})  ${ZIP.replaceAll('.', '\\.')}$`))?.[1];
    if (!digest) {
      console.error(`${tag}'s ${ZIP}.sha256 doesn't hold a digest: ${line}`);
      process.exit(1);
    }
    pinEngine(tag, digest);
    return;
  }

  console.log(`building ${tag} at ${head.slice(0, 9)}`);
  const device = buildFork(repo, 'ios-arm64');
  const simulator = buildFork(repo, 'ios-sim-arm64');
  // Both targets stage under the same root, as <target>/lib/libembedpdf.a.
  sh(
    `LIB_ROOT=${JSON.stringify(path.dirname(device.staging))} bash platforms/apple/build-xcframework.sh`,
  );
  console.log('  $ bash platforms/apple/package-engine-framework.sh');
  const packaged = execFileSync('bash', [path.join(apple, 'package-engine-framework.sh')], {
    cwd: repo,
    encoding: 'utf8',
    stdio: ['ignore', 'pipe', 'inherit'],
  }).trim();
  process.stdout.write(`${packaged}\n`);
  const checksum = packaged.split('\n').at(-1);
  const zip = path.join(apple, '.build/publish', ZIP);
  // SwiftPM's checksum is the zip's sha256, so the .sha256 asset can carry it
  // in the same form as the wasm payload's.
  if (checksum !== sha256(zip)) {
    console.error(
      `swift package compute-checksum gave ${checksum}, but ${ZIP} hashes to ${sha256(zip)}`,
    );
    process.exit(1);
  }
  writeFileSync(`${zip}.sha256`, `${checksum}  ${ZIP}\n`);

  // The framework as published, which only this build exercises: a manifest
  // evaluated in a fresh derived data directory, so it reads the variable.
  const derived = mkdtempSync(path.join(tmpdir(), 'apple-engine-dd-'));
  const destination =
    process.env.EMBEDPDF_TEST_DESTINATION ?? 'platform=iOS Simulator,name=iPhone 17';
  sh(
    `EMBEDPDF_PREBUILT_ENGINE=local xcodebuild test -scheme EmbedPDF-Package ` +
      `-destination ${JSON.stringify(destination)} -derivedDataPath ${JSON.stringify(derived)}`,
    path.join(apple, 'EmbedPDF'),
  );
  rmSync(derived, { recursive: true, force: true });

  const xcode = run('xcodebuild', ['-version']).split('\n')[0];
  const notes = [
    `iOS engine for platforms/apple/EmbedPDF/Package.swift: the uniffi Swift, Rust core and PDFium in one static framework.`,
    '',
    `- Engine hash: \`${hash}\``,
    `- Viewer commit: \`${head}\``,
    `- Fork commit (runtime-src): \`${device.sha}\`, built for ios-arm64 and ios-sim-arm64`,
    `- ios-arm64 libembedpdf.a sha256: \`${device.libSha256}\``,
    `- ios-sim-arm64 libembedpdf.a sha256: \`${simulator.libSha256}\``,
    `- ${rustc}`,
    `- ${xcode}`,
    `- SwiftPM checksum (${ZIP} sha256): \`${checksum}\``,
  ].join('\n');
  const args = [
    'release',
    'create',
    tag,
    zip,
    `${zip}.sha256`,
    '--repo',
    RELEASE_REPO,
    // As for the wasm payload: the fork commit both slices' PDFium came from.
    '--target',
    device.sha,
    '--title',
    tag,
    '--notes',
    notes,
    '--prerelease',
  ];
  if (dryRun) {
    console.log(`\n--dry-run: would run gh ${args.map((a) => JSON.stringify(a)).join(' ')}`);
  } else {
    execFileSync('gh', args, { cwd: repo, stdio: 'inherit' });
  }
  pinEngine(tag, checksum);
  console.log(`${dryRun ? 'built' : 'published'} ${tag}`);
}

// Points Package.swift's binary target at a published release. Each constant
// sits on its own line, and a second match would mean the manifest changed
// shape under this script.
function pinEngine(tag, checksum) {
  let text = readFileSync(manifest, 'utf8');
  for (const [name, value] of [
    ['engineRelease', tag],
    ['engineChecksum', checksum],
  ]) {
    const line = new RegExp(`^let ${name} = ".*"$`, 'gm');
    const count = text.match(line)?.length ?? 0;
    if (count !== 1) {
      console.error(`expected one \`let ${name} = "\` line in Package.swift, found ${count}`);
      process.exit(1);
    }
    text = text.replace(line, `let ${name} = "${value}"`);
  }
  if (dryRun) {
    console.log(`--dry-run: would pin Package.swift to ${tag}, checksum ${checksum}`);
    return;
  }
  writeFileSync(manifest, text);
  console.log('commit platforms/apple/EmbedPDF/Package.swift');
}
