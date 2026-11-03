import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { existsSync, mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import test from 'node:test';
import {
  acquireRunLock,
  checkedGroupAlive,
  checkedProcessRows,
  startOwnedStep,
  surveyProcesses,
} from './publish-process.mjs';
import { identityOf, recordedGroupAlive } from './publish-guard.mjs';

const temporary = (fn) => {
  const dir = mkdtempSync(path.join(tmpdir(), 'publish-process-'));
  try {
    return fn(dir);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
};
const mine = {
  pid: process.pid,
  pgid: process.pid,
  lstart: 'Tue Nov 3 21:00:00 2026',
  args: 'node test',
};
const live = {
  pid: 999999,
  pgid: 999999,
  lstart: 'Tue Nov 3 21:01:00 2026',
  args: 'node publisher',
};

test('a publisher that starts after the first survey keeps its lock', () =>
  temporary((dir) => {
    const lock = path.join(dir, 'lock');
    let surveys = 0;
    const owner = JSON.stringify({ pid: live.pid, start: live.lstart });
    assert.throws(
      () =>
        acquireRunLock(lock, {
          survey: () => {
            surveys++;
            if (surveys === 1) {
              writeFileSync(lock, owner);
              return [mine];
            }
            return [mine, live];
          },
        }),
      /another publish/,
    );
    assert.equal(readFileSync(lock, 'utf8'), owner);
    assert.equal(surveys, 2);
  }));

test('an initializing or unreadable owner is preserved', () =>
  temporary((dir) => {
    const lock = path.join(dir, 'lock');
    for (const content of ['', '{', JSON.stringify({ pid: live.pid })]) {
      writeFileSync(lock, content);
      assert.throws(() => acquireRunLock(lock, { survey: () => [mine] }), /unknown|unreadable/);
      assert.equal(readFileSync(lock, 'utf8'), content);
    }
  }));

test('an acquisition window excludes contenders before owner publication', () =>
  temporary((dir) => {
    const lock = path.join(dir, 'lock');
    mkdirSync(lock + '.gate');
    assert.throws(() => acquireRunLock(lock, { survey: () => [mine] }), /acquisition/);
    assert.equal(existsSync(lock), false);
    assert.equal(existsSync(lock + '.gate'), true);
  }));

test('competing stale reclaimers cannot both acquire ownership', () =>
  temporary((dir) => {
    const lock = path.join(dir, 'lock');
    writeFileSync(lock, JSON.stringify({ pid: 1000000000, start: 'Mon Nov 2 10:00:00 2026' }));
    let surveys = 0,
      competitorRefused = false;
    const release = acquireRunLock(lock, {
      survey: () => {
        if (++surveys === 2) {
          assert.throws(() => acquireRunLock(lock, { survey: () => [mine] }), /acquisition/);
          competitorRefused = true;
        }
        return [mine];
      },
    });
    try {
      assert(competitorRefused);
      const published = readFileSync(lock, 'utf8');
      assert.equal(JSON.parse(published).pid, process.pid);
      assert.throws(() => acquireRunLock(lock, { survey: () => [mine] }), /another publish/);
      assert.equal(readFileSync(lock, 'utf8'), published);
    } finally {
      release();
    }
  }));

test('a replacement observed during stale inspection is not unlinked', () =>
  temporary((dir) => {
    const lock = path.join(dir, 'lock');
    writeFileSync(lock, JSON.stringify({ pid: 1000000000, start: 'Mon Nov 2 10:00:00 2026' }));
    const replacement = JSON.stringify({ pid: live.pid, start: live.lstart });
    let surveys = 0;
    assert.throws(
      () =>
        acquireRunLock(lock, {
          survey: () => {
            if (++surveys === 2) writeFileSync(lock, replacement);
            return [mine];
          },
        }),
      /changed/,
    );
    assert.equal(readFileSync(lock, 'utf8'), replacement);
  }));

for (const [name, result] of [
  ['failed', { status: 1, stdout: '', stderr: 'failed' }],
  ['error', { status: null, error: new Error('unavailable'), stdout: '' }],
  ['signaled', { status: null, signal: 'SIGTERM', stdout: '' }],
  ['partial', { status: 0, stdout: `${mine.pid} ${mine.pgid} ${mine.lstart} node\n` }],
  [
    'malformed',
    { status: 0, stdout: `${mine.pid} ${mine.pgid} ${mine.lstart} node\n999 truncated\nEND\n` },
  ],
  ['missing self', { status: 0, stdout: `${live.pid} ${live.pgid} ${live.lstart} node\n\nEND\n` }],
])
  test(`${name} process survey refuses unknown ownership`, () => {
    assert.throws(
      () => checkedProcessRows(result, { pid: process.pid, marker: 'END' }),
      /survey|identity/,
    );
  });

test('a complete successful process survey includes its caller', () => {
  assert(identityOf(surveyProcesses(), process.pid));
});

test('a real shell exec retains its recorded group ownership', async () => {
  const child = spawn('bash', ['-euo', 'pipefail', '-c', 'read -r ready; exec sleep 20'], {
    detached: true,
    stdio: ['pipe', 'ignore', 'ignore'],
  });
  const exited = new Promise((resolve) => child.once('exit', resolve));
  try {
    let before;
    const readyDeadline = Date.now() + 2000;
    while (Date.now() < readyDeadline && !before) {
      before = identityOf(surveyProcesses(), child.pid);
      if (!before) await new Promise((resolve) => setTimeout(resolve, 10));
    }
    assert(before);
    assert.match(before.args, /^bash /);
    const record = { pgid: child.pid, start: before.start, args: 'bash -euo pipefail -c' };
    child.stdin.end('go\n');
    let rows, after;
    const execDeadline = Date.now() + 2000;
    do {
      rows = surveyProcesses();
      after = identityOf(rows, child.pid);
      if (after?.args === 'sleep 20') break;
      await new Promise((resolve) => setTimeout(resolve, 10));
    } while (Date.now() < execDeadline);
    assert.equal(after?.args, 'sleep 20');
    assert.equal(after.start, before.start);
    assert(recordedGroupAlive(rows, record));
  } finally {
    try {
      process.kill(-child.pid, 'SIGTERM');
    } catch {}
    await exited;
  }
});

for (const code of [0, 1])
  test(`exit ${code} drains a background writer before settling`, async () => {
    const dir = mkdtempSync(path.join(tmpdir(), 'publish-writer-'));
    const file = path.join(dir, 'payload');
    try {
      writeFileSync(file, 'new');
      let group;
      const step = startOwnedStep(
        'bash',
        ['-euo', 'pipefail', '-c', `(sleep 0.3; printf late > '${file}') & exit ${code}`],
        {
          onGroup: (value) => {
            group = value;
          },
        },
      );
      if (code) await assert.rejects(step.completion, /exit 1/);
      else {
        await step.completion;
        assert.equal(readFileSync(file, 'utf8'), 'late');
      }
      assert(group);
      assert.equal(recordedGroupAlive(surveyProcesses(), group), false);
      writeFileSync(file, 'settled');
      await new Promise((resolve) => setTimeout(resolve, 400));
      assert.equal(readFileSync(file, 'utf8'), 'settled');
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });

test('an unknown drain survey preserves ownership instead of reporting completion', async () => {
  let fail = false,
    group;
  const step = startOwnedStep('bash', ['-c', 'sleep 20'], {
    onGroup: (value) => {
      group = value;
    },
    survey: () => {
      if (fail) throw new Error('process survey unavailable');
      return surveyProcesses();
    },
  });
  step.completion.catch(() => {});
  try {
    await step.started;
    fail = true;
    await assert.rejects(step.stop('SIGTERM'), (error) => error.preservePayload === true);
  } finally {
    fail = false;
    await step.stop('SIGTERM');
    await assert.rejects(step.completion);
    assert.equal(recordedGroupAlive(surveyProcesses(), group), false);
  }
});

test('a live owner omitted from a partial survey cannot be reclaimed', () =>
  temporary((dir) => {
    const lock = path.join(dir, 'lock');
    writeFileSync(lock, JSON.stringify({ pid: process.pid, start: mine.lstart }));
    // The first survey establishes self, but the owner survey omits that same live PID.
    let count = 0;
    assert.throws(
      () => acquireRunLock(lock, { survey: () => (++count === 1 ? [mine] : []) }),
      /kernel still sees/,
    );
    assert.equal(JSON.parse(readFileSync(lock, 'utf8')).pid, process.pid);
  }));

test('a live group omitted from a partial survey is unknown rather than dead', async () => {
  const child = spawn('bash', ['-c', 'exec sleep 20'], { detached: true, stdio: 'ignore' });
  const exited = new Promise((resolve) => child.once('exit', resolve));
  try {
    await new Promise((resolve) => setTimeout(resolve, 100));
    const rows = surveyProcesses();
    const identity = identityOf(rows, child.pid);
    assert(identity);
    assert.throws(
      () =>
        checkedGroupAlive(
          rows.filter((row) => row.pgid !== child.pid),
          { pgid: child.pid, start: identity.start },
        ),
      /kernel still sees/,
    );
  } finally {
    try {
      process.kill(-child.pid, 'SIGTERM');
    } catch {}
    await exited;
  }
});

test(
  'an orphaned noninterruptible release finishes before its leader disappears',
  { timeout: 15000 },
  async () => {
    const dir = mkdtempSync(path.join(tmpdir(), 'publish-orphan-'));
    const file = path.join(dir, 'completed');
    const moduleUrl = new URL('./publish-process.mjs', import.meta.url).href;
    const source = `import {startOwnedStep} from ${JSON.stringify(moduleUrl)};
    const step = startOwnedStep('bash', ['-c', 'sleep 0.3; printf complete > "$1"', 'release', ${JSON.stringify(file)}], {
      orphanPolicy: 'continue', onGroup: (group) => process.send(group),
    });
    step.completion.catch(() => {});
    await step.started; process.send({started: true}); await step.completion;`;
    const owner = spawn(process.execPath, ['--input-type=module', '-e', source], {
      stdio: ['ignore', 'inherit', 'inherit', 'ipc'],
    });
    const exited = new Promise((resolve) => owner.once('exit', resolve));
    let group, timer;
    try {
      await new Promise((resolve, reject) => {
        timer = setTimeout(() => reject(new Error('release did not start')), 5000);
        owner.on('message', (message) => {
          if (message.pgid) group = message;
          if (message.started) resolve();
        });
        owner.once('error', reject);
        owner.once('exit', () => reject(new Error('publisher exited before starting')));
      });
      clearTimeout(timer);
      owner.kill('SIGKILL');
      await exited;
      const deadline = Date.now() + 8000;
      while (
        Date.now() < deadline &&
        (!existsSync(file) || recordedGroupAlive(surveyProcesses(), group))
      )
        await new Promise((resolve) => setTimeout(resolve, 50));
      assert.equal(readFileSync(file, 'utf8'), 'complete');
      assert.equal(recordedGroupAlive(surveyProcesses(), group), false);
    } finally {
      clearTimeout(timer);
      owner.kill('SIGKILL');
      if (group && recordedGroupAlive(surveyProcesses(), group)) {
        // This group was created by this fixture; its birth identity still matches.
        process.kill(-group.pgid, 'SIGKILL');
        const deadline = Date.now() + 2000;
        while (Date.now() < deadline && recordedGroupAlive(surveyProcesses(), group))
          await new Promise((resolve) => setTimeout(resolve, 50));
        assert.equal(
          recordedGroupAlive(surveyProcesses(), group),
          false,
          `preserve undrained fixture ${dir}`,
        );
      }
      rmSync(dir, { recursive: true, force: true });
    }
  },
);

test('birth surveys remain stable across publisher time zones', () => {
  const original = process.env.TZ;
  try {
    process.env.TZ = 'Pacific/Honolulu';
    const first = identityOf(surveyProcesses(), process.pid);
    process.env.TZ = 'Europe/Amsterdam';
    const second = identityOf(surveyProcesses(), process.pid);
    assert.equal(first.start, second.start);
  } finally {
    if (original === undefined) delete process.env.TZ;
    else process.env.TZ = original;
  }
});
