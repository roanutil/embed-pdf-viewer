// Publish ownership: complete process surveys, serialized atomic run locks,
// and detached steps whose writers drain before a payload can move or be marked.
import { spawn, spawnSync } from 'node:child_process';
import { randomUUID } from 'node:crypto';
import {
  linkSync,
  lstatSync,
  mkdirSync,
  readFileSync,
  rmdirSync,
  unlinkSync,
  writeFileSync,
} from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import {
  identityOf,
  lockHeld,
  parseProcessRows,
  recordedGroupAlive,
  isProcessStart,
} from './publish-guard.mjs';

/** Refuses failed, truncated or unprovable surveys instead of declaring owners dead. */
export function checkedProcessRows(result, { pid, marker }) {
  const ending = `\n${marker}\n`;
  if (
    result.error ||
    result.signal ||
    result.status !== 0 ||
    typeof result.stdout !== 'string' ||
    !result.stdout.endsWith(ending)
  ) {
    throw new Error(
      `process survey failed or is incomplete: ${result.error?.message ?? result.stderr ?? result.signal ?? result.status}`,
    );
  }
  const rows = parseProcessRows(result.stdout.slice(0, -ending.length));
  if (!identityOf(rows, pid)) throw new Error('process survey cannot prove the caller identity');
  return rows;
}

/** A successful complete survey includes this publisher and an end-of-stream marker. */
export function surveyProcesses() {
  const marker = `PUBLISH_SURVEY_${randomUUID()}`;
  const result = spawnSync(
    'bash',
    [
      '-euo',
      'pipefail',
      '-c',
      'ps -A -ww -o pid=,pgid=,lstart=,args=; printf "\\n%s\\n" "$1"',
      'publish-survey',
      marker,
    ],
    {
      encoding: 'utf8',
      // Inspection is read-only and cannot keep the inspected writer group alive.
      detached: true,
      maxBuffer: 16 * 1024 * 1024,
      env: { ...process.env, LC_ALL: 'C', TZ: 'UTC' },
    },
  );
  return checkedProcessRows(result, { pid: process.pid, marker });
}

// A missing survey row is not proof of death when the kernel still sees it.
function isProcessGone(pid) {
  try {
    process.kill(pid, 0);
    return false;
  } catch (error) {
    if (error.code === 'ESRCH') return true;
    throw new Error(`process ownership cannot be inspected: ${error.message}`);
  }
}

export function checkedGroupAlive(rows, record) {
  if (record === null) return false;
  if (recordedGroupAlive(rows, record)) return true;
  const leader = rows.find((row) => row.pid === record.pgid);
  if (leader && leader.lstart !== record.start) {
    if (record.clock === 'UTC') return false;
    throw new Error('legacy group birth identity is unknown across time zones');
  }
  if (!isProcessGone(-record.pgid))
    throw new Error('process survey omitted a group the kernel still sees');
  return false;
}

const readOwner = (file) => {
  try {
    const stat = lstatSync(file);
    if (!stat.isFile()) throw new Error('not a regular ownership file');
    const text = readFileSync(file, 'utf8');
    const owner = JSON.parse(text);
    if (!Number.isInteger(owner.pid) || owner.pid <= 0 || !isProcessStart(owner.start))
      throw new Error('incomplete identity');
    return { stat, text, owner };
  } catch (error) {
    throw Object.assign(
      new Error(`publish lock ownership is unknown or unreadable: ${error.message}`),
      { code: error.code },
    );
  }
};

