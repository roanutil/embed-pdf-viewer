import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import test from 'node:test';
import { forkContains } from './fork-ancestry.mjs';
import { ALL_BOUND_SINCE } from './payload-notes.mjs';

// A repo with base, then fork on top of it, and side, a sibling of fork.
function repo() {
  const dir = mkdtempSync(path.join(tmpdir(), 'fork-ancestry-'));
  const git = (...args) =>
    execFileSync('git', ['-C', dir, ...args], {
      encoding: 'utf8',
      env: {
        ...process.env,
        GIT_AUTHOR_NAME: 't',
        GIT_AUTHOR_EMAIL: 't@example.invalid',
        GIT_COMMITTER_NAME: 't',
        GIT_COMMITTER_EMAIL: 't@example.invalid',
      },
    }).trim();
  git('init', '--quiet', '--initial-branch=main');
  const commit = (name) => {
    writeFileSync(path.join(dir, name), name);
    git('add', name);
    git('commit', '--quiet', '-m', name);
    return git('rev-parse', 'HEAD');
  };
  const base = commit('base');
  const fork = commit('fork');
  git('checkout', '--quiet', '-b', 'side', base);
  const side = commit('side');
  return { dir, base, fork, side };
}

test('forkContains answers whether the fork commit has the other in its history', () => {
  const { dir, base, fork, side } = repo();
  try {
    assert.equal(forkContains(dir, fork, base), true);
    assert.equal(forkContains(dir, fork, fork), true);
    assert.equal(forkContains(dir, base, fork), false);
    assert.equal(forkContains(dir, fork, side), false);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test('forkContains says to fetch a commit runtime-src does not have', () => {
  const { dir, fork } = repo();
  try {
    assert.throws(
      () => forkContains(dir, fork, ALL_BOUND_SINCE),
      new RegExp(`fetch ${ALL_BOUND_SINCE} \\(and ${fork} if missing\\) into runtime-src`),
    );
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test('forkContains says to fetch when git itself cannot run', () => {
  const { dir, fork } = repo();
  const path0 = process.env.PATH;
  try {
    // No git on PATH: spawnSync fails before git runs, with no stderr.
    process.env.PATH = '';
    assert.throws(
      () => forkContains(dir, fork, ALL_BOUND_SINCE),
      new RegExp(`ENOENT.*fetch ${ALL_BOUND_SINCE} \\(and ${fork} if missing\\) into runtime-src`),
    );
  } finally {
    process.env.PATH = path0;
    rmSync(dir, { recursive: true, force: true });
  }
});
