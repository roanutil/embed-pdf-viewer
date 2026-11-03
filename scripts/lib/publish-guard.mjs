// The parts of publish-engine-payload.mjs's run guard that don't touch the
// world themselves: reading `ps`, telling a recorded process group or run lock
// apart from an unrelated process that reuses its id, and a payload's digests.

import { createHash } from 'node:crypto';
import { readdirSync, readFileSync, statSync } from 'node:fs';
import path from 'node:path';

/**
 * `ps -A -ww -o pid=,pgid=,lstart=,args=` as rows. `lstart` is the five fields
 * ps prints for a start time (`Tue Nov  3 19:11:02 2026`), with its runs of
 * spaces collapsed, so two reads of one process compare equal.
 */
export function parseProcessRows(text) {
  const rows = [];
  for (const line of text.split('\n')) {
    const fields = line.trim().split(/\s+/);
    if (fields.length < 7) continue;
    const pid = Number(fields[0]);
    const pgid = Number(fields[1]);
    if (!Number.isInteger(pid) || !Number.isInteger(pgid)) continue;
    rows.push({
      pid,
      pgid,
      lstart: fields.slice(2, 7).join(' '),
      args: fields.slice(7).join(' '),
    });
  }
  return rows;
}

/** The identity of `pid` among `rows`: its start time and command, or null. */
export function identityOf(rows, pid) {
  const row = rows.find((candidate) => candidate.pid === pid);
  return row ? { start: row.lstart, args: row.args } : null;
}

/**
 * Whether the process group recorded as `{ pgid, start, args }` still has a
 * process of its own among `rows`. A leader with that pid counts only with the
 * recorded start time and a command that starts with the recorded one; any
 * other is an unrelated group that reuses the id. With the leader gone, members
 * left in the group are ours: a group id isn't handed out again while members
 * of it live.
 */
export function recordedGroupAlive(rows, record) {
  if (!record || !Number.isInteger(record.pgid) || record.pgid <= 0) return false;
  const members = rows.filter((row) => row.pgid === record.pgid);
  if (members.length === 0) return false;
  const leader = members.find((row) => row.pid === record.pgid);
  if (!leader) return true;
  return leader.lstart === record.start && leader.args.startsWith(record.args);
}

/**
 * Whether a run lock `{ pid, start }` belongs to a run still going: its pid is
 * running with the recorded start time. Anything else is a lock a dead run left.
 */
export function lockHeld(rows, lock) {
  if (!lock || !Number.isInteger(lock.pid)) return false;
  const now = identityOf(rows, lock.pid);
  return Boolean(now && now.start === lock.start);
}

/**
 * The sha256 of every file under `dir`, by its path relative to `dir`, without
 * the top-level `.payload-source` marker. Subdirectories are walked, so a
 * payload that grows one is still digested whole.
 */
export function fileDigests(dir) {
  const out = {};
  const walk = (relative) => {
    for (const name of readdirSync(path.join(dir, relative)).sort()) {
      const entry = relative ? `${relative}/${name}` : name;
      if (entry === '.payload-source') continue;
      const full = path.join(dir, entry);
      if (statSync(full).isDirectory()) walk(entry);
      else out[entry] = createHash('sha256').update(readFileSync(full)).digest('hex');
    }
  };
  walk('');
  return out;
}