// All conforming claimants and releases use this gate. It is never reclaimed
// automatically: an initializing or abandoned gate requires verified manual care.
function withAcquisitionGate(lock, text, action) {
  const candidate = `${lock}.owner-${randomUUID()}`;
  const gate = `${lock}.gate`;
  let created = false;
  let published = false;
  writeFileSync(candidate, text, { flag: 'wx', mode: 0o600 });
  try {
    try {
      mkdirSync(gate);
      created = true;
    } catch (error) {
      if (error.code === 'EEXIST')
        throw new Error(
          `publish lock acquisition is in progress or interrupted: inspect ${gate}/owner; leave it until its owner is verified stopped`,
        );
      throw error;
    }
    linkSync(candidate, path.join(gate, 'owner'));
    published = true;
    return action(candidate);
  } finally {
    if (published) {
      // The exclusive gate serializes every conforming mutation. Its immutable
      // random owner token is stable even when a shared filesystem remaps inodes.
      const owner = path.join(gate, 'owner');
      if (readFileSync(owner, 'utf8') !== text)
        throw new Error('publish acquisition owner changed; left as it is');
      unlinkSync(owner);
      rmdirSync(gate);
    }
    // An initialization that could not publish ownership stays for inspection.
    if (!created || published) unlinkSync(candidate);
  }
}

/** Publishes a complete owner atomically; stale recovery is serialized and reverified. */
export function acquireRunLock(lock, { survey = surveyProcesses } = {}) {
  const pid = process.pid;
  const mine = identityOf(survey(), pid);
  if (!mine) throw new Error('process survey cannot prove the publisher identity');
  const text = JSON.stringify({ pid, start: mine.start, clock: 'UTC', token: randomUUID() });
  withAcquisitionGate(lock, text, (candidate) => {
    try {
      linkSync(candidate, lock);
    } catch (error) {
      if (error.code !== 'EEXIST') throw error;
      const prior = readOwner(lock);
      const rows = survey();
      if (lockHeld(rows, prior.owner))
        throw new Error(`another publish is running (pid ${prior.owner.pid}); wait for it`);
      if (identityOf(rows, prior.owner.pid) && prior.owner.clock !== 'UTC')
        throw new Error('legacy lock birth identity is unknown across time zones');
      if (!identityOf(rows, prior.owner.pid) && !isProcessGone(prior.owner.pid))
        throw new Error('process survey omitted a lock owner the kernel still sees');
      const current = readOwner(lock);
      if (prior.text !== current.text)
        throw new Error('publish lock changed during stale inspection; left as it is');
      unlinkSync(lock);
      linkSync(candidate, lock);
    }
  });
  return () =>
    withAcquisitionGate(lock, text, () => {
      let current;
      try {
        current = readOwner(lock);
      } catch (error) {
        if (error.code === 'ENOENT') return;
        throw error;
      }
      if (current.text === text) unlinkSync(lock);
    });
}

const pause = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const createDrainError = (error) =>
  Object.assign(new Error(`cannot prove the publish step drained: ${error.message}`), {
    preservePayload: true,
  });

