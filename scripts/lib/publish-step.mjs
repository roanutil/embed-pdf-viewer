// A detached step's group leader stays alive until its publisher has drained
// every writer. The command starts only after the publisher records that leader.
import { spawn } from 'node:child_process';
import { surveyProcesses } from './publish-process.mjs';

let started = false;
let released = false;
let orphaned = false;
let finished = false;
let draining = false;
let orphanPolicy = 'stop';
for (const signal of ['SIGHUP', 'SIGINT', 'SIGTERM']) process.on(signal, () => {});
const send = (message) => {
  if (process.connected) process.send(message);
};
const killOwnGroup = (signal) => {
  try {
    process.kill(-process.pid, signal);
  } catch {}
};
const drainOrphanedRelease = async () => {
  if (draining) return;
  draining = true;
  const deadline = Date.now() + 5000;
  for (;;) {
    try {
      const writers = surveyProcesses().filter(
        (row) => row.pgid === process.pid && row.pid !== process.pid,
      );
      if (writers.length === 0) process.exit(0);
    } catch {} // An unknown survey cannot establish that writers have stopped.
    if (Date.now() >= deadline) killOwnGroup('SIGKILL');
    await new Promise((resolve) => setTimeout(resolve, 50));
  }
};
const complete = (outcome) => {
  finished = true;
  send({ type: 'finished', ...outcome });
  if (orphaned && orphanPolicy === 'continue') void drainOrphanedRelease();
};
process.on('message', (message) => {
  if (message.type === 'release') {
    released = true;
    process.exit(0);
  }
  if (message.type !== 'start' || started) return;
  started = true;
  orphanPolicy = message.orphanPolicy;
  const child = spawn(message.file, message.args, {
    cwd: message.cwd,
    env: message.env,
    stdio: 'inherit',
  });
  send({ type: 'started' });
  child.once('error', (error) => complete({ error: error.message }));
  child.once('exit', (code, signal) => complete({ code, signal }));
});
process.on('disconnect', () => {
  if (released) return;
  if (!started) process.exit(0);
  orphaned = true;
  // A release remains noninterruptible if its publisher disappears. Its leader
  // keeps ownership visible until the upload and any remaining writers finish.
  if (orphanPolicy === 'continue') {
    if (finished) void drainOrphanedRelease();
    return;
  }
  // This persistent process owns the detached group it leads.
  killOwnGroup('SIGTERM');
  setTimeout(() => killOwnGroup('SIGKILL'), 5000);
});
send({ type: 'ready' });
