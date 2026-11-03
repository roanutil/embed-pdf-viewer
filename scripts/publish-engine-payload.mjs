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
// The wasm half checks the linked engine against the committed bindings
// (src/core/pdf-functions.generated.ts), not the ones the header generator
// writes on the way. A fork stamp whose library came from a fork release
// archive (its `source`) may be at another commit than runtime-src, as long as
// the archive's commit is an ancestor of runtime-src and the commits between
// them change only docs/, the fork's tests (scripts/embedpdf-runtime/tests/),
// the BUILD_NEUTRAL_SCRIPTS, and the PINNED_SCRIPT_BLOBS, each only as its
// recorded pair of blobs at the archive's commit and at the pin
// (scripts/lib/payload-notes.mjs). The emsdk-version.sh pin for
// the target and the target-args.sh gn args, both read at runtime-src, must
// match the archive's emsdk and args.gn. The notes then say so.
//
// Such a stamp is written by hand, at
// <PERF_WORK_DIR>/fork/<runtime-src commit>/wasm32-eh.json, beside the
// archive's contents unpacked into `staging`:
//
//   { "recipe": <build-arm.mjs's FORK_RECIPE>, "sha": "<the archive's fork commit>",
//     "seeded": false, "staging": "<the unpacked archive>",
//     "libSha256": "<sha256 of staging/lib/libembedpdf.a>",
//     "source": { "archive": "<its file name>", "archivePath": "<the .tar.gz>",
//                 "archiveSha256": "<its sha256>", "emsdk": "<the emsdk it was built with>" } }
//
// The publish checks it against its files: it hashes libembedpdf.a and the
// archive again, requires the staging libembedpdf.a and args.gn to be the
// archive's own (read with tar -xzOf), reads the fork commit, and the emsdk
// when the archive records one (emsdkVersion), from the archive's own
// BUILD-METADATA.json, and refuses on any mismatch. An archive without
// emsdkVersion (the 6dfc03697 one) leaves source.emsdk the one stamp field
// taken on trust.
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
import {
  chmodSync,
  cpSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readdirSync,
  readFileSync,
  renameSync,
  rmSync,
  statSync,
  utimesSync,
  writeFileSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { buildFork } from '../perf-compare/scripts/build-arm.mjs';
import { workDir } from '../perf-compare/scripts/materialize.mjs';
import { forkContains } from './lib/fork-ancestry.mjs';
import { linkedLocalCrates, listedCrates } from './lib/linked-crates.mjs';
import { assertGroupRecord, fileDigests } from './lib/publish-guard.mjs';
import {
  acquireRunLock,
  checkedGroupAlive,
  startOwnedStep,
  surveyProcesses,
} from './lib/publish-process.mjs';
import {
  ALL_BOUND_SINCE,
  abiCheckFunctions,
  assertAllBound,
  assertKnownUnbound,
  assertStampMatches,
  committedSignatures,
  PINNED_SCRIPT_BLOBS,
  forkLines,
  payloadNotes,
  releaseVerdict,
} from './lib/payload-notes.mjs';

const RELEASE_REPO = 'roanutil/embed-pdf-runtime';
const TARGET = 'wasm32';
const ASSET = `engine-runtime-payload-${TARGET}.tar.gz`;

const repo = fileURLToPath(new URL('..', import.meta.url));
const runtime = path.join(repo, 'packages/engine/runtime');
const libDir = path.join(runtime, 'npm', TARGET, 'lib');
// Where the payload that was in libDir waits while a new one is checked
// (gitignored). One fixed name, so a killed run's is found and put back.
const libBackup = path.join(runtime, 'npm', TARGET, '.lib-backup');

// libBackup holds what libDir held before a swap: the old payload, or, when there
// was none, nothing but this sentinel, so that a restore removes the new one.
const ABSENT = '.absent';
// Written into libBackup just before `gh release create`: a backup that has it
// may belong to a release that went out, which the next run asks the fork about.
const PUBLISHING = '.publishing';
// Beside it, the sha256 of each file in libDir as it went into the release, so a
// later run keeps libDir as published only while it is still that payload.
const PUBLISHED_DIGESTS = '.publishing-digests.json';
// The process group of the step running while the payload is swapped in, as
// { pgid, start, args } (its leader's birth identity and diagnostic command). A run that finds
// that group still alive refuses: that build may still write into libDir.
const PGID = '.pgid';
const MARKERS = [PUBLISHING, PUBLISHED_DIGESTS, PGID];
// One publish of the wasm half at a time: { pid, start } of the run holding it.
const runLock = path.join(runtime, 'npm', TARGET, '.publish.lock');

// What libDir held before the swap, back in libDir: the old payload, or no
// payload at all. Synchronous, for the signal handlers.
function restorePayload() {
  if (!existsSync(libBackup)) return;
  rmSync(libDir, { recursive: true, force: true });
  for (const marker of MARKERS) rmSync(path.join(libBackup, marker), { force: true });
  if (existsSync(path.join(libBackup, ABSENT))) rmSync(libBackup, { recursive: true, force: true });
  else renameSync(libBackup, libDir);
}

// Every file in `dir` readable by all and writable by its owner (0644), whatever
// the link and the copies left (a container build through a bind mount can leave
// 0200), so the archive ships files anyone can read.
function normalizeModes(dir) {
  for (const name of readdirSync(dir)) {
    const file = path.join(dir, name);
    if (statSync(file).isFile()) chmodSync(file, 0o644);
  }
}

// Unknown process inspection never counts as proof that an owner is gone.
function processRows() {
  return surveyProcesses();
}

function takeRunLock() {
  const release = acquireRunLock(runLock);
  process.on('exit', () => {
    try {
      release();
    } catch (error) {
      console.error(`publish lock left for verified recovery: ${error.message}`);
    }
  });
}

const apple = path.join(repo, 'platforms/apple');
const manifest = path.join(apple, 'EmbedPDF/Package.swift');
// The fork target the wasm32 payload links, as build-arm.mjs builds it.
const FORK_TARGET = buildArmConstant('FORK_TARGET');
const dryRun = process.argv.includes('--dry-run');
const onlyAt = process.argv.indexOf('--only');
const only = onlyAt === -1 ? null : process.argv[onlyAt + 1];
if (only !== null && only !== 'wasm' && only !== 'apple') {
  console.error('--only takes wasm or apple');
  process.exit(64);
}

const run = (cmd, args, opts = {}) =>
  execFileSync(cmd, args, { cwd: repo, encoding: 'utf8', ...opts }).trim();
const sh = (cmd, cwd = repo, env = {}) => {
  console.log(`  $ ${cmd}`);
  execFileSync('bash', ['-euo', 'pipefail', '-c', cmd], {
    cwd,
    stdio: 'inherit',
    env: { ...process.env, ...env },
  });
};
// A step settles only once its persistent leader and every owned writer have
// drained. Success, errors and signals share that boundary before payload moves.
let running = null;
let stopping = false;
let releasing = false;
let deferredSignal = null;
const runOwned = async (file, args, cwd = repo, env = {}, orphanPolicy = 'stop') => {
  if (stopping) throw new Error(`not started, the run is stopping: ${file}`);
  const step = startOwnedStep(file, args, {
    cwd,
    env: { ...process.env, ...env },
    orphanPolicy,
    onGroup: (group) => {
      if (existsSync(libBackup)) writeFileSync(path.join(libBackup, PGID), JSON.stringify(group));
    },
  });
  running = step;
  let drained = false;
  try {
    await step.completion;
    drained = true;
  } catch (error) {
    drained = !error.preservePayload;
    throw error;
  } finally {
    if (drained && running === step) running = null;
  }
};
const shAsync = (cmd, cwd = repo, env = {}) => {
  console.log(`  $ ${cmd}`);
  return runOwned('bash', ['-euo', 'pipefail', '-c', cmd], cwd, env);
};
const sha256 = (file) => createHash('sha256').update(readFileSync(file)).digest('hex');
// Release creation shares step ownership, while its caller defers user signals.
const ghAsync = (args) => runOwned('gh', args, repo, {}, 'continue');
// The pack, the same bytes for the same files: names sorted, times, owners and
// modes fixed, and gzip without a name or time. GNU tar does it with flags; with
// bsdtar the files' times are set first and the names listed in order.
function packCommand(dir, archive) {
  const gnu = /GNU tar/.test(spawnSync('tar', ['--version'], { encoding: 'utf8' }).stdout ?? '');
  const names = readdirSync(dir)
    .filter((name) => name !== '.payload-source')
    .sort();
  const quoted = (value) => JSON.stringify(value);
  if (gnu) {
    return (
      `tar --sort=name --mtime=@0 --owner=0 --group=0 --numeric-owner --mode=u=rw,go=r ` +
      `-cf - -C ${quoted(dir)} ${names.map(quoted).join(' ')} | gzip -n > ${quoted(archive)}`
    );
  }
  for (const name of names) utimesSync(path.join(dir, name), 0, 0);
  return (
    `tar --uid 0 --gid 0 --numeric-owner -cf - -C ${quoted(dir)} ${names.map(quoted).join(' ')} ` +
    `| gzip -n > ${quoted(archive)}`
  );
}
// The archive digest a release carries in its .sha256 asset, or null when gh
// can't fetch it.
function releasedArchiveSha256(tag) {
  const out = mkdtempSync(path.join(tmpdir(), 'released-sha256-'));
  try {
    const download = spawnSync(
      'gh',
      [
        'release',
        'download',
        tag,
        '--repo',
        RELEASE_REPO,
        '--pattern',
        `${ASSET}.sha256`,
        '--dir',
        out,
      ],
      { cwd: repo, encoding: 'utf8' },
    );
    if (download.status !== 0) return null;
    const line = readFileSync(path.join(out, `${ASSET}.sha256`), 'utf8').trim();
    return line.match(/^([0-9a-f]{64})\s/)?.[1] ?? null;
  } catch {
    return null;
  } finally {
    rmSync(out, { recursive: true, force: true });
  }
}
// What the fork has under `tag`, by releaseVerdict: only a release that isn't a
// draft and has `asset` and its .sha256 is published. gh's "release not found"
// is missing; any other gh failure (login, network) throws, since the answer
// is unknown and nothing may act on a guess.
const releaseState = (tag, asset) => {
  const view = spawnSync(
    'gh',
    ['release', 'view', tag, '--repo', RELEASE_REPO, '--json', 'assets,isDraft'],
    { cwd: repo, encoding: 'utf8' },
  );
  if (view.status === 0) return releaseVerdict(JSON.parse(view.stdout), asset);
  if (/release not found/i.test(view.stderr ?? '')) return releaseVerdict(null, asset);
  throw new Error(
    `gh couldn't say whether ${tag} exists on ${RELEASE_REPO}: ${(view.stderr || view.error?.message || `exit ${view.status}`).trim()}`,
  );
};

// The release names a commit, so that commit has to be what was built.
const dirty = run('git', ['status', '--porcelain', '--untracked-files=no']);
if (dirty) {
  console.error(`Commit or discard these first; the release records HEAD:\n${dirty}`);
  // The header generator rewrites the committed bindings until the link puts
  // them back, so a run killed in between leaves them changed.
  if (dirty.includes('pdf-functions.generated.ts')) {
    console.error(
      'pdf-functions.generated.ts is what the header generator wrote in a run that was stopped; ' +
        'restore it with git checkout -- packages/engine/runtime/src/core/pdf-functions.generated.ts',
    );
  }
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
if (only !== 'apple') await publishWasm();
if (only !== 'wasm') publishApple();

async function publishWasm() {
  takeRunLock();
  // A run killed while its new payload was in place leaves what was there before
  // in libBackup. One killed after its release went out leaves the new payload as
  // published: keep it. Any other is an interrupted swap: put the old state back.
  if (existsSync(libBackup)) {
    // Its build may still be running (a SIGKILL'd node, a dropped terminal before
    // the SIGHUP handler): leave everything until it ends.
    const pgidFile = path.join(libBackup, PGID);
    let group = null;
    if (existsSync(pgidFile)) {
      try {
        group = assertGroupRecord(JSON.parse(readFileSync(pgidFile, 'utf8')));
      } catch (error) {
        throw new Error(
          `${path.relative(repo, pgidFile)} has unknown ownership; verify the previous build has stopped before changing it: ${error.message}`,
        );
      }
    }
    if (checkedGroupAlive(processRows(), group)) {
      console.error(
        `a previous publish's build is still running: pgid ${group.pgid} (${group.args} …, ` +
          `started ${group.start}). Wait for it to end. If this is not the publish, remove ` +
          `${path.relative(repo, pgidFile)} and rerun.`,
      );
      process.exit(1);
    }
    const marker = path.join(libBackup, PUBLISHING);
    let publishing = null;
    try {
      publishing = existsSync(marker) ? JSON.parse(readFileSync(marker, 'utf8')) : null;
    } catch {}
    const tag = publishing?.tag ?? null;
    let state = null;
    if (tag) {
      try {
        state = releaseState(tag, ASSET);
      } catch (error) {
        // Unknown: keep the backup and its marker for a run that can ask.
        console.error(`${error.message}\n${path.relative(repo, libBackup)} is left as it is.`);
        process.exit(1);
      }
    }
    if (tag && state === 'published') {
      // Keep libDir as that payload only if it is still what went into the release.
      const digestsFile = path.join(libBackup, PUBLISHED_DIGESTS);
      const recorded = existsSync(digestsFile) ? readFileSync(digestsFile, 'utf8') : null;
      const now = existsSync(libDir) ? JSON.stringify(fileDigests(libDir)) : null;
      if (!recorded || recorded !== now) {
        console.error(
          `${tag} is published, but ${path.relative(repo, libDir)} isn't the payload that went into ` +
            `it. Fetch the published payload (fetch-payload.sh) or restore by hand; ` +
            `${path.relative(repo, libBackup)} is left as it is.`,
        );
        process.exit(1);
      }
      // And the release must carry the archive this run packed, by its .sha256 asset.
      const released = releasedArchiveSha256(tag);
      if (released !== publishing.archiveSha256) {
        console.error(
          `${tag} is published with an archive that hashes to ${released}, not the ` +
            `${publishing.archiveSha256} this run packed; ${path.relative(repo, libBackup)} is left as it is.`,
        );
        process.exit(1);
      }
      writeFileSync(path.join(libDir, '.payload-source'), tag.replace(/^runtime-payloads-/, ''));
      rmSync(libBackup, { recursive: true, force: true });
      console.error(
        `A previous publish of ${tag} was stopped after its release went out; ` +
          `${path.relative(repo, libDir)} is kept as that payload. Run the publish again.`,
      );
    } else {
      restorePayload();
      console.error(
        `A previous publish was stopped while its payload was swapped in; ${path.relative(repo, libDir)} ` +
          `is put back as it was. Run the publish again.`,
      );
    }
    process.exit(1);
  }
  const hash = run('bash', [path.join(runtime, 'scripts/runtime-payload-hash.sh')]);
  const tag = `runtime-payloads-${hash}`;
  let state = null;
  if (!dryRun) {
    try {
      state = releaseState(tag, ASSET);
    } catch (error) {
      console.error(error.message);
      process.exit(1);
    }
  }
  if (state === 'published') {
    console.log(`${tag} is already published on ${RELEASE_REPO}; nothing to do.`);
    return;
  }
  if (state === 'incomplete') {
    console.error(
      `release ${tag} on ${RELEASE_REPO} is incomplete (a draft, or ${ASSET} or its .sha256 ` +
        `missing); finish or delete it before publishing again.`,
    );
    process.exit(1);
  }

  console.log(`building payload ${hash} at ${head.slice(0, 9)}`);
  // Every check that can refuse runs before the tree changes: the stamp, the
  // archive, the delta to the pin, the known unbound bindings. The new payload
  // links into a directory of its own and replaces npm/wasm32/lib only once it
  // has passed the postlink check. The old one is moved aside first, and put
  // back if anything after the swap throws (the package build, verify-rust-core,
  // the pack, the release), so a failed run leaves the payload it found.
  const fork = buildFork(repo);
  assertStampMatches(fork, stampFacts(fork), FORK_TARGET);
  const runtimeSrc = runtimeSrcCommit();
  // Before anything links: a library that doesn't match the pin stops here.
  const delta = fork.sha === runtimeSrc ? undefined : forkDelta(fork, runtimeSrc);
  forkLines({ fork, runtimeSrc, target: FORK_TARGET, delta });
  const { unbound, staged } = linkAgainstCommittedBindings(fork);
  // While the new payload is in place unverified, a SIGINT or SIGTERM puts the old
  // one back before exiting; a kill that can't be caught leaves the backup, which
  // the next run restores before anything else.
  // The temp dirs this half made, removed on the signal path too.
  const temps = new Set([staged]);
  let preserveTemps = false;
  const onSignal = async (signal) => {
    if (releasing) {
      // gh is uploading: stopping it could leave a release half made.
      if (!deferredSignal) {
        deferredSignal = signal;
        console.error(`${signal}: the release step is not interruptible; stopping once it ends.`);
      }
      return;
    }
    if (stopping) return;
    stopping = true;
    try {
      if (running) await running.stop(signal);
    } catch (error) {
      preserveTemps = true;
      console.error(
        `${signal}: ${error.message}; the payload, backup, process record and temporary directories are left as they are.`,
      );
      process.exit(1);
    }
    restorePayload();
    for (const dir of temps) rmSync(dir, { recursive: true, force: true });
    console.error(
      `${signal}: ${path.relative(repo, libDir)} put back as it was; nothing published.`,
    );
    process.exit({ SIGHUP: 129, SIGINT: 130 }[signal] ?? 143);
  };
  try {
    rmSync(libBackup, { recursive: true, force: true });
    if (existsSync(libDir)) renameSync(libDir, libBackup);
    else {
      mkdirSync(libBackup, { recursive: true });
      writeFileSync(path.join(libBackup, ABSENT), '');
    }
    process.on('SIGHUP', onSignal);
    process.on('SIGINT', onSignal);
    process.on('SIGTERM', onSignal);
    try {
      cpSync(staged, libDir, { recursive: true });
      normalizeModes(libDir);
      await publishLinked();
    } catch (error) {
      // A step stopped by onSignal: the handler restores, cleans up and exits.
      if (stopping) await new Promise(() => {});
      if (error.preservePayload) {
        preserveTemps = true;
        console.error(
          `${error.message}; the payload, backup, process record and temporary directories are left as they are.`,
        );
        throw error;
      }
      restorePayload();
      throw error;
    } finally {
      process.off('SIGHUP', onSignal);
      process.off('SIGINT', onSignal);
      process.off('SIGTERM', onSignal);
    }
    rmSync(libBackup, { recursive: true, force: true });
  } finally {
    if (!preserveTemps) rmSync(staged, { recursive: true, force: true });
  }
  if (deferredSignal) {
    console.error(`${deferredSignal}: the release step ended; ${tag} is published, stopping now.`);
    process.exit({ SIGHUP: 129, SIGINT: 130 }[deferredSignal] ?? 143);
  }

  async function publishLinked() {
    // Once a signal handler is stopping the run, nothing more happens here: the
    // handler restores and exits.
    const parkIfStopping = async () => {
      if (stopping) await new Promise(() => {});
    };
    // verify-rust-core.mjs imports @embedpdf/engine-runtime, which resolves to dist/.
    await shAsync('pnpm --filter @embedpdf/engine-runtime build');
    await parkIfStopping();
    await shAsync('node packages/engine/runtime/scripts/verify-rust-core.mjs');
    await parkIfStopping();

    const out = mkdtempSync(path.join(tmpdir(), 'engine-payload-'));
    temps.add(out);
    const archive = path.join(out, ASSET);
    // The package build writes into libDir too; its files get the same modes.
    normalizeModes(libDir);
    await shAsync(packCommand(libDir, archive));
    await parkIfStopping();
    const digest = sha256(archive);
    writeFileSync(`${archive}.sha256`, `${digest}  ${ASSET}\n`);

    const notes = payloadNotes({
      hash,
      head,
      fork,
      runtimeSrc,
      target: FORK_TARGET,
      delta,
      rustc,
      asset: ASSET,
      digest,
      unbound,
    });
    if (dryRun) console.log(`\n--dry-run: the release notes would read:\n${notes}\n`);

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
      await parkIfStopping();
      // From here a kill may come after the release went out; the next run asks.
      writeFileSync(path.join(libBackup, PUBLISHED_DIGESTS), JSON.stringify(fileDigests(libDir)));
      writeFileSync(
        path.join(libBackup, PUBLISHING),
        JSON.stringify({ tag, archiveSha256: digest }),
      );
      console.log('  the release step is not interruptible; a signal now is honored once it ends');
      releasing = true;
      try {
        await ghAsync(args);
      } finally {
        releasing = false;
      }
      rmSync(out, { recursive: true, force: true });
    }
    if (dryRun) temps.delete(out);
    await parkIfStopping();
    // Another run must not have touched the swap meanwhile (the run lock keeps it
    // out; this is the last check before the payload is named).
    if (!existsSync(libBackup)) {
      throw new Error(
        `${path.relative(repo, libBackup)} is gone; something else changed ${path.relative(repo, libDir)} during this run, so it isn't marked`,
      );
    }
    // A published payload matches its release, so fetch-payload.sh keeps it. A dry
    // run's matches no release, so its marker names none and fetch-payload.sh
    // fetches the published one over it.
    writeFileSync(path.join(libDir, '.payload-source'), dryRun ? `dry-run-${hash}` : hash);
  }
  console.log(`${dryRun ? 'built' : 'published'} ${tag}`);
}

// build-arm.mjs's constants for the Rust-core link, read as runtime-payload-hash.sh reads
// them. Read, not imported, because build-arm.mjs is an input of the Apple engine's hash.
function buildArmConstant(name) {
  const text = readFileSync(path.join(repo, 'perf-compare/scripts/build-arm.mjs'), 'utf8');
  const value = text.match(new RegExp(`^const ${name} = '([^']+)';$`, 'm'))?.[1];
  if (!value) throw new Error(`no ${name} in perf-compare/scripts/build-arm.mjs`);
  return value;
}

function runtimeSrcCommit() {
  return run('git', ['ls-tree', 'HEAD', 'packages/engine/runtime/runtime-src']).split(/\s+/)[2];
}

// How the pinned runtime-src differs from the fork commit the library was built from: the
// commits between them, the paths they change, and whether the library's args.gn is what
// target-args.sh prints at the pin. payloadNotes refuses any delta that could change the
// library.
function forkDelta(fork, runtimeSrc) {
  const src = path.join(runtime, 'runtime-src');
  const git = (args) => run('git', ['-C', src, ...args]);
  // Counting fork..pin only means something when the fork commit is an ancestor.
  const ancestor =
    spawnSync('git', ['-C', src, 'merge-base', '--is-ancestor', fork.sha, runtimeSrc]).status === 0;
  if (!ancestor) return { ancestor, commits: 0, paths: [], emsdkPin: null, argsMatch: false };
  const commits = Number(git(['rev-list', '--count', `${fork.sha}..${runtimeSrc}`]));
  const paths = git(['diff', '--name-only', fork.sha, runtimeSrc]).split('\n').filter(Boolean);
  const lines = (text) =>
    text
      .split('\n')
      .map((line) => line.trim())
      .filter(Boolean);
  // The fork's scripts as they are at the pin, not in the working tree.
  const atPin = (file) =>
    spawnSync('git', ['-C', src, 'cat-file', '-e', `${runtimeSrc}:${file}`]).status === 0;
  const runAtPin = (file, ...args) =>
    execFileSync('bash', ['-euo', 'pipefail', '-s', '--', ...args], {
      input: git(['show', `${runtimeSrc}:${file}`]),
      encoding: 'utf8',
      env: { ...process.env, PDF_RUNTIME_SOURCE_DIR: src },
    }).trim();
  const pinned = runAtPin('scripts/embedpdf-runtime/target-args.sh', FORK_TARGET);
  const built = readFileSync(path.join(fork.staging, 'args.gn'), 'utf8');
  const argsMatch = lines(pinned).join('\n') === lines(built).join('\n');
  // Nothing for a pin from before emsdk-version.sh, which then can't match.
  const pinScript = 'scripts/embedpdf-runtime/emsdk-version.sh';
  const emsdkPin = atPin(pinScript) ? runAtPin(pinScript, FORK_TARGET) : null;
  // Each pinned script's blob at both ends: the archive's commit and the pin.
  const blobAt = (commit, file) =>
    spawnSync('git', ['-C', src, 'rev-parse', '--verify', '-q', `${commit}:${file}`], {
      encoding: 'utf8',
    }).stdout.trim() || null;
  const blobs = Object.fromEntries(
    Object.keys(PINNED_SCRIPT_BLOBS).map((file) => [
      file,
      { from: blobAt(fork.sha, file), to: blobAt(runtimeSrc, file) },
    ]),
  );
  return { ancestor, commits, paths, emsdkPin, argsMatch, blobs };
}

// What the fork stamp's files really are, for assertStampMatches: the library's
// hash, and for a library from a release archive, the archive's hash and the fork
// commit its own BUILD-METADATA.json names.
function stampFacts(fork) {
  const facts = { libSha256: sha256(path.join(fork.staging, 'lib/libembedpdf.a')) };
  const archive = fork.source?.archivePath;
  if (!fork.source || !archive) return facts;
  if (!existsSync(archive)) {
    return { ...facts, archiveSha256: null, archiveSha: null, archiveEmsdk: null };
  }
  const member = (name, encoding) =>
    execFileSync('tar', ['-xzOf', archive, name], { maxBuffer: 1 << 30, encoding });
  const metadata = JSON.parse(member('./BUILD-METADATA.json', 'utf8'));
  return {
    ...facts,
    archiveSha256: sha256(archive),
    archiveSha: metadata.sha,
    archiveEmsdk: metadata.emsdkVersion ?? null,
    archiveTarget: metadata.target ?? null,
    // The staging files must be the archive's own, not just named by it.
    archiveLibSha256: createHash('sha256').update(member('./lib/libembedpdf.a')).digest('hex'),
    archiveArgs: member('./args.gn', 'utf8'),
    stagingArgs: readFileSync(path.join(fork.staging, 'args.gn'), 'utf8'),
  };
}

// The viewer half of `build:target wasm32`, as build-arm.mjs's linkRustCore runs it, with
// one difference: the postlink check (build/verify-wasm-abi.mjs, run at the end of
// compile.esm.sh) reads the committed bindings. The header generator still runs, for the
// export list, but its regenerated pdf-functions.generated.ts is put back as committed,
// and functions.json is rewritten from the committed signatures. Returns the committed
// bindings this runtime doesn't declare, which the check can't hold it to and the notes
// name.
function linkAgainstCommittedBindings(fork) {
  const pdfium = path.join(runtime, 'build/libpdfium', FORK_TARGET);
  rmSync(pdfium, { recursive: true, force: true });
  mkdirSync(path.dirname(pdfium), { recursive: true });
  sh(`cp -R ${JSON.stringify(fork.staging)} ${JSON.stringify(pdfium)}`);

  const version = buildArmConstant('EH_EMSDK_VERSION');
  const emsdk = path.join(workDir(), 'emsdk');
  if (!existsSync(emsdk)) {
    sh(
      `git clone --depth 1 https://github.com/emscripten-core/emsdk.git ${JSON.stringify(emsdk)}`,
      workDir(),
    );
  }
  sh(`./emsdk install ${version} && ./emsdk activate ${version}`, emsdk);
  const env = `source ${JSON.stringify(path.join(emsdk, 'emsdk_env.sh'))}`;
  const staticlib = path.join(repo, 'target/wasm32-unknown-emscripten/release/libepdf_wasm.a');
  sh(`${env} && cargo build -p epdf-wasm --target wasm32-unknown-emscripten --release`);

  const bindings = path.join(runtime, 'src/core/pdf-functions.generated.ts');
  const committed = readFileSync(bindings, 'utf8');
  try {
    sh(`${env} && node build/generate-functions.mjs ${FORK_TARGET}`, runtime);
  } finally {
    writeFileSync(bindings, committed);
  }
  const functionsJson = path.join(runtime, 'build/generated/functions.json');
  const declared = JSON.parse(readFileSync(functionsJson, 'utf8')).functions.map((fn) => fn.name);
  const { functions, unbound } = abiCheckFunctions(committedSignatures(committed), declared);
  // Only a known gap may be missing; anything else stops before the link.
  assertKnownUnbound(unbound);
  assertAllBound(unbound, {
    containsAllBoundSince: forkContains(
      path.join(runtime, 'runtime-src'),
      fork.sha,
      ALL_BOUND_SINCE,
    ),
  });
  writeFileSync(functionsJson, JSON.stringify({ target: FORK_TARGET, functions }, null, 2) + '\n');
  if (unbound.length > 0) {
    console.warn(
      `  bound in the TypeScript but not declared by this runtime: ${unbound.join(', ')}`,
    );
  }

  // Linked and checked in a directory of its own; publishWasm swaps it in.
  const staged = mkdtempSync(path.join(tmpdir(), 'engine-payload-lib-'));
  try {
    sh(
      `${env} && node build/generate-runtime-methods.mjs build/generated && bash build/compile.esm.sh`,
      runtime,
      {
        EMBEDPDF_RUST_STATICLIB: staticlib,
        PDF_RUNTIME_TARGET: FORK_TARGET,
        PDF_RUNTIME_WASM_OUT_DIR: staged,
      },
    );
  } catch (error) {
    rmSync(staged, { recursive: true, force: true });
    throw error;
  }
  return { unbound, staged };
}

function publishApple() {
  const ZIP = 'EmbedPDFBindings.xcframework.zip';
  // A manifest that pinEngine can't rewrite would only be found after the
  // upload, so find it before anything is built.
  pinEngine('', '', true);
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
    // A release made but not yet filled (a draft, or the zip not uploaded)
    // has no digest worth pinning.
    let release;
    try {
      release = JSON.parse(
        run('gh', ['release', 'view', tag, '--repo', RELEASE_REPO, '--json', 'assets,isDraft']),
      );
    } catch (err) {
      console.error(`couldn't read release ${tag} on ${RELEASE_REPO}: ${err.message}`);
      process.exit(1);
    }
    if (release.isDraft || !release.assets.some((a) => a.name === ZIP)) {
      console.error(
        `release ${tag} on ${RELEASE_REPO} is incomplete (draft: ${release.isDraft}, ` +
          `${ZIP} ${release.assets.some((a) => a.name === ZIP) ? 'present' : 'missing'}); ` +
          `finish or delete it before pinning.`,
      );
      process.exit(1);
    }
    const out = mkdtempSync(path.join(tmpdir(), 'apple-engine-'));
    let line;
    try {
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
      line = readFileSync(path.join(out, `${ZIP}.sha256`), 'utf8').trim();
    } catch (err) {
      console.error(`couldn't download ${ZIP}.sha256 from release ${tag}: ${err.message}`);
      process.exit(1);
    } finally {
      rmSync(out, { recursive: true, force: true });
    }
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
function pinEngine(tag, checksum, validateOnly = false) {
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
  if (validateOnly) return;
  if (dryRun) {
    console.log(`--dry-run: would pin Package.swift to ${tag}, checksum ${checksum}`);
    return;
  }
  writeFileSync(manifest, text);
  console.log('commit platforms/apple/EmbedPDF/Package.swift');
}