/** A persistent group leader starts the command only after ownership is recorded. */
export function startOwnedStep(
  file,
  args,
  {
    cwd,
    env = process.env,
    onGroup = () => {},
    survey = surveyProcesses,
    orphanPolicy = 'stop',
  } = {},
) {
  const helper = spawn(
    process.execPath,
    [fileURLToPath(new URL('./publish-step.mjs', import.meta.url))],
    {
      cwd,
      env,
      detached: true,
      stdio: ['inherit', 'inherit', 'inherit', 'ipc'],
    },
  );
  let readyResolve, readyReject, startedResolve, resultResolve, resultReject, exitResolve;
  const ready = new Promise((resolve, reject) => {
    readyResolve = resolve;
    readyReject = reject;
  });
  const acknowledged = new Promise((resolve) => {
    startedResolve = resolve;
  });
  const result = new Promise((resolve, reject) => {
    resultResolve = resolve;
    resultReject = reject;
  });
  result.catch(() => {});
  const exited = new Promise((resolve) => {
    exitResolve = resolve;
  });
  const knownMembers = new Map();
  let leaderExited = false;
  let group,
    stopSignal,
    commandSent = false,
    released = false,
    drainPromise;
  helper.on('message', (message) => {
    if (message.type === 'ready') readyResolve();
    if (message.type === 'started') startedResolve();
    if (message.type === 'finished') resultResolve(message);
  });
  helper.once('error', (error) => {
    readyReject(error);
    resultReject(error);
    startedResolve();
    exitResolve();
  });
  helper.once('exit', (code, signal) => {
    leaderExited = true;
    exitResolve();
    startedResolve();
    readyReject(new Error(`step leader exited (${signal ?? code})`));
    resultReject(new Error(`step leader exited (${signal ?? code})`));
  });
  const initialized = (async () => {
    await ready;
    const rows = survey();
    const identity = identityOf(rows, helper.pid);
    if (!identity || !rows.some((row) => row.pid === helper.pid && row.pgid === helper.pid))
      throw new Error('process survey cannot establish step ownership');
    group = { pgid: helper.pid, start: identity.start, clock: 'UTC', args: identity.args };
    knownMembers.set(helper.pid, identity.start);
    onGroup(group);
    if (!stopSignal) {
      commandSent = true;
      helper.send({ type: 'start', file, args, cwd, env, orphanPolicy });
      await acknowledged;
    }
  })();
  const releaseIdle = async () => {
    if (!released) {
      released = true;
      if (helper.connected) helper.send({ type: 'release' });
      else helper.kill('SIGKILL');
    }
    await exited;
  };
  const drain = (signal = 'SIGTERM') => {
    if (drainPromise) return drainPromise;
    drainPromise = (async () => {
      if (!group) {
        await releaseIdle();
        return false;
      }
      let signaled = false,
        killed = false,
        terminated = false;
      const deadline = Date.now() + 5000;
      for (;;) {
        const rows = survey();
        const alive = checkedGroupAlive(rows, group);
        const helperRow = rows.find((row) => row.pid === helper.pid);
        if (helperRow && helperRow.lstart !== group.start)
          throw new Error('step leader identity changed');
        if (!alive) {
          if (!leaderExited) await Promise.race([exited, pause(100)]);
          if (!leaderExited) throw new Error('survey lost a step leader whose exit is unconfirmed');
          return terminated;
        }
        const members = rows.filter((row) => row.pgid === group.pgid);
        const leaderProven = !leaderExited && helperRow?.lstart === group.start;
        const memberProven = members.some(
          (row) => row.pid !== helper.pid && knownMembers.get(row.pid) === row.lstart,
        );
        if (!leaderProven && !memberProven)
          throw new Error('remaining group ownership cannot be proved');
        for (const member of members) knownMembers.set(member.pid, member.lstart);
        const writers = members.filter((row) => row.pid !== helper.pid);
        if (writers.length === 0) {
          await releaseIdle();
          if (recordedGroupAlive(survey(), group))
            throw new Error('step group remains after releasing its leader');
          return terminated;
        }
        const requested = stopSignal ?? signal;
        if ((!signaled && requested) || (!killed && Date.now() >= deadline)) {
          const next = Date.now() >= deadline ? 'SIGKILL' : requested;
          try {
            process.kill(-group.pgid, next);
          } catch (error) {
            if (error.code !== 'ESRCH') throw error;
          }
          if (next === 'SIGKILL') killed = true;
          signaled = true;
          terminated = true;
        }
        if (killed && Date.now() >= deadline + 2000)
          throw new Error(`pgid ${group.pgid} survives SIGKILL`);
        await pause(50);
      }
    })().catch((error) => {
      drainPromise = null;
      throw createDrainError(error);
    });
    return drainPromise;
  };
  const completion = (async () => {
    try {
      await initialized;
      const outcome = commandSent ? await result : { signal: stopSignal };
      const terminated = await drain(outcome.code === 0 ? null : 'SIGTERM');
      if (outcome.code === 0 && terminated)
        throw new Error(`Command left writers after exit: ${file}`);
      if (outcome.error || outcome.code !== 0)
        throw new Error(
          `Command failed (${outcome.error ?? outcome.signal ?? `exit ${outcome.code}`}): ${file} ${args.join(' ')}`,
        );
    } catch (error) {
      try {
        await drain();
      } catch (drainError) {
        throw drainError;
      }
      throw error;
    }
  })();
  return {
    completion,
    started: initialized,
    stop: async (signal = 'SIGTERM') => {
      stopSignal ??= signal;
      try {
        await initialized;
      } catch {}
      await drain(signal);
    },
  };
}
