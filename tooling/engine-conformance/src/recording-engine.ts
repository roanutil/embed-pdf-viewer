/**
 * A local engine that keeps the worker requests it sends, so a generator
 * whose cases are written in an old spelling can record the request it
 * actually sent beside the case. The Rust runner replays that `request`
 * field as the wire request.
 *
 * It is `createLocalEngine` with one difference: the transport between the
 * engine's queue and the in-thread `WorkerHost` copies each request before
 * passing it on. `capture(run)` returns the write requests (`effect` one of
 * `write`, `contentWrite` and `runtimeWrite`) sent while `run` was in
 * flight. Calls are awaited one at a time
 * in every generator, so nothing else is in flight then.
 *
 * Two fields of a request are the session's own counters rather than the
 * case's: `jobId` (a process-wide counter) and `opId` (a fresh UUID per write
 * unless the caller names one). Both are recorded as placeholders, the way
 * the read vectors record `docSessionId`. Bytes are recorded as number arrays.
 */
import {
  LazyTransport,
  LocalEngine,
  type Transport,
  type WorkerRequest,
} from '../../../packages/engine/main/src/index';
import { createInlineTransport } from '../../../packages/engine/main/src/transport/InlineTransport';

export interface RecordingEngine {
  readonly engine: LocalEngine;
  /**
   * Runs `run` and returns what it resolved to or threw, with the write
   * requests sent meanwhile: one request as itself, several as a list, none
   * as `null` (a call the engine refused before sending anything).
   */
  capture<T>(run: () => Promise<T>): Promise<Captured<T>>;
}

export type Captured<T> =
  | { readonly ok: true; readonly value: T; readonly request: unknown }
  | { readonly ok: false; readonly error: unknown; readonly request: unknown };

function plain(value: unknown): unknown {
  if (value instanceof ArrayBuffer) return Array.from(new Uint8Array(value));
  if (ArrayBuffer.isView(value)) {
    return Array.from(new Uint8Array(value.buffer, value.byteOffset, value.byteLength));
  }
  if (Array.isArray(value)) return value.map(plain);
  if (value === null || typeof value !== 'object') return value;
  return Object.fromEntries(
    Object.entries(value).map(([key, entry]) => {
      if (key === 'jobId') return [key, '<job>'];
      if (key === 'opId' && typeof entry === 'string') return [key, '<op>'];
      return [key, plain(entry)];
    }),
  );
}

const WRITES = new Set(['write', 'contentWrite', 'runtimeWrite']);

type RuntimeOptions = Parameters<typeof createInlineTransport>[0];

export function createRecordingEngine(runtime: RuntimeOptions = {}): RecordingEngine {
  let recording: unknown[] | null = null;
  const inner = new LazyTransport(() => createInlineTransport(runtime));
  const transport: Transport = {
    send(pack) {
      const request = pack.payload as WorkerRequest & { effect?: string };
      if (recording && WRITES.has(request.effect ?? '')) recording.push(plain(request));
      inner.send(pack);
    },
    start: () => inner.start(),
    onMessage: (handler) => inner.onMessage(handler),
    terminate: () => inner.terminate(),
  };
  const engine = LocalEngine.fromTransport({ transport });

  return {
    engine,
    async capture(run) {
      const requests: unknown[] = [];
      recording = requests;
      const request = () =>
        requests.length === 0 ? null : requests.length === 1 ? requests[0] : requests;
      try {
        const value = await run();
        return { ok: true, value, request: request() };
      } catch (error) {
        return { ok: false, error, request: request() };
      } finally {
        recording = null;
      }
    },
  };
}
