import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { loadConfig } from '../lib/config.mjs';
import { assertPinned, workDir, worktreePath } from './materialize.mjs';

const self = readFileSync(fileURLToPath(import.meta.url));
const repo = fileURLToPath(new URL('../..', import.meta.url));
const sha256 = (buf) => createHash('sha256').update(buf).digest('hex');

function sh(cmd, cwd, env = {}) {
  console.log(`  $ ${cmd}`);
  execFileSync('bash', ['-euo', 'pipefail', '-c', cmd], {
    cwd,
    stdio: 'inherit',
    env: { ...process.env, ...env },
  });
}

function ensureEmsdk(version) {
  const root = path.join(workDir(), 'emsdk');
  if (!existsSync(root)) sh(`git clone --depth 1 https://github.com/emscripten-core/emsdk.git ${root}`, workDir());
  sh(`./emsdk install ${version} && ./emsdk activate ${version}`, root);
  return path.join(root, 'emsdk_env.sh');
}

const RUNTIME_SRC = 'packages/engine/runtime/runtime-src';
const FORK_TARGET = 'wasm32-eh';
// Bump to rebuild every cached fork archive. Not tied to this file's hash,
// because a fork build takes minutes and most edits here don't touch it.
const FORK_RECIPE = 3;
// The Emscripten the wasm32-eh target needs, for both the fork build and the
// Rust-core link. Hard-coded, unlike every other emsdk version here, because
// no branch declares it anywhere a script can read: compile.esm.sh passes
// -sWASM_LEGACY_EXCEPTIONS=0, a setting that first exists in 4.0.0
// (docs/research/wasm-eh-encoding.md), runtime-src's
// patches/embedpdf-runtime/wasm/config-eh.gn is written against 4.0.0, and
// tooling/engine-baseline/results/geometry-service-boundary.json records
// emcc 4.0.0. build-engine-runtime.yml's EMSDK_VERSION is for wasm32.
const EH_EMSDK_VERSION = '4.0.0';
const git = (args, cwd) => execFileSync('git', args, { cwd, encoding: 'utf8' }).trim();

// gclient-managed dependencies of a synced runtime-src: every ignored path
// except build output. `git status --ignored` lists each as one directory.
function syncedDeps(src) {
  return git(['status', '--porcelain', '--ignored', '--untracked-files=normal'], src)
    .split('\n')
    .filter((line) => line.startsWith('!! '))
    .map((line) => line.slice(3).replace(/\/$/, ''))
    .filter((p) => p !== 'out' && !p.startsWith('out/'));
}

// Copies the host checkout's synced dependencies into a fresh fork checkout,
// as APFS clones where the filesystem supports them. Returns false when the
// host has none or they were synced for a different DEPS file.
function seedDeps(src) {
  const host = path.join(repo, RUNTIME_SRC);
  if (!existsSync(path.join(host, 'third_party/llvm-build'))) return false;
  const depsSha = (dir) => git(['hash-object', path.join(dir, 'DEPS')], dir);
  const stamp = path.join(host, '.embedpdf-runtime/deps-sync.stamp');
  if (!existsSync(stamp) || !readFileSync(stamp, 'utf8').includes(`deps_sha=${depsSha(src)}\n`)) return false;
  console.log(`  seeding fork dependencies from ${host}`);
  for (const dep of syncedDeps(host)) {
    const to = path.join(src, dep);
    rmSync(to, { recursive: true, force: true });
    mkdirSync(path.dirname(to), { recursive: true });
    sh(`cp -cR ${JSON.stringify(path.join(host, dep))} ${JSON.stringify(to)} 2>/dev/null || cp -R ${JSON.stringify(path.join(host, dep))} ${JSON.stringify(to)}`, src);
  }
  return true;
}

