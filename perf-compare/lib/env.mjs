// meta.json: everything about a run that decides whether two runs compare.
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { existsSync, readFileSync } from 'node:fs';
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

// The wire kernel's wasm, when the worktree has one (the plugins recipe).
export function kernelWasm(worktreeDir) {
  const file = path.join(worktreeDir, 'packages/core-wasm/wasm/epdf_web_bg.wasm');
  return existsSync(file) ? file : null;
}

function powerSource() {
  if (process.platform !== 'darwin') return 'unknown';
  const batt = run('pmset', ['-g', 'batt']) ?? '';
  return /'([^']+)'/.exec(batt)?.[1] ?? 'unknown';
}

export function collectEnv({ config, stamps, worktreeDirs, fixtures, browserVersion, profile, flows }) {
  const arms = {};
  for (const id of config.armOrder) {
    const arm = config.arms[id];
    const stamp = stamps[arm.worktree];
    const kernel = arm.kernel === 'rust' ? kernelWasm(worktreeDirs[arm.worktree]) : null;
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
    harnessCommit: run('git', ['-C', here, 'rev-parse', 'HEAD']) ?? 'unknown',
    harnessDirty: Boolean(run('git', ['-C', path.join(here, '..'), 'status', '--porcelain', '--', '.'])),
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
