// meta.json: everything about a run that decides whether two runs compare.
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { existsSync, readdirSync, readFileSync } from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
const fixturesDir = path.join(here, '../fixtures');
const require = (await import('node:module')).createRequire(import.meta.url);

export const VIEWPORT = { width: 1280, height: 800 };
export const DEVICE_SCALE_FACTOR = 2;
export const THROTTLE = { desktop: 1, slow: 4 };

const sha256 = (file) => createHash('sha256').update(readFileSync(file)).digest('hex');
const run = (cmd, args) => {
  try {
    return execFileSync(cmd, args, { encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'] }).trim();
  } catch {
    return null;
  }
};

// Recomputes every fixture's hash and throws on a mismatch with fixtures.json.
export function verifyFixtures() {
  const expected = JSON.parse(readFileSync(path.join(fixturesDir, 'fixtures.json'), 'utf8'));
  for (const [name, hash] of Object.entries(expected)) {
    const actual = sha256(path.join(fixturesDir, `${name}.pdf`));
    if (actual !== hash) throw new Error(`fixture ${name}.pdf is ${actual}, fixtures.json says ${hash}`);
  }
  return expected;
}

// The wire kernel's wasm in an arm's built entry, when it has one (the
// plugins recipes): the file that actually ran, which a remote run has even
// though it has no worktrees.
export function kernelWasm(dist) {
  const assets = path.join(dist, 'assets');
  if (!existsSync(assets)) return null;
  const name = readdirSync(assets).find((f) => /^epdf_web_bg.*\.wasm$/.test(f));
  return name ? path.join(assets, name) : null;
}

function powerSource() {
  if (process.platform !== 'darwin') return 'unknown';
  const batt = run('pmset', ['-g', 'batt']) ?? '';
  return /'([^']+)'/.exec(batt)?.[1] ?? 'unknown';
}

export function collectEnv({ config, arms: armIds = config.armOrder, stamps, dists, fixtures, browserVersion, profile, flows }) {
  const arms = {};
  for (const id of armIds) {
    const arm = config.arms[id];
    const stamp = stamps[arm.worktree];
    const kernel = arm.kernel === 'rust' ? kernelWasm(dists[id]) : null;
    arms[id] = {
      ...arm,
      ref: config.worktrees[arm.worktree].ref,
      sha: config.worktrees[arm.worktree].sha,
      payload: stamp.source,
      fork: stamp.fork ?? null,
      engineWasmSha256: stamp.engineWasmSha256,
      kernelWasmSha256: kernel ? sha256(kernel) : null,
    };
  }
  const cpus = os.cpus();
  return {
    // A remote run's copy of the harness isn't a checkout: scripts/remote.mjs
    // passes the commit and dirtiness of the one it was copied from.
    harnessCommit:
      process.env.PERF_HARNESS_COMMIT ?? run('git', ['-C', here, 'rev-parse', 'HEAD']) ?? 'unknown',
    harnessDirty:
      process.env.PERF_HARNESS_DIRTY !== undefined
        ? process.env.PERF_HARNESS_DIRTY === '1'
        : Boolean(run('git', ['-C', path.join(here, '..'), 'status', '--porcelain', '--', '.'])),
    host: os.hostname().split('.')[0],
    arms,
    fixtures,
    browserVersion,
    playwrightVersion: require('playwright-core/package.json').version,
    nodeVersion: process.version,
    os: `${os.type()} ${os.release()} ${os.arch()}`,
    cpuModel: cpus[0]?.model ?? 'unknown',
    cpuCores: cpus.length,
    totalMemoryBytes: os.totalmem(),
    powerSource: powerSource(),
    viewport: VIEWPORT,
    deviceScaleFactor: DEVICE_SCALE_FACTOR,
    profile,
    throttleRate: THROTTLE[profile],
    flows,
  };
}

const COMPARED = ['browserVersion', 'cpuModel', 'fixtures', 'profile', 'viewport', 'deviceScaleFactor', 'flows'];

// The keys that make two runs incomparable. Empty when they compare.
export function compatible(a, b) {
  return COMPARED.filter((key) => JSON.stringify(a[key]) !== JSON.stringify(b[key]));
}

// Byte sizes of an arm's built entry: the engine and wire-kernel wasm, and
// every .js, raw and brotli level 11.
export function distSizes(dist) {
  const { readdirSync, statSync } = require('node:fs');
  const { brotliCompressSync, constants } = require('node:zlib');
  const brotli = (buf) => brotliCompressSync(buf, { params: { [constants.BROTLI_PARAM_QUALITY]: 11 } }).length;
  const files = [];
  const walk = (dir) => {
    for (const name of readdirSync(dir)) {
      const full = path.join(dir, name);
      if (statSync(full).isDirectory()) walk(full);
      else files.push(full);
    }
  };
  walk(dist);
  const sum = (pick) => {
    let raw = 0;
    let br = 0;
    for (const f of files.filter(pick)) {
      const buf = readFileSync(f);
      raw += buf.length;
      br += brotli(buf);
    }
    return [raw, br];
  };
  const base = (f) => path.basename(f);
  const [engineWasmBytes, engineWasmBrotliBytes] = sum((f) => /^embedpdf-.*\.wasm$/.test(base(f)));
  const [kernelWasmBytes, kernelWasmBrotliBytes] = sum((f) => /^epdf_web_bg.*\.wasm$/.test(base(f)));
  const [jsBytes, jsBrotliBytes] = sum((f) => f.endsWith('.js'));
  return { engineWasmBytes, engineWasmBrotliBytes, kernelWasmBytes, kernelWasmBrotliBytes, jsBytes, jsBrotliBytes };
}