// Builds the PDFium fork at the worktree's recorded runtime-src commit, for
// the wasm32-eh target the Rust engine core links against. No fork release
// publishes wasm32-eh yet (engine-runtime-build.json marks it pending), and
// the Rust core calls fork functions newer than the published wasm32 archive,
// so the Rust-core recipes can't use a published archive. Cached by commit.
function buildFork(dir) {
  const [, , sha] = git(['ls-tree', 'HEAD', RUNTIME_SRC], dir).split(/\s+/);
  const root = path.join(workDir(), 'fork', sha);
  const src = path.join(root, 'runtime-src');
  const stampFile = path.join(root, `${FORK_TARGET}.json`);
  if (existsSync(stampFile)) {
    const stamp = JSON.parse(readFileSync(stampFile, 'utf8'));
    if (stamp.recipe === FORK_RECIPE && existsSync(stamp.staging)) return stamp;
  }
  console.log(`  building fork ${sha.slice(0, 9)} for ${FORK_TARGET}`);
  const seededMarker = path.join(root, 'seeded');
  if (!existsSync(path.join(src, '.git'))) {
    rmSync(root, { recursive: true, force: true });
    mkdirSync(root, { recursive: true });
    // The worktree's submodule already has the commit, including one that only
    // exists in a fork the host fetched.
    git(['clone', '--quiet', '--no-checkout', path.join(dir, RUNTIME_SRC), src], root);
    git(['checkout', '--quiet', '--detach', sha], src);
    git(['remote', 'set-url', 'origin', 'https://github.com/embedpdf/runtime.git'], src);
    if (seedDeps(src)) writeFileSync(seededMarker, '');
  }
  // Its own depot_tools, as the fork's release workflow does, rather than the
  // copy under third_party/, which gclient syncs but never bootstraps.
  const depotTools = path.join(workDir(), 'depot_tools');
  if (!existsSync(path.join(depotTools, 'python3_bin_reldir.txt'))) {
    if (!existsSync(depotTools)) {
      sh(`git clone --depth 1 https://chromium.googlesource.com/chromium/tools/depot_tools.git ${depotTools}`, workDir());
    }
    sh('./ensure_bootstrap', depotTools);
  }
  // Recorded on disk so a rerun after a failed build still skips the resync.
  const seeded = existsSync(seededMarker);
  // The fork's GN toolchain compiles with //third_party/emsdk and nothing else
  // (build/toolchain/wasm/BUILD.gn), so the version goes there, as the fork's
  // release workflow puts it.
  const forkEmsdk = path.join(src, 'third_party/emsdk');
  if (!existsSync(forkEmsdk)) sh(`git clone https://github.com/emscripten-core/emsdk.git ${forkEmsdk}`, src);
  sh(`./emsdk install ${EH_EMSDK_VERSION} && ./emsdk activate ${EH_EMSDK_VERSION}`, forkEmsdk);
  const env = path.join(forkEmsdk, 'emsdk_env.sh');
  const artifacts = path.join(root, 'artifacts');
  // Ninja doesn't track the compiler's version, so a build after a toolchain
  // change would reuse objects the old one produced.
  rmSync(path.join(src, 'out/embedpdf-runtime', FORK_TARGET), { recursive: true, force: true });
  sh(
    `source ${env} && scripts/embedpdf-runtime/build-target.sh ${FORK_TARGET} && scripts/embedpdf-runtime/package-target.sh ${FORK_TARGET}`,
    src,
    {
      PATH: `${depotTools}:${process.env.PATH}`,
      DEPOT_TOOLS_UPDATE: '0',
      PDF_RUNTIME_TARGET_OS_LIST: 'emscripten',
      // Seeded deps match this commit's DEPS but were synced for another
      // target_os, which would otherwise force a full gclient resync.
      PDF_RUNTIME_SYNC: seeded ? 'skip' : 'auto',
      PDF_RUNTIME_ARTIFACT_DIR: artifacts,
    },
  );
  const staging = path.join(src, 'out/embedpdf-runtime-staging', FORK_TARGET);
  const lib = path.join(staging, 'lib/libembedpdf.a');
  const stamp = { recipe: FORK_RECIPE, sha, seeded, staging, libSha256: sha256(readFileSync(lib)) };
  writeFileSync(stampFile, JSON.stringify(stamp, null, 2));
  return stamp;
}

// The viewer half of `build:target wasm32` with the fork archive swapped for
// the one buildFork made: the steps of packages/engine/runtime/scripts/
// build-target.sh after its fetch, with PDF_RUNTIME_TARGET=wasm32-eh, as
// tooling/engine-conformance/README.md documents for the Rust core.
function linkRustCore(dir, fork) {
  const runtime = path.join(dir, 'packages/engine/runtime');
  const libDir = path.join(runtime, 'build/libpdfium', FORK_TARGET);
  rmSync(libDir, { recursive: true, force: true });
  mkdirSync(path.dirname(libDir), { recursive: true });
  sh(`cp -R ${fork.staging} ${libDir}`, dir);
  const env = ensureEmsdk(EH_EMSDK_VERSION);
  const staticlib = path.join(dir, 'target/wasm32-unknown-emscripten/release/libepdf_wasm.a');
  sh(`source ${env} && cargo build -p epdf-wasm --target wasm32-unknown-emscripten --release`, dir);
  sh(
    `source ${env} && node build/generate-functions.mjs ${FORK_TARGET} && node build/generate-runtime-methods.mjs build/generated && bash build/compile.esm.sh`,
    runtime,
    { EMBEDPDF_RUST_STATICLIB: staticlib, PDF_RUNTIME_TARGET: FORK_TARGET },
  );
}

const wasmPath = (dir) => path.join(dir, 'packages/engine/runtime/npm/wasm32/lib/embedpdf.wasm');

function payload(dir, { allowLocal }) {
  // fetch-payload.sh leaves any unmarked payload alone and exits 0, so a
  // previous local build would otherwise be reported as fetched.
  rmSync(path.dirname(wasmPath(dir)), { recursive: true, force: true });
  try {
    sh('pnpm --filter @embedpdf/engine-runtime fetch:payload', dir);
    return { source: 'fetched' };
  } catch (error) {
    if (!allowLocal) throw error;
    console.log('  no published payload for this tree; building it locally');
    const fork = buildFork(dir);
    linkRustCore(dir, fork);
    return { source: 'built', fork: { sha: fork.sha, target: FORK_TARGET, libSha256: fork.libSha256 } };
  }
}

export function buildWorktree(id, config) {
  const w = config.worktrees[id];
  const dir = worktreePath(id);
  // Before the cache check, so a stamp is never reused for edited source.
  assertPinned(id, w.sha);
  const key = sha256(Buffer.concat([Buffer.from(w.sha + w.recipe), self]));
  const stampFile = path.join(workDir(), 'stamps', `${id}.json`);
  if (existsSync(stampFile)) {
    const stamp = JSON.parse(readFileSync(stampFile, 'utf8'));
    if (stamp.key === key && existsSync(stamp.engineWasm)) return stamp;
  }
  console.log(`building ${id} (${w.recipe})`);
  sh('pnpm install --frozen-lockfile', dir);
  const { source, fork } = payload(dir, { allowLocal: w.recipe !== 'typescript' });
  sh('pnpm exec turbo run build --filter=@embedpdf/viewer...', dir);
  // After the build: the script self-imports @embedpdf/engine-runtime, which resolves to dist/.
  let rustCore = false;
  if (w.recipe === 'rust-core') {
    sh('node packages/engine/runtime/scripts/verify-rust-core.mjs', dir);
    rustCore = true;
  }
  if (w.recipe === 'rust-kernel') {
    sh('pnpm --filter @embedpdf/core-wasm build:wasm', dir);
    sh('pnpm bootstrap', path.join(dir, 'examples/rust-kernel-eval'));
  }
  const engineWasm = wasmPath(dir);
  const stamp = { key, source, fork, rustCore, engineWasm, engineWasmSha256: sha256(readFileSync(engineWasm)) };
  mkdirSync(path.dirname(stampFile), { recursive: true });
  writeFileSync(stampFile, JSON.stringify(stamp, null, 2));
  return stamp;
}

if (import.meta.url === `file://${process.argv[1]}`) {
  const config = loadConfig();
  const only = process.argv.slice(2);
  for (const id of Object.keys(config.worktrees)) {
    if (!only.length || only.includes(id)) console.log(buildWorktree(id, config));
  }
}
